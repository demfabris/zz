use std::{collections::BTreeMap, fmt, path::PathBuf};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Error as _, SeqAccess, Visitor},
};
use zz_terminal::{
    AppearanceProvenance, ClipboardTarget, DEFAULT_HISTORY_LIMIT, DEFAULT_WORD_SEPARATORS,
    KeyInput, PackedCell, SearchDirection, TerminalAppearance, TerminalColorScheme,
    TerminalDictionary, TerminalViewAction, TerminalViewport, TerminalViewportPatch,
};

use crate::{ClientId, ClientInstanceId, MuxSnapshot, PaneId, SessionId, SplitId, WindowId};

/// Client and daemon must match this exactly. The handshake rejects any
/// mismatch instead of negotiating down.
pub const PROTOCOL_VERSION: u16 = 86;
pub const NEW_SESSION_ATTACH_CAPABILITY: &str = "new-session-attach-v1";
pub const CLIENT_TERMINAL_CAPABILITY: &str = "client-terminal-v1";
pub const CLIENT_NESTED_CAPABILITY: &str = "client-nested-v1";
/// Value-token prefix naming the caller's controlling tty, `client-tty-v1:/dev/ttys007`.
pub const CLIENT_TTY_CAPABILITY_PREFIX: &str = "client-tty-v1:";
/// Value-token prefix naming the caller's terminal size, `client-size-v1:80x24`.
pub const CLIENT_SIZE_CAPABILITY_PREFIX: &str = "client-size-v1:";
pub const SPLIT_RATIO_BASIS: u16 = 10_000;
pub const MAX_COMMAND_PROMPT_BYTES: usize = 64 * 1024;
pub const MAX_CHOOSE_TREE_QUERY_BYTES: usize = 4 * 1024;
pub const MAX_CHOOSE_BUFFER_QUERY_BYTES: usize = 4 * 1024;
pub const MAX_BROWSER_KEY_REPEAT: u32 = 9_999;
/// Longest either half of a rendered status line may be.
pub const MAX_STATUS_TEXT_BYTES: usize = 4096;
/// Most personalized status rows one client may be shown.
pub const MAX_STATUS_ROWS: usize = 5;
/// Longest expanded `display-panes-format` label one pane indicator may carry.
pub const MAX_PANE_INDICATOR_LABEL_BYTES: usize = 1024;
/// Longest shortcut key spelling one chooser row may carry.
pub const MAX_CHOOSE_ITEM_KEY_BYTES: usize = 64;
/// Longest payload `agent-send` may push into a GUI-owned composer or prompt.
pub const MAX_AGENT_SEND_BYTES: usize = 1024 * 1024;
/// Longest path or human-readable message carried by a GUI request or its reply.
pub const MAX_GUI_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_CLIENT_WORKING_DIRECTORY_BYTES: usize = 16 * 1024;
pub const MAX_CLIENT_ENVIRONMENT_ENTRIES: usize = 4096;
pub const MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES: usize = 16_367;
pub const MAX_CLIENT_ENVIRONMENT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_STARTUP_CONFIG_CAUSES: usize = 1024;
pub const MAX_STARTUP_CONFIG_CAUSE_BYTES: usize = 64 * 1024;
pub const MAX_STARTUP_CONFIG_CAUSES_BYTES: usize = 1024 * 1024;
/// Largest complete agent prompt: its text plus every attached image. Clients
/// normalize prompt images the way pasted ones are, so this mirrors
/// [`MAX_PASTE_UPLOAD_BYTES`].
pub const MAX_AGENT_PROMPT_BYTES: usize = 6 * 1024 * 1024;
pub const MAX_AGENT_PROMPT_IMAGES: usize = 64;
pub const MAX_AGENT_QUEUED_PROMPTS: usize = 4;
pub const MAX_AGENT_AUTH_METHODS: usize = 32;
pub const MAX_AGENT_PERMISSION_OPTIONS: usize = 32;
pub const MAX_AGENT_AVAILABLE_COMMANDS: usize = 256;
pub const MAX_AGENT_CONFIG_OPTIONS: usize = 64;
pub const MAX_AGENT_CONFIG_CHOICES: usize = 128;
pub const MAX_AGENT_MODES: usize = 64;
pub const MAX_AGENT_TOOL_CONTENT_ITEMS: usize = 128;
/// Longest encoded-format label one prompt image may name.
pub const MAX_AGENT_IMAGE_FORMAT_BYTES: usize = 64;
/// Longest option, mode, or authentication-method identifier on the agent lane.
pub const MAX_AGENT_OPTION_BYTES: usize = 4 * 1024;
/// Longest agent session identifier, matching what an agent pane descriptor accepts.
pub const MAX_AGENT_SESSION_ID_BYTES: usize = 16 * 1024;
pub const MAX_AGENT_SESSION_DIRECTORIES: usize = 256;
/// Largest batch of journal items one `AgentUpdates` event may carry. The
/// daemon splits a longer coalescing window across frames.
pub const MAX_AGENT_UPDATES_BYTES: usize = 9 * 1024 * 1024;
/// Largest JSON blob one [`AgentPaneWire`] field may carry.
pub const MAX_AGENT_STATE_BLOB_BYTES: usize = 256 * 1024;
/// Largest pending permission request payload carried by [`AgentPaneWire`].
pub const MAX_AGENT_PERMISSION_BYTES: usize = 64 * 1024;
/// Largest JSON reply to an agent session listing or turn-diff request.
pub const MAX_AGENT_RESULT_BYTES: usize = 1024 * 1024;
/// Largest complete image one paste upload may carry. Clients normalize
/// pasted images to 5 MiB first, so this leaves that cap headroom.
pub const MAX_PASTE_UPLOAD_BYTES: u32 = 6 * 1024 * 1024;
/// Largest single chunk of a paste upload.
pub const MAX_PASTE_UPLOAD_CHUNK_BYTES: usize = 1024 * 1024;
/// Largest decoded premultiplied BGRA Kitty image carried by the control lane.
pub const MAX_KITTY_IMAGE_BYTES: u32 = 16 * 1024 * 1024;
/// Largest ordered slice of one decoded Kitty image.
pub const MAX_KITTY_IMAGE_CHUNK_BYTES: usize = 1024 * 1024;
/// Longest file extension a paste upload may name.
pub const MAX_PASTE_UPLOAD_EXTENSION_BYTES: usize = 8;
pub(crate) const MAX_SERVER_CAPABILITIES: usize = 64;
pub(crate) const MAX_SERVER_CAPABILITY_BYTES: usize = 256;
pub(crate) const MAX_DEVICE_NAME_BYTES: usize = 256;
pub(crate) const MAX_CONFIG_OVERRIDE_ENTRIES: usize = 1024;
pub(crate) const MAX_CONFIG_OVERRIDE_KEY_BYTES: usize = 128;
pub(crate) const MAX_CONFIG_OVERRIDE_VALUE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_MUX_OPTION_VALUE_BYTES: usize = 64 * 1024;

/// The adapter commands an agent pane spawns, version-pinned on purpose:
/// `@latest` costs a registry round trip on every pane spawn, so bumps are
/// deliberate edits.
pub const DEFAULT_AGENT_COMMAND: &str = "npx -y @agentclientprotocol/codex-acp@1.3.0";
pub const DEFAULT_AGENT_CLAUDE_CODE_COMMAND: &str =
    "npx -y @agentclientprotocol/claude-agent-acp@0.68.0";
pub const DEFAULT_AGENT_AUTO_APPROVE: bool = true;
/// Longest adapter command line an agent mux option may carry.
pub const MAX_AGENT_COMMAND_BYTES: usize = 4 * 1024;

pub type ConfigOverrideEntry = (String, String);

/// One daemon-owned mux option exposed through the native settings surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MuxOptionKey {
    Prefix,
    ModeKeys,
    HistoryLimit,
    WordSeparators,
    CopyCommand,
    SetClipboard,
    BufferLimit,
    SynchronizePanes,
    // Postcard tags variants by index: append new keys, never reorder.
    ExperimentalAgentPane,
    ExperimentalEditorPane,
    HistoryTrickle,
    AgentCommand,
    AgentClaudeCodeCommand,
    AgentAutoApprove,
    Mouse,
    EscapeTime,
    Prefix2,
}

impl MuxOptionKey {
    pub const ALL: [Self; 17] = [
        Self::Prefix,
        Self::ModeKeys,
        Self::HistoryLimit,
        Self::WordSeparators,
        Self::CopyCommand,
        Self::SetClipboard,
        Self::BufferLimit,
        Self::SynchronizePanes,
        Self::ExperimentalAgentPane,
        Self::ExperimentalEditorPane,
        Self::HistoryTrickle,
        Self::AgentCommand,
        Self::AgentClaudeCodeCommand,
        Self::AgentAutoApprove,
        Self::Mouse,
        Self::EscapeTime,
        Self::Prefix2,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prefix => "prefix",
            Self::ModeKeys => "mode-keys",
            Self::HistoryLimit => "history-limit",
            Self::WordSeparators => "word-separators",
            Self::CopyCommand => "copy-command",
            Self::SetClipboard => "set-clipboard",
            Self::BufferLimit => "buffer-limit",
            Self::SynchronizePanes => "synchronize-panes",
            Self::ExperimentalAgentPane => "experimental-agent-pane",
            Self::ExperimentalEditorPane => "experimental-editor-pane",
            Self::HistoryTrickle => "history-trickle",
            Self::AgentCommand => "agent-command",
            Self::AgentClaudeCodeCommand => "agent-claude-code-command",
            Self::AgentAutoApprove => "agent-auto-approve",
            Self::Mouse => "mouse",
            Self::EscapeTime => "escape-time",
            Self::Prefix2 => "prefix2",
        }
    }

    #[must_use]
    pub fn from_config_key(key: &str) -> Option<Self> {
        match key {
            "prefix" => Some(Self::Prefix),
            "mode-keys" => Some(Self::ModeKeys),
            "history-limit" => Some(Self::HistoryLimit),
            "word-separators" => Some(Self::WordSeparators),
            "copy-command" => Some(Self::CopyCommand),
            "set-clipboard" => Some(Self::SetClipboard),
            "buffer-limit" => Some(Self::BufferLimit),
            "synchronize-panes" => Some(Self::SynchronizePanes),
            "experimental-agent-pane" => Some(Self::ExperimentalAgentPane),
            "experimental-editor-pane" => Some(Self::ExperimentalEditorPane),
            "history-trickle" => Some(Self::HistoryTrickle),
            "agent-command" => Some(Self::AgentCommand),
            "agent-claude-code-command" => Some(Self::AgentClaudeCodeCommand),
            "agent-auto-approve" => Some(Self::AgentAutoApprove),
            "mouse" => Some(Self::Mouse),
            "escape-time" => Some(Self::EscapeTime),
            "prefix2" => Some(Self::Prefix2),
            _ => None,
        }
    }

    fn default_display_value(self) -> String {
        match self {
            Self::Prefix => "C-b".to_owned(),
            Self::ModeKeys => "emacs".to_owned(),
            Self::HistoryLimit => DEFAULT_HISTORY_LIMIT.to_string(),
            Self::WordSeparators => DEFAULT_WORD_SEPARATORS.to_owned(),
            Self::CopyCommand => String::new(),
            Self::SetClipboard => "external".to_owned(),
            Self::BufferLimit => "50".to_owned(),
            Self::SynchronizePanes | Self::ExperimentalAgentPane | Self::ExperimentalEditorPane => {
                "off".to_owned()
            }
            Self::HistoryTrickle => "2000".to_owned(),
            Self::Mouse => "on".to_owned(),
            Self::EscapeTime => "10".to_owned(),
            Self::Prefix2 => "None".to_owned(),
            Self::AgentCommand => DEFAULT_AGENT_COMMAND.to_owned(),
            Self::AgentClaudeCodeCommand => DEFAULT_AGENT_CLAUDE_CODE_COMMAND.to_owned(),
            Self::AgentAutoApprove => if DEFAULT_AGENT_AUTO_APPROVE {
                "on"
            } else {
                "off"
            }
            .to_owned(),
        }
    }
}

/// The last writer of one effective daemon-owned mux option.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MuxOptionSource {
    #[default]
    Default,
    TmuxConfig,
    Override,
    RuntimeCommand,
}

/// One effective mux option value and its last-writer provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MuxOptionValue {
    #[serde(deserialize_with = "deserialize_mux_option_value")]
    pub value: String,
    pub source: MuxOptionSource,
}

/// Canonically ordered, complete map of daemon-owned mux option state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MuxOptions(BTreeMap<MuxOptionKey, MuxOptionValue>);

impl Default for MuxOptions {
    fn default() -> Self {
        Self(
            MuxOptionKey::ALL
                .into_iter()
                .map(|key| {
                    (
                        key,
                        MuxOptionValue {
                            value: key.default_display_value(),
                            source: MuxOptionSource::Default,
                        },
                    )
                })
                .collect(),
        )
    }
}

