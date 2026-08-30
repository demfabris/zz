use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use zz_protocol::{
    AgentCommand, AgentPaneWire, BrowserCommand, ChooseBufferSearchState, ChooseBufferState,
    ChooseTreeSearchState, ChooseTreeState, ClientMessageKind, CommandPromptState, CommandResponse,
    ConfirmState, DisplayPanesState, Event, EventPayload, KeyBindingSnapshot, KeyTableSnapshot,
    MenuState, MuxOptions, MuxSnapshot, PaneId, PopupState, ProtocolMessage, ServerHello,
    SessionId, StatusLine, TerminalUiCommand,
};
use zz_terminal::{
    AppearanceProvenance, ClipboardTarget, PackedCell, TerminalAppearance, TerminalDictionary,
    TerminalViewport, TerminalViewportPatch,
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
    Detached {
        session: SessionId,
        by: Option<String>,
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
    key_tables: Vec<KeyTableSnapshot>,
    status: StatusLine,
    snapshot: Arc<MuxSnapshot>,
    attached_session: Option<SessionId>,
    attached_read_only: bool,
    attached_client_flags: String,
    last_detach_reason: Option<CoreDetachReason>,
    viewports: HashMap<PaneId, TerminalViewport>,
    agent_states: HashMap<PaneId, AgentPaneWire>,
    full_pending: HashSet<PaneId>,
    prefix_armed: bool,
    command_prompt: Option<CommandPromptState>,
    command_output: Option<(u64, PaneId, TerminalViewport)>,
    command_output_watermark: u64,
    choose_tree: Option<ChooseTreeState>,
    choose_buffer: Option<ChooseBufferState>,
    display_panes: Option<DisplayPanesState>,
    popup: Option<PopupState>,
    menu: Option<MenuState>,
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
            ProtocolMessage::ServerHello(hello) => self.reset_connection(hello),
            ProtocolMessage::Attached {
                session,
                snapshot,
                read_only,
                client_flags,
            } => {
                self.attached_session = Some(session);
                self.attached_read_only = read_only;
                self.attached_client_flags = client_flags;
                self.snapshot = Arc::new(snapshot);
                self.viewports.clear();
                self.agent_states.clear();
                self.full_pending.clear();
                self.command_output = None;
                self.popup = None;
                self.menu = None;
                self.confirm = None;
                self.events.push_back(CoreEvent::Attached { session });
                self.events.push_back(CoreEvent::SnapshotChanged);
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
    pub fn key_tables(&self) -> &[KeyTableSnapshot] {
        &self.key_tables
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
        self.viewports.get(&pane)
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
    pub const fn command_prompt(&self) -> Option<&CommandPromptState> {
        self.command_prompt.as_ref()
    }

    #[must_use]
    pub fn command_output(&self) -> Option<(PaneId, &TerminalViewport)> {
        self.command_output
            .as_ref()
            .map(|(_, pane, viewport)| (*pane, viewport))
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
        self.key_tables = key_tables;
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
        self.command_prompt = None;
        self.command_output = None;
        self.choose_tree = None;
        self.choose_buffer = None;
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
        self.viewports.clear();
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
                self.snapshot = Arc::new(snapshot);
                self.full_pending.clear();
                self.retain_snapshot_panes();
                self.events.push_back(CoreEvent::SnapshotChanged);
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
                self.events.push_back(CoreEvent::MuxOptionsChanged);
            }
            EventPayload::StatusChanged { status } => {
                self.status = status;
                self.events.push_back(CoreEvent::StatusChanged);
            }
            EventPayload::KeyTablesChanged { tables } => {
                self.key_tables = tables;
                self.events.push_back(CoreEvent::KeyTablesChanged);
            }
            EventPayload::TerminalViewport { pane, viewport } => {
                self.full_pending.remove(&pane);
                self.viewports.insert(pane, viewport);
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
                self.events.push_back(CoreEvent::ChooseTreeChanged);
            }
            EventPayload::ChooseTreeUpdate { search, selected } => {
                self.update_choose_tree(search, selected);
            }
            EventPayload::ChooseBuffer { state } => {
                self.choose_buffer = state;
                self.events.push_back(CoreEvent::ChooseBufferChanged);
            }
            EventPayload::ChooseBufferUpdate { search, selected } => {
                self.update_choose_buffer(search, selected);
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
                    self.full_pending.remove(&previous.pane);
                }
                self.popup = state;
                self.events.push_back(CoreEvent::PopupChanged);
            }
            EventPayload::Menu { state } => {
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
            EventPayload::PaneRemoved(pane) => {
                self.viewports.remove(&pane);
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
                self.events.push_back(CoreEvent::Detached { session, by });
            }
            EventPayload::ServerStopping => self.events.push_back(CoreEvent::ServerStopping),
            EventPayload::Bell { pane } => self.events.push_back(CoreEvent::Bell { pane }),
            EventPayload::FocusSidebar => self.events.push_back(CoreEvent::FocusSidebar),
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
            } => {
                self.events.push_back(CoreEvent::Clipboard {
                    pane,
                    request_id,
                    target,
                    text,
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
            | EventPayload::ControlCommandOutput { .. }
            | EventPayload::ControlSourceFile { .. }
            | EventPayload::StartupConfigCauses { .. }
            | EventPayload::SubscriptionChanged { .. } => {}
        }
    }

    fn apply_patch(&mut self, pane: PaneId, patch: TerminalViewportPatch) {
        let Some(viewport) = self.viewports.get_mut(&pane) else {
            self.request_full(pane);
            return;
        };
        let damage = patch_damage(viewport, &patch);
        if viewport.apply_patch(patch).is_ok() {
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
                self.command_output = Some((output_id, pane, viewport));
                self.events.push_back(CoreEvent::CommandOutputChanged);
            }
            Some(viewport) if self.command_output_id() == Some(output_id) => {
                self.command_output = Some((output_id, pane, viewport));
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

/// Which rows a patch will touch, computed against the pre-apply viewport.
fn patch_damage(previous: &TerminalViewport, patch: &TerminalViewportPatch) -> ViewportDamage {
    if patch.scroll != 0
        || patch.foreground != previous.foreground
        || patch.background != previous.background
    {
        return ViewportDamage::All;
    }
    let mut rows = patch.changed_rows.row_indices().to_vec();
    rows.extend(previous.overlays.iter().map(|overlay| overlay.row));
    rows.extend(patch.overlays.iter().map(|overlay| overlay.row));
    rows.sort_unstable();
    rows.dedup();
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
        ProtocolMessage::ServerHello(ServerHello {
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
        })
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
            vec![CoreEvent::Detached { session, by: None }]
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
        core.adopt_hello(hello);
        assert_eq!(core.command_output_id(), Some(20));

        core.handle_message(command_output_frame(pane, 1, 2));
        assert_eq!(drain(&mut core), vec![CoreEvent::CommandOutputChanged]);
        assert_eq!(core.command_output_id(), Some(1));
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
                enabled: true,
            })],
            selected: Some(0),
            stay_open: false,
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
    }
}
