use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use zz_protocol::{
    AgentCommand, AgentPaneWire, BrowserCommand, ChooseBufferSearchState, ChooseBufferState,
    ChooseTreeSearchState, ChooseTreeState, ChooserPresentation, ClientExitAction,
    ClientMessageKind, ClientView, ClipboardProducer, CommandPromptState, CommandResponse,
    ConfirmState, DisplayPanesState, Event, EventPayload, KeyBindingSnapshot, KeyTableSnapshot,
    MenuState, MouseBindings, MuxOptions, MuxSnapshot, PaneId, PopupState, ProtocolMessage,
    ServerHello, SessionId, StatusLine, TerminalUiCommand, Welcome, key_tables_hash,
};
use zz_terminal::{
    AppearanceProvenance, ClipboardTarget, PackedCell, TerminalAppearance, TerminalDictionary,
    TerminalPatchFields, TerminalViewport, TerminalViewportPatch,
};

use crate::scrollback::{
    HistoryRing, MAX_HISTORY_ROWS, RetainedTerminalViewport, apply_history_chunk,
    apply_retained_patch, new_retained_viewport, replace_retained_viewport,
};

/// Which viewport rows a terminal frame or patch touched, so a skin repaints
/// only damaged panes and rows instead of the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewportDamage {
    All,
    Rows(Vec<u16>),
}

