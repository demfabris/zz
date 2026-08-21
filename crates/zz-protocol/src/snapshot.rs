use std::{
    cell::Cell,
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

use crate::{PaneId, SessionId, SplitId, TmuxColour, WindowId, message::deserialize_bounded_text};

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    Horizontal,
    #[default]
    Vertical,
}

const MAX_LAYOUT_DEPTH: usize = 256;
const MAX_LAYOUT_NODES: usize = 65_535;
pub const MAX_WINDOW_STATUS_LABEL_BYTES: usize = 1024;

#[derive(Clone, Copy, Default)]
struct LayoutDecodeState {
    depth: usize,
    nodes: usize,
}

thread_local! {
    static LAYOUT_DECODE_STATE: Cell<LayoutDecodeState> =
        const { Cell::new(LayoutDecodeState { depth: 0, nodes: 0 }) };
}

struct LayoutDecodeGuard {
    root: bool,
}

impl LayoutDecodeGuard {
    fn enter() -> Result<Self, &'static str> {
        LAYOUT_DECODE_STATE.with(|state| {
            let mut current = state.get();
            let root = current.depth == 0;
            if root {
                current.nodes = 0;
            }
            if current.depth >= MAX_LAYOUT_DEPTH {
                return Err("layout nesting exceeds the protocol limit");
            }
            if current.nodes >= MAX_LAYOUT_NODES {
                return Err("layout node count exceeds the protocol limit");
            }
            current.depth += 1;
            current.nodes += 1;
            state.set(current);
            Ok(Self { root })
        })
    }
}

impl Drop for LayoutDecodeGuard {
    fn drop(&mut self) {
        LAYOUT_DECODE_STATE.with(|state| {
            let mut current = state.get();
            current.depth = current.depth.saturating_sub(1);
            if self.root {
                current = LayoutDecodeState::default();
            }
            state.set(current);
        });
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum LayoutNode {
    Pane(PaneId),
    Split {
        id: SplitId,
        axis: Axis,
        ratio: f32,
        first: Box<Self>,
        second: Box<Self>,
    },
}

#[derive(Deserialize)]
enum LayoutNodeWire {
    Pane(PaneId),
    Split {
        id: SplitId,
        axis: Axis,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl<'de> Deserialize<'de> for LayoutNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _guard = LayoutDecodeGuard::enter().map_err(D::Error::custom)?;
        Ok(match LayoutNodeWire::deserialize(deserializer)? {
            LayoutNodeWire::Pane(pane) => Self::Pane(pane),
            LayoutNodeWire::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => Self::Split {
                id,
                axis,
                ratio,
                first,
                second,
            },
        })
    }
}

impl LayoutNode {
    #[must_use]
    pub fn contains(&self, pane: PaneId) -> bool {
        match self {
            Self::Pane(id) => *id == pane,
            Self::Split { first, second, .. } => first.contains(pane) || second.contains(pane),
        }
    }

    pub fn panes(&self, output: &mut Vec<PaneId>) {
        match self {
            Self::Pane(id) => output.push(*id),
            Self::Split { first, second, .. } => {
                first.panes(output);
                second.panes(output);
            }
        }
    }

    #[must_use]
    pub fn contains_split(&self, split: SplitId) -> bool {
        match self {
            Self::Pane(_) => false,
            Self::Split {
                id, first, second, ..
            } => *id == split || first.contains_split(split) || second.contains_split(split),
        }
    }

    pub fn splits(&self, output: &mut Vec<SplitId>) {
        if let Self::Split {
            id, first, second, ..
        } = self
        {
            output.push(*id);
            first.splits(output);
            second.splits(output);
        }
    }
}

pub const DEFAULT_BROWSER_PROFILE: &str = "default";
pub const MAX_BROWSER_PROFILE_NAME_BYTES: usize = 64;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum BrowserProfileNameError {
    #[error("browser profile name cannot be empty")]
    Empty,
    #[error("browser profile name cannot exceed {MAX_BROWSER_PROFILE_NAME_BYTES} bytes")]
    TooLong,
    #[error("browser profile name cannot contain control characters")]
    ControlCharacter,
}

/// Normalizes a browser profile name. `zz-default` is the legacy alias for `default`.
pub fn normalize_browser_profile_name(value: &str) -> Result<String, BrowserProfileNameError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(BrowserProfileNameError::Empty);
    }
    if value.len() > MAX_BROWSER_PROFILE_NAME_BYTES {
        return Err(BrowserProfileNameError::TooLong);
    }
    if value.chars().any(char::is_control) {
        return Err(BrowserProfileNameError::ControlCharacter);
    }
    if value == "zz-default" {
        return Ok(DEFAULT_BROWSER_PROFILE.to_owned());
    }
    Ok(value.to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDescriptor {
    /// Every tab's URL in strip order. Never empty.
    pub tabs: Vec<String>,
    /// Index into `tabs` of the tab the pane shows.
    pub active_tab: usize,
    pub profile: String,
}

impl BrowserDescriptor {
    #[must_use]
    pub fn single(url: String, profile: String) -> Self {
        Self {
            tabs: vec![url],
            active_tab: 0,
            profile,
        }
    }

    /// The active tab's URL, or `about:blank` if the index is out of range.
    #[must_use]
    pub fn url(&self) -> &str {
        self.tabs
            .get(self.active_tab)
            .or_else(|| self.tabs.first())
            .map_or("about:blank", String::as_str)
    }
}

/// The built-in ACP agent backing an Agent pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AgentProvider {
    #[default]
    Codex,
    ClaudeCode,
}

impl AgentProvider {
    pub const ALL: [Self; 2] = [Self::Codex, Self::ClaudeCode];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
        }
    }
}

