use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use zz_protocol::{
    AgentCommand, BrowserCommand, ChooseBufferSearchState, ChooseBufferState,
    ChooseTreeSearchState, ChooseTreeState, ClientMessageKind, CommandPromptState, CommandResponse,
    DisplayPanesState, Event, EventPayload, KeyBindingSnapshot, KeyTableSnapshot, MuxOptions,
    MuxSnapshot, PaneId, ProtocolMessage, ServerHello, SessionId, StatusLine, TerminalUiCommand,
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
    CommandPromptChanged,
    CommandOutputChanged,
    ChooseTreeChanged,
    ChooseBufferChanged,
    DisplayPanesChanged,
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
    /// An inbound message the core does not reduce (pasted-image previews,
    /// echoing of client-to-daemon variants); the shell keeps its own handling.
    Message(Box<ProtocolMessage>),
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
    viewports: HashMap<PaneId, TerminalViewport>,
    full_pending: HashSet<PaneId>,
    prefix_armed: bool,
    command_prompt: Option<CommandPromptState>,
    command_output: Option<(PaneId, TerminalViewport)>,
    choose_tree: Option<ChooseTreeState>,
    choose_buffer: Option<ChooseBufferState>,
    display_panes: Option<DisplayPanesState>,
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
            ProtocolMessage::Attached { session, snapshot } => {
                self.attached_session = Some(session);
                self.snapshot = Arc::new(snapshot);
                self.viewports.clear();
                self.full_pending.clear();
                self.command_output = None;
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
    pub fn viewport(&self, pane: PaneId) -> Option<&TerminalViewport> {
        self.viewports.get(&pane)
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
            .map(|(pane, viewport)| (*pane, viewport))
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

    fn reset_connection(&mut self, hello: ServerHello) {
        let ServerHello {
            protocol_version: _,
            server_id: _,
            client_id: _,
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
        self.snapshot = Arc::new(MuxSnapshot::default());
        self.attached_session = None;
        self.viewports.clear();
        self.full_pending.clear();
        self.prefix_armed = false;
        self.command_prompt = None;
        self.command_output = None;
        self.choose_tree = None;
        self.choose_buffer = None;
        self.display_panes = None;
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
            EventPayload::CommandOutput { pane, viewport } => {
                self.command_output = viewport.map(|viewport| (pane, viewport));
                self.events.push_back(CoreEvent::CommandOutputChanged);
            }
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
            EventPayload::PrefixArmed { armed } => {
                self.prefix_armed = armed;
                self.events.push_back(CoreEvent::PrefixArmed { armed });
            }
            EventPayload::PaneRemoved(pane) => {
                self.viewports.remove(&pane);
                self.full_pending.remove(&pane);
                if self
                    .command_output
                    .as_ref()
                    .is_some_and(|(output_pane, _)| *output_pane == pane)
                {
                    self.command_output = None;
                    self.events.push_back(CoreEvent::CommandOutputChanged);
                }
                self.events.push_back(CoreEvent::PaneRemoved { pane });
            }
            EventPayload::Detached { session, by } => {
                if self.attached_session == Some(session) {
                    self.attached_session = None;
                }
                self.events.push_back(CoreEvent::Detached { session, by });
            }
            EventPayload::ServerStopping => self.events.push_back(CoreEvent::ServerStopping),
            EventPayload::Bell { pane } => self.events.push_back(CoreEvent::Bell { pane }),
            EventPayload::FocusSidebar => self.events.push_back(CoreEvent::FocusSidebar),
            EventPayload::ClientMessage { pane, kind, text } => {
                self.events
                    .push_back(CoreEvent::ClientMessage { pane, kind, text });
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

    fn request_full(&mut self, pane: PaneId) {
        if self.full_pending.insert(pane) {
            self.outbound.push_back(Outbound::RequestFull(pane));
        }
    }

    /// Drop retained viewports for panes the snapshot no longer contains.
    fn retain_snapshot_panes(&mut self) {
        let live: HashSet<PaneId> = self
            .snapshot
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .flat_map(|window| window.panes.keys().copied())
            .collect();
        self.viewports.retain(|pane, _| live.contains(pane));
        self.full_pending.retain(|pane| live.contains(pane));
    }

    fn update_choose_tree(&mut self, search: Option<ChooseTreeSearchState>, selected: u32) {
        if let Some(state) = self.choose_tree.as_mut() {
            state.search = search;
            state.selected = selected;
            self.events.push_back(CoreEvent::ChooseTreeChanged);
        }
    }

    fn update_choose_buffer(&mut self, search: Option<ChooseBufferSearchState>, selected: u32) {
        if let Some(state) = self.choose_buffer.as_mut() {
            state.search = search;
            state.selected = selected;
            self.events.push_back(CoreEvent::ChooseBufferChanged);
        }
    }
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
