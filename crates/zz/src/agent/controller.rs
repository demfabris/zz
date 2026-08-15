use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
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
use async_channel::Sender;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gpui::{App, Context, Entity, EventEmitter, Image, ImageFormat, Task};
use zz_daemon::{
    AgentAuthMethod as StreamAuthMethod, AgentPromptOutcome, AgentStreamItem, AgentStreamPayload,
    AgentTurnDiffOutcome, SdkTaskEvent, TurnDiff,
};
use zz_protocol::{
    AgentConnectionPhase, AgentDescriptor, AgentPaneWire, AgentProvider, AgentSessionOpKind,
    MAX_AGENT_AUTH_METHODS, MAX_AGENT_AVAILABLE_COMMANDS, MAX_AGENT_CONFIG_CHOICES,
    MAX_AGENT_CONFIG_OPTIONS, MAX_AGENT_MODES, MAX_AGENT_PERMISSION_OPTIONS,
    MAX_AGENT_QUEUED_PROMPTS, MAX_AGENT_SESSION_DIRECTORIES, MAX_AGENT_TOOL_CONTENT_ITEMS,
    MAX_GUI_TEXT_BYTES, PaneId,
};

use crate::{
    agent::attachment,
    agent::preferences::{AgentPreferenceKind, AgentPreferences},
    agent::profile::{
        CodexCollaboration, MemoryCitation, Segment, TaskNotification, codex_collab_label,
        codex_collaboration, codex_subagent_activity, codex_tool_subagent,
        format_codex_collaboration, scan_text,
    },
    agent::sound::AgentPaneStatus,
    config::AgentConfig,
    mux::client::{AgentRequest, MuxClient},
};

const LEGACY_MODE_PREFERENCE_ID: &str = "legacy-session-mode";
const MAX_SESSION_ID_BYTES: usize = 16 * 1024;
const MAX_SESSION_TITLE_BYTES: usize = 4 * 1024;
const MAX_SESSION_TIMESTAMP_BYTES: usize = 256;
const MAX_SESSION_CURSOR_BYTES: usize = 16 * 1024;
/// Tool payloads live in the thread for as long as the pane does, so the
/// reducer caps what it keeps: agents happily emit multi-megabyte outputs.
const MAX_TOOL_PAYLOAD_BYTES: usize = 512 * 1024;
const MAX_DIFF_SIDE_BYTES: usize = 1024 * 1024;
const TRUNCATION_MARKER: &str = "… [truncated]";
/// A derived pane title is the opening words of the first prompt: enough to
/// tell agent panes apart in the tree without wrapping the pane header.
const MAX_TITLE_WORDS: usize = 7;
const MAX_TITLE_CHARS: usize = 48;
const TURN_DIFF_TIMEOUT: Duration = Duration::from_secs(30);
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
        memory_citations: Vec<MemoryCitation>,
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
        subagent: bool,
        children: Vec<AgentThreadEntry>,
    },
    Plan {
        id: u64,
        markdown: String,
    },
    Notification {
        id: u64,
        task_id: String,
        tool_use_id: String,
        status: String,
        summary: String,
        result_markdown: String,
    },
}