impl fmt::Display for AgentProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for AgentProvider {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" | "claude-code" | "claudecode" => Ok(Self::ClaudeCode),
            _ => Err(format!(
                "unknown agent provider `{value}`; expected codex or claude-code"
            )),
        }
    }
}

/// Metadata needed to start or restore an ACP session for an Agent pane.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDescriptor {
    pub provider: AgentProvider,
    pub cwd: Option<PathBuf>,
    pub session_id: Option<String>,
}

pub const MAX_EDITOR_PATH_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum EditorDescriptorError {
    #[error("editor {field} must be absolute")]
    Relative { field: &'static str },
    #[error("editor {field} cannot exceed {MAX_EDITOR_PATH_BYTES} bytes")]
    TooLong { field: &'static str },
    #[error("editor {field} cannot contain control characters")]
    ControlCharacter { field: &'static str },
}

/// Daemon-owned restore metadata for an Editor pane. Buffer bytes stay GUI-local.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EditorDescriptor {
    pub path: Option<String>,
    pub cwd: String,
}

#[derive(Deserialize)]
struct EditorDescriptorWire {
    path: Option<String>,
    cwd: String,
}

impl<'de> Deserialize<'de> for EditorDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = EditorDescriptorWire::deserialize(deserializer)?;
        let descriptor = Self {
            path: wire.path,
            cwd: wire.cwd,
        };
        descriptor.validate().map_err(D::Error::custom)?;
        Ok(descriptor)
    }
}

impl EditorDescriptor {
    /// Rejects relative, oversized, or control-character paths.
    pub fn validate(&self) -> Result<(), EditorDescriptorError> {
        validate_editor_absolute_path("working directory", &self.cwd)?;
        if let Some(path) = &self.path {
            validate_editor_absolute_path("path", path)?;
        }
        Ok(())
    }
}