/// A wire request the core needs its shell to send. Drained with
/// [`ClientCore::poll_outbound`] after every [`ClientCore::handle_message`].
///
/// Event sequence numbers are deliberately **not** gap-checked: the daemon's
/// outbound mailbox supersedes stale terminal frames under backpressure, so a
/// healthy stream legitimately skips sequences. `Resync` stays a shell-level
/// error-path request, never an automatic reaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outbound {
    /// A patch could not apply; ask the daemon for a full viewport.
    RequestFull(PaneId),
    TreeSync,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentAttentionStatus {
    Idle,
    Working,
    NeedsInput,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentAttentionEdge {
    Request,
    Done,
    Failed,
}

#[must_use]
pub fn agent_attention_status(state: &AgentPaneWire) -> AgentAttentionStatus {
    if state.pending_permission.is_some() {
        return AgentAttentionStatus::NeedsInput;
    }
    match &state.phase {
        zz_protocol::AgentConnectionPhase::Running
        | zz_protocol::AgentConnectionPhase::AwaitingPermission => AgentAttentionStatus::Working,
        zz_protocol::AgentConnectionPhase::Failed { .. } => AgentAttentionStatus::Failed,
        zz_protocol::AgentConnectionPhase::Starting | zz_protocol::AgentConnectionPhase::Ready => {
            AgentAttentionStatus::Idle
        }
    }
}

fn agent_attention_edge(
    previous: &AgentPaneWire,
    current: &AgentPaneWire,
) -> Option<AgentAttentionEdge> {
    let previous = agent_attention_status(previous);
    let current = agent_attention_status(current);
    match (previous, current) {
        (previous, AgentAttentionStatus::NeedsInput)
            if previous != AgentAttentionStatus::NeedsInput =>
        {
            Some(AgentAttentionEdge::Request)
        }
        (AgentAttentionStatus::Working, AgentAttentionStatus::Idle) => {
            Some(AgentAttentionEdge::Done)
        }
        (previous, AgentAttentionStatus::Failed) if previous != AgentAttentionStatus::Failed => {
            Some(AgentAttentionEdge::Failed)
        }
        _ => None,
    }
}

/// One state change or side effect produced by reduction. State changes are
/// notifications — read the new value through the accessors; side effects
/// (clipboard, URIs, GUI work) carry their payload because the core stores
/// none of it.
#[derive(Clone, Debug, PartialEq)]
pub enum CoreEvent {
    HelloReceived,
    Attached {
        session: SessionId,
    },
    SnapshotChanged,
    ViewportChanged {
        pane: PaneId,
        damage: ViewportDamage,
    },
    AppearanceChanged,
    MuxOptionsChanged,
    KeyTablesChanged,
    StatusChanged,
    PrefixArmed {
        armed: bool,
    },
    PrefixCancelled {
        request_id: u64,
    },
    KeyTableChanged,
    CommandPromptChanged,
    CommandOutputChanged,
    ChooseTreeChanged,
    ChooseBufferChanged,
    DisplayPanesChanged,
    PopupChanged,
    MenuChanged,
    ConfirmChanged,
    PaneRemoved {
        pane: PaneId,
    },
    Bell {
        pane: PaneId,
    },
    FocusSidebar,
    OpenPathPicker {
        pane: PaneId,
        start_dir: Option<String>,
    },
    Detached {
        session: SessionId,
        by: Option<String>,
        action: ClientExitAction,
    },
    ServerStopping,
    CommandResponse(CommandResponse),
    ClientMessage {
        pane: Option<PaneId>,
        kind: ClientMessageKind,
        text: String,
        duration_ms: Option<u32>,
        /// Present only for daemon-timed messages, which are the only ones the
        /// daemon can retire early with [`CoreEvent::ClientMessageCleared`].
        message_id: Option<u64>,
    },
    /// The daemon retired the identified message. Surfaces must drop it only
    /// when the identity still matches what they are showing.
    ClientMessageCleared {
        message_id: u64,
    },
    Clipboard {
        pane: PaneId,
        request_id: u64,
        target: ClipboardTarget,
        text: String,
        /// Which of the pin's two OSC 52 writers produced this selection, which
        /// is what decides the field a raw client names.
        producer: ClipboardProducer,
    },
    OpenUri {
        pane: PaneId,
        uri: String,
    },
    AgentCommand {
        pane: PaneId,
        request_id: u64,
        command: AgentCommand,
    },
    BrowserCommand {
        pane: PaneId,
        command: BrowserCommand,
    },
    TerminalUiCommand {
        pane: PaneId,
        command: TerminalUiCommand,
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
    /// One coalesced batch of JSON agent stream items. The core stores none of
    /// them: the transcript reducer lives in the shell, and `first_seq` is the
    /// replay cursor it must track to answer
    /// [`CoreEvent::AgentLagged`].
    AgentUpdates {
        pane: PaneId,
        first_seq: u64,
        items: Vec<Vec<u8>>,
    },
    /// The pane's agent state changed; read it with [`ClientCore::agent_state`].
    AgentStateChanged {
        pane: PaneId,
        attention: Option<AgentAttentionEdge>,
    },
    /// The daemon cleared this pane's agent lane; the shell answers with
    /// `AgentReplay` from `next_seq`.
    AgentLagged {
        pane: PaneId,
        next_seq: u64,
    },
    AgentSessions {
        pane: PaneId,
        request_id: u64,
        result: String,
    },
    /// An inbound message the core does not reduce (pasted-image previews,
    /// echoing of client-to-daemon variants); the shell keeps its own handling.
    Message(Box<ProtocolMessage>),
}

type PaneMap<V> = HashMap<PaneId, V, foldhash::fast::FixedState>;

#[derive(Debug)]
struct PrefixKeys([Option<String>; 2]);

impl PrefixKeys {
    fn from_options(options: &MuxOptions) -> Self {
        Self(
            [
                zz_protocol::MuxOptionKey::Prefix,
                zz_protocol::MuxOptionKey::Prefix2,
            ]
            .map(|key| {
                options
                    .get(key)
                    .filter(|option| !option.value.eq_ignore_ascii_case("none"))
                    .map(|option| zz_protocol::canonical_key(&option.value))
            }),
        )
    }

    fn matches(&self, key: &str) -> bool {
        self.0.iter().flatten().any(|prefix| prefix == key)
    }
}

impl Default for PrefixKeys {
    fn default() -> Self {
        Self::from_options(&MuxOptions::default())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreDetachReason {
    Requested,
    Evicted,
    SessionDestroyed,
    ServerStopping,
}

/// The sans-IO client brain: decoded protocol messages in, [`CoreEvent`]s and
/// [`Outbound`] requests out, reduced state behind accessors. One instance per
/// daemon connection; a [`ProtocolMessage::ServerHello`] resets it for reuse
/// across reconnects.
#[derive(Debug, Default)]
pub struct ClientCore {
    hello_received: bool,
    capabilities: Vec<String>,
    appearance: Option<Box<TerminalAppearance>>,
    appearance_provenance: AppearanceProvenance,
    mux_options: MuxOptions,
    terminal_negotiation: Option<(Vec<String>, Vec<String>)>,
    prefix_keys: PrefixKeys,
    key_tables: Vec<KeyTableSnapshot>,
    key_tables_hash: u64,
    mouse_bindings: MouseBindings,
    status: StatusLine,
    snapshot: Arc<MuxSnapshot>,
    tree: MuxSnapshot,
    client_view: ClientView,
    tree_sync_pending: bool,
    batch_reducing: bool,
    event_group_start: usize,
    tree_dirty: bool,
    attached_session: Option<SessionId>,
    attached_read_only: bool,
    attached_client_flags: String,
    last_detach_reason: Option<CoreDetachReason>,
    viewports: PaneMap<RetainedTerminalViewport>,
    spare_cells: PaneMap<SpareCells>,
    next_row_revision: u64,
    history_limit: usize,
    agent_states: HashMap<PaneId, AgentPaneWire>,
    full_pending: HashSet<PaneId>,
    prefix_armed: bool,
    key_table: Option<(String, bool)>,
    command_prompt: Option<CommandPromptState>,
    command_output: Option<(u64, PaneId, RetainedTerminalViewport)>,
    command_output_watermark: u64,
    choose_tree: Option<ChooseTreeState>,
    choose_buffer: Option<ChooseBufferState>,
    chooser_presentation: Option<ChooserPresentation>,
    display_panes: Option<DisplayPanesState>,
    popup: Option<PopupState>,
    menu: Option<MenuState>,
    menu_opened: u64,
    confirm: Option<ConfirmState>,
    outbound: VecDeque<Outbound>,
    events: VecDeque<CoreEvent>,
}

impl ClientCore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reduce one decoded message. Drain [`Self::poll_outbound`] and
    /// [`Self::poll_event`] afterwards.
    pub fn handle_message(&mut self, message: ProtocolMessage) {
        match message {
            ProtocolMessage::ServerHello(hello) => self.reset_connection(*hello),
            ProtocolMessage::Welcome(welcome) => {
                self.clear_attachment();
                self.reset_session();
                self.appearance = None;
                self.mux_options = MuxOptions::default();
                self.refresh_prefix_keys();
                self.key_tables.clear();
                self.key_tables_hash = 0;
                self.mouse_bindings = MouseBindings::default();
                self.status = StatusLine::default();
                self.adopt_welcome(welcome);
                self.events.push_back(CoreEvent::HelloReceived);
            }
            ProtocolMessage::Batch(batch) => {
                let Ok(messages) = batch.messages() else {
                    self.request_tree_sync();
                    return;
                };
                self.begin_event_group();
                for message in messages {
                    self.handle_message(message);
                }
                self.finish_event_group();
            }
            ProtocolMessage::Attached {
                session,
                snapshot,
                read_only,
                client_flags,
            } => {
                self.attached_session = Some(session);
                self.attached_read_only = read_only;
                self.attached_client_flags = client_flags;
                self.tree = snapshot.clone();
                self.snapshot = Arc::new(snapshot);
                self.client_view = ClientView::default();
                self.tree_sync_pending = false;
                self.viewports.clear();
                self.spare_cells.clear();
                self.full_pending.clear();
                let prefix_changed = self.prefix_armed;
                let key_table_changed = self.key_table.is_some();
                let command_prompt_changed = self.command_prompt.is_some();
                let command_output_changed = self.command_output.is_some();
                let choose_tree_changed = self.choose_tree.is_some();
                let choose_buffer_changed = self.choose_buffer.is_some();
                let display_panes_changed = self.display_panes.is_some();
                let popup_changed = self.popup.is_some();
                let menu_changed = self.menu.is_some();
                let confirm_changed = self.confirm.is_some();
                self.reset_session();
                self.events.push_back(CoreEvent::Attached { session });
                self.events.push_back(CoreEvent::SnapshotChanged);
                if prefix_changed {
                    self.events
                        .push_back(CoreEvent::PrefixArmed { armed: false });
                }
                if key_table_changed {
                    self.events.push_back(CoreEvent::KeyTableChanged);
                }
                if command_prompt_changed {
                    self.events.push_back(CoreEvent::CommandPromptChanged);
                }
                if command_output_changed {
                    self.events.push_back(CoreEvent::CommandOutputChanged);
                }
                if choose_tree_changed {
                    self.events.push_back(CoreEvent::ChooseTreeChanged);
                }
                if choose_buffer_changed {
                    self.events.push_back(CoreEvent::ChooseBufferChanged);
                }
                if display_panes_changed {
                    self.events.push_back(CoreEvent::DisplayPanesChanged);
                }
                if popup_changed {
                    self.events.push_back(CoreEvent::PopupChanged);
                }
                if menu_changed {
                    self.events.push_back(CoreEvent::MenuChanged);
                }
                if confirm_changed {
                    self.events.push_back(CoreEvent::ConfirmChanged);
                }
            }
            ProtocolMessage::Event(Event {
                sequence: _,
                payload,
            }) => self.handle_payload(payload),
            ProtocolMessage::CommandResponse(response) => {
                self.events.push_back(CoreEvent::CommandResponse(response));
            }
            other => {
                self.events.push_back(CoreEvent::Message(Box::new(other)));
            }
        }
    }

    /// The next wire request the shell must send, if any.
    pub fn poll_outbound(&mut self) -> Option<Outbound> {
        self.outbound.pop_front()
    }

    /// The next state change or side effect, if any.
    pub fn poll_event(&mut self) -> Option<CoreEvent> {
        self.events.pop_front()
    }

    pub fn begin_event_group(&mut self) {
        self.batch_reducing = true;
        self.event_group_start = self.events.len();
    }

    pub fn finish_event_group(&mut self) {
        self.batch_reducing = false;
        if self.tree_dirty {
            self.materialize_tree();
        }
        let mut pending = self
            .events
            .split_off(self.event_group_start.min(self.events.len()));
        let mut grouped = VecDeque::new();
        while let Some(event) = pending.pop_front() {
            if matches!(
                event,
                CoreEvent::SnapshotChanged
                    | CoreEvent::StatusChanged
                    | CoreEvent::MuxOptionsChanged
                    | CoreEvent::KeyTablesChanged
            ) && grouped.contains(&event)
            {
                continue;
            }
            grouped.push_back(event);
        }
        self.events.append(&mut grouped);
    }

    #[must_use]
    pub const fn hello_received(&self) -> bool {
        self.hello_received
    }

    #[must_use]
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    #[must_use]
    pub fn appearance(&self) -> Option<&TerminalAppearance> {
        self.appearance.as_deref()
    }

    #[must_use]
    pub const fn appearance_provenance(&self) -> &AppearanceProvenance {
        &self.appearance_provenance
    }

    #[must_use]
    pub const fn mux_options(&self) -> &MuxOptions {
        &self.mux_options
    }

    #[must_use]
    pub const fn terminal_negotiation(&self) -> Option<&(Vec<String>, Vec<String>)> {
        self.terminal_negotiation.as_ref()
    }

    #[must_use]
    pub fn key_tables(&self) -> &[KeyTableSnapshot] {
        &self.key_tables
    }

    #[must_use]
    pub const fn key_tables_hash(&self) -> u64 {
        self.key_tables_hash
    }

    #[must_use]
    pub const fn mouse_bindings(&self) -> &MouseBindings {
        &self.mouse_bindings
    }

    #[must_use]
    pub const fn layout_generation(&self) -> u64 {
        self.client_view.layout_generation
    }

    pub fn adopt_welcome(&mut self, welcome: Welcome) {
        self.hello_received = true;
        self.capabilities = welcome.capability_strings();
        self.command_output_watermark = 0;
    }

    /// The published prefix table's bindings, or empty before the hello.
    #[must_use]
    pub fn prefix_bindings(&self) -> &[KeyBindingSnapshot] {
        self.key_tables
            .iter()
            .find(|table| table.name == "prefix")
            .map_or(&[], |table| table.bindings.as_slice())
    }

    #[must_use]
    pub const fn status(&self) -> &StatusLine {
        &self.status
    }

    #[must_use]
    pub const fn snapshot(&self) -> &Arc<MuxSnapshot> {
        &self.snapshot
    }

    #[must_use]
    pub const fn attached_session(&self) -> Option<SessionId> {
        self.attached_session
    }

    #[must_use]
    pub const fn attached_read_only(&self) -> bool {
        self.attached_read_only
    }

    #[must_use]
    pub fn attached_client_flags(&self) -> &str {
        &self.attached_client_flags
    }

    #[must_use]
    pub const fn last_detach_reason(&self) -> Option<CoreDetachReason> {
        self.last_detach_reason
    }

    #[must_use]
    pub const fn last_detach_was_session_destroyed(&self) -> bool {
        matches!(
            self.last_detach_reason,
            Some(CoreDetachReason::SessionDestroyed)
        )
    }

    #[must_use]
    pub const fn last_detach_was_server_stopping(&self) -> bool {
        matches!(
            self.last_detach_reason,
            Some(CoreDetachReason::ServerStopping)
        )
    }

    #[must_use]
    pub fn viewport(&self, pane: PaneId) -> Option<&TerminalViewport> {
        self.viewports.get(&pane).map(|retained| &retained.viewport)
    }

    #[must_use]
    pub fn retained_viewport(&self, pane: PaneId) -> Option<&RetainedTerminalViewport> {
        self.viewports.get(&pane)
    }

    pub fn retain_history(&mut self) {
        self.history_limit = MAX_HISTORY_ROWS;
        for retained in self.viewports.values_mut() {
            if retained.history.limit() != MAX_HISTORY_ROWS {
                retained.history = HistoryRing::with_limit(MAX_HISTORY_ROWS);
            }
        }
    }

    #[must_use]
    pub fn history_trickle_budget(&self) -> usize {
        self.mux_options
            .get(zz_protocol::MuxOptionKey::HistoryTrickle)
            .and_then(|option| option.value.parse::<usize>().ok())
            .unwrap_or_default()
            .min(MAX_HISTORY_ROWS)
    }

    pub fn apply_history_chunk(
        &mut self,
        pane: PaneId,
        start: u32,
        total: u32,
        offset: u32,
        columns: u16,
        rows: Vec<Vec<PackedCell>>,
        dictionary: TerminalDictionary,
    ) -> bool {
        let Some(retained) = self.viewports.get_mut(&pane) else {
            return false;
        };
        let applied = apply_history_chunk(
            retained,
            start,
            total,
            offset,
            columns,
            rows,
            dictionary,
            &mut self.next_row_revision,
        );
        if applied {
            self.events.push_back(CoreEvent::ViewportChanged {
                pane,
                damage: ViewportDamage::Rows(Vec::new()),
            });
        }
        applied
    }

    /// The daemon-published state of an agent pane, or `None` before its first
    /// publication. Read after [`CoreEvent::AgentStateChanged`].
    #[must_use]
    pub fn agent_state(&self, pane: PaneId) -> Option<&AgentPaneWire> {
        self.agent_states.get(&pane)
    }

    #[must_use]
    pub const fn prefix_armed(&self) -> bool {
        self.prefix_armed
    }

    #[must_use]
    pub fn key_table(&self) -> Option<(&str, bool)> {
        self.key_table
            .as_ref()
            .map(|(table, repeat)| (table.as_str(), *repeat))
    }

    pub fn claims_prefix_input(&self, input: &zz_terminal::KeyInput) -> bool {
        if input.modifiers.platform() {
            return false;
        }
        if self.prefix_armed || self.key_table.is_some() {
            return true;
        }
        self.prefix_keys
            .matches(zz_protocol::input_key_name(input).as_str())
    }

    fn refresh_prefix_keys(&mut self) {
        self.prefix_keys = PrefixKeys::from_options(&self.mux_options);
    }

    #[must_use]
    pub const fn command_prompt(&self) -> Option<&CommandPromptState> {
        self.command_prompt.as_ref()
    }

    #[must_use]
    pub fn command_output(&self) -> Option<(PaneId, &TerminalViewport)> {
        self.command_output
            .as_ref()
            .map(|(_, pane, retained)| (*pane, &retained.viewport))
    }

    #[must_use]
    pub fn retained_command_output(&self) -> Option<(PaneId, &RetainedTerminalViewport)> {
        self.command_output
            .as_ref()
            .map(|(_, pane, retained)| (*pane, retained))
    }

    #[must_use]
    pub fn command_output_id(&self) -> Option<u64> {
        self.command_output
            .as_ref()
            .map(|(output_id, _, _)| *output_id)
    }

    #[must_use]
    pub const fn choose_tree(&self) -> Option<&ChooseTreeState> {
        self.choose_tree.as_ref()
    }

    #[must_use]
    pub const fn choose_buffer(&self) -> Option<&ChooseBufferState> {
        self.choose_buffer.as_ref()
    }

    #[must_use]
    pub const fn chooser_presentation(&self) -> Option<&ChooserPresentation> {
        self.chooser_presentation.as_ref()
    }

    #[must_use]
    pub const fn display_panes(&self) -> Option<&DisplayPanesState> {
        self.display_panes.as_ref()
    }

    #[must_use]
    pub const fn popup(&self) -> Option<&PopupState> {
        self.popup.as_ref()
    }

    #[must_use]
    pub const fn menu(&self) -> Option<&MenuState> {
        self.menu.as_ref()
    }

    #[must_use]
    pub const fn menu_opened(&self) -> u64 {
        self.menu_opened
    }

    #[must_use]
    pub const fn confirm(&self) -> Option<&ConfirmState> {
        self.confirm.as_ref()
    }

    /// Adopt a handshake's settings — capabilities, appearance, options, key
    /// tables, status — and nothing else. A shell that keeps rendering its
    /// last frame across a reconnect calls this instead of feeding the hello
    /// through [`Self::handle_message`], which is the whole reset:
    /// `adopt_hello` + [`Self::clear_attachment`] + [`Self::reset_session`].
    ///
    /// Emits no events, for the same reason as [`Self::reset_session`].
    pub fn adopt_hello(&mut self, hello: ServerHello) {
        self.command_output_watermark = 0;
        let ServerHello {
            protocol_version: _,
            server_id: _,
            client_id: _,
            client_instance_id: _,
            capabilities,
            appearance,
            appearance_provenance,
            mux_options,
            status,
            key_tables,
        } = hello;
        self.hello_received = true;
        self.capabilities = capabilities;
        self.appearance = Some(Box::new(appearance));
        self.appearance_provenance = appearance_provenance;
        self.mux_options = mux_options;
        self.refresh_prefix_keys();
        self.key_tables = key_tables;
        self.refresh_key_tables_metadata();
        self.status = status;
    }

    /// Drop the per-session state a reattach republishes — prefix arming,
    /// prompt, command output, choosers, display-panes, agent pane state —
    /// while keeping what the hello established. A shell calls this when the session goes away
    /// under it (detach, server stopping, host loss) so stale overlays do not
    /// outlive it.
    ///
    /// Emits no events: the caller drove the reset and already knows what it
    /// cleared, so events here would double-fire against its own bookkeeping.
    pub fn reset_session(&mut self) {
        self.prefix_armed = false;
        self.key_table = None;
        self.command_prompt = None;
        self.command_output = None;
        self.choose_tree = None;
        self.choose_buffer = None;
        self.chooser_presentation = None;
        self.display_panes = None;
        self.popup = None;
        self.menu = None;
        self.confirm = None;
        self.agent_states.clear();
    }

    /// Forget the current attachment — session, snapshot, retained viewports
    /// and agent pane state — without disturbing hello state. A shell calls this when it
    /// moves to a different daemon, so the old machine's layout cannot render
    /// against the new one. Emits no events, for the same reason as
    /// [`Self::reset_session`].
    pub fn clear_attachment(&mut self) {
        self.attached_session = None;
        self.attached_read_only = false;
        self.attached_client_flags.clear();
        self.last_detach_reason = None;
        self.snapshot = Arc::new(MuxSnapshot::default());
        self.tree = MuxSnapshot::default();
        self.client_view = ClientView::default();
        self.tree_sync_pending = false;
        self.viewports.clear();
        self.spare_cells.clear();
        self.agent_states.clear();
        self.full_pending.clear();
    }

    fn reset_connection(&mut self, hello: ServerHello) {
        self.adopt_hello(hello);
        self.clear_attachment();
        self.reset_session();
        self.events.push_back(CoreEvent::HelloReceived);
    }

    fn handle_payload(&mut self, payload: EventPayload) {
        match payload {
            EventPayload::Snapshot(snapshot) => {
                if self.client_view.attachment_generation == 0 {
                    self.client_view.focused_window = snapshot.focused_window;
                }
                self.tree = snapshot;
                self.tree_sync_pending = false;
                self.full_pending.clear();
                self.materialize_tree();
            }
            EventPayload::TreeDelta(delta) => {
                if self.tree_sync_pending || delta.apply(&mut self.tree).is_err() {
                    self.request_tree_sync();
                } else {
                    self.materialize_tree();
                }
            }
            EventPayload::ClientView(view) => {
                let changed_attachment = view.session != self.attached_session
                    || view.attachment_generation != self.client_view.attachment_generation;
                self.client_view = view;
                self.attached_session = self.client_view.session;
                self.attached_read_only = self.client_view.read_only;
                self.attached_client_flags
                    .clone_from(&self.client_view.client_flags);
                if changed_attachment {
                    let reset_events = self.reset_session_events();
                    self.viewports.clear();
                    self.spare_cells.clear();
                    self.full_pending.clear();
                    if let Some(session) = self.attached_session {
                        self.events.push_back(CoreEvent::Attached { session });
                    }
                    self.events.extend(reset_events);
                }
                self.materialize_tree();
            }
            EventPayload::AppearanceChanged {
                appearance,
                provenance,
            } => {
                self.appearance = Some(appearance);
                self.appearance_provenance = provenance;
                self.events.push_back(CoreEvent::AppearanceChanged);
            }
            EventPayload::MuxOptionsChanged { options } => {
                self.mux_options = options;
                self.refresh_prefix_keys();
                self.events.push_back(CoreEvent::MuxOptionsChanged);
            }
            EventPayload::MuxOptionsPatched { options } => {
                self.mux_options.merge(options);
                self.refresh_prefix_keys();
                self.events.push_back(CoreEvent::MuxOptionsChanged);
            }
            EventPayload::TerminalNegotiation {
                features,
                user_keys,
            } => {
                self.terminal_negotiation = Some((features, user_keys));
                self.events.push_back(CoreEvent::MuxOptionsChanged);
            }
            EventPayload::StatusChanged { status } => {
                self.status = status;
                self.events.push_back(CoreEvent::StatusChanged);
            }
            EventPayload::KeyTablesChanged { tables } => {
                self.key_tables = tables;
                self.refresh_key_tables_metadata();
                self.events.push_back(CoreEvent::KeyTablesChanged);
            }
            EventPayload::KeyTablesPatched { tables, removed } => {
                self.key_tables
                    .retain(|table| !removed.contains(&table.name));
                for table in tables {
                    if let Some(current) = self
                        .key_tables
                        .iter_mut()
                        .find(|current| current.name == table.name)
                    {
                        *current = table;
                    } else {
                        let at = self
                            .key_tables
                            .partition_point(|current| current.name < table.name);
                        self.key_tables.insert(at, table);
                    }
                }
                self.refresh_key_tables_metadata();
                self.events.push_back(CoreEvent::KeyTablesChanged);
            }
            EventPayload::KeyTablesHashChanged { hash, mouse } => {
                self.key_tables_hash = hash;
                self.mouse_bindings = mouse;
                self.events.push_back(CoreEvent::KeyTablesChanged);
            }
            EventPayload::TerminalViewport { pane, viewport } => {
                self.full_pending.remove(&pane);
                self.spare_cells.remove(&pane);
                if let Some(retained) = self.viewports.get_mut(&pane) {
                    replace_retained_viewport(retained, viewport, &mut self.next_row_revision);
                } else {
                    let mut retained = new_retained_viewport(viewport, &mut self.next_row_revision);
                    retained.history = HistoryRing::with_limit(self.history_limit);
                    self.viewports.insert(pane, retained);
                }
                self.events.push_back(CoreEvent::ViewportChanged {
                    pane,
                    damage: ViewportDamage::All,
                });
            }
            EventPayload::TerminalPatch { pane, patch } => self.apply_patch(pane, patch),
            EventPayload::CommandPrompt { state } => {
                self.command_prompt = state;
                self.events.push_back(CoreEvent::CommandPromptChanged);
            }
            EventPayload::CommandOutput {
                pane,
                output_id,
                viewport,
            } => self.apply_command_output(pane, output_id, viewport),
            EventPayload::ChooseTree { state } => {
                self.choose_tree = state;
                self.chooser_presentation = None;
                self.events.push_back(CoreEvent::ChooseTreeChanged);
            }
            EventPayload::ChooseTreeUpdate { search, selected } => {
                self.update_choose_tree(search, selected);
            }
            EventPayload::ChooseBuffer { state } => {
                self.choose_buffer = state;
                self.chooser_presentation = None;
                self.events.push_back(CoreEvent::ChooseBufferChanged);
            }
            EventPayload::ChooseBufferUpdate { search, selected } => {
                self.update_choose_buffer(search, selected);
            }
            EventPayload::ChooserPresentation { presentation } => {
                self.chooser_presentation = presentation.map(|presentation| *presentation);
                if self.choose_tree.is_some() {
                    self.events.push_back(CoreEvent::ChooseTreeChanged);
                } else if self.choose_buffer.is_some() {
                    self.events.push_back(CoreEvent::ChooseBufferChanged);
                }
            }
            EventPayload::DisplayPanes { state } => {
                self.display_panes = state;
                self.events.push_back(CoreEvent::DisplayPanesChanged);
            }
            EventPayload::Popup { state } => {
                if let Some(previous) = self.popup.as_ref()
                    && state.as_ref().is_none_or(|next| next.pane != previous.pane)
                {
                    self.viewports.remove(&previous.pane);
                    self.spare_cells.remove(&previous.pane);
                    self.full_pending.remove(&previous.pane);
                }
                self.popup = state;
                self.events.push_back(CoreEvent::PopupChanged);
            }
            EventPayload::Menu { state } => {
                if self.menu.is_none() && state.is_some() {
                    self.menu_opened = self.menu_opened.wrapping_add(1);
                }
                self.menu = state;
                self.events.push_back(CoreEvent::MenuChanged);
            }
            EventPayload::Confirm { state } => {
                self.confirm = state;
                self.events.push_back(CoreEvent::ConfirmChanged);
            }
            EventPayload::PrefixArmed { armed } => {
                self.prefix_armed = armed;
                self.events.push_back(CoreEvent::PrefixArmed { armed });
            }
            EventPayload::PrefixCancelled { request_id } => {
                self.events
                    .push_back(CoreEvent::PrefixCancelled { request_id });
            }
            EventPayload::KeyTableActive { table, repeat } => {
                self.key_table = table.map(|table| (table, repeat));
                self.events.push_back(CoreEvent::KeyTableChanged);
            }
            EventPayload::PaneRemoved(pane) => {
                self.viewports.remove(&pane);
                self.spare_cells.remove(&pane);
                self.agent_states.remove(&pane);
                self.full_pending.remove(&pane);
                if self
                    .command_output
                    .as_ref()
                    .is_some_and(|(_, output_pane, _)| *output_pane == pane)
                {
                    self.command_output = None;
                    self.events.push_back(CoreEvent::CommandOutputChanged);
                }
                self.events.push_back(CoreEvent::PaneRemoved { pane });
            }
            EventPayload::Detached {
                session,
                by,
                reason,
                action,
            } => {
                if self.attached_session == Some(session) {
                    self.attached_session = None;
                }
                self.last_detach_reason = Some(if reason.is_requested() {
                    CoreDetachReason::Requested
                } else if reason.is_evicted() {
                    CoreDetachReason::Evicted
                } else if reason.is_session_destroyed() {
                    CoreDetachReason::SessionDestroyed
                } else {
                    debug_assert!(reason.is_server_stopping());
                    CoreDetachReason::ServerStopping
                });
                self.events.push_back(CoreEvent::Detached {
                    session,
                    by,
                    action,
                });
            }
            EventPayload::ServerStopping => self.events.push_back(CoreEvent::ServerStopping),
            EventPayload::Bell { pane } => self.events.push_back(CoreEvent::Bell { pane }),
            EventPayload::FocusSidebar => self.events.push_back(CoreEvent::FocusSidebar),
            EventPayload::OpenPathPicker { pane, start_dir } => {
                self.events
                    .push_back(CoreEvent::OpenPathPicker { pane, start_dir });
            }
            EventPayload::ClientMessage { pane, kind, text } => {
                self.events.push_back(CoreEvent::ClientMessage {
                    pane,
                    kind,
                    text,
                    duration_ms: None,
                    message_id: None,
                });
            }
            EventPayload::TimedClientMessage {
                pane,
                kind,
                text,
                duration_ms,
                message_id,
            } => {
                self.events.push_back(CoreEvent::ClientMessage {
                    pane,
                    kind,
                    text,
                    duration_ms: Some(duration_ms),
                    message_id: Some(message_id),
                });
            }
            EventPayload::TimedClientMessageCleared { message_id } => {
                self.events
                    .push_back(CoreEvent::ClientMessageCleared { message_id });
            }
            EventPayload::Clipboard {
                pane,
                request_id,
                target,
                text,
                producer,
            } => {
                self.events.push_back(CoreEvent::Clipboard {
                    pane,
                    request_id,
                    target,
                    text,
                    producer,
                });
            }
            EventPayload::OpenUri { pane, uri } => {
                self.events.push_back(CoreEvent::OpenUri { pane, uri });
            }
            EventPayload::AgentCommand {
                pane,
                request_id,
                command,
            } => {
                self.events.push_back(CoreEvent::AgentCommand {
                    pane,
                    request_id,
                    command,
                });
            }
            EventPayload::BrowserCommand { pane, command } => {
                self.events
                    .push_back(CoreEvent::BrowserCommand { pane, command });
            }
            EventPayload::TerminalUiCommand { pane, command } => {
                self.events
                    .push_back(CoreEvent::TerminalUiCommand { pane, command });
            }
            EventPayload::HistoryChunk {
                pane,
                start,
                total,
                offset,
                columns,
                rows,
                dictionary,
            } => {
                self.events.push_back(CoreEvent::HistoryChunk {
                    pane,
                    start,
                    total,
                    offset,
                    columns,
                    rows,
                    dictionary,
                });
            }
            EventPayload::KittyImageBegin {
                pane,
                image_id,
                generation,
                width,
                height,
                total_bytes,
            } => {
                self.events.push_back(CoreEvent::KittyImageBegin {
                    pane,
                    image_id,
                    generation,
                    width,
                    height,
                    total_bytes,
                });
            }
            EventPayload::KittyImageChunk {
                pane,
                image_id,
                generation,
                bytes,
            } => {
                self.events.push_back(CoreEvent::KittyImageChunk {
                    pane,
                    image_id,
                    generation,
                    bytes,
                });
            }
            EventPayload::KittyImagesRemoved { pane, image_ids } => {
                self.events
                    .push_back(CoreEvent::KittyImagesRemoved { pane, image_ids });
            }
            EventPayload::AgentUpdates {
                pane,
                first_seq,
                items,
            } => {
                self.events.push_back(CoreEvent::AgentUpdates {
                    pane,
                    first_seq,
                    items,
                });
            }
            EventPayload::AgentState { pane, state } => {
                let attention = self
                    .agent_states
                    .get(&pane)
                    .and_then(|previous| agent_attention_edge(previous, &state));
                self.agent_states.insert(pane, state);
                self.events
                    .push_back(CoreEvent::AgentStateChanged { pane, attention });
            }
            EventPayload::AgentLagged { pane, next_seq } => {
                self.events
                    .push_back(CoreEvent::AgentLagged { pane, next_seq });
            }
            EventPayload::AgentSessions {
                pane,
                request_id,
                result,
            } => {
                self.events.push_back(CoreEvent::AgentSessions {
                    pane,
                    request_id,
                    result,
                });
            }
            EventPayload::ControlExit { .. }
            | EventPayload::HookEvent { .. }
            | EventPayload::PaneOutput { .. }
            | EventPayload::PaneOutputState { .. }
            | EventPayload::PaneOutputAged { .. }
            | EventPayload::ControlFlags { .. }
            | EventPayload::ControlCommandGuard { .. }
            | EventPayload::ControlCommandGuardRaw { .. }
            | EventPayload::ControlCommandStarted { .. }
            | EventPayload::ControlCommandOutput { .. }
            | EventPayload::ControlConfigError { .. }
            | EventPayload::ControlSourceFile { .. }
            | EventPayload::StartupConfigCauses { .. }
            | EventPayload::CommandStdout { .. }
            | EventPayload::CommandClientExit
            | EventPayload::SubscriptionChanged { .. } => {}
        }
    }

    fn reset_session_events(&mut self) -> Vec<CoreEvent> {
        let events = [
            (self.prefix_armed, CoreEvent::PrefixArmed { armed: false }),
            (self.key_table.is_some(), CoreEvent::KeyTableChanged),
            (
                self.command_prompt.is_some(),
                CoreEvent::CommandPromptChanged,
            ),
            (
                self.command_output.is_some(),
                CoreEvent::CommandOutputChanged,
            ),
            (self.choose_tree.is_some(), CoreEvent::ChooseTreeChanged),
            (self.choose_buffer.is_some(), CoreEvent::ChooseBufferChanged),
            (self.display_panes.is_some(), CoreEvent::DisplayPanesChanged),
            (self.popup.is_some(), CoreEvent::PopupChanged),
            (self.menu.is_some(), CoreEvent::MenuChanged),
            (self.confirm.is_some(), CoreEvent::ConfirmChanged),
        ]
        .into_iter()
        .filter_map(|(changed, event)| changed.then_some(event))
        .collect();
        self.reset_session();
        events
    }

    fn request_tree_sync(&mut self) {
        if !self.tree_sync_pending {
            self.tree_sync_pending = true;
            self.outbound.push_back(Outbound::TreeSync);
        }
    }

    fn materialize_tree(&mut self) {
        self.tree_dirty = true;
        if self.batch_reducing {
            return;
        }
        let Ok(snapshot) = self.client_view.apply_owned(self.tree.clone()) else {
            self.request_tree_sync();
            return;
        };
        self.tree_dirty = false;
        self.snapshot = Arc::new(snapshot);
        self.retain_snapshot_panes();
        self.events.push_back(CoreEvent::SnapshotChanged);
    }

    fn refresh_key_tables_metadata(&mut self) {
        self.key_tables_hash = key_tables_hash(&self.key_tables);
        self.mouse_bindings = MouseBindings::from_tables(&self.key_tables);
    }

    fn apply_patch(&mut self, pane: PaneId, patch: TerminalViewportPatch) {
        let Some(retained) = self.viewports.get_mut(&pane) else {
            self.request_full(pane);
            return;
        };
        let damage = patch_damage(&retained.viewport, &patch);
        let retired = adopt_spare_cells(&mut retained.viewport, self.spare_cells.remove(&pane))
            .filter(|_| patch.scroll == 0)
            .map(|(cells, mut stale)| {
                stale.extend(patch.changed_rows.row_indices());
                (cells, stale)
            });
        if apply_retained_patch(retained, patch, &mut self.next_row_revision).is_ok() {
            let viewport = &retained.viewport;
            if let Some((cells, stale)) = retired
                && !Arc::ptr_eq(&cells, &viewport.cells)
            {
                self.spare_cells.insert(
                    pane,
                    SpareCells {
                        cells,
                        stale,
                        generation: (viewport.generation, viewport.view_generation),
                    },
                );
            }
            self.events
                .push_back(CoreEvent::ViewportChanged { pane, damage });
        } else {
            self.viewports.remove(&pane);
            self.request_full(pane);
        }
    }

    fn apply_command_output(
        &mut self,
        pane: PaneId,
        output_id: u64,
        viewport: Option<TerminalViewport>,
    ) {
        if output_id == 0 {
            if viewport.is_none() {
                self.command_output = None;
                self.events.push_back(CoreEvent::CommandOutputChanged);
            }
            return;
        }
        if output_id < self.command_output_watermark {
            return;
        }

        match viewport {
            Some(viewport) if output_id > self.command_output_watermark => {
                self.command_output_watermark = output_id;
                self.command_output = Some((
                    output_id,
                    pane,
                    new_retained_viewport(viewport, &mut self.next_row_revision),
                ));
                self.events.push_back(CoreEvent::CommandOutputChanged);
            }
            Some(viewport) if self.command_output_id() == Some(output_id) => {
                self.command_output = Some((
                    output_id,
                    pane,
                    new_retained_viewport(viewport, &mut self.next_row_revision),
                ));
                self.events.push_back(CoreEvent::CommandOutputChanged);
            }
            None if output_id > self.command_output_watermark => {
                self.command_output_watermark = output_id;
                self.command_output = None;
                self.events.push_back(CoreEvent::CommandOutputChanged);
            }
            None if self.command_output_id() == Some(output_id) => {
                self.command_output = None;
                self.events.push_back(CoreEvent::CommandOutputChanged);
            }
            Some(_) | None => {}
        }
    }

    fn request_full(&mut self, pane: PaneId) {
        if self.full_pending.insert(pane) {
            self.outbound.push_back(Outbound::RequestFull(pane));
        }
    }

    /// Drop retained viewports and agent state for panes the snapshot no
    /// longer contains.
    fn retain_snapshot_panes(&mut self) {
        let live: HashSet<PaneId> = self
            .snapshot
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .flat_map(|window| window.panes.keys().copied())
            .collect();
        let popup = self.popup.as_ref().map(|popup| popup.pane);
        self.viewports
            .retain(|pane, _| live.contains(pane) || popup == Some(*pane));
        self.spare_cells
            .retain(|pane, _| live.contains(pane) || popup == Some(*pane));
        self.agent_states.retain(|pane, _| live.contains(pane));
        self.full_pending
            .retain(|pane| live.contains(pane) || popup == Some(*pane));
    }

    fn update_choose_tree(&mut self, search: Option<ChooseTreeSearchState>, selected: u32) {
        if let Some(state) = self.choose_tree.as_mut() {
            state.search = search;
            state.selected = clamp_selected(selected, state.items.len());
            self.events.push_back(CoreEvent::ChooseTreeChanged);
        }
    }

    fn update_choose_buffer(&mut self, search: Option<ChooseBufferSearchState>, selected: u32) {
        if let Some(state) = self.choose_buffer.as_mut() {
            state.search = search;
            state.selected = clamp_selected(selected, state.items.len());
            self.events.push_back(CoreEvent::ChooseBufferChanged);
        }
    }
}

/// A cursor delta indexes the item list the client already holds. The daemon
/// only sends one when that list is unchanged, so an out-of-range index means
/// the two sides disagree; parking on the last row beats pointing past the end.
fn clamp_selected(selected: u32, items: usize) -> u32 {
    selected.min(u32::try_from(items.saturating_sub(1)).unwrap_or(u32::MAX))
}

#[derive(Debug)]
struct SpareCells {
    cells: Arc<[PackedCell]>,
    stale: Vec<u16>,
    generation: (u64, u64),
}

fn adopt_spare_cells(
    viewport: &mut TerminalViewport,
    spare: Option<SpareCells>,
) -> Option<(Arc<[PackedCell]>, Vec<u16>)> {
    if Arc::get_mut(&mut viewport.cells).is_some() {
        return None;
    }
    let retired = Arc::clone(&viewport.cells);
    let Some(SpareCells {
        mut cells,
        mut stale,
        generation,
    }) = spare
    else {
        return Some((retired, Vec::new()));
    };
    if generation == (viewport.generation, viewport.view_generation)
        && cells.len() == retired.len()
        && let Some(buffer) = Arc::get_mut(&mut cells)
    {
        let columns = usize::from(viewport.columns);
        for row in stale.iter().map(|row| usize::from(*row) * columns) {
            if let (Some(target), Some(source)) = (
                buffer.get_mut(row..row + columns),
                retired.get(row..row + columns),
            ) {
                target.copy_from_slice(source);
            }
        }
        viewport.cells = cells;
    }
    stale.clear();
    Some((retired, stale))
}

/// Which rows a patch will touch, computed against the pre-apply viewport.
fn patch_damage(previous: &TerminalViewport, patch: &TerminalViewportPatch) -> ViewportDamage {
    if patch.scroll != 0
        || patch.carries(TerminalPatchFields::COLORS)
            && (patch.foreground != previous.foreground || patch.background != previous.background)
    {
        return ViewportDamage::All;
    }
    let mut rows = patch.changed_rows.row_indices().collect::<Vec<_>>();
    if patch.carries(TerminalPatchFields::OVERLAYS) {
        rows.extend(previous.overlays.iter().map(|overlay| overlay.row));
        rows.extend(patch.overlays.iter().map(|overlay| overlay.row));
        rows.sort_unstable();
        rows.dedup();
    }
    ViewportDamage::Rows(rows)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zz_protocol::{
        AgentConnectionPhase, ClientId, LayoutNode, PROTOCOL_VERSION, PaneKindSnapshot,
        PaneSnapshot, SessionSnapshot, WindowId, WindowSnapshot,
    };
    use zz_terminal::TerminalAppearance;

    use super::*;

    fn event(payload: EventPayload) -> ProtocolMessage {
        ProtocolMessage::Event(Event {
            sequence: 0,
            payload,
        })
    }

    fn hello() -> ProtocolMessage {
        ProtocolMessage::ServerHello(Box::new(ServerHello {
            protocol_version: PROTOCOL_VERSION,
            server_id: 1,
            client_id: ClientId(1),
            client_instance_id: zz_protocol::ClientInstanceId(1),
            capabilities: Vec::new(),
            appearance: TerminalAppearance::default(),
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: MuxOptions::default(),
            status: StatusLine::default(),
            key_tables: Vec::new(),
        }))
    }

    fn command_output_frame(pane: PaneId, output_id: u64, generation: u64) -> ProtocolMessage {
        let mut viewport = TerminalViewport::blank(80, 24, zz_terminal::SessionStatus::Running);
        viewport.generation = generation;
        event(EventPayload::CommandOutput {
            pane,
            output_id,
            viewport: Some(viewport),
        })
    }

    fn command_output_close(pane: PaneId, output_id: u64) -> ProtocolMessage {
        event(EventPayload::CommandOutput {
            pane,
            output_id,
            viewport: None,
        })
    }

    fn agent_state(title: &str) -> AgentPaneWire {
        AgentPaneWire {
            phase: AgentConnectionPhase::Running,
            queued_prompts: 2,
            title: Some(title.to_owned()),
            ..AgentPaneWire::default()
        }
    }

    fn snapshot_with(panes: &[PaneId]) -> MuxSnapshot {
        let entries: BTreeMap<PaneId, PaneSnapshot> = panes
            .iter()
            .map(|pane| {
                (
                    *pane,
                    PaneSnapshot {
                        id: *pane,
                        title: String::new(),
                        kind: PaneKindSnapshot::Terminal,
                        synchronized_input: false,
                        bell: false,
                        dead: false,
                        dead_status: None,
                        border_colour: None,
                        active_border_colour: None,
                        border_status_text: String::new(),
                        mode: None,
                        status: None,
                    },
                )
            })
            .collect();
        let first = panes.first().copied().unwrap_or_default();
        MuxSnapshot {
            generation: 1,
            sessions: vec![SessionSnapshot {
                id: SessionId(0),
                name: "0".to_owned(),
                active_window: WindowId(0),
                windows: vec![WindowSnapshot {
                    id: WindowId(0),
                    index: 0,
                    name: "win".to_owned(),
                    automatic_rename: true,
                    active_pane: first,
                    zoomed_pane: None,
                    layout: LayoutNode::Pane(first),
                    panes: entries,
                    layout_dump: String::new(),
                    visible_layout_dump: String::new(),
                    status_label: String::new(),
                    activity: false,
                    silence: false,
                    pane_border_status: zz_protocol::PaneBorderStatus::Off,
                    pane_border_lines: zz_protocol::PaneBorderLines::Single,
                    pane_border_indicators: zz_protocol::PaneBorderIndicators::Colour,
                    pane_order: Vec::new(),
                    pane_z_order: Vec::new(),
                }],
                viewers: Vec::new(),
            }],
            focused_window: None,
        }
    }

    fn drain(core: &mut ClientCore) -> Vec<CoreEvent> {
        let mut events = Vec::new();
        while let Some(event) = core.poll_event() {
            events.push(event);
        }
        events
    }

    /// Identity has to reach every surface: the timed message carries the id
    /// the daemon can retire it by, the untimed one carries none, and the clear
    /// arrives as its own event rather than folded into a message.
    #[test]
    fn timed_messages_carry_their_identity_and_the_clear_arrives_on_its_own() {
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::TimedClientMessage {
            pane: None,
            kind: ClientMessageKind::Info,
            text: "timed".to_owned(),
            duration_ms: 750,
            message_id: 12,
        }));
        core.handle_message(event(EventPayload::ClientMessage {
            pane: None,
            kind: ClientMessageKind::Warning,
            text: "untimed".to_owned(),
        }));
        core.handle_message(event(EventPayload::TimedClientMessageCleared {
            message_id: 12,
        }));
        assert_eq!(
            drain(&mut core),
            vec![
                CoreEvent::ClientMessage {
                    pane: None,
                    kind: ClientMessageKind::Info,
                    text: "timed".to_owned(),
                    duration_ms: Some(750),
                    message_id: Some(12),
                },
                CoreEvent::ClientMessage {
                    pane: None,
                    kind: ClientMessageKind::Warning,
                    text: "untimed".to_owned(),
                    duration_ms: None,
                    message_id: None,
                },
                CoreEvent::ClientMessageCleared { message_id: 12 },
            ]
        );
    }

    #[test]
    fn prefix_cancel_ack_passes_through_with_its_request_id() {
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::PrefixCancelled { request_id: 73 }));
        assert_eq!(
            drain(&mut core),
            vec![CoreEvent::PrefixCancelled { request_id: 73 }]
        );
    }

    #[test]
    fn detached_reason_is_retained_without_changing_the_shell_event_shape() {
        let session = SessionId(7);
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::detached_session_destroyed(session)));
        assert_eq!(
            drain(&mut core),
            vec![CoreEvent::Detached {
                session,
                by: None,
                action: ClientExitAction::Exit
            }]
        );
        assert!(core.last_detach_was_session_destroyed());
        assert!(!core.last_detach_was_server_stopping());
    }

    #[test]
    fn attachment_options_track_the_daemon_and_survive_detach_for_reconnect() {
        let session = SessionId(7);
        let mut core = ClientCore::new();
        core.handle_message(ProtocolMessage::Attached {
            session,
            snapshot: snapshot_with(&[]),
            read_only: false,
            client_flags: "active-pane".to_owned(),
        });
        assert!(!core.attached_read_only());
        assert_eq!(core.attached_client_flags(), "active-pane");

        core.handle_message(ProtocolMessage::Attached {
            session,
            snapshot: snapshot_with(&[]),
            read_only: true,
            client_flags: "ignore-size,active-pane".to_owned(),
        });
        assert!(core.attached_read_only());
        assert_eq!(core.attached_client_flags(), "ignore-size,active-pane");

        core.handle_message(event(EventPayload::detached_requested(session, None)));
        assert!(core.attached_read_only());
        assert_eq!(core.attached_client_flags(), "ignore-size,active-pane");

        core.clear_attachment();
        assert!(!core.attached_read_only());
        assert_eq!(core.attached_client_flags(), "");
    }

    #[test]
    fn agent_state_is_stored_and_notified() {
        let pane = PaneId(7);
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::AgentState {
            pane,
            state: agent_state("first"),
        }));
        assert_eq!(
            drain(&mut core),
            vec![CoreEvent::AgentStateChanged {
                pane,
                attention: None,
            }]
        );
        assert_eq!(core.agent_state(pane), Some(&agent_state("first")));

        core.handle_message(event(EventPayload::AgentState {
            pane,
            state: agent_state("second"),
        }));
        assert_eq!(
            drain(&mut core),
            vec![CoreEvent::AgentStateChanged {
                pane,
                attention: None,
            }]
        );
        assert_eq!(
            core.agent_state(pane).and_then(|state| state.title.clone()),
            Some("second".to_owned())
        );
        assert_eq!(core.agent_state(PaneId(8)), None);
    }

    #[test]
    fn agent_attention_edges_are_lossless_core_events() {
        let pane = PaneId(7);
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::AgentState {
            pane,
            state: agent_state("work"),
        }));
        drain(&mut core);

        let ready = AgentPaneWire {
            phase: AgentConnectionPhase::Ready,
            ..agent_state("done")
        };
        core.handle_message(event(EventPayload::AgentState { pane, state: ready }));
        assert_eq!(
            drain(&mut core),
            vec![CoreEvent::AgentStateChanged {
                pane,
                attention: Some(AgentAttentionEdge::Done),
            }]
        );

        let permission = AgentPaneWire {
            phase: AgentConnectionPhase::AwaitingPermission,
            pending_permission: Some(zz_protocol::AgentPermissionWire {
                request_id: 9,
                payload: "{}".to_owned(),
            }),
            ..agent_state("permission")
        };
        core.handle_message(event(EventPayload::AgentState {
            pane,
            state: permission,
        }));
        assert_eq!(
            drain(&mut core),
            vec![CoreEvent::AgentStateChanged {
                pane,
                attention: Some(AgentAttentionEdge::Request),
            }]
        );

        let failed = AgentPaneWire {
            phase: AgentConnectionPhase::Failed {
                message: "boom".to_owned(),
            },
            ..agent_state("failed")
        };
        core.handle_message(event(EventPayload::AgentState {
            pane,
            state: failed,
        }));
        assert_eq!(
            drain(&mut core),
            vec![CoreEvent::AgentStateChanged {
                pane,
                attention: Some(AgentAttentionEdge::Failed),
            }]
        );
    }

    #[test]
    fn agent_stream_payloads_pass_through_without_retention() {
        let pane = PaneId(3);
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::AgentUpdates {
            pane,
            first_seq: 41,
            items: vec![b"{\"kind\":\"chunk\"}".to_vec(), b"{}".to_vec()],
        }));
        core.handle_message(event(EventPayload::AgentLagged { pane, next_seq: 43 }));
        core.handle_message(event(EventPayload::AgentSessions {
            pane,
            request_id: 9,
            result: "[]".to_owned(),
        }));

        assert_eq!(
            drain(&mut core),
            vec![
                CoreEvent::AgentUpdates {
                    pane,
                    first_seq: 41,
                    items: vec![b"{\"kind\":\"chunk\"}".to_vec(), b"{}".to_vec()],
                },
                CoreEvent::AgentLagged { pane, next_seq: 43 },
                CoreEvent::AgentSessions {
                    pane,
                    request_id: 9,
                    result: "[]".to_owned(),
                },
            ]
        );
        assert_eq!(core.agent_state(pane), None);
        assert!(core.poll_outbound().is_none());
    }

    #[test]
    fn agent_state_drops_with_its_pane() {
        let kept = PaneId(1);
        let lost = PaneId(2);
        let mut core = ClientCore::new();
        for pane in [kept, lost] {
            core.handle_message(event(EventPayload::AgentState {
                pane,
                state: agent_state("live"),
            }));
        }

        core.handle_message(event(EventPayload::Snapshot(snapshot_with(&[kept]))));
        assert!(core.agent_state(kept).is_some());
        assert_eq!(core.agent_state(lost), None);

        core.handle_message(event(EventPayload::PaneRemoved(kept)));
        assert_eq!(core.agent_state(kept), None);
    }

    #[test]
    fn reconnect_and_session_reset_clear_agent_state() {
        let pane = PaneId(5);
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::AgentState {
            pane,
            state: agent_state("live"),
        }));
        core.reset_session();
        assert_eq!(core.agent_state(pane), None);

        core.handle_message(event(EventPayload::AgentState {
            pane,
            state: agent_state("live"),
        }));
        core.handle_message(hello());
        assert_eq!(core.agent_state(pane), None);

        core.handle_message(event(EventPayload::AgentState {
            pane,
            state: agent_state("live"),
        }));
        core.handle_message(ProtocolMessage::Attached {
            session: SessionId(0),
            snapshot: snapshot_with(&[pane]),
            read_only: false,
            client_flags: String::new(),
        });
        assert_eq!(core.agent_state(pane), None);
    }

    #[test]
    fn command_output_actor_updates_replace_and_ignore_stale_traffic() {
        let pane = PaneId(5);
        let mut core = ClientCore::new();

        core.handle_message(command_output_frame(pane, 10, 1));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), Some(10));
        assert_eq!(
            core.command_output()
                .map(|(output_pane, viewport)| (output_pane, viewport.generation)),
            Some((pane, 1))
        );

        core.handle_message(command_output_frame(pane, 10, 2));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(
            core.command_output()
                .map(|(output_pane, viewport)| (output_pane, viewport.generation)),
            Some((pane, 2))
        );

        core.handle_message(command_output_frame(PaneId(4), 9, 3));
        core.handle_message(command_output_close(pane, 9));
        assert!(drain(&mut core).is_empty());
        assert_eq!(core.command_output_id(), Some(10));

        core.handle_message(command_output_frame(pane, 11, 2));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), Some(11));

        core.handle_message(command_output_close(pane, 10));
        assert!(drain(&mut core).is_empty());
        assert_eq!(core.command_output_id(), Some(11));

        core.handle_message(command_output_close(pane, 11));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), None);
    }

    #[test]
    fn command_output_newer_close_and_zero_resync_prevent_resurrection() {
        let pane = PaneId(5);
        let mut core = ClientCore::new();

        core.handle_message(command_output_frame(pane, 5, 1));
        drain(&mut core);
        core.handle_message(command_output_close(pane, 7));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), None);

        core.handle_message(command_output_frame(pane, 6, 2));
        core.handle_message(command_output_close(pane, 6));
        core.handle_message(command_output_frame(pane, 7, 3));
        assert!(drain(&mut core).is_empty());
        assert_eq!(core.command_output_id(), None);

        core.handle_message(command_output_frame(pane, 8, 4));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), Some(8));

        core.handle_message(command_output_close(pane, 0));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), None);

        core.handle_message(command_output_frame(pane, 8, 5));
        assert!(drain(&mut core).is_empty());
        assert_eq!(core.command_output_id(), None);

        core.handle_message(command_output_frame(pane, 9, 6));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), Some(9));
    }

    #[test]
    fn command_output_watermark_resets_with_the_connection() {
        let pane = PaneId(5);
        let mut core = ClientCore::new();

        core.handle_message(command_output_frame(pane, 20, 1));
        drain(&mut core);
        core.handle_message(hello());
        assert_eq!(drain(&mut core), vec![CoreEvent::HelloReceived]);
        assert_eq!(core.command_output_id(), None);

        core.handle_message(command_output_frame(pane, 1, 2));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), Some(1));
    }

    #[test]
    fn command_output_watermark_resets_when_adopting_a_handshake() {
        let pane = PaneId(5);
        let mut core = ClientCore::new();

        core.handle_message(command_output_frame(pane, 20, 1));
        drain(&mut core);
        let ProtocolMessage::ServerHello(hello) = hello() else {
            unreachable!();
        };
        core.adopt_hello(*hello);
        assert_eq!(core.command_output_id(), Some(20));

        core.handle_message(command_output_frame(pane, 1, 2));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), Some(1));
    }

    #[test]
    fn a_new_chooser_state_drops_the_presentation_the_last_one_left() {
        let tree_state = || ChooseTreeState {
            items: Vec::new(),
            search: None,
            selected: 0,
            kind: zz_protocol::ChooseTreeKind::Windows,
            filter_no_matches: false,
            prompt: String::new(),
            help: false,
        };
        let presentation = |rows: &str| ChooserPresentation {
            selected: 0,
            rows: vec![zz_protocol::ChooserRow {
                name: String::new(),
                text: rows.to_owned(),
                align: false,
            }],
            sort: "index".to_owned(),
            view: "preview".to_owned(),
            filter: false,
            selection_style: String::new(),
            border_style: String::new(),
            prompt_style: String::new(),
            preview_size: zz_protocol::ChooserPreviewSize::Normal,
            preview: None,
        };

        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::ChooseTree {
            state: Some(tree_state()),
        }));
        core.handle_message(event(EventPayload::ChooserPresentation {
            presentation: Some(Box::new(presentation("ZZTREE<%1>"))),
        }));
        assert!(core.chooser_presentation().is_some());

        core.handle_message(event(EventPayload::ChooseTree { state: None }));
        assert!(core.chooser_presentation().is_none());

        core.handle_message(event(EventPayload::ChooseTree {
            state: Some(tree_state()),
        }));
        assert!(core.chooser_presentation().is_none());

        core.handle_message(event(EventPayload::ChooserPresentation {
            presentation: Some(Box::new(presentation("bash"))),
        }));
        assert_eq!(
            core.chooser_presentation()
                .map(|presentation| presentation.rows[0].text.as_str()),
            Some("bash")
        );
    }

    #[test]
    fn chooser_deltas_preserve_static_filter_fallback_state() {
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::ChooseTree {
            state: Some(ChooseTreeState {
                items: Vec::new(),
                search: None,
                selected: 0,
                kind: zz_protocol::ChooseTreeKind::Windows,
                filter_no_matches: true,
                prompt: String::new(),
                help: false,
            }),
        }));
        core.handle_message(event(EventPayload::ChooseTreeUpdate {
            search: Some(ChooseTreeSearchState {
                query: "tree".to_owned(),
                reverse: false,
            }),
            selected: 4,
        }));
        let tree = core.choose_tree().expect("retained tree chooser");
        assert!(tree.filter_no_matches);
        assert_eq!(
            tree.search.as_ref().map(|search| search.query.as_str()),
            Some("tree")
        );

        core.handle_message(event(EventPayload::ChooseBuffer {
            state: Some(ChooseBufferState {
                items: Vec::new(),
                search: None,
                selected: 0,
                filter_no_matches: true,
                help: false,
                prompt: String::new(),
            }),
        }));
        core.handle_message(event(EventPayload::ChooseBufferUpdate {
            search: Some(ChooseBufferSearchState {
                query: "buffer".to_owned(),
                reverse: true,
            }),
            selected: 5,
        }));
        let buffer = core.choose_buffer().expect("retained buffer chooser");
        assert!(buffer.filter_no_matches);
        assert_eq!(
            buffer.search.as_ref().map(|search| search.query.as_str()),
            Some("buffer")
        );
    }

    #[test]
    fn popup_descriptor_owns_its_synthetic_viewport_lifetime() {
        let pane = PaneId(u64::MAX - 1);
        let state = PopupState {
            pane,
            left: 4,
            top: 3,
            width: 40,
            height: 12,
            client_columns: 80,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 18,
            title: "popup".to_owned(),
            style: "bg=default,fg=default".to_owned(),
            border_style: "fg=default".to_owned(),
            border_lines: zz_protocol::PopupBorderLines::Single,
            close_on_exit: false,
            close_on_exit_zero: false,
            close_on_any_key: false,
            dead: false,
        };
        let mut core = ClientCore::new();

        core.handle_message(event(EventPayload::Popup {
            state: Some(state.clone()),
        }));
        assert_eq!(drain(&mut core), vec![CoreEvent::PopupChanged]);
        assert_eq!(core.popup(), Some(&state));

        core.handle_message(event(EventPayload::TerminalViewport {
            pane,
            viewport: TerminalViewport::blank(38, 10, zz_terminal::SessionStatus::Running),
        }));
        assert!(core.viewport(pane).is_some());
        drain(&mut core);

        core.handle_message(event(EventPayload::Popup { state: None }));
        assert_eq!(drain(&mut core), vec![CoreEvent::PopupChanged]);
        assert_eq!(core.popup(), None);
        assert_eq!(core.viewport(pane), None);
    }

    #[test]
    fn menu_and_confirm_descriptors_reduce_and_clear_with_attachment_state() {
        let menu = MenuState {
            left: 2,
            top: 3,
            width: 20,
            height: 3,
            client_columns: 80,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 18,
            title: "menu".to_owned(),
            style: "default".to_owned(),
            selected_style: "default".to_owned(),
            border_style: "default".to_owned(),
            border_lines: zz_protocol::PopupBorderLines::Single,
            items: vec![Some(zz_protocol::MenuItem {
                name: "Item".to_owned(),
                key: Some("i".to_owned()),
                annotation: Some("i".to_owned()),
                enabled: true,
            })],
            selected: Some(0),
            stay_open: false,
            mouse_keys: false,
        };
        let confirm = ConfirmState {
            prompt: "Confirm? ".to_owned(),
            confirm_key: b'y',
            default_yes: false,
        };
        let mut core = ClientCore::new();

        core.handle_message(event(EventPayload::Menu {
            state: Some(menu.clone()),
        }));
        core.handle_message(event(EventPayload::Confirm {
            state: Some(confirm.clone()),
        }));
        assert_eq!(
            drain(&mut core),
            vec![CoreEvent::MenuChanged, CoreEvent::ConfirmChanged]
        );
        assert_eq!(core.menu(), Some(&menu));
        assert_eq!(core.confirm(), Some(&confirm));

        core.handle_message(ProtocolMessage::Attached {
            session: SessionId(0),
            snapshot: snapshot_with(&[]),
            read_only: false,
            client_flags: String::new(),
        });
        assert_eq!(core.menu(), None);
        assert_eq!(core.confirm(), None);
        assert_eq!(
            drain(&mut core),
            vec![
                CoreEvent::Attached {
                    session: SessionId(0)
                },
                CoreEvent::SnapshotChanged,
                CoreEvent::MenuChanged,
                CoreEvent::ConfirmChanged,
            ]
        );
    }

    #[test]
    fn attachment_clears_session_state_and_notifies_changes() {
        let pane = PaneId(7);
        let mut core = ClientCore::new();
        core.prefix_armed = true;
        core.key_table = Some(("prefix".to_owned(), false));
        core.command_prompt = Some(CommandPromptState {
            prompt: ":".to_owned(),
            input: "echo".to_owned(),
            cursor: 4,
            kind: zz_protocol::CommandPromptKind::Command,
            history: Vec::new(),
            prompt_type: zz_protocol::CommandPromptType::Command,
            mode: zz_protocol::CommandPromptMode::Text,
            no_freeze: false,
            pane: None,
        });
        core.command_output = Some((
            3,
            pane,
            new_retained_viewport(
                TerminalViewport::blank(8, 4, zz_terminal::SessionStatus::Running),
                &mut 1,
            ),
        ));
        core.choose_tree = Some(ChooseTreeState {
            items: Vec::new(),
            search: None,
            selected: 0,
            kind: zz_protocol::ChooseTreeKind::Windows,
            filter_no_matches: false,
            prompt: String::new(),
            help: false,
        });
        core.choose_buffer = Some(ChooseBufferState {
            items: Vec::new(),
            search: None,
            selected: 0,
            filter_no_matches: false,
            help: false,
            prompt: String::new(),
        });
        core.display_panes = Some(DisplayPanesState {
            window: zz_protocol::WindowId(2),
            duration_ms: 1000,
            indicators: Vec::new(),
            colour: None,
            active_colour: None,
        });
        core.popup = Some(PopupState {
            pane,
            left: 1,
            top: 1,
            width: 8,
            height: 4,
            client_columns: 80,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 18,
            title: String::new(),
            style: String::new(),
            border_style: String::new(),
            border_lines: zz_protocol::PopupBorderLines::Single,
            close_on_exit: false,
            close_on_exit_zero: false,
            close_on_any_key: false,
            dead: false,
        });
        core.menu = Some(MenuState {
            left: 1,
            top: 1,
            width: 8,
            height: 4,
            client_columns: 80,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 18,
            title: String::new(),
            style: String::new(),
            selected_style: String::new(),
            border_style: String::new(),
            border_lines: zz_protocol::PopupBorderLines::Single,
            items: Vec::new(),
            selected: None,
            stay_open: false,
            mouse_keys: false,
        });
        core.confirm = Some(ConfirmState {
            prompt: "continue?".to_owned(),
            confirm_key: b'y',
            default_yes: false,
        });

        core.handle_message(ProtocolMessage::Attached {
            session: SessionId(9),
            snapshot: snapshot_with(&[]),
            read_only: false,
            client_flags: String::new(),
        });

        assert!(!core.prefix_armed());
        assert_eq!(core.key_table(), None);
        assert!(core.command_prompt().is_none());
        assert!(core.command_output().is_none());
        assert!(core.choose_tree().is_none());
        assert!(core.choose_buffer().is_none());
        assert!(core.display_panes().is_none());
        assert!(core.popup().is_none());
        assert!(core.menu().is_none());
        assert!(core.confirm().is_none());
        assert_eq!(
            drain(&mut core),
            vec![
                CoreEvent::Attached {
                    session: SessionId(9)
                },
                CoreEvent::SnapshotChanged,
                CoreEvent::PrefixArmed { armed: false },
                CoreEvent::KeyTableChanged,
                CoreEvent::CommandPromptChanged,
                CoreEvent::CommandOutputChanged,
                CoreEvent::ChooseTreeChanged,
                CoreEvent::ChooseBufferChanged,
                CoreEvent::DisplayPanesChanged,
                CoreEvent::PopupChanged,
                CoreEvent::MenuChanged,
                CoreEvent::ConfirmChanged,
            ]
        );
    }

    #[test]
    fn patched_key_tables_replace_insert_and_remove_by_name() {
        let table = |name: &str, keys: &[&str]| KeyTableSnapshot {
            name: name.to_owned(),
            bindings: keys
                .iter()
                .map(|key| KeyBindingSnapshot {
                    key: (*key).to_owned(),
                    commands: Vec::new(),
                    repeat: false,
                    note: None,
                })
                .collect(),
        };
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::KeyTablesChanged {
            tables: vec![
                table("copy-mode", &["q"]),
                table("prefix", &["c"]),
                table("root", &["F1"]),
            ],
        }));
        drain(&mut core);
        core.handle_message(event(EventPayload::KeyTablesPatched {
            tables: vec![table("resize", &["h"]), table("root", &["F2"])],
            removed: vec!["copy-mode".to_owned()],
        }));
        assert_eq!(drain(&mut core), vec![CoreEvent::KeyTablesChanged]);
        assert_eq!(
            core.key_tables(),
            [
                table("prefix", &["c"]),
                table("resize", &["h"]),
                table("root", &["F2"]),
            ]
        );
    }

    #[test]
    fn key_table_active_is_stored_and_every_publication_emits() {
        let mut core = ClientCore::new();
        for (table, repeat) in [
            (Some("prefix"), false),
            (Some("prefix"), false),
            (Some("resize"), true),
            (None, false),
        ] {
            core.handle_message(ProtocolMessage::Event(Event {
                sequence: 0,
                payload: EventPayload::KeyTableActive {
                    table: table.map(str::to_owned),
                    repeat,
                },
            }));
            assert_eq!(core.key_table(), table.map(|table| (table, repeat)));
            assert_eq!(drain(&mut core), vec![CoreEvent::KeyTableChanged]);
        }
    }

    #[test]
    fn an_active_key_table_claims_every_key_but_platform_chords() {
        let mut core = ClientCore::new();
        let key = |character: char, platform: bool| zz_terminal::KeyInput {
            action: zz_terminal::KeyAction::Press,
            key: zz_terminal::KeyCode::Character(character),
            modifiers: zz_terminal::Modifiers::new(false, false, false, platform),
            text: Some(character.to_string().into_boxed_str()),
            unshifted_codepoint: None,
        };
        let publish = |core: &mut ClientCore, table: Option<&str>, repeat: bool| {
            core.handle_message(ProtocolMessage::Event(Event {
                sequence: 0,
                payload: EventPayload::KeyTableActive {
                    table: table.map(str::to_owned),
                    repeat,
                },
            }));
        };
        assert!(!core.claims_prefix_input(&key('x', false)));
        publish(&mut core, Some("resize"), false);
        assert!(core.claims_prefix_input(&key('x', false)));
        assert!(!core.claims_prefix_input(&key('x', true)));
        publish(&mut core, Some("resize"), true);
        assert!(core.claims_prefix_input(&key('h', false)));
        publish(&mut core, None, false);
        assert!(!core.claims_prefix_input(&key('x', false)));
        publish(&mut core, Some("resize"), false);
        core.reset_session();
        assert!(!core.claims_prefix_input(&key('x', false)));
    }

    #[test]
    fn prefix_claims_follow_both_live_options_and_armed_state() {
        let mut core = ClientCore::new();
        let set = |core: &mut ClientCore, key, value: &str| {
            core.mux_options
                .set(key, value, zz_protocol::MuxOptionSource::RuntimeCommand);
            core.refresh_prefix_keys();
        };
        set(&mut core, zz_protocol::MuxOptionKey::Prefix, "Ctrl-a");
        set(&mut core, zz_protocol::MuxOptionKey::Prefix2, "Alt-Space");
        let input = |character: char, control: bool, alt: bool| zz_terminal::KeyInput {
            action: zz_terminal::KeyAction::Press,
            key: zz_terminal::KeyCode::Character(character),
            modifiers: zz_terminal::Modifiers::new(false, control, alt, false),
            text: Some(character.to_string().into_boxed_str()),
            unshifted_codepoint: Some(character),
        };
        assert!(core.claims_prefix_input(&input('a', true, false)));
        assert!(core.claims_prefix_input(&input(' ', false, true)));
        assert!(!core.claims_prefix_input(&input('b', true, false)));
        assert!(!core.claims_prefix_input(&input('x', false, false)));
        core.prefix_armed = true;
        assert!(core.claims_prefix_input(&input('x', false, false)));
        core.prefix_armed = false;
        set(&mut core, zz_protocol::MuxOptionKey::Prefix2, "none");
        assert!(!core.claims_prefix_input(&input(' ', false, true)));
        set(&mut core, zz_protocol::MuxOptionKey::Prefix, "None");
        assert!(!core.claims_prefix_input(&input('a', true, false)));
    }

    fn compact_batch(messages: Vec<ProtocolMessage>) -> ProtocolMessage {
        ProtocolMessage::Batch(zz_protocol::Batch::from_messages(1, messages).unwrap())
    }

    fn compact_view(epoch: u64, generation: u64) -> ClientView {
        ClientView {
            session: Some(SessionId(0)),
            focused_window: Some(WindowId(0)),
            layout_generation: generation,
            attachment_generation: epoch,
            ..ClientView::default()
        }
    }

    #[test]
    fn compact_batch_reattach_resets_once_and_ordinary_view_keeps_frames() {
        let pane = PaneId(4);
        let frame = || {
            event(EventPayload::TerminalViewport {
                pane,
                viewport: TerminalViewport::blank(80, 24, zz_terminal::SessionStatus::Running),
            })
        };
        let mut core = ClientCore::new();
        core.handle_message(compact_batch(vec![
            event(EventPayload::Snapshot(snapshot_with(&[pane]))),
            event(EventPayload::ClientView(compact_view(1, 10))),
            frame(),
        ]));
        assert_eq!(core.layout_generation(), 10);
        assert!(core.viewport(pane).is_some());
        let events = drain(&mut core);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, CoreEvent::SnapshotChanged))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, CoreEvent::Attached { .. }))
                .count(),
            1
        );
        core.handle_message(event(EventPayload::ClientView(compact_view(1, 11))));
        assert!(core.viewport(pane).is_some());
        assert!(
            !drain(&mut core)
                .iter()
                .any(|event| matches!(event, CoreEvent::Attached { .. }))
        );
        core.handle_message(event(EventPayload::PrefixArmed { armed: true }));
        drain(&mut core);
        core.handle_message(event(EventPayload::ClientView(compact_view(2, 12))));
        assert!(core.viewport(pane).is_none());
        let events = drain(&mut core);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, CoreEvent::Attached { .. }))
        );
        assert!(events.contains(&CoreEvent::PrefixArmed { armed: false }));
    }

    #[test]
    fn tree_base_mismatch_requests_one_sync_and_full_snapshot_recovers() {
        let original = snapshot_with(&[PaneId(4)]);
        let mut next = original.clone();
        next.generation += 1;
        next.sessions[0].name = "after".to_owned();
        let mut core = ClientCore::new();
        core.handle_message(event(EventPayload::Snapshot(original.clone())));
        let mut delta = zz_protocol::TreeDelta::between(&original, &next);
        delta.base += 20;
        core.handle_message(event(EventPayload::TreeDelta(delta.clone())));
        core.handle_message(event(EventPayload::TreeDelta(delta)));
        assert_eq!(core.snapshot().as_ref(), &original);
        assert_eq!(core.poll_outbound(), Some(Outbound::TreeSync));
        assert_eq!(core.poll_outbound(), None);
        core.handle_message(event(EventPayload::Snapshot(next.clone())));
        let mut latest = next.clone();
        latest.generation += 1;
        latest.sessions[0].name = "latest".to_owned();
        core.handle_message(event(EventPayload::TreeDelta(
            zz_protocol::TreeDelta::between(&next, &latest),
        )));
        assert_eq!(core.snapshot().as_ref(), &latest);
    }

    #[test]
    fn shared_tree_deltas_reapply_personalized_overlay_without_drift() {
        let mut raw = snapshot_with(&[PaneId(4)]);
        let mut stamped = raw.clone();
        stamped.sessions[0].windows[0].status_label = "own label".to_owned();
        stamped.sessions[0].windows[0]
            .panes
            .get_mut(&PaneId(4))
            .unwrap()
            .border_status_text = "own border".to_owned();
        let mut view = compact_view(1, 1);
        view.overlay = zz_protocol::TreeDelta::between(&raw, &stamped).ops;
        let mut core = ClientCore::new();
        core.handle_message(compact_batch(vec![
            event(EventPayload::Snapshot(raw.clone())),
            event(EventPayload::ClientView(view.clone())),
        ]));
        for version in 2..42 {
            let before = raw.clone();
            raw.generation = version;
            raw.sessions[0].windows[0].name = format!("window-{version}");
            core.handle_message(compact_batch(vec![
                event(EventPayload::TreeDelta(zz_protocol::TreeDelta::between(
                    &before, &raw,
                ))),
                event(EventPayload::ClientView(view.clone())),
            ]));
            let mut expected = raw.clone();
            view.apply(&mut expected).unwrap();
            assert_eq!(core.snapshot().as_ref(), &expected);
            assert_eq!(core.poll_outbound(), None);
        }
    }

    #[test]
    fn batch_removal_replaces_obsolete_overlay_before_materializing() {
        let raw = snapshot_with(&[PaneId(4)]);
        let mut stamped = raw.clone();
        stamped.sessions[0].windows[0].status_label = "own label".to_owned();
        let mut view = compact_view(1, 1);
        view.overlay = zz_protocol::TreeDelta::between(&raw, &stamped).ops;
        let empty = MuxSnapshot {
            generation: 2,
            ..MuxSnapshot::default()
        };
        for payload in [
            EventPayload::TreeDelta(zz_protocol::TreeDelta::between(&raw, &empty)),
            EventPayload::Snapshot(empty),
        ] {
            let mut core = ClientCore::new();
            core.handle_message(compact_batch(vec![
                event(EventPayload::Snapshot(raw.clone())),
                event(EventPayload::ClientView(view.clone())),
            ]));
            core.handle_message(compact_batch(vec![
                event(payload),
                event(EventPayload::ClientView(ClientView::default())),
            ]));
            assert!(core.snapshot().sessions.is_empty());
            assert_eq!(core.poll_outbound(), None);
        }
    }

    #[test]
    fn malformed_batch_does_not_partially_reduce() {
        let mut core = ClientCore::new();
        let valid =
            zz_protocol::encode_protocol_message(&event(EventPayload::Snapshot(snapshot_with(&[
                PaneId(4),
            ]))))
            .unwrap();
        core.handle_message(ProtocolMessage::Batch(zz_protocol::Batch {
            sequence: 1,
            frames: vec![valid, vec![0, 1]],
        }));
        assert!(core.snapshot().sessions.is_empty());
        assert_eq!(core.poll_outbound(), Some(Outbound::TreeSync));
    }

    #[test]
    fn reused_cell_buffers_follow_patches_while_clones_are_held() {
        let pane = PaneId(4);
        let mut adoptable = 0;
        for seed in 1..=4_u64 {
            let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let mut next = move |bound: usize| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state % bound as u64) as usize
            };
            let mut core = ClientCore::new();
            let mut reference = TerminalViewport::blank(12, 6, zz_terminal::SessionStatus::Running);
            core.handle_message(event(EventPayload::TerminalViewport {
                pane,
                viewport: reference.clone(),
            }));
            let mut held: Vec<(TerminalViewport, Vec<PackedCell>)> = Vec::new();
            for step in 0..600 {
                let mut current = reference.clone();
                current.generation += 1;
                let cells = Arc::make_mut(&mut current.cells);
                let len = cells.len();
                match next(6) {
                    0 => {
                        let shift = (1 + next(2)) * 12;
                        cells.copy_within(shift.., 0);
                        cells[len - shift..].fill(PackedCell::EMPTY);
                    }
                    1 => {
                        let row = next(6) * 12;
                        let start = row + next(12);
                        cells[start..row + 12].fill(PackedCell::EMPTY);
                    }
                    _ => {}
                }
                for _ in 0..next(5) {
                    let index = next(len);
                    let glyph = u32::from(b'a') + next(26) as u32;
                    cells[index] = PackedCell::new(glyph, 0, zz_terminal::CellWidth::Narrow);
                }
                adoptable += usize::from(
                    core.spare_cells
                        .get(&pane)
                        .is_some_and(|spare| Arc::strong_count(&spare.cells) == 1),
                );
                if next(97) == 0 {
                    core.handle_message(event(EventPayload::TerminalViewport {
                        pane,
                        viewport: current.clone(),
                    }));
                } else {
                    let patch = TerminalViewport::diff(&reference, &current).unwrap();
                    core.handle_message(event(EventPayload::TerminalPatch { pane, patch }));
                }
                reference = current;
                drain(&mut core);
                assert_eq!(
                    core.viewport(pane).unwrap().cells[..],
                    reference.cells[..],
                    "seed {seed} step {step}"
                );
                for (clone, cells) in &held {
                    assert_eq!(clone.cells[..], cells[..], "seed {seed} step {step}");
                }
                match next(4) {
                    0 if held.len() < 3 => {}
                    1 => {
                        held.drain(..held.len().min(1));
                        continue;
                    }
                    2 => {
                        held.clear();
                        continue;
                    }
                    _ => held.clear(),
                }
                let clone = core.viewport(pane).unwrap().clone();
                let cells = clone.cells.to_vec();
                held.push((clone, cells));
            }
        }
        assert!(adoptable > 0);
    }

    #[test]
    fn retained_history_is_opt_in_and_a_chunk_reports_a_viewport_change() {
        let pane = PaneId(3);
        let frame = |rows: [u32; 3], offset: u32, generation: u64| {
            let mut viewport = TerminalViewport::blank(1, 3, zz_terminal::SessionStatus::Running);
            viewport.generation = generation;
            viewport.view_generation = generation;
            viewport.scrollbar = zz_terminal::ScrollbarState {
                total: offset + 3,
                offset,
                len: 3,
            };
            for (cell, row) in Arc::make_mut(&mut viewport.cells).iter_mut().zip(rows) {
                *cell = PackedCell::new(0xe000 + row, 0, zz_terminal::CellWidth::Narrow);
            }
            viewport
        };
        let scrolled = |core: &mut ClientCore| {
            let first = frame([5, 6, 7], 5, 1);
            let patch = TerminalViewport::diff(&first, &frame([6, 7, 8], 6, 2)).unwrap();
            assert_eq!(patch.scroll, -1);
            core.handle_message(event(EventPayload::TerminalViewport {
                pane,
                viewport: first,
            }));
            core.handle_message(event(EventPayload::TerminalPatch { pane, patch }));
            drain(core);
        };

        let mut plain = ClientCore::new();
        scrolled(&mut plain);
        assert!(plain.retained_viewport(pane).unwrap().history.is_empty());

        let mut retaining = ClientCore::new();
        retaining.retain_history();
        scrolled(&mut retaining);
        assert_eq!(retaining.retained_viewport(pane).unwrap().history.len(), 1);
        let rows = [3, 4]
            .map(|row| {
                vec![PackedCell::new(
                    0xe000 + row,
                    0,
                    zz_terminal::CellWidth::Narrow,
                )]
            })
            .to_vec();
        let dictionary = retaining
            .viewport(pane)
            .unwrap()
            .dictionary
            .as_ref()
            .clone();
        assert!(retaining.apply_history_chunk(pane, 3, 9, 6, 1, rows, dictionary));
        assert_eq!(retaining.retained_viewport(pane).unwrap().history.len(), 3);
        assert_eq!(
            drain(&mut retaining),
            [CoreEvent::ViewportChanged {
                pane,
                damage: ViewportDamage::Rows(Vec::new()),
            }]
        );
    }

    #[test]
    fn a_terminal_negotiation_is_kept_for_the_terminal_client() {
        let mut core = ClientCore::new();
        assert!(core.terminal_negotiation().is_none());
        core.handle_message(event(EventPayload::TerminalNegotiation {
            features: vec!["RGB".to_owned()],
            user_keys: vec!["\x1b[99~".to_owned()],
        }));
        assert_eq!(drain(&mut core), [CoreEvent::MuxOptionsChanged]);
        assert_eq!(
            core.terminal_negotiation(),
            Some(&(vec!["RGB".to_owned()], vec!["\x1b[99~".to_owned()]))
        );
    }

    #[test]
    fn selected_option_patch_preserves_options_outside_subscription() {
        let mut core = ClientCore::new();
        let patch = MuxOptions::from_entries([(
            zz_protocol::MuxOptionKey::ExtendedKeys,
            zz_protocol::MuxOptionValue {
                value: "always".to_owned(),
                source: zz_protocol::MuxOptionSource::RuntimeCommand,
            },
        )]);
        core.handle_message(event(EventPayload::MuxOptionsPatched { options: patch }));
        assert_eq!(
            core.mux_options()
                .get(zz_protocol::MuxOptionKey::ExtendedKeys)
                .unwrap()
                .value,
            "always"
        );
        assert_eq!(
            core.mux_options()
                .get(zz_protocol::MuxOptionKey::Prefix)
                .unwrap()
                .value,
            "C-b"
        );
    }
}