impl MuxOptions {
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = (MuxOptionKey, MuxOptionValue)>) -> Self {
        Self(entries.into_iter().collect())
    }

    #[must_use]
    pub fn get(&self, key: MuxOptionKey) -> Option<&MuxOptionValue> {
        self.0.get(&key)
    }

    pub fn set(
        &mut self,
        key: MuxOptionKey,
        value: impl Into<String>,
        source: MuxOptionSource,
    ) -> bool {
        let next = MuxOptionValue {
            value: value.into(),
            source,
        };
        if self.0.get(&key) == Some(&next) {
            return false;
        }
        self.0.insert(key, next);
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (MuxOptionKey, &MuxOptionValue)> {
        self.0.iter().map(|(key, value)| (*key, value))
    }

    /// Ensure a wire payload contains exactly one bounded value for every supported key.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.0.len() != MuxOptionKey::ALL.len()
            || MuxOptionKey::ALL
                .iter()
                .any(|key| !self.0.contains_key(key))
        {
            return Err("mux options must contain every supported key exactly once");
        }
        if self
            .0
            .values()
            .any(|value| value.value.len() > MAX_MUX_OPTION_VALUE_BYTES)
        {
            return Err("mux option values exceed the wire byte limit");
        }
        Ok(())
    }
}

fn deserialize_mux_option_value<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct MuxOptionValueVisitor;

    impl<'de> Visitor<'de> for MuxOptionValueVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "a mux option value no longer than {MAX_MUX_OPTION_VALUE_BYTES} bytes"
            )
        }

        fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            self.visit_str(value)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if value.len() > MAX_MUX_OPTION_VALUE_BYTES {
                return Err(E::invalid_length(value.len(), &self));
            }
            Ok(value.to_owned())
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if value.len() > MAX_MUX_OPTION_VALUE_BYTES {
                return Err(E::invalid_length(value.len(), &self));
            }
            Ok(value)
        }
    }

    deserializer.deserialize_str(MuxOptionValueVisitor)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientKind {
    Interactive,
    Command,
    Control,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessageKind {
    Info,
    Success,
    Warning,
    Error,
}

/// Where the status block sits relative to the main canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusPosition {
    Top,
    #[default]
    Bottom,
}

impl StatusPosition {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

/// One rendered tmux status line, expanded by the daemon. Text, never formats:
/// a `#()` command runs once on the daemon's host, not once per client.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusLine {
    #[serde(deserialize_with = "deserialize_status_text")]
    pub left: String,
    #[serde(deserialize_with = "deserialize_status_text")]
    pub right: String,
    #[serde(deserialize_with = "deserialize_status_text")]
    pub title: String,
    #[serde(deserialize_with = "deserialize_status_text")]
    pub base_style: String,
    #[serde(deserialize_with = "deserialize_status_rows")]
    pub rows: Vec<String>,
    pub position: StatusPosition,
    pub message_line: u8,
    pub customized: bool,
}

impl StatusLine {
    /// Whether the status block is off: zero published rows. Blank rows still
    /// count — they consume geometry and paint `base_style`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Ensure a wire payload stays inside its byte, row, style, and
    /// message-line bounds.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.left.len() > MAX_STATUS_TEXT_BYTES
            || self.right.len() > MAX_STATUS_TEXT_BYTES
            || self.title.len() > MAX_STATUS_TEXT_BYTES
            || self.base_style.len() > MAX_STATUS_TEXT_BYTES
        {
            return Err("status text exceeds the wire byte limit");
        }
        if self.rows.len() > MAX_STATUS_ROWS {
            return Err("status rows exceed the wire row limit");
        }
        if self
            .rows
            .iter()
            .any(|row| row.len() > MAX_STATUS_TEXT_BYTES)
        {
            return Err("status row exceeds the wire byte limit");
        }
        if crate::parse_style(&self.base_style).is_none() {
            return Err("status base style does not parse as a style");
        }
        if self.rows.is_empty() {
            if self.message_line != 0 {
                return Err("status message line names a row while no rows are published");
            }
        } else if usize::from(self.message_line) >= self.rows.len() {
            return Err("status message line names a row outside the published rows");
        }
        Ok(())
    }
}

fn deserialize_status_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_STATUS_TEXT_BYTES)
}

struct BoundedStatusRow(String);

impl<'de> Deserialize<'de> for BoundedStatusRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StatusRowVisitor;

        impl Visitor<'_> for StatusRowVisitor {
            type Value = BoundedStatusRow;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "a status row no longer than {MAX_STATUS_TEXT_BYTES} bytes"
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_STATUS_TEXT_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedStatusRow(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_STATUS_TEXT_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedStatusRow(value))
            }
        }

        deserializer.deserialize_str(StatusRowVisitor)
    }
}

fn deserialize_status_rows<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StatusRowsVisitor;

    impl<'de> Visitor<'de> for StatusRowsVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "at most {MAX_STATUS_ROWS} bounded status rows")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let hint = sequence.size_hint();
            if hint.is_some_and(|length| length > MAX_STATUS_ROWS) {
                return Err(A::Error::invalid_length(
                    hint.unwrap_or(MAX_STATUS_ROWS.saturating_add(1)),
                    &self,
                ));
            }
            let mut rows = Vec::with_capacity(hint.unwrap_or(0).min(MAX_STATUS_ROWS));
            while rows.len() < MAX_STATUS_ROWS {
                let Some(row) = sequence.next_element::<BoundedStatusRow>()? else {
                    return Ok(rows);
                };
                rows.push(row.0);
            }
            if sequence.next_element::<BoundedStatusRow>()?.is_some() {
                return Err(A::Error::invalid_length(
                    MAX_STATUS_ROWS.saturating_add(1),
                    &self,
                ));
            }
            Ok(rows)
        }
    }

    deserializer.deserialize_seq(StatusRowsVisitor)
}

fn deserialize_agent_send_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_AGENT_SEND_BYTES)
}

fn deserialize_gui_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_GUI_TEXT_BYTES)
}

fn deserialize_agent_prompt_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_AGENT_PROMPT_BYTES)
}

fn deserialize_agent_image_format<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_AGENT_IMAGE_FORMAT_BYTES)
}

fn deserialize_agent_image_data<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let bytes = Vec::<u8>::deserialize(deserializer)?;
    if bytes.len() > MAX_AGENT_PROMPT_BYTES {
        return Err(D::Error::invalid_length(
            bytes.len(),
            &"a prompt image within the wire byte limit",
        ));
    }
    Ok(bytes)
}

fn deserialize_agent_option_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_AGENT_OPTION_BYTES)
}

fn deserialize_optional_agent_option_text<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_optional_text(deserializer, MAX_AGENT_OPTION_BYTES)
}

fn deserialize_agent_session_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_AGENT_SESSION_ID_BYTES)
}

fn deserialize_optional_agent_session_id<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_optional_text(deserializer, MAX_AGENT_SESSION_ID_BYTES)
}

fn deserialize_agent_state_blob<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_AGENT_STATE_BLOB_BYTES)
}

fn deserialize_optional_agent_state_blob<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_optional_text(deserializer, MAX_AGENT_STATE_BLOB_BYTES)
}

fn deserialize_agent_images<'de, D>(deserializer: D) -> Result<Vec<AgentImage>, D::Error>
where
    D: Deserializer<'de>,
{
    let images = Vec::<AgentImage>::deserialize(deserializer)?;
    if images.len() > MAX_AGENT_PROMPT_IMAGES {
        return Err(D::Error::invalid_length(
            images.len(),
            &"an agent prompt within the image count limit",
        ));
    }
    Ok(images)
}

fn deserialize_agent_permission_payload<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_AGENT_PERMISSION_BYTES)
}

fn deserialize_agent_result<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_AGENT_RESULT_BYTES)
}

fn deserialize_agent_update_items<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    let items = Vec::<Vec<u8>>::deserialize(deserializer)?;
    if agent_update_batch_bytes(&items) > MAX_AGENT_UPDATES_BYTES {
        return Err(D::Error::invalid_length(
            items.len(),
            &"an agent update batch within the wire byte limit",
        ));
    }
    Ok(items)
}

/// Total wire bytes an `AgentUpdates` batch carries, saturating rather than
/// wrapping so a forged length can never appear small.
#[must_use]
pub fn agent_update_batch_bytes(items: &[Vec<u8>]) -> usize {
    items
        .iter()
        .fold(0_usize, |total, item| total.saturating_add(item.len()))
}

/// Whether `extension` can safely suffix a daemon-written file name. The daemon
/// interpolates it into a path, so only lowercase ASCII alphanumerics pass.
#[must_use]
pub fn paste_upload_extension_is_valid(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= MAX_PASTE_UPLOAD_EXTENSION_BYTES
        && extension
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

/// What the daemon should do after it assembles one pasted-image upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasteUploadPurpose {
    /// Write the bytes on the daemon host and paste the resulting path.
    PastePath,
    /// Keep the encoded image until the terminal prints its numbered placeholder.
    RecordPastedImage,
}

/// Encoded formats accepted by the pasted-image preview cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PastedImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl PastedImageFormat {
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "gif" => Self::Gif,
            "webp" => Self::Webp,
            _ => return None,
        })
    }
}

fn deserialize_paste_upload_extension<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let extension = String::deserialize(deserializer)?;
    if !paste_upload_extension_is_valid(&extension) {
        return Err(D::Error::invalid_value(
            serde::de::Unexpected::Str(&extension),
            &"1 to 8 lowercase ASCII alphanumerics",
        ));
    }
    Ok(extension)
}

fn deserialize_paste_upload_total<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let total_bytes = u32::deserialize(deserializer)?;
    if total_bytes == 0 || total_bytes > MAX_PASTE_UPLOAD_BYTES {
        return Err(D::Error::invalid_length(
            total_bytes as usize,
            &"a nonempty paste upload within the wire byte limit",
        ));
    }
    Ok(total_bytes)
}

fn deserialize_paste_upload_chunk<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let bytes = Vec::<u8>::deserialize(deserializer)?;
    if bytes.len() > MAX_PASTE_UPLOAD_CHUNK_BYTES {
        return Err(D::Error::invalid_length(
            bytes.len(),
            &"a paste upload chunk within the wire byte limit",
        ));
    }
    Ok(bytes)
}

pub(crate) fn deserialize_bounded_text<'de, D>(
    deserializer: D,
    limit: usize,
) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    if text.len() > limit {
        return Err(D::Error::invalid_length(
            text.len(),
            &"text within the wire byte limit",
        ));
    }
    Ok(text)
}

fn deserialize_bounded_optional_text<'de, D>(
    deserializer: D,
    limit: usize,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let text = Option::<String>::deserialize(deserializer)?;
    if let Some(text) = &text
        && text.len() > limit
    {
        return Err(D::Error::invalid_length(
            text.len(),
            &"text within the wire byte limit",
        ));
    }
    Ok(text)
}

struct BoundedClientWorkingDirectory(PathBuf);

impl<'de> Deserialize<'de> for BoundedClientWorkingDirectory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WorkingDirectoryVisitor;

        impl<'de> Visitor<'de> for WorkingDirectoryVisitor {
            type Value = BoundedClientWorkingDirectory;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "a client working directory no longer than \
                     {MAX_CLIENT_WORKING_DIRECTORY_BYTES} bytes"
                )
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_CLIENT_WORKING_DIRECTORY_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedClientWorkingDirectory(PathBuf::from(value)))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_CLIENT_WORKING_DIRECTORY_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedClientWorkingDirectory(PathBuf::from(value)))
            }
        }

        deserializer.deserialize_str(WorkingDirectoryVisitor)
    }
}

fn deserialize_client_working_directory<'de, D>(
    deserializer: D,
) -> Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        Option::<BoundedClientWorkingDirectory>::deserialize(deserializer)?
            .map(|working_directory| working_directory.0),
    )
}

struct BoundedClientEnvironmentEntry(String);

impl<'de> Deserialize<'de> for BoundedClientEnvironmentEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ClientEnvironmentEntryVisitor;

        impl<'de> Visitor<'de> for ClientEnvironmentEntryVisitor {
            type Value = BoundedClientEnvironmentEntry;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "a NAME=VALUE environment entry no longer than \
                     {MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES} bytes"
                )
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if !client_environment_entry_is_valid(value) {
                    return Err(E::custom("invalid client environment entry"));
                }
                Ok(BoundedClientEnvironmentEntry(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if !client_environment_entry_is_valid(&value) {
                    return Err(E::custom("invalid client environment entry"));
                }
                Ok(BoundedClientEnvironmentEntry(value))
            }
        }

        deserializer.deserialize_str(ClientEnvironmentEntryVisitor)
    }
}

fn client_environment_entry_is_valid(entry: &str) -> bool {
    entry.len() <= MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES
        && !entry.contains('\0')
        && entry
            .split_once('=')
            .is_some_and(|(name, _)| !name.is_empty())
}

