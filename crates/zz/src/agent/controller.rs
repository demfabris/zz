use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    str::FromStr as _,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use agent_client_protocol::{
    AcpAgent, Agent, Client as AcpClientRole, ConnectTo, ConnectionTo, LineDirection, Responder,
    schema::{
        MaybeUndefined, ProtocolVersion,
        v1::{
            AgentNotification, AuthMethod, AuthenticateRequest, AvailableCommand,
            AvailableCommandInput, CancelNotification, ClientCapabilities,
            ClientSessionCapabilities, CloseSessionRequest, ContentBlock, ContentChunk,
            DeleteSessionRequest, ImageContent, Implementation, InitializeRequest,
            ListSessionsRequest, LoadSessionRequest, NewSessionRequest, PermissionOption,
            PermissionOptionKind, Plan, PlanEntryStatus, PromptRequest, RequestPermissionOutcome,
            RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
            SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
            SessionConfigOptionValue, SessionConfigOptionsCapabilities, SessionId as AcpSessionId,
            SessionModeState, SessionUpdate, SetSessionConfigOptionRequest, SetSessionModeRequest,
            StopReason, TextContent, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
            ToolKind,
        },
    },
};
use async_channel::{Receiver, Sender};
use gpui::{Context, EventEmitter, Image, ImageFormat, Task};
use parking_lot::Mutex;
use zz_protocol::{AgentDescriptor, AgentProvider, PaneId};

use crate::{
    agent::environment::AgentWorkspaceEnvironment,
    agent::preferences::{AgentPreferenceKind, AgentPreferences},
    agent::profile::{
        CodexCollaboration, MemoryCitation, SdkTaskEvent, Segment, TaskNotification,
        client_meta_caps, codex_collab_label, codex_collaboration, codex_subagent_activity,
        codex_tool_subagent, format_codex_collaboration, is_sdk_message_method,
        parse_sdk_task_event, scan_text, session_meta,
    },
    config::AgentConfig,
};

const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);
const RUNTIME_EVENT_BATCH_LIMIT: usize = 256;
const LEGACY_MODE_PREFERENCE_ID: &str = "legacy-session-mode";
const MAX_SESSION_ID_BYTES: usize = 16 * 1024;
const MAX_SESSION_TITLE_BYTES: usize = 4 * 1024;
const MAX_SESSION_TIMESTAMP_BYTES: usize = 256;
const MAX_SESSION_CURSOR_BYTES: usize = 16 * 1024;

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
    pub(crate) mode: Option<Arc<str>>,
    pub(crate) modes: Arc<[AgentMode]>,
    pub(crate) config_options: Arc<[AgentConfigOption]>,
    pub(crate) available_commands: Arc<[AgentCommand]>,
    pub(crate) usage: Option<(u64, u64)>,
    /// Text `agent-send` routed here, waiting for the pane's view to fold it
    /// into the composer draft.
    pub(crate) pending_composer: Option<Arc<str>>,
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
    suppress_user_echo: bool,
    opened_generation: Option<u64>,
}

impl AgentThread {
    fn new(provider: AgentProvider, cwd: PathBuf, session_id: Option<String>) -> Self {
        Self {
            provider,
            connection: AgentConnectionState::Starting,
            entries: Vec::new(),
            entry_revisions: Vec::new(),
            entry_indices: HashMap::new(),
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
            suppress_user_echo: false,
            opened_generation: None,
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
            mode: self.mode.clone(),
            modes: self.modes.clone(),
            config_options: self.config_options.clone(),
            available_commands: self.available_commands.clone(),
            usage: self.usage,
            pending_composer: None,
        }
    }

    fn reset_for_open(&mut self, restoring: bool) {
        self.connection = if restoring {
            AgentConnectionState::Restoring
        } else {
            AgentConnectionState::Starting
        };
        self.entries.clear();
        self.entry_revisions.clear();
        self.entry_indices.clear();
        self.pending_permissions = Arc::from([]);
        self.error = None;
        self.title = None;
        self.mode = None;
        self.modes = Arc::from([]);
        self.config_options = Arc::from([]);
        self.available_commands = Arc::from([]);
        self.usage = None;
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
        self.suppress_user_echo = false;
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
        self.entry_indices.insert(entry.id(), self.entries.len());
        self.entries.push(entry);
        self.entry_revisions.push(revision);
    }

    fn entry_index(&self, id: u64) -> Option<usize> {
        self.entry_indices.get(&id).copied()
    }

    fn touch_entry(&mut self, index: usize) {
        let revision = self.allocate_entry_revision();
        if let Some(entry_revision) = self.entry_revisions.get_mut(index) {
            *entry_revision = revision;
        }
    }