impl AgentThreadEntry {
    pub(crate) const fn id(&self) -> u64 {
        match self {
            Self::User { id, .. }
            | Self::Assistant { id, .. }
            | Self::Reasoning { id, .. }
            | Self::Tool { id, .. }
            | Self::Plan { id, .. }
            | Self::Notification { id, .. } => *id,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentCommandKind {
    Skill,
    Command,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_hint: Option<String>,
    pub(crate) kind: AgentCommandKind,
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
    child_entry_revisions: HashMap<u64, u64>,
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
    text_carries: BTreeMap<(StreamRole, Option<String>), String>,
    active_text_stream: Option<(StreamRole, Option<String>)>,
    tool_entries: HashMap<String, u64>,
    structured_tool_outputs: BTreeSet<String>,
    plan_entry: Option<u64>,
    child_message_entries: BTreeMap<(String, StreamRole, String), u64>,
    child_active_streams: HashMap<String, (StreamRole, u64)>,
    child_text_carries: BTreeMap<(String, StreamRole, Option<String>), String>,
    child_active_text_streams: HashMap<String, (StreamRole, Option<String>)>,
    child_tool_entries: HashMap<String, u64>,
    child_tool_roots: HashMap<String, String>,
    child_structured_tool_outputs: BTreeSet<String>,
    child_plan_entries: HashMap<String, u64>,
    live_task_tools: HashMap<String, String>,
    task_labels: HashMap<String, String>,
    active_context_compaction: Option<String>,
    suppress_user_echo: bool,
    /// Set once the session's first prompt named the pane. A title is derived
    /// exactly once per session, so a later rename is never overwritten.
    auto_titled: bool,
    /// When the pane last heard from the agent. Stamped by the reducer, which
    /// sees every runtime event, so the quiesce watchdog only ever reads it.
    last_activity: Instant,
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
            child_entry_revisions: HashMap::new(),
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
            text_carries: BTreeMap::new(),
            active_text_stream: None,
            tool_entries: HashMap::new(),
            structured_tool_outputs: BTreeSet::new(),
            plan_entry: None,
            child_message_entries: BTreeMap::new(),
            child_active_streams: HashMap::new(),
            child_text_carries: BTreeMap::new(),
            child_active_text_streams: HashMap::new(),
            child_tool_entries: HashMap::new(),
            child_tool_roots: HashMap::new(),
            child_structured_tool_outputs: BTreeSet::new(),
            child_plan_entries: HashMap::new(),
            live_task_tools: HashMap::new(),
            task_labels: HashMap::new(),
            active_context_compaction: None,
            suppress_user_echo: false,
            auto_titled: false,
            last_activity: Instant::now(),
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
        self.child_entry_revisions.clear();
        self.pending_permissions = Arc::from([]);
        self.error = None;
        self.title = None;
        self.mode = None;
        self.modes = Arc::from([]);
        self.config_options = Arc::from([]);
        self.available_commands = Arc::from([]);
        self.usage = None;
        self.session_history.loading = false;
        self.settings_busy = false;
        self.preference_reconcile_skips.clear();
        self.next_entry_id = 1;
        self.message_entries.clear();
        self.active_stream = None;
        self.text_carries.clear();
        self.active_text_stream = None;
        self.tool_entries.clear();
        self.structured_tool_outputs.clear();
        self.plan_entry = None;
        self.child_message_entries.clear();
        self.child_active_streams.clear();
        self.child_text_carries.clear();
        self.child_active_text_streams.clear();
        self.child_tool_entries.clear();
        self.child_tool_roots.clear();
        self.child_structured_tool_outputs.clear();
        self.child_plan_entries.clear();
        self.live_task_tools.clear();
        self.task_labels.clear();
        self.active_context_compaction = None;
        self.suppress_user_echo = false;
        self.auto_titled = false;
        self.last_activity = Instant::now();
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

    fn touch_child_entry(&mut self, id: u64) {
        let revision = self.allocate_entry_revision();
        self.child_entry_revisions.insert(id, revision);
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
        self.last_activity = Instant::now();
    }

    fn apply_update(&mut self, update: SessionUpdate) {
        let parent_tool_use_id = (self.provider == AgentProvider::ClaudeCode)
            .then(|| session_update_parent_tool_use_id(&update))
            .flatten()
            .map(str::to_owned);
        if let Some(parent_tool_use_id) = parent_tool_use_id
            && let Some(root_tool_id) = self.child_root_for_parent(&parent_tool_use_id)
        {
            self.child_tool_roots
                .insert(parent_tool_use_id.clone(), root_tool_id.clone());
            self.apply_child_update(&root_tool_id, &parent_tool_use_id, update);
            return;
        }
        self.apply_flat_update(update);
    }

    fn apply_flat_update(&mut self, update: SessionUpdate) {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => {
                self.apply_profiled_chunk(StreamRole::User, chunk);
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                self.settle_context_compaction();
                self.apply_profiled_chunk(StreamRole::Assistant, chunk);
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                self.flush_active_text_stream();
                self.append_chunk(StreamRole::Reasoning, chunk);
            }
            SessionUpdate::ToolCall(tool) => {
                self.flush_active_text_stream();
                self.active_stream = None;
                self.upsert_tool(tool);
            }
            SessionUpdate::ToolCallUpdate(update) => {
                self.flush_active_text_stream();
                self.active_stream = None;
                self.apply_tool_update(update);
            }
            SessionUpdate::Plan(plan) => {
                self.flush_active_text_stream();
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

    fn child_root_for_parent(&self, parent_tool_use_id: &str) -> Option<String> {
        self.child_tool_roots
            .get(parent_tool_use_id)
            .cloned()
            .or_else(|| {
                self.tool_entries
                    .contains_key(parent_tool_use_id)
                    .then(|| parent_tool_use_id.to_owned())
            })
    }

    fn apply_profiled_chunk(&mut self, role: StreamRole, chunk: ContentChunk) {
        if !matches!(&chunk.content, ContentBlock::Text(_)) {
            self.flush_active_text_stream();
            if role != StreamRole::User || !self.suppress_user_echo {
                self.append_chunk(role, chunk);
            }
            return;
        }

        let message_id = chunk
            .message_id
            .as_ref()
            .map(|message_id| message_id.0.to_string());
        let stream_key = (role, message_id.clone());
        if self.active_text_stream.as_ref() != Some(&stream_key) {
            self.flush_active_text_stream();
            self.active_text_stream = Some(stream_key.clone());
        }
        let mut carry = self.text_carries.remove(&stream_key).unwrap_or_default();
        let text = content_block_markdown(&chunk.content);
        let segments = scan_text(self.provider, &text, &mut carry);
        if !carry.is_empty() {
            self.text_carries.insert(stream_key, carry);
        }

        for segment in segments {
            match segment {
                Segment::Clean(markdown) => {
                    self.append_profiled_clean(role, message_id.clone(), &markdown);
                }
                Segment::Notification(notification) => {
                    self.push_notification(notification);
                    self.break_message_stream(role, message_id.as_deref());
                }
                Segment::Stripped {
                    kind,
                    memory_citations,
                } => {
                    if !memory_citations.is_empty() {
                        self.attach_memory_citations(role, message_id.as_deref(), memory_citations);
                    }
                    log::trace!(
                        target: "zz::agent::profile",
                        "stripped {kind} artifact from {:?} agent stream",
                        self.provider
                    );
                }
            }
        }
    }

    fn append_profiled_clean(
        &mut self,
        role: StreamRole,
        message_id: Option<String>,
        markdown: &str,
    ) {
        if role == StreamRole::User && self.suppress_user_echo {
            return;
        }
        let (markdown, images) = if role == StreamRole::User {
            split_inline_images(markdown)
        } else {
            (markdown.to_owned(), Vec::new())
        };
        self.append_stream_content(role, message_id, &markdown, images);
    }

    fn apply_task_event(&mut self, event: SdkTaskEvent) {
        match event {
            SdkTaskEvent::Started {
                task_id,
                tool_use_id,
                is_agent,
            } => {
                if !is_agent {
                    return;
                }
                self.live_task_tools.insert(task_id, tool_use_id.clone());
                self.set_task_tool_status(&tool_use_id, AgentToolStatusModel::Running);
            }
            SdkTaskEvent::Notification(notification) => {
                self.settle_task(&notification.task_id, &notification.status);
                self.push_notification(notification);
            }
            SdkTaskEvent::Settled { task_id, status } => {
                self.settle_task(&task_id, &status);
            }
        }
    }

    fn apply_codex_collaboration(&mut self, protocol_id: &str, collab: &CodexCollaboration) {
        if matches!(collab.tool.as_str(), "spawnAgent" | "resumeAgent") {
            for thread in &collab.receiver_thread_ids {
                self.live_task_tools
                    .insert(thread.clone(), protocol_id.to_owned());
                if let Some(prompt) = collab
                    .prompt
                    .as_deref()
                    .map(str::trim)
                    .filter(|prompt| !prompt.is_empty())
                {
                    let snippet: String = prompt
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .take(60)
                        .collect();
                    self.task_labels.entry(thread.clone()).or_insert(snippet);
                }
            }
            if !collab.receiver_thread_ids.is_empty() {
                self.set_task_tool_status(protocol_id, AgentToolStatusModel::Running);
            }
        }
        for (thread, state) in &collab.agents_states {
            match state.status.as_str() {
                "completed" | "errored" => {
                    self.settle_task(thread, &state.status);
                    let verb = if state.status == "errored" {
                        "failed"
                    } else {
                        "finished"
                    };
                    let subject = self.task_labels.get(thread).map_or_else(
                        || format!("Subagent {verb}"),
                        |label| format!("Agent \"{label}\" {verb}"),
                    );
                    let summary = match state
                        .message
                        .as_deref()
                        .map(str::trim)
                        .filter(|message| !message.is_empty())
                    {
                        Some(message) => format!("{subject} \u{2014} {message}"),
                        None => subject,
                    };
                    self.push_notification(TaskNotification {
                        task_id: thread.clone(),
                        tool_use_id: thread.clone(),
                        agent_task: true,
                        status: if state.status == "errored" {
                            "failed".to_owned()
                        } else {
                            state.status.clone()
                        },
                        summary,
                        result_markdown: String::new(),
                    });
                }
                "interrupted" | "shutdown" | "notFound" => {
                    self.settle_task(thread, "interrupted");
                }
                _ => {}
            }
        }
    }

    fn settle_task(&mut self, task_id: &str, status: &str) {
        let Some(tool_use_id) = self.live_task_tools.remove(task_id) else {
            return;
        };
        let settled = match status {
            "failed" | "killed" | "errored" => AgentToolStatusModel::Failed,
            "interrupted" => AgentToolStatusModel::Canceled,
            _ => AgentToolStatusModel::Completed,
        };
        self.set_task_tool_status(&tool_use_id, settled);
    }

    fn set_task_tool_status(&mut self, protocol_id: &str, next: AgentToolStatusModel) {
        let Some(entry_id) = self.tool_entries.get(protocol_id).copied() else {
            return;
        };
        let Some(index) = self.entry_index(entry_id) else {
            return;
        };
        let changed = if let AgentThreadEntry::Tool { status, .. } = &mut self.entries[index]
            && *status != next
        {
            *status = next;
            true
        } else {
            false
        };
        if changed {
            self.touch_entry(index);
        }
    }

    fn tool_status_held(&self, protocol_id: &str) -> bool {
        self.live_task_tools
            .values()
            .any(|tool| tool == protocol_id)
    }

    fn push_notification(&mut self, notification: TaskNotification) {
        if !notification.agent_task {
            log::trace!(
                target: "zz::agent",
                "skipping non-agent task notification: {}",
                notification.summary
            );
            return;
        }
        let existing = (!notification.tool_use_id.is_empty()).then(|| {
            self.entries.iter().position(|entry| {
                matches!(
                    entry,
                    AgentThreadEntry::Notification { tool_use_id, .. }
                        if *tool_use_id == notification.tool_use_id
                )
            })
        });
        if let Some(Some(index)) = existing {
            if let AgentThreadEntry::Notification {
                task_id,
                status,
                summary,
                result_markdown,
                ..
            } = &mut self.entries[index]
            {
                *task_id = notification.task_id;
                *status = notification.status;
                if !notification.summary.is_empty() {
                    *summary = notification.summary;
                }
                if !notification.result_markdown.is_empty() {
                    *result_markdown = notification.result_markdown;
                }
            }
            self.touch_entry(index);
            return;
        }
        let id = self.allocate_entry_id();
        self.push_entry(AgentThreadEntry::Notification {
            id,
            task_id: notification.task_id,
            tool_use_id: notification.tool_use_id,
            status: notification.status,
            summary: notification.summary,
            result_markdown: notification.result_markdown,
        });
    }

    fn break_message_stream(&mut self, role: StreamRole, message_id: Option<&str>) {
        if let Some(message_id) = message_id {
            self.message_entries.remove(&(role, message_id.to_owned()));
        }
        if self
            .active_stream
            .is_some_and(|(active_role, _)| active_role == role)
        {
            self.active_stream = None;
        }
    }

    fn attach_memory_citations(
        &mut self,
        role: StreamRole,
        message_id: Option<&str>,
        citations: Vec<MemoryCitation>,
    ) {
        let entry_id = message_id
            .and_then(|message_id| {
                self.message_entries
                    .get(&(role, message_id.to_owned()))
                    .copied()
            })
            .or_else(|| {
                self.active_stream
                    .filter(|(active_role, _)| *active_role == role)
                    .map(|(_, id)| id)
            })
            .or_else(|| {
                self.entries.iter().rev().find_map(|entry| match entry {
                    AgentThreadEntry::Assistant { id, .. } => Some(*id),
                    _ => None,
                })
            });
        let Some(entry_id) = entry_id else {
            return;
        };
        let Some(index) = self.entry_index(entry_id) else {
            return;
        };
        let AgentThreadEntry::Assistant {
            memory_citations, ..
        } = &mut self.entries[index]
        else {
            return;
        };
        for citation in citations {
            if !memory_citations.contains(&citation) {
                memory_citations.push(citation);
            }
        }
        self.touch_entry(index);
    }

    fn flush_active_text_stream(&mut self) {
        let Some((role, message_id)) = self.active_text_stream.take() else {
            return;
        };
        let Some(carry) = self.text_carries.remove(&(role, message_id.clone())) else {
            return;
        };
        if !carry.is_empty() {
            self.append_profiled_clean(role, message_id, &carry);
        }
    }

    fn finish_text_streams(&mut self) {
        self.flush_active_text_stream();
        let carries = std::mem::take(&mut self.text_carries);
        for ((role, message_id), carry) in carries {
            if !carry.is_empty() {
                self.append_profiled_clean(role, message_id, &carry);
            }
        }
    }

    fn apply_runtime_update(&mut self, update: SessionUpdate) {
        let settle_after_update = self.connection.accepts_prompt()
            && matches!(
                &update,
                SessionUpdate::ToolCall(_) | SessionUpdate::ToolCallUpdate(_)
            );
        self.apply_update(update);
        if settle_after_update {
            self.settle_inflight(AgentToolStatusModel::Completed);
        }
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
                AgentThreadEntry::Tool { .. }
                | AgentThreadEntry::Plan { .. }
                | AgentThreadEntry::Notification { .. } => false,
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
                memory_citations: Vec::new(),
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
        let context_compaction = context_compaction(tool.meta.as_ref());
        if context_compaction {
            self.active_context_compaction = if matches!(
                tool.status,
                ToolCallStatus::Completed | ToolCallStatus::Failed
            ) {
                None
            } else {
                Some(protocol_id.clone())
            };
        }
        let subagent = match self.provider {
            AgentProvider::ClaudeCode => claude_tool_subagent(tool.meta.as_ref()).unwrap_or(false),
            AgentProvider::Codex => codex_tool_subagent(tool.meta.as_ref()),
        };
        let collaboration = (self.provider == AgentProvider::Codex)
            .then(|| codex_collaboration(tool.meta.as_ref(), tool.raw_input.as_ref()))
            .flatten();
        if self.provider == AgentProvider::Codex
            && let Some(activity) = codex_subagent_activity(tool.meta.as_ref())
        {
            self.task_labels.insert(activity.thread_id, activity.name);
        }
        if tool.content.is_empty() {
            self.structured_tool_outputs.remove(&protocol_id);
        } else {
            self.structured_tool_outputs.insert(protocol_id.clone());
        }
        let location = tool_location(&tool);
        let input = collaboration
            .as_ref()
            .and_then(format_codex_collaboration)
            .map(ToolPayload::Text)
            .or_else(|| tool_input(&tool));
        let mut output = tool_output(&tool);
        apply_terminal_frame(
            &mut output,
            TerminalFrame::from_meta(tool.meta.as_ref(), tool.raw_input.as_ref(), Some(&self.cwd)),
            &tool.title,
        );
        let title = collaboration
            .as_ref()
            .and_then(codex_collab_label)
            .unwrap_or(tool.title);
        let held = self.tool_status_held(&protocol_id);
        if let Some(entry_id) = self.tool_entries.get(&protocol_id).copied()
            && let Some(index) = self.entry_index(entry_id)
            && let AgentThreadEntry::Tool {
                kind,
                status,
                label,
                location: entry_location,
                input: entry_input,
                output: entry_output,
                subagent: entry_subagent,
                ..
            } = &mut self.entries[index]
        {
            *kind = map_tool_kind(tool.kind);
            *status = if held {
                AgentToolStatusModel::Running
            } else {
                map_tool_status(tool.status)
            };
            *label = title;
            *entry_location = location;
            *entry_input = input;
            *entry_output = output;
            *entry_subagent = subagent;
            self.touch_entry(index);
            if subagent {
                self.child_tool_roots
                    .insert(protocol_id.clone(), protocol_id.clone());
            }
            if let Some(collaboration) = &collaboration {
                self.apply_codex_collaboration(&protocol_id, collaboration);
            }
            return;
        }
        let id = self.allocate_entry_id();
        self.tool_entries.insert(protocol_id.clone(), id);
        self.push_entry(AgentThreadEntry::Tool {
            id,
            protocol_id: protocol_id.clone(),
            kind: map_tool_kind(tool.kind),
            status: if held {
                AgentToolStatusModel::Running
            } else {
                map_tool_status(tool.status)
            },
            label: title,
            location,
            input,
            output,
            default_expanded: matches!(tool.status, ToolCallStatus::Failed),
            subagent,
            children: Vec::new(),
        });
        if subagent {
            self.child_tool_roots
                .insert(protocol_id.clone(), protocol_id.clone());
        }
        if let Some(collaboration) = &collaboration {
            self.apply_codex_collaboration(&protocol_id, collaboration);
        }
    }

    fn apply_tool_update(&mut self, update: ToolCallUpdate) {
        let protocol_id = update.tool_call_id.0.to_string();
        let context_compaction = context_compaction(update.meta.as_ref())
            || self.active_context_compaction.as_deref() == Some(protocol_id.as_str());
        let context_compaction_finished = context_compaction
            && update.fields.status.as_ref().is_some_and(|status| {
                matches!(status, ToolCallStatus::Completed | ToolCallStatus::Failed)
            });
        let subagent = match self.provider {
            AgentProvider::ClaudeCode => claude_tool_subagent(update.meta.as_ref()),
            AgentProvider::Codex => codex_tool_subagent(update.meta.as_ref()).then_some(true),
        };
        let collaboration = (self.provider == AgentProvider::Codex)
            .then(|| codex_collaboration(update.meta.as_ref(), update.fields.raw_input.as_ref()))
            .flatten();
        if self.provider == AgentProvider::Codex
            && let Some(activity) = codex_subagent_activity(update.meta.as_ref())
        {
            self.task_labels.insert(activity.thread_id, activity.name);
        }
        let terminal_frame = TerminalFrame::from_meta(
            update.meta.as_ref(),
            update.fields.raw_input.as_ref(),
            Some(&self.cwd),
        );
        let has_terminal_frame = !terminal_frame.is_empty();
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
        let held = self.tool_status_held(&protocol_id);
        let AgentThreadEntry::Tool {
            kind,
            status,
            label,
            location,
            input,
            output,
            subagent: entry_subagent,
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
            *status = if held {
                AgentToolStatusModel::Running
            } else {
                map_tool_status(next)
            };
            changed = true;
        }
        if let Some(next) = update.fields.title.filter(|title| !title.trim().is_empty()) {
            *label = collaboration
                .as_ref()
                .and_then(codex_collab_label)
                .unwrap_or(next);
            changed = true;
        }
        if let Some(next) = subagent {
            *entry_subagent = next;
            changed = true;
        }
        if let Some(raw_input) = update.fields.raw_input {
            *input = collaboration
                .as_ref()
                .and_then(format_codex_collaboration)
                .map(|text| ToolPayload::Text(capped_payload(text)))
                .or_else(|| json_payload(&raw_input));
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
            let terminal_content = content
                .iter()
                .any(|content| matches!(content, ToolCallContent::Terminal(_)));
            let structured = tool_content_payloads(&content);
            if structured.is_empty() {
                self.structured_tool_outputs.remove(&protocol_id);
            } else {
                self.structured_tool_outputs.insert(protocol_id.clone());
            }
            if !(terminal_content && (has_terminal_frame || has_terminal_payload(output))) {
                *output = if structured.is_empty() {
                    raw_output.into_iter().collect()
                } else {
                    structured
                };
            }
            changed = true;
        } else if let Some(raw_output) = raw_output {
            if !had_structured_output && !has_terminal_frame && !has_terminal_payload(output) {
                *output = vec![raw_output];
            }
            changed = true;
        }
        if apply_terminal_frame(output, terminal_frame, label) {
            changed = true;
        }
        if changed {
            self.touch_entry(index);
        }
        if context_compaction_finished {
            self.active_context_compaction = None;
        } else if context_compaction {
            self.active_context_compaction = Some(protocol_id.clone());
        }
        if subagent == Some(true) {
            self.child_tool_roots
                .insert(protocol_id.clone(), protocol_id.clone());
        }
        if let Some(collaboration) = &collaboration {
            self.apply_codex_collaboration(&protocol_id, collaboration);
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

    fn apply_child_update(
        &mut self,
        root_tool_id: &str,
        parent_tool_use_id: &str,
        update: SessionUpdate,
    ) {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => {
                self.apply_child_profiled_chunk(
                    root_tool_id,
                    parent_tool_use_id,
                    StreamRole::User,
                    chunk,
                );
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                self.apply_child_profiled_chunk(
                    root_tool_id,
                    parent_tool_use_id,
                    StreamRole::Assistant,
                    chunk,
                );
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                self.flush_child_active_text_stream(root_tool_id, parent_tool_use_id);
                self.append_child_chunk(
                    root_tool_id,
                    parent_tool_use_id,
                    StreamRole::Reasoning,
                    chunk,
                );
            }
            SessionUpdate::ToolCall(tool) => {
                self.flush_child_active_text_stream(root_tool_id, parent_tool_use_id);
                self.child_active_streams.remove(parent_tool_use_id);
                self.upsert_child_tool(root_tool_id, tool);
            }
            SessionUpdate::ToolCallUpdate(update) => {
                self.flush_child_active_text_stream(root_tool_id, parent_tool_use_id);
                self.child_active_streams.remove(parent_tool_use_id);
                self.apply_child_tool_update(root_tool_id, update);
            }
            SessionUpdate::Plan(plan) => {
                self.flush_child_active_text_stream(root_tool_id, parent_tool_use_id);
                self.child_active_streams.remove(parent_tool_use_id);
                self.apply_child_plan(root_tool_id, &plan);
            }
            _ => {}
        }
    }

    fn apply_child_profiled_chunk(
        &mut self,
        root_tool_id: &str,
        parent_tool_use_id: &str,
        role: StreamRole,
        chunk: ContentChunk,
    ) {
        if !matches!(&chunk.content, ContentBlock::Text(_)) {
            self.flush_child_active_text_stream(root_tool_id, parent_tool_use_id);
            self.append_child_chunk(root_tool_id, parent_tool_use_id, role, chunk);
            return;
        }

        let message_id = chunk
            .message_id
            .as_ref()
            .map(|message_id| message_id.0.to_string());
        let stream = (role, message_id.clone());
        if self.child_active_text_streams.get(parent_tool_use_id) != Some(&stream) {
            self.flush_child_active_text_stream(root_tool_id, parent_tool_use_id);
            self.child_active_text_streams
                .insert(parent_tool_use_id.to_owned(), stream);
        }
        let carry_key = (parent_tool_use_id.to_owned(), role, message_id.clone());
        let mut carry = self
            .child_text_carries
            .remove(&carry_key)
            .unwrap_or_default();
        let text = content_block_markdown(&chunk.content);
        let segments = scan_text(self.provider, &text, &mut carry);
        if !carry.is_empty() {
            self.child_text_carries.insert(carry_key, carry);
        }

        for segment in segments {
            match segment {
                Segment::Clean(markdown) => self.append_child_stream_content(
                    root_tool_id,
                    parent_tool_use_id,
                    role,
                    message_id.clone(),
                    &markdown,
                    Vec::new(),
                ),
                Segment::Notification(notification) => {
                    self.push_child_notification(root_tool_id, notification);
                    self.break_child_message_stream(
                        parent_tool_use_id,
                        role,
                        message_id.as_deref(),
                    );
                }
                Segment::Stripped { kind, .. } => {
                    log::trace!(
                        target: "zz::agent::profile",
                        "stripped {kind} artifact from nested {:?} agent stream",
                        self.provider
                    );
                }
            }
        }
    }

    fn append_child_chunk(
        &mut self,
        root_tool_id: &str,
        parent_tool_use_id: &str,
        role: StreamRole,
        chunk: ContentChunk,
    ) {
        let (markdown, images) = match (role, &chunk.content) {
            (StreamRole::User, ContentBlock::Image(image)) => match inbound_image(image) {
                Some(image) => (String::new(), vec![image]),
                None => (content_block_markdown(&chunk.content), Vec::new()),
            },
            (StreamRole::User, content) => split_inline_images(&content_block_markdown(content)),
            (_, content) => (content_block_markdown(content), Vec::new()),
        };
        let message_id = chunk.message_id.map(|message_id| message_id.0.to_string());
        self.append_child_stream_content(
            root_tool_id,
            parent_tool_use_id,
            role,
            message_id,
            &markdown,
            images,
        );
    }

    fn append_child_stream_content(
        &mut self,
        root_tool_id: &str,
        parent_tool_use_id: &str,
        role: StreamRole,
        message_id: Option<String>,
        markdown: &str,
        images: Vec<Arc<Image>>,
    ) {
        if markdown.is_empty() && images.is_empty() {
            return;
        }
        let message_key =
            message_id.map(|message_id| (parent_tool_use_id.to_owned(), role, message_id));
        let entry_id = if let Some(key) = message_key.as_ref() {
            if let Some(id) = self.child_message_entries.get(key).copied() {
                id
            } else {
                let id = self.push_child_stream_entry(root_tool_id, role);
                if id == 0 {
                    return;
                }
                self.child_message_entries.insert(key.clone(), id);
                id
            }
        } else if let Some((active_role, id)) =
            self.child_active_streams.get(parent_tool_use_id).copied()
        {
            if active_role == role {
                id
            } else {
                self.push_child_stream_entry(root_tool_id, role)
            }
        } else {
            self.push_child_stream_entry(root_tool_id, role)
        };
        if entry_id == 0 {
            return;
        }
        self.child_active_streams
            .insert(parent_tool_use_id.to_owned(), (role, entry_id));
        let Some((root_index, child_index)) = self.child_entry_position(root_tool_id, entry_id)
        else {
            return;
        };
        let changed = match &mut self.entries[root_index] {
            AgentThreadEntry::Tool { children, .. } => match &mut children[child_index] {
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
                AgentThreadEntry::Tool { .. }
                | AgentThreadEntry::Plan { .. }
                | AgentThreadEntry::Notification { .. } => false,
            },
            _ => false,
        };
        if changed {
            self.touch_child_entry(entry_id);
            self.touch_entry(root_index);
        }
    }

    fn push_child_stream_entry(&mut self, root_tool_id: &str, role: StreamRole) -> u64 {
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
                memory_citations: Vec::new(),
            },
            StreamRole::Reasoning => AgentThreadEntry::Reasoning {
                id,
                label: "Reasoning".to_owned(),
                markdown: String::new(),
                default_expanded: false,
            },
        };
        if self.push_child_entry(root_tool_id, entry) {
            id
        } else {
            0
        }
    }

    fn push_child_notification(&mut self, root_tool_id: &str, notification: TaskNotification) {
        let id = self.allocate_entry_id();
        self.push_child_entry(
            root_tool_id,
            AgentThreadEntry::Notification {
                id,
                task_id: notification.task_id,
                tool_use_id: notification.tool_use_id,
                status: notification.status,
                summary: notification.summary,
                result_markdown: notification.result_markdown,
            },
        );
    }

    fn break_child_message_stream(
        &mut self,
        parent_tool_use_id: &str,
        role: StreamRole,
        message_id: Option<&str>,
    ) {
        if let Some(message_id) = message_id {
            self.child_message_entries.remove(&(
                parent_tool_use_id.to_owned(),
                role,
                message_id.to_owned(),
            ));
        }
        if self
            .child_active_streams
            .get(parent_tool_use_id)
            .is_some_and(|(active_role, _)| *active_role == role)
        {
            self.child_active_streams.remove(parent_tool_use_id);
        }
    }

    fn flush_child_active_text_stream(&mut self, root_tool_id: &str, parent_tool_use_id: &str) {
        let Some((role, message_id)) = self.child_active_text_streams.remove(parent_tool_use_id)
        else {
            return;
        };
        let carry_key = (parent_tool_use_id.to_owned(), role, message_id.clone());
        let Some(carry) = self.child_text_carries.remove(&carry_key) else {
            return;
        };
        if !carry.is_empty() {
            self.append_child_stream_content(
                root_tool_id,
                parent_tool_use_id,
                role,
                message_id,
                &carry,
                Vec::new(),
            );
        }
    }

    fn finish_child_text_streams(&mut self) {
        let parents = self
            .child_active_text_streams
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for parent_tool_use_id in parents {
            if let Some(root_tool_id) = self.child_root_for_parent(&parent_tool_use_id) {
                self.flush_child_active_text_stream(&root_tool_id, &parent_tool_use_id);
            }
        }
        let carries = std::mem::take(&mut self.child_text_carries);
        for ((parent_tool_use_id, role, message_id), carry) in carries {
            if carry.is_empty() {
                continue;
            }
            if let Some(root_tool_id) = self.child_root_for_parent(&parent_tool_use_id) {
                self.append_child_stream_content(
                    &root_tool_id,
                    &parent_tool_use_id,
                    role,
                    message_id,
                    &carry,
                    Vec::new(),
                );
            }
        }
    }

    fn upsert_child_tool(&mut self, root_tool_id: &str, tool: ToolCall) {
        let protocol_id = tool.tool_call_id.0.to_string();
        let subagent = self.provider == AgentProvider::ClaudeCode
            && claude_tool_subagent(tool.meta.as_ref()).unwrap_or(false);
        if tool.content.is_empty() {
            self.child_structured_tool_outputs.remove(&protocol_id);
        } else {
            self.child_structured_tool_outputs
                .insert(protocol_id.clone());
        }
        let location = tool_location(&tool);
        let input = tool_input(&tool);
        let mut output = tool_output(&tool);
        apply_terminal_frame(
            &mut output,
            TerminalFrame::from_meta(tool.meta.as_ref(), tool.raw_input.as_ref(), Some(&self.cwd)),
            &tool.title,
        );
        if let Some(entry_id) = self.child_tool_entries.get(&protocol_id).copied()
            && let Some((root_index, child_index)) =
                self.child_entry_position(root_tool_id, entry_id)
            && let AgentThreadEntry::Tool { children, .. } = &mut self.entries[root_index]
            && let AgentThreadEntry::Tool {
                kind,
                status,
                label,
                location: entry_location,
                input: entry_input,
                output: entry_output,
                subagent: entry_subagent,
                ..
            } = &mut children[child_index]
        {
            *kind = map_tool_kind(tool.kind);
            *status = map_tool_status(tool.status);
            *label = tool.title;
            *entry_location = location;
            *entry_input = input;
            *entry_output = output;
            *entry_subagent = subagent;
            self.touch_child_entry(entry_id);
            self.touch_entry(root_index);
            self.child_tool_roots
                .insert(protocol_id, root_tool_id.to_owned());
            return;
        }
        let id = self.allocate_entry_id();
        self.child_tool_entries.insert(protocol_id.clone(), id);
        self.child_tool_roots
            .insert(protocol_id.clone(), root_tool_id.to_owned());
        self.push_child_entry(
            root_tool_id,
            AgentThreadEntry::Tool {
                id,
                protocol_id,
                kind: map_tool_kind(tool.kind),
                status: map_tool_status(tool.status),
                label: tool.title,
                location,
                input,
                output,
                default_expanded: matches!(tool.status, ToolCallStatus::Failed),
                subagent,
                children: Vec::new(),
            },
        );
    }

    fn apply_child_tool_update(&mut self, root_tool_id: &str, update: ToolCallUpdate) {
        let protocol_id = update.tool_call_id.0.to_string();
        let subagent = (self.provider == AgentProvider::ClaudeCode)
            .then(|| claude_tool_subagent(update.meta.as_ref()))
            .flatten();
        let terminal_frame = TerminalFrame::from_meta(
            update.meta.as_ref(),
            update.fields.raw_input.as_ref(),
            Some(&self.cwd),
        );
        let has_terminal_frame = !terminal_frame.is_empty();
        let carries_shape = update_carries_tool_shape(&update.fields);
        let Some(entry_id) = self.child_tool_entries.get(&protocol_id).copied() else {
            if let Ok(tool) = ToolCall::try_from(update) {
                self.upsert_child_tool(root_tool_id, tool);
            }
            return;
        };
        let Some((root_index, child_index)) = self.child_entry_position(root_tool_id, entry_id)
        else {
            return;
        };
        let had_structured_output = self.child_structured_tool_outputs.contains(&protocol_id);
        let AgentThreadEntry::Tool { children, .. } = &mut self.entries[root_index] else {
            return;
        };
        let AgentThreadEntry::Tool {
            kind,
            status,
            label,
            location,
            input,
            output,
            subagent: entry_subagent,
            ..
        } = &mut children[child_index]
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
        if let Some(next) = subagent {
            *entry_subagent = next;
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
            let terminal_content = content
                .iter()
                .any(|content| matches!(content, ToolCallContent::Terminal(_)));
            let structured = tool_content_payloads(&content);
            if structured.is_empty() {
                self.child_structured_tool_outputs.remove(&protocol_id);
            } else {
                self.child_structured_tool_outputs
                    .insert(protocol_id.clone());
            }
            if !(terminal_content && (has_terminal_frame || has_terminal_payload(output))) {
                *output = if structured.is_empty() {
                    raw_output.into_iter().collect()
                } else {
                    structured
                };
            }
            changed = true;
        } else if let Some(raw_output) = raw_output {
            if !had_structured_output && !has_terminal_frame && !has_terminal_payload(output) {
                *output = vec![raw_output];
            }
            changed = true;
        }
        if apply_terminal_frame(output, terminal_frame, label) {
            changed = true;
        }
        if changed {
            self.touch_child_entry(entry_id);
            self.touch_entry(root_index);
        }
        self.child_tool_roots
            .insert(protocol_id, root_tool_id.to_owned());
    }

    fn apply_child_plan(&mut self, root_tool_id: &str, plan: &Plan) {
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
        if let Some(id) = self.child_plan_entries.get(root_tool_id).copied()
            && let Some((root_index, child_index)) = self.child_entry_position(root_tool_id, id)
            && let AgentThreadEntry::Tool { children, .. } = &mut self.entries[root_index]
            && let AgentThreadEntry::Plan {
                markdown: current, ..
            } = &mut children[child_index]
        {
            if *current != markdown {
                *current = markdown;
                self.touch_child_entry(id);
                self.touch_entry(root_index);
            }
            return;
        }
        let id = self.allocate_entry_id();
        self.child_plan_entries.insert(root_tool_id.to_owned(), id);
        self.push_child_entry(root_tool_id, AgentThreadEntry::Plan { id, markdown });
    }

    fn push_child_entry(&mut self, root_tool_id: &str, entry: AgentThreadEntry) -> bool {
        let Some(root_index) = self.root_tool_index(root_tool_id) else {
            return false;
        };
        let entry_id = entry.id();
        let AgentThreadEntry::Tool { children, .. } = &mut self.entries[root_index] else {
            return false;
        };
        children.push(entry);
        self.touch_child_entry(entry_id);
        self.touch_entry(root_index);
        true
    }

    fn root_tool_index(&self, root_tool_id: &str) -> Option<usize> {
        self.tool_entries
            .get(root_tool_id)
            .and_then(|id| self.entry_index(*id))
    }

    fn child_entry_position(&self, root_tool_id: &str, entry_id: u64) -> Option<(usize, usize)> {
        let root_index = self.root_tool_index(root_tool_id)?;
        let AgentThreadEntry::Tool { children, .. } = &self.entries[root_index] else {
            return None;
        };
        let child_index = children.iter().position(|entry| entry.id() == entry_id)?;
        Some((root_index, child_index))
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

    fn settle_context_compaction(&mut self) {
        let Some(protocol_id) = self.active_context_compaction.take() else {
            return;
        };
        let Some(entry_id) = self.tool_entries.get(&protocol_id).copied() else {
            return;
        };
        let Some(index) = self.entry_index(entry_id) else {
            return;
        };
        let changed = if let AgentThreadEntry::Tool { status, label, .. } = &mut self.entries[index]
        {
            let mut changed = false;
            if matches!(
                status,
                AgentToolStatusModel::Pending | AgentToolStatusModel::Running
            ) {
                *status = AgentToolStatusModel::Completed;
                changed = true;
            }
            if label == "Context compacting" {
                "Context compacted".clone_into(label);
                changed = true;
            }
            changed
        } else {
            false
        };
        if changed {
            self.touch_entry(index);
        }
    }

    /// What the quiesce watchdog refuses to park through: a permission the user
    /// still owes an answer to (a question is one), a subagent task still
    /// reporting, or any tool call the agent has not resolved.
    /// Finalize a turn the agent went quiet on without touching the child: the
    /// streaming entries settle, the pane accepts prompts again, and any later
    /// output opens a fresh segment because `active_stream` is cleared.
    fn park_turn(&mut self) {
        self.settle_inflight(AgentToolStatusModel::Completed);
        self.connection = AgentConnectionState::Ready;
    }

    fn settle_inflight(&mut self, settled_status: AgentToolStatusModel) {
        debug_assert!(matches!(
            settled_status,
            AgentToolStatusModel::Completed
                | AgentToolStatusModel::Failed
                | AgentToolStatusModel::Canceled
        ));
        self.finish_text_streams();
        self.finish_child_text_streams();
        if settled_status == AgentToolStatusModel::Completed {
            self.settle_context_compaction();
        } else {
            self.active_context_compaction = None;
        }
        self.pending_permissions = Arc::from([]);
        self.suppress_user_echo = false;
        self.active_stream = None;
        let held: std::collections::HashSet<String> =
            self.live_task_tools.values().cloned().collect();
        for index in 0..self.entries.len() {
            let mut changed_children = Vec::new();
            let changed = if let AgentThreadEntry::Tool {
                protocol_id,
                status,
                children,
                ..
            } = &mut self.entries[index]
            {
                if held.contains(protocol_id.as_str()) {
                    continue;
                }
                let mut changed = false;
                if matches!(
                    status,
                    AgentToolStatusModel::Pending
                        | AgentToolStatusModel::Running
                        | AgentToolStatusModel::NeedsApproval
                ) {
                    *status = settled_status;
                    changed = true;
                }
                for child in children {
                    if let AgentThreadEntry::Tool { id, status, .. } = child
                        && matches!(
                            status,
                            AgentToolStatusModel::Pending
                                | AgentToolStatusModel::Running
                                | AgentToolStatusModel::NeedsApproval
                        )
                    {
                        *status = settled_status;
                        changed_children.push(*id);
                        changed = true;
                    }
                }
                changed
            } else {
                false
            };
            for id in changed_children {
                self.touch_child_entry(id);
            }
            if changed {
                self.touch_entry(index);
            }
        }
    }

    fn fail_inflight(&mut self) {
        self.live_task_tools.clear();
        self.settle_inflight(AgentToolStatusModel::Failed);
    }

    fn finish_replay(&mut self) {
        self.finish_text_streams();
        self.finish_child_text_streams();
        self.dedupe_replayed_assistant_entries();
        self.message_entries.clear();
        self.active_stream = None;
    }

    fn dedupe_replayed_assistant_entries(&mut self) {
        let mut index = 1;
        while index < self.entries.len() {
            let duplicate_citations = match (&self.entries[index - 1], &self.entries[index]) {
                (
                    AgentThreadEntry::Assistant {
                        markdown: previous, ..
                    },
                    AgentThreadEntry::Assistant {
                        markdown: current,
                        memory_citations,
                        ..
                    },
                ) if previous == current => Some(memory_citations.clone()),
                _ => None,
            };
            let Some(citations) = duplicate_citations else {
                index += 1;
                continue;
            };

            if let AgentThreadEntry::Assistant {
                memory_citations, ..
            } = &mut self.entries[index - 1]
            {
                for citation in citations {
                    if !memory_citations.contains(&citation) {
                        memory_citations.push(citation);
                    }
                }
            }
            self.entries.remove(index);
            self.entry_revisions.remove(index);
            self.entry_changes.clear();
            self.entry_change_floor = self.next_entry_revision;
        }

        self.entry_indices.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            self.entry_indices.insert(entry.id(), index);
        }
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

/// The reducer's input shape. The daemon owns the adapter now, so every one of
/// these is translated from an [`AgentStreamPayload`] the wire carried; the
/// enum stays because it is what [`AgentThread`] has always been fed.
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
    TaskEvent {
        pane: PaneId,
        event: SdkTaskEvent,
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
    TurnStarted {
        pane: PaneId,
        turn_id: u64,
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
    /// The daemon settled a turn that went quiet past its quiesce window.
    Parked {
        pane: PaneId,
    },
    /// Prompts the daemon queued and is handing back, so the composer refills.
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

/// A pane's viewport onto the daemon-owned runtime: where its stream has been
/// applied to, and the request bookkeeping the reducer's inputs need answered.
#[derive(Default)]
struct PaneViewport {
    /// Highest stream seq handed to the reducer. The replay cursor, and the
    /// signal that this pane has anything at all to resume from.
    last_applied: u64,
    /// The setting request in flight, so the acknowledgement can be paired with
    /// the origin that asked for it: a user's pick reports failures, a sticky
    /// preference restores quietly.
    pending_setting: Option<AgentSettingRequest>,
    /// How many prompts the daemon is holding behind the live turn.
    queued_prompts: usize,
    /// Whether a turn has been dispatched, which is when the daemon takes the
    /// worktree snapshot the turn diff is measured against.
    turn_dispatched: bool,
    turn_generation: u64,
    last_turn_id: Option<u64>,
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
    /// The wire. Installed by the workspace once the mux client exists; a
    /// controller without one accepts no sends, which is how the composer
    /// reports a disconnected daemon.
    mux: Option<Entity<MuxClient>>,
    panes: BTreeMap<PaneId, AgentThread>,
    viewports: BTreeMap<PaneId, PaneViewport>,
    pending_composer: BTreeMap<PaneId, String>,
    /// Images reclaimed from queued prompts, waiting for the pane's view to
    /// re-attach them beside the reclaimed text.
    pending_images: BTreeMap<PaneId, Vec<Arc<Image>>>,
    retained_panes: BTreeSet<PaneId>,
    /// Turn-diff captures waiting on the daemon's answer, keyed by request.
    turn_diffs: BTreeMap<u64, (PaneId, u64, Sender<Result<TurnDiff, String>>)>,
    next_turn_diff_request: u64,
    shutting_down: bool,
}

impl AgentController {
    /// A controller with no wire: view tests that only need a sidebar.
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
            turn_diffs: BTreeMap::new(),
            next_turn_diff_request: 0,
            shutting_down: false,
        }
    }

    /// Point the controller at the daemon connection. Every agent request goes
    /// out through the mux client, so this is what turns the proxy on.
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

    /// How many prompts the daemon is holding behind the pane's live turn, as
    /// its last published state reported.
    pub(crate) fn queued_count(&self, pane: PaneId) -> usize {
        self.viewports
            .get(&pane)
            .map_or(0, |viewport| viewport.queued_prompts)
    }

    /// Whether the pane has a turn to diff against. The daemon snapshots the
    /// worktree at dispatch, so a pane that has never prompted has no base and
    /// a pane outside a worktree learns so from the capture itself.
    pub(crate) fn has_turn_base(&self, pane: PaneId) -> bool {
        self.viewports
            .get(&pane)
            .is_some_and(|viewport| viewport.turn_dispatched)
    }

    pub(crate) fn turn_generation(&self, pane: PaneId) -> u64 {
        self.viewports
            .get(&pane)
            .map_or(0, |viewport| viewport.turn_generation)
    }

    pub(crate) fn conversation_epoch(&self, pane: PaneId) -> u64 {
        self.viewports
            .get(&pane)
            .map_or(0, |viewport| viewport.conversation_epoch)
    }

    /// Ask the daemon to diff the pane's worktree against the base its turn
    /// started from. Git runs on the daemon's machine — which is the machine
    /// the worktree is on — and the task carries the answer back.
    pub(crate) fn capture_turn_diff(
        &mut self,
        pane: PaneId,
        cx: &App,
    ) -> Option<Task<Result<TurnDiff, String>>> {
        if !self.has_turn_base(pane) {
            return None;
        }
        self.turn_diffs
            .retain(|_, (_, _, sender)| !sender.is_closed());
        self.next_turn_diff_request = self.next_turn_diff_request.saturating_add(1);
        let request_id = self.next_turn_diff_request;
        if !self.send(pane, AgentRequest::TurnDiff { request_id }, cx) {
            return Some(Task::ready(Err("agent daemon is not connected".to_owned())));
        }
        let (sender, receiver) = async_channel::bounded(1);
        let generation = self.turn_generation(pane);
        self.turn_diffs
            .insert(request_id, (pane, generation, sender));
        let timer = cx.background_executor().timer(TURN_DIFF_TIMEOUT);
        Some(cx.background_executor().spawn(async move {
            futures_lite::future::race(
                async {
                    receiver
                        .recv()
                        .await
                        .unwrap_or_else(|_| Err("the turn diff was abandoned".to_owned()))
                },
                async {
                    timer.await;
                    Err("the turn diff timed out".to_owned())
                },
            )
            .await
        }))
    }

    /// Answer one outstanding turn-diff capture.
    fn resolve_turn_diff(
        &mut self,
        pane: PaneId,
        request_id: u64,
        outcome: Result<TurnDiff, String>,
    ) -> Option<u64> {
        if self
            .turn_diffs
            .get(&request_id)
            .is_some_and(|(request_pane, _, _)| *request_pane == pane)
            && let Some((_, generation, sender)) = self.turn_diffs.remove(&request_id)
        {
            let _ = sender.try_send(outcome);
            return Some(generation);
        }
        None
    }

    fn abandon_turn(&mut self, pane: PaneId) -> bool {
        let mut changed = false;
        if let Some(viewport) = self.viewports.get_mut(&pane) {
            changed |= viewport.turn_dispatched;
            viewport.turn_dispatched = false;
        }
        let requests = self
            .turn_diffs
            .iter()
            .filter_map(|(request, (request_pane, _, _))| {
                (*request_pane == pane).then_some(*request)
            })
            .collect::<Vec<_>>();
        for request in requests {
            if let Some((_, _, sender)) = self.turn_diffs.remove(&request) {
                changed = true;
                let _ = sender.try_send(Err("the agent session changed".to_owned()));
            }
        }
        changed
    }

    /// Which bucket one pane lands in, in the same order the fleet rollup uses.
    /// The sidebar reads this per agent pane instead of cloning whole states.
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

    pub(crate) fn pane_entries(
        &self,
        pane: PaneId,
    ) -> Option<(&[AgentThreadEntry], &[u64], &HashMap<u64, u64>, u64)> {
        self.panes.get(&pane).map(|thread| {
            (
                thread.entries.as_slice(),
                thread.entry_revisions.as_slice(),
                &thread.child_entry_revisions,
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
        if provider_changed {
            self.abandon_turn(pane);
        }
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
            viewport.last_turn_id = None;
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
            self.abandon_turn(pane);
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
        self.abandon_turn(pane);
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
        self.abandon_turn(pane);
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
        let turn_changed =
            matches!(state.phase, AgentConnectionPhase::Failed { .. }) && self.abandon_turn(pane);
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
        if turn_changed || self.pane_state(pane).as_ref() != Some(&before) {
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

    /// Reduce the answer to one `AgentTurnDiff` request.
    pub(crate) fn apply_turn_diff_result(
        &mut self,
        pane: PaneId,
        result: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(payload) = decode_state_blob::<AgentStreamPayload>(result) else {
            return;
        };
        if let AgentStreamPayload::TurnDiff {
            request_id,
            outcome,
            ..
        } = payload
        {
            let (outcome, unavailable) = match outcome {
                AgentTurnDiffOutcome::Captured { diff } => (Ok(diff), false),
                AgentTurnDiffOutcome::Unavailable { message } => (Err(message), true),
                AgentTurnDiffOutcome::Failed { message } => (Err(message), false),
            };
            let generation = self.resolve_turn_diff(pane, request_id, outcome);
            if unavailable
                && generation == Some(self.turn_generation(pane))
                && let Some(viewport) = self.viewports.get_mut(&pane)
                && std::mem::take(&mut viewport.turn_dispatched)
            {
                cx.notify();
            }
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
            AgentStreamPayload::StateSynced { .. }
            | AgentStreamPayload::TurnAbandoned { .. }
            | AgentStreamPayload::PromptAccepted { .. } => return None,
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
                replay: replay.into_iter().filter_map(decode_value).collect(),
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
                update: decode_value(update)?,
            },
            AgentStreamPayload::TaskEvent { event } => RuntimeEvent::TaskEvent { pane, event },
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
            AgentStreamPayload::TurnStarted { turn_id } => {
                RuntimeEvent::TurnStarted { pane, turn_id }
            }
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
            AgentStreamPayload::Parked => RuntimeEvent::Parked { pane },
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
            AgentStreamPayload::TurnDiff {
                request_id,
                outcome,
                ..
            } => {
                self.resolve_turn_diff(
                    pane,
                    request_id,
                    match outcome {
                        AgentTurnDiffOutcome::Captured { diff } => Ok(diff),
                        AgentTurnDiffOutcome::Unavailable { message }
                        | AgentTurnDiffOutcome::Failed { message } => Err(message),
                    },
                );
                return None;
            }
        })
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
                self.abandon_turn(pane);
                let viewport = self.viewport_mut(pane);
                viewport.conversation_epoch = viewport.conversation_epoch.saturating_add(1);
                viewport.last_turn_id = None;
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
                    thread.settle_inflight(AgentToolStatusModel::Completed);
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
                self.abandon_turn(pane);
                self.viewport_mut(pane).session_change_pending = false;
                if self
                    .panes
                    .get(&pane)
                    .is_some_and(|thread| !thread.session_reset)
                {
                    let viewport = self.viewport_mut(pane);
                    viewport.conversation_epoch = viewport.conversation_epoch.saturating_add(1);
                }
                self.viewport_mut(pane).last_turn_id = None;
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
                    thread.settle_inflight(AgentToolStatusModel::Completed);
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
            RuntimeEvent::TaskEvent { pane, event } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.apply_task_event(event);
                    changed_pane = Some(pane);
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
                let mut failed = false;
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.finish_text_streams();
                    thread.suppress_user_echo = false;
                    thread.active_stream = None;
                    match result {
                        Ok(StopReason::Cancelled) => {
                            thread.connection = AgentConnectionState::Ready;
                            thread.error = None;
                            thread.cancel_inflight();
                        }
                        Ok(_) => {
                            thread.connection = AgentConnectionState::Ready;
                            thread.error = None;
                            thread.settle_inflight(AgentToolStatusModel::Completed);
                        }
                        Err(error) => {
                            thread.connection = AgentConnectionState::Failed;
                            thread.error = Some(Arc::from(error));
                            thread.fail_inflight();
                            failed = true;
                        }
                    }
                    changed_pane = Some(pane);
                    if thread.connection.accepts_prompt() {
                        reconcile_pane = Some(pane);
                    }
                }
                if failed {
                    self.abandon_turn(pane);
                }
            }
            RuntimeEvent::TurnStarted { pane, turn_id } => {
                let viewport = self.viewport_mut(pane);
                let started = viewport.last_turn_id != Some(turn_id);
                if started {
                    viewport.last_turn_id = Some(turn_id);
                    viewport.turn_generation = viewport.turn_generation.saturating_add(1);
                }
                let had_base = viewport.turn_dispatched;
                viewport.turn_dispatched = true;
                if started || !had_base {
                    changed_pane = Some(pane);
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
                self.abandon_turn(pane);
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
            RuntimeEvent::Parked { pane } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.park_turn();
                    changed_pane = Some(pane);
                    reconcile_pane = Some(pane);
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
        if let Some(thread) = changed_pane.and_then(|pane| self.panes.get_mut(&pane)) {
            thread.last_activity = Instant::now();
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

/// Re-type one JSON value the stream carried verbatim. A payload that does not
/// survive is dropped rather than half-applied: the ACP schema is the contract
/// between the daemon and the adapter, and a shape zz cannot read is a shape it
/// has nothing to render.
fn decode_value<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Option<T> {
    serde_json::from_value(value)
        .map_err(|error| {
            log::warn!(target: "zz::agent", "dropping an agent payload zz could not re-type: {error}");
        })
        .ok()
}

fn decode_json<T: serde::de::DeserializeOwned>(value: Option<serde_json::Value>) -> Option<T> {
    value.and_then(decode_value)
}

/// Read one of [`AgentPaneWire`]'s JSON blobs. Empty means "not published".
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

/// The parked permission request rides the pane state so a client that attaches
/// mid-question still sees it.
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
    let kind = if command.name.starts_with('$') {
        AgentCommandKind::Skill
    } else {
        AgentCommandKind::Command
    };
    AgentCommand {
        name: command.name,
        description: command.description,
        input_hint,
        kind,
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

fn session_update_parent_tool_use_id(update: &SessionUpdate) -> Option<&str> {
    let meta = match update {
        SessionUpdate::UserMessageChunk(update)
        | SessionUpdate::AgentMessageChunk(update)
        | SessionUpdate::AgentThoughtChunk(update) => update.meta.as_ref(),
        SessionUpdate::ToolCall(update) => update.meta.as_ref(),
        SessionUpdate::ToolCallUpdate(update) => update.meta.as_ref(),
        SessionUpdate::Plan(update) => update.meta.as_ref(),
        SessionUpdate::AvailableCommandsUpdate(update) => update.meta.as_ref(),
        SessionUpdate::CurrentModeUpdate(update) => update.meta.as_ref(),
        SessionUpdate::ConfigOptionUpdate(update) => update.meta.as_ref(),
        SessionUpdate::SessionInfoUpdate(update) => update.meta.as_ref(),
        SessionUpdate::UsageUpdate(update) => update.meta.as_ref(),
        _ => None,
    }?;
    meta.get("claudeCode")?
        .as_object()?
        .get("parentToolUseId")?
        .as_str()
}

fn context_compaction(meta: Option<&serde_json::Map<String, serde_json::Value>>) -> bool {
    meta.and_then(|meta| meta.get("contextCompaction"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn claude_tool_subagent(meta: Option<&serde_json::Map<String, serde_json::Value>>) -> Option<bool> {
    meta?
        .get("claudeCode")?
        .as_object()?
        .get("subagent")?
        .as_bool()
}

#[derive(Default)]
struct TerminalFrame {
    info: Option<TerminalInfoFrame>,
    output: Option<String>,
    exit: Option<TerminalExitFrame>,
}

struct TerminalInfoFrame {
    terminal_id: String,
    command: Option<String>,
    cwd: Option<String>,
}

struct TerminalExitFrame {
    exit_code: Option<String>,
    signal: Option<String>,
}

impl TerminalFrame {
    fn from_meta(
        meta: Option<&serde_json::Map<String, serde_json::Value>>,
        raw_input: Option<&serde_json::Value>,
        fallback_cwd: Option<&std::path::Path>,
    ) -> Self {
        let Some(meta) = meta else {
            return Self::default();
        };
        let info = meta
            .get("terminal_info")
            .and_then(serde_json::Value::as_object)
            .and_then(|info| {
                let terminal_id = info.get("terminal_id")?.as_str()?.to_owned();
                let command = json_command(
                    info.get("command")
                        .or_else(|| raw_input.and_then(|input| input.get("command"))),
                );
                let cwd = info
                    .get("cwd")
                    .or_else(|| raw_input.and_then(|input| input.get("cwd")))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| fallback_cwd.map(|cwd| cwd.display().to_string()));
                Some(TerminalInfoFrame {
                    terminal_id,
                    command,
                    cwd,
                })
            });
        let output = meta
            .get("terminal_output")
            .and_then(serde_json::Value::as_object)
            .filter(|output| {
                output
                    .get("terminal_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
            })
            .and_then(|output| output.get("data"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let exit = meta
            .get("terminal_exit")
            .and_then(serde_json::Value::as_object)
            .filter(|exit| {
                exit.get("terminal_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
            })
            .map(|exit| TerminalExitFrame {
                exit_code: exit.get("exit_code").and_then(json_integer),
                signal: exit
                    .get("signal")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            });
        Self { info, output, exit }
    }

    const fn is_empty(&self) -> bool {
        self.info.is_none() && self.output.is_none() && self.exit.is_none()
    }
}

fn json_command(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(command) => Some(command.clone()),
        serde_json::Value::Array(arguments) => {
            let arguments = arguments
                .iter()
                .map(serde_json::Value::as_str)
                .collect::<Option<Vec<_>>>()?;
            Some(arguments.join(" "))
        }
        _ => None,
    }
}

fn json_integer(value: &serde_json::Value) -> Option<String> {
    value
        .as_i64()
        .map(|value| value.to_string())
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn has_terminal_payload(output: &[ToolPayload]) -> bool {
    output
        .iter()
        .any(|payload| matches!(payload, ToolPayload::Terminal(_)))
}

fn apply_terminal_frame(
    output: &mut Vec<ToolPayload>,
    frame: TerminalFrame,
    fallback_label: &str,
) -> bool {
    if frame.is_empty() {
        return false;
    }
    let index = output
        .iter()
        .position(|payload| matches!(payload, ToolPayload::Terminal(_)))
        .unwrap_or_else(|| {
            output.push(ToolPayload::Terminal(String::new()));
            output.len() - 1
        });
    let ToolPayload::Terminal(terminal) = &mut output[index] else {
        unreachable!();
    };
    if let Some(info) = frame.info {
        let command = info
            .command
            .as_deref()
            .and_then(command_from_title)
            .or_else(|| command_from_title(fallback_label));
        terminal.clear();
        if let Some(command) = command {
            terminal.push_str("$ ");
            terminal.push_str(&command);
            terminal.push('\n');
        }
        if let Some(cwd) = info.cwd.filter(|cwd| !cwd.trim().is_empty()) {
            terminal.push_str("cwd: ");
            terminal.push_str(cwd.trim());
            terminal.push('\n');
        }
        if terminal.is_empty() {
            terminal.push_str("[terminal ");
            terminal.push_str(&info.terminal_id);
            terminal.push_str("]\n");
        }
        terminal.push('\n');
    }
    if let Some(data) = frame.output {
        terminal.push_str(&data);
    }
    if let Some(exit) = frame.exit {
        if !terminal.is_empty() && !terminal.ends_with('\n') {
            terminal.push('\n');
        }
        terminal.push_str("[exit status: ");
        terminal.push_str(exit.exit_code.as_deref().unwrap_or("unknown"));
        if let Some(signal) = exit.signal.filter(|signal| !signal.is_empty()) {
            terminal.push_str(", signal: ");
            terminal.push_str(&signal);
        }
        terminal.push(']');
    }
    truncate_payload(terminal, MAX_TOOL_PAYLOAD_BYTES);
    true
}

/// Generic display titles agents attach before (or instead of) real arguments.
/// They are labels, not commands: rendering one as `$ Terminal` puts a lie in
/// the transcript.
const PLACEHOLDER_TOOL_TITLES: [&str; 16] = [
    "grep",
    "find",
    "terminal",
    "shell",
    "read file",
    "edit file",
    "delete file",
    "write file",
    "web search",
    "web fetch",
    "codebase search",
    "read todos",
    "update todos",
    "read lints",
    "subagent task",
    "task: subagent task",
];

fn is_placeholder_title(title: &str) -> bool {
    PLACEHOLDER_TOOL_TITLES
        .iter()
        .any(|placeholder| title.trim().eq_ignore_ascii_case(placeholder))
}

/// A title usable as a shell command: unwrapped from its markdown code span
/// when it wears one, and never a generic placeholder label.
fn command_from_title(title: &str) -> Option<String> {
    let command = unwrap_command_span(title).unwrap_or_else(|| title.trim().to_owned());
    (!command.is_empty() && !is_placeholder_title(&command)).then_some(command)
}

fn unwrap_command_span(title: &str) -> Option<String> {
    let inner = title.trim().strip_prefix('`')?.strip_suffix('`')?;
    let mut command = String::with_capacity(inner.len());
    let mut characters = inner.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.peek() == Some(&'`') {
            characters.next();
            command.push('`');
        } else {
            command.push(character);
        }
    }
    Some(command.trim().to_owned())
}

/// Does this update carry tool *shape* (what the call is) rather than only its
/// result? Status-and-output updates arrive after the shape is settled, so they
/// must not re-type the call.
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

/// A completion update that repeats the default `other` kind would otherwise
/// downgrade an already-typed tool into a generic one.
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

/// Cut `text` to `max_bytes` on a char boundary, leaving a visible marker. The
/// marker is budgeted inside the cap, so the result never exceeds it.
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
    fn context_compaction_completion_stops_its_spinner() {
        let mut thread = thread();
        let meta = claude_meta(&serde_json::json!({"contextCompaction": true}));
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("compact-1", "Context compacting")
                .status(ToolCallStatus::InProgress)
                .meta(meta.clone()),
        ));
        thread.apply_update(SessionUpdate::ToolCallUpdate(
            ToolCallUpdate::new(
                "compact-1",
                ToolCallUpdateFields::new()
                    .title("Context compacted")
                    .status(ToolCallStatus::Completed),
            )
            .meta(meta),
        ));

        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Completed,
                label,
                ..
            } if label == "Context compacted"
        ));
        assert!(thread.active_context_compaction.is_none());
    }

    #[test]
    fn following_answer_settles_a_compaction_with_a_lost_completion() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("compact-1", "Context compacting")
                .status(ToolCallStatus::InProgress)
                .meta(claude_meta(&serde_json::json!({"contextCompaction": true}))),
        ));
        thread.apply_update(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("The answer")),
        )));

        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Completed,
                label,
                ..
            } if label == "Context compacted"
        ));
        assert!(matches!(
            &thread.entries[1],
            AgentThreadEntry::Assistant { markdown, .. } if markdown == "The answer"
        ));
    }

    #[test]
    fn reducer_keeps_large_markdown_blocks_in_one_protocol_message() {
        let mut thread = thread();
        let opening = format!("```text\n{}", "long line\n".repeat(8_000));
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(opening.clone())))
                .message_id(MessageId::new("large-message")),
        ));
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new("```")))
                .message_id(MessageId::new("large-message")),
        ));

        assert_eq!(thread.entries.len(), 1);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Assistant { markdown, .. }
                if markdown == &format!("{opening}```")
        ));
    }

    #[test]
    fn reducer_routes_mid_turn_notification_despite_user_echo_suppression() {
        let mut thread =
            AgentThread::new(AgentProvider::ClaudeCode, PathBuf::from("/workspace"), None);
        thread.begin_prompt("run the task".to_owned(), Vec::new());
        thread.apply_update(SessionUpdate::UserMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new(
                r#"<task-notification>
<task-id>a27cef9442328a156</task-id>
<tool-use-id>toolu_01U1bVbZb8V55Vx3eLB9Zaxb</tool-use-id>
<output-file>/private/tmp/claude-501/.../tasks/a27cef9442328a156.output</output-file>
<status>completed</status>
<summary>Agent "Fix zz-daemon/zz-terminal slop findings" finished</summary>
<note>A task-notification fires each time this agent stops...</note>
<result>All 12 findings addressed. ...</result>
</task-notification>"#,
            )),
        )));

        assert_eq!(thread.entries.len(), 2);
        assert!(matches!(thread.entries[0], AgentThreadEntry::User { .. }));
        assert!(matches!(
            &thread.entries[1],
            AgentThreadEntry::Notification {
                task_id,
                status,
                summary,
                result_markdown,
                ..
            } if task_id == "a27cef9442328a156"
                && status == "completed"
                && summary == "Agent \"Fix zz-daemon/zz-terminal slop findings\" finished"
                && result_markdown == "All 12 findings addressed. ..."
        ));
    }

    #[test]
    fn live_sdk_notification_and_replayed_envelope_share_one_card() {
        let mut thread =
            AgentThread::new(AgentProvider::ClaudeCode, PathBuf::from("/workspace"), None);
        thread.push_notification(TaskNotification {
            task_id: "a27cef9442328a156".to_owned(),
            tool_use_id: "toolu_01U1bVbZb8V55Vx3eLB9Zaxb".to_owned(),
            agent_task: true,
            status: "completed".to_owned(),
            summary: "Agent finished".to_owned(),
            result_markdown: String::new(),
        });
        thread.push_notification(TaskNotification {
            task_id: "a27cef9442328a156".to_owned(),
            tool_use_id: "toolu_01U1bVbZb8V55Vx3eLB9Zaxb".to_owned(),
            agent_task: true,
            status: "completed".to_owned(),
            summary: "Agent \"Fix findings\" finished".to_owned(),
            result_markdown: "All 12 findings addressed.".to_owned(),
        });

        assert_eq!(thread.entries.len(), 1);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Notification {
                summary,
                result_markdown,
                ..
            } if summary == "Agent \"Fix findings\" finished"
                && result_markdown == "All 12 findings addressed."
        ));

        thread.push_notification(TaskNotification {
            task_id: "other".to_owned(),
            tool_use_id: "toolu_other".to_owned(),
            agent_task: true,
            status: "completed".to_owned(),
            summary: "Another agent finished".to_owned(),
            result_markdown: String::new(),
        });
        assert_eq!(thread.entries.len(), 2);
    }

    #[test]
    fn codex_spawned_agent_holds_the_spawn_row_and_cards_on_completion() {
        let mut thread = AgentThread::new(AgentProvider::Codex, PathBuf::from("/workspace"), None);
        let collab_meta =
            |value: serde_json::Value| value.as_object().expect("collab meta object").clone();

        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("spawn-1", "spawnAgent")
                .status(ToolCallStatus::Completed)
                .raw_input(serde_json::json!({"prompt": "sleep 5s and return done"}))
                .meta(collab_meta(serde_json::json!({
                    "codex": {"collaboration": {
                        "tool": "spawnAgent",
                        "receiverThreadIds": ["thread-1"],
                    }},
                }))),
        ));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Running,
                subagent: true,
                label,
                ..
            } if label == "Spawn subagent \u{2014} sleep 5s and return done"
        ));

        thread.settle_inflight(AgentToolStatusModel::Completed);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Running,
                ..
            }
        ));

        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("act-1", "Start subagent Pauli")
                .status(ToolCallStatus::Completed)
                .meta(collab_meta(serde_json::json!({
                    "codex": {"subagent": {
                        "threadId": "thread-1",
                        "path": "agents/Pauli",
                        "activity": "started",
                    }},
                }))),
        ));

        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("wait-1", "wait")
                .status(ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "agentsStates": {"thread-1": {"status": "completed", "message": "done"}},
                }))
                .meta(collab_meta(serde_json::json!({
                    "codex": {"collaboration": {
                        "tool": "wait",
                        "receiverThreadIds": ["thread-1"],
                    }},
                }))),
        ));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Completed,
                ..
            }
        ));
        assert!(matches!(
            &thread.entries[2],
            AgentThreadEntry::Tool {
                label,
                subagent: false,
                input: Some(ToolPayload::Text(text)),
                ..
            } if label == "Wait for subagents" && text == "agent thread-1\u{2026} completed: done"
        ));
        assert!(matches!(
            &thread.entries[3],
            AgentThreadEntry::Notification { summary, task_id, .. }
                if summary == "Agent \"Pauli\" finished \u{2014} done" && task_id == "thread-1"
        ));

        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "wait-1",
            ToolCallUpdateFields::new().raw_input(serde_json::json!({
                "agentsStates": {"thread-1": {"status": "completed", "message": "done"}},
            })),
        )));
        assert_eq!(
            thread
                .entries
                .iter()
                .filter(|entry| matches!(entry, AgentThreadEntry::Notification { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn shell_task_notifications_do_not_become_cards() {
        let mut thread =
            AgentThread::new(AgentProvider::ClaudeCode, PathBuf::from("/workspace"), None);
        thread.push_notification(TaskNotification {
            task_id: "bckbkz157".to_owned(),
            tool_use_id: "toolu_01TLXpbaj3Mw71nx3e1VbHVS".to_owned(),
            agent_task: false,
            status: "completed".to_owned(),
            summary: "sleep 5".to_owned(),
            result_markdown: String::new(),
        });
        assert!(thread.entries.is_empty());
    }

    fn claude_meta(value: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        value
            .as_object()
            .expect("fixture metadata must be an object")
            .clone()
    }

    #[test]
    fn reducer_attaches_parented_subagent_updates_to_the_task_tool() {
        let mut thread =
            AgentThread::new(AgentProvider::ClaudeCode, PathBuf::from("/workspace"), None);
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("task-tool", "Research")
                .kind(ToolKind::Think)
                .status(ToolCallStatus::InProgress)
                .meta(claude_meta(&serde_json::json!({
                    "claudeCode": {"subagent": true}
                }))),
        ));
        let first_revision = thread.entry_revisions[0];
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new("nested answer"))).meta(
                claude_meta(&serde_json::json!({
                    "claudeCode": {"parentToolUseId": "task-tool"}
                })),
            ),
        ));

        assert_eq!(thread.entries.len(), 1);
        assert!(thread.entry_revisions[0] > first_revision);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                subagent: true,
                children,
                ..
            } if matches!(
                children.as_slice(),
                [AgentThreadEntry::Assistant { markdown, .. }] if markdown == "nested answer"
            )
        ));
    }

    #[test]
    fn async_subagent_tool_stays_running_until_its_task_settles() {
        let mut thread =
            AgentThread::new(AgentProvider::ClaudeCode, PathBuf::from("/workspace"), None);
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("task-tool", "Sleep 5 then return done")
                .kind(ToolKind::Think)
                .status(ToolCallStatus::InProgress)
                .meta(claude_meta(&serde_json::json!({
                    "claudeCode": {"subagent": true}
                }))),
        ));
        thread.apply_task_event(SdkTaskEvent::Started {
            task_id: "a5ee3d5032432ddf6".to_owned(),
            tool_use_id: "task-tool".to_owned(),
            is_agent: true,
        });
        thread.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "task-tool",
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        )));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Running,
                ..
            }
        ));

        thread.settle_inflight(AgentToolStatusModel::Completed);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Running,
                ..
            }
        ));

        thread.apply_task_event(SdkTaskEvent::Notification(TaskNotification {
            task_id: "a5ee3d5032432ddf6".to_owned(),
            tool_use_id: "task-tool".to_owned(),
            agent_task: true,
            status: "completed".to_owned(),
            summary: "done".to_owned(),
            result_markdown: String::new(),
        }));
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Completed,
                ..
            }
        ));
        assert!(matches!(
            &thread.entries[1],
            AgentThreadEntry::Notification { summary, .. } if summary == "done"
        ));

        thread.apply_task_event(SdkTaskEvent::Started {
            task_id: "bc5stcnro".to_owned(),
            tool_use_id: "bash-tool".to_owned(),
            is_agent: false,
        });
        assert!(thread.live_task_tools.is_empty());
    }

    #[test]
    fn reducer_keeps_orphaned_subagent_updates_in_the_flat_timeline() {
        let mut thread =
            AgentThread::new(AgentProvider::ClaudeCode, PathBuf::from("/workspace"), None);
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new("orphan answer"))).meta(
                claude_meta(&serde_json::json!({
                    "claudeCode": {"parentToolUseId": "missing-task"}
                })),
            ),
        ));

        assert!(matches!(
            thread.entries.as_slice(),
            [AgentThreadEntry::Assistant { markdown, .. }] if markdown == "orphan answer"
        ));
    }

    #[test]
    fn reducer_flattens_deeper_subagent_updates_into_the_root_task() {
        let mut thread =
            AgentThread::new(AgentProvider::ClaudeCode, PathBuf::from("/workspace"), None);
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("root-task", "Root task").meta(claude_meta(&serde_json::json!({
                "claudeCode": {"subagent": true}
            }))),
        ));
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("nested-task", "Nested task").meta(claude_meta(&serde_json::json!({
                "claudeCode": {
                    "subagent": true,
                    "parentToolUseId": "root-task"
                }
            }))),
        ));
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new("deep answer"))).meta(
                claude_meta(&serde_json::json!({
                    "claudeCode": {"parentToolUseId": "nested-task"}
                })),
            ),
        ));

        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool { children, .. }
                if matches!(
                    children.as_slice(),
                    [
                        AgentThreadEntry::Tool {
                            children: nested_children,
                            ..
                        },
                        AgentThreadEntry::Assistant { markdown, .. },
                    ] if nested_children.is_empty() && markdown == "deep answer"
                )
        ));
    }

    #[test]
    fn replay_dedupes_adjacent_sanitized_assistant_answers_only_at_finish() {
        let mut thread = thread();
        let answer = concat!(
            "Committed as `fa0c328f4`.\n\n",
            "::git-stage{cwd=\"/Users/demfabris/Documents/Development/Clairvo/backend\"}\n",
            "::git-commit{cwd=\"/Users/demfabris/Documents/Development/Clairvo/backend\"}\n\n",
        );
        let raw = format!(
            "{answer}<oai-mem-citation>\n\
             <citation_entries>\n\
             MEMORY.md:872-886|note=[used scoped commit guidance for a shared dirty tree]\n\
             MEMORY.md:899-904|note=[used explicit and partial staging guidance]\n\
             </citation_entries>\n\
             <rollout_ids>\n\
             019f7ff0-6773-7c13-9ef5-a46d634a02ff\n\
             </rollout_ids>\n\
             </oai-mem-citation>"
        );
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(answer)))
                .message_id(MessageId::new("sanitized")),
        ));
        thread.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(raw)))
                .message_id(MessageId::new("raw")),
        ));

        assert_eq!(thread.entries.len(), 2, "live reduction never dedupes");
        thread.finish_replay();

        assert_eq!(thread.entries.len(), 1);
        assert_eq!(thread.entries.len(), thread.entry_revisions.len());
        assert_eq!(thread.entry_indices.len(), 1);
        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Assistant {
                markdown,
                memory_citations,
                ..
            } if markdown.starts_with("Committed as `fa0c328f4`.")
                && !markdown.contains("::git-")
                && memory_citations.len() == 2
                && memory_citations[0].path == "MEMORY.md"
        ));
    }

    #[test]
    fn reducer_tracks_agent_commands_skills_and_generic_config_options() {
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
                AvailableCommand::new("$brainstorm", "Explore an idea"),
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
                kind: AgentCommandKind::Command,
                input_hint: Some(hint),
                ..
            }, AgentCommand {
                kind: AgentCommandKind::Skill,
                ..
            }] if hint == "optional instructions"
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
    fn turn_boundaries_settle_tools_without_terminal_updates() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-running", "Run tests").status(ToolCallStatus::InProgress),
        ));
        thread.apply_update(SessionUpdate::ToolCall(ToolCall::new(
            "tool-pending",
            "Read file",
        )));
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-failed", "Edit file").status(ToolCallStatus::Failed),
        ));

        thread.settle_inflight(AgentToolStatusModel::Completed);

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
            [
                AgentToolStatusModel::Completed,
                AgentToolStatusModel::Completed,
                AgentToolStatusModel::Failed,
            ]
        );

        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-error", "Fetch resource").status(ToolCallStatus::InProgress),
        ));
        thread.settle_inflight(AgentToolStatusModel::Failed);
        assert!(matches!(
            thread.entries.last(),
            Some(AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Failed,
                ..
            })
        ));

        thread.connection = AgentConnectionState::Ready;
        thread.apply_runtime_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-late-replay", "Read replayed file")
                .status(ToolCallStatus::InProgress),
        ));
        assert!(matches!(
            thread.entries.last(),
            Some(AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Completed,
                ..
            })
        ));

        thread.connection = AgentConnectionState::Running;
        thread.apply_runtime_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-live", "Read live file").status(ToolCallStatus::InProgress),
        ));
        assert!(matches!(
            thread.entries.last(),
            Some(AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Running,
                ..
            })
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
    fn terminal_frames_append_output_and_record_exit_status() {
        let mut thread = thread();
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("terminal-tool", "cargo test")
                .kind(ToolKind::Execute)
                .status(ToolCallStatus::InProgress)
                .raw_input(serde_json::json!({"command": "cargo test"}))
                .content(vec![ToolCallContent::Terminal(Terminal::new(
                    "terminal-tool",
                ))])
                .meta(claude_meta(&serde_json::json!({
                    "terminal_info": {
                        "terminal_id": "terminal-tool"
                    }
                }))),
        ));
        for data in ["first line\n", "second line"] {
            thread.apply_update(SessionUpdate::ToolCallUpdate(
                ToolCallUpdate::new("terminal-tool", ToolCallUpdateFields::new()).meta(
                    claude_meta(&serde_json::json!({
                        "terminal_output": {
                            "terminal_id": "terminal-tool",
                            "data": data
                        }
                    })),
                ),
            ));
        }
        thread.apply_update(SessionUpdate::ToolCallUpdate(
            ToolCallUpdate::new(
                "terminal-tool",
                ToolCallUpdateFields::new()
                    .status(ToolCallStatus::Completed)
                    .content(vec![ToolCallContent::Terminal(Terminal::new(
                        "terminal-tool",
                    ))]),
            )
            .meta(claude_meta(&serde_json::json!({
                "terminal_exit": {
                    "terminal_id": "terminal-tool",
                    "exit_code": 0,
                    "signal": null
                }
            }))),
        ));

        assert!(matches!(
            &thread.entries[0],
            AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Completed,
                output,
                ..
            } if matches!(
                output.as_slice(),
                [ToolPayload::Terminal(terminal)]
                    if terminal
                        == "$ cargo test\ncwd: /workspace\n\nfirst line\nsecond line\n[exit status: 0]"
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
    fn placeholder_titles_never_become_terminal_commands() {
        let frame = || {
            TerminalFrame::from_meta(
                Some(&claude_meta(&serde_json::json!({
                    "terminal_info": { "terminal_id": "term-1" }
                }))),
                None,
                None,
            )
        };
        let terminal = |label: &str| {
            let mut output = Vec::new();
            assert!(apply_terminal_frame(&mut output, frame(), label));
            match output.as_slice() {
                [ToolPayload::Terminal(terminal)] => terminal.clone(),
                other => panic!("expected a terminal payload, got {other:?}"),
            }
        };
        assert_eq!(terminal("Terminal"), "[terminal term-1]\n\n");
        assert_eq!(terminal("Read File"), "[terminal term-1]\n\n");
        assert_eq!(terminal("`ls -la`"), "$ ls -la\n\n");
        assert_eq!(terminal("cargo test"), "$ cargo test\n\n");
        assert_eq!(command_from_title("`grep`"), None);
        assert_eq!(
            command_from_title("`printf '\\`'`"),
            Some("printf '`'".to_owned())
        );
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

    fn quiet_turn() -> AgentThread {
        let mut thread = thread();
        thread.connection = AgentConnectionState::Ready;
        thread.begin_prompt("do the thing".to_owned(), Vec::new());
        thread.apply_update(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("working on it")),
        )));
        thread
    }

    #[test]
    fn parking_settles_the_streaming_turn_without_an_error() {
        let mut thread = quiet_turn();
        let streaming = thread.entries.last().expect("streamed answer").id();
        let revision = thread.entry_revisions.last().copied().expect("revision");
        assert!(thread.active_stream.is_some());

        thread.park_turn();

        assert_eq!(thread.connection, AgentConnectionState::Ready);
        assert!(thread.connection.accepts_prompt());
        assert!(thread.error.is_none(), "a park is not a failure");
        assert!(thread.active_stream.is_none());
        assert!(!thread.suppress_user_echo);
        assert!(matches!(
            thread.entries.last(),
            Some(AgentThreadEntry::Assistant { id, markdown, .. })
                if *id == streaming && markdown == "working on it"
        ));
        assert!(thread.entry_revisions.last().copied() >= Some(revision));
    }

    #[test]
    fn a_parked_tool_call_settles_completed_rather_than_failed() {
        let mut thread = quiet_turn();
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-1", "Run tests").status(ToolCallStatus::Completed),
        ));
        thread.apply_update(SessionUpdate::ToolCall(
            ToolCall::new("tool-2", "Read file").status(ToolCallStatus::Pending),
        ));

        thread.park_turn();

        assert!(
            !thread.entries.iter().any(|entry| matches!(
                entry,
                AgentThreadEntry::Tool {
                    status: AgentToolStatusModel::Pending
                        | AgentToolStatusModel::Running
                        | AgentToolStatusModel::NeedsApproval,
                    ..
                }
            )),
            "parking leaves nothing half-open in the transcript"
        );
        assert!(matches!(
            thread.entries.last(),
            Some(AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Completed,
                ..
            })
        ));
    }

    #[test]
    fn output_after_a_park_opens_a_new_segment() {
        let mut thread = quiet_turn();
        thread.park_turn();
        let parked = thread.entries.len();

        thread.apply_runtime_update(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("actually, one more thing")),
        )));

        assert_eq!(
            thread.entries.len(),
            parked + 1,
            "a late answer starts its own segment instead of reopening the parked one"
        );
        assert!(matches!(
            thread.entries.last(),
            Some(AgentThreadEntry::Assistant { markdown, .. })
                if markdown == "actually, one more thing"
        ));
        assert!(matches!(
            &thread.entries[parked - 1],
            AgentThreadEntry::Assistant { markdown, .. } if markdown == "working on it"
        ));
    }

    /// A controller wired to a mux client whose agent requests the test reads
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
                            5,
                            AgentStreamPayload::PermissionResolved {
                                request_id: 7,
                                canceled: false,
                            },
                        ),
                        item(6, turn_finished(StopReason::EndTurn)),
                        item(
                            7,
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
                assert_eq!(
                    controller.take_pending_composer(pane).as_deref(),
                    Some("retry that"),
                    "a reclaimed prompt is visible in the draft again"
                );
                assert_eq!(
                    controller.viewports[&pane].last_applied, 7,
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
            });
        });
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
                };

                controller.apply_pane_state(pane, &state, cx);

                assert_eq!(controller.queued_count(pane), 2);
                let pane_state = controller.pane_state(pane).expect("the pane is registered");
                assert_eq!(pane_state.queued_prompts, 2);
                assert_eq!(pane_state.session_id.as_deref(), Some("s-9"));
                assert_eq!(pane_state.auth_methods.len(), 1);
                assert_eq!(pane_state.config_options.len(), 1);
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
                    },
                    cx,
                );
                let pane_state = controller.pane_state(pane).expect("the pane is registered");
                assert_eq!(pane_state.queued_prompts, 0);
                assert_eq!(pane_state.session_id.as_deref(), Some("s-9"));
                assert!(pane_state.auth_methods.is_empty());
                assert!(pane_state.config_options.is_empty());
                assert!(pane_state.pending_permissions.is_empty());
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
                controller
                    .viewports
                    .get_mut(&pane)
                    .expect("viewport")
                    .turn_dispatched = true;

                controller.retry(pane, cx);
                controller.retry(pane, cx);

                let state = controller.pane_state(pane).expect("pane state");
                assert_eq!(state.connection, AgentConnectionState::Failed);
                assert_eq!(state.error.as_deref(), Some("could not spawn adapter"));
                assert!(!state.session_history.loading);
                assert!(state.lifecycle_pending);
                assert!(!controller.has_turn_base(pane));
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
    fn a_rejected_session_change_keeps_the_previous_turn_diff(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(24);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                let thread = controller.panes.get_mut(&pane).expect("pane");
                thread.session_id = Some(Arc::from("current"));
                thread.session_capabilities.load = true;
                controller
                    .viewports
                    .get_mut(&pane)
                    .expect("viewport")
                    .turn_dispatched = true;

                controller
                    .switch_session(
                        pane,
                        AgentSessionSummary {
                            session_id: "rejected".to_owned(),
                            cwd: PathBuf::from("/workspace"),
                            additional_directories: Vec::new(),
                            title: None,
                            updated_at: None,
                        },
                        cx,
                    )
                    .expect("session request");
                assert!(controller.has_turn_base(pane));

                controller.handle_runtime_event(
                    pane,
                    RuntimeEvent::SessionSwitchFailed {
                        pane,
                        message: "could not restore".to_owned(),
                    },
                    cx,
                );
                assert!(controller.has_turn_base(pane));

                controller.handle_runtime_event(
                    pane,
                    RuntimeEvent::SessionReset {
                        pane,
                        restoring: false,
                    },
                    cx,
                );
                assert!(!controller.has_turn_base(pane));
            });
        });
    }

    #[gpui::test]
    fn fatal_state_settles_subagent_spinners(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(22);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                let thread = controller.panes.get_mut(&pane).expect("pane");
                thread.apply_update(SessionUpdate::ToolCall(
                    ToolCall::new("task-tool", "Research")
                        .kind(ToolKind::Think)
                        .status(ToolCallStatus::InProgress)
                        .meta(claude_meta(&serde_json::json!({
                            "claudeCode": {"subagent": true}
                        }))),
                ));
                thread.apply_task_event(SdkTaskEvent::Started {
                    task_id: "task-1".to_owned(),
                    tool_use_id: "task-tool".to_owned(),
                    is_agent: true,
                });
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
                assert!(thread.live_task_tools.is_empty());
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
    fn unavailable_turn_diff_clears_only_its_live_turn_base(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(23);
        let (sender, receiver) = async_channel::bounded(1);
        let result = serde_json::to_string(&AgentStreamPayload::TurnDiff {
            client: zz_protocol::ClientId(1),
            request_id: 9,
            outcome: AgentTurnDiffOutcome::Unavailable {
                message: "this pane has no turn to diff".to_owned(),
            },
        })
        .expect("turn diff encodes");
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller
                    .viewports
                    .get_mut(&pane)
                    .expect("viewport")
                    .turn_dispatched = true;
                let generation = controller.turn_generation(pane);
                controller.turn_diffs.insert(9, (pane, generation, sender));
                controller.apply_turn_diff_result(pane, &result, cx);
                assert!(!controller.has_turn_base(pane));
                controller
                    .viewports
                    .get_mut(&pane)
                    .expect("viewport")
                    .turn_dispatched = true;
                controller.apply_turn_diff_result(pane, &result, cx);
                assert!(controller.has_turn_base(pane));
            });
        });
        assert!(matches!(
            receiver.try_recv(),
            Ok(Err(message)) if message == "this pane has no turn to diff"
        ));
    }

    #[gpui::test]
    fn turn_diff_replies_are_correlated_by_pane_and_abandoned_on_failure(cx: &mut TestAppContext) {
        let (controller, _sink) = proxy_controller(cx);
        let pane = PaneId(18);
        let other = PaneId(19);
        let (sender, receiver) = async_channel::bounded(1);
        cx.update(|cx| {
            controller.update(cx, |controller, cx| {
                ready_pane(controller, pane);
                controller
                    .viewports
                    .get_mut(&pane)
                    .expect("viewport")
                    .turn_dispatched = true;
                let generation = controller.turn_generation(pane);
                controller.turn_diffs.insert(7, (pane, generation, sender));
                controller.resolve_turn_diff(other, 7, Err("wrong pane".to_owned()));
                assert!(controller.turn_diffs.contains_key(&7));

                controller.handle_runtime_event(
                    pane,
                    RuntimeEvent::PaneFailed {
                        pane,
                        message: "adapter exited".to_owned(),
                    },
                    cx,
                );
                assert!(!controller.turn_diffs.contains_key(&7));
                assert!(!controller.has_turn_base(pane));
            });
        });
        assert!(matches!(
            receiver.try_recv(),
            Ok(Err(message)) if message == "the agent session changed"
        ));
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