pub(crate) fn client_environment_is_valid(environment: &[String]) -> bool {
    environment.len() <= MAX_CLIENT_ENVIRONMENT_ENTRIES
        && environment
            .iter()
            .all(|entry| client_environment_entry_is_valid(entry))
        && environment
            .iter()
            .try_fold(0usize, |total, entry| total.checked_add(entry.len()))
            .is_some_and(|total| total <= MAX_CLIENT_ENVIRONMENT_BYTES)
}

fn deserialize_client_environment<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ClientEnvironmentVisitor;

    impl<'de> Visitor<'de> for ClientEnvironmentVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "at most {MAX_CLIENT_ENVIRONMENT_ENTRIES} environment entries totaling at most \
                 {MAX_CLIENT_ENVIRONMENT_BYTES} bytes"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let hint = sequence.size_hint();
            if hint.is_some_and(|length| length > MAX_CLIENT_ENVIRONMENT_ENTRIES) {
                return Err(A::Error::invalid_length(
                    hint.unwrap_or(MAX_CLIENT_ENVIRONMENT_ENTRIES.saturating_add(1)),
                    &self,
                ));
            }
            let mut environment =
                Vec::with_capacity(hint.unwrap_or(0).min(MAX_CLIENT_ENVIRONMENT_ENTRIES));
            let mut total_bytes = 0usize;
            while environment.len() < MAX_CLIENT_ENVIRONMENT_ENTRIES {
                let Some(entry) = sequence.next_element::<BoundedClientEnvironmentEntry>()? else {
                    return Ok(environment);
                };
                total_bytes = total_bytes
                    .checked_add(entry.0.len())
                    .ok_or_else(|| A::Error::invalid_length(usize::MAX, &self))?;
                if total_bytes > MAX_CLIENT_ENVIRONMENT_BYTES {
                    return Err(A::Error::invalid_length(total_bytes, &self));
                }
                environment.push(entry.0);
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(
                    MAX_CLIENT_ENVIRONMENT_ENTRIES.saturating_add(1),
                    &self,
                ));
            }
            Ok(environment)
        }
    }

    deserializer.deserialize_seq(ClientEnvironmentVisitor)
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHello {
    pub protocol_version: u16,
    pub client_instance_id: ClientInstanceId,
    pub kind: ClientKind,
    #[serde(deserialize_with = "deserialize_device_name")]
    pub device_name: Option<String>,
    #[serde(deserialize_with = "deserialize_capabilities")]
    pub capabilities: Vec<String>,
    pub color_scheme: Option<TerminalColorScheme>,
    /// The pane the client was invoked from (`$ZZ_PANE`). Untargeted commands
    /// resolve against it, matching tmux's `$TMUX_PANE`.
    pub origin: Option<PaneId>,
    #[serde(deserialize_with = "deserialize_client_working_directory")]
    pub working_directory: Option<PathBuf>,
    #[serde(deserialize_with = "deserialize_client_environment")]
    pub environment: Vec<String>,
    pub process_id: u32,
}

impl fmt::Debug for ClientHello {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientHello")
            .field("protocol_version", &self.protocol_version)
            .field("client_instance_id", &self.client_instance_id)
            .field("kind", &self.kind)
            .field("device_name", &self.device_name)
            .field("capabilities", &self.capabilities)
            .field("color_scheme", &self.color_scheme)
            .field("origin", &self.origin)
            .field("working_directory", &self.working_directory)
            .field("environment_entries", &self.environment.len())
            .field("process_id", &self.process_id)
            .finish()
    }
}

impl ClientHello {
    pub const CLIENT_TERMINAL_CAPABILITY: &'static str = CLIENT_TERMINAL_CAPABILITY;
    pub const CLIENT_NESTED_CAPABILITY: &'static str = CLIENT_NESTED_CAPABILITY;
    pub const CLIENT_TTY_CAPABILITY_PREFIX: &'static str = CLIENT_TTY_CAPABILITY_PREFIX;
    pub const CLIENT_SIZE_CAPABILITY_PREFIX: &'static str = CLIENT_SIZE_CAPABILITY_PREFIX;
    pub const STARTUP_CONFIG_OWNER_CAPABILITY: &'static str = "startup-config-owner-v1";
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerHello {
    pub protocol_version: u16,
    pub server_id: u64,
    pub client_id: ClientId,
    pub client_instance_id: ClientInstanceId,
    #[serde(deserialize_with = "deserialize_capabilities")]
    pub capabilities: Vec<String>,
    pub appearance: TerminalAppearance,
    pub appearance_provenance: AppearanceProvenance,
    pub mux_options: MuxOptions,
    pub status: StatusLine,
    /// Every key table at attach time, refreshed by
    /// [`EventPayload::KeyTablesChanged`].
    pub key_tables: Vec<KeyTableSnapshot>,
}

/// One key table flattened for the wire, so clients can label key hints,
/// render binding help, and detect conflicts against the daemon's real
/// bindings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyTableSnapshot {
    /// The table name: `root`, `prefix`, `copy-mode`, `copy-mode-vi`, or a
    /// custom `-T` table.
    pub name: String,
    pub bindings: Vec<KeyBindingSnapshot>,
}

/// One binding in a published key table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBindingSnapshot {
    /// The key in tmux-grammar spelling (`|`, `C-o`, `M-1`). Convert before display.
    pub key: String,
    /// The bound command sequence, with canonical names rather than aliases.
    pub commands: Vec<CommandInvocation>,
    /// Whether the binding repeats without leaving its table (`bind -r`).
    pub repeat: bool,
    /// The `bind -N` annotation, when one was given.
    pub note: Option<String>,
}

struct BoundedCapability(String);

impl<'de> Deserialize<'de> for BoundedCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CapabilityVisitor;

        impl<'de> Visitor<'de> for CapabilityVisitor {
            type Value = BoundedCapability;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "a capability no longer than {MAX_SERVER_CAPABILITY_BYTES} bytes"
                )
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_SERVER_CAPABILITY_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedCapability(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_SERVER_CAPABILITY_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedCapability(value))
            }
        }

        deserializer.deserialize_str(CapabilityVisitor)
    }
}

fn deserialize_device_name<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let name = Option::<String>::deserialize(deserializer)?;
    if let Some(name) = &name
        && name.len() > MAX_DEVICE_NAME_BYTES
    {
        return Err(serde::de::Error::invalid_length(
            name.len(),
            &"a device name no longer than 256 bytes",
        ));
    }
    Ok(name)
}

fn deserialize_capabilities<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct CapabilitiesVisitor;

    impl<'de> Visitor<'de> for CapabilitiesVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "at most {MAX_SERVER_CAPABILITIES} bounded capabilities"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let hint = sequence.size_hint();
            if hint.is_some_and(|length| length > MAX_SERVER_CAPABILITIES) {
                return Err(A::Error::invalid_length(
                    hint.unwrap_or(MAX_SERVER_CAPABILITIES.saturating_add(1)),
                    &self,
                ));
            }
            let mut capabilities =
                Vec::with_capacity(hint.unwrap_or(0).min(MAX_SERVER_CAPABILITIES));
            while capabilities.len() < MAX_SERVER_CAPABILITIES {
                let Some(capability) = sequence.next_element::<BoundedCapability>()? else {
                    return Ok(capabilities);
                };
                capabilities.push(capability.0);
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(
                    MAX_SERVER_CAPABILITIES.saturating_add(1),
                    &self,
                ));
            }
            Ok(capabilities)
        }
    }

    deserializer.deserialize_seq(CapabilitiesVisitor)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub source: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInvocation {
    pub name: String,
    pub args: Vec<String>,
    pub source: Option<SourceSpan>,
    command_blocks: Vec<u32>,
}

