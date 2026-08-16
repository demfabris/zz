use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use agent_client_protocol::schema::{
    MaybeUndefined,
    v1::{
        AvailableCommand, AvailableCommandInput, ContentBlock, ContentChunk, ImageContent,
        PermissionOption, PermissionOptionKind, Plan, PlanEntryStatus, SessionConfigKind,
        SessionConfigOption, SessionConfigOptionCategory, SessionModeState, SessionUpdate,
        StopReason, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields, ToolKind,
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gpui::{App, Context, Entity, EventEmitter, Image, ImageFormat, Task};
use zz_daemon::{
    AgentAuthMethod as StreamAuthMethod, AgentPromptOutcome, AgentStreamItem, AgentStreamPayload,
};
use zz_protocol::{
    AgentConnectionPhase, AgentDescriptor, AgentGitSummary, AgentPaneWire, AgentProvider,
    AgentSessionOpKind, MAX_AGENT_AUTH_METHODS, MAX_AGENT_AVAILABLE_COMMANDS,
    MAX_AGENT_CONFIG_CHOICES, MAX_AGENT_CONFIG_OPTIONS, MAX_AGENT_MODES,
    MAX_AGENT_PERMISSION_OPTIONS, MAX_AGENT_QUEUED_PROMPTS, MAX_AGENT_SESSION_DIRECTORIES,
    MAX_AGENT_TOOL_CONTENT_ITEMS, MAX_GUI_TEXT_BYTES, PaneId,
};

use crate::{
    agent::attachment,
    agent::preferences::{AgentPreferenceKind, AgentPreferences},
    agent::sound::AgentPaneStatus,
    config::AgentConfig,
    mux::client::{AgentRequest, MuxClient},
};

const LEGACY_MODE_PREFERENCE_ID: &str = "legacy-session-mode";
const MAX_SESSION_ID_BYTES: usize = 16 * 1024;
const MAX_SESSION_TITLE_BYTES: usize = 4 * 1024;
const MAX_SESSION_TIMESTAMP_BYTES: usize = 256;
const MAX_SESSION_CURSOR_BYTES: usize = 16 * 1024;
/// A skipped update is described by its `sessionUpdate` tag and nothing else:
/// the payload behind it is adapter-supplied and can be megabytes of tool
/// output, so neither it nor the serde error quoting it belongs in a log line.
const MAX_SESSION_UPDATE_TAG_BYTES: usize = 64;
const MAX_DECODE_ERROR_BYTES: usize = 256;
const UNTAGGED_SESSION_UPDATE: &str = "<untagged>";
/// Tool payloads live in the thread for as long as the pane does, so the
/// reducer caps what it keeps: agents happily emit multi-megabyte outputs.
const MAX_TOOL_PAYLOAD_BYTES: usize = 512 * 1024;
const MAX_DIFF_SIDE_BYTES: usize = 1024 * 1024;
const TRUNCATION_MARKER: &str = "… [truncated]";
/// A derived pane title is the opening words of the first prompt: enough to
/// tell agent panes apart in the tree without wrapping the pane header.
const MAX_TITLE_WORDS: usize = 7;
const MAX_TITLE_CHARS: usize = 48;
const LIFECYCLE_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const ENTRY_CHANGE_LOG_CAPACITY: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentConnectionState {
    Starting,
    Restoring,
    Ready,
    Running,
    Cancelling,
    Failed,
    Disconnected,
}

impl AgentConnectionState {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Starting => "STARTING",
            Self::Restoring => "RESTORING",
            Self::Ready => "READY",
            Self::Running => "RUNNING",
            Self::Cancelling => "CANCELLING",
            Self::Failed => "FAILED",
            Self::Disconnected => "OFFLINE",
        }
    }

    pub(crate) const fn accepts_prompt(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub(crate) const fn has_active_turn(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentToolKindModel {
    Read,
    Search,
    Edit,
    Delete,
    Move,
    Execute,
    Fetch,
    Think,
    SwitchMode,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentToolStatusModel {
    Pending,
    Running,
    NeedsApproval,
    Completed,
    Failed,
    Canceled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ToolPayload {
    Diff {
        path: String,
        old: Option<String>,
        new: String,
    },
    Text(String),
    Json(String),
    Terminal(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AgentThreadEntry {
    User {
        id: u64,
        markdown: String,
        /// Images sent with the message, normalized to what went on the wire.
        images: Vec<Arc<Image>>,
    },
    Assistant {
        id: u64,
        markdown: String,
    },
    Reasoning {
        id: u64,
        label: String,
        markdown: String,
        default_expanded: bool,
    },
    Tool {
        id: u64,
        protocol_id: String,
        kind: AgentToolKindModel,
        status: AgentToolStatusModel,
        label: String,
        location: Option<String>,
        input: Option<ToolPayload>,
        output: Vec<ToolPayload>,
        default_expanded: bool,
    },
    Plan {
        id: u64,
        markdown: String,
    },
}

impl AgentThreadEntry {
    pub(crate) const fn id(&self) -> u64 {
        match self {
            Self::User { id, .. }
            | Self::Assistant { id, .. }
            | Self::Reasoning { id, .. }
            | Self::Tool { id, .. }
            | Self::Plan { id, .. } => *id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum StreamRole {
    User,
    Assistant,
    Reasoning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentPermissionOption {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) kind: AgentPermissionKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentPermissionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentPermissionRequest {
    pub(crate) request_id: u64,
    pub(crate) tool_call_id: String,
    pub(crate) title: String,
    pub(crate) options: Vec<AgentPermissionOption>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentAuthMethod {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_hint: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentConfigCategory {
    Mode,
    Model,
    ModelConfig,
    ThoughtLevel,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentConfigChoice {
    pub(crate) value: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentConfigOption {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) category: AgentConfigCategory,
    pub(crate) current_value: String,
    pub(crate) choices: Vec<AgentConfigChoice>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentMode {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "ACP exposes these six independent capabilities"
)]
pub(crate) struct AgentSessionCapabilities {
    pub(crate) load: bool,
    pub(crate) list: bool,
    pub(crate) close: bool,
    pub(crate) delete: bool,
    pub(crate) additional_directories: bool,
    /// A prompt capability rather than a session one, but it arrives in the
    /// same handshake and gates the composer the same way.
    pub(crate) images: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentSessionSummary {
    pub(crate) session_id: String,
    pub(crate) cwd: PathBuf,
    pub(crate) additional_directories: Vec<PathBuf>,
    pub(crate) title: Option<String>,
    pub(crate) updated_at: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentSessionHistoryState {
    pub(crate) sessions: Arc<[AgentSessionSummary]>,
    pub(crate) loading: bool,
    pub(crate) error: Option<Arc<str>>,
    pub(crate) next_cursor: Option<Arc<str>>,
    pub(crate) cwd_filter: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentPaneState {
    pub(crate) provider: AgentProvider,
    pub(crate) connection: AgentConnectionState,
    pub(crate) pending_permissions: Arc<[AgentPermissionRequest]>,
    pub(crate) auth_methods: Arc<[AgentAuthMethod]>,
    pub(crate) error: Option<Arc<str>>,
    pub(crate) agent_name: Option<Arc<str>>,
    pub(crate) cwd: PathBuf,
    pub(crate) session_id: Option<Arc<str>>,
    pub(crate) session_capabilities: AgentSessionCapabilities,
    pub(crate) session_history: AgentSessionHistoryState,
    pub(crate) settings_busy: bool,
    pub(crate) lifecycle_pending: bool,
    pub(crate) mode: Option<Arc<str>>,
    pub(crate) modes: Arc<[AgentMode]>,
    pub(crate) config_options: Arc<[AgentConfigOption]>,
    pub(crate) available_commands: Arc<[AgentCommand]>,
    pub(crate) usage: Option<(u64, u64)>,
    pub(crate) git: Option<AgentGitSummary>,
    /// Text `agent-send` routed here, waiting for the pane's view to fold it
    /// into the composer draft.
    pub(crate) pending_composer: Option<Arc<str>>,
    /// Prompts typed during the live turn, waiting for it to settle.
    pub(crate) queued_prompts: usize,
}

/// The fleet rollup the workspace sidebar renders: how many agents are blocked
/// on a permission, dead, or mid-turn. Each pane lands in exactly one bucket, so
/// the counts partition the fleet.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentAttention {
    pub(crate) waiting: usize,
    pub(crate) failed: usize,
    pub(crate) running: usize,
    /// First pane blocked on a permission, for the rollup to jump to.
    pub(crate) waiting_pane: Option<PaneId>,
    /// First pane whose agent failed, for the rollup to jump to.
    pub(crate) failed_pane: Option<PaneId>,
}

impl AgentAttention {
    /// Nothing to say: every agent is idle or absent.
    pub(crate) fn is_quiet(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug)]
struct AgentThread {
    provider: AgentProvider,
    connection: AgentConnectionState,
    entries: Vec<AgentThreadEntry>,
    entry_revisions: Vec<u64>,
    entry_indices: HashMap<u64, usize>,
    entry_changes: VecDeque<(u64, usize)>,
    entry_change_floor: u64,
    pending_permissions: Arc<[AgentPermissionRequest]>,
    auth_methods: Arc<[AgentAuthMethod]>,
    error: Option<Arc<str>>,
    agent_name: Option<Arc<str>>,
    agent_key: Arc<str>,
    title: Option<Arc<str>>,
    mode: Option<Arc<str>>,
    modes: Arc<[AgentMode]>,
    config_options: Arc<[AgentConfigOption]>,
    available_commands: Arc<[AgentCommand]>,
    usage: Option<(u64, u64)>,
    git: Option<AgentGitSummary>,
    cwd: PathBuf,
    session_id: Option<Arc<str>>,
    session_capabilities: AgentSessionCapabilities,
    session_history: AgentSessionHistoryState,
    settings_busy: bool,
    session_reset: bool,
    preference_reconcile_skips: BTreeSet<(AgentPreferenceKind, String)>,
    next_entry_id: u64,
    next_entry_revision: u64,
    message_entries: BTreeMap<(StreamRole, String), u64>,
    active_stream: Option<(StreamRole, u64)>,
    tool_entries: HashMap<String, u64>,
    structured_tool_outputs: BTreeSet<String>,
    plan_entry: Option<u64>,
    suppress_user_echo: bool,
    /// Set once the session's first prompt named the pane. A title is derived
    /// exactly once per session, so a later rename is never overwritten.
    auto_titled: bool,
    /// How many `session/update`s this build could not read. ACP reserves
    /// unknown `sessionUpdate` values for future variants, so skipping one is
    /// forward compatibility rather than a fault — the running count is what
    /// keeps a silent skip diagnosable. Survives a session reset: it describes
    /// the adapter zz is talking to, not the conversation.
    unknown_updates: u64,
}

impl AgentThread {
    fn new(provider: AgentProvider, cwd: PathBuf, session_id: Option<String>) -> Self {
        Self {
            provider,
            connection: AgentConnectionState::Starting,
            entries: Vec::new(),
            entry_revisions: Vec::new(),
            entry_indices: HashMap::new(),
            entry_changes: VecDeque::new(),
            entry_change_floor: 1,
            pending_permissions: Arc::from([]),
            auth_methods: Arc::from([]),
            error: None,
            agent_name: None,
            agent_key: Arc::from("acp-agent"),
            title: None,
            mode: None,
            modes: Arc::from([]),
            config_options: Arc::from([]),
            available_commands: Arc::from([]),
            usage: None,
            git: None,
            cwd,
            session_id: session_id.map(Arc::from),
            session_capabilities: AgentSessionCapabilities::default(),
            session_history: AgentSessionHistoryState::default(),
            settings_busy: false,
            session_reset: false,
            preference_reconcile_skips: BTreeSet::new(),
            next_entry_id: 1,
            next_entry_revision: 1,
            message_entries: BTreeMap::new(),
            active_stream: None,
            tool_entries: HashMap::new(),
            structured_tool_outputs: BTreeSet::new(),
            plan_entry: None,
            suppress_user_echo: false,
            auto_titled: false,
            unknown_updates: 0,
        }
    }

    fn snapshot(&self) -> AgentPaneState {
        AgentPaneState {
            provider: self.provider,
            connection: self.connection,
            pending_permissions: self.pending_permissions.clone(),
            auth_methods: self.auth_methods.clone(),
            error: self.error.clone(),
            agent_name: self.agent_name.clone(),
            cwd: self.cwd.clone(),
            session_id: self.session_id.clone(),
            session_capabilities: self.session_capabilities,
            session_history: self.session_history.clone(),
            settings_busy: self.settings_busy,
            lifecycle_pending: false,
            mode: self.mode.clone(),
            modes: self.modes.clone(),
            config_options: self.config_options.clone(),
            available_commands: self.available_commands.clone(),
            usage: self.usage,
            git: self.git.clone(),
            pending_composer: None,
            queued_prompts: 0,
        }
    }

    fn reset_for_open(&mut self, restoring: bool) {
        self.session_reset = true;
        self.connection = if restoring {
            AgentConnectionState::Restoring
        } else {
            AgentConnectionState::Starting
        };
        self.entries.clear();
        self.entry_revisions.clear();
        self.entry_indices.clear();
        self.entry_changes.clear();
        self.entry_change_floor = self.next_entry_revision;
        self.pending_permissions = Arc::from([]);
        self.error = None;
        self.title = None;
        self.mode = None;
        self.modes = Arc::from([]);
        self.config_options = Arc::from([]);
        self.available_commands = Arc::from([]);
        self.usage = None;
        self.git = None;
        self.session_history.loading = false;
        self.settings_busy = false;
        self.preference_reconcile_skips.clear();
        self.next_entry_id = 1;
        self.message_entries.clear();
        self.active_stream = None;
        self.tool_entries.clear();
        self.structured_tool_outputs.clear();
        self.plan_entry = None;
        self.suppress_user_echo = false;
        self.auto_titled = false;
    }

    fn set_session_configuration(
        &mut self,
        modes: Option<SessionModeState>,
        config_options: Option<Vec<SessionConfigOption>>,
    ) {
        if let Some(config_options) = config_options {
            self.config_options = config_option_models(config_options).into();
            self.mode = None;
            self.modes = Arc::from([]);
            return;
        }
        if let Some(modes) = modes {
            self.config_options = Arc::from([]);
            self.mode = Some(Arc::from(modes.current_mode_id.0.as_ref()));
            self.modes = modes
                .available_modes
                .into_iter()
                .take(MAX_AGENT_MODES)
                .map(|mode| AgentMode {
                    id: mode.id.0.to_string(),
                    name: mode.name,
                    description: mode.description,
                })
                .collect::<Vec<_>>()
                .into();
        }
    }

    fn allocate_entry_id(&mut self) -> u64 {
        let id = self.next_entry_id;
        self.next_entry_id = self.next_entry_id.saturating_add(1);
        id
    }

    fn allocate_entry_revision(&mut self) -> u64 {
        let revision = self.next_entry_revision;
        self.next_entry_revision = self.next_entry_revision.saturating_add(1);
        revision
    }

    fn push_entry(&mut self, entry: AgentThreadEntry) {
        let revision = self.allocate_entry_revision();
        let index = self.entries.len();
        self.entry_indices.insert(entry.id(), index);
        self.entries.push(entry);
        self.entry_revisions.push(revision);
        self.record_entry_change(revision, index);
    }

    fn entry_index(&self, id: u64) -> Option<usize> {
        self.entry_indices.get(&id).copied()
    }

    fn touch_entry(&mut self, index: usize) {
        let revision = self.allocate_entry_revision();
        if let Some(entry_revision) = self.entry_revisions.get_mut(index) {
            *entry_revision = revision;
            self.record_entry_change(revision, index);
        }
    }

    fn record_entry_change(&mut self, revision: u64, index: usize) {
        if self.entry_changes.len() == ENTRY_CHANGE_LOG_CAPACITY
            && let Some((discarded, _)) = self.entry_changes.pop_front()
        {
            self.entry_change_floor = discarded.saturating_add(1);
        }
        self.entry_changes.push_back((revision, index));
    }

    fn prompt_refusal(&self, has_images: bool) -> Option<Arc<str>> {
        if !self.connection.accepts_prompt() {
            return Some(Arc::from(format!(
                "agent is not ready ({})",
                self.connection.label().to_ascii_lowercase()
            )));
        }
        self.image_refusal(has_images)
    }

    /// Checked before a prompt is queued as well as before it is sent: an agent
    /// that takes no images will not take them a turn later either.
    fn image_refusal(&self, has_images: bool) -> Option<Arc<str>> {
        (has_images && !self.session_capabilities.images)
            .then(|| Arc::<str>::from("this agent does not accept images"))
    }

    fn begin_prompt(&mut self, prompt: String, images: Vec<Arc<Image>>) {
        self.active_stream = None;
        let id = self.allocate_entry_id();
        self.push_entry(AgentThreadEntry::User {
            id,
            markdown: prompt,
            images,
        });
        self.connection = AgentConnectionState::Running;
        self.error = None;
        self.suppress_user_echo = true;
    }

    fn apply_update(&mut self, update: SessionUpdate) {
        self.apply_flat_update(update);
    }

    fn apply_flat_update(&mut self, update: SessionUpdate) {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => {
                self.apply_message_chunk(StreamRole::User, chunk);
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                self.apply_message_chunk(StreamRole::Assistant, chunk);
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                self.append_chunk(StreamRole::Reasoning, chunk);
            }
            SessionUpdate::ToolCall(tool) => {
                self.active_stream = None;
                self.upsert_tool(tool);
            }
            SessionUpdate::ToolCallUpdate(update) => {
                self.active_stream = None;
                self.apply_tool_update(update);
            }
            SessionUpdate::Plan(plan) => {
                self.active_stream = None;
                self.apply_plan(&plan);
            }
            SessionUpdate::CurrentModeUpdate(update) => {
                self.mode = Some(Arc::from(update.current_mode_id.0.as_ref()));
            }
            SessionUpdate::AvailableCommandsUpdate(update) => {
                self.available_commands = update
                    .available_commands
                    .into_iter()
                    .take(MAX_AGENT_AVAILABLE_COMMANDS)
                    .map(agent_command_model)
                    .collect::<Vec<_>>()
                    .into();
            }
            SessionUpdate::ConfigOptionUpdate(update) => {
                self.config_options = config_option_models(update.config_options).into();
            }
            SessionUpdate::SessionInfoUpdate(update) => match update.title {
                MaybeUndefined::Undefined => {}
                MaybeUndefined::Null => self.title = None,
                MaybeUndefined::Value(title) => self.title = Some(Arc::from(title)),
            },
            SessionUpdate::UsageUpdate(update) => {
                self.usage = Some((update.used, update.size));
            }
            _ => {}
        }
    }

    fn apply_message_chunk(&mut self, role: StreamRole, chunk: ContentChunk) {
        if role == StreamRole::User && self.suppress_user_echo {
            return;
        }
        self.append_chunk(role, chunk);
    }

    fn apply_runtime_update(&mut self, update: SessionUpdate) {
        self.apply_update(update);
    }

    fn append_chunk(&mut self, role: StreamRole, chunk: ContentChunk) {
        let (markdown, images) = match (role, &chunk.content) {
            (StreamRole::User, ContentBlock::Image(image)) => match inbound_image(image) {
                Some(image) => (String::new(), vec![image]),
                None => (content_block_markdown(&chunk.content), Vec::new()),
            },
            (StreamRole::User, content) => split_inline_images(&content_block_markdown(content)),
            (_, content) => (content_block_markdown(content), Vec::new()),
        };
        if markdown.is_empty() && images.is_empty() {
            return;
        }
        let message_id = chunk.message_id.map(|message_id| message_id.0.to_string());
        self.append_stream_content(role, message_id, &markdown, images);
    }

    fn append_stream_content(
        &mut self,
        role: StreamRole,
        message_id: Option<String>,
        markdown: &str,
        images: Vec<Arc<Image>>,
    ) {
        if markdown.is_empty() && images.is_empty() {
            return;
        }
        let message_key = message_id.map(|message_id| (role, message_id));
        let entry_id = if let Some(key) = message_key.as_ref() {
            if let Some(id) = self.message_entries.get(key).copied() {
                id
            } else {
                let id = self.push_stream_entry(role);
                self.message_entries.insert(key.clone(), id);
                id
            }
        } else if let Some((active_role, id)) = self.active_stream {
            if active_role == role {
                id
            } else {
                self.push_stream_entry(role)
            }
        } else {
            self.push_stream_entry(role)
        };
        self.active_stream = Some((role, entry_id));
        if let Some(index) = self.entry_index(entry_id) {
            let changed = match &mut self.entries[index] {
                AgentThreadEntry::User {
                    markdown: text,
                    images: attached,
                    ..
                } => {
                    attached.extend(images);
                    text.push_str(markdown);
                    true
                }
                AgentThreadEntry::Assistant { markdown: text, .. }
                | AgentThreadEntry::Reasoning { markdown: text, .. } => {
                    text.push_str(markdown);
                    true
                }
                AgentThreadEntry::Tool { .. } | AgentThreadEntry::Plan { .. } => false,
            };
            if changed {
                self.touch_entry(index);
            }
        }
    }

    fn push_stream_entry(&mut self, role: StreamRole) -> u64 {
        let id = self.allocate_entry_id();
        let entry = match role {
            StreamRole::User => AgentThreadEntry::User {
                id,
                markdown: String::new(),
                images: Vec::new(),
            },
            StreamRole::Assistant => AgentThreadEntry::Assistant {
                id,
                markdown: String::new(),
            },
            StreamRole::Reasoning => AgentThreadEntry::Reasoning {
                id,
                label: "Reasoning".to_owned(),
                markdown: String::new(),
                default_expanded: false,
            },
        };
        self.push_entry(entry);
        id
    }

    fn upsert_tool(&mut self, tool: ToolCall) {
        let protocol_id = tool.tool_call_id.0.to_string();
        if tool.content.is_empty() {
            self.structured_tool_outputs.remove(&protocol_id);
        } else {
            self.structured_tool_outputs.insert(protocol_id.clone());
        }
        let location = tool_location(&tool);
        let input = tool_input(&tool);
        let output = tool_output(&tool);
        if let Some(entry_id) = self.tool_entries.get(&protocol_id).copied()
            && let Some(index) = self.entry_index(entry_id)
            && let AgentThreadEntry::Tool {
                kind,
                status,
                label,
                location: entry_location,
                input: entry_input,
                output: entry_output,
                ..
            } = &mut self.entries[index]
        {
            *kind = map_tool_kind(tool.kind);
            *status = map_tool_status(tool.status);
            *label = tool.title;
            *entry_location = location;
            *entry_input = input;
            *entry_output = output;
            self.touch_entry(index);
            return;
        }
        let id = self.allocate_entry_id();
        self.tool_entries.insert(protocol_id.clone(), id);
        self.push_entry(AgentThreadEntry::Tool {
            id,
            protocol_id: protocol_id.clone(),
            kind: map_tool_kind(tool.kind),
            status: map_tool_status(tool.status),
            label: tool.title,
            location,
            input,
            output,
            default_expanded: matches!(tool.status, ToolCallStatus::Failed),
        });
    }

    fn apply_tool_update(&mut self, update: ToolCallUpdate) {
        let protocol_id = update.tool_call_id.0.to_string();
        let carries_shape = update_carries_tool_shape(&update.fields);
        let Some(entry_id) = self.tool_entries.get(&protocol_id).copied() else {
            if let Ok(tool) = ToolCall::try_from(update) {
                self.upsert_tool(tool);
            }
            return;
        };
        let Some(index) = self.entry_index(entry_id) else {
            return;
        };
        let had_structured_output = self.structured_tool_outputs.contains(&protocol_id);
        let AgentThreadEntry::Tool {
            kind,
            status,
            label,
            location,
            input,
            output,
            ..
        } = &mut self.entries[index]
        else {
            return;
        };
        let mut changed = false;
        if let Some(next) = update.fields.kind
            && reclassifies_tool(*kind, next, carries_shape)
        {
            *kind = map_tool_kind(next);
            changed = true;
        }
        if let Some(next) = update.fields.status {
            *status = map_tool_status(next);
            changed = true;
        }
        if let Some(next) = update.fields.title.filter(|title| !title.trim().is_empty()) {
            *label = next;
            changed = true;
        }
        if let Some(raw_input) = update.fields.raw_input {
            *input = json_payload(&raw_input);
            changed = true;
        }
        if let Some(locations) = update.fields.locations {
            *location = locations.first().map(|location| {
                location.line.map_or_else(
                    || location.path.display().to_string(),
                    |line| format!("{}:{line}", location.path.display()),
                )
            });
            changed = true;
        }
        let raw_output = update
            .fields
            .raw_output
            .and_then(|raw_output| json_payload(&raw_output));
        if let Some(content) = update.fields.content {
            let structured = tool_content_payloads(&content);
            if structured.is_empty() {
                self.structured_tool_outputs.remove(&protocol_id);
            } else {
                self.structured_tool_outputs.insert(protocol_id.clone());
            }
            *output = if structured.is_empty() {
                raw_output.into_iter().collect()
            } else {
                structured
            };
            changed = true;
        } else if let Some(raw_output) = raw_output {
            if !had_structured_output {
                *output = vec![raw_output];
            }
            changed = true;
        }
        if changed {
            self.touch_entry(index);
        }
    }

    fn apply_plan(&mut self, plan: &Plan) {
        let markdown = plan
            .entries
            .iter()
            .map(|entry| {
                let marker = match entry.status {
                    PlanEntryStatus::InProgress => "~",
                    PlanEntryStatus::Completed => "x",
                    _ => " ",
                };
                format!("- [{marker}] {}", entry.content)
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(id) = self.plan_entry
            && let Some(index) = self.entry_index(id)
            && let AgentThreadEntry::Plan {
                markdown: current, ..
            } = &mut self.entries[index]
        {
            if *current != markdown {
                *current = markdown;
                self.touch_entry(index);
            }
            return;
        }
        let id = self.allocate_entry_id();
        self.plan_entry = Some(id);
        self.push_entry(AgentThreadEntry::Plan { id, markdown });
    }

    fn request_permission(
        &mut self,
        request_id: u64,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) {
        let tool_call_id = tool_call.tool_call_id.0.to_string();
        let updated_title = tool_call.fields.title.clone();
        self.apply_tool_update(tool_call);
        let title = updated_title
            .or_else(|| {
                self.tool_entries
                    .get(&tool_call_id)
                    .and_then(|id| self.entry_index(*id))
                    .and_then(|index| self.entries.get(index))
                    .and_then(|entry| match entry {
                        AgentThreadEntry::Tool { label, .. } => Some(label.clone()),
                        _ => None,
                    })
            })
            .unwrap_or_else(|| "Tool approval".to_owned());
        if let Some(entry_id) = self.tool_entries.get(&tool_call_id).copied()
            && let Some(index) = self.entry_index(entry_id)
            && let AgentThreadEntry::Tool { status, .. } = &mut self.entries[index]
            && *status != AgentToolStatusModel::NeedsApproval
        {
            *status = AgentToolStatusModel::NeedsApproval;
            self.touch_entry(index);
        }
        let request = AgentPermissionRequest {
            request_id,
            tool_call_id,
            title,
            options: options
                .into_iter()
                .take(MAX_AGENT_PERMISSION_OPTIONS)
                .map(|option| AgentPermissionOption {
                    id: option.option_id.0.to_string(),
                    name: option.name,
                    kind: map_permission_kind(option.kind),
                })
                .collect(),
        };
        let mut pending_permissions = self.pending_permissions.to_vec();
        if let Some(existing) = pending_permissions
            .iter_mut()
            .find(|pending| pending.request_id == request_id)
        {
            *existing = request;
        } else {
            pending_permissions.push(request);
        }
        self.pending_permissions = pending_permissions.into();
    }

    fn resolve_permission(&mut self, request_id: u64, canceled: bool) {
        let Some(index) = self
            .pending_permissions
            .iter()
            .position(|permission| permission.request_id == request_id)
        else {
            return;
        };
        let mut pending_permissions = self.pending_permissions.to_vec();
        let permission = pending_permissions.remove(index);
        self.pending_permissions = pending_permissions.into();
        if let Some(entry_id) = self.tool_entries.get(&permission.tool_call_id).copied()
            && let Some(index) = self.entry_index(entry_id)
            && let AgentThreadEntry::Tool { status, .. } = &mut self.entries[index]
        {
            let next = if canceled {
                AgentToolStatusModel::Canceled
            } else {
                AgentToolStatusModel::Pending
            };
            if *status != next {
                *status = next;
                self.touch_entry(index);
            }
        }
    }

    fn cancel_inflight(&mut self) {
        self.settle_inflight(AgentToolStatusModel::Canceled);
    }

    fn finish_turn(&mut self) {
        self.pending_permissions = Arc::from([]);
        self.suppress_user_echo = false;
        self.active_stream = None;
    }

    fn settle_inflight(&mut self, settled_status: AgentToolStatusModel) {
        debug_assert!(matches!(
            settled_status,
            AgentToolStatusModel::Failed | AgentToolStatusModel::Canceled
        ));
        self.finish_turn();
        for index in 0..self.entries.len() {
            let changed = if let AgentThreadEntry::Tool { status, .. } = &mut self.entries[index] {
                if matches!(
                    status,
                    AgentToolStatusModel::Pending
                        | AgentToolStatusModel::Running
                        | AgentToolStatusModel::NeedsApproval
                ) {
                    *status = settled_status;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if changed {
                self.touch_entry(index);
            }
        }
    }

    fn fail_inflight(&mut self) {
        self.settle_inflight(AgentToolStatusModel::Failed);
    }

    fn finish_replay(&mut self) {
        self.message_entries.clear();
        self.active_stream = None;
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AgentControllerEvent {
    Provider {
        pane: PaneId,
        provider: AgentProvider,
    },
    Restart {
        pane: PaneId,
    },
    Title {
        pane: PaneId,
        title: Arc<str>,
    },
}

#[derive(Debug)]
enum RuntimeEvent {
    Ready {
        agent_name: String,
        agent_key: String,
        auth_methods: Vec<AgentAuthMethod>,
        session_capabilities: AgentSessionCapabilities,
    },
    SessionReset {
        pane: PaneId,
        restoring: bool,
    },
    SessionReady {
        pane: PaneId,
        session_id: String,
        modes: Option<SessionModeState>,
        config_options: Option<Vec<SessionConfigOption>>,
    },
    SessionsListed {
        pane: PaneId,
        sessions: Vec<AgentSessionSummary>,
        next_cursor: Option<String>,
        cwd_filter: Option<PathBuf>,
        replace: bool,
    },
    SessionListFailed {
        pane: PaneId,
        message: String,
    },
    SessionSwitched {
        pane: PaneId,
        session_id: String,
        cwd: PathBuf,
        modes: Option<SessionModeState>,
        config_options: Option<Vec<SessionConfigOption>>,
        replay: Vec<SessionUpdate>,
    },
    SessionSwitchFailed {
        pane: PaneId,
        message: String,
    },
    SessionDeleted {
        pane: PaneId,
        session_id: String,
    },
    SessionDeleteFailed {
        pane: PaneId,
        message: String,
    },
    SessionUpdate {
        pane: PaneId,
        update: SessionUpdate,
    },
    PermissionRequested {
        pane: PaneId,
        request_id: u64,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    },
    PermissionResolved {
        pane: PaneId,
        request_id: u64,
        canceled: bool,
    },
    PromptFinished {
        pane: PaneId,
        result: Result<StopReason, String>,
    },
    Authenticated,
    AuthenticationFailed {
        message: String,
    },
    ConfigOptionsChanged {
        pane: PaneId,
        config_options: Vec<SessionConfigOption>,
        request: AgentSettingRequest,
    },
    ModeChanged {
        pane: PaneId,
        mode_id: String,
        origin: AgentSettingOrigin,
    },
    SettingFailed {
        pane: PaneId,
        message: String,
        option_id: String,
        origin: AgentSettingOrigin,
    },
    PaneFailed {
        pane: PaneId,
        message: String,
    },
    PromptsReclaimed {
        pane: PaneId,
        count: usize,
        text: String,
        images: Vec<Arc<Image>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentSettingOrigin {
    User(Option<AgentPreferenceKind>),
    Preference(AgentPreferenceKind),
}

impl AgentSettingOrigin {
    const fn preference_kind(self) -> Option<AgentPreferenceKind> {
        match self {
            Self::User(kind) => kind,
            Self::Preference(kind) => Some(kind),
        }
    }

    const fn is_user(self) -> bool {
        matches!(self, Self::User(_))
    }
}

#[derive(Clone, Debug)]
struct AgentSettingRequest {
    config_id: String,
    value: String,
    origin: AgentSettingOrigin,
}

#[derive(Default)]
struct PaneViewport {
    last_applied: u64,
    pending_setting: Option<AgentSettingRequest>,
    queued_prompts: usize,
    conversation_epoch: u64,
    last_reclaim_id: u64,
    lifecycle_pending: Option<LifecycleRequest>,
    lifecycle_token: u64,
    pending_provider_state: Option<AgentPaneWire>,
    session_change_pending: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecycleRequest {
    Retry,
    Provider(AgentProvider),
}

pub struct AgentController {
    config: AgentConfig,
    preferences: AgentPreferences,
    mux: Option<Entity<MuxClient>>,
    panes: BTreeMap<PaneId, AgentThread>,
    viewports: BTreeMap<PaneId, PaneViewport>,
    pending_composer: BTreeMap<PaneId, String>,
    pending_images: BTreeMap<PaneId, Vec<Arc<Image>>>,
    retained_panes: BTreeSet<PaneId>,
    shutting_down: bool,
}

impl AgentController {
    #[cfg(test)]
    pub(crate) fn new(config: AgentConfig) -> Self {
        Self::build(config, AgentPreferences::default())
    }

    pub fn with_preferences(config: AgentConfig, preferences: AgentPreferences) -> Self {
        Self::build(config, preferences)
    }

    fn build(config: AgentConfig, preferences: AgentPreferences) -> Self {
        Self {
            config,
            preferences,
            mux: None,
            panes: BTreeMap::new(),
            viewports: BTreeMap::new(),
            pending_composer: BTreeMap::new(),
            pending_images: BTreeMap::new(),
            retained_panes: BTreeSet::new(),
            shutting_down: false,
        }
    }

    pub(crate) fn attach_mux(&mut self, mux: Entity<MuxClient>) {
        self.mux = Some(mux);
    }

    pub(crate) fn pane_state(&self, pane: PaneId) -> Option<AgentPaneState> {
        self.panes.get(&pane).map(|thread| {
            let mut state = thread.snapshot();
            state.pending_composer = self
                .pending_composer
                .get(&pane)
                .map(|text| Arc::from(text.as_str()));
            state.queued_prompts = self.queued_count(pane);
            state.lifecycle_pending = self
                .viewports
                .get(&pane)
                .is_some_and(|viewport| viewport.lifecycle_pending.is_some());
            state
        })
    }

    pub(crate) fn queued_count(&self, pane: PaneId) -> usize {
        self.viewports
            .get(&pane)
            .map_or(0, |viewport| viewport.queued_prompts)
    }

    pub(crate) fn conversation_epoch(&self, pane: PaneId) -> u64 {
        self.viewports
            .get(&pane)
            .map_or(0, |viewport| viewport.conversation_epoch)
    }

    pub(crate) fn pane_status(&self, pane: PaneId) -> Option<AgentPaneStatus> {
        let thread = self.panes.get(&pane)?;
        Some(if !thread.pending_permissions.is_empty() {
            AgentPaneStatus::NeedsInput
        } else if thread.connection == AgentConnectionState::Failed {
            AgentPaneStatus::Failed
        } else if thread.connection.has_active_turn() {
            AgentPaneStatus::Working
        } else {
            AgentPaneStatus::Idle
        })
    }

    /// Fold every pane's thread into the sidebar's [`AgentAttention`] rollup.
    pub(crate) fn attention(&self) -> AgentAttention {
        let mut attention = AgentAttention::default();
        for (&pane, thread) in &self.panes {
            if !thread.pending_permissions.is_empty() {
                attention.waiting += 1;
                attention.waiting_pane.get_or_insert(pane);
            } else if thread.connection == AgentConnectionState::Failed {
                attention.failed += 1;
                attention.failed_pane.get_or_insert(pane);
            } else if thread.connection.has_active_turn() {
                attention.running += 1;
            }
        }
        attention
    }

    pub(crate) fn pane_entries(&self, pane: PaneId) -> Option<(&[AgentThreadEntry], &[u64], u64)> {
        self.panes.get(&pane).map(|thread| {
            (
                thread.entries.as_slice(),
                thread.entry_revisions.as_slice(),
                thread.next_entry_revision,
            )
        })
    }

    pub(crate) fn pane_entry_changes(&self, pane: PaneId, since: u64) -> Option<Vec<usize>> {
        let thread = self.panes.get(&pane)?;
        if since < thread.entry_change_floor {
            return None;
        }
        let mut changes = thread
            .entry_changes
            .iter()
            .filter_map(|(revision, index)| (*revision >= since).then_some(*index))
            .collect::<Vec<_>>();
        changes.sort_unstable();
        changes.dedup();
        Some(changes)
    }

    pub(crate) fn registered_panes(&self) -> BTreeSet<PaneId> {
        self.panes.keys().copied().collect()
    }

    pub(crate) fn ensure_pane(
        &mut self,
        pane: PaneId,
        descriptor: &AgentDescriptor,
        cx: &mut Context<Self>,
    ) {
        let configured_cwd = self.config.working_directory.clone();
        let cwd = descriptor
            .cwd
            .clone()
            .or(configured_cwd)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        let session_id = descriptor.session_id.clone();
        let provider_changed = self
            .panes
            .get(&pane)
            .is_some_and(|thread| thread.provider != descriptor.provider);
        let descriptor_changed = provider_changed;
        let thread = self.panes.entry(pane).or_insert_with(|| {
            AgentThread::new(descriptor.provider, cwd.clone(), session_id.clone())
        });
        if provider_changed {
            *thread = AgentThread::new(descriptor.provider, cwd.clone(), session_id.clone());
        }
        self.retained_panes.insert(pane);
        let viewport = self.viewports.entry(pane).or_default();
        let mut pending_provider_state = None;
        if provider_changed {
            viewport.conversation_epoch = viewport.conversation_epoch.saturating_add(1);
            viewport.lifecycle_pending = None;
            viewport.session_change_pending = false;
            pending_provider_state = viewport.pending_provider_state.take();
        }
        if descriptor_changed {
            cx.notify();
        }
        if let Some(state) = pending_provider_state {
            self.apply_pane_state(pane, &state, cx);
        }
    }

    pub(crate) fn retain_panes(&mut self, retained: &BTreeSet<PaneId>, cx: &mut Context<Self>) {
        let removed = self
            .retained_panes
            .difference(retained)
            .copied()
            .collect::<Vec<_>>();
        for pane in removed {
            self.panes.remove(&pane);
        }
        self.viewports.retain(|pane, _| retained.contains(pane));
        self.pending_composer
            .retain(|pane, _| retained.contains(pane));
        self.pending_images
            .retain(|pane, _| retained.contains(pane));
        let changed = self.retained_panes != *retained;
        self.retained_panes.clone_from(retained);
        if changed {
            cx.notify();
        }
    }

    pub(crate) fn synchronize_config(&mut self, config: AgentConfig, cx: &mut Context<Self>) {
        if self.config == config {
            return;
        }
        self.config = config;
        cx.notify();
    }

    /// Switch the pane's agent. The descriptor is daemon-authoritative, so this
    /// only reports the choice; the daemon reopens the adapter and the pane's
    /// stream starts over.
    pub(crate) fn select_provider(
        &mut self,
        pane: PaneId,
        provider: AgentProvider,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let Some(thread) = self.panes.get(&pane) else {
            return Err(Arc::from("agent pane is not registered"));
        };
        if thread.provider == provider {
            return Ok(());
        }
        if thread.connection.has_active_turn() {
            return Err(Arc::from(
                "finish or cancel the current turn before switching agents",
            ));
        }
        if !self.begin_lifecycle_request(pane, LifecycleRequest::Provider(provider), cx) {
            return Ok(());
        }
        cx.emit(AgentControllerEvent::Provider { pane, provider });
        cx.notify();
        Ok(())
    }

    pub(crate) fn list_sessions(
        &mut self,
        pane: PaneId,
        all_projects: bool,
        append: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let (cwd, cursor, replace) = {
            let Some(thread) = self.panes.get(&pane) else {
                return Err(Arc::from("agent pane is not registered"));
            };
            if !thread.session_capabilities.list {
                return Err(Arc::from("this agent does not support session history"));
            }
            if thread.session_history.loading {
                return Ok(());
            }
            let cursor = if append {
                let Some(cursor) = thread.session_history.next_cursor.as_deref() else {
                    return Ok(());
                };
                Some(cursor.to_owned())
            } else {
                None
            };
            ((!all_projects).then(|| thread.cwd.clone()), cursor, !append)
        };
        if !self.send(
            pane,
            AgentRequest::SessionOp {
                op: AgentSessionOpKind::List {
                    cwd: cwd.clone(),
                    cursor,
                    replace,
                },
            },
            cx,
        ) {
            return Err(Arc::from("agent daemon is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.session_history.loading = true;
            thread.session_history.error = None;
            if replace {
                thread.session_history.sessions = Arc::from([]);
            }
            thread.session_history.cwd_filter = cwd;
            if replace {
                thread.session_history.next_cursor = None;
            }
        }
        cx.notify();
        Ok(())
    }

    pub(crate) fn switch_session(
        &mut self,
        pane: PaneId,
        session: AgentSessionSummary,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        if !valid_session_summary(&session) {
            return Err(Arc::from(
                "the selected session has invalid restore metadata",
            ));
        }
        let Some(thread) = self.panes.get(&pane) else {
            return Err(Arc::from("agent pane is not registered"));
        };
        if !thread.session_capabilities.load {
            return Err(Arc::from("this agent cannot load session history"));
        }
        if !thread.connection.accepts_prompt() || !thread.pending_permissions.is_empty() {
            return Err(Arc::from(
                "finish or cancel the current turn before switching sessions",
            ));
        }
        if thread.session_id.as_deref() == Some(session.session_id.as_str()) {
            return Ok(());
        }
        if self.panes.iter().any(|(other_pane, other)| {
            *other_pane != pane
                && other.provider == thread.provider
                && other.session_id.as_deref() == Some(session.session_id.as_str())
        }) {
            return Err(Arc::from(
                "that session is already open in another agent pane",
            ));
        }
        if !self.send(
            pane,
            AgentRequest::SessionOp {
                op: AgentSessionOpKind::Switch {
                    session_id: session.session_id,
                    cwd: session.cwd,
                    additional_directories: session.additional_directories,
                },
            },
            cx,
        ) {
            return Err(Arc::from("agent daemon is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.connection = AgentConnectionState::Restoring;
            thread.error = None;
        }
        self.viewport_mut(pane).session_change_pending = true;
        cx.notify();
        Ok(())
    }

    pub(crate) fn new_session(
        &mut self,
        pane: PaneId,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let Some(thread) = self.panes.get(&pane) else {
            return Err(Arc::from("agent pane is not registered"));
        };
        if !thread.connection.accepts_prompt() || !thread.pending_permissions.is_empty() {
            return Err(Arc::from(
                "finish or cancel the current turn before starting a new session",
            ));
        }
        let cwd = thread.cwd.clone();
        if !self.send(
            pane,
            AgentRequest::SessionOp {
                op: AgentSessionOpKind::New { cwd },
            },
            cx,
        ) {
            return Err(Arc::from("agent daemon is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.connection = AgentConnectionState::Starting;
            thread.error = None;
        }
        self.viewport_mut(pane).session_change_pending = true;
        cx.notify();
        Ok(())
    }

    pub(crate) fn set_working_directory(
        &mut self,
        pane: PaneId,
        cwd: &Path,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let Some(thread) = self.panes.get(&pane) else {
            return Err(Arc::from("agent pane is not registered"));
        };
        if !cwd.is_absolute() || !valid_session_directory(cwd) {
            return Err(Arc::from(
                "the working directory must be an absolute path within the wire limit",
            ));
        }
        if thread.cwd == cwd {
            return Ok(());
        }
        if !thread.connection.accepts_prompt() || !thread.pending_permissions.is_empty() {
            return Err(Arc::from(
                "finish or cancel the current turn before changing workspaces",
            ));
        }
        if !self.send(
            pane,
            AgentRequest::SessionOp {
                op: AgentSessionOpKind::New {
                    cwd: cwd.to_path_buf(),
                },
            },
            cx,
        ) {
            return Err(Arc::from("agent daemon is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.connection = AgentConnectionState::Starting;
            thread.error = None;
        }
        self.viewport_mut(pane).session_change_pending = true;
        cx.notify();
        Ok(())
    }

    pub(crate) fn delete_session(
        &mut self,
        pane: PaneId,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let Some(thread) = self.panes.get(&pane) else {
            return Err(Arc::from("agent pane is not registered"));
        };
        if !thread.session_capabilities.delete {
            return Err(Arc::from("this agent does not support deleting sessions"));
        }
        if !valid_session_id(session_id) {
            return Err(Arc::from("the selected session ID is invalid"));
        }
        if self.panes.values().any(|candidate| {
            candidate.provider == thread.provider
                && candidate.session_id.as_deref() == Some(session_id)
        }) {
            return Err(Arc::from(
                "a session that is open in an agent pane cannot be deleted",
            ));
        }
        if !self.send(
            pane,
            AgentRequest::SessionOp {
                op: AgentSessionOpKind::Delete {
                    session_id: session_id.to_owned(),
                },
            },
            cx,
        ) {
            return Err(Arc::from("agent daemon is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.session_history.loading = true;
            thread.session_history.error = None;
        }
        cx.notify();
        Ok(())
    }

    /// Send the prompt. The daemon queues it behind a turn already running and
    /// hands it back through the stream if that turn is stopped, so the
    /// at-least-once rule holds without a client-side queue.
    pub(crate) fn prompt(
        &mut self,
        pane: PaneId,
        text: &str,
        images: Vec<Arc<Image>>,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let text = text.trim().to_owned();
        if text.is_empty() && images.is_empty() {
            return Ok(());
        }
        let Some(thread) = self.panes.get(&pane) else {
            return Err(Arc::from("agent pane is not registered"));
        };
        if let Some(refusal) = thread.image_refusal(!images.is_empty()) {
            return Err(refusal);
        }
        let queueing = thread.connection.has_active_turn();
        if !queueing && let Some(refusal) = thread.prompt_refusal(!images.is_empty()) {
            return Err(refusal);
        }
        if queueing && self.queued_count(pane) >= MAX_AGENT_QUEUED_PROMPTS {
            return Err(Arc::from(
                "finish or unqueue one of the four queued prompts first",
            ));
        }
        let wire_images = attachment::wire_images(&images);
        if !self.send(
            pane,
            AgentRequest::Prompt {
                text: text.clone(),
                images: wire_images,
            },
            cx,
        ) {
            if let Some(thread) = self.panes.get_mut(&pane) {
                thread.connection = AgentConnectionState::Failed;
                thread.error = Some(Arc::from("agent daemon is not connected"));
            }
            cx.notify();
            return Err(Arc::from("agent daemon is not connected"));
        }
        if queueing {
            // The daemon publishes the queue depth; showing it immediately keeps
            // the composer's Queue affordance honest between publications.
            self.viewport_mut(pane).queued_prompts += 1;
            cx.notify();
            return Ok(());
        }
        let title = derive_pane_title(&text);
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.begin_prompt(text, images);
        }
        self.name_pane_after_prompt(pane, title, cx);
        cx.notify();
        Ok(())
    }

    fn viewport_mut(&mut self, pane: PaneId) -> &mut PaneViewport {
        self.viewports.entry(pane).or_default()
    }

    /// Name the pane after the session's opening prompt. The mux owns pane
    /// titles, so this only asks for the rename; anything already named — by
    /// the agent, by an earlier prompt, or by the user — keeps its name.
    fn name_pane_after_prompt(
        &mut self,
        pane: PaneId,
        title: Option<Arc<str>>,
        cx: &mut Context<Self>,
    ) {
        let Some(title) = title else {
            return;
        };
        let Some(thread) = self.panes.get_mut(&pane) else {
            return;
        };
        if thread.auto_titled || thread.title.is_some() {
            return;
        }
        thread.auto_titled = true;
        thread.title = Some(title.clone());
        cx.emit(AgentControllerEvent::Title { pane, title });
    }

    /// Empty the pane's queue back into its composer draft on the user's ask.
    /// The prompts themselves come back through the stream.
    pub(crate) fn unqueue_prompts(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if self.queued_count(pane) == 0 {
            return;
        }
        self.send(pane, AgentRequest::Unqueue, cx);
    }

    /// Queue `agent-send` text for the pane's composer draft. The view folds it
    /// in when it next renders; repeated sends stack with a newline between.
    pub(crate) fn append_composer(&mut self, pane: PaneId, text: &str, cx: &mut Context<Self>) {
        if !self.queue_composer_text(pane, text) {
            return;
        }
        cx.notify();
    }

    fn queue_composer_text(&mut self, pane: PaneId, text: &str) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let pending = self.pending_composer.entry(pane).or_default();
        if !pending.is_empty() {
            pending.push('\n');
        }
        pending.push_str(text);
        true
    }

    /// Hand the queued composer text to the pane's view, which owns the draft.
    pub(crate) fn take_pending_composer(&mut self, pane: PaneId) -> Option<String> {
        self.pending_composer
            .remove(&pane)
            .filter(|text| !text.is_empty())
    }

    /// Hand back the images a reclaimed prompt carried, for the view to put in
    /// its attachment strip again.
    pub(crate) fn take_pending_images(&mut self, pane: PaneId) -> Vec<Arc<Image>> {
        self.pending_images.remove(&pane).unwrap_or_default()
    }

    pub(crate) fn cancel(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let Some(thread) = self.panes.get(&pane) else {
            return;
        };
        if !thread.connection.has_active_turn() {
            return;
        }
        if self.send(pane, AgentRequest::Cancel, cx) {
            self.panes
                .get_mut(&pane)
                .expect("pane checked above")
                .connection = AgentConnectionState::Cancelling;
        } else if let Some(thread) = self.panes.get_mut(&pane) {
            thread.error = Some(Arc::from("agent daemon is not connected"));
        }
        cx.notify();
    }

    pub(crate) fn respond_permission(
        &mut self,
        pane: PaneId,
        request_id: u64,
        option_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let canceled = option_id.is_none();
        if self.send(
            pane,
            AgentRequest::RespondPermission {
                request_id,
                option_id,
            },
            cx,
        ) {
            if let Some(thread) = self.panes.get_mut(&pane) {
                thread.resolve_permission(request_id, canceled);
            }
            cx.notify();
            return true;
        }
        false
    }

    pub(crate) fn authenticate(&mut self, pane: PaneId, method_id: String, cx: &mut Context<Self>) {
        if self.send(pane, AgentRequest::Authenticate { method_id }, cx) {
            if let Some(thread) = self.panes.get_mut(&pane) {
                thread.connection = AgentConnectionState::Starting;
                thread.error = None;
            }
            cx.notify();
        }
    }

    pub(crate) fn set_config_option(
        &mut self,
        pane: PaneId,
        config_id: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let selection = self.panes.get(&pane).and_then(|thread| {
            (!thread.settings_busy)
                .then_some(thread)
                .and_then(|thread| {
                    thread.config_options.iter().find(|option| {
                        option.id == config_id
                            && option.choices.iter().any(|choice| choice.value == value)
                    })
                })
                .map(|option| preference_kind_for_category(option.category))
        });
        let Some(preference_kind) = selection else {
            return Err(Arc::from("the agent no longer advertises that setting"));
        };
        let request = AgentSettingRequest {
            config_id: config_id.to_owned(),
            value: value.to_owned(),
            origin: AgentSettingOrigin::User(preference_kind),
        };
        if !self.dispatch_setting(pane, request, cx) {
            return Err(Arc::from("agent daemon is not connected"));
        }
        cx.notify();
        Ok(())
    }

    pub(crate) fn set_mode(
        &mut self,
        pane: PaneId,
        mode_id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let valid = self.panes.get(&pane).is_some_and(|thread| {
            !thread.settings_busy
                && !thread
                    .config_options
                    .iter()
                    .any(|option| option.category == AgentConfigCategory::Mode)
                && thread.modes.iter().any(|mode| mode.id == mode_id)
        });
        if !valid {
            return Err(Arc::from(
                "the agent no longer advertises that permission mode",
            ));
        }
        let request = AgentSettingRequest {
            config_id: LEGACY_MODE_PREFERENCE_ID.to_owned(),
            value: mode_id.to_owned(),
            origin: AgentSettingOrigin::User(Some(AgentPreferenceKind::Permission)),
        };
        if !self.dispatch_setting(pane, request, cx) {
            return Err(Arc::from("agent daemon is not connected"));
        }
        cx.notify();
        Ok(())
    }

    /// Send one setting change and remember who asked for it. A mode change
    /// rides the same slot: only one setting is ever in flight per pane, which
    /// is what `settings_busy` has always meant.
    fn dispatch_setting(&mut self, pane: PaneId, request: AgentSettingRequest, cx: &App) -> bool {
        let wire = if request.config_id == LEGACY_MODE_PREFERENCE_ID {
            AgentRequest::SetMode {
                mode_id: request.value.clone(),
            }
        } else {
            AgentRequest::SetConfigOption {
                option_id: request.config_id.clone(),
                value: request.value.clone(),
            }
        };
        if !self.send(pane, wire, cx) {
            return false;
        }
        self.viewport_mut(pane).pending_setting = Some(request);
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.error = None;
            thread.settings_busy = true;
        }
        true
    }

    /// The origin behind the setting the daemon just acknowledged. An
    /// acknowledgement the client did not ask for — another attached client
    /// changed it — reads as a user pick with nothing sticky behind it.
    fn take_setting_request(&mut self, pane: PaneId, option_id: &str) -> AgentSettingRequest {
        self.viewports
            .get_mut(&pane)
            .and_then(|viewport| viewport.pending_setting.take())
            .filter(|request| request.config_id == option_id)
            .unwrap_or_else(|| AgentSettingRequest {
                config_id: option_id.to_owned(),
                value: String::new(),
                origin: AgentSettingOrigin::User(None),
            })
    }

    fn reconcile_preferences(&mut self, pane: PaneId, cx: &App) {
        let Some(thread) = self.panes.get(&pane) else {
            return;
        };
        if thread.settings_busy || !thread.connection.accepts_prompt() {
            return;
        }
        if let Some(request) = preferred_setting_command(thread, &self.preferences) {
            self.dispatch_setting(pane, request, cx);
        }
    }

    pub(crate) fn retry(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if !self.panes.contains_key(&pane) {
            return;
        }
        if !self.begin_lifecycle_request(pane, LifecycleRequest::Retry, cx) {
            return;
        }
        let thread = self.panes.get_mut(&pane).expect("pane checked above");
        thread.session_history.loading = false;
        thread.settings_busy = false;
        let viewport = self.viewport_mut(pane);
        viewport.pending_setting = None;
        viewport.session_change_pending = false;
        cx.emit(AgentControllerEvent::Restart { pane });
        cx.notify();
    }

    fn begin_lifecycle_request(
        &mut self,
        pane: PaneId,
        request: LifecycleRequest,
        cx: &mut Context<Self>,
    ) -> bool {
        let viewport = self.viewport_mut(pane);
        if viewport.lifecycle_pending.is_some() {
            return false;
        }
        viewport.lifecycle_pending = Some(request);
        viewport.lifecycle_token = viewport.lifecycle_token.saturating_add(1);
        let token = viewport.lifecycle_token;
        cx.spawn(async move |controller, cx| {
            cx.background_executor()
                .timer(LIFECYCLE_REQUEST_TIMEOUT)
                .await;
            controller
                .update(cx, |controller, cx| {
                    let viewport = controller.viewport_mut(pane);
                    if viewport.lifecycle_pending.is_some() && viewport.lifecycle_token == token {
                        viewport.lifecycle_pending = None;
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
        true
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    pub(crate) fn is_shutdown_complete(&self) -> bool {
        self.shutting_down
    }

    /// Nothing to wind down: the adapters are the daemon's children and a
    /// running turn is meant to outlive the window.
    pub(crate) fn shutdown(&mut self, _cx: &mut Context<Self>) -> Task<bool> {
        self.shutting_down = true;
        Task::ready(true)
    }

    fn send(&self, pane: PaneId, request: AgentRequest, cx: &App) -> bool {
        self.mux
            .as_ref()
            .is_some_and(|mux| mux.read(cx).send_agent_request(pane, request))
    }

    /// Apply one batch of stream items, in seq order, to the pane's reducer.
    /// A pane's viewport and its transcript are dropped together, so an item at
    /// or below the applied cursor is one this transcript already holds: an
    /// attach clears the mux client's cursor and replays from the top, and the
    /// pane it replays into may be the same one that reduced those items.
    pub(crate) fn apply_stream_items(
        &mut self,
        pane: PaneId,
        items: Vec<AgentStreamItem>,
        cx: &mut Context<Self>,
    ) {
        if !self.panes.contains_key(&pane) {
            return;
        }
        let mut changed = false;
        for item in items {
            if item.seq <= self.viewport_mut(pane).last_applied {
                continue;
            }
            self.viewport_mut(pane).last_applied = item.seq;
            if let AgentStreamPayload::StateSynced { state } = &item.payload {
                self.apply_pane_state(pane, state, cx);
                continue;
            }
            let Some(event) = self.translate_stream_payload(pane, item.payload, cx) else {
                continue;
            };
            changed |= self.handle_runtime_event(pane, event, cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// Adopt the daemon's published pane state: connection phase, queue depth,
    /// and the permission request a late-attaching client has to see.
    pub(crate) fn apply_pane_state(
        &mut self,
        pane: PaneId,
        state: &AgentPaneWire,
        cx: &mut Context<Self>,
    ) {
        let Some(before) = self.pane_state(pane) else {
            return;
        };
        let viewport = self.viewport_mut(pane);
        viewport.queued_prompts = state.queued_prompts as usize;
        if matches!(
            viewport.lifecycle_pending,
            Some(LifecycleRequest::Provider(_))
        ) {
            viewport.pending_provider_state = Some(state.clone());
            return;
        }
        if matches!(viewport.lifecycle_pending, Some(LifecycleRequest::Retry))
            && !matches!(state.phase, AgentConnectionPhase::Failed { .. })
        {
            viewport.lifecycle_pending = None;
        }
        let session_change_pending = viewport.session_change_pending;
        if matches!(state.phase, AgentConnectionPhase::Failed { .. }) {
            viewport.session_change_pending = false;
        }
        let thread = self.panes.get_mut(&pane).expect("pane checked above");
        let was_failed = thread.connection == AgentConnectionState::Failed;
        match &state.phase {
            AgentConnectionPhase::Starting => {
                thread.connection = AgentConnectionState::Starting;
                thread.error = state.error.as_deref().map(Arc::from);
            }
            AgentConnectionPhase::Ready => {
                if matches!(
                    thread.connection,
                    AgentConnectionState::Starting
                        | AgentConnectionState::Failed
                        | AgentConnectionState::Disconnected
                ) && !session_change_pending
                {
                    thread.connection = AgentConnectionState::Ready;
                }
                if was_failed || state.error.is_some() {
                    thread.error = state.error.as_deref().map(Arc::from);
                }
            }
            AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission => {
                if !thread.connection.has_active_turn() {
                    thread.connection = AgentConnectionState::Running;
                }
                thread.error = state.error.as_deref().map(Arc::from);
            }
            AgentConnectionPhase::Failed { message } => {
                thread.connection = AgentConnectionState::Failed;
                thread.error = Some(Arc::from(message.as_str()));
                thread.fail_inflight();
            }
        }
        thread.title = state.title.as_deref().map(Arc::from);
        thread.git.clone_from(&state.git);
        if state.auth_methods.is_empty() {
            thread.auth_methods = Arc::from([]);
        } else if let Some(methods) =
            decode_state_blob::<Vec<StreamAuthMethod>>(&state.auth_methods)
        {
            thread.auth_methods = methods
                .into_iter()
                .take(MAX_AGENT_AUTH_METHODS)
                .map(|method| AgentAuthMethod {
                    id: method.id,
                    name: method.name,
                    description: method.description,
                })
                .collect::<Vec<_>>()
                .into();
        }
        thread.pending_permissions = Arc::from([]);
        if let Some(permission) = &state.pending_permission
            && let Some((tool_call, options)) = decode_permission_payload(&permission.payload)
        {
            thread.request_permission(permission.request_id, tool_call, options);
        }
        if state.config_options.is_empty() && state.modes.is_empty() {
            thread.config_options = Arc::from([]);
            thread.mode = None;
            thread.modes = Arc::from([]);
        } else if !state.config_options.is_empty() {
            if let Some(config_options) =
                decode_state_blob::<Vec<SessionConfigOption>>(&state.config_options)
            {
                thread.set_session_configuration(None, Some(config_options));
            }
        } else if let Some(modes) = decode_state_blob::<SessionModeState>(&state.modes) {
            thread.set_session_configuration(Some(modes), None);
        }
        if self.pane_state(pane).as_ref() != Some(&before) {
            cx.notify();
        }
    }

    /// Reduce the answer to `AgentSessionOp::List`. It arrives pane-wide, so it
    /// carries no request the client has to match.
    pub(crate) fn apply_sessions_result(
        &mut self,
        pane: PaneId,
        result: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(payload) = decode_state_blob::<AgentStreamPayload>(result) else {
            return;
        };
        let Some(event) = self.translate_stream_payload(pane, payload, cx) else {
            return;
        };
        if self.handle_runtime_event(pane, event, cx) {
            cx.notify();
        }
    }

    /// Turn one wire item into the reducer's input. `None` is an item the
    /// reducer has nothing to do with, or one whose payload did not survive
    /// re-typing — the stream carries the ACP SDK's JSON verbatim.
    fn translate_stream_payload(
        &mut self,
        pane: PaneId,
        payload: AgentStreamPayload,
        cx: &App,
    ) -> Option<RuntimeEvent> {
        Some(match payload {
            AgentStreamPayload::Ready {
                agent_name,
                agent_key,
                auth_methods,
                capabilities,
            } => RuntimeEvent::Ready {
                agent_name,
                agent_key,
                auth_methods: auth_methods
                    .into_iter()
                    .take(MAX_AGENT_AUTH_METHODS)
                    .map(|method| AgentAuthMethod {
                        id: method.id,
                        name: method.name,
                        description: method.description,
                    })
                    .collect(),
                session_capabilities: AgentSessionCapabilities {
                    load: capabilities.load,
                    list: capabilities.list,
                    close: capabilities.close,
                    delete: capabilities.delete,
                    additional_directories: capabilities.additional_directories,
                    images: capabilities.images,
                },
            },
            AgentStreamPayload::SessionReset { restoring } => {
                RuntimeEvent::SessionReset { pane, restoring }
            }
            AgentStreamPayload::SessionReady {
                session_id,
                modes,
                config_options,
            } => RuntimeEvent::SessionReady {
                pane,
                session_id,
                modes: decode_json(modes),
                config_options: decode_json(config_options),
            },
            AgentStreamPayload::StateSynced { .. } | AgentStreamPayload::PromptAccepted { .. } => {
                return None;
            }
            AgentStreamPayload::SessionsListed {
                sessions,
                next_cursor,
                cwd_filter,
                replace,
                ..
            } => RuntimeEvent::SessionsListed {
                pane,
                sessions: sessions
                    .into_iter()
                    .map(|session| AgentSessionSummary {
                        session_id: session.session_id,
                        cwd: session.cwd,
                        additional_directories: session.additional_directories,
                        title: session.title,
                        updated_at: session.updated_at,
                    })
                    .collect(),
                next_cursor: next_cursor.filter(|cursor| valid_session_cursor(cursor)),
                cwd_filter,
                replace,
            },
            AgentStreamPayload::SessionListFailed { message, .. } => {
                RuntimeEvent::SessionListFailed { pane, message }
            }
            AgentStreamPayload::SessionSwitched {
                session_id,
                cwd,
                modes,
                config_options,
                replay,
            } => RuntimeEvent::SessionSwitched {
                pane,
                session_id,
                cwd,
                modes: decode_json(modes),
                config_options: decode_json(config_options),
                replay: replay
                    .into_iter()
                    .filter_map(|update| self.decode_session_update(pane, update))
                    .collect(),
            },
            AgentStreamPayload::SessionSwitchFailed { message } => {
                RuntimeEvent::SessionSwitchFailed { pane, message }
            }
            AgentStreamPayload::SessionDeleted { session_id, .. } => {
                RuntimeEvent::SessionDeleted { pane, session_id }
            }
            AgentStreamPayload::SessionDeleteFailed { message, .. } => {
                RuntimeEvent::SessionDeleteFailed { pane, message }
            }
            AgentStreamPayload::Update { update } => RuntimeEvent::SessionUpdate {
                pane,
                update: self.decode_session_update(pane, update)?,
            },
            AgentStreamPayload::PermissionRequested {
                request_id,
                tool_call,
                options,
            } => RuntimeEvent::PermissionRequested {
                pane,
                request_id,
                tool_call: decode_value(tool_call)?,
                options: decode_value(options)?,
            },
            AgentStreamPayload::PermissionResolved {
                request_id,
                canceled,
            } => RuntimeEvent::PermissionResolved {
                pane,
                request_id,
                canceled,
            },
            AgentStreamPayload::PromptFinished { outcome, .. } => RuntimeEvent::PromptFinished {
                pane,
                result: match outcome {
                    AgentPromptOutcome::Finished { stop_reason } => Ok(decode_value(stop_reason)?),
                    AgentPromptOutcome::Failed { message } => Err(message),
                },
            },
            AgentStreamPayload::TurnStarted { .. } => return None,
            AgentStreamPayload::Authenticated => RuntimeEvent::Authenticated,
            AgentStreamPayload::AuthenticationFailed { message } => {
                RuntimeEvent::AuthenticationFailed { message }
            }
            AgentStreamPayload::ConfigOptionsChanged {
                option_id,
                value,
                config_options,
            } => {
                let mut request = self.take_setting_request(pane, &option_id);
                request.value = value;
                RuntimeEvent::ConfigOptionsChanged {
                    pane,
                    config_options: decode_value(config_options)?,
                    request,
                }
            }
            AgentStreamPayload::ModeChanged { mode_id } => {
                let request = self.take_setting_request(pane, LEGACY_MODE_PREFERENCE_ID);
                RuntimeEvent::ModeChanged {
                    pane,
                    mode_id,
                    origin: request.origin,
                }
            }
            AgentStreamPayload::SettingFailed { option_id, message } => {
                let request = self.take_setting_request(pane, &option_id);
                RuntimeEvent::SettingFailed {
                    pane,
                    message,
                    option_id,
                    origin: request.origin,
                }
            }
            AgentStreamPayload::PaneFailed { message } => {
                RuntimeEvent::PaneFailed { pane, message }
            }
            AgentStreamPayload::PromptsReclaimed { prompts } => {
                let count = prompts.len();
                let mut text = String::new();
                let mut images = Vec::new();
                for prompt in prompts {
                    if !prompt.text.trim().is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&prompt.text);
                    }
                    images.extend(prompt.images.iter().filter_map(attachment::inbound_image));
                }
                RuntimeEvent::PromptsReclaimed {
                    pane,
                    count,
                    text,
                    images,
                }
            }
            AgentStreamPayload::PromptsRestored {
                reclaim_id,
                prompts,
            } => {
                let client = self
                    .mux
                    .as_ref()
                    .and_then(|mux| mux.read(cx).client_instance_id());
                let prompts = prompts
                    .into_iter()
                    .filter(|prompt| prompt.owner.0 == 0 || Some(prompt.owner) == client)
                    .collect::<Vec<_>>();
                if prompts.is_empty() {
                    return None;
                }
                {
                    let viewport = self.viewport_mut(pane);
                    if reclaim_id <= viewport.last_reclaim_id {
                        return None;
                    }
                    viewport.last_reclaim_id = reclaim_id;
                }
                _ = self.send(
                    pane,
                    AgentRequest::AcknowledgePromptRestore { reclaim_id },
                    cx,
                );
                let count = prompts.len();
                let mut text = String::new();
                let mut images = Vec::new();
                for prompt in prompts {
                    if !prompt.text.trim().is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&prompt.text);
                    }
                    images.extend(prompt.images.iter().filter_map(attachment::inbound_image));
                }
                RuntimeEvent::PromptsReclaimed {
                    pane,
                    count,
                    text,
                    images,
                }
            }
        })
    }

    /// Re-type one `session/update`, skipping what this build cannot read.
    /// ACP reserves unknown `sessionUpdate` values for future variants and
    /// `_`-prefixed ones for adapter extensions, so a shape zz does not know is
    /// forward compatibility, not a fault: the item is counted on the pane and
    /// dropped on its own, leaving the rest of its batch — and any message it
    /// interrupts mid-coalesce — untouched. Only the discriminant and a clipped
    /// error reach the log; the payload can be megabytes of tool output.
    fn decode_session_update(
        &mut self,
        pane: PaneId,
        update: serde_json::Value,
    ) -> Option<SessionUpdate> {
        let tag = session_update_tag(&update);
        match serde_json::from_value(update) {
            Ok(update) => Some(update),
            Err(error) => {
                let skipped = self.panes.get_mut(&pane).map_or(0, |thread| {
                    thread.unknown_updates = thread.unknown_updates.saturating_add(1);
                    thread.unknown_updates
                });
                log::debug!(
                    target: "zz::agent",
                    "skipping a session update zz cannot read pane={pane} tag={tag} skipped={skipped}: {}",
                    log_excerpt(&error.to_string(), MAX_DECODE_ERROR_BYTES)
                );
                None
            }
        }
    }

    fn handle_runtime_event(
        &mut self,
        runtime_pane: PaneId,
        event: RuntimeEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed_pane = None;
        let mut reconcile_pane = None;
        let mut remembered_preference = None;
        match event {
            RuntimeEvent::Ready {
                agent_name,
                agent_key,
                auth_methods,
                session_capabilities,
            } => {
                if let Some(thread) = self.panes.get_mut(&runtime_pane) {
                    thread.agent_name = Some(Arc::from(agent_name));
                    thread.agent_key = Arc::from(agent_key);
                    thread.auth_methods = auth_methods
                        .into_iter()
                        .take(MAX_AGENT_AUTH_METHODS)
                        .collect::<Vec<_>>()
                        .into();
                    thread.session_capabilities = session_capabilities;
                    changed_pane = Some(runtime_pane);
                }
            }
            RuntimeEvent::SessionReset { pane, restoring } => {
                let viewport = self.viewport_mut(pane);
                viewport.conversation_epoch = viewport.conversation_epoch.saturating_add(1);
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.reset_for_open(restoring);
                    changed_pane = Some(pane);
                }
            }
            RuntimeEvent::SessionReady {
                pane,
                session_id,
                modes,
                config_options,
            } => {
                self.viewport_mut(pane).session_change_pending = false;
                if let Some(thread) = self.panes.get_mut(&pane) {
                    let session_id: Arc<str> = Arc::from(session_id);
                    thread.session_id = Some(session_id.clone());
                    thread.set_session_configuration(modes, config_options);
                    if thread.connection == AgentConnectionState::Restoring {
                        thread.finish_replay();
                    }
                    thread.session_reset = false;
                    thread.finish_turn();
                    thread.connection = AgentConnectionState::Ready;
                    thread.error = None;
                    changed_pane = Some(pane);
                    reconcile_pane = Some(pane);
                }
            }
            RuntimeEvent::SessionsListed {
                pane,
                sessions,
                next_cursor,
                cwd_filter,
                replace,
            } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    let mut next = if replace {
                        Vec::new()
                    } else {
                        thread.session_history.sessions.to_vec()
                    };
                    let mut known = next
                        .iter()
                        .map(|session| session.session_id.clone())
                        .collect::<BTreeSet<_>>();
                    next.extend(
                        sessions
                            .into_iter()
                            .filter(valid_session_summary)
                            .filter(|session| known.insert(session.session_id.clone())),
                    );
                    thread.session_history.sessions = next.into();
                    thread.session_history.loading = false;
                    thread.session_history.error = None;
                    thread.session_history.next_cursor = next_cursor.map(Arc::from);
                    thread.session_history.cwd_filter = cwd_filter;
                    changed_pane = Some(pane);
                }
            }
            RuntimeEvent::SessionListFailed { pane, message }
            | RuntimeEvent::SessionDeleteFailed { pane, message } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.session_history.loading = false;
                    thread.session_history.error = Some(Arc::from(message));
                    changed_pane = Some(pane);
                }
            }
            RuntimeEvent::SessionSwitched {
                pane,
                session_id,
                cwd,
                modes,
                config_options,
                replay,
            } => {
                self.viewport_mut(pane).session_change_pending = false;
                if self
                    .panes
                    .get(&pane)
                    .is_some_and(|thread| !thread.session_reset)
                {
                    let viewport = self.viewport_mut(pane);
                    viewport.conversation_epoch = viewport.conversation_epoch.saturating_add(1);
                }
                if let Some(thread) = self.panes.get_mut(&pane) {
                    let previous_title = thread.title.clone();
                    if !thread.session_reset {
                        thread.reset_for_open(true);
                    }
                    thread.cwd = cwd;
                    for update in replay {
                        thread.apply_update(update);
                    }
                    thread.finish_replay();
                    thread.session_reset = false;
                    thread.finish_turn();
                    let session_id: Arc<str> = Arc::from(session_id);
                    thread.session_id = Some(session_id.clone());
                    thread.set_session_configuration(modes, config_options);
                    thread.connection = AgentConnectionState::Ready;
                    thread.error = None;
                    if thread.title != previous_title {
                        let title = thread.title.clone().unwrap_or_else(|| Arc::from("agent"));
                        cx.emit(AgentControllerEvent::Title { pane, title });
                    }
                    changed_pane = Some(pane);
                    reconcile_pane = Some(pane);
                }
            }
            RuntimeEvent::SessionSwitchFailed { pane, message } => {
                self.viewport_mut(pane).session_change_pending = false;
                if let Some(thread) = self.panes.get_mut(&pane) {
                    if matches!(
                        thread.connection,
                        AgentConnectionState::Starting | AgentConnectionState::Restoring
                    ) {
                        thread.connection = AgentConnectionState::Ready;
                    }
                    if thread.connection != AgentConnectionState::Failed {
                        thread.error = Some(Arc::from(message));
                    }
                    changed_pane = Some(pane);
                }
            }
            RuntimeEvent::SessionDeleted { pane, session_id } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    let sessions = thread
                        .session_history
                        .sessions
                        .iter()
                        .filter(|session| session.session_id != session_id)
                        .cloned()
                        .collect::<Vec<_>>();
                    thread.session_history.sessions = sessions.into();
                    thread.session_history.loading = false;
                    thread.session_history.error = None;
                    changed_pane = Some(pane);
                }
            }
            RuntimeEvent::SessionUpdate { pane, update } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    let configuration_changed = matches!(
                        &update,
                        SessionUpdate::CurrentModeUpdate(_) | SessionUpdate::ConfigOptionUpdate(_)
                    );
                    let previous_title = thread.title.clone();
                    thread.apply_runtime_update(update);
                    if thread.title != previous_title {
                        let title = thread.title.clone().unwrap_or_else(|| Arc::from("agent"));
                        cx.emit(AgentControllerEvent::Title { pane, title });
                    }
                    changed_pane = Some(pane);
                    if configuration_changed {
                        reconcile_pane = Some(pane);
                    }
                }
            }
            RuntimeEvent::PermissionRequested {
                pane,
                request_id,
                tool_call,
                options,
            } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.request_permission(request_id, tool_call, options);
                    changed_pane = Some(pane);
                }
            }
            RuntimeEvent::PermissionResolved {
                pane,
                request_id,
                canceled,
            } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.resolve_permission(request_id, canceled);
                    changed_pane = Some(pane);
                }
            }
            RuntimeEvent::PromptFinished { pane, result } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    match result {
                        Ok(StopReason::Cancelled) => {
                            thread.connection = AgentConnectionState::Ready;
                            thread.error = None;
                            thread.cancel_inflight();
                        }
                        Ok(_) => {
                            thread.connection = AgentConnectionState::Ready;
                            thread.error = None;
                            thread.finish_turn();
                        }
                        Err(error) => {
                            thread.connection = AgentConnectionState::Failed;
                            thread.error = Some(Arc::from(error));
                            thread.fail_inflight();
                        }
                    }
                    changed_pane = Some(pane);
                    if thread.connection.accepts_prompt() {
                        reconcile_pane = Some(pane);
                    }
                }
            }
            RuntimeEvent::Authenticated => {
                if let Some(thread) = self.panes.get_mut(&runtime_pane) {
                    thread.connection = AgentConnectionState::Starting;
                    thread.error = None;
                }
                changed_pane = Some(runtime_pane);
            }
            RuntimeEvent::AuthenticationFailed { message } => {
                if let Some(thread) = self.panes.get_mut(&runtime_pane) {
                    thread.connection = AgentConnectionState::Failed;
                    thread.error = Some(Arc::from(message));
                    changed_pane = Some(runtime_pane);
                }
            }
            RuntimeEvent::ConfigOptionsChanged {
                pane,
                config_options,
                request,
            } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.config_options = config_option_models(config_options).into();
                    thread.settings_busy = false;
                    let applied = thread.config_options.iter().any(|option| {
                        option.id == request.config_id && option.current_value == request.value
                    });
                    let preference_kind = request.origin.preference_kind();
                    if applied {
                        if let Some(kind) = preference_kind {
                            thread
                                .preference_reconcile_skips
                                .remove(&(kind, request.config_id.clone()));
                        }
                        thread.error = None;
                    } else {
                        if let Some(kind) = preference_kind {
                            thread
                                .preference_reconcile_skips
                                .insert((kind, request.config_id.clone()));
                        }
                        if request.origin.is_user() {
                            thread.error = Some(Arc::from(format!(
                                "the agent acknowledged `{}` but did not apply the selected value",
                                request.config_id
                            )));
                        } else {
                            log::warn!(
                                target: "zz::agent",
                                "agent acknowledged sticky setting option={} without applying it",
                                request.config_id
                            );
                        }
                    }
                    if applied
                        && request.origin.is_user()
                        && let Some(kind) = preference_kind
                    {
                        remembered_preference = Some((
                            thread.provider,
                            thread.agent_key.clone(),
                            kind,
                            request.config_id,
                            request.value,
                        ));
                    }
                    changed_pane = Some(pane);
                    reconcile_pane = Some(pane);
                }
            }
            RuntimeEvent::ModeChanged {
                pane,
                mode_id,
                origin,
            } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.mode = Some(Arc::from(mode_id.clone()));
                    thread.settings_busy = false;
                    thread.preference_reconcile_skips.remove(&(
                        AgentPreferenceKind::Permission,
                        LEGACY_MODE_PREFERENCE_ID.to_owned(),
                    ));
                    thread.error = None;
                    if origin.is_user() {
                        remembered_preference = Some((
                            thread.provider,
                            thread.agent_key.clone(),
                            AgentPreferenceKind::Permission,
                            LEGACY_MODE_PREFERENCE_ID.to_owned(),
                            mode_id,
                        ));
                    }
                    changed_pane = Some(pane);
                    reconcile_pane = Some(pane);
                }
            }
            RuntimeEvent::SettingFailed {
                pane,
                message,
                option_id,
                origin,
            } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.settings_busy = false;
                    if origin.is_user() {
                        thread.error = Some(Arc::from(message));
                    } else if let Some(kind) = origin.preference_kind() {
                        thread.preference_reconcile_skips.insert((kind, option_id));
                        log::warn!(target: "zz::agent", "could not restore sticky agent setting: {message}");
                    }
                    changed_pane = Some(pane);
                    reconcile_pane = Some(pane);
                }
            }
            RuntimeEvent::PaneFailed { pane, message } => {
                self.viewport_mut(pane).session_change_pending = false;
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.connection = AgentConnectionState::Failed;
                    thread.error = Some(Arc::from(message));
                    thread.fail_inflight();
                    thread.session_history.loading = false;
                    thread.settings_busy = false;
                    changed_pane = Some(pane);
                }
            }
            RuntimeEvent::PromptsReclaimed {
                pane,
                count,
                text,
                images,
            } => {
                let viewport = self.viewport_mut(pane);
                viewport.queued_prompts = viewport.queued_prompts.saturating_sub(count);
                self.queue_composer_text(pane, &text);
                if !images.is_empty() {
                    self.pending_images.entry(pane).or_default().extend(images);
                }
                changed_pane = Some(pane);
            }
        }
        if let Some((provider, agent_key, kind, option_id, value)) = remembered_preference {
            self.preferences
                .remember(provider, &agent_key, kind, &option_id, &value);
        }
        if let Some(pane) = reconcile_pane {
            self.reconcile_preferences(pane, cx);
        }
        changed_pane.is_some()
    }
}

fn derive_pane_title(prompt: &str) -> Option<Arc<str>> {
    let first_line = prompt.trim().lines().next().unwrap_or_default();
    let cleaned = first_line
        .trim_start_matches(['"', '\'', '#', '>', '`', '*', ' ', '\t'])
        .trim_end_matches(['"', '\'', '`', '*', ' ', '\t']);
    let words = cleaned
        .split_whitespace()
        .take(MAX_TITLE_WORDS)
        .collect::<Vec<_>>()
        .join(" ");
    let title = words.chars().take(MAX_TITLE_CHARS).collect::<String>();
    let title = title.trim_end();
    (!title.is_empty()).then(|| Arc::from(title))
}

fn preferred_setting_command(
    thread: &AgentThread,
    preferences: &AgentPreferences,
) -> Option<AgentSettingRequest> {
    for kind in [
        AgentPreferenceKind::Model,
        AgentPreferenceKind::Effort,
        AgentPreferenceKind::Permission,
    ] {
        let option = thread
            .config_options
            .iter()
            .find(|option| preference_kind_for_category(option.category) == Some(kind));
        if let Some(option) = option {
            if thread
                .preference_reconcile_skips
                .contains(&(kind, option.id.clone()))
            {
                continue;
            }
            let Some(value) =
                preferences.desired(thread.provider, &thread.agent_key, kind, &option.id)
            else {
                continue;
            };
            if value == option.current_value
                || !option.choices.iter().any(|choice| choice.value == value)
            {
                continue;
            }
            return Some(AgentSettingRequest {
                config_id: option.id.clone(),
                value: value.to_owned(),
                origin: AgentSettingOrigin::Preference(kind),
            });
        }
        if kind == AgentPreferenceKind::Permission {
            if thread.preference_reconcile_skips.contains(&(
                AgentPreferenceKind::Permission,
                LEGACY_MODE_PREFERENCE_ID.to_owned(),
            )) {
                continue;
            }
            let Some(value) = preferences.desired(
                thread.provider,
                &thread.agent_key,
                AgentPreferenceKind::Permission,
                LEGACY_MODE_PREFERENCE_ID,
            ) else {
                continue;
            };
            if thread.mode.as_deref() == Some(value)
                || !thread.modes.iter().any(|mode| mode.id == value)
            {
                continue;
            }
            return Some(AgentSettingRequest {
                config_id: LEGACY_MODE_PREFERENCE_ID.to_owned(),
                value: value.to_owned(),
                origin: AgentSettingOrigin::Preference(AgentPreferenceKind::Permission),
            });
        }
    }
    None
}

fn decode_value<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Option<T> {
    serde_json::from_value(value)
        .map_err(|error| {
            log::warn!(
                target: "zz::agent",
                "dropping an agent payload zz could not re-type: {}",
                log_excerpt(&error.to_string(), MAX_DECODE_ERROR_BYTES)
            );
        })
        .ok()
}

fn decode_json<T: serde::de::DeserializeOwned>(value: Option<serde_json::Value>) -> Option<T> {
    value.and_then(decode_value)
}

fn session_update_tag(update: &serde_json::Value) -> String {
    log_excerpt(
        update
            .get("sessionUpdate")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(UNTAGGED_SESSION_UPDATE),
        MAX_SESSION_UPDATE_TAG_BYTES,
    )
}

fn log_excerpt(text: &str, max_bytes: usize) -> String {
    capped(
        text.chars()
            .filter(|character| !character.is_control())
            .collect(),
        max_bytes,
    )
}

fn decode_state_blob<T: serde::de::DeserializeOwned>(blob: &str) -> Option<T> {
    if blob.is_empty() {
        return None;
    }
    serde_json::from_str(blob)
        .map_err(|error| {
            log::warn!(target: "zz::agent", "dropping an agent state blob zz could not re-type: {error}");
        })
        .ok()
}

fn decode_permission_payload(payload: &str) -> Option<(ToolCallUpdate, Vec<PermissionOption>)> {
    let value = decode_state_blob::<serde_json::Value>(payload)?;
    let tool_call = decode_value(value.get("toolCall")?.clone())?;
    let options = decode_value(value.get("options")?.clone())?;
    Some((tool_call, options))
}

impl EventEmitter<AgentControllerEvent> for AgentController {}

fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_BYTES
        && !session_id.chars().any(char::is_control)
}

fn valid_session_cursor(cursor: &str) -> bool {
    !cursor.is_empty()
        && cursor.len() <= MAX_SESSION_CURSOR_BYTES
        && !cursor.chars().any(char::is_control)
}

fn valid_session_summary(session: &AgentSessionSummary) -> bool {
    valid_session_id(&session.session_id)
        && valid_session_directory(&session.cwd)
        && session.additional_directories.len() <= MAX_AGENT_SESSION_DIRECTORIES
        && session
            .additional_directories
            .iter()
            .all(|directory| valid_session_directory(directory))
        && session.title.as_deref().is_none_or(|title| {
            title.len() <= MAX_SESSION_TITLE_BYTES && !title.chars().any(char::is_control)
        })
        && session.updated_at.as_deref().is_none_or(|timestamp| {
            timestamp.len() <= MAX_SESSION_TIMESTAMP_BYTES
                && !timestamp.chars().any(char::is_control)
        })
}

fn valid_session_directory(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.as_os_str().as_encoded_bytes().len() <= MAX_GUI_TEXT_BYTES
}

const fn preference_kind_for_category(
    category: AgentConfigCategory,
) -> Option<AgentPreferenceKind> {
    match category {
        AgentConfigCategory::Mode => Some(AgentPreferenceKind::Permission),
        AgentConfigCategory::Model => Some(AgentPreferenceKind::Model),
        AgentConfigCategory::ThoughtLevel => Some(AgentPreferenceKind::Effort),
        AgentConfigCategory::ModelConfig | AgentConfigCategory::Other => None,
    }
}

fn agent_command_model(command: AvailableCommand) -> AgentCommand {
    let input_hint = command.input.and_then(|input| match input {
        AvailableCommandInput::Unstructured(input) => Some(input.hint),
        _ => None,
    });
    AgentCommand {
        name: command.name,
        description: command.description,
        input_hint,
    }
}

fn config_option_models(options: Vec<SessionConfigOption>) -> Vec<AgentConfigOption> {
    options
        .into_iter()
        .take(MAX_AGENT_CONFIG_OPTIONS)
        .filter_map(config_option_model)
        .collect()
}

fn config_option_model(option: SessionConfigOption) -> Option<AgentConfigOption> {
    let SessionConfigKind::Select(select) = option.kind else {
        return None;
    };
    let choices = match select.options {
        agent_client_protocol::schema::v1::SessionConfigSelectOptions::Ungrouped(options) => {
            options
        }
        agent_client_protocol::schema::v1::SessionConfigSelectOptions::Grouped(groups) => {
            groups.into_iter().flat_map(|group| group.options).collect()
        }
        _ => Vec::new(),
    }
    .into_iter()
    .take(MAX_AGENT_CONFIG_CHOICES)
    .map(|choice| AgentConfigChoice {
        value: choice.value.0.to_string(),
        name: choice.name,
        description: choice.description,
    })
    .collect();
    let category = match option.category {
        Some(SessionConfigOptionCategory::Mode) => AgentConfigCategory::Mode,
        Some(SessionConfigOptionCategory::Model) => AgentConfigCategory::Model,
        Some(SessionConfigOptionCategory::ModelConfig) => AgentConfigCategory::ModelConfig,
        Some(SessionConfigOptionCategory::ThoughtLevel) => AgentConfigCategory::ThoughtLevel,
        _ => AgentConfigCategory::Other,
    };
    Some(AgentConfigOption {
        id: option.id.0.to_string(),
        name: option.name,
        description: option.description,
        category,
        current_value: select.current_value.0.to_string(),
        choices,
    })
}

fn map_permission_kind(kind: PermissionOptionKind) -> AgentPermissionKind {
    match kind {
        PermissionOptionKind::AllowOnce => AgentPermissionKind::AllowOnce,
        PermissionOptionKind::AllowAlways => AgentPermissionKind::AllowAlways,
        PermissionOptionKind::RejectAlways => AgentPermissionKind::RejectAlways,
        _ => AgentPermissionKind::RejectOnce,
    }
}

fn update_carries_tool_shape(fields: &ToolCallUpdateFields) -> bool {
    fields.title.is_some()
        || fields.raw_input.is_some()
        || fields.locations.is_some()
        || fields.content.as_ref().is_some_and(|content| {
            content
                .iter()
                .any(|content| matches!(content, ToolCallContent::Diff(_)))
        })
}

fn reclassifies_tool(current: AgentToolKindModel, next: ToolKind, carries_shape: bool) -> bool {
    carries_shape
        || current == AgentToolKindModel::Other
        || map_tool_kind(next) != AgentToolKindModel::Other
}

fn map_tool_kind(kind: ToolKind) -> AgentToolKindModel {
    match kind {
        ToolKind::Read => AgentToolKindModel::Read,
        ToolKind::Search => AgentToolKindModel::Search,
        ToolKind::Edit => AgentToolKindModel::Edit,
        ToolKind::Delete => AgentToolKindModel::Delete,
        ToolKind::Move => AgentToolKindModel::Move,
        ToolKind::Execute => AgentToolKindModel::Execute,
        ToolKind::Fetch => AgentToolKindModel::Fetch,
        ToolKind::Think => AgentToolKindModel::Think,
        ToolKind::SwitchMode => AgentToolKindModel::SwitchMode,
        _ => AgentToolKindModel::Other,
    }
}

fn map_tool_status(status: ToolCallStatus) -> AgentToolStatusModel {
    match status {
        ToolCallStatus::InProgress => AgentToolStatusModel::Running,
        ToolCallStatus::Completed => AgentToolStatusModel::Completed,
        ToolCallStatus::Failed => AgentToolStatusModel::Failed,
        _ => AgentToolStatusModel::Pending,
    }
}

fn inbound_image(image: &ImageContent) -> Option<Arc<Image>> {
    let format = ImageFormat::from_mime_type(&image.mime_type)?;
    let bytes = BASE64.decode(&image.data).ok()?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn split_inline_images(text: &str) -> (String, Vec<Arc<Image>>) {
    const MARKER: &str = "data:image/";
    if !text.contains(MARKER) {
        return (text.to_owned(), Vec::new());
    }
    let mut kept = String::with_capacity(text.len());
    let mut images = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find(MARKER) {
        let end = remaining[start..]
            .find(|character: char| {
                character.is_whitespace() || matches!(character, ')' | '"' | '\'' | '<' | '>')
            })
            .map_or(remaining.len(), |offset| start + offset);
        let Some(image) = decode_data_uri(&remaining[start..end]) else {
            kept.push_str(&remaining[..end]);
            remaining = &remaining[end..];
            continue;
        };
        images.push(image);
        let before = &remaining[..start];
        let (prefix_end, closer) = before
            .strip_suffix("](")
            .and_then(|before| before.rfind('['))
            .map_or((before.len(), ""), |open| (open, ")"));
        kept.push_str(&remaining[..prefix_end]);
        remaining = remaining[end..]
            .strip_prefix(closer)
            .unwrap_or(&remaining[end..]);
    }
    kept.push_str(remaining);
    (kept.trim().to_owned(), images)
}

fn decode_data_uri(uri: &str) -> Option<Arc<Image>> {
    let (mime, payload) = uri.strip_prefix("data:")?.split_once(";base64,")?;
    let format = ImageFormat::from_mime_type(mime)?;
    let bytes = BASE64.decode(payload).ok()?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn content_block_markdown(content: &ContentBlock) -> String {
    match content {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::Image(image) => format!("*[Image: {}]*", image.mime_type),
        ContentBlock::Audio(audio) => format!("*[Audio: {}]*", audio.mime_type),
        ContentBlock::ResourceLink(resource) => {
            let label = resource.title.as_deref().unwrap_or(&resource.name);
            format!("[{label}]({})", resource.uri)
        }
        ContentBlock::Resource(resource) => match &resource.resource {
            agent_client_protocol::schema::v1::EmbeddedResourceResource::TextResourceContents(
                resource,
            ) => resource.text.clone(),
            agent_client_protocol::schema::v1::EmbeddedResourceResource::BlobResourceContents(
                resource,
            ) => format!("*[Embedded resource: {}]*", resource.uri),
            _ => pretty_json_markdown(content),
        },
        _ => pretty_json_markdown(content),
    }
}

fn tool_location(tool: &ToolCall) -> Option<String> {
    tool.locations.first().map(|location| {
        location.line.map_or_else(
            || location.path.display().to_string(),
            |line| format!("{}:{line}", location.path.display()),
        )
    })
}

fn tool_input(tool: &ToolCall) -> Option<ToolPayload> {
    tool.raw_input.as_ref().and_then(json_payload)
}

fn tool_output(tool: &ToolCall) -> Vec<ToolPayload> {
    let structured = tool_content_payloads(&tool.content);
    if structured.is_empty() {
        tool.raw_output
            .as_ref()
            .and_then(json_payload)
            .into_iter()
            .collect()
    } else {
        structured
    }
}

fn tool_content_payloads(content: &[ToolCallContent]) -> Vec<ToolPayload> {
    content
        .iter()
        .take(MAX_AGENT_TOOL_CONTENT_ITEMS)
        .map(tool_content_payload)
        .collect()
}

fn tool_content_payload(content: &ToolCallContent) -> ToolPayload {
    match content {
        ToolCallContent::Diff(diff) => ToolPayload::Diff {
            path: diff.path.display().to_string(),
            old: diff
                .old_text
                .clone()
                .map(|old| capped(old, MAX_DIFF_SIDE_BYTES)),
            new: capped(diff.new_text.clone(), MAX_DIFF_SIDE_BYTES),
        },
        ToolCallContent::Content(content) => match &content.content {
            ContentBlock::Text(text) => ToolPayload::Text(capped_payload(text.text.clone())),
            _ => ToolPayload::Json(capped_payload(pretty_json(content).unwrap_or_default())),
        },
        ToolCallContent::Terminal(terminal) => {
            ToolPayload::Terminal(format!("[terminal {}]", terminal.terminal_id.0))
        }
        _ => ToolPayload::Json(capped_payload(pretty_json(content).unwrap_or_default())),
    }
}

fn pretty_json(value: &impl serde::Serialize) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

fn json_payload(value: &impl serde::Serialize) -> Option<ToolPayload> {
    pretty_json(value).map(|json| ToolPayload::Json(capped_payload(json)))
}

fn capped_payload(text: String) -> String {
    capped(text, MAX_TOOL_PAYLOAD_BYTES)
}

fn capped(mut text: String, max_bytes: usize) -> String {
    truncate_payload(&mut text, max_bytes);
    text
}

fn truncate_payload(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes.saturating_sub(TRUNCATION_MARKER.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(TRUNCATION_MARKER);
}

fn pretty_json_markdown(value: &impl serde::Serialize) -> String {
    pretty_json(value).map_or_else(String::new, |value| format!("```json\n{value}\n```"))
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::{
        AvailableCommandsUpdate, ConfigOptionUpdate, Diff, MessageId, PlanEntry, PlanEntryPriority,
        SessionConfigSelectOption, Terminal, TextContent, ToolCallLocation,
    };
    use std::{cell::RefCell, rc::Rc};

    use gpui::{AppContext as _, TestAppContext};
    use parking_lot::Mutex;

    use super::*;

    fn thread() -> AgentThread {
        AgentThread::new(AgentProvider::Codex, PathBuf::from("/workspace"), None)
    }

    fn select_option(
        id: &str,
        category: AgentConfigCategory,
        current_value: &str,
        other_value: &str,
    ) -> AgentConfigOption {
        AgentConfigOption {
            id: id.to_owned(),
            name: id.to_owned(),
            description: None,
            category,
            current_value: current_value.to_owned(),
            choices: vec![
                AgentConfigChoice {
                    value: current_value.to_owned(),
                    name: current_value.to_owned(),
                    description: None,
                },
                AgentConfigChoice {
                    value: other_value.to_owned(),
                    name: other_value.to_owned(),
                    description: None,
                },
            ],
        }
    }

    #[test]
    fn sticky_settings_restore_in_model_effort_permission_order() {
        let mut thread = thread();
        thread.connection = AgentConnectionState::Ready;
        thread.config_options = vec![
            select_option("model", AgentConfigCategory::Model, "small", "large"),
            select_option(
                "effort",
                AgentConfigCategory::ThoughtLevel,
                "medium",
                "high",
            ),
            select_option("permission", AgentConfigCategory::Mode, "ask", "auto"),
        ]
        .into();
        let mut preferences = AgentPreferences::default();
        for (kind, option, value) in [
            (AgentPreferenceKind::Model, "model", "large"),
            (AgentPreferenceKind::Effort, "effort", "high"),
            (AgentPreferenceKind::Permission, "permission", "auto"),
        ] {
            preferences.remember(thread.provider, &thread.agent_key, kind, option, value);
        }

        let next = |thread: &AgentThread| preferred_setting_command(thread, &preferences);
        assert!(matches!(
            next(&thread),
            Some(AgentSettingRequest { ref config_id, .. }) if config_id == "model"
        ));
        Arc::make_mut(&mut thread.config_options)[0].current_value = "large".to_owned();
        assert!(matches!(
            next(&thread),
            Some(AgentSettingRequest { ref config_id, .. }) if config_id == "effort"
        ));
        Arc::make_mut(&mut thread.config_options)[1].current_value = "high".to_owned();
        assert!(matches!(
            next(&thread),
            Some(AgentSettingRequest { ref config_id, .. }) if config_id == "permission"
        ));
    }

    #[test]
    fn sticky_legacy_permission_mode_can_coexist_with_other_config_options() {
        let mut thread = thread();
        thread.connection = AgentConnectionState::Ready;
        thread.config_options = vec![select_option(
            "model",
            AgentConfigCategory::Model,
            "small",
            "large",
        )]
        .into();
        thread.mode = Some(Arc::from("ask"));
        thread.modes = vec![
            AgentMode {
                id: "ask".to_owned(),
                name: "Ask".to_owned(),
                description: None,
            },
            AgentMode {
                id: "code".to_owned(),
                name: "Code".to_owned(),
                description: None,
            },
        ]
        .into();
        let mut preferences = AgentPreferences::default();
        preferences.remember(
            thread.provider,
            &thread.agent_key,
            AgentPreferenceKind::Permission,
            LEGACY_MODE_PREFERENCE_ID,
            "code",
        );

        assert!(matches!(
            preferred_setting_command(&thread, &preferences),
            Some(AgentSettingRequest { ref config_id, ref value, .. })
                if config_id == LEGACY_MODE_PREFERENCE_ID && value == "code"
        ));
    }

    #[test]
    fn reducer_coalesces_message_chunks_by_protocol_id() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new("hello ")))
                .message_id(MessageId::new("message-1")),
        ));
        let entry_id = thread.entries[0].id();
        let first_revision = thread.entry_revisions[0];
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new("world")))
                .message_id(MessageId::new("message-1")),
        ));

        assert_eq!(thread.entries.len(), 1);
        assert_eq!(thread.entry_revisions.len(), thread.entries.len());
        assert_eq!(thread.entry_indices.get(&entry_id), Some(&0));
        assert!(thread.entry_revisions[0] > first_revision);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Assistant { markdown, .. } if markdown == "hello world"
        ));
    }

    #[test]
    fn reducer_tracks_available_commands_and_generic_config_options() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::AvailableCommandsUpdate(
            AvailableCommandsUpdate::new(vec![
                AvailableCommand::new("review", "Review changes").input(
                    AvailableCommandInput::Unstructured(
                        agent_client_protocol::schema::v1::UnstructuredCommandInput::new(
                            "optional instructions",
                        ),
                    ),
                ),
                AvailableCommand::new("brainstorm", "Explore an idea"),
            ]),
        ));
        thread.apply_update(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
            vec![
                SessionConfigOption::select(
                    "model",
                    "Model",
                    "gpt-5",
                    vec![
                        SessionConfigSelectOption::new("gpt-5", "GPT-5"),
                        SessionConfigSelectOption::new("gpt-5-mini", "GPT-5 mini"),
                    ],
                )
                .category(SessionConfigOptionCategory::Model),
                SessionConfigOption::select(
                    "reasoning_effort",
                    "Reasoning effort",
                    "medium",
                    vec![
                        SessionConfigSelectOption::new("low", "Low"),
                        SessionConfigSelectOption::new("medium", "Medium"),
                    ],
                )
                .category(SessionConfigOptionCategory::ThoughtLevel),
            ],
        )));

        assert!(matches!(
            thread.available_commands.as_ref(),
            [AgentCommand {
                name,
                input_hint: Some(hint),
                ..
            }, AgentCommand {
                name: second_name,
                ..
            }] if name == "review"
                && hint == "optional instructions"
                && second_name == "brainstorm"
        ));
        assert!(matches!(
            thread.config_options.as_ref(),
            [AgentConfigOption {
                category: AgentConfigCategory::Model,
                current_value: model,
                choices: models,
                ..
            }, AgentConfigOption {
                category: AgentConfigCategory::ThoughtLevel,
                current_value: effort,
                choices: efforts,
                ..
            }] if model == "gpt-5"
                && models.len() == 2
                && effort == "medium"
                && efforts.len() == 2
        ));
        thread.set_session_configuration(None, None);
        assert_eq!(thread.config_options.len(), 2);
    }

    #[test]
    fn adapter_controlled_ui_collections_are_bounded() {
        let mut thread = thread();
        let commands = (0..MAX_AGENT_AVAILABLE_COMMANDS + 8)
            .map(|index| AvailableCommand::new(format!("command-{index}"), "run it"))
            .collect();
        thread.apply_update(SessionUpdate::AvailableCommandsUpdate(
            AvailableCommandsUpdate::new(commands),
        ));
        assert_eq!(
            thread.available_commands.len(),
            MAX_AGENT_AVAILABLE_COMMANDS
        );

        let choices = (0..MAX_AGENT_CONFIG_CHOICES + 8)
            .map(|index| {
                SessionConfigSelectOption::new(format!("value-{index}"), format!("Value {index}"))
            })
            .collect::<Vec<_>>();
        let options = (0..MAX_AGENT_CONFIG_OPTIONS + 8)
            .map(|index| {
                SessionConfigOption::select(
                    format!("option-{index}"),
                    format!("Option {index}"),
                    "value-0",
                    choices.clone(),
                )
            })
            .collect();
        thread.apply_update(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
            options,
        )));
        assert_eq!(thread.config_options.len(), MAX_AGENT_CONFIG_OPTIONS);
        assert!(
            thread
                .config_options
                .iter()
                .all(|option| option.choices.len() == MAX_AGENT_CONFIG_CHOICES)
        );

        let content = (0..MAX_AGENT_TOOL_CONTENT_ITEMS + 8)
            .map(|index| ContentBlock::Text(TextContent::new(index.to_string())).into())
            .collect::<Vec<_>>();
        assert_eq!(
            tool_content_payloads(&content).len(),
            MAX_AGENT_TOOL_CONTENT_ITEMS
        );

        let permission_options = (0..MAX_AGENT_PERMISSION_OPTIONS + 8)
            .map(|index| {
                PermissionOption::new(
                    format!("allow-{index}"),
                    format!("Allow {index}"),
                    PermissionOptionKind::AllowOnce,
                )
            })
            .collect();
        thread.request_permission(
            42,
            ToolCallUpdate::new(
                "tool-cap",
                ToolCallUpdateFields::new().title("Bound choices"),
            ),
            permission_options,
        );
        assert_eq!(
            thread.pending_permissions[0].options.len(),
            MAX_AGENT_PERMISSION_OPTIONS
        );
    }

    #[test]
    fn tool_updates_keep_a_stable_native_entry_id() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-1", "Run tests")
                .kind(ToolKind::Execute)
                .status(ToolCallStatus::InProgress),
        ));
        let id = thread.entries[0].id();
        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tool-1",
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .raw_output(serde_json::json!({"ok": true})),
        )));

        assert_eq!(thread.entries.len(), 1);
        assert_eq!(thread.entries[0].id(), id);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Completed,
                output,
                ..
            } if matches!(
                output.as_slice(),
                [ToolPayload::Json(output)] if output.contains("ok")
            )
        ));
    }

    #[test]
    fn vendor_metadata_is_inert_and_parented_updates_stay_flat() {
        let mut thread = thread();
        let metadata = serde_json::json!({
            "claudeCode": {
                "subagent": true,
                "parentToolUseId": "parent-tool"
            },
            "codex": {
                "subagent": {
                    "threadId": "thread-1",
                    "activity": "started"
                },
                "collaboration": {
                    "tool": "spawnAgent"
                }
            },
            "contextCompaction": true,
            "terminal_info": {
                "terminal_id": "terminal-1"
            }
        })
        .as_object()
        .unwrap()
        .clone();
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-1", "Task")
                .status(ToolCallStatus::InProgress)
                .raw_input(serde_json::json!({"prompt": "sleep"}))
                .meta(metadata.clone()),
        ));
        thread.apply_update(SessionUpdate::ToolCallUpdate(
            ToolCallUpdate::new("tool-1", ToolCallUpdateFields::new()).meta(metadata.clone()),
        ));
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new("child output"))).meta(metadata),
        ));

        assert!(matches!(
            thread.entries.as_slice(),
            [
                AgentThreadEntry::Tool {
                    label,
                    status: AgentToolStatusModel::Running,
                    input: Some(ToolPayload::Json(input)),
                    output,
                    ..
                },
                AgentThreadEntry::Assistant { markdown, .. }
            ] if label == "Task"
                && input.contains("sleep")
                && output.is_empty()
                && markdown == "child output"
        ));
    }

    #[test]
    fn prompt_completion_ends_activity_without_fabricating_tool_completion() {
        let mut thread = thread();
        thread.connection = AgentConnectionState::Running;
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-running", "Run tests").status(ToolCallStatus::InProgress),
        ));
        thread.apply_update(SessionUpdate::ToolCall(ToolCall::new(
            "tool-pending",
            "Read file",
        )));

        thread.finish_turn();
        thread.connection = AgentConnectionState::Ready;

        let statuses = thread
            .entries
            .iter()
            .filter_map(|entry| match entry {
                AgentThreadEntry::Tool { status, .. } => Some(*status),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            statuses,
            [AgentToolStatusModel::Running, AgentToolStatusModel::Pending,]
        );
        assert_eq!(thread.connection, AgentConnectionState::Ready);
    }

    #[test]
    fn late_standard_updates_change_data_without_reviving_pane_activity() {
        let mut thread = thread();
        thread.connection = AgentConnectionState::Ready;

        thread.apply_runtime_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-late", "Read replayed file").status(ToolCallStatus::InProgress),
        ));
        thread.apply_runtime_update(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("late output")),
        )));

        assert_eq!(thread.connection, AgentConnectionState::Ready);
        assert!(matches!(
            thread.entries.as_slice(),
            [
                AgentThreadEntry::Tool {
                    status: AgentToolStatusModel::Running,
                    ..
                },
                AgentThreadEntry::Assistant { markdown, .. }
            ] if markdown == "late output"
        ));
    }

    #[test]
    fn tool_payloads_preserve_structure_and_prefer_content_over_raw_output() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::ToolCall(ToolCall::new(
            "tool-1",
            "Edit file",
        )));
        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tool-1",
            ToolCallUpdateFields::new()
                .locations(vec![
                    ToolCallLocation::new("/workspace/src/lib.rs").line(12),
                ])
                .raw_input(serde_json::json!({"path": "/workspace/src/lib.rs"}))
                .content(vec![
                    Diff::new("/workspace/src/lib.rs", "new text")
                        .old_text("old text")
                        .into(),
                    ContentBlock::Text(TextContent::new("finished")).into(),
                    ToolCallContent::Terminal(Terminal::new("terminal-1")),
                ])
                .raw_output(serde_json::json!({"ignored": true})),
        )));

        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                location: Some(location),
                input: Some(ToolPayload::Json(input)),
                output,
                ..
            } if location == "/workspace/src/lib.rs:12"
                && input.contains("path")
                && matches!(
                    output.as_slice(),
                    [
                        ToolPayload::Diff { path, old: Some(old), new },
                        ToolPayload::Text(text),
                        ToolPayload::Terminal(terminal),
                    ] if path == "/workspace/src/lib.rs"
                        && old == "old text"
                        && new == "new text"
                        && text == "finished"
                        && terminal == "[terminal terminal-1]"
                )
        ));

        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tool-1",
            ToolCallUpdateFields::new().raw_output(serde_json::json!({"late": true})),
        )));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool { output, .. }
                if matches!(output.first(), Some(ToolPayload::Diff { .. }))
        ));

        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tool-1",
            ToolCallUpdateFields::new()
                .content(Vec::new())
                .raw_output(serde_json::json!({"fallback": true})),
        )));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool { output, .. }
                if matches!(
                    output.as_slice(),
                    [ToolPayload::Json(output)] if output.contains("fallback")
                )
        ));
    }

    #[test]
    fn plan_updates_replace_the_existing_plan_entry() {
        let mut thread = thread();
        let plan = |status| {
            Plan::new(vec![PlanEntry::new(
                "Implement ACP",
                PlanEntryPriority::High,
                status,
            )])
        };
        thread.apply_update(SessionUpdate::Plan(plan(PlanEntryStatus::InProgress)));
        let id = thread.entries[0].id();
        thread.apply_update(SessionUpdate::Plan(plan(PlanEntryStatus::Completed)));

        assert_eq!(thread.entries.len(), 1);
        assert_eq!(thread.entries[0].id(), id);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Plan { markdown, .. } if markdown.starts_with("- [x]")
        ));
    }

    #[test]
    fn permission_lifecycle_marks_and_releases_the_tool() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::ToolCall(ToolCall::new(
            "tool-1",
            "Edit file",
        )));
        thread.request_permission(
            7,
            ToolCallUpdate::new(
                "tool-1",
                ToolCallUpdateFields::new().title("Edit file".to_owned()),
            ),
            vec![PermissionOption::new(
                "allow-once",
                "Allow once",
                PermissionOptionKind::AllowOnce,
            )],
        );
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::NeedsApproval,
                ..
            }
        ));
        thread.resolve_permission(7, false);
        assert!(thread.pending_permissions.is_empty());
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Pending,
                ..
            }
        ));
    }

    const PNG_PIXEL: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn attachment() -> Arc<Image> {
        Arc::new(Image::from_bytes(ImageFormat::Png, PNG_PIXEL.to_vec()))
    }

    #[test]
    fn a_replayed_user_image_returns_to_the_transcript() {
        let mut thread = thread();
        let attachment = attachment();
        let sent = ContentBlock::Image(ImageContent::new(
            BASE64.encode(&attachment.bytes),
            attachment.format.mime_type(),
        ));

        thread.apply_update(SessionUpdate::UserMessageChunk(ContentChunk::new(sent)));

        let Some(AgentThreadEntry::User {
            markdown, images, ..
        }) = thread.entries.first()
        else {
            panic!("the replayed chunk should open a user entry");
        };
        assert!(
            markdown.is_empty(),
            "an image should not leave a text placeholder behind"
        );
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].bytes, PNG_PIXEL);
    }

    #[test]
    fn a_replayed_data_uri_becomes_an_image_rather_than_a_link() {
        let mut thread = thread();
        let replayed = format!(
            "hi can you read this image properly?[@image](data:image/png;base64,{})",
            BASE64.encode(PNG_PIXEL)
        );
        thread.apply_update(SessionUpdate::UserMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new(replayed)),
        )));

        let Some(AgentThreadEntry::User {
            markdown, images, ..
        }) = thread.entries.first()
        else {
            panic!("the replayed chunk should open a user entry");
        };
        assert_eq!(
            markdown, "hi can you read this image properly?",
            "the link should leave with the image, not linger as `[@image]`"
        );
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].bytes, PNG_PIXEL);
    }

    #[test]
    fn text_that_only_looks_like_an_attachment_is_left_alone() {
        let (kept, images) = split_inline_images("see data:image/png;base64,not-base64!!");
        assert!(images.is_empty());
        assert_eq!(kept, "see data:image/png;base64,not-base64!!");

        let (kept, images) = split_inline_images("just a sentence");
        assert!(images.is_empty());
        assert_eq!(kept, "just a sentence");
    }

    #[test]
    fn an_agent_that_takes_no_images_refuses_them_before_the_turn_starts() {
        let mut thread = thread();
        thread.connection = AgentConnectionState::Ready;

        assert!(
            !thread.session_capabilities.images,
            "an agent is assumed to take no images until it advertises them"
        );
        assert!(
            thread.prompt_refusal(false).is_none(),
            "text is always fine"
        );
        assert_eq!(
            thread.prompt_refusal(true).as_deref(),
            Some("this agent does not accept images")
        );

        thread.session_capabilities.images = true;
        assert!(thread.prompt_refusal(true).is_none());

        thread.connection = AgentConnectionState::Starting;
        assert!(thread.prompt_refusal(true).is_some());
    }

    #[test]
    fn transport_loss_clears_approvals_and_cancels_inflight_tools() {
        let mut thread = thread();
        thread.begin_prompt("make a change".to_owned(), Vec::new());
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-1", "Edit file")
                .kind(ToolKind::Edit)
                .status(ToolCallStatus::InProgress),
        ));
        thread.request_permission(
            7,
            ToolCallUpdate::new("tool-1", ToolCallUpdateFields::new()),
            vec![PermissionOption::new(
                "allow-once",
                "Allow once",
                PermissionOptionKind::AllowOnce,
            )],
        );

        thread.cancel_inflight();

        assert!(thread.pending_permissions.is_empty());
        assert!(!thread.suppress_user_echo);
        assert!(thread.active_stream.is_none());
        assert!(matches!(
            thread.entries.last(),
            Some(AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Canceled,
                ..
            })
        ));
    }

    #[test]
    fn attention_buckets_partition_the_fleet() {
        let mut controller = AgentController::new(AgentConfig::default());
        assert!(controller.attention().is_quiet());

        let mut running = thread();
        running.connection = AgentConnectionState::Running;
        let mut waiting = thread();
        waiting.connection = AgentConnectionState::Running;
        waiting.pending_permissions = Arc::from([AgentPermissionRequest {
            request_id: 1,
            tool_call_id: "call".to_owned(),
            title: "Run tests".to_owned(),
            options: Vec::new(),
        }]);
        let mut failed = thread();
        failed.connection = AgentConnectionState::Failed;
        for (pane, thread) in [(1, running), (2, waiting), (3, failed), (4, thread())] {
            controller.panes.insert(PaneId(pane), thread);
        }

        let attention = controller.attention();
        assert!(!attention.is_quiet());
        assert_eq!(
            (attention.waiting, attention.failed, attention.running),
            (1, 1, 1)
        );
        assert_eq!(attention.waiting_pane, Some(PaneId(2)));
        assert_eq!(attention.failed_pane, Some(PaneId(3)));
    }

    #[test]
    fn pending_composer_text_stacks_and_is_taken_once() {
        let mut controller = AgentController::new(AgentConfig::default());
        let pane = PaneId(7);
        assert!(controller.take_pending_composer(pane).is_none());

        assert!(controller.queue_composer_text(pane, "first"));
        assert!(controller.queue_composer_text(pane, "second"));
        assert!(!controller.queue_composer_text(pane, "   "));
        assert!(controller.pane_state(pane).is_none());

        controller.panes.insert(pane, thread());
        assert_eq!(
            controller
                .pane_state(pane)
                .expect("state")
                .pending_composer
                .as_deref(),
            Some("first\nsecond")
        );
        assert_eq!(
            controller.take_pending_composer(pane).as_deref(),
            Some("first\nsecond")
        );
        assert!(controller.take_pending_composer(pane).is_none());
        assert!(
            controller
                .pane_state(pane)
                .expect("state")
                .pending_composer
                .is_none()
        );
    }

    #[test]
    fn a_prompt_is_refused_unless_the_pane_is_ready() {
        let mut thread = thread();
        assert!(thread.prompt_refusal(false).is_some());
        thread.connection = AgentConnectionState::Ready;
        assert!(thread.prompt_refusal(false).is_none());
        thread.connection = AgentConnectionState::Running;
        assert!(thread.prompt_refusal(false).is_some());
    }

    #[test]
    fn session_ids_are_bounded_before_they_reach_daemon_metadata() {
        assert!(valid_session_id("opaque/session:1"));
        assert!(!valid_session_id(""));
        assert!(!valid_session_id("bad\nsession"));
        assert!(!valid_session_id(&"x".repeat(MAX_SESSION_ID_BYTES + 1)));
        assert!(valid_session_cursor("opaque-next-page"));
        assert!(!valid_session_cursor(""));
        assert!(!valid_session_cursor("bad\ncursor"));
        assert!(!valid_session_cursor(
            &"x".repeat(MAX_SESSION_CURSOR_BYTES + 1)
        ));
        assert!(valid_session_directory(Path::new("/srv/work")));
        assert!(valid_session_directory(Path::new(r"C:\work")));
        assert!(!valid_session_directory(Path::new("")));
    }

    /// The question branch is unreachable while zz speaks ACP v1: the schema
    /// types an option kind as a closed enum, so a question-shaped kind never
    /// survives deserialization to reach [`is_user_question`].
    #[test]
    fn acp_v1_rejects_permission_options_with_an_unknown_kind() {
        assert!(
            serde_json::from_value::<PermissionOption>(serde_json::json!({
                "optionId": "pick-a",
                "name": "Option A",
                "kind": "answer"
            }))
            .is_err()
        );
    }

    #[test]
    fn status_only_updates_never_retype_a_typed_tool() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-1", "cargo test")
                .kind(ToolKind::Execute)
                .status(ToolCallStatus::InProgress),
        ));
        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tool-1",
            ToolCallUpdateFields::new()
                .kind(ToolKind::Other)
                .status(ToolCallStatus::Completed)
                .raw_output(serde_json::json!({"ok": true})),
        )));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                kind: AgentToolKindModel::Execute,
                status: AgentToolStatusModel::Completed,
                label,
                ..
            } if label == "cargo test"
        ));

        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tool-1",
            ToolCallUpdateFields::new().title(String::new()),
        )));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool { label, .. } if label == "cargo test"
        ));

        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tool-1",
            ToolCallUpdateFields::new()
                .kind(ToolKind::Other)
                .title("Something else".to_owned()),
        )));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                kind: AgentToolKindModel::Other,
                label,
                ..
            } if label == "Something else"
        ));
    }

    #[test]
    fn oversized_tool_payloads_are_capped_on_char_boundaries() {
        let mut text = "é".repeat(16);
        truncate_payload(&mut text, TRUNCATION_MARKER.len() + 5);
        assert_eq!(text, "éé".to_owned() + TRUNCATION_MARKER);

        let mut thread = thread();
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-big", "Read file").content(vec![ToolCallContent::from(
                ContentBlock::Text(TextContent::new("é".repeat(MAX_TOOL_PAYLOAD_BYTES))),
            )]),
        ));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool { output, .. }
                if matches!(
                    output.as_slice(),
                    [ToolPayload::Text(text)]
                        if text.len() <= MAX_TOOL_PAYLOAD_BYTES
                            && text.ends_with(TRUNCATION_MARKER)
                )
        ));
    }

    /// back, standing in for the daemon on the other end of the wire.
    fn proxy_controller(
        cx: &mut TestAppContext,
    ) -> (
        Entity<AgentController>,
        Rc<RefCell<Vec<(PaneId, AgentRequest)>>>,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(zz_daemon::DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let sink = mux.update(cx, |mux, _| {
                mux.set_agent_client_instance_id_for_test(zz_protocol::ClientInstanceId(1));
                mux.record_agent_requests_for_test()
            });
            let controller = cx.new(|_| AgentController::new(AgentConfig::default()));
            controller.update(cx, |controller, _| controller.attach_mux(mux));
            (controller, sink)
        })
    }

    /// A registered pane whose agent is connected and taking prompts.
    fn ready_pane(controller: &mut AgentController, pane: PaneId) {
        let mut thread = thread();
        thread.connection = AgentConnectionState::Ready;
        thread.session_capabilities.images = true;
        controller.panes.insert(pane, thread);
        controller.viewports.insert(pane, PaneViewport::default());
        controller.retained_panes.insert(pane);
    }

    fn item(seq: u64, payload: AgentStreamPayload) -> AgentStreamItem {
        AgentStreamItem { seq, payload }
    }

    fn json(value: &impl serde::Serialize) -> serde_json::Value {
        serde_json::to_value(value).expect("the ACP schema encodes")
    }

    /// One `session/update` carrying a `sessionUpdate` this build has no
    /// variant for: a future ACP addition, or an `_`-prefixed extension.
    fn unknown_update(tag: &str) -> AgentStreamPayload {
        AgentStreamPayload::Update {
            update: serde_json::json!({"sessionUpdate": tag, "payload": {"opaque": true}}),
        }
    }

    fn chunk_update(text: &str, message_id: &str) -> AgentStreamPayload {
        AgentStreamPayload::Update {
            update: json(&SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                    .message_id(MessageId::new(message_id)),
            )),
        }
    }

    fn turn_finished(reason: StopReason) -> AgentStreamPayload {
        AgentStreamPayload::PromptFinished {
            turn_id: 1,
            outcome: AgentPromptOutcome::Finished {
                stop_reason: json(&reason),
            },
        }
    }

    #[gpui::test]
    fn stream_items_reduce_exactly_as_the_runtime_events_did(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(3);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller
                    .viewports
                    .get_mut(&pane)
                    .expect("viewport")
                    .queued_prompts = 1;
                controller.apply_stream_items(
                    pane,
                    vec![
                        item(
                            1,
                            AgentStreamPayload::Ready {
                                agent_name: "Codex".to_owned(),
                                agent_key: "codex".to_owned(),
                                auth_methods: vec![zz_daemon::AgentAuthMethod {
                                    id: "api".to_owned(),
                                    name: "API key".to_owned(),
                                    description: None,
                                }],
                                capabilities: zz_daemon::AgentSessionCapabilities {
                                    list: true,
                                    load: true,
                                    ..zz_daemon::AgentSessionCapabilities::default()
                                },
                            },
                        ),
                        item(
                            2,
                            AgentStreamPayload::SessionReady {
                                session_id: "s-1".to_owned(),
                                modes: None,
                                config_options: None,
                            },
                        ),
                        item(
                            3,
                            AgentStreamPayload::Update {
                                update: json(&SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                    ContentBlock::Text(TextContent::new("hello")),
                                ))),
                            },
                        ),
                        item(
                            4,
                            AgentStreamPayload::Update {
                                update: json(&SessionUpdate::ToolCall(
                                    ToolCall::new("tool-1", "Read file")
                                        .status(ToolCallStatus::InProgress),
                                )),
                            },
                        ),
                        item(
                            5,
                            AgentStreamPayload::PermissionRequested {
                                request_id: 7,
                                tool_call: json(&ToolCallUpdate::new(
                                    "tool-1",
                                    ToolCallUpdateFields::new(),
                                )),
                                options: json(&vec![PermissionOption::new(
                                    "allow-once",
                                    "Allow once",
                                    PermissionOptionKind::AllowOnce,
                                )]),
                            },
                        ),
                        item(
                            6,
                            AgentStreamPayload::PermissionResolved {
                                request_id: 7,
                                canceled: false,
                            },
                        ),
                        item(7, turn_finished(StopReason::EndTurn)),
                        item(
                            8,
                            AgentStreamPayload::PromptsReclaimed {
                                prompts: vec![zz_daemon::AgentPrompt {
                                    owner: zz_protocol::ClientInstanceId::default(),
                                    text: "retry that".to_owned(),
                                    images: Vec::new(),
                                }],
                            },
                        ),
                    ],
                    cx,
                );

                let state = controller.pane_state(pane).expect("the pane is registered");
                assert_eq!(state.agent_name.as_deref(), Some("Codex"));
                assert_eq!(state.session_id.as_deref(), Some("s-1"));
                assert!(state.session_capabilities.list);
                assert_eq!(state.auth_methods.len(), 1);
                assert_eq!(state.connection, AgentConnectionState::Ready);
                assert!(state.pending_permissions.is_empty());
                assert_eq!(state.queued_prompts, 0);
                let (entries, ..) = controller.pane_entries(pane).expect("entries");
                assert!(matches!(
                    entries.first(),
                    Some(AgentThreadEntry::Assistant { markdown, .. }) if markdown == "hello"
                ));
                assert!(matches!(
                    entries.get(1),
                    Some(AgentThreadEntry::Tool {
                        status: AgentToolStatusModel::Pending,
                        ..
                    })
                ));
                assert_eq!(
                    controller.take_pending_composer(pane).as_deref(),
                    Some("retry that"),
                    "a reclaimed prompt is visible in the draft again"
                );
                assert_eq!(
                    controller.viewports[&pane].last_applied, 8,
                    "the cursor follows the highest applied seq"
                );
            });
        });
    }

    #[gpui::test]
    fn restored_prompts_only_refill_their_owners_composer(cx: &mut TestAppContext) {
        let (controller, sink) = proxy_controller(cx);
        let pane = PaneId(4);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.apply_stream_items(
                    pane,
                    vec![
                        item(
                            1,
                            AgentStreamPayload::PromptsRestored {
                                reclaim_id: 1,
                                prompts: vec![zz_daemon::AgentPrompt {
                                    owner: zz_protocol::ClientInstanceId(2),
                                    text: "someone else's draft".to_owned(),
                                    images: Vec::new(),
                                }],
                            },
                        ),
                        item(
                            2,
                            AgentStreamPayload::PromptsRestored {
                                reclaim_id: 2,
                                prompts: vec![zz_daemon::AgentPrompt {
                                    owner: zz_protocol::ClientInstanceId(1),
                                    text: "my draft".to_owned(),
                                    images: Vec::new(),
                                }],
                            },
                        ),
                    ],
                    cx,
                );

                assert_eq!(
                    controller.take_pending_composer(pane).as_deref(),
                    Some("my draft")
                );
            });
        });
        assert_eq!(
            &*sink.borrow(),
            &[(
                pane,
                AgentRequest::AcknowledgePromptRestore { reclaim_id: 2 }
            )]
        );
    }

    #[gpui::test]
    fn an_item_the_client_cannot_re_type_is_dropped_without_stalling_the_cursor(
        cx: &mut TestAppContext,
    ) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(4);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.apply_stream_items(
                    pane,
                    vec![
                        item(
                            1,
                            AgentStreamPayload::Update {
                                update: serde_json::json!({"sessionUpdate": "from the future"}),
                            },
                        ),
                        item(
                            2,
                            AgentStreamPayload::Update {
                                update: json(&SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                    ContentBlock::Text(TextContent::new("still here")),
                                ))),
                            },
                        ),
                    ],
                    cx,
                );

                let (entries, ..) = controller.pane_entries(pane).expect("entries");
                assert_eq!(entries.len(), 1);
                assert_eq!(controller.viewports[&pane].last_applied, 2);
                assert_eq!(controller.panes[&pane].unknown_updates, 1);
            });
        });
    }

    #[gpui::test]
    fn an_unknown_update_between_two_known_ones_only_skips_itself(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(40);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.apply_stream_items(
                    pane,
                    vec![
                        item(1, chunk_update("first", "message-1")),
                        item(2, unknown_update("terminal_output_chunk")),
                        item(3, chunk_update("second", "message-2")),
                    ],
                    cx,
                );

                let (entries, ..) = controller.pane_entries(pane).expect("entries");
                assert_eq!(entries.len(), 2, "an unknown item poisons nothing after it");
                assert!(matches!(
                    (&entries[0], &entries[1]),
                    (
                        AgentThreadEntry::Assistant { markdown: first, .. },
                        AgentThreadEntry::Assistant { markdown: second, .. },
                    ) if first == "first" && second == "second"
                ));
                assert_eq!(controller.viewports[&pane].last_applied, 3);
                assert_eq!(controller.panes[&pane].unknown_updates, 1);
            });
        });
    }

    #[gpui::test]
    fn a_replay_over_a_surviving_transcript_reduces_each_item_once(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(44);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.apply_stream_items(
                    pane,
                    vec![
                        item(1, chunk_update("first", "message-1")),
                        item(2, chunk_update("second", "message-2")),
                    ],
                    cx,
                );

                controller.apply_stream_items(
                    pane,
                    vec![
                        item(1, chunk_update("first", "message-1")),
                        item(2, chunk_update("second", "message-2")),
                        item(3, chunk_update("third", "message-3")),
                    ],
                    cx,
                );

                let (entries, ..) = controller.pane_entries(pane).expect("entries");
                assert_eq!(
                    entries.len(),
                    3,
                    "an attach replays over the transcript it already reduced"
                );
                assert_eq!(controller.viewports[&pane].last_applied, 3);
            });
        });
    }

    #[gpui::test]
    fn an_unknown_update_mid_message_does_not_break_its_coalescing(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(41);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.apply_stream_items(
                    pane,
                    vec![
                        item(1, chunk_update("hello ", "message-1")),
                        item(2, unknown_update("_zz_private_extension")),
                        item(3, chunk_update("world", "message-1")),
                    ],
                    cx,
                );

                let (entries, ..) = controller.pane_entries(pane).expect("entries");
                assert_eq!(entries.len(), 1, "the message stays one entry");
                assert!(matches!(
                    &entries[0],
                    AgentThreadEntry::Assistant { markdown, .. } if markdown == "hello world"
                ));
                assert_eq!(controller.panes[&pane].unknown_updates, 1);
            });
        });
    }

    #[gpui::test]
    fn a_batch_of_only_unknown_updates_advances_the_cursor(cx: &mut TestAppContext) {
        let (controller, sink) = proxy_controller(cx);
        let pane = PaneId(42);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.apply_stream_items(
                    pane,
                    vec![
                        item(5, unknown_update("state_update")),
                        item(6, unknown_update("_vendor_thing")),
                        item(
                            7,
                            AgentStreamPayload::Update {
                                update: serde_json::json!({
                                    "sessionUpdate": "agent_message_chunk",
                                    "content": 42,
                                }),
                            },
                        ),
                    ],
                    cx,
                );

                assert!(
                    controller
                        .pane_entries(pane)
                        .is_some_and(|(entries, ..)| entries.is_empty())
                );
                assert_eq!(
                    controller.viewports[&pane].last_applied, 7,
                    "every seq counts as consumed, so nothing asks to be replayed"
                );
                assert_eq!(controller.panes[&pane].unknown_updates, 3);
            });
        });
        assert!(
            sink.borrow().is_empty(),
            "a batch zz skipped whole is not a hole in the journal"
        );
    }

    #[gpui::test]
    fn a_restored_session_replays_around_updates_this_build_cannot_read(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(43);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.apply_stream_items(
                    pane,
                    vec![item(
                        1,
                        AgentStreamPayload::SessionSwitched {
                            session_id: "s-2".to_owned(),
                            cwd: PathBuf::from("/workspace"),
                            modes: None,
                            config_options: None,
                            replay: vec![
                                json(&SessionUpdate::AgentMessageChunk(
                                    ContentChunk::new(ContentBlock::Text(TextContent::new(
                                        "restored",
                                    )))
                                    .message_id(MessageId::new("message-1")),
                                )),
                                serde_json::json!({"sessionUpdate": "plan_update", "planId": "p1"}),
                                json(&SessionUpdate::AgentMessageChunk(
                                    ContentChunk::new(ContentBlock::Text(TextContent::new(
                                        " and whole",
                                    )))
                                    .message_id(MessageId::new("message-1")),
                                )),
                            ],
                        },
                    )],
                    cx,
                );

                let state = controller.pane_state(pane).expect("pane state");
                assert_eq!(state.session_id.as_deref(), Some("s-2"));
                assert_eq!(state.connection, AgentConnectionState::Ready);
                let (entries, ..) = controller.pane_entries(pane).expect("entries");
                assert_eq!(entries.len(), 1);
                assert!(matches!(
                    &entries[0],
                    AgentThreadEntry::Assistant { markdown, .. } if markdown == "restored and whole"
                ));
                assert_eq!(controller.panes[&pane].unknown_updates, 1);
            });
        });
    }

    #[test]
    fn a_skipped_update_is_logged_by_tag_and_never_by_payload() {
        let payload = "x".repeat(4 * 1024);
        let update = serde_json::json!({
            "sessionUpdate": format!("_evil\n{payload}"),
            "content": payload,
        });

        let tag = session_update_tag(&update);
        assert!(tag.len() <= MAX_SESSION_UPDATE_TAG_BYTES);
        assert!(tag.starts_with("_evil"));
        assert!(!tag.contains('\n'), "a tag cannot forge a second log line");
        assert!(!tag.contains(&payload));

        let error = serde_json::from_value::<SessionUpdate>(update)
            .expect_err("an unknown discriminant does not decode");
        let excerpt = log_excerpt(&error.to_string(), MAX_DECODE_ERROR_BYTES);
        assert!(excerpt.len() <= MAX_DECODE_ERROR_BYTES);

        assert_eq!(
            session_update_tag(&serde_json::json!({"content": "no tag"})),
            UNTAGGED_SESSION_UPDATE
        );
    }

    #[gpui::test]
    fn published_pane_state_drives_the_composer_and_the_wizard(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(5);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.panes.get_mut(&pane).expect("pane").session_id = Some(Arc::from("s-9"));
                let state = AgentPaneWire {
                    phase: AgentConnectionPhase::AwaitingPermission,
                    queued_prompts: 2,
                    session_id: Some("s-9".to_owned()),
                    title: Some("ship it".to_owned()),
                    error: None,
                    auth_methods: serde_json::to_string(&vec![zz_daemon::AgentAuthMethod {
                        id: "api".to_owned(),
                        name: "API key".to_owned(),
                        description: None,
                    }])
                    .expect("auth methods encode"),
                    config_options: serde_json::to_string(&vec![SessionConfigOption::select(
                        "model",
                        "Model",
                        "gpt-5",
                        vec![SessionConfigSelectOption::new("gpt-5", "GPT-5")],
                    )])
                    .expect("config options encode"),
                    modes: String::new(),
                    pending_permission: Some(zz_protocol::AgentPermissionWire {
                        request_id: 11,
                        payload: serde_json::json!({
                            "toolCall": json(&ToolCallUpdate::new(
                                "tool-9",
                                ToolCallUpdateFields::new(),
                            )),
                            "options": json(&vec![PermissionOption::new(
                                "allow-once",
                                "Allow once",
                                PermissionOptionKind::AllowOnce,
                            )]),
                        })
                        .to_string(),
                    }),
                    git: Some(AgentGitSummary {
                        branch: Some("main".to_owned()),
                        changed_files: 3,
                        additions: 21,
                        deletions: 8,
                    }),
                };

                controller.apply_pane_state(pane, &state, cx);

                assert_eq!(controller.queued_count(pane), 2);
                let pane_state = controller.pane_state(pane).expect("the pane is registered");
                assert_eq!(pane_state.queued_prompts, 2);
                assert_eq!(pane_state.session_id.as_deref(), Some("s-9"));
                assert_eq!(pane_state.auth_methods.len(), 1);
                assert_eq!(pane_state.config_options.len(), 1);
                assert_eq!(pane_state.git, state.git);
                assert_eq!(
                    pane_state
                        .pending_permissions
                        .first()
                        .map(|request| request.request_id),
                    Some(11),
                    "a client that attaches mid-question still sees the wizard"
                );

                controller.apply_pane_state(
                    pane,
                    &AgentPaneWire {
                        phase: AgentConnectionPhase::Ready,
                        queued_prompts: 0,
                        session_id: None,
                        title: None,
                        error: None,
                        auth_methods: String::new(),
                        config_options: String::new(),
                        modes: String::new(),
                        pending_permission: None,
                        git: None,
                    },
                    cx,
                );
                let pane_state = controller.pane_state(pane).expect("the pane is registered");
                assert_eq!(pane_state.queued_prompts, 0);
                assert_eq!(pane_state.session_id.as_deref(), Some("s-9"));
                assert!(pane_state.auth_methods.is_empty());
                assert!(pane_state.config_options.is_empty());
                assert!(pane_state.pending_permissions.is_empty());
                assert_eq!(pane_state.git, None);
            });
        });
    }

    #[gpui::test]
    fn ordered_state_sync_wins_after_a_synthesized_replay(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(16);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.apply_stream_items(
                    pane,
                    vec![
                        item(1, AgentStreamPayload::SessionReset { restoring: true }),
                        item(
                            2,
                            AgentStreamPayload::SessionReady {
                                session_id: "s-1".to_owned(),
                                modes: None,
                                config_options: None,
                            },
                        ),
                        item(
                            3,
                            AgentStreamPayload::StateSynced {
                                state: AgentPaneWire {
                                    phase: AgentConnectionPhase::Failed {
                                        message: "adapter exited".to_owned(),
                                    },
                                    session_id: Some("s-1".to_owned()),
                                    ..AgentPaneWire::default()
                                },
                            },
                        ),
                    ],
                    cx,
                );

                let state = controller.pane_state(pane).expect("pane state");
                assert_eq!(state.connection, AgentConnectionState::Failed);
                assert_eq!(state.error.as_deref(), Some("adapter exited"));
                assert_eq!(controller.viewports[&pane].last_applied, 3);
            });
        });
    }

    #[gpui::test]
    fn retry_preserves_the_failure_until_the_daemon_acknowledges_restart(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(17);
        let events = Arc::new(Mutex::new(Vec::new()));
        cx.update(|cx| {
            let seen = Arc::clone(&events);
            cx.subscribe(&controller, move |_, event: &AgentControllerEvent, _| {
                if let AgentControllerEvent::Restart { pane } = event {
                    seen.lock().push(*pane);
                }
            })
            .detach();
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                let thread = controller.panes.get_mut(&pane).expect("pane");
                thread.connection = AgentConnectionState::Failed;
                thread.error = Some(Arc::from("could not spawn adapter"));
                thread.session_history.loading = true;
                controller.retry(pane, cx);
                controller.retry(pane, cx);

                let state = controller.pane_state(pane).expect("pane state");
                assert_eq!(state.connection, AgentConnectionState::Failed);
                assert_eq!(state.error.as_deref(), Some("could not spawn adapter"));
                assert!(!state.session_history.loading);
                assert!(state.lifecycle_pending);
            });
        });
        assert_eq!(&*events.lock(), &[pane]);
    }

    #[gpui::test]
    fn provider_switch_waits_for_the_daemon_descriptor(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(20);
        let events = Arc::new(Mutex::new(Vec::new()));
        cx.update(|cx| {
            let seen = Arc::clone(&events);
            cx.subscribe(&controller, move |_, event: &AgentControllerEvent, _| {
                if let AgentControllerEvent::Provider { pane, provider } = event {
                    seen.lock().push((*pane, *provider));
                }
            })
            .detach();
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                let thread = controller.panes.get_mut(&pane).expect("pane");
                thread.connection = AgentConnectionState::Failed;
                thread.error = Some(Arc::from("codex failed"));

                controller
                    .select_provider(pane, AgentProvider::ClaudeCode, cx)
                    .expect("provider request");
                controller
                    .select_provider(pane, AgentProvider::ClaudeCode, cx)
                    .expect("duplicate provider request");
                let pending = controller.pane_state(pane).expect("pane state");
                assert_eq!(pending.provider, AgentProvider::Codex);
                assert_eq!(pending.connection, AgentConnectionState::Failed);
                assert_eq!(pending.error.as_deref(), Some("codex failed"));
                assert!(pending.lifecycle_pending);

                controller.ensure_pane(
                    pane,
                    &AgentDescriptor {
                        provider: AgentProvider::ClaudeCode,
                        cwd: Some(PathBuf::from("/workspace")),
                        session_id: None,
                    },
                    cx,
                );
                let acknowledged = controller.pane_state(pane).expect("pane state");
                assert_eq!(acknowledged.provider, AgentProvider::ClaudeCode);
                assert_eq!(acknowledged.connection, AgentConnectionState::Starting);
                assert!(acknowledged.error.is_none());
                assert!(!acknowledged.lifecycle_pending);
            });
        });
        assert_eq!(&*events.lock(), &[(pane, AgentProvider::ClaudeCode)]);
    }

    #[gpui::test]
    fn session_descriptor_waits_for_the_ordered_switch_boundary(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(24);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                let thread = controller.panes.get_mut(&pane).expect("pane");
                thread.session_id = Some(Arc::from("old-session"));
                thread.cwd = PathBuf::from("/old");
                thread.apply_update(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new("old transcript")),
                )));

                controller.ensure_pane(
                    pane,
                    &AgentDescriptor {
                        provider: AgentProvider::Codex,
                        cwd: Some(PathBuf::from("/new")),
                        session_id: Some("new-session".to_owned()),
                    },
                    cx,
                );

                let thread = controller.panes.get(&pane).expect("pane");
                assert_eq!(thread.session_id.as_deref(), Some("old-session"));
                assert_eq!(thread.cwd, PathBuf::from("/old"));
                assert_eq!(thread.entries.len(), 1);
            });
        });
    }

    #[gpui::test]
    fn new_session_waits_for_the_ordered_switch_boundary(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(25);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.panes.get_mut(&pane).expect("pane").session_id =
                    Some(Arc::from("old-session"));

                controller.new_session(pane, cx).expect("session request");
                controller.apply_pane_state(
                    pane,
                    &AgentPaneWire {
                        phase: AgentConnectionPhase::Ready,
                        session_id: Some("new-session".to_owned()),
                        ..AgentPaneWire::default()
                    },
                    cx,
                );

                let pending = controller.pane_state(pane).expect("pane state");
                assert_eq!(pending.connection, AgentConnectionState::Starting);
                assert_eq!(pending.session_id.as_deref(), Some("old-session"));

                controller.handle_runtime_event(
                    pane,
                    RuntimeEvent::SessionSwitched {
                        pane,
                        session_id: "new-session".to_owned(),
                        cwd: PathBuf::from("/workspace"),
                        modes: None,
                        config_options: None,
                        replay: Vec::new(),
                    },
                    cx,
                );

                let ready = controller.pane_state(pane).expect("pane state");
                assert_eq!(ready.connection, AgentConnectionState::Ready);
                assert_eq!(ready.session_id.as_deref(), Some("new-session"));
            });
        });
    }

    #[gpui::test]
    fn nonfatal_wire_error_survives_the_ordered_ready_state(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(21);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller.handle_runtime_event(
                    pane,
                    RuntimeEvent::SessionSwitchFailed {
                        pane,
                        message: "could not restore".to_owned(),
                    },
                    cx,
                );
                controller.apply_pane_state(
                    pane,
                    &AgentPaneWire {
                        phase: AgentConnectionPhase::Ready,
                        error: Some("could not restore".to_owned()),
                        ..AgentPaneWire::default()
                    },
                    cx,
                );
                assert_eq!(
                    controller.pane_state(pane).and_then(|state| state.error),
                    Some(Arc::from("could not restore"))
                );
            });
        });
    }

    #[gpui::test]
    fn fatal_state_fails_inflight_tools(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(22);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                let thread = controller.panes.get_mut(&pane).expect("pane");
                thread.apply_update(SessionUpdate::ToolCall(
                    ToolCall::new("task-tool", "Research")
                        .kind(ToolKind::Think)
                        .status(ToolCallStatus::InProgress),
                ));
                controller.apply_pane_state(
                    pane,
                    &AgentPaneWire {
                        phase: AgentConnectionPhase::Failed {
                            message: "adapter exited".to_owned(),
                        },
                        ..AgentPaneWire::default()
                    },
                    cx,
                );
                let thread = controller.panes.get(&pane).expect("pane");
                assert!(matches!(
                    thread.entries.first(),
                    Some(AgentThreadEntry::Tool {
                        status: AgentToolStatusModel::Failed,
                        ..
                    })
                ));
            });
        });
    }

    #[gpui::test]
    fn a_prompt_reaches_the_wire_with_normalized_attachments(cx: &mut TestAppContext) {
        let (controller, sink) = proxy_controller(cx);
        let pane = PaneId(6);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller
                    .prompt(pane, "  ship it  ", vec![attachment()], cx)
                    .expect("the prompt is accepted");
            });
        });

        assert_eq!(
            &*sink.borrow(),
            &[(
                pane,
                AgentRequest::Prompt {
                    text: "ship it".to_owned(),
                    images: vec![zz_protocol::AgentImage {
                        format: "image/png".to_owned(),
                        data: PNG_PIXEL.to_vec(),
                    }],
                }
            )],
            "the daemon receives bytes plus format, never a gpui image"
        );
    }

    #[gpui::test]
    fn a_prompt_typed_mid_turn_is_queued_by_the_daemon(cx: &mut TestAppContext) {
        let (controller, sink) = proxy_controller(cx);
        let pane = PaneId(7);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller
                    .prompt(pane, "first", Vec::new(), cx)
                    .expect("first prompt");
                controller
                    .prompt(pane, "second", Vec::new(), cx)
                    .expect("second prompt is queued rather than refused");

                assert_eq!(controller.queued_count(pane), 1);
                controller.unqueue_prompts(pane, cx);
            });
        });

        assert_eq!(
            sink.borrow()
                .iter()
                .map(|(_, request)| request.clone())
                .collect::<Vec<_>>(),
            vec![
                AgentRequest::Prompt {
                    text: "first".to_owned(),
                    images: Vec::new(),
                },
                AgentRequest::Prompt {
                    text: "second".to_owned(),
                    images: Vec::new(),
                },
                AgentRequest::Unqueue,
            ]
        );
    }

    #[gpui::test]
    fn session_controls_send_the_daemon_every_filter_and_restore_path(cx: &mut TestAppContext) {
        let (controller, sink) = proxy_controller(cx);
        let pane = PaneId(15);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                let thread = controller.panes.get_mut(&pane).expect("pane");
                thread.cwd = PathBuf::from("/workspace");
                thread.session_capabilities.list = true;
                thread.session_capabilities.load = true;

                controller
                    .list_sessions(pane, false, false, cx)
                    .expect("list this workspace");
                let thread = controller.panes.get_mut(&pane).expect("pane");
                thread.session_history.loading = false;
                thread.session_history.next_cursor = Some(Arc::from("page-2"));
                controller
                    .list_sessions(pane, true, true, cx)
                    .expect("continue all workspaces");

                controller
                    .switch_session(
                        pane,
                        AgentSessionSummary {
                            session_id: "restored".to_owned(),
                            cwd: PathBuf::from("/other"),
                            additional_directories: vec![PathBuf::from("/shared")],
                            title: None,
                            updated_at: None,
                        },
                        cx,
                    )
                    .expect("restore session");
                controller.panes.get_mut(&pane).expect("pane").connection =
                    AgentConnectionState::Ready;
                controller
                    .set_working_directory(pane, Path::new("/new-workspace"), cx)
                    .expect("start in another workspace");
            });
        });

        assert_eq!(
            sink.borrow()
                .iter()
                .map(|(_, request)| request.clone())
                .collect::<Vec<_>>(),
            vec![
                AgentRequest::SessionOp {
                    op: AgentSessionOpKind::List {
                        cwd: Some(PathBuf::from("/workspace")),
                        cursor: None,
                        replace: true,
                    },
                },
                AgentRequest::SessionOp {
                    op: AgentSessionOpKind::List {
                        cwd: None,
                        cursor: Some("page-2".to_owned()),
                        replace: false,
                    },
                },
                AgentRequest::SessionOp {
                    op: AgentSessionOpKind::Switch {
                        session_id: "restored".to_owned(),
                        cwd: PathBuf::from("/other"),
                        additional_directories: vec![PathBuf::from("/shared")],
                    },
                },
                AgentRequest::SessionOp {
                    op: AgentSessionOpKind::New {
                        cwd: PathBuf::from("/new-workspace"),
                    },
                },
            ]
        );
    }

    #[gpui::test]
    fn a_settings_acknowledgement_is_paired_with_the_origin_that_asked(cx: &mut TestAppContext) {
        let (controller, sink) = proxy_controller(cx);
        let pane = PaneId(9);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                if let Some(thread) = controller.panes.get_mut(&pane) {
                    thread.config_options = vec![select_option(
                        "model",
                        AgentConfigCategory::Model,
                        "small",
                        "large",
                    )]
                    .into();
                }
                controller
                    .set_config_option(pane, "model", "large", cx)
                    .expect("the option is advertised");
                assert!(controller.panes[&pane].settings_busy);

                controller.apply_stream_items(
                    pane,
                    vec![item(
                        1,
                        AgentStreamPayload::ConfigOptionsChanged {
                            option_id: "model".to_owned(),
                            value: "large".to_owned(),
                            config_options: json(&Vec::<SessionConfigOption>::new()),
                        },
                    )],
                    cx,
                );

                assert!(!controller.panes[&pane].settings_busy);
                assert!(controller.viewports[&pane].pending_setting.is_none());
            });
        });

        assert_eq!(
            sink.borrow()
                .iter()
                .map(|(_, request)| request.clone())
                .collect::<Vec<_>>(),
            vec![AgentRequest::SetConfigOption {
                option_id: "model".to_owned(),
                value: "large".to_owned(),
            }]
        );
    }

    #[gpui::test]
    fn the_first_prompt_of_a_session_names_the_pane_once(cx: &mut TestAppContext) {
        let pane = PaneId(10);
        let titles = Arc::new(Mutex::new(Vec::new()));
        let (controller, _sink) = proxy_controller(cx);
        cx.update(|cx| {
            let sink = titles.clone();
            cx.subscribe(&controller, move |_, event: &AgentControllerEvent, _| {
                if let AgentControllerEvent::Title { title, .. } = event {
                    sink.lock().push(title.to_string());
                }
            })
            .detach();
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller
                    .prompt(pane, "  \"Fix the flaky daemon test\"  ", Vec::new(), cx)
                    .expect("first prompt");
                controller.apply_stream_items(
                    pane,
                    vec![item(1, turn_finished(StopReason::EndTurn))],
                    cx,
                );
                controller
                    .prompt(pane, "now do the other thing", Vec::new(), cx)
                    .expect("second prompt");
            });
        });
        cx.run_until_parked();

        assert_eq!(
            &*titles.lock(),
            &["Fix the flaky daemon test".to_owned()],
            "only the prompt that opened the session names the pane"
        );
    }

    #[test]
    fn derived_titles_drop_dressing_and_stay_short() {
        let title = |prompt: &str| derive_pane_title(prompt).map(|title| title.to_string());

        assert_eq!(
            title("fix the flaky daemon test"),
            Some("fix the flaky daemon test".to_owned())
        );
        assert_eq!(
            title("\"quoted request\""),
            Some("quoted request".to_owned())
        );
        assert_eq!(
            title("## Heading style prompt"),
            Some("Heading style prompt".to_owned())
        );
        assert_eq!(
            title("> quoted line\nand the rest of the message"),
            Some("quoted line".to_owned())
        );
        assert_eq!(
            title("one two three four five six seven eight nine"),
            Some("one two three four five six seven".to_owned())
        );
        assert_eq!(
            title("修复终端里的宽字符换行问题 and then some"),
            Some("修复终端里的宽字符换行问题 and then some".to_owned())
        );
        assert_eq!(
            title(&"の".repeat(80)).map(|title| title.chars().count()),
            Some(MAX_TITLE_CHARS),
            "the cap counts characters, never bytes"
        );
        assert_eq!(title("   \n   "), None);
        assert_eq!(title(""), None);
    }
}