fn validate_editor_absolute_path(
    field: &'static str,
    value: &str,
) -> Result<(), EditorDescriptorError> {
    if value.len() > MAX_EDITOR_PATH_BYTES {
        return Err(EditorDescriptorError::TooLong { field });
    }
    if value.chars().any(char::is_control) {
        return Err(EditorDescriptorError::ControlCharacter { field });
    }
    if !Path::new(value).is_absolute() {
        return Err(EditorDescriptorError::Relative { field });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneKindSnapshot {
    /// A newly split pane with no runtime surface yet.
    Picker,
    Terminal,
    Browser(BrowserDescriptor),
    Agent(AgentDescriptor),
    Editor(EditorDescriptor),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub id: PaneId,
    pub title: String,
    pub kind: PaneKindSnapshot,
    pub synchronized_input: bool,
    /// A BEL rang here and nobody has visited since. Latched until the pane is read.
    pub bell: bool,
    pub dead: bool,
    pub dead_status: Option<u32>,
    /// `pane-border-style` colour override; `None` means theme fallback.
    pub border_colour: Option<TmuxColour>,
    /// `pane-active-border-style` colour override; `None` means theme fallback.
    pub active_border_colour: Option<TmuxColour>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub id: WindowId,
    pub index: u32,
    pub name: String,
    pub automatic_rename: bool,
    pub active_pane: PaneId,
    pub zoomed_pane: Option<PaneId>,
    pub layout: LayoutNode,
    pub panes: BTreeMap<PaneId, PaneSnapshot>,
    pub layout_dump: String,
    pub visible_layout_dump: String,
    #[serde(deserialize_with = "deserialize_window_status_label")]
    pub status_label: String,
}

fn deserialize_window_status_label<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_WINDOW_STATUS_LABEL_BYTES)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionViewer {
    pub name: String,
    pub window: WindowId,
    pub is_self: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub name: String,
    pub active_window: WindowId,
    pub windows: Vec<WindowSnapshot>,
    #[serde(default)]
    pub viewers: Vec<SessionViewer>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MuxSnapshot {
    pub generation: u64,
    pub sessions: Vec<SessionSnapshot>,
    #[serde(default)]
    pub focused_window: Option<WindowId>,
}

impl MuxSnapshot {
    /// The window the recipient should render. Falls back to the session default.
    #[must_use]
    pub fn focused_window_for(&self, session: &SessionSnapshot) -> WindowId {
        self.focused_window
            .filter(|focused| session.windows.iter().any(|window| window.id == *focused))
            .unwrap_or(session.active_window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute_test_root() -> PathBuf {
        std::env::current_dir()
            .expect("current test directory")
            .join("target")
            .join("zz-protocol-tests")
    }

    fn absolute_test_path_with_len(len: usize) -> String {
        let mut path = absolute_test_root().to_string_lossy().into_owned();
        if !path.ends_with(std::path::MAIN_SEPARATOR) {
            path.push(std::path::MAIN_SEPARATOR);
        }
        assert!(path.len() <= len, "test directory exceeds fixture length");
        path.push_str(&"x".repeat(len - path.len()));
        path
    }

    #[test]
    fn focused_window_resolves_stamped_valid_and_default_fallbacks() {
        let pane = PaneId(1);
        let window = |id| WindowSnapshot {
            id,
            index: u32::try_from(id.0).expect("fixture index"),
            name: id.to_string(),
            automatic_rename: true,
            active_pane: pane,
            zoomed_pane: None,
            layout: LayoutNode::Pane(pane),
            panes: BTreeMap::new(),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
            status_label: String::new(),
        };
        let first = WindowId(1);
        let second = WindowId(2);
        let session = SessionSnapshot {
            id: SessionId(1),
            name: "shared".to_owned(),
            active_window: first,
            windows: vec![window(first), window(second)],
            viewers: Vec::new(),
        };
        let stamped = MuxSnapshot {
            generation: 1,
            sessions: vec![session.clone()],
            focused_window: Some(second),
        };
        assert_eq!(stamped.focused_window_for(&session), second);

        let stale = MuxSnapshot {
            focused_window: Some(WindowId(99)),
            ..stamped.clone()
        };
        assert_eq!(stale.focused_window_for(&session), first);

        let detached = MuxSnapshot {
            focused_window: None,
            ..stamped
        };
        assert_eq!(detached.focused_window_for(&session), first);
    }

    #[test]
    fn window_status_label_deserialization_is_bounded() {
        let pane = PaneId(1);
        let window = |status_label: String| WindowSnapshot {
            id: WindowId(1),
            index: 0,
            name: "main".to_owned(),
            automatic_rename: true,
            active_pane: pane,
            zoomed_pane: None,
            layout: LayoutNode::Pane(pane),
            panes: BTreeMap::new(),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
            status_label,
        };
        let boundary = window("x".repeat(MAX_WINDOW_STATUS_LABEL_BYTES));
        let encoded = postcard::to_stdvec(&boundary).expect("encode boundary label");
        assert_eq!(
            postcard::from_bytes::<WindowSnapshot>(&encoded).expect("decode boundary label"),
            boundary
        );

        let oversized = window("x".repeat(MAX_WINDOW_STATUS_LABEL_BYTES + 1));
        let encoded = postcard::to_stdvec(&oversized).expect("encode oversized label");
        assert!(postcard::from_bytes::<WindowSnapshot>(&encoded).is_err());
    }

    #[test]
    fn normalizes_browser_profile_names_and_legacy_default_alias() {
        assert_eq!(
            normalize_browser_profile_name("  Work  ").expect("valid profile"),
            "Work"
        );
        assert_eq!(
            normalize_browser_profile_name("zz-default").expect("legacy default alias"),
            DEFAULT_BROWSER_PROFILE
        );
    }

    #[test]
    fn rejects_invalid_browser_profile_names() {
        assert_eq!(
            normalize_browser_profile_name(" \t "),
            Err(BrowserProfileNameError::Empty)
        );
        assert_eq!(
            normalize_browser_profile_name("work\nprofile"),
            Err(BrowserProfileNameError::ControlCharacter)
        );
        assert_eq!(
            normalize_browser_profile_name(&"x".repeat(MAX_BROWSER_PROFILE_NAME_BYTES + 1)),
            Err(BrowserProfileNameError::TooLong)
        );
    }

    #[test]
    fn editor_kind_round_trips_on_the_control_encoding() {
        let root = absolute_test_root();
        let descriptor = EditorDescriptor {
            path: Some(
                root.join("src")
                    .join("main.rs")
                    .to_string_lossy()
                    .into_owned(),
            ),
            cwd: root.to_string_lossy().into_owned(),
        };
        descriptor.validate().expect("valid editor descriptor");
        let encoded = postcard::to_stdvec(&PaneKindSnapshot::Editor(descriptor.clone()))
            .expect("encode editor");
        assert_eq!(
            postcard::from_bytes::<PaneKindSnapshot>(&encoded).expect("decode editor"),
            PaneKindSnapshot::Editor(descriptor)
        );
    }

    #[test]
    fn editor_descriptor_rejects_relative_oversized_and_control_paths() {
        let root = absolute_test_root().to_string_lossy().into_owned();
        let descriptor = |cwd: &str, path: Option<&str>| EditorDescriptor {
            cwd: cwd.to_owned(),
            path: path.map(str::to_owned),
        };

        assert!(
            descriptor(&absolute_test_path_with_len(MAX_EDITOR_PATH_BYTES), None,)
                .validate()
                .is_ok()
        );
        assert!(descriptor("workspace", None).validate().is_err());
        assert!(descriptor(&root, Some("src/main.rs")).validate().is_err());
        let control_path = absolute_test_root()
            .join("bad\nname")
            .to_string_lossy()
            .into_owned();
        assert!(descriptor(&root, Some(&control_path)).validate().is_err());
        assert!(
            descriptor(
                &absolute_test_path_with_len(MAX_EDITOR_PATH_BYTES + 1),
                None
            )
            .validate()
            .is_err()
        );
        let oversized_path = absolute_test_path_with_len(MAX_EDITOR_PATH_BYTES + 1);
        assert!(descriptor(&root, Some(&oversized_path)).validate().is_err());

        let invalid = descriptor(&root, Some("relative.rs"));
        let encoded = postcard::to_stdvec(&invalid).expect("encode invalid descriptor");
        assert!(postcard::from_bytes::<EditorDescriptor>(&encoded).is_err());
    }

    fn skewed_layout(split_count: usize) -> LayoutNode {
        let mut layout = LayoutNode::Pane(PaneId(0));
        for index in 0..split_count {
            layout = LayoutNode::Split {
                id: SplitId(u64::try_from(index).expect("small fixture")),
                axis: Axis::Vertical,
                ratio: 0.5,
                first: Box::new(layout),
                second: Box::new(LayoutNode::Pane(PaneId(
                    u64::try_from(index + 1).expect("small fixture"),
                ))),
            };
        }
        layout
    }

    fn balanced_layout(leaves: usize, next_id: &mut u64) -> LayoutNode {
        if leaves == 1 {
            let pane = PaneId(*next_id);
            *next_id = next_id.saturating_add(1);
            return LayoutNode::Pane(pane);
        }
        let first_leaves = leaves / 2;
        let second_leaves = leaves - first_leaves;
        let first = balanced_layout(first_leaves, next_id);
        let second = balanced_layout(second_leaves, next_id);
        let id = SplitId(*next_id);
        *next_id = next_id.saturating_add(1);
        LayoutNode::Split {
            id,
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    #[test]
    fn layout_deserialization_accepts_the_depth_boundary() {
        let layout = skewed_layout(MAX_LAYOUT_DEPTH - 1);
        let encoded = postcard::to_stdvec(&layout).expect("encode boundary layout");
        let decoded = postcard::from_bytes::<LayoutNode>(&encoded).expect("decode boundary layout");
        assert_eq!(decoded, layout);
    }

    #[test]
    fn layout_deserialization_rejects_excessive_depth() {
        let layout = skewed_layout(MAX_LAYOUT_DEPTH);
        let encoded = postcard::to_stdvec(&layout).expect("encode over-depth layout");
        assert!(postcard::from_bytes::<LayoutNode>(&encoded).is_err());
    }

    #[test]
    fn layout_deserialization_rejects_excessive_node_count() {
        let mut next_id = 0;
        let at_limit = balanced_layout(MAX_LAYOUT_NODES.div_ceil(2), &mut next_id);
        let layout = LayoutNode::Split {
            id: SplitId(next_id),
            axis: Axis::Vertical,
            ratio: 0.5,
            first: Box::new(at_limit),
            second: Box::new(LayoutNode::Pane(PaneId(next_id.saturating_add(1)))),
        };
        let encoded = postcard::to_stdvec(&layout).expect("encode over-budget layout");
        assert!(postcard::from_bytes::<LayoutNode>(&encoded).is_err());
    }
}