impl CommandInvocation {
    #[must_use]
    pub fn new(name: impl Into<String>, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            name: name.into(),
            args: args.into_iter().map(Into::into).collect(),
            source: None,
            command_blocks: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_source(mut self, source: SourceSpan) -> Self {
        self.source = Some(source);
        self
    }

    #[must_use]
    pub fn with_command_blocks(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.command_blocks = indices
            .into_iter()
            .filter(|index| *index < self.args.len())
            .filter_map(|index| u32::try_from(index).ok())
            .collect();
        self.command_blocks.sort_unstable();
        self.command_blocks.dedup();
        self
    }

    #[must_use]
    pub fn argument_is_command_block(&self, index: usize) -> bool {
        u32::try_from(index).is_ok_and(|index| self.command_blocks.contains(&index))
    }

    pub fn append_args(&mut self, command: Self) {
        let offset = self.args.len();
        let appended_len = command.args.len();
        self.args.extend(command.args);
        self.command_blocks
            .extend(command.command_blocks.into_iter().filter_map(|index| {
                let index = usize::try_from(index).ok()?;
                (index < appended_len)
                    .then(|| offset.checked_add(index))
                    .flatten()
                    .and_then(|index| u32::try_from(index).ok())
            }));
        self.command_blocks
            .retain(|index| usize::try_from(*index).is_ok_and(|index| index < self.args.len()));
        self.command_blocks.sort_unstable();
        self.command_blocks.dedup();
    }

    #[must_use]
    pub fn split_commands_from(&self, start: usize) -> Vec<Self> {
        split_tagged_command_words(
            self.args
                .iter()
                .cloned()
                .enumerate()
                .skip(start)
                .map(|(index, word)| (word, self.argument_is_command_block(index))),
        )
        .into_iter()
        .filter_map(|words| {
            let mut words = words.into_iter();
            let (name, name_is_command_block) = words.next()?;
            if name_is_command_block {
                return None;
            }
            let mut args = Vec::new();
            let mut command_blocks = Vec::new();
            for (index, (word, is_command_block)) in words.enumerate() {
                args.push(word);
                if is_command_block {
                    command_blocks.push(index);
                }
            }
            Some(Self::new(name, args).with_command_blocks(command_blocks))
        })
        .collect()
    }
}

/// tmux's `cmd_parse_from_arguments` word grammar: a word's trailing
/// unescaped `;` ends a command (a bare `;` word ends one with no argument),
/// while a trailing `\;` keeps a literal `;` in the word.
#[must_use]
pub fn split_command_words(words: impl IntoIterator<Item = String>) -> Vec<Vec<String>> {
    split_tagged_command_words(words.into_iter().map(|word| (word, ())))
        .into_iter()
        .map(|words| words.into_iter().map(|(word, ())| word).collect())
        .collect()
}

fn split_tagged_command_words<T>(
    words: impl IntoIterator<Item = (String, T)>,
) -> Vec<Vec<(String, T)>> {
    let mut commands = Vec::new();
    let mut current = Vec::new();
    for (mut word, tag) in words {
        let mut end = false;
        if word.ends_with(';') {
            word.pop();
            if word.ends_with('\\') {
                word.pop();
                word.push(';');
            } else {
                end = true;
            }
        }
        if !end || !word.is_empty() {
            current.push((word, tag));
        }
        if end && !current.is_empty() {
            commands.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        commands.push(current);
    }
    commands
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub request_id: u64,
    pub command: CommandInvocation,
    pub prepared: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedCommand {
    pub invocation: CommandInvocation,
    pub canonical_name: Option<String>,
    pub alias_matched: bool,
    pub result: PreparedCommandResult,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreparedCommandResult {
    Ready,
    Error(ServerError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandResponse {
    Success {
        request_id: u64,
        output: String,
        exit_code: u8,
        stderr: String,
    },
    Error {
        request_id: u64,
        error: ServerError,
        output: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ServerError {
    #[error("server protocol mismatch: client {client}, server {server}")]
    ProtocolMismatch { client: u16, server: u16 },
    #[error("target not found: {0}")]
    MissingTarget(String),
    #[error("ambiguous target: {0}")]
    AmbiguousTarget(String),
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    #[error("unsupported command: {0}")]
    UnsupportedCommand(String),
    #[error("invalid command: {0}")]
    InvalidCommand(String),
    #[error("pane is not attached: {0}")]
    PaneNotAttached(PaneId),
    #[error("pane has exited: {0}")]
    PaneExited(PaneId),
    #[error("internal server error: {0}")]
    Internal(String),
    #[error("can't find session: {0}")]
    SessionNotFound(String),
    #[error("can't find window: {0}")]
    WindowNotFound(String),
    #[error("can't find pane: {0}")]
    PaneNotFound(String),
    #[error("invalid command: {0}")]
    CommandParse(String),
    #[error("{0}")]
    PostAdmissionCallback(Box<ServerError>),
}

impl ServerError {
    #[must_use]
    pub const fn is_command_parse(&self) -> bool {
        match self {
            Self::CommandParse(_) => true,
            Self::PostAdmissionCallback(error) => error.is_command_parse(),
            _ => false,
        }
    }

    #[must_use]
    pub const fn is_post_admission_callback(&self) -> bool {
        matches!(self, Self::PostAdmissionCallback(_))
    }

    #[must_use]
    pub fn tmux_message(&self) -> String {
        match self {
            Self::InvalidCommand(message) | Self::CommandParse(message) => message.clone(),
            Self::PostAdmissionCallback(error) => error.tmux_message(),
            error => error.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyToken {
    Literal(String),
    Named(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMessage {
    Text {
        pane: PaneId,
        text: String,
    },
    Key {
        pane: PaneId,
        input: KeyInput,
        /// Whether a committed-text message from the input method follows this key.
        text_follows: bool,
    },
    BrowserSurfaceText {
        pane: PaneId,
        text: String,
    },
    BrowserSurfaceKey {
        pane: PaneId,
        input: KeyInput,
        /// Whether a committed-text message from the input method follows this key.
        text_follows: bool,
    },
    ResizeTerminal {
        pane: PaneId,
        columns: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    },
    TerminalView {
        pane: PaneId,
        action: TerminalViewAction,
    },
    ResizeCommandOutput {
        columns: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    },
    CommandOutputView {
        action: TerminalViewAction,
    },
    ChooseTree {
        action: ChooseTreeAction,
    },
    ChooseBuffer {
        action: ChooseBufferAction,
    },
    DisplayPanes {
        action: DisplayPanesAction,
    },
    CommandPrompt {
        action: CommandPromptAction,
    },
    /// Commits a split-divider drag. The ratio spans the split's full logical
    /// extent in units of [`SPLIT_RATIO_BASIS`].
    ResizeSplit {
        window: WindowId,
        split: SplitId,
        ratio_basis_points: u16,
    },
    CancelPrefix {
        request_id: u64,
    },
    Popup {
        action: PopupAction,
    },
    Menu {
        action: MenuAction,
    },
    Confirm {
        action: ConfirmAction,
    },
    /// The client's outer terminal was resized (`SIGWINCH`). The TUI emits it
    /// after each resize; the daemon stores the facts per client.
    ClientTerminalSize {
        columns: u16,
        rows: u16,
    },
    ClientFocus {
        focused: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandPromptAction {
    /// Persists locally edited text without echoing a prompt event back.
    Update {
        input: String,
        cursor: u32,
    },
    /// Submits the final value through the daemon-owned command path.
    Submit {
        input: String,
    },
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrowserCommand {
    Navigate(String),
    Reload,
    Back,
    Forward,
    SendKeys(Vec<KeyToken>),
    Key(KeyInput),
    /// Writes the pane's latest frame to `path` as a PNG, then answers
    /// `request_id` with [`GuiResponse`]. The daemon blocks the CLI until it replies.
    Screenshot {
        request_id: u64,
        #[serde(deserialize_with = "deserialize_gui_text")]
        path: String,
    },
    SendKeysRepeated {
        keys: Vec<KeyToken>,
        count: u32,
    },
}

/// Work the daemon asks the GUI to perform on an Agent pane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentCommand {
    /// Appends `text` to the pane's composer draft for the user to review.
    ComposerAppend {
        #[serde(deserialize_with = "deserialize_agent_send_text")]
        text: String,
    },
    /// Submits `text` as an ACP prompt. The GUI errors when the pane is busy.
    Prompt {
        #[serde(deserialize_with = "deserialize_agent_send_text")]
        text: String,
    },
}

impl AgentCommand {
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::ComposerAppend { text } | Self::Prompt { text } => text,
        }
    }
}

/// One image attached to an agent prompt, in the encoded form the daemon
/// converts into an ACP content block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentImage {
    #[serde(deserialize_with = "deserialize_agent_image_format")]
    pub format: String,
    #[serde(deserialize_with = "deserialize_agent_image_data")]
    pub data: Vec<u8>,
}

/// One session-management request against a pane's daemon-owned adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSessionOpKind {
    List {
        cwd: Option<PathBuf>,
        #[serde(deserialize_with = "deserialize_optional_agent_session_id")]
        cursor: Option<String>,
        replace: bool,
    },
    New {
        cwd: PathBuf,
    },
    Switch {
        #[serde(deserialize_with = "deserialize_agent_session_id")]
        session_id: String,
        cwd: PathBuf,
        additional_directories: Vec<PathBuf>,
    },
    Delete {
        #[serde(deserialize_with = "deserialize_agent_session_id")]
        session_id: String,
    },
}

/// The adapter connection phase a client renders without reading the stream.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentConnectionPhase {
    #[default]
    Starting,
    Ready,
    Running,
    AwaitingPermission,
    Failed {
        #[serde(deserialize_with = "deserialize_agent_state_blob")]
        message: String,
    },
}

/// The permission request parked in the daemon, so a late-attaching client
/// sees the same prompt the running client does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPermissionWire {
    pub request_id: u64,
    /// JSON: the tool call plus its options, shaped like the reducer's input.
    #[serde(deserialize_with = "deserialize_agent_permission_payload")]
    pub payload: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentGitSummary {
    #[serde(deserialize_with = "deserialize_optional_agent_option_text")]
    pub branch: Option<String>,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
}

/// One agent pane's live state, small enough to publish to every client
/// attached to the session. Postcard cannot carry the ACP SDK's JSON-shaped
/// types, so auth methods, config options, and modes cross as JSON blobs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPaneWire {
    pub phase: AgentConnectionPhase,
    pub queued_prompts: u32,
    #[serde(deserialize_with = "deserialize_optional_agent_session_id")]
    pub session_id: Option<String>,
    #[serde(deserialize_with = "deserialize_optional_agent_option_text")]
    pub title: Option<String>,
    #[serde(deserialize_with = "deserialize_optional_agent_state_blob")]
    pub error: Option<String>,
    #[serde(deserialize_with = "deserialize_agent_state_blob")]
    pub auth_methods: String,
    #[serde(deserialize_with = "deserialize_agent_state_blob")]
    pub config_options: String,
    #[serde(deserialize_with = "deserialize_agent_state_blob")]
    pub modes: String,
    pub pending_permission: Option<AgentPermissionWire>,
    pub git: Option<AgentGitSummary>,
}

impl AgentPaneWire {
    /// Ensure a wire payload stays inside the agent state byte limits.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let AgentConnectionPhase::Failed { message } = &self.phase
            && message.len() > MAX_AGENT_STATE_BLOB_BYTES
        {
            return Err("agent failure message exceeds the wire byte limit");
        }
        if self
            .session_id
            .as_ref()
            .is_some_and(|session_id| session_id.len() > MAX_AGENT_SESSION_ID_BYTES)
        {
            return Err("agent session ID exceeds the wire byte limit");
        }
        if self
            .title
            .as_ref()
            .is_some_and(|title| title.len() > MAX_AGENT_OPTION_BYTES)
        {
            return Err("agent title exceeds the wire byte limit");
        }
        if self
            .error
            .as_ref()
            .is_some_and(|error| error.len() > MAX_AGENT_STATE_BLOB_BYTES)
        {
            return Err("agent error exceeds the wire byte limit");
        }
        if self.auth_methods.len() > MAX_AGENT_STATE_BLOB_BYTES
            || self.config_options.len() > MAX_AGENT_STATE_BLOB_BYTES
            || self.modes.len() > MAX_AGENT_STATE_BLOB_BYTES
        {
            return Err("agent state blob exceeds the wire byte limit");
        }
        if self
            .pending_permission
            .as_ref()
            .is_some_and(|permission| permission.payload.len() > MAX_AGENT_PERMISSION_BYTES)
        {
            return Err("agent permission payload exceeds the wire byte limit");
        }
        if self
            .git
            .as_ref()
            .and_then(|git| git.branch.as_ref())
            .is_some_and(|branch| branch.len() > MAX_AGENT_OPTION_BYTES)
        {
            return Err("agent Git branch exceeds the wire byte limit");
        }
        Ok(())
    }
}

/// The GUI's answer to one daemon-issued request, correlated by `request_id`.
/// The only client-to-daemon reply in the protocol.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuiResponse {
    Success {
        request_id: u64,
        #[serde(deserialize_with = "deserialize_gui_text")]
        output: String,
    },
    Error {
        request_id: u64,
        #[serde(deserialize_with = "deserialize_gui_text")]
        message: String,
    },
}

impl GuiResponse {
    #[must_use]
    pub const fn request_id(&self) -> u64 {
        match self {
            Self::Success { request_id, .. } | Self::Error { request_id, .. } => *request_id,
        }
    }

    /// Ensure a wire payload stays inside [`MAX_GUI_TEXT_BYTES`].
    pub fn validate(&self) -> Result<(), &'static str> {
        let text = match self {
            Self::Success { output, .. } => output,
            Self::Error { message, .. } => message,
        };
        if text.len() > MAX_GUI_TEXT_BYTES {
            return Err("GUI response text exceeds the wire byte limit");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalUiCommand {
    BeginSearch { direction: SearchDirection },
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandPromptKind {
    Command,
    Value,
}

/// Which prompt history and completion family a prompt belongs to (`-T`).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandPromptType {
    #[default]
    Command,
    Search,
}

/// How the prompt consumes keys before it submits.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandPromptMode {
    #[default]
    Text,
    Single,
    Numeric,
    Incremental,
    Key,
    BackspaceExit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandPromptState {
    pub prompt: String,
    pub input: String,
    /// Cursor position measured in Unicode scalar values, not bytes.
    pub cursor: u32,
    pub kind: CommandPromptKind,
    /// Oldest-to-newest bounded history, populated only for command prompts.
    pub history: Vec<String>,
    pub prompt_type: CommandPromptType,
    pub mode: CommandPromptMode,
    pub no_freeze: bool,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChooseTreeKind {
    Windows,
    Panes,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChooseTreePaneKind {
    Terminal,
    Browser,
    Agent,
    Editor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChooseTreeTarget {
    Session(SessionId),
    Window(WindowId),
    Pane(PaneId),
}

impl std::fmt::Display for ChooseTreeTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(id) => id.fmt(formatter),
            Self::Window(id) => id.fmt(formatter),
            Self::Pane(id) => id.fmt(formatter),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChooseTreeItem {
    pub label: String,
    pub detail: String,
    pub target: ChooseTreeTarget,
    pub depth: u8,
    pub flags: u8,
    pub pane_kind: Option<ChooseTreePaneKind>,
    /// The row's shortcut key in tmux-grammar spelling, empty for none.
    #[serde(deserialize_with = "deserialize_choose_item_key")]
    pub key: String,
}

fn deserialize_choose_item_key<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_CHOOSE_ITEM_KEY_BYTES)
}

impl ChooseTreeItem {
    pub const EXPANDED: u8 = 1 << 0;
    pub const HAS_CHILDREN: u8 = 1 << 1;
    pub const ACTIVE: u8 = 1 << 2;

    #[must_use]
    pub const fn expanded(&self) -> bool {
        self.flags & Self::EXPANDED != 0
    }

    #[must_use]
    pub const fn has_children(&self) -> bool {
        self.flags & Self::HAS_CHILDREN != 0
    }

    #[must_use]
    pub const fn active(&self) -> bool {
        self.flags & Self::ACTIVE != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChooseTreeSearchState {
    pub query: String,
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChooseTreeState {
    pub items: Vec<ChooseTreeItem>,
    pub search: Option<ChooseTreeSearchState>,
    pub selected: u32,
    pub kind: ChooseTreeKind,
    pub filter_no_matches: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChooseTreeAction {
    Previous,
    Next,
    PagePrevious,
    PageNext,
    First,
    Last,
    Collapse,
    Expand,
    Activate,
    Select(u32),
    ActivateIndex(u32),
    SearchStart {
        reverse: bool,
    },
    SearchAppend(String),
    SearchBackspace,
    SearchAccept,
    SearchCancel,
    SearchNext {
        reverse: bool,
    },
    Close,
    /// A raw key press the daemon resolves through the `choose-tree` key
    /// table, so bindings stay rebindable and identical for every client.
    Key(KeyInput),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChooseBufferItem {
    pub name: String,
    pub preview: String,
    pub size_bytes: u64,
    pub created_unix_seconds: u64,
    /// The row's shortcut key in tmux-grammar spelling, empty for none.
    #[serde(deserialize_with = "deserialize_choose_item_key")]
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChooseBufferSearchState {
    pub query: String,
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChooseBufferState {
    pub items: Vec<ChooseBufferItem>,
    pub search: Option<ChooseBufferSearchState>,
    pub selected: u32,
    pub filter_no_matches: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChooseBufferAction {
    Previous,
    Next,
    PagePrevious,
    PageNext,
    First,
    Last,
    Paste,
    Delete,
    Select(u32),
    PasteIndex(u32),
    SearchStart {
        reverse: bool,
    },
    SearchAppend(String),
    SearchBackspace,
    SearchAccept,
    SearchCancel,
    SearchNext {
        reverse: bool,
    },
    Close,
    /// A raw key press the daemon resolves through the `choose-buffer` key
    /// table, so bindings stay rebindable and identical for every client.
    Key(KeyInput),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneIndicator {
    pub pane: PaneId,
    pub index: u32,
    /// ASCII key that selects this pane, or zero when the pane has no shortcut.
    pub select_key: u8,
    pub flags: u8,
    /// The expanded `display-panes-format` product for this pane, empty until
    /// the daemon publishes it.
    #[serde(deserialize_with = "deserialize_pane_indicator_label")]
    pub label: String,
}

impl PaneIndicator {
    pub const ACTIVE: u8 = 1 << 0;

    #[must_use]
    pub const fn active(&self) -> bool {
        self.flags & Self::ACTIVE != 0
    }

    #[must_use]
    pub const fn selection_key(&self) -> Option<char> {
        if self.select_key == 0 {
            None
        } else {
            Some(self.select_key as char)
        }
    }
}

fn deserialize_pane_indicator_label<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_text(deserializer, MAX_PANE_INDICATOR_LABEL_BYTES)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayPanesState {
    pub window: WindowId,
    pub duration_ms: u32,
    pub indicators: Vec<PaneIndicator>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayPanesAction {
    Key(KeyInput),
    Select(PaneId),
    Close,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopupBorderLines {
    #[default]
    Single,
    Double,
    Heavy,
    Simple,
    Rounded,
    Padded,
    None,
}

impl PopupBorderLines {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Double => "double",
            Self::Heavy => "heavy",
            Self::Simple => "simple",
            Self::Rounded => "rounded",
            Self::Padded => "padded",
            Self::None => "none",
        }
    }
}

impl std::str::FromStr for PopupBorderLines {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "single" => Ok(Self::Single),
            "double" => Ok(Self::Double),
            "heavy" => Ok(Self::Heavy),
            "simple" => Ok(Self::Simple),
            "rounded" => Ok(Self::Rounded),
            "padded" => Ok(Self::Padded),
            "none" => Ok(Self::None),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopupState {
    pub pane: PaneId,
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub client_columns: u16,
    pub client_rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
    pub title: String,
    pub style: String,
    pub border_style: String,
    pub border_lines: PopupBorderLines,
    pub close_on_exit: bool,
    pub close_on_exit_zero: bool,
    pub close_on_any_key: bool,
    pub dead: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopupAction {
    Text(String),
    Key { input: KeyInput, text_follows: bool },
    TerminalView(TerminalViewAction),
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    pub name: String,
    pub key: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuState {
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub client_columns: u16,
    pub client_rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
    pub title: String,
    pub style: String,
    pub selected_style: String,
    pub border_style: String,
    pub border_lines: PopupBorderLines,
    pub items: Vec<Option<MenuItem>>,
    pub selected: Option<u32>,
    pub stay_open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MenuAction {
    Choose(u32),
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmState {
    pub prompt: String,
    pub confirm_key: u8,
    pub default_yes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfirmAction {
    Reply(bool),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub sequence: u64,
    pub payload: EventPayload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetachReason {
    Requested,
    Evicted,
    SessionDestroyed,
    ServerStopping,
}

impl DetachReason {
    #[must_use]
    pub const fn is_requested(self) -> bool {
        matches!(self, Self::Requested)
    }

    #[must_use]
    pub const fn is_evicted(self) -> bool {
        matches!(self, Self::Evicted)
    }

    #[must_use]
    pub const fn is_session_destroyed(self) -> bool {
        matches!(self, Self::SessionDestroyed)
    }

    #[must_use]
    pub const fn is_server_stopping(self) -> bool {
        matches!(self, Self::ServerStopping)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ControlSourceFileEvent {
    ReadError(String),
    Complete,
}

struct BoundedStartupConfigCause(String);

impl<'de> Deserialize<'de> for BoundedStartupConfigCause {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StartupConfigCauseVisitor;

        impl<'de> Visitor<'de> for StartupConfigCauseVisitor {
            type Value = BoundedStartupConfigCause;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "a startup configuration cause no longer than {MAX_STARTUP_CONFIG_CAUSE_BYTES} bytes"
                )
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_STARTUP_CONFIG_CAUSE_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedStartupConfigCause(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_STARTUP_CONFIG_CAUSE_BYTES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(BoundedStartupConfigCause(value))
            }
        }

        deserializer.deserialize_str(StartupConfigCauseVisitor)
    }
}

fn deserialize_startup_config_causes<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StartupConfigCausesVisitor;

    impl<'de> Visitor<'de> for StartupConfigCausesVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "at most {MAX_STARTUP_CONFIG_CAUSES} startup configuration causes totaling at most {MAX_STARTUP_CONFIG_CAUSES_BYTES} bytes"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let hint = sequence.size_hint();
            if hint.is_some_and(|length| length > MAX_STARTUP_CONFIG_CAUSES) {
                return Err(A::Error::invalid_length(
                    hint.unwrap_or(MAX_STARTUP_CONFIG_CAUSES.saturating_add(1)),
                    &self,
                ));
            }
            let mut causes = Vec::with_capacity(hint.unwrap_or(0).min(MAX_STARTUP_CONFIG_CAUSES));
            let mut total_bytes = 0usize;
            while causes.len() < MAX_STARTUP_CONFIG_CAUSES {
                let Some(cause) = sequence.next_element::<BoundedStartupConfigCause>()? else {
                    return Ok(causes);
                };
                total_bytes = total_bytes
                    .checked_add(cause.0.len())
                    .ok_or_else(|| A::Error::invalid_length(usize::MAX, &self))?;
                if total_bytes > MAX_STARTUP_CONFIG_CAUSES_BYTES {
                    return Err(A::Error::invalid_length(total_bytes, &self));
                }
                causes.push(cause.0);
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(
                    MAX_STARTUP_CONFIG_CAUSES.saturating_add(1),
                    &self,
                ));
            }
            Ok(causes)
        }
    }

    deserializer.deserialize_seq(StartupConfigCausesVisitor)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EventPayload {
    Snapshot(MuxSnapshot),
    AppearanceChanged {
        appearance: Box<TerminalAppearance>,
        provenance: AppearanceProvenance,
    },
    MuxOptionsChanged {
        options: MuxOptions,
    },
    StatusChanged {
        status: StatusLine,
    },
    TerminalViewport {
        pane: PaneId,
        viewport: TerminalViewport,
    },
    TerminalPatch {
        pane: PaneId,
        patch: TerminalViewportPatch,
    },
    Clipboard {
        pane: PaneId,
        request_id: u64,
        target: ClipboardTarget,
        text: String,
    },
    BrowserCommand {
        pane: PaneId,
        command: BrowserCommand,
    },
    /// Composer or prompt work for a GUI-owned Agent pane. The GUI must answer
    /// `request_id` with a [`GuiResponse`]; a CLI `agent-send` blocks on it.
    AgentCommand {
        pane: PaneId,
        request_id: u64,
        command: AgentCommand,
    },
    TerminalUiCommand {
        pane: PaneId,
        command: TerminalUiCommand,
    },
    CommandPrompt {
        state: Option<CommandPromptState>,
    },
    CommandOutput {
        pane: PaneId,
        output_id: u64,
        viewport: Option<TerminalViewport>,
    },
    ChooseTree {
        state: Option<ChooseTreeState>,
    },
    ChooseTreeUpdate {
        search: Option<ChooseTreeSearchState>,
        selected: u32,
    },
    ChooseBuffer {
        state: Option<ChooseBufferState>,
    },
    ChooseBufferUpdate {
        search: Option<ChooseBufferSearchState>,
        selected: u32,
    },
    DisplayPanes {
        state: Option<DisplayPanesState>,
    },
    ClientMessage {
        pane: Option<PaneId>,
        kind: ClientMessageKind,
        text: String,
    },
    PaneRemoved(PaneId),
    ServerStopping,
    OpenUri {
        pane: PaneId,
        uri: String,
    },
    FocusSidebar,
    /// The receiving client's prefix sequence armed or cleared, so the client
    /// can claim those keys from focus contexts the daemon never sees.
    PrefixArmed {
        armed: bool,
    },
    Detached {
        session: SessionId,
        by: Option<String>,
        reason: DetachReason,
    },
    HistoryChunk {
        pane: PaneId,
        start: u32,
        total: u32,
        offset: u32,
        columns: u16,
        rows: Vec<Vec<PackedCell>>,
        dictionary: TerminalDictionary,
    },
    // Postcard tags variants by index: append new payloads, never reorder.
    /// A program rang BEL in `pane` and the bell was not already showing. The
    /// latched state rides [`PaneSnapshot::bell`].
    Bell {
        pane: PaneId,
    },
    /// The full replacement key tables, mirroring
    /// [`ServerHello::key_tables`].
    KeyTablesChanged {
        tables: Vec<KeyTableSnapshot>,
    },
    KittyImageBegin {
        pane: PaneId,
        image_id: u32,
        generation: u64,
        width: u32,
        height: u32,
        total_bytes: u32,
    },
    KittyImageChunk {
        pane: PaneId,
        image_id: u32,
        generation: u64,
        bytes: Vec<u8>,
    },
    KittyImagesRemoved {
        pane: PaneId,
        image_ids: Vec<u32>,
    },
    /// One coalesced batch of JSON agent stream items, numbered from the
    /// pane's journal sequence. `first_seq` names the first item; the rest
    /// follow it one by one.
    AgentUpdates {
        pane: PaneId,
        first_seq: u64,
        #[serde(deserialize_with = "deserialize_agent_update_items")]
        items: Vec<Vec<u8>>,
    },
    AgentState {
        pane: PaneId,
        state: AgentPaneWire,
    },
    /// The client's agent lane overflowed and was cleared; the client answers
    /// with [`ProtocolMessage::AgentReplay`] from `next_seq`.
    AgentLagged {
        pane: PaneId,
        next_seq: u64,
    },
    AgentSessions {
        pane: PaneId,
        request_id: u64,
        #[serde(deserialize_with = "deserialize_agent_result")]
        result: String,
    },
    TimedClientMessage {
        pane: Option<PaneId>,
        kind: ClientMessageKind,
        text: String,
        duration_ms: u32,
        message_id: u64,
    },
    PrefixCancelled {
        request_id: u64,
    },
    Popup {
        state: Option<PopupState>,
    },
    Menu {
        state: Option<MenuState>,
    },
    Confirm {
        state: Option<ConfirmState>,
    },
    ControlExit {
        reason: String,
    },
    HookEvent {
        name: String,
        variables: BTreeMap<String, String>,
    },
    PaneOutput {
        pane: PaneId,
        bytes: Vec<u8>,
    },
    PaneOutputState {
        pane: PaneId,
        paused: bool,
    },
    PaneOutputAged {
        pane: PaneId,
        age_ms: u64,
        bytes: Vec<u8>,
    },
    ControlFlags {
        wait_exit: bool,
        pause_after_ms: Option<u64>,
        no_output: bool,
    },
    SubscriptionChanged {
        name: String,
        session: SessionId,
        window: Option<WindowId>,
        window_index: Option<u32>,
        pane: Option<PaneId>,
        value: String,
    },
    /// The daemon retired the identified timed message: its duration ran out,
    /// or a key dismissed it. Surfaces drop the message only while the identity
    /// still matches what they are showing, so a retired message's timer can
    /// never take down the message that replaced it.
    TimedClientMessageCleared {
        message_id: u64,
    },
    ControlCommandGuard {
        output: String,
        error: bool,
        sticky_failure: bool,
        flags: u8,
    },
    ControlSourceFile {
        event: ControlSourceFileEvent,
    },
    StartupConfigCauses {
        #[serde(deserialize_with = "deserialize_startup_config_causes")]
        causes: Vec<String>,
    },
    ControlCommandOutput {
        output: String,
    },
}

impl EventPayload {
    #[must_use]
    pub fn detached_requested(session: SessionId, by: Option<String>) -> Self {
        Self::Detached {
            session,
            by,
            reason: DetachReason::Requested,
        }
    }

    #[must_use]
    pub fn detached_evicted(session: SessionId, by: Option<String>) -> Self {
        Self::Detached {
            session,
            by,
            reason: DetachReason::Evicted,
        }
    }

    #[must_use]
    pub fn detached_session_destroyed(session: SessionId) -> Self {
        Self::Detached {
            session,
            by: None,
            reason: DetachReason::SessionDestroyed,
        }
    }

    #[must_use]
    pub fn detached_server_stopping(session: SessionId) -> Self {
        Self::Detached {
            session,
            by: None,
            reason: DetachReason::ServerStopping,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[allow(
    clippy::large_enum_variant,
    reason = "messages are moved into framing immediately and enum layout must stay allocation-free for terminal events"
)]
pub enum ProtocolMessage {
    ClientHello(ClientHello),
    ServerHello(ServerHello),
    CommandRequest(CommandRequest),
    CommandResponse(CommandResponse),
    Attach {
        session: String,
    },
    Attached {
        session: SessionId,
        snapshot: MuxSnapshot,
        read_only: bool,
        client_flags: String,
    },
    Detach,
    SetColorScheme(TerminalColorScheme),
    SetConfigOverrides {
        entries: Vec<ConfigOverrideEntry>,
    },
    Input(InputMessage),
    Event(Event),
    /// Reply to an `EventPayload::AgentCommand` or `BrowserCommand::Screenshot`.
    GuiResponse(GuiResponse),
    Resync,
    RequestFull {
        pane: PaneId,
    },
    HistoryRequest {
        pane: PaneId,
        start: u32,
        count: u32,
    },
    /// Starts streaming one pasted image to the host `pane` lives on. `purpose`
    /// decides whether completion pastes a file path or records preview bytes.
    PasteUploadBegin {
        upload_id: u64,
        pane: PaneId,
        purpose: PasteUploadPurpose,
        #[serde(deserialize_with = "deserialize_paste_upload_extension")]
        extension: String,
        #[serde(deserialize_with = "deserialize_paste_upload_total")]
        total_bytes: u32,
    },
    /// One ordered slice of the upload `upload_id` announced. No offsets, no end
    /// marker: the upload completes at the declared `total_bytes`.
    PasteUploadChunk {
        upload_id: u64,
        #[serde(deserialize_with = "deserialize_paste_upload_chunk")]
        bytes: Vec<u8>,
    },
    FetchPastedImage {
        pane: PaneId,
        number: u32,
    },
    PastedImageBegin {
        pane: PaneId,
        number: u32,
        format: PastedImageFormat,
        #[serde(deserialize_with = "deserialize_paste_upload_total")]
        total_bytes: u32,
    },
    PastedImageChunk {
        pane: PaneId,
        number: u32,
        #[serde(deserialize_with = "deserialize_paste_upload_chunk")]
        bytes: Vec<u8>,
    },
    PastedImageUnavailable {
        pane: PaneId,
        number: u32,
    },
    // Postcard tags variants by index: append new messages, never reorder.
    AgentPrompt {
        pane: PaneId,
        #[serde(deserialize_with = "deserialize_agent_prompt_text")]
        text: String,
        #[serde(deserialize_with = "deserialize_agent_images")]
        images: Vec<AgentImage>,
    },
    AgentCancel {
        pane: PaneId,
    },
    /// Reclaim the pane's queued prompts; the daemon returns them inside the
    /// stream so the composer refills.
    AgentUnqueue {
        pane: PaneId,
    },
    /// Answer a parked permission request. `None` cancels it, and the first
    /// answer wins: a late one is a no-op.
    AgentRespondPermission {
        pane: PaneId,
        request_id: u64,
        #[serde(deserialize_with = "deserialize_optional_agent_option_text")]
        option_id: Option<String>,
    },
    AgentSetConfigOption {
        pane: PaneId,
        #[serde(deserialize_with = "deserialize_agent_option_text")]
        option_id: String,
        #[serde(deserialize_with = "deserialize_agent_option_text")]
        value: String,
    },
    AgentSetMode {
        pane: PaneId,
        #[serde(deserialize_with = "deserialize_agent_option_text")]
        mode_id: String,
    },
    AgentAuthenticate {
        pane: PaneId,
        #[serde(deserialize_with = "deserialize_agent_option_text")]
        method_id: String,
    },
    AgentSessionOp {
        pane: PaneId,
        op: AgentSessionOpKind,
    },
    /// Replay the pane's journal from `from_seq`, then tail it. Sent on
    /// attach, after [`EventPayload::AgentLagged`], and when a pane becomes
    /// visible.
    AgentReplay {
        pane: PaneId,
        from_seq: u64,
    },
    AgentAcknowledgePromptRestore {
        pane: PaneId,
        reclaim_id: u64,
    },
    PrepareCommandList {
        request_id: u64,
        commands: Vec<CommandInvocation>,
    },
    PreparedCommandList {
        request_id: u64,
        commands: Vec<PreparedCommand>,
    },
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use zz_terminal::{
        Modifiers, PointerCellEvent, TerminalMouseButton, TerminalMouseInput, TerminalMousePhase,
    };

    use super::{MuxOptionKey, MuxOptions};

    #[derive(Serialize)]
    struct LegacyTerminalMouseInput {
        phase: TerminalMousePhase,
        button: Option<TerminalMouseButton>,
        cell: PointerCellEvent,
        x: u32,
        y: u32,
        screen_width: u32,
        screen_height: u32,
        cell_width: u32,
        cell_height: u32,
        modifiers: Modifiers,
        force_selection: bool,
    }

    #[test]
    fn server_error_tmux_message_strips_only_invalid_command_prefixes() {
        assert_eq!(
            super::ServerError::InvalidCommand("width too small".to_owned()).tmux_message(),
            "width too small"
        );
        assert_eq!(
            super::ServerError::CommandParse("usage: split-window".to_owned()).tmux_message(),
            "usage: split-window"
        );
        assert_eq!(
            super::ServerError::SessionNotFound("missing".to_owned()).tmux_message(),
            "can't find session: missing"
        );
    }

    #[test]
    fn command_parse_error_holds_wire_tag_twelve() {
        let error = super::ServerError::CommandParse("usage: split-window".to_owned());
        let bytes = postcard::to_stdvec(&error).expect("command parse error encodes");
        assert_eq!(bytes[0], 12);
        assert_eq!(
            postcard::from_bytes::<super::ServerError>(&bytes)
                .expect("command parse error decodes"),
            error
        );
        assert!(error.is_command_parse());
        assert!(!super::ServerError::InvalidCommand(String::new()).is_command_parse());
    }

    #[test]
    fn post_admission_callback_holds_wire_tag_thirteen() {
        let error = super::ServerError::PostAdmissionCallback(Box::new(
            super::ServerError::SessionNotFound("missing".to_owned()),
        ));
        let bytes = postcard::to_stdvec(&error).expect("callback error encodes");
        assert_eq!(bytes[0], 13);
        assert_eq!(
            postcard::from_bytes::<super::ServerError>(&bytes).expect("callback error decodes"),
            error
        );
        assert!(error.is_post_admission_callback());
        assert_eq!(error.tmux_message(), "can't find session: missing");
    }

    #[test]
    fn command_invocation_command_blocks_round_trip_and_append() {
        let mut command = super::CommandInvocation::new("bind-key", ["x", "{ first }"])
            .with_command_blocks([1, 1, 9]);
        let appended = super::CommandInvocation::new("unused", ["plain", "{ second }"])
            .with_command_blocks([1]);
        command.append_args(appended);

        assert_eq!(command.args, ["x", "{ first }", "plain", "{ second }"]);
        assert!(!command.argument_is_command_block(0));
        assert!(command.argument_is_command_block(1));
        assert!(!command.argument_is_command_block(2));
        assert!(command.argument_is_command_block(3));
        assert!(!command.argument_is_command_block(4));
        assert_eq!(command.command_blocks, [1, 3]);

        let bytes = postcard::to_stdvec(&command).expect("command invocation encodes");
        let decoded = postcard::from_bytes::<super::CommandInvocation>(&bytes)
            .expect("command invocation decodes");
        assert_eq!(decoded, command);
    }

    #[test]
    fn command_invocation_new_has_no_typed_blocks() {
        let command = super::CommandInvocation::new("display-message", ["{ literal }"]);
        assert!(!command.argument_is_command_block(0));
        assert!(command.command_blocks.is_empty());
    }

    #[test]
    fn command_invocation_split_preserves_typed_positions_across_chains() {
        let command = super::CommandInvocation::new(
            "bind-key",
            [
                "F10",
                "display-message",
                "first",
                ";",
                "if-shell",
                "-F",
                "1",
                "{ display-message -p true }",
                "{ display-message -p false }",
            ],
        )
        .with_command_blocks([7, 8]);

        let commands = command.split_commands_from(1);

        assert_eq!(
            commands,
            [
                super::CommandInvocation::new("display-message", ["first"]),
                super::CommandInvocation::new(
                    "if-shell",
                    [
                        "-F",
                        "1",
                        "{ display-message -p true }",
                        "{ display-message -p false }",
                    ],
                )
                .with_command_blocks([2, 3]),
            ]
        );
    }

    #[test]
    fn command_invocation_split_omits_typed_command_names() {
        let command = super::CommandInvocation::new(
            "bind-key",
            [
                "F4",
                "display-message",
                "first",
                ";",
                "{ display-message second }",
            ],
        )
        .with_command_blocks([4]);

        assert_eq!(
            command.split_commands_from(1),
            [super::CommandInvocation::new("display-message", ["first"],)]
        );
    }

    #[test]
    fn terminal_modifiers_use_one_validated_control_byte() {
        let modifiers = Modifiers::new(true, true, true, true);
        assert_eq!(postcard::to_stdvec(&modifiers).expect("encode"), [0x0f]);
        assert_eq!(
            postcard::from_bytes::<Modifiers>(&[0x0f]).expect("decode"),
            modifiers
        );
        assert!(postcard::from_bytes::<Modifiers>(&[0x10]).is_err());
    }

    #[test]
    fn packed_terminal_mouse_input_preserves_the_control_wire_layout() {
        let phase = TerminalMousePhase::Motion;
        let button = Some(TerminalMouseButton::Middle);
        let cell = PointerCellEvent {
            column: 41,
            row: 17,
            click_count: 3,
            rectangle: true,
        };
        let modifiers = Modifiers::new(true, false, true, true);
        let compact = TerminalMouseInput::new(
            phase, button, cell, 321, 654, 1_920, 1_080, 9, 18, modifiers, true,
        );
        let legacy = LegacyTerminalMouseInput {
            phase,
            button,
            cell,
            x: 321,
            y: 654,
            screen_width: 1_920,
            screen_height: 1_080,
            cell_width: 9,
            cell_height: 18,
            modifiers,
            force_selection: true,
        };
        let compact_bytes = postcard::to_stdvec(&compact).expect("compact mouse input");
        let legacy_bytes = postcard::to_stdvec(&legacy).expect("legacy mouse input");

        assert_eq!(compact_bytes, legacy_bytes);
        assert_eq!(
            postcard::from_bytes::<TerminalMouseInput>(&compact_bytes)
                .expect("decoded mouse input"),
            compact
        );
    }

    #[test]
    fn agent_runtime_variants_hold_the_wire_tails_they_were_appended_to() {
        let pane = crate::PaneId(1);
        let message_tag = |message: &super::ProtocolMessage| {
            postcard::to_stdvec(message).expect("message encodes")[0]
        };
        let payload_tag = |payload: super::EventPayload| {
            postcard::to_stdvec(&super::Event {
                sequence: 0,
                payload,
            })
            .expect("event encodes")[1]
        };

        assert_eq!(
            message_tag(&super::ProtocolMessage::PastedImageUnavailable { pane, number: 0 }),
            20
        );
        for (index, message) in [
            super::ProtocolMessage::AgentPrompt {
                pane,
                text: String::new(),
                images: Vec::new(),
            },
            super::ProtocolMessage::AgentCancel { pane },
            super::ProtocolMessage::AgentUnqueue { pane },
            super::ProtocolMessage::AgentRespondPermission {
                pane,
                request_id: 0,
                option_id: None,
            },
            super::ProtocolMessage::AgentSetConfigOption {
                pane,
                option_id: String::new(),
                value: String::new(),
            },
            super::ProtocolMessage::AgentSetMode {
                pane,
                mode_id: String::new(),
            },
            super::ProtocolMessage::AgentAuthenticate {
                pane,
                method_id: String::new(),
            },
            super::ProtocolMessage::AgentSessionOp {
                pane,
                op: super::AgentSessionOpKind::List {
                    cwd: None,
                    cursor: None,
                    replace: true,
                },
            },
            super::ProtocolMessage::AgentReplay { pane, from_seq: 0 },
            super::ProtocolMessage::AgentAcknowledgePromptRestore {
                pane,
                reclaim_id: 0,
            },
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                u8::try_from(21 + index).expect("tag"),
                message_tag(&message)
            );
        }

        let invocation = super::CommandInvocation::new("list-sessions", std::iter::empty::<&str>());
        let prepared = super::PreparedCommand {
            invocation: invocation.clone(),
            canonical_name: Some("list-sessions".to_owned()),
            alias_matched: false,
            result: super::PreparedCommandResult::Ready,
        };
        for (message, tag) in [
            (
                super::ProtocolMessage::PrepareCommandList {
                    request_id: 7,
                    commands: vec![invocation],
                },
                31,
            ),
            (
                super::ProtocolMessage::PreparedCommandList {
                    request_id: 7,
                    commands: vec![prepared],
                },
                32,
            ),
        ] {
            let bytes = postcard::to_stdvec(&message).expect("prepare message encodes");
            assert_eq!(bytes[0], tag);
            assert_eq!(
                postcard::from_bytes::<super::ProtocolMessage>(&bytes)
                    .expect("prepare message decodes"),
                message
            );
        }

        assert_eq!(
            payload_tag(super::EventPayload::KittyImagesRemoved {
                pane,
                image_ids: Vec::new(),
            }),
            29
        );
        for (index, payload) in [
            super::EventPayload::AgentUpdates {
                pane,
                first_seq: 0,
                items: Vec::new(),
            },
            super::EventPayload::AgentState {
                pane,
                state: super::AgentPaneWire::default(),
            },
            super::EventPayload::AgentLagged { pane, next_seq: 0 },
            super::EventPayload::AgentSessions {
                pane,
                request_id: 0,
                result: String::new(),
            },
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(u8::try_from(30 + index).expect("tag"), payload_tag(payload));
        }
        assert_eq!(
            payload_tag(super::EventPayload::TimedClientMessage {
                pane: None,
                kind: super::ClientMessageKind::Info,
                text: String::new(),
                duration_ms: 0,
                message_id: 0,
            }),
            34
        );
        assert_eq!(
            payload_tag(super::EventPayload::PrefixCancelled { request_id: 0 }),
            35
        );
    }

    #[test]
    fn recent_mux_options_are_appended_with_their_defaults() {
        assert_eq!(
            postcard::to_stdvec(&MuxOptionKey::HistoryTrickle).expect("history option"),
            [10]
        );
        assert_eq!(
            MuxOptionKey::from_config_key("history-trickle"),
            Some(MuxOptionKey::HistoryTrickle)
        );
        assert_eq!(
            MuxOptions::default()
                .get(MuxOptionKey::HistoryTrickle)
                .expect("history trickle default")
                .value,
            "2000"
        );
        assert_eq!(
            postcard::to_stdvec(&MuxOptionKey::AgentAutoApprove).expect("agent option"),
            [13]
        );
        assert_eq!(
            MuxOptionKey::from_config_key("agent-command"),
            Some(MuxOptionKey::AgentCommand)
        );
        assert_eq!(
            MuxOptions::default()
                .get(MuxOptionKey::AgentCommand)
                .expect("agent command default")
                .value,
            super::DEFAULT_AGENT_COMMAND
        );
        assert_eq!(
            MuxOptions::default()
                .get(MuxOptionKey::AgentAutoApprove)
                .expect("agent auto-approve default")
                .value,
            "on"
        );
    }

    #[test]
    fn popup_variants_hold_the_appended_wire_tails() {
        let input = super::InputMessage::Popup {
            action: super::PopupAction::Close,
        };
        let input_bytes = postcard::to_stdvec(&input).expect("popup input encodes");
        assert_eq!(input_bytes[0], 14);
        assert_eq!(
            postcard::from_bytes::<super::InputMessage>(&input_bytes).expect("popup input decodes"),
            input
        );

        let event = super::Event {
            sequence: 7,
            payload: super::EventPayload::Popup { state: None },
        };
        let event_bytes = postcard::to_stdvec(&event).expect("popup event encodes");
        assert_eq!(event_bytes[1], 36);
        assert_eq!(
            postcard::from_bytes::<super::Event>(&event_bytes).expect("popup event decodes"),
            event
        );

        for (input, tag) in [
            (
                super::InputMessage::Menu {
                    action: super::MenuAction::Cancel,
                },
                15,
            ),
            (
                super::InputMessage::Confirm {
                    action: super::ConfirmAction::Reply(false),
                },
                16,
            ),
        ] {
            let bytes = postcard::to_stdvec(&input).expect("overlay input encodes");
            assert_eq!(bytes[0], tag);
            assert_eq!(
                postcard::from_bytes::<super::InputMessage>(&bytes).expect("overlay input decodes"),
                input
            );
        }
        for (payload, tag) in [
            (super::EventPayload::Menu { state: None }, 37),
            (super::EventPayload::Confirm { state: None }, 38),
        ] {
            let event = super::Event {
                sequence: 7,
                payload,
            };
            let bytes = postcard::to_stdvec(&event).expect("overlay event encodes");
            assert_eq!(bytes[1], tag);
            assert_eq!(
                postcard::from_bytes::<super::Event>(&bytes).expect("overlay event decodes"),
                event
            );
        }
    }

    #[test]
    fn control_event_variants_hold_the_appended_wire_tails() {
        let payload_tag = |payload| {
            postcard::to_stdvec(&super::Event {
                sequence: 0,
                payload,
            })
            .expect("control event encodes")[1]
        };

        assert_eq!(
            payload_tag(super::EventPayload::ControlExit {
                reason: String::new(),
            }),
            39
        );
        assert_eq!(
            payload_tag(super::EventPayload::HookEvent {
                name: String::new(),
                variables: std::collections::BTreeMap::new(),
            }),
            40
        );
        assert_eq!(
            payload_tag(super::EventPayload::PaneOutput {
                pane: crate::PaneId(1),
                bytes: Vec::new(),
            }),
            41
        );
        assert_eq!(
            payload_tag(super::EventPayload::PaneOutputState {
                pane: crate::PaneId(1),
                paused: false,
            }),
            42
        );
        assert_eq!(
            payload_tag(super::EventPayload::PaneOutputAged {
                pane: crate::PaneId(1),
                age_ms: 0,
                bytes: Vec::new(),
            }),
            43
        );
        assert_eq!(
            payload_tag(super::EventPayload::ControlFlags {
                wait_exit: false,
                pause_after_ms: None,
                no_output: false,
            }),
            44
        );
        assert_eq!(
            payload_tag(super::EventPayload::SubscriptionChanged {
                name: String::new(),
                session: crate::SessionId(1),
                window: None,
                window_index: None,
                pane: None,
                value: String::new(),
            }),
            45
        );
    }

    #[test]
    fn v71_mux_option_keys_hold_appended_tags_and_defaults() {
        for (key, tag, name, config_key) in [
            (
                MuxOptionKey::Mouse,
                14_u8,
                "mouse",
                Some(MuxOptionKey::Mouse),
            ),
            (
                MuxOptionKey::EscapeTime,
                15,
                "escape-time",
                Some(MuxOptionKey::EscapeTime),
            ),
            (
                MuxOptionKey::Prefix2,
                16,
                "prefix2",
                Some(MuxOptionKey::Prefix2),
            ),
        ] {
            assert_eq!(postcard::to_stdvec(&key).expect("mux option key"), [tag]);
            assert_eq!(key.as_str(), name);
            assert_eq!(MuxOptionKey::from_config_key(name), config_key);
        }
        let defaults = MuxOptions::default();
        assert_eq!(
            defaults.get(MuxOptionKey::Mouse).expect("mouse").value,
            "on"
        );
        assert_eq!(
            defaults
                .get(MuxOptionKey::EscapeTime)
                .expect("escape-time")
                .value,
            "10"
        );
        assert_eq!(
            defaults.get(MuxOptionKey::Prefix2).expect("prefix2").value,
            "None"
        );
        assert_eq!(defaults.validate(), Ok(()));
    }

    #[test]
    fn status_position_default_bottom_is_the_tag_one_variant() {
        assert_eq!(
            super::StatusPosition::default(),
            super::StatusPosition::Bottom
        );
        assert_eq!(
            postcard::to_stdvec(&super::StatusPosition::Top).expect("top"),
            [0]
        );
        assert_eq!(
            postcard::to_stdvec(&super::StatusPosition::Bottom).expect("bottom"),
            [1]
        );
        assert_eq!(
            postcard::from_bytes::<super::StatusPosition>(&[1]).expect("bottom decodes"),
            super::StatusPosition::Bottom
        );
        assert!(postcard::from_bytes::<super::StatusPosition>(&[2]).is_err());
    }

    #[derive(Serialize)]
    struct UnboundedStatusLine {
        left: String,
        right: String,
        title: String,
        base_style: String,
        rows: Vec<String>,
        position: super::StatusPosition,
        message_line: u8,
        customized: bool,
    }

    fn unbounded_status(rows: Vec<String>, message_line: u8) -> Vec<u8> {
        postcard::to_stdvec(&UnboundedStatusLine {
            left: String::new(),
            right: String::new(),
            title: String::new(),
            base_style: String::new(),
            rows,
            position: super::StatusPosition::Bottom,
            message_line,
            customized: false,
        })
        .expect("status line shape")
    }

    #[test]
    fn status_line_appends_after_the_v70_halves() {
        #[derive(Serialize)]
        struct LegacyStatusLine {
            left: String,
            right: String,
        }

        let status = super::StatusLine {
            left: "L".to_owned(),
            right: "R".to_owned(),
            ..super::StatusLine::default()
        };
        let bytes = postcard::to_stdvec(&status).expect("status");
        let legacy = postcard::to_stdvec(&LegacyStatusLine {
            left: "L".to_owned(),
            right: "R".to_owned(),
        })
        .expect("legacy halves");
        assert!(bytes.starts_with(&legacy));
        assert_eq!(
            postcard::from_bytes::<super::StatusLine>(&bytes).expect("status decodes"),
            status
        );
        assert!(status.is_empty(), "no rows means the status block is off");
        assert!(super::StatusLine::default().is_empty());
        assert!(
            !super::StatusLine {
                rows: vec![String::new()],
                ..super::StatusLine::default()
            }
            .is_empty(),
            "a blank row still counts as an on status block"
        );
    }

    #[test]
    fn status_rows_accept_zero_through_five_and_reject_a_sixth_before_allocation() {
        for count in 0..=super::MAX_STATUS_ROWS {
            let rows = (0..count)
                .map(|index| {
                    if index % 2 == 0 {
                        String::new()
                    } else {
                        format!("row {index}")
                    }
                })
                .collect::<Vec<_>>();
            let message_line = u8::try_from(count.saturating_sub(1)).expect("row index");
            let bytes = unbounded_status(rows.clone(), message_line);
            let decoded =
                postcard::from_bytes::<super::StatusLine>(&bytes).expect("bounded rows decode");
            assert_eq!(decoded.rows, rows);
            assert_eq!(decoded.validate(), Ok(()));
        }
        let blanks = unbounded_status(vec![String::new(); super::MAX_STATUS_ROWS], 4);
        let decoded =
            postcard::from_bytes::<super::StatusLine>(&blanks).expect("blank rows decode");
        assert_eq!(decoded.rows.len(), super::MAX_STATUS_ROWS);
        assert_eq!(decoded.validate(), Ok(()));

        let sixth = unbounded_status(vec![String::new(); super::MAX_STATUS_ROWS + 1], 0);
        assert!(postcard::from_bytes::<super::StatusLine>(&sixth).is_err());
        let exact = unbounded_status(vec!["x".repeat(super::MAX_STATUS_TEXT_BYTES)], 0);
        assert!(postcard::from_bytes::<super::StatusLine>(&exact).is_ok());
        let oversized = unbounded_status(vec!["x".repeat(super::MAX_STATUS_TEXT_BYTES + 1)], 0);
        assert!(postcard::from_bytes::<super::StatusLine>(&oversized).is_err());
    }

    #[test]
    fn status_line_validation_enforces_rows_style_and_message_line_rules() {
        let mut status = super::StatusLine::default();
        assert_eq!(status.validate(), Ok(()));
        status.title = "zz".to_owned();
        assert_eq!(status.validate(), Ok(()));
        status.message_line = 1;
        assert!(status.validate().is_err());
        status.rows = vec![String::new(), "#[fg=red]row".to_owned()];
        assert_eq!(status.validate(), Ok(()));
        status.message_line = 2;
        assert!(status.validate().is_err());
        status.message_line = 0;
        status.base_style = "bg=blue,fg=white".to_owned();
        assert_eq!(status.validate(), Ok(()));
        status.base_style = "fg=nope".to_owned();
        assert!(status.validate().is_err());
        status.base_style = String::new();
        status.title = "x".repeat(super::MAX_STATUS_TEXT_BYTES + 1);
        assert!(status.validate().is_err());
        status.title = String::new();
        status.rows = vec![String::new(); super::MAX_STATUS_ROWS + 1];
        status.message_line = 0;
        assert!(status.validate().is_err());
    }

    #[test]
    fn pane_indicator_labels_are_bounded_at_one_kibibyte() {
        let indicator = super::PaneIndicator {
            pane: crate::PaneId(3),
            index: 1,
            select_key: b'1',
            flags: super::PaneIndicator::ACTIVE,
            label: "x".repeat(super::MAX_PANE_INDICATOR_LABEL_BYTES),
        };
        let bytes = postcard::to_stdvec(&indicator).expect("indicator");
        assert_eq!(
            postcard::from_bytes::<super::PaneIndicator>(&bytes).expect("indicator decodes"),
            indicator
        );
        assert!(indicator.active());
        assert_eq!(indicator.selection_key(), Some('1'));

        #[derive(Serialize)]
        struct UnboundedPaneIndicator {
            pane: crate::PaneId,
            index: u32,
            select_key: u8,
            flags: u8,
            label: String,
        }
        let oversized = postcard::to_stdvec(&UnboundedPaneIndicator {
            pane: crate::PaneId(3),
            index: 1,
            select_key: 0,
            flags: 0,
            label: "x".repeat(super::MAX_PANE_INDICATOR_LABEL_BYTES + 1),
        })
        .expect("oversized shape");
        assert!(postcard::from_bytes::<super::PaneIndicator>(&oversized).is_err());
    }

    #[test]
    fn chooser_row_keys_are_bounded_at_sixty_four_bytes() {
        let tree = super::ChooseTreeItem {
            label: "dev".to_owned(),
            detail: "2 windows".to_owned(),
            target: super::ChooseTreeTarget::Session(crate::SessionId(2)),
            depth: 0,
            flags: 0,
            pane_kind: None,
            key: "M".repeat(super::MAX_CHOOSE_ITEM_KEY_BYTES),
        };
        let bytes = postcard::to_stdvec(&tree).expect("tree item");
        assert_eq!(
            postcard::from_bytes::<super::ChooseTreeItem>(&bytes).expect("tree item decodes"),
            tree
        );

        let buffer = super::ChooseBufferItem {
            name: "buffer0001".to_owned(),
            preview: "hello".to_owned(),
            size_bytes: 5,
            created_unix_seconds: 42,
            key: "0".to_owned(),
        };
        let bytes = postcard::to_stdvec(&buffer).expect("buffer item");
        assert_eq!(
            postcard::from_bytes::<super::ChooseBufferItem>(&bytes).expect("buffer item decodes"),
            buffer
        );

        #[derive(Serialize)]
        struct UnboundedBufferItem {
            name: String,
            preview: String,
            size_bytes: u64,
            created_unix_seconds: u64,
            key: String,
        }
        let oversized = postcard::to_stdvec(&UnboundedBufferItem {
            name: String::new(),
            preview: String::new(),
            size_bytes: 0,
            created_unix_seconds: 0,
            key: "k".repeat(super::MAX_CHOOSE_ITEM_KEY_BYTES + 1),
        })
        .expect("oversized shape");
        assert!(postcard::from_bytes::<super::ChooseBufferItem>(&oversized).is_err());

        #[derive(Serialize)]
        struct UnboundedTreeItem {
            label: String,
            detail: String,
            target: super::ChooseTreeTarget,
            depth: u8,
            flags: u8,
            pane_kind: Option<super::ChooseTreePaneKind>,
            key: String,
        }
        let oversized = postcard::to_stdvec(&UnboundedTreeItem {
            label: String::new(),
            detail: String::new(),
            target: super::ChooseTreeTarget::Session(crate::SessionId(2)),
            depth: 0,
            flags: 0,
            pane_kind: None,
            key: "k".repeat(super::MAX_CHOOSE_ITEM_KEY_BYTES + 1),
        })
        .expect("oversized shape");
        assert!(postcard::from_bytes::<super::ChooseTreeItem>(&oversized).is_err());
    }

    #[test]
    fn command_prompt_appends_type_mode_and_no_freeze() {
        assert_eq!(
            super::CommandPromptType::default(),
            super::CommandPromptType::Command
        );
        assert_eq!(
            super::CommandPromptMode::default(),
            super::CommandPromptMode::Text
        );
        assert_eq!(
            postcard::to_stdvec(&super::CommandPromptType::Search).expect("search"),
            [1]
        );
        assert_eq!(
            postcard::to_stdvec(&super::CommandPromptMode::BackspaceExit).expect("backspace exit"),
            [5]
        );
        assert!(postcard::from_bytes::<super::CommandPromptType>(&[2]).is_err());
        assert!(postcard::from_bytes::<super::CommandPromptMode>(&[6]).is_err());

        let state = super::CommandPromptState {
            prompt: "(search up)".to_owned(),
            input: "needle".to_owned(),
            cursor: 6,
            kind: super::CommandPromptKind::Value,
            history: Vec::new(),
            prompt_type: super::CommandPromptType::Search,
            mode: super::CommandPromptMode::Incremental,
            no_freeze: true,
        };
        let bytes = postcard::to_stdvec(&state).expect("prompt state");
        assert_eq!(
            postcard::from_bytes::<super::CommandPromptState>(&bytes).expect("state decodes"),
            state
        );
    }

    #[test]
    fn command_success_appends_stderr_after_exit_code() {
        #[derive(Serialize)]
        struct LegacySuccess {
            request_id: u64,
            output: String,
            exit_code: u8,
        }

        let success = super::CommandResponse::Success {
            request_id: 7,
            output: "out".to_owned(),
            exit_code: 1,
            stderr: "err".to_owned(),
        };
        let bytes = postcard::to_stdvec(&success).expect("success");
        let mut legacy = vec![0];
        legacy.extend(
            postcard::to_stdvec(&LegacySuccess {
                request_id: 7,
                output: "out".to_owned(),
                exit_code: 1,
            })
            .expect("legacy success"),
        );
        assert!(bytes.starts_with(&legacy));
        assert_eq!(
            postcard::from_bytes::<super::CommandResponse>(&bytes).expect("success decodes"),
            success
        );
    }

    #[test]
    fn timed_message_identity_and_clear_hold_the_appended_wire_tail() {
        let message = super::EventPayload::TimedClientMessage {
            pane: None,
            kind: super::ClientMessageKind::Info,
            text: "hello".to_owned(),
            duration_ms: 750,
            message_id: 9,
        };
        let event = super::Event {
            sequence: 5,
            payload: message,
        };
        let bytes = postcard::to_stdvec(&event).expect("timed message encodes");
        assert_eq!(bytes[1], 34);
        assert_eq!(
            postcard::from_bytes::<super::Event>(&bytes).expect("timed message decodes"),
            event
        );

        let cleared = super::Event {
            sequence: 6,
            payload: super::EventPayload::TimedClientMessageCleared { message_id: 9 },
        };
        let bytes = postcard::to_stdvec(&cleared).expect("cleared encodes");
        assert_eq!(bytes[1], 46);
        assert_eq!(
            postcard::from_bytes::<super::Event>(&bytes).expect("cleared decodes"),
            cleared
        );
    }

    #[test]
    fn control_command_guard_holds_wire_tag_forty_seven_and_round_trips_flags() {
        for flags in [0, 1] {
            let event = super::Event {
                sequence: 7,
                payload: super::EventPayload::ControlCommandGuard {
                    output: "diagnostic\n".to_owned(),
                    error: true,
                    sticky_failure: false,
                    flags,
                },
            };
            let bytes = postcard::to_stdvec(&event).expect("control command guard encodes");
            assert_eq!(bytes[1], 47);
            assert_eq!(bytes.last(), Some(&flags));
            assert_eq!(
                postcard::from_bytes::<super::Event>(&bytes)
                    .expect("control command guard decodes"),
                event
            );
        }
    }

    #[test]
    fn control_source_file_holds_wire_tag_forty_eight_and_round_trips_events() {
        for source_event in [
            super::ControlSourceFileEvent::ReadError("Is a directory: source.conf".to_owned()),
            super::ControlSourceFileEvent::Complete,
        ] {
            let event = super::Event {
                sequence: 8,
                payload: super::EventPayload::ControlSourceFile {
                    event: source_event,
                },
            };
            let bytes = postcard::to_stdvec(&event).expect("control source-file event encodes");
            assert_eq!(bytes[1], 48);
            assert_eq!(
                postcard::from_bytes::<super::Event>(&bytes)
                    .expect("control source-file event decodes"),
                event
            );
        }
    }

    #[test]
    fn startup_config_causes_hold_wire_tag_forty_nine_and_round_trip() {
        let event = super::Event {
            sequence: 9,
            payload: super::EventPayload::StartupConfigCauses {
                causes: vec!["one".to_owned(), "two\ncontinued".to_owned()],
            },
        };
        let bytes = postcard::to_stdvec(&event).expect("startup config causes encode");
        assert_eq!(bytes[1], 49);
        assert_eq!(
            postcard::from_bytes::<super::Event>(&bytes).expect("startup config causes decode"),
            event
        );
    }

    #[test]
    fn control_command_output_holds_wire_tag_fifty() {
        let event = super::Event {
            sequence: 10,
            payload: super::EventPayload::ControlCommandOutput {
                output: "child output\n'exit 3' returned 3".to_owned(),
            },
        };
        let bytes = postcard::to_stdvec(&event).expect("control command output encodes");
        assert_eq!(bytes[1], 50);
        assert_eq!(
            postcard::from_bytes::<super::Event>(&bytes).expect("control command output decodes"),
            event
        );
    }

    #[test]
    fn client_terminal_size_input_holds_wire_tag_seventeen() {
        assert_eq!(super::CLIENT_NESTED_CAPABILITY, "client-nested-v1");
        assert_eq!(super::CLIENT_TTY_CAPABILITY_PREFIX, "client-tty-v1:");
        assert_eq!(super::CLIENT_SIZE_CAPABILITY_PREFIX, "client-size-v1:");
        let input = super::InputMessage::ClientTerminalSize {
            columns: 120,
            rows: 40,
        };
        let bytes = postcard::to_stdvec(&input).expect("client size encodes");
        assert_eq!(bytes[0], 17);
        assert_eq!(
            postcard::from_bytes::<super::InputMessage>(&bytes).expect("client size decodes"),
            input
        );
    }

    #[test]
    fn client_focus_input_holds_wire_tag_eighteen() {
        let input = super::InputMessage::ClientFocus { focused: true };
        let bytes = postcard::to_stdvec(&input).expect("client focus encodes");
        assert_eq!(bytes[0], 18);
        assert_eq!(
            postcard::from_bytes::<super::InputMessage>(&bytes).expect("client focus decodes"),
            input
        );
    }

    #[test]
    fn command_request_carries_the_v74_prepared_bit() {
        let request = |prepared| {
            super::ProtocolMessage::CommandRequest(super::CommandRequest {
                request_id: 7,
                command: super::CommandInvocation::new("list-sessions", std::iter::empty::<&str>()),
                prepared,
            })
        };
        let ordinary = postcard::to_stdvec(&request(false)).expect("ordinary request encodes");
        let prepared = postcard::to_stdvec(&request(true)).expect("prepared request encodes");
        assert_eq!(ordinary.last(), Some(&0));
        assert_eq!(prepared.last(), Some(&1));
        assert_eq!(
            postcard::from_bytes::<super::ProtocolMessage>(&ordinary)
                .expect("ordinary request decodes"),
            request(false)
        );
        assert_eq!(
            postcard::from_bytes::<super::ProtocolMessage>(&prepared)
                .expect("prepared request decodes"),
            request(true)
        );
    }

    #[test]
    fn enter_copy_mode_with_holds_wire_tag_twenty_seven() {
        use zz_terminal::TerminalViewAction;

        assert_eq!(
            postcard::to_stdvec(&TerminalViewAction::EnterCopyMode).expect("enter")[0],
            15
        );
        assert_eq!(
            postcard::to_stdvec(&TerminalViewAction::EnterCopyModeScrollExit).expect("scroll exit")
                [0],
            26
        );
        let action = TerminalViewAction::EnterCopyModeWith {
            scroll_exit: true,
            hide_position: true,
        };
        let bytes = postcard::to_stdvec(&action).expect("composed enter");
        assert_eq!(bytes[0], 27);
        assert_eq!(
            postcard::from_bytes::<TerminalViewAction>(&bytes).expect("composed enter decodes"),
            action
        );
    }

    #[test]
    fn counted_copy_mode_action_holds_wire_tag_twenty_eight_without_recursive_actions() {
        use zz_terminal::{CopyModeAction, TerminalViewAction};

        let action = TerminalViewAction::CopyModeCounted {
            action: CopyModeAction::NextMatchingBracket,
            count: u32::MAX,
        };
        let bytes = postcard::to_stdvec(&action).expect("counted copy action encodes");
        assert_eq!(bytes[0], 28);
        assert_eq!(bytes[1], 45);
        assert_eq!(
            postcard::from_bytes::<TerminalViewAction>(&bytes)
                .expect("counted copy action decodes"),
            action
        );
        assert!(postcard::from_bytes::<CopyModeAction>(&[50, 0, 1]).is_err());
        assert!(postcard::from_bytes::<TerminalViewAction>(&[28, 50, 0, 1]).is_err());
    }

    #[test]
    fn repeated_browser_keys_hold_wire_tag_seven() {
        let command = super::BrowserCommand::SendKeysRepeated {
            keys: vec![super::KeyToken::Literal("x".to_owned())],
            count: u32::MAX,
        };
        let bytes = postcard::to_stdvec(&command).expect("repeated browser keys encode");
        assert_eq!(bytes[0], 7);
        assert_eq!(
            postcard::from_bytes::<super::BrowserCommand>(&bytes)
                .expect("repeated browser keys decode"),
            command
        );
    }

    #[test]
    fn detached_reason_holds_its_appended_wire_field() {
        assert_eq!(super::PROTOCOL_VERSION, 86);
        for (reason, tag) in [
            (super::DetachReason::Requested, 0),
            (super::DetachReason::Evicted, 1),
            (super::DetachReason::SessionDestroyed, 2),
            (super::DetachReason::ServerStopping, 3),
        ] {
            let event = super::Event {
                sequence: 0,
                payload: super::EventPayload::Detached {
                    session: crate::SessionId(1),
                    by: None,
                    reason,
                },
            };
            let bytes = postcard::to_stdvec(&event).expect("detached event encodes");
            assert_eq!(bytes[1], 23);
            assert_eq!(bytes.last(), Some(&tag));
            assert_eq!(
                postcard::from_bytes::<super::Event>(&bytes).expect("detached event decodes"),
                event
            );
        }
    }
}