    fn prompt_refusal(&self, has_images: bool) -> Option<Arc<str>> {
        if !self.connection.accepts_prompt() {
            return Some(Arc::from(format!(
                "agent is not ready ({})",
                self.connection.label().to_ascii_lowercase()
            )));
        }
        if has_images && !self.session_capabilities.images {
            return Some(Arc::from("this agent does not accept images"));
        }
        None
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
        let entry_id = if let Some(message_id) = message_id {
            let key = (role, message_id);
            if let Some(id) = self.message_entries.get(&key).copied() {
                id
            } else {
                let id = self.push_stream_entry(role);
                self.message_entries.insert(key, id);
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
        if let Some(next) = update.fields.kind {
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
        if let Some(next) = update.fields.title {
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
                .map(ToolPayload::Text)
                .or_else(|| pretty_json(&raw_input).map(ToolPayload::Json));
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
            .and_then(|raw_output| pretty_json(&raw_output))
            .map(ToolPayload::Json);
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
        let entry_id = if let Some(message_id) = message_id {
            let key = (parent_tool_use_id.to_owned(), role, message_id);
            if let Some(id) = self.child_message_entries.get(&key).copied() {
                id
            } else {
                let id = self.push_child_stream_entry(root_tool_id, role);
                if id == 0 {
                    return;
                }
                self.child_message_entries.insert(key, id);
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
        if let Some(next) = update.fields.kind {
            *kind = map_tool_kind(next);
            changed = true;
        }
        if let Some(next) = update.fields.status {
            *status = map_tool_status(next);
            changed = true;
        }
        if let Some(next) = update.fields.title {
            *label = next;
            changed = true;
        }
        if let Some(next) = subagent {
            *entry_subagent = next;
            changed = true;
        }
        if let Some(raw_input) = update.fields.raw_input {
            *input = pretty_json(&raw_input).map(ToolPayload::Json);
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
            .and_then(|raw_output| pretty_json(&raw_output))
            .map(ToolPayload::Json);
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
        let AgentThreadEntry::Tool { children, .. } = &mut self.entries[root_index] else {
            return false;
        };
        children.push(entry);
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
        let mut pending_permissions = self.pending_permissions.to_vec();
        pending_permissions.push(AgentPermissionRequest {
            request_id,
            tool_call_id,
            title,
            options: options
                .into_iter()
                .map(|option| AgentPermissionOption {
                    id: option.option_id.0.to_string(),
                    name: option.name,
                    kind: map_permission_kind(option.kind),
                })
                .collect(),
        });
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

    fn settle_inflight(&mut self, settled_status: AgentToolStatusModel) {
        debug_assert!(matches!(
            settled_status,
            AgentToolStatusModel::Completed
                | AgentToolStatusModel::Failed
                | AgentToolStatusModel::Canceled
        ));
        self.finish_text_streams();
        self.finish_child_text_streams();
        self.pending_permissions = Arc::from([]);
        self.suppress_user_echo = false;
        self.active_stream = None;
        let held: std::collections::HashSet<String> =
            self.live_task_tools.values().cloned().collect();
        for index in 0..self.entries.len() {
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
                    if let AgentThreadEntry::Tool { status, .. } = child
                        && matches!(
                            status,
                            AgentToolStatusModel::Pending
                                | AgentToolStatusModel::Running
                                | AgentToolStatusModel::NeedsApproval
                        )
                    {
                        *status = settled_status;
                        changed = true;
                    }
                }
                changed
            } else {
                false
            };
            if changed {
                self.touch_entry(index);
            }
        }
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
        }

        self.entry_indices.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            self.entry_indices.insert(entry.id(), index);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AgentControllerEvent {
    Pane {
        pane: PaneId,
    },
    Provider {
        pane: PaneId,
        provider: AgentProvider,
    },
    Session {
        pane: PaneId,
        session_id: Arc<str>,
        cwd: PathBuf,
    },
    Title {
        pane: PaneId,
        title: Arc<str>,
    },
}

#[derive(Debug)]
enum RuntimeCommand {
    Open {
        pane: PaneId,
        cwd: PathBuf,
        resume_session: Option<String>,
    },
    ListSessions {
        pane: PaneId,
        cwd: Option<PathBuf>,
        cursor: Option<String>,
        replace: bool,
    },
    SwitchSession {
        pane: PaneId,
        session: AgentSessionSummary,
    },
    NewSession {
        pane: PaneId,
        cwd: PathBuf,
    },
    DeleteSession {
        pane: PaneId,
        session_id: String,
    },
    Prompt {
        pane: PaneId,
        text: String,
        images: Vec<Arc<Image>>,
    },
    Cancel {
        pane: PaneId,
    },
    RespondPermission {
        request_id: u64,
        option_id: Option<String>,
    },
    Authenticate {
        method_id: String,
    },
    SetConfigOption {
        pane: PaneId,
        request: AgentSettingRequest,
    },
    SetMode {
        pane: PaneId,
        mode_id: String,
        origin: AgentSettingOrigin,
    },
    Shutdown,
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

struct PendingPermissionResponder {
    pane: PaneId,
    responder: Responder<RequestPermissionResponse>,
}

#[derive(Default)]
struct RuntimeRouting {
    session_to_pane: HashMap<String, PaneId>,
    staged_updates: HashMap<String, Vec<SessionUpdate>>,
    permissions: HashMap<u64, PendingPermissionResponder>,
}

struct PaneRuntime {
    command_tx: Sender<RuntimeCommand>,
    _runtime_task: Task<()>,
    _event_task: Task<()>,
    generation: u64,
    stopping: bool,
    restart_after_stop: bool,
}

pub struct AgentController {
    config: AgentConfig,
    preferences: AgentPreferences,
    workspace: AgentWorkspaceEnvironment,
    panes: BTreeMap<PaneId, AgentThread>,
    pending_composer: BTreeMap<PaneId, String>,
    retained_panes: BTreeSet<PaneId>,
    runtimes: BTreeMap<PaneId, PaneRuntime>,
    next_runtime_generation: u64,
    shutting_down: bool,
}

impl AgentController {
    #[cfg(test)]
    pub(crate) fn new(config: AgentConfig) -> Self {
        Self::with_preferences(config, AgentPreferences::default(), None)
    }

    pub fn with_preferences(
        config: AgentConfig,
        preferences: AgentPreferences,
        socket: Option<String>,
    ) -> Self {
        Self {
            config,
            preferences,
            workspace: AgentWorkspaceEnvironment {
                socket,
                ..AgentWorkspaceEnvironment::default()
            },
            panes: BTreeMap::new(),
            pending_composer: BTreeMap::new(),
            retained_panes: BTreeSet::new(),
            runtimes: BTreeMap::new(),
            next_runtime_generation: 0,
            shutting_down: false,
        }
    }

    pub(crate) fn pane_state(&self, pane: PaneId) -> Option<AgentPaneState> {
        self.panes.get(&pane).map(|thread| {
            let mut state = thread.snapshot();
            state.pending_composer = self
                .pending_composer
                .get(&pane)
                .map(|text| Arc::from(text.as_str()));
            state
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

    pub(crate) fn ensure_pane(
        &mut self,
        pane: PaneId,
        descriptor: &AgentDescriptor,
        cx: &mut Context<Self>,
    ) {
        let configured_cwd = self.config.working_directory.clone();
        let cwd = if descriptor.session_id.is_some() {
            descriptor.cwd.clone().or(configured_cwd)
        } else {
            configured_cwd.or_else(|| descriptor.cwd.clone())
        }
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"));
        let session_id = descriptor.session_id.clone();
        let provider_changed = self
            .panes
            .get(&pane)
            .is_some_and(|thread| thread.provider != descriptor.provider);
        if provider_changed {
            self.stop_runtime(pane, true);
        }
        let thread = self.panes.entry(pane).or_insert_with(|| {
            AgentThread::new(descriptor.provider, cwd.clone(), session_id.clone())
        });
        if provider_changed {
            *thread = AgentThread::new(descriptor.provider, cwd.clone(), session_id.clone());
        }
        if thread.cwd != cwd {
            thread.cwd.clone_from(&cwd);
            thread.opened_generation = None;
        }
        if thread.opened_generation.is_none()
            && session_id.is_some()
            && thread.session_id.as_deref() != session_id.as_deref()
        {
            thread.session_id = session_id.clone().map(Arc::from);
        }
        self.retained_panes.insert(pane);
        self.ensure_runtime(pane, cx);
        self.open_pane_if_needed(pane);
    }

    /// Record the daemon session an ACP child should be told about. Only
    /// children started afterwards see it.
    pub(crate) fn set_session_name(&mut self, session: Option<String>) {
        if self.workspace.session != session {
            self.workspace.session = session;
        }
    }

    pub(crate) fn retain_panes(&mut self, retained: &BTreeSet<PaneId>) {
        let removed = self
            .retained_panes
            .difference(retained)
            .copied()
            .collect::<Vec<_>>();
        for pane in removed {
            self.stop_runtime(pane, false);
            self.panes.remove(&pane);
        }
        self.pending_composer
            .retain(|pane, _| retained.contains(pane));
        self.retained_panes.clone_from(retained);
    }

    pub(crate) fn synchronize_config(&mut self, config: AgentConfig, cx: &mut Context<Self>) {
        if self.config == config {
            return;
        }
        self.config = config;
        for thread in self.panes.values_mut() {
            thread.opened_generation = None;
            thread.connection = AgentConnectionState::Starting;
            thread.error = None;
        }
        let panes = self.retained_panes.iter().copied().collect::<Vec<_>>();
        for pane in panes {
            if self.runtimes.contains_key(&pane) {
                self.stop_runtime(pane, true);
            } else {
                self.ensure_runtime(pane, cx);
                self.open_pane_if_needed(pane);
            }
        }
        cx.notify();
    }

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
        let cwd = thread.cwd.clone();
        self.stop_runtime(pane, true);
        self.panes
            .insert(pane, AgentThread::new(provider, cwd, None));
        cx.emit(AgentControllerEvent::Provider { pane, provider });
        if !self.runtimes.contains_key(&pane) {
            self.ensure_runtime(pane, cx);
            self.open_pane_if_needed(pane);
        }
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
        let (cwd, cursor) = {
            let Some(thread) = self.panes.get(&pane) else {
                return Err(Arc::from("agent pane is not registered"));
            };
            if !thread.session_capabilities.list {
                return Err(Arc::from("this agent does not support session history"));
            }
            if thread.session_history.loading {
                return Ok(());
            }
            let cursor = append
                .then(|| {
                    thread
                        .session_history
                        .next_cursor
                        .as_deref()
                        .map(ToOwned::to_owned)
                })
                .flatten();
            if append && cursor.is_none() {
                return Ok(());
            }
            ((!all_projects).then(|| thread.cwd.clone()), cursor)
        };
        let command = RuntimeCommand::ListSessions {
            pane,
            cwd: cwd.clone(),
            cursor,
            replace: !append,
        };
        if !self.send(pane, command) {
            return Err(Arc::from("agent runtime is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.session_history.loading = true;
            thread.session_history.error = None;
            if !append {
                thread.session_history.sessions = Arc::from([]);
                thread.session_history.cwd_filter = cwd;
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
        if !self.send(pane, RuntimeCommand::SwitchSession { pane, session }) {
            return Err(Arc::from("agent runtime is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.connection = AgentConnectionState::Restoring;
            thread.error = None;
        }
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
        if !self.send(pane, RuntimeCommand::NewSession { pane, cwd }) {
            return Err(Arc::from("agent runtime is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.connection = AgentConnectionState::Starting;
            thread.error = None;
        }
        cx.notify();
        Ok(())
    }

    /// Point the pane at another workspace. An ACP session is bound to the
    /// directory it was created in, so this opens a new session in `cwd`, and a
    /// refused switch leaves the pane where it was.
    pub(crate) fn set_working_directory(
        &mut self,
        pane: PaneId,
        cwd: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        let Some(thread) = self.panes.get(&pane) else {
            return Err(Arc::from("agent pane is not registered"));
        };
        if !cwd.is_absolute() {
            return Err(Arc::from("the working directory must be an absolute path"));
        }
        if thread.cwd == cwd {
            return Ok(());
        }
        if thread.connection.has_active_turn() || !thread.pending_permissions.is_empty() {
            return Err(Arc::from(
                "finish or cancel the current turn before changing the working directory",
            ));
        }
        if !self.send(pane, RuntimeCommand::NewSession { pane, cwd }) {
            return Err(Arc::from("agent runtime is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.connection = AgentConnectionState::Starting;
            thread.error = None;
        }
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
            RuntimeCommand::DeleteSession {
                pane,
                session_id: session_id.to_owned(),
            },
        ) {
            return Err(Arc::from("agent runtime is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.session_history.loading = true;
            thread.session_history.error = None;
        }
        cx.notify();
        Ok(())
    }

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
        {
            let Some(thread) = self.panes.get_mut(&pane) else {
                return Err(Arc::from("agent pane is not registered"));
            };
            if let Some(refusal) = thread.prompt_refusal(!images.is_empty()) {
                return Err(refusal);
            }
            thread.begin_prompt(text.clone(), images.clone());
        }
        if !self.send(pane, RuntimeCommand::Prompt { pane, text, images }) {
            if let Some(thread) = self.panes.get_mut(&pane) {
                thread.connection = AgentConnectionState::Failed;
                thread.error = Some(Arc::from("agent runtime is not connected"));
            }
            return Err(Arc::from("agent runtime is not connected"));
        }
        cx.notify();
        Ok(())
    }

    /// Queue `agent-send` text for the pane's composer draft. The view folds it
    /// in when it next renders; repeated sends stack with a newline between.
    pub(crate) fn append_composer(&mut self, pane: PaneId, text: &str, cx: &mut Context<Self>) {
        if !self.queue_composer_text(pane, text) {
            return;
        }
        cx.emit(AgentControllerEvent::Pane { pane });
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

    pub(crate) fn cancel(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let Some(thread) = self.panes.get_mut(&pane) else {
            return;
        };
        if !thread.connection.has_active_turn() {
            return;
        }
        thread.connection = AgentConnectionState::Cancelling;
        self.send(pane, RuntimeCommand::Cancel { pane });
        cx.notify();
    }

    pub(crate) fn respond_permission(
        &mut self,
        pane: PaneId,
        request_id: u64,
        option_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let canceled = option_id.is_none();
        if self.send(
            pane,
            RuntimeCommand::RespondPermission {
                request_id,
                option_id,
            },
        ) {
            if let Some(thread) = self.panes.get_mut(&pane) {
                thread.resolve_permission(request_id, canceled);
            }
            cx.notify();
        }
    }

    pub(crate) fn authenticate(&mut self, pane: PaneId, method_id: String, cx: &mut Context<Self>) {
        if self.send(pane, RuntimeCommand::Authenticate { method_id }) {
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
        if !self.send(
            pane,
            RuntimeCommand::SetConfigOption {
                pane,
                request: AgentSettingRequest {
                    config_id: config_id.to_owned(),
                    value: value.to_owned(),
                    origin: AgentSettingOrigin::User(preference_kind),
                },
            },
        ) {
            return Err(Arc::from("agent runtime is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.error = None;
            thread.settings_busy = true;
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
        if !self.send(
            pane,
            RuntimeCommand::SetMode {
                pane,
                mode_id: mode_id.to_owned(),
                origin: AgentSettingOrigin::User(Some(AgentPreferenceKind::Permission)),
            },
        ) {
            return Err(Arc::from("agent runtime is not connected"));
        }
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.error = None;
            thread.settings_busy = true;
        }
        cx.notify();
        Ok(())
    }

    fn reconcile_preferences(&mut self, pane: PaneId) {
        let Some(thread) = self.panes.get(&pane) else {
            return;
        };
        if thread.settings_busy || !thread.connection.accepts_prompt() {
            return;
        }
        let command = preferred_setting_command(thread, &self.preferences, pane);
        if let Some(command) = command
            && self.send(pane, command)
            && let Some(thread) = self.panes.get_mut(&pane)
        {
            thread.settings_busy = true;
        }
    }

    pub(crate) fn retry(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if let Some(thread) = self.panes.get_mut(&pane) {
            thread.error = None;
            thread.connection = AgentConnectionState::Starting;
            thread.opened_generation = None;
        }
        self.ensure_runtime(pane, cx);
        self.open_pane_if_needed(pane);
        cx.notify();
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    pub(crate) fn is_shutdown_complete(&self) -> bool {
        self.shutting_down && self.runtimes.is_empty()
    }

    pub(crate) fn shutdown(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        if !self.shutting_down {
            self.shutting_down = true;
            let panes = self.runtimes.keys().copied().collect::<Vec<_>>();
            for pane in panes {
                self.stop_runtime(pane, false);
            }
        }
        if self.runtimes.is_empty() {
            return Task::ready(true);
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SHUTDOWN_POLL_INTERVAL).await;
                match this.read_with(cx, |controller, _| controller.runtimes.is_empty()) {
                    Ok(true) => return true,
                    Ok(false) => {}
                    Err(error) => {
                        log::error!("lost agent controller during shutdown: {error}");
                        return false;
                    }
                }
            }
        })
    }

    fn ensure_runtime(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if self.shutting_down {
            return;
        }
        if let Some(runtime) = self.runtimes.get_mut(&pane) {
            if runtime.stopping {
                runtime.restart_after_stop = true;
            }
            return;
        }
        let Some(provider) = self.panes.get(&pane).map(|thread| thread.provider) else {
            return;
        };
        self.next_runtime_generation = self.next_runtime_generation.saturating_add(1);
        let generation = self.next_runtime_generation;
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let config = self.config.clone();
        let workspace = AgentWorkspaceEnvironment {
            pane: Some(pane.to_string()),
            ..self.workspace.clone()
        };
        let background = cx.background_executor().spawn(async move {
            run_agent_runtime(config, provider, workspace, command_rx, event_tx).await
        });
        let runtime_task = cx.spawn(async move |this, cx| {
            let result = background.await;
            let _ = this.update(cx, |controller, cx| {
                controller.runtime_finished(pane, generation, result, cx);
            });
        });
        let event_task = cx.spawn(async move |this, cx| {
            while let Ok(event) = event_rx.recv().await {
                let mut events = Vec::with_capacity(RUNTIME_EVENT_BATCH_LIMIT);
                events.push(event);
                while events.len() < RUNTIME_EVENT_BATCH_LIMIT {
                    let Ok(event) = event_rx.try_recv() else {
                        break;
                    };
                    events.push(event);
                }
                if this
                    .update(cx, |controller, cx| {
                        if controller.runtimes.get(&pane).is_some_and(|runtime| {
                            runtime.generation == generation && !runtime.stopping
                        }) {
                            let mut changed = false;
                            for event in events {
                                changed |= controller.handle_runtime_event(pane, event, cx);
                            }
                            if changed {
                                cx.emit(AgentControllerEvent::Pane { pane });
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        self.runtimes.insert(
            pane,
            PaneRuntime {
                command_tx,
                _runtime_task: runtime_task,
                _event_task: event_task,
                generation,
                stopping: false,
                restart_after_stop: false,
            },
        );
    }

    fn open_pane_if_needed(&mut self, pane: PaneId) {
        let Some(generation) = self
            .runtimes
            .get(&pane)
            .filter(|runtime| !runtime.stopping)
            .map(|runtime| runtime.generation)
        else {
            return;
        };
        let command = {
            let Some(thread) = self.panes.get(&pane) else {
                return;
            };
            if thread.opened_generation == Some(generation) {
                return;
            }
            RuntimeCommand::Open {
                pane,
                cwd: thread.cwd.clone(),
                resume_session: thread.session_id.as_deref().map(ToOwned::to_owned),
            }
        };
        if self.send(pane, command)
            && let Some(thread) = self.panes.get_mut(&pane)
        {
            thread.opened_generation = Some(generation);
            thread.connection = AgentConnectionState::Starting;
        }
    }

    fn send(&self, pane: PaneId, command: RuntimeCommand) -> bool {
        self.runtimes
            .get(&pane)
            .filter(|runtime| !runtime.stopping)
            .is_some_and(|runtime| runtime.command_tx.try_send(command).is_ok())
    }

    fn stop_runtime(&mut self, pane: PaneId, restart_after_stop: bool) {
        let Some(runtime) = self.runtimes.get_mut(&pane) else {
            return;
        };
        runtime.restart_after_stop |= restart_after_stop;
        if runtime.stopping {
            return;
        }
        runtime.stopping = true;
        let _ = runtime.command_tx.try_send(RuntimeCommand::Shutdown);
    }

    fn runtime_finished(
        &mut self,
        pane: PaneId,
        generation: u64,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.runtimes.get(&pane) else {
            return;
        };
        if runtime.generation != generation {
            return;
        }
        let intentional_stop = runtime.stopping;
        let restart = runtime.restart_after_stop
            && self.retained_panes.contains(&pane)
            && !self.shutting_down;
        self.runtimes.remove(&pane);
        if !self.shutting_down
            && !intentional_stop
            && let Some(thread) = self.panes.get_mut(&pane)
        {
            let error = result.err().map_or_else(
                || Arc::from("agent process disconnected unexpectedly"),
                Arc::<str>::from,
            );
            thread.opened_generation = None;
            thread.connection = AgentConnectionState::Failed;
            thread.cancel_inflight();
            thread.error = Some(error);
        }
        if restart {
            self.ensure_runtime(pane, cx);
            self.open_pane_if_needed(pane);
        }
        cx.notify();
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
                    thread.auth_methods = auth_methods.into();
                    thread.session_capabilities = session_capabilities;
                    changed_pane = Some(runtime_pane);
                }
            }
            RuntimeEvent::SessionReset { pane, restoring } => {
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
                if let Some(thread) = self.panes.get_mut(&pane) {
                    let session_id: Arc<str> = Arc::from(session_id);
                    thread.session_id = Some(session_id.clone());
                    thread.set_session_configuration(modes, config_options);
                    if thread.connection == AgentConnectionState::Restoring {
                        thread.finish_replay();
                    }
                    thread.settle_inflight(AgentToolStatusModel::Completed);
                    thread.connection = AgentConnectionState::Ready;
                    thread.error = None;
                    cx.emit(AgentControllerEvent::Session {
                        pane,
                        session_id,
                        cwd: thread.cwd.clone(),
                    });
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
                if let Some(thread) = self.panes.get_mut(&pane) {
                    let previous_title = thread.title.clone();
                    thread.reset_for_open(true);
                    thread.cwd = cwd;
                    for update in replay {
                        thread.apply_update(update);
                    }
                    thread.finish_replay();
                    thread.settle_inflight(AgentToolStatusModel::Completed);
                    let session_id: Arc<str> = Arc::from(session_id);
                    thread.session_id = Some(session_id.clone());
                    thread.set_session_configuration(modes, config_options);
                    thread.connection = AgentConnectionState::Ready;
                    thread.error = None;
                    cx.emit(AgentControllerEvent::Session {
                        pane,
                        session_id,
                        cwd: thread.cwd.clone(),
                    });
                    if thread.title != previous_title {
                        let title = thread.title.clone().unwrap_or_else(|| Arc::from("agent"));
                        cx.emit(AgentControllerEvent::Title { pane, title });
                    }
                    changed_pane = Some(pane);
                    reconcile_pane = Some(pane);
                }
            }
            RuntimeEvent::SessionSwitchFailed { pane, message } => {
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.connection = AgentConnectionState::Ready;
                    thread.error = Some(Arc::from(message));
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
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.finish_text_streams();
                    thread.suppress_user_echo = false;
                    thread.active_stream = None;
                    match result {
                        Ok(StopReason::Cancelled) => {
                            thread.connection = AgentConnectionState::Ready;
                            thread.cancel_inflight();
                        }
                        Ok(_) => {
                            thread.connection = AgentConnectionState::Ready;
                            thread.settle_inflight(AgentToolStatusModel::Completed);
                        }
                        Err(error) => {
                            thread.connection = AgentConnectionState::Failed;
                            thread.error = Some(Arc::from(error));
                            thread.settle_inflight(AgentToolStatusModel::Failed);
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
                    thread.opened_generation = None;
                    thread.error = None;
                }
                self.open_pane_if_needed(runtime_pane);
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
                if let Some(thread) = self.panes.get_mut(&pane) {
                    thread.connection = AgentConnectionState::Failed;
                    thread.error = Some(Arc::from(message));
                    changed_pane = Some(pane);
                }
            }
        }
        if let Some((provider, agent_key, kind, option_id, value)) = remembered_preference {
            self.preferences
                .remember(provider, &agent_key, kind, &option_id, &value);
        }
        if let Some(pane) = reconcile_pane {
            self.reconcile_preferences(pane);
        }
        changed_pane.is_some()
    }
}

fn preferred_setting_command(
    thread: &AgentThread,
    preferences: &AgentPreferences,
    pane: PaneId,
) -> Option<RuntimeCommand> {
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
            return Some(RuntimeCommand::SetConfigOption {
                pane,
                request: AgentSettingRequest {
                    config_id: option.id.clone(),
                    value: value.to_owned(),
                    origin: AgentSettingOrigin::Preference(kind),
                },
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
            return Some(RuntimeCommand::SetMode {
                pane,
                mode_id: value.to_owned(),
                origin: AgentSettingOrigin::Preference(AgentPreferenceKind::Permission),
            });
        }
    }
    None
}

impl EventEmitter<AgentControllerEvent> for AgentController {}

async fn run_agent_runtime(
    config: AgentConfig,
    provider: AgentProvider,
    workspace: AgentWorkspaceEnvironment,
    command_rx: Receiver<RuntimeCommand>,
    event_tx: Sender<RuntimeEvent>,
) -> Result<(), String> {
    let agent = AcpAgent::from_str(config.command_for(provider))
        .map_err(|error| format!("invalid {}: {error}", AgentConfig::key_for(provider)))?;
    let agent = crate::agent::environment::with_platform_environment(agent);
    let agent = crate::agent::environment::with_workspace_environment(agent, &workspace)
        .with_debug(|line, direction| {
            if matches!(direction, LineDirection::Stderr) {
                log::warn!(target: "zz::agent::stderr", "{line}");
            }
        });

    run_agent_connection(provider, agent, command_rx, event_tx).await
}

fn new_session_request(provider: AgentProvider, cwd: PathBuf) -> NewSessionRequest {
    let mut request = NewSessionRequest::new(cwd);
    request.meta = session_meta(provider);
    request
}

fn load_session_request(
    provider: AgentProvider,
    session_id: AcpSessionId,
    cwd: PathBuf,
) -> LoadSessionRequest {
    let mut request = LoadSessionRequest::new(session_id, cwd);
    request.meta = session_meta(provider);
    request
}

async fn run_agent_connection(
    provider: AgentProvider,
    agent: impl ConnectTo<AcpClientRole>,
    command_rx: Receiver<RuntimeCommand>,
    event_tx: Sender<RuntimeEvent>,
) -> Result<(), String> {
    let routing = Arc::new(Mutex::new(RuntimeRouting::default()));
    let next_permission_id = Arc::new(AtomicU64::new(1));

    let notification_routing = Arc::clone(&routing);
    let notification_events = event_tx.clone();
    let ext_routing = Arc::clone(&routing);
    let ext_events = event_tx.clone();
    let permission_routing = Arc::clone(&routing);
    let permission_events = event_tx.clone();
    let permission_ids = Arc::clone(&next_permission_id);

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |notification: AgentNotification, _| {
                match notification {
                    AgentNotification::SessionNotification(notification) => {
                        let session_id = notification.session_id.0.to_string();
                        let mut routing = notification_routing.lock();
                        if let Some(updates) = routing.staged_updates.get_mut(&session_id) {
                            updates.push(notification.update);
                            return Ok(());
                        }
                        let pane = routing.session_to_pane.get(&session_id).copied();
                        drop(routing);
                        if let Some(pane) = pane {
                            permission_safe_send(
                                &notification_events,
                                RuntimeEvent::SessionUpdate {
                                    pane,
                                    update: notification.update,
                                },
                            )?;
                        }
                        Ok(())
                    }
                    AgentNotification::ExtNotification(notification) => {
                        log::debug!(
                            target: "zz::agent",
                            "ext notification received: {}",
                            notification.method
                        );
                        if !is_sdk_message_method(notification.method.as_ref()) {
                            return Ok(());
                        }
                        let Ok(params) =
                            serde_json::from_str::<serde_json::Value>(notification.params.get())
                        else {
                            return Ok(());
                        };
                        let Some((session_id, event)) = parse_sdk_task_event(&params) else {
                            return Ok(());
                        };
                        log::debug!(
                            target: "zz::agent",
                            "sdk task event for session {session_id}: {event:?}"
                        );
                        let routing = ext_routing.lock();
                        if routing.staged_updates.contains_key(&session_id) {
                            return Ok(());
                        }
                        let pane = routing.session_to_pane.get(&session_id).copied();
                        drop(routing);
                        if let Some(pane) = pane {
                            permission_safe_send(
                                &ext_events,
                                RuntimeEvent::TaskEvent { pane, event },
                            )?;
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _| {
                let session_id = request.session_id.0.to_string();
                let routing = permission_routing.lock();
                if routing.staged_updates.contains_key(&session_id) {
                    drop(routing);
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }
                let pane = routing.session_to_pane.get(&session_id).copied();
                drop(routing);
                let Some(pane) = pane else {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                };
                let request_id = permission_ids.fetch_add(1, Ordering::Relaxed);
                permission_routing.lock().permissions.insert(
                    request_id,
                    PendingPermissionResponder {
                        pane,
                        responder,
                    },
                );
                if let Err(error) = permission_safe_send(
                    &permission_events,
                    RuntimeEvent::PermissionRequested {
                        pane,
                        request_id,
                        tool_call: request.tool_call,
                        options: request.options,
                    },
                ) {
                    if let Some(pending) = permission_routing.lock().permissions.remove(&request_id)
                    {
                        let _ = pending.responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ));
                    }
                    return Err(error);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            let initialize = InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(
                    ClientCapabilities::new()
                        .session(
                            ClientSessionCapabilities::new()
                                .config_options(SessionConfigOptionsCapabilities::new()),
                        )
                        .meta(client_meta_caps(provider)),
                )
                .client_info(
                    Implementation::new("zz", env!("CARGO_PKG_VERSION")).title("zz"),
                );
            let response = connection.send_request(initialize).block_task().await?;
            if response.protocol_version != ProtocolVersion::V1 {
                return Err(agent_client_protocol::Error::internal_error().data(format!(
                    "agent selected unsupported protocol version {:?}",
                    response.protocol_version
                )));
            }
            let session_capabilities = AgentSessionCapabilities {
                load: response.agent_capabilities.load_session,
                list: response.agent_capabilities.session_capabilities.list.is_some(),
                close: response
                    .agent_capabilities
                    .session_capabilities
                    .close
                    .is_some(),
                delete: response
                    .agent_capabilities
                    .session_capabilities
                    .delete
                    .is_some(),
                additional_directories: response
                    .agent_capabilities
                    .session_capabilities
                    .additional_directories
                    .is_some(),
                images: response.agent_capabilities.prompt_capabilities.image,
            };
            let (agent_name, agent_key) = response.agent_info.map_or_else(
                || ("ACP agent".to_owned(), "acp-agent".to_owned()),
                |info| {
                    let agent_key = info.name.clone();
                    (info.title.unwrap_or(info.name), agent_key)
                },
            );
            let auth_methods = response
                .auth_methods
                .iter()
                .map(auth_method_model)
                .collect::<Vec<_>>();
            permission_safe_send(
                &event_tx,
                RuntimeEvent::Ready {
                    agent_name,
                    agent_key,
                    auth_methods,
                    session_capabilities,
                },
            )?;

            let mut sessions = BTreeMap::<PaneId, AcpSessionId>::new();
            while let Ok(command) = command_rx.recv().await {
                match command {
                    RuntimeCommand::Open {
                        pane,
                        cwd,
                        resume_session,
                    } => {
                        let restoring = session_capabilities.load && resume_session.is_some();
                        permission_safe_send(
                            &event_tx,
                            RuntimeEvent::SessionReset { pane, restoring },
                        )?;
                        let session_result = if let Some(resume) =
                            resume_session.filter(|_| session_capabilities.load)
                        {
                            let session_id = AcpSessionId::new(resume);
                            routing
                                .lock()
                                .session_to_pane
                                .insert(session_id.0.to_string(), pane);
                            match connection
                                .send_request(load_session_request(
                                    provider,
                                    session_id.clone(),
                                    cwd.clone(),
                                ))
                                .block_task()
                                .await
                            {
                                Ok(response) => Ok((
                                    session_id,
                                    response.modes,
                                    response.config_options,
                                )),
                                Err(error) => {
                                    routing
                                        .lock()
                                        .session_to_pane
                                        .remove(session_id.0.as_ref());
                                    log::warn!(
                                        target: "zz::agent",
                                        "could not restore ACP session for pane {pane}: {error}; creating a new session"
                                    );
                                    connection
                                        .send_request(new_session_request(provider, cwd))
                                        .block_task()
                                        .await
                                        .map(|response| {
                                            (
                                                response.session_id,
                                                response.modes,
                                                response.config_options,
                                            )
                                        })
                                }
                            }
                        } else {
                            connection
                                .send_request(new_session_request(provider, cwd))
                                .block_task()
                                .await
                                .map(|response| {
                                    (
                                        response.session_id,
                                        response.modes,
                                        response.config_options,
                                    )
                                })
                        };
                        match session_result {
                            Ok((session_id, modes, config_options)) => {
                                if !valid_session_id(session_id.0.as_ref()) {
                                    routing
                                        .lock()
                                        .session_to_pane
                                        .retain(|_, routed_pane| *routed_pane != pane);
                                    permission_safe_send(
                                        &event_tx,
                                        RuntimeEvent::PaneFailed {
                                            pane,
                                            message: "agent returned an invalid session ID"
                                                .to_owned(),
                                        },
                                    )?;
                                    continue;
                                }
                                routing
                                    .lock()
                                    .session_to_pane
                                    .insert(session_id.0.to_string(), pane);
                                sessions.insert(pane, session_id.clone());
                                permission_safe_send(
                                    &event_tx,
                                    RuntimeEvent::SessionReady {
                                        pane,
                                        session_id: session_id.0.to_string(),
                                        modes,
                                        config_options,
                                    },
                                )?;
                            }
                            Err(error) => {
                                permission_safe_send(
                                    &event_tx,
                                    RuntimeEvent::PaneFailed {
                                        pane,
                                        message: error.to_string(),
                                    },
                                )?;
                            }
                        }
                    }
                    RuntimeCommand::ListSessions {
                        pane,
                        cwd,
                        cursor,
                        replace,
                    } => {
                        if !session_capabilities.list {
                            permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SessionListFailed {
                                    pane,
                                    message: "agent does not support session/list".to_owned(),
                                },
                            )?;
                            continue;
                        }
                        let request = ListSessionsRequest::new()
                            .cwd(cwd.clone())
                            .cursor(cursor);
                        match connection.send_request(request).block_task().await {
                            Ok(response) => {
                                let sessions = response
                                    .sessions
                                    .into_iter()
                                    .filter_map(|session| {
                                        let summary = AgentSessionSummary {
                                            session_id: session.session_id.0.to_string(),
                                            cwd: session.cwd,
                                            additional_directories: session.additional_directories,
                                            title: session.title.and_then(|title| {
                                                clean_session_metadata(
                                                    &title,
                                                    MAX_SESSION_TITLE_BYTES,
                                                )
                                            }),
                                            updated_at: session.updated_at.and_then(|timestamp| {
                                                clean_session_metadata(
                                                    &timestamp,
                                                    MAX_SESSION_TIMESTAMP_BYTES,
                                                )
                                            }),
                                        };
                                        valid_session_summary(&summary).then_some(summary)
                                    })
                                    .collect();
                                permission_safe_send(
                                    &event_tx,
                                    RuntimeEvent::SessionsListed {
                                        pane,
                                        sessions,
                                        next_cursor: response
                                            .next_cursor
                                            .filter(|cursor| valid_session_cursor(cursor)),
                                        cwd_filter: cwd,
                                        replace,
                                    },
                                )?;
                            }
                            Err(error) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SessionListFailed {
                                    pane,
                                    message: format!("could not list agent sessions: {error}"),
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::SwitchSession { pane, session } => {
                        if !session_capabilities.load {
                            permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SessionSwitchFailed {
                                    pane,
                                    message: "agent does not support session/load".to_owned(),
                                },
                            )?;
                            continue;
                        }
                        let session_id = AcpSessionId::new(session.session_id.clone());
                        {
                            let mut routing = routing.lock();
                            routing
                                .session_to_pane
                                .insert(session.session_id.clone(), pane);
                            routing
                                .staged_updates
                                .insert(session.session_id.clone(), Vec::new());
                        }
                        let mut request =
                            load_session_request(provider, session_id.clone(), session.cwd.clone());
                        if session_capabilities.additional_directories {
                            request = request.additional_directories(
                                session.additional_directories.clone(),
                            );
                        }
                        match connection.send_request(request).block_task().await {
                            Ok(response) => {
                                let previous = sessions.insert(pane, session_id.clone());
                                let previous = previous.filter(|previous| previous != &session_id);
                                {
                                    let mut routes = routing.lock();
                                    let replay = routes
                                        .staged_updates
                                        .remove(&session.session_id)
                                        .unwrap_or_default();
                                    if let Some(previous) = &previous {
                                        routes.session_to_pane.remove(previous.0.as_ref());
                                    }
                                    permission_safe_send(
                                        &event_tx,
                                        RuntimeEvent::SessionSwitched {
                                            pane,
                                            session_id: session.session_id,
                                            cwd: session.cwd,
                                            modes: response.modes,
                                            config_options: response.config_options,
                                            replay,
                                        },
                                    )?;
                                }
                                if session_capabilities.close
                                    && let Some(previous) = previous
                                {
                                    spawn_close_session(
                                        &connection,
                                        previous,
                                        "previous ACP session after switch",
                                    )?;
                                }
                            }
                            Err(error) => {
                                {
                                    let mut routes = routing.lock();
                                    routes.staged_updates.remove(&session.session_id);
                                    routes.session_to_pane.remove(&session.session_id);
                                }
                                permission_safe_send(
                                    &event_tx,
                                    RuntimeEvent::SessionSwitchFailed {
                                        pane,
                                        message: format!(
                                            "could not load selected session: {error}"
                                        ),
                                    },
                                )?;
                                if session_capabilities.close {
                                    spawn_close_session(
                                        &connection,
                                        session_id,
                                        "failed ACP session target",
                                    )?;
                                }
                            }
                        }
                    }
                    RuntimeCommand::NewSession { pane, cwd } => {
                        match connection
                            .send_request(new_session_request(provider, cwd.clone()))
                            .block_task()
                            .await
                        {
                            Ok(response) if valid_session_id(response.session_id.0.as_ref()) => {
                                let session_id = response.session_id;
                                let previous = sessions.insert(pane, session_id.clone());
                                let previous = previous.filter(|previous| previous != &session_id);
                                {
                                    let mut routes = routing.lock();
                                    routes
                                        .session_to_pane
                                        .insert(session_id.0.to_string(), pane);
                                    if let Some(previous) = &previous {
                                        routes.session_to_pane.remove(previous.0.as_ref());
                                    }
                                    permission_safe_send(
                                        &event_tx,
                                        RuntimeEvent::SessionSwitched {
                                            pane,
                                            session_id: session_id.0.to_string(),
                                            cwd,
                                            modes: response.modes,
                                            config_options: response.config_options,
                                            replay: Vec::new(),
                                        },
                                    )?;
                                }
                                if session_capabilities.close
                                    && let Some(previous) = previous
                                {
                                    spawn_close_session(
                                        &connection,
                                        previous,
                                        "previous ACP session after creating a new one",
                                    )?;
                                }
                            }
                            Ok(_) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SessionSwitchFailed {
                                    pane,
                                    message: "agent returned an invalid session ID".to_owned(),
                                },
                            )?,
                            Err(error) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SessionSwitchFailed {
                                    pane,
                                    message: format!("could not create a new session: {error}"),
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::DeleteSession { pane, session_id } => {
                        if !session_capabilities.delete {
                            permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SessionDeleteFailed {
                                    pane,
                                    message: "agent does not support session/delete".to_owned(),
                                },
                            )?;
                            continue;
                        }
                        match connection
                            .send_request(DeleteSessionRequest::new(session_id.clone()))
                            .block_task()
                            .await
                        {
                            Ok(_) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SessionDeleted { pane, session_id },
                            )?,
                            Err(error) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SessionDeleteFailed {
                                    pane,
                                    message: format!("could not delete session: {error}"),
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::Prompt { pane, text, images } => {
                        let Some(session_id) = sessions.get(&pane).cloned() else {
                            permission_safe_send(
                                &event_tx,
                                RuntimeEvent::PaneFailed {
                                    pane,
                                    message: "agent session is not ready".to_owned(),
                                },
                            )?;
                            continue;
                        };
                        let prompt_events = event_tx.clone();
                        let request = connection
                            .send_request(PromptRequest::new(session_id, prompt_blocks(text, &images)));
                        connection.spawn(async move {
                            let result = request
                                .block_task()
                                .await
                                .map(|response| response.stop_reason)
                                .map_err(|error| error.to_string());
                            let _ = prompt_events
                                .send(RuntimeEvent::PromptFinished { pane, result })
                                .await;
                            Ok(())
                        })?;
                    }
                    RuntimeCommand::Cancel { pane } => {
                        if let Some(session_id) = sessions.get(&pane).cloned() {
                            connection.send_notification(CancelNotification::new(session_id))?;
                            cancel_pending_permissions(&routing, pane, &event_tx)?;
                        }
                    }
                    RuntimeCommand::RespondPermission {
                        request_id,
                        option_id,
                    } => {
                        let pending = routing.lock().permissions.remove(&request_id);
                        if let Some(pending) = pending {
                            let canceled = option_id.is_none();
                            let outcome = option_id.map_or(
                                RequestPermissionOutcome::Cancelled,
                                |option_id| {
                                    RequestPermissionOutcome::Selected(
                                        SelectedPermissionOutcome::new(option_id),
                                    )
                                },
                            );
                            pending
                                .responder
                                .respond(RequestPermissionResponse::new(outcome))?;
                            permission_safe_send(
                                &event_tx,
                                RuntimeEvent::PermissionResolved {
                                    pane: pending.pane,
                                    request_id,
                                    canceled,
                                },
                            )?;
                        }
                    }
                    RuntimeCommand::Authenticate { method_id } => {
                        match connection
                            .send_request(AuthenticateRequest::new(method_id))
                            .block_task()
                            .await
                        {
                            Ok(_) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::Authenticated,
                            )?,
                            Err(error) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::AuthenticationFailed {
                                    message: format!("authentication failed: {error}"),
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::SetConfigOption {
                        pane,
                        request,
                    } => {
                        let Some(session_id) = sessions.get(&pane).cloned() else {
                            permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SettingFailed {
                                    pane,
                                    message: "agent session is not ready".to_owned(),
                                    option_id: request.config_id,
                                    origin: request.origin,
                                },
                            )?;
                            continue;
                        };
                        match connection
                            .send_request(SetSessionConfigOptionRequest::new(
                                session_id,
                                request.config_id.clone(),
                                SessionConfigOptionValue::value_id(request.value.clone()),
                            ))
                            .block_task()
                            .await
                        {
                            Ok(response) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::ConfigOptionsChanged {
                                    pane,
                                    config_options: response.config_options,
                                    request,
                                },
                            )?,
                            Err(error) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SettingFailed {
                                    pane,
                                    message: format!("could not change agent setting: {error}"),
                                    option_id: request.config_id,
                                    origin: request.origin,
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::SetMode {
                        pane,
                        mode_id,
                        origin,
                    } => {
                        let Some(session_id) = sessions.get(&pane).cloned() else {
                            permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SettingFailed {
                                    pane,
                                    message: "agent session is not ready".to_owned(),
                                    option_id: LEGACY_MODE_PREFERENCE_ID.to_owned(),
                                    origin,
                                },
                            )?;
                            continue;
                        };
                        match connection
                            .send_request(SetSessionModeRequest::new(
                                session_id,
                                mode_id.clone(),
                            ))
                            .block_task()
                            .await
                        {
                            Ok(_) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::ModeChanged {
                                    pane,
                                    mode_id,
                                    origin,
                                },
                            )?,
                            Err(error) => permission_safe_send(
                                &event_tx,
                                RuntimeEvent::SettingFailed {
                                    pane,
                                    message: format!(
                                        "could not change agent permission mode: {error}"
                                    ),
                                    option_id: LEGACY_MODE_PREFERENCE_ID.to_owned(),
                                    origin,
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::Shutdown => {
                        for (pane, session_id) in sessions
                            .iter()
                            .map(|(pane, session_id)| (*pane, session_id.clone()))
                            .collect::<Vec<_>>()
                        {
                            if session_capabilities.close {
                                if let Err(error) = connection
                                    .send_request(CloseSessionRequest::new(session_id))
                                    .block_task()
                                    .await
                                {
                                    log::warn!(target: "zz::agent", "could not close ACP session during shutdown: {error}");
                                }
                            } else {
                                connection
                                    .send_notification(CancelNotification::new(session_id))?;
                            }
                            cancel_pending_permissions(&routing, pane, &event_tx)?;
                        }
                        break;
                    }
                }
            }
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())
}

#[track_caller]
fn spawn_close_session(
    connection: &ConnectionTo<Agent>,
    session_id: AcpSessionId,
    reason: &'static str,
) -> Result<(), agent_client_protocol::Error> {
    let close = connection.send_request(CloseSessionRequest::new(session_id));
    connection.spawn(async move {
        if let Err(error) = close.block_task().await {
            log::warn!(target: "zz::agent", "could not close {reason}: {error}");
        }
        Ok(())
    })
}

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
        && session.cwd.is_absolute()
        && session
            .additional_directories
            .iter()
            .all(|directory| directory.is_absolute())
        && session.title.as_deref().is_none_or(|title| {
            title.len() <= MAX_SESSION_TITLE_BYTES && !title.chars().any(char::is_control)
        })
        && session.updated_at.as_deref().is_none_or(|timestamp| {
            timestamp.len() <= MAX_SESSION_TIMESTAMP_BYTES
                && !timestamp.chars().any(char::is_control)
        })
}

fn clean_session_metadata(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
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

fn permission_safe_send(
    sender: &Sender<RuntimeEvent>,
    event: RuntimeEvent,
) -> Result<(), agent_client_protocol::Error> {
    sender.try_send(event).map_err(|error| {
        agent_client_protocol::Error::internal_error()
            .data(format!("agent UI event channel is unavailable: {error}"))
    })
}

fn cancel_pending_permissions(
    routing: &Arc<Mutex<RuntimeRouting>>,
    pane: PaneId,
    event_tx: &Sender<RuntimeEvent>,
) -> Result<(), agent_client_protocol::Error> {
    let pending_ids = routing
        .lock()
        .permissions
        .iter()
        .filter_map(|(id, pending)| (pending.pane == pane).then_some(*id))
        .collect::<Vec<_>>();
    for request_id in pending_ids {
        let pending = routing.lock().permissions.remove(&request_id);
        if let Some(pending) = pending {
            pending.responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ))?;
            permission_safe_send(
                event_tx,
                RuntimeEvent::PermissionResolved {
                    pane,
                    request_id,
                    canceled: true,
                },
            )?;
        }
    }
    Ok(())
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

fn auth_method_model(method: &AuthMethod) -> AgentAuthMethod {
    AgentAuthMethod {
        id: method.id().0.to_string(),
        name: method.name().to_owned(),
        description: method.description().map(ToOwned::to_owned),
    }
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
            .filter(|command| !command.trim().is_empty())
            .unwrap_or_else(|| fallback_label.to_owned());
        terminal.clear();
        if !command.trim().is_empty() {
            terminal.push_str("$ ");
            terminal.push_str(command.trim());
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
    true
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

fn prompt_blocks(text: String, images: &[Arc<Image>]) -> Vec<ContentBlock> {
    let mut blocks = Vec::with_capacity(usize::from(!text.is_empty()) + images.len());
    if !text.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(text)));
    }
    blocks.extend(images.iter().map(|image| {
        ContentBlock::Image(ImageContent::new(
            BASE64.encode(&image.bytes),
            image.format.mime_type(),
        ))
    }));
    blocks
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
    tool.raw_input
        .as_ref()
        .and_then(pretty_json)
        .map(ToolPayload::Json)
}

fn tool_output(tool: &ToolCall) -> Vec<ToolPayload> {
    let structured = tool_content_payloads(&tool.content);
    if structured.is_empty() {
        tool.raw_output
            .as_ref()
            .and_then(pretty_json)
            .map(ToolPayload::Json)
            .into_iter()
            .collect()
    } else {
        structured
    }
}

fn tool_content_payloads(content: &[ToolCallContent]) -> Vec<ToolPayload> {
    content.iter().map(tool_content_payload).collect()
}

fn tool_content_payload(content: &ToolCallContent) -> ToolPayload {
    match content {
        ToolCallContent::Diff(diff) => ToolPayload::Diff {
            path: diff.path.display().to_string(),
            old: diff.old_text.clone(),
            new: diff.new_text.clone(),
        },
        ToolCallContent::Content(content) => match &content.content {
            ContentBlock::Text(text) => ToolPayload::Text(text.text.clone()),
            _ => ToolPayload::Json(pretty_json(content).unwrap_or_default()),
        },
        ToolCallContent::Terminal(terminal) => {
            ToolPayload::Terminal(format!("[terminal {}]", terminal.terminal_id.0))
        }
        _ => ToolPayload::Json(pretty_json(content).unwrap_or_default()),
    }
}

fn pretty_json(value: &impl serde::Serialize) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

fn pretty_json_markdown(value: &impl serde::Serialize) -> String {
    pretty_json(value).map_or_else(String::new, |value| format!("```json\n{value}\n```"))
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethodAgent, AvailableCommandsUpdate, CloseSessionResponse,
        ConfigOptionUpdate, ContentChunk, DeleteSessionResponse, Diff, InitializeResponse,
        ListSessionsResponse, LoadSessionResponse, MessageId, NewSessionRequest,
        NewSessionResponse, PlanEntry, PlanEntryPriority, PromptResponse,
        SessionAdditionalDirectoriesCapabilities, SessionCapabilities, SessionCloseCapabilities,
        SessionConfigSelectOption, SessionDeleteCapabilities, SessionInfo, SessionListCapabilities,
        SessionMode, SessionModeState, SetSessionConfigOptionResponse, SetSessionModeResponse,
        Terminal, ToolCallLocation, ToolCallUpdateFields,
    };

    use super::*;
    use agent_client_protocol::schema::v1::SessionNotification;

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
        let pane = PaneId(9);
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

        let next = |thread: &AgentThread| preferred_setting_command(thread, &preferences, pane);
        assert!(matches!(
            next(&thread),
            Some(RuntimeCommand::SetConfigOption {
                request: AgentSettingRequest { ref config_id, .. },
                ..
            }) if config_id == "model"
        ));
        Arc::make_mut(&mut thread.config_options)[0].current_value = "large".to_owned();
        assert!(matches!(
            next(&thread),
            Some(RuntimeCommand::SetConfigOption {
                request: AgentSettingRequest { ref config_id, .. },
                ..
            }) if config_id == "effort"
        ));
        Arc::make_mut(&mut thread.config_options)[1].current_value = "high".to_owned();
        assert!(matches!(
            next(&thread),
            Some(RuntimeCommand::SetConfigOption {
                request: AgentSettingRequest { ref config_id, .. },
                ..
            }) if config_id == "permission"
        ));
    }

    #[test]
    fn sticky_legacy_permission_mode_can_coexist_with_other_config_options() {
        let pane = PaneId(10);
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
            preferred_setting_command(&thread, &preferences, pane),
            Some(RuntimeCommand::SetMode { ref mode_id, .. }) if mode_id == "code"
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
    fn a_prompt_carries_its_attachments_as_inline_image_blocks() {
        let image = attachment();
        let blocks = prompt_blocks("look at this".to_owned(), std::slice::from_ref(&image));

        assert_eq!(blocks.len(), 2, "the text leads, the image follows");
        assert!(matches!(&blocks[0], ContentBlock::Text(text) if text.text == "look at this"));
        let ContentBlock::Image(sent) = &blocks[1] else {
            panic!("the attachment should travel as an image block");
        };
        assert_eq!(sent.mime_type, "image/png");
        assert_eq!(
            BASE64.decode(&sent.data).expect("data should be base64"),
            PNG_PIXEL,
            "the agent should receive the bytes we rendered"
        );

        let alone = prompt_blocks(String::new(), std::slice::from_ref(&image));
        assert!(matches!(alone.as_slice(), [ContentBlock::Image(_)]));
    }

    #[test]
    fn a_replayed_user_image_returns_to_the_transcript() {
        let mut thread = thread();
        let sent = prompt_blocks(String::new(), &[attachment()]);

        thread.apply_update(SessionUpdate::UserMessageChunk(ContentChunk::new(
            sent[0].clone(),
        )));

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

    fn next_runtime_event(receiver: &Receiver<RuntimeEvent>) -> RuntimeEvent {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            match receiver.try_recv() {
                Ok(event) => return event,
                Err(async_channel::TryRecvError::Empty) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "timed out waiting for ACP runtime event"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(async_channel::TryRecvError::Closed) => {
                    panic!("ACP runtime event channel closed unexpectedly")
                }
            }
        }
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
    }

    #[test]
    fn acp_runtime_lists_and_transactionally_switches_sessions() {
        let (list_tx, list_rx) = std::sync::mpsc::channel();
        let (prompt_tx, prompt_rx) = std::sync::mpsc::channel();
        let (closed_tx, closed_rx) = std::sync::mpsc::channel();
        let (deleted_tx, deleted_rx) = std::sync::mpsc::channel();
        let agent = Agent
            .builder()
            .on_receive_request(
                async |initialize: InitializeRequest, responder, _| {
                    assert!(!initialize.client_capabilities.terminal);
                    let meta = initialize
                        .client_capabilities
                        .meta
                        .as_ref()
                        .expect("Claude profile metadata");
                    assert_eq!(
                        meta.get("terminal_output"),
                        Some(&serde_json::Value::Bool(true))
                    );
                    assert_eq!(
                        meta.get("subagent-transcript"),
                        Some(&serde_json::Value::Bool(true))
                    );
                    responder.respond(
                        InitializeResponse::new(initialize.protocol_version).agent_capabilities(
                            AgentCapabilities::new()
                                .load_session(true)
                                .session_capabilities(
                                    SessionCapabilities::new()
                                        .list(SessionListCapabilities::new())
                                        .delete(SessionDeleteCapabilities::new())
                                        .close(SessionCloseCapabilities::new())
                                        .additional_directories(
                                            SessionAdditionalDirectoriesCapabilities::new(),
                                        ),
                                ),
                        ),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async |request: NewSessionRequest, responder, _| {
                    assert_eq!(request.cwd, PathBuf::from("/tmp/zz-agent-history-test"));
                    responder.respond(NewSessionResponse::new("active-session"))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let list_tx = list_tx.clone();
                    async move |request: ListSessionsRequest, responder, _| {
                        list_tx
                            .send((request.cwd, request.cursor))
                            .map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(error.to_string())
                            })?;
                        responder.respond(
                            ListSessionsResponse::new(vec![
                                SessionInfo::new("history-session", "/tmp/zz-agent-history-test")
                                    .additional_directories(vec![PathBuf::from("/tmp/shared")])
                                    .title("Previous work")
                                    .updated_at("2026-07-20T17:00:00Z"),
                                SessionInfo::new("invalid-session", "relative/path"),
                            ])
                            .next_cursor("next-page"),
                        )
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async |request: LoadSessionRequest, responder, connection| match request
                    .session_id
                    .0
                    .as_ref()
                {
                    "history-session" => {
                        assert_eq!(
                            request.additional_directories,
                            vec![PathBuf::from("/tmp/shared")]
                        );
                        connection.send_notification(SessionNotification::new(
                            request.session_id,
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new("loaded history")),
                            )),
                        ))?;
                        responder.respond(LoadSessionResponse::new())
                    }
                    "missing-session" => {
                        connection.send_notification(SessionNotification::new(
                            request.session_id,
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new("discard this replay")),
                            )),
                        ))?;
                        responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params()
                                .data("fixture session is missing"),
                        )
                    }
                    _ => responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params()
                            .data("unexpected fixture session"),
                    ),
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let prompt_tx = prompt_tx.clone();
                    async move |request: PromptRequest, responder, _| {
                        prompt_tx
                            .send(request.session_id.0.to_string())
                            .map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(error.to_string())
                            })?;
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let closed_tx = closed_tx.clone();
                    async move |request: CloseSessionRequest, responder, _| {
                        closed_tx
                            .send(request.session_id.0.to_string())
                            .map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(error.to_string())
                            })?;
                        responder.respond(CloseSessionResponse::new())
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: DeleteSessionRequest, responder, _| {
                    deleted_tx
                        .send(request.session_id.0.to_string())
                        .map_err(|error| {
                            agent_client_protocol::Error::internal_error().data(error.to_string())
                        })?;
                    responder.respond(DeleteSessionResponse::new())
                },
                agent_client_protocol::on_receive_request!(),
            );

        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let runtime = std::thread::spawn(move || {
            pollster::block_on(run_agent_connection(
                AgentProvider::ClaudeCode,
                agent,
                command_rx,
                event_tx,
            ))
        });
        let pane = PaneId(70);
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::Ready {
                session_capabilities: AgentSessionCapabilities {
                    load: true,
                    list: true,
                    close: true,
                    delete: true,
                    additional_directories: true,
                    images: false,
                },
                ..
            }
        ));
        command_tx
            .send_blocking(RuntimeCommand::Open {
                pane,
                cwd: PathBuf::from("/tmp/zz-agent-history-test"),
                resume_session: None,
            })
            .expect("open session");
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::SessionReset { pane: event_pane, restoring: false }
                if event_pane == pane
        ));
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::SessionReady { pane: event_pane, session_id, .. }
                if event_pane == pane && session_id == "active-session"
        ));

        command_tx
            .send_blocking(RuntimeCommand::ListSessions {
                pane,
                cwd: Some(PathBuf::from("/tmp/zz-agent-history-test")),
                cursor: None,
                replace: true,
            })
            .expect("list sessions");
        assert_eq!(
            list_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("list request"),
            (Some(PathBuf::from("/tmp/zz-agent-history-test")), None)
        );
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::SessionsListed { sessions, next_cursor, .. }
                if sessions.len() == 1
                    && sessions[0].session_id == "history-session"
                    && sessions[0].title.as_deref() == Some("Previous work")
                    && next_cursor.as_deref() == Some("next-page")
        ));

        command_tx
            .send_blocking(RuntimeCommand::SwitchSession {
                pane,
                session: AgentSessionSummary {
                    session_id: "missing-session".to_owned(),
                    cwd: PathBuf::from("/tmp/zz-agent-history-test"),
                    additional_directories: Vec::new(),
                    title: None,
                    updated_at: None,
                },
            })
            .expect("switch to missing session");
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::SessionSwitchFailed { pane: event_pane, message }
                if event_pane == pane && message.contains("fixture session is missing")
        ));
        assert_eq!(
            closed_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("failed target session closed"),
            "missing-session"
        );
        command_tx
            .send_blocking(RuntimeCommand::Prompt {
                pane,
                text: "still active".to_owned(),
                images: Vec::new(),
            })
            .expect("prompt old session after failed switch");
        assert_eq!(
            prompt_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("prompt request after failed switch"),
            "active-session"
        );
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::PromptFinished { pane: event_pane, result: Ok(StopReason::EndTurn) }
                if event_pane == pane
        ));

        command_tx
            .send_blocking(RuntimeCommand::SwitchSession {
                pane,
                session: AgentSessionSummary {
                    session_id: "history-session".to_owned(),
                    cwd: PathBuf::from("/tmp/zz-agent-history-test"),
                    additional_directories: vec![PathBuf::from("/tmp/shared")],
                    title: Some("Previous work".to_owned()),
                    updated_at: Some("2026-07-20T17:00:00Z".to_owned()),
                },
            })
            .expect("switch to history session");
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::SessionSwitched { pane: event_pane, session_id, replay, .. }
                if event_pane == pane
                    && session_id == "history-session"
                    && matches!(
                        replay.as_slice(),
                        [SessionUpdate::AgentMessageChunk(chunk)]
                            if matches!(
                                &chunk.content,
                                ContentBlock::Text(text) if text.text == "loaded history"
                            )
                    )
        ));
        assert_eq!(
            closed_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("old session closed"),
            "active-session"
        );

        command_tx
            .send_blocking(RuntimeCommand::DeleteSession {
                pane,
                session_id: "obsolete-session".to_owned(),
            })
            .expect("delete session");
        assert_eq!(
            deleted_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("delete request"),
            "obsolete-session"
        );
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::SessionDeleted { pane: event_pane, session_id }
                if event_pane == pane && session_id == "obsolete-session"
        ));

        command_tx
            .send_blocking(RuntimeCommand::Shutdown)
            .expect("shutdown command");
        assert_eq!(
            closed_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("active history session closed"),
            "history-session"
        );
        assert!(runtime.join().expect("runtime thread").is_ok());
    }

    #[test]
    fn acp_runtime_restores_streams_approves_cancels_and_shuts_down() {
        let (cancel_tx, cancel_rx) = async_channel::unbounded::<AcpSessionId>();
        let (permission_tx, permission_rx) = std::sync::mpsc::channel();
        let (setting_tx, setting_rx) = std::sync::mpsc::channel();
        let prompt_count = Arc::new(AtomicU64::new(0));
        let prompt_cancels = cancel_rx.clone();
        let prompt_permissions = permission_tx.clone();
        let prompts = Arc::clone(&prompt_count);
        let agent = Agent
            .builder()
            .on_receive_request(
                async |initialize: InitializeRequest, responder, _| {
                    assert!(
                        initialize
                            .client_capabilities
                            .session
                            .as_ref()
                            .and_then(|session| session.config_options.as_ref())
                            .is_some()
                    );
                    assert!(!initialize.client_capabilities.terminal);
                    let meta = initialize
                        .client_capabilities
                        .meta
                        .as_ref()
                        .expect("Codex profile metadata");
                    assert_eq!(
                        meta.get("terminal_output"),
                        Some(&serde_json::Value::Bool(true))
                    );
                    assert!(!meta.contains_key("subagent-transcript"));
                    responder.respond(
                        InitializeResponse::new(initialize.protocol_version)
                            .agent_capabilities(AgentCapabilities::new().load_session(true))
                            .agent_info(
                                Implementation::new("zz-test-agent", "1.0").title("zz test agent"),
                            )
                            .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                                "fixture-login",
                                "Fixture login",
                            ))]),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async |request: AuthenticateRequest, responder, _| {
                    assert_eq!(request.method_id.0.as_ref(), "fixture-login");
                    responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params()
                            .data("fixture authentication failed"),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async |request: NewSessionRequest, responder, _| {
                    assert!(request.cwd.is_absolute());
                    responder.respond(NewSessionResponse::new("new-session").config_options(vec![
                            SessionConfigOption::select(
                                "reasoning_effort",
                                "Reasoning effort",
                                "medium",
                                vec![
                                    SessionConfigSelectOption::new("medium", "Medium"),
                                    SessionConfigSelectOption::new("high", "High"),
                                ],
                            )
                            .category(SessionConfigOptionCategory::ThoughtLevel),
                        ]))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async |request: LoadSessionRequest, responder, connection| {
                    assert!(request.cwd.is_absolute());
                    if request.session_id.0.as_ref() == "restored-session" {
                        connection.send_notification(SessionNotification::new(
                            request.session_id,
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new("restored history")),
                            )),
                        ))?;
                        responder.respond(LoadSessionResponse::new().modes(SessionModeState::new(
                            "code",
                            vec![SessionMode::new("code", "Code")],
                        )))
                    } else {
                        responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params()
                                .data("fixture session is missing"),
                        )
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let setting_tx = setting_tx.clone();
                    async move |request: SetSessionModeRequest, responder, _| {
                        setting_tx
                            .send(format!("mode:{}", request.mode_id.0))
                            .map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(error.to_string())
                            })?;
                        responder.respond(SetSessionModeResponse::new())
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let setting_tx = setting_tx.clone();
                    async move |request: SetSessionConfigOptionRequest, responder, _| {
                        let value = request
                            .value
                            .as_value_id()
                            .map(|value| value.0.to_string())
                            .unwrap_or_default();
                        setting_tx
                            .send(format!("config:{}:{value}", request.config_id.0))
                            .map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(error.to_string())
                            })?;
                        responder.respond(SetSessionConfigOptionResponse::new(vec![
                            SessionConfigOption::select(
                                "reasoning_effort",
                                "Reasoning effort",
                                value,
                                vec![
                                    SessionConfigSelectOption::new("medium", "Medium"),
                                    SessionConfigSelectOption::new("high", "High"),
                                ],
                            )
                            .category(SessionConfigOptionCategory::ThoughtLevel),
                        ]))
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: PromptRequest, responder, connection| {
                    let prompt_index = prompts.fetch_add(1, Ordering::Relaxed);
                    if prompt_index != 1 {
                        connection.send_notification(SessionNotification::new(
                            request.session_id.clone(),
                            SessionUpdate::ToolCall(
                                ToolCall::new("tool-1", "Write fixture")
                                    .kind(ToolKind::Edit)
                                    .status(ToolCallStatus::InProgress),
                            ),
                        ))?;
                        let permission = connection.send_request(RequestPermissionRequest::new(
                            request.session_id,
                            ToolCallUpdate::new(
                                "tool-1",
                                ToolCallUpdateFields::new().title("Write fixture".to_owned()),
                            ),
                            vec![PermissionOption::new(
                                "allow-once",
                                "Allow once",
                                PermissionOptionKind::AllowOnce,
                            )],
                        ));
                        let permission_tx = prompt_permissions.clone();
                        permission.on_receiving_result(async move |result| {
                            let response = result?;
                            let stop_reason = if matches!(
                                response.outcome,
                                RequestPermissionOutcome::Cancelled
                            ) {
                                StopReason::Cancelled
                            } else {
                                StopReason::EndTurn
                            };
                            permission_tx.send(response.outcome).map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(error.to_string())
                            })?;
                            responder.respond(PromptResponse::new(stop_reason))
                        })?;
                    } else {
                        connection.send_notification(SessionNotification::new(
                            request.session_id,
                            SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new("waiting for cancellation")),
                            )),
                        ))?;
                        let cancel_rx = prompt_cancels.clone();
                        connection.spawn(async move {
                            cancel_rx.recv().await.map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(error.to_string())
                            })?;
                            responder.respond(PromptResponse::new(StopReason::Cancelled))
                        })?;
                    }
                    Ok(())
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_notification(
                async move |cancel: CancelNotification, _| {
                    cancel_tx.try_send(cancel.session_id).map_err(|error| {
                        agent_client_protocol::Error::internal_error().data(error.to_string())
                    })
                },
                agent_client_protocol::on_receive_notification!(),
            );

        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let runtime = std::thread::spawn(move || {
            pollster::block_on(run_agent_connection(
                AgentProvider::Codex,
                agent,
                command_rx,
                event_tx,
            ))
        });

        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::Ready {
                agent_name,
                auth_methods,
                ..
            } if agent_name == "zz test agent"
                && auth_methods.first().is_some_and(|method| method.id == "fixture-login")
        ));
        command_tx
            .send_blocking(RuntimeCommand::Authenticate {
                method_id: "fixture-login".to_owned(),
            })
            .expect("authentication command");
        assert!(matches!(
            next_runtime_event(&event_rx),
            RuntimeEvent::AuthenticationFailed { message }
                if message.contains("fixture authentication failed")
        ));
        let pane = PaneId(44);
        command_tx
            .send_blocking(RuntimeCommand::Open {
                pane,
                cwd: PathBuf::from("/tmp/zz-agent-runtime-test"),
                resume_session: Some("restored-session".to_owned()),
            })
            .expect("open command");

        let mut saw_reset = false;
        let mut saw_history = false;
        let mut saw_ready = false;
        while !(saw_reset && saw_history && saw_ready) {
            match next_runtime_event(&event_rx) {
                RuntimeEvent::SessionReset {
                    pane: event_pane,
                    restoring,
                } => saw_reset = event_pane == pane && restoring,
                RuntimeEvent::SessionUpdate {
                    pane: event_pane,
                    update: SessionUpdate::AgentMessageChunk(chunk),
                } => {
                    saw_history = event_pane == pane
                        && matches!(
                            chunk.content,
                            ContentBlock::Text(text) if text.text == "restored history"
                        );
                }
                RuntimeEvent::SessionReady {
                    pane: event_pane,
                    session_id,
                    modes,
                    config_options,
                } => {
                    saw_ready = event_pane == pane
                        && session_id == "restored-session"
                        && config_options.is_none()
                        && modes.is_some_and(|modes| modes.current_mode_id.0.as_ref() == "code");
                }
                _ => {}
            }
        }

        command_tx
            .send_blocking(RuntimeCommand::SetMode {
                pane,
                mode_id: "code".to_owned(),
                origin: AgentSettingOrigin::User(Some(AgentPreferenceKind::Permission)),
            })
            .expect("set legacy mode command");
        assert_eq!(
            setting_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("agent received mode setting"),
            "mode:code"
        );
        loop {
            if matches!(
                next_runtime_event(&event_rx),
                RuntimeEvent::ModeChanged {
                    pane: event_pane,
                    mode_id,
                    ..
                } if event_pane == pane && mode_id == "code"
            ) {
                break;
            }
        }

        let fallback_pane = PaneId(45);
        command_tx
            .send_blocking(RuntimeCommand::Open {
                pane: fallback_pane,
                cwd: PathBuf::from("/tmp/zz-agent-runtime-test"),
                resume_session: Some("missing-session".to_owned()),
            })
            .expect("fallback open command");
        let mut saw_fallback_reset = false;
        let mut saw_fallback_ready = false;
        while !(saw_fallback_reset && saw_fallback_ready) {
            match next_runtime_event(&event_rx) {
                RuntimeEvent::SessionReset {
                    pane: event_pane,
                    restoring,
                } => saw_fallback_reset = event_pane == fallback_pane && restoring,
                RuntimeEvent::SessionReady {
                    pane: event_pane,
                    session_id,
                    ..
                } => {
                    saw_fallback_ready = event_pane == fallback_pane && session_id == "new-session";
                }
                _ => {}
            }
        }

        command_tx
            .send_blocking(RuntimeCommand::SetConfigOption {
                pane: fallback_pane,
                request: AgentSettingRequest {
                    config_id: "reasoning_effort".to_owned(),
                    value: "high".to_owned(),
                    origin: AgentSettingOrigin::User(Some(AgentPreferenceKind::Effort)),
                },
            })
            .expect("set config option command");
        assert_eq!(
            setting_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("agent received config setting"),
            "config:reasoning_effort:high"
        );
        loop {
            if matches!(
                next_runtime_event(&event_rx),
                RuntimeEvent::ConfigOptionsChanged {
                    pane: event_pane,
                    config_options,
                    ..
                } if event_pane == fallback_pane
                    && config_options.first().is_some_and(|option| {
                        matches!(
                            &option.kind,
                            SessionConfigKind::Select(select)
                                if select.current_value.0.as_ref() == "high"
                        )
                    })
            ) {
                break;
            }
        }

        command_tx
            .send_blocking(RuntimeCommand::Prompt {
                pane,
                text: "request permission".to_owned(),
                images: Vec::new(),
            })
            .expect("prompt command");
        let request_id = loop {
            if let RuntimeEvent::PermissionRequested {
                pane: event_pane,
                request_id,
                options,
                ..
            } = next_runtime_event(&event_rx)
            {
                assert_eq!(event_pane, pane);
                assert_eq!(options[0].option_id.0.as_ref(), "allow-once");
                break request_id;
            }
        };
        command_tx
            .send_blocking(RuntimeCommand::RespondPermission {
                request_id,
                option_id: Some("allow-once".to_owned()),
            })
            .expect("permission response");
        let outcome = permission_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("agent received permission response");
        assert!(matches!(
            outcome,
            RequestPermissionOutcome::Selected(selected)
                if selected.option_id.0.as_ref() == "allow-once"
        ));
        let mut saw_resolved = false;
        let mut saw_completed = false;
        while !(saw_resolved && saw_completed) {
            match next_runtime_event(&event_rx) {
                RuntimeEvent::PermissionResolved {
                    pane: event_pane,
                    request_id: event_request,
                    canceled,
                } => {
                    saw_resolved = event_pane == pane && event_request == request_id && !canceled;
                }
                RuntimeEvent::PromptFinished {
                    pane: event_pane,
                    result: Ok(StopReason::EndTurn),
                } => saw_completed = event_pane == pane,
                _ => {}
            }
        }

        command_tx
            .send_blocking(RuntimeCommand::Prompt {
                pane,
                text: "wait".to_owned(),
                images: Vec::new(),
            })
            .expect("cancelable prompt");
        loop {
            if matches!(
                next_runtime_event(&event_rx),
                RuntimeEvent::SessionUpdate {
                    pane: event_pane,
                    update: SessionUpdate::AgentThoughtChunk(_),
                } if event_pane == pane
            ) {
                break;
            }
        }
        command_tx
            .send_blocking(RuntimeCommand::Cancel { pane })
            .expect("cancel command");
        loop {
            if matches!(
                next_runtime_event(&event_rx),
                RuntimeEvent::PromptFinished {
                    pane: event_pane,
                    result: Ok(StopReason::Cancelled),
                } if event_pane == pane
            ) {
                break;
            }
        }

        command_tx
            .send_blocking(RuntimeCommand::Prompt {
                pane,
                text: "cancel permission".to_owned(),
                images: Vec::new(),
            })
            .expect("permission prompt to cancel");
        let canceled_request = loop {
            if let RuntimeEvent::PermissionRequested {
                pane: event_pane,
                request_id,
                ..
            } = next_runtime_event(&event_rx)
            {
                assert_eq!(event_pane, pane);
                break request_id;
            }
        };
        command_tx
            .send_blocking(RuntimeCommand::Cancel { pane })
            .expect("cancel pending permission");
        assert!(matches!(
            permission_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("agent received canceled permission response"),
            RequestPermissionOutcome::Cancelled
        ));
        let mut saw_permission_cancel = false;
        let mut saw_prompt_cancel = false;
        while !(saw_permission_cancel && saw_prompt_cancel) {
            match next_runtime_event(&event_rx) {
                RuntimeEvent::PermissionResolved {
                    pane: event_pane,
                    request_id,
                    canceled,
                } => {
                    saw_permission_cancel =
                        event_pane == pane && request_id == canceled_request && canceled;
                }
                RuntimeEvent::PromptFinished {
                    pane: event_pane,
                    result: Ok(StopReason::Cancelled),
                } => saw_prompt_cancel = event_pane == pane,
                _ => {}
            }
        }

        command_tx
            .send_blocking(RuntimeCommand::Shutdown)
            .expect("shutdown command");
        assert_eq!(
            runtime.join().expect("runtime thread did not panic"),
            Ok(())
        );
    }
}
