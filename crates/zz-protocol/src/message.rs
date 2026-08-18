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
pub const PROTOCOL_VERSION: u16 = 67;
pub const NEW_SESSION_ATTACH_CAPABILITY: &str = "new-session-attach-v1";
pub const SPLIT_RATIO_BASIS: u16 = 10_000;
pub const MAX_COMMAND_PROMPT_BYTES: usize = 64 * 1024;
pub const MAX_CHOOSE_TREE_QUERY_BYTES: usize = 4 * 1024;
pub const MAX_CHOOSE_BUFFER_QUERY_BYTES: usize = 4 * 1024;
/// Longest either half of a rendered status line may be.
pub const MAX_STATUS_TEXT_BYTES: usize = 1024;
/// Longest payload `agent-send` may push into a GUI-owned composer or prompt.
pub const MAX_AGENT_SEND_BYTES: usize = 1024 * 1024;
/// Longest path or human-readable message carried by a GUI request or its reply.
pub const MAX_GUI_TEXT_BYTES: usize = 64 * 1024;
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
}

impl MuxOptionKey {
    pub const ALL: [Self; 14] = [
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

/// One rendered tmux status line, expanded by the daemon. Text, never formats:
/// a `#()` command runs once on the daemon's host, not once per client.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusLine {
    #[serde(deserialize_with = "deserialize_status_text")]
    pub left: String,
    #[serde(deserialize_with = "deserialize_status_text")]
    pub right: String,
}

impl StatusLine {
    /// Whether both halves expanded to nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.right.is_empty()
    }

    /// Ensure a wire payload stays inside [`MAX_STATUS_TEXT_BYTES`].
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.left.len() > MAX_STATUS_TEXT_BYTES || self.right.len() > MAX_STATUS_TEXT_BYTES {
            return Err("status text exceeds the wire byte limit");
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

fn deserialize_bounded_text<'de, D>(deserializer: D, limit: usize) -> Result<String, D::Error>
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl CommandInvocation {
    #[must_use]
    pub fn new(name: impl Into<String>, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            name: name.into(),
            args: args.into_iter().map(Into::into).collect(),
            source: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub request_id: u64,
    pub command: CommandInvocation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandResponse {
    Success {
        request_id: u64,
        output: String,
        exit_code: u8,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandPromptState {
    pub prompt: String,
    pub input: String,
    /// Cursor position measured in Unicode scalar values, not bytes.
    pub cursor: u32,
    pub kind: CommandPromptKind,
    /// Oldest-to-newest bounded history, populated only for command prompts.
    pub history: Vec<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneIndicator {
    pub pane: PaneId,
    pub index: u32,
    /// ASCII key that selects this pane, or zero when the pane has no shortcut.
    pub select_key: u8,
    pub flags: u8,
}

impl PaneIndicator {
    pub const ACTIVE: u8 = 1 << 0;

    #[must_use]
    pub const fn active(self) -> bool {
        self.flags & Self::ACTIVE != 0
    }

    #[must_use]
    pub const fn selection_key(self) -> Option<char> {
        if self.select_key == 0 {
            None
        } else {
            Some(self.select_key as char)
        }
    }
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
    }
}
