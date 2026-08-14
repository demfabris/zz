mod frames;

pub use frames::{FrameInbox, FrameUpdate, merge_damage};

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, MutexGuard},
    thread,
};

use async_channel::{Receiver, Sender};
use zz_client::{ChromeKeymap, ChromeProfile, ClientCore, CoreEvent, Outbound};
use zz_daemon::{Endpoint, InteractiveClient};
use zz_protocol::{
    BrowserCommand, ChooseBufferState, ChooseTreeState, CommandInvocation, CommandPromptState,
    CommandResponse, GuiResponse, InputMessage, LayoutNode, MuxSnapshot, PaneId, PaneKindSnapshot,
    PaneSnapshot, ProtocolMessage, SessionId, StatusLine, WindowId,
};
use zz_terminal::{
    ClipboardTarget, KeyInput, TerminalAppearance, TerminalColorScheme, TerminalViewport,
};

/// What the UI must react to. State changes are notifications — the new value
/// is read back through the accessors, exactly as [`ClientCore`] intends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    Attached(SessionId),
    SnapshotChanged,
    /// Frames are waiting in the inbox; drain with [`Engine::take_frames`].
    FramesReady,
    StatusChanged,
    AppearanceChanged,
    /// A daemon-owned overlay moved: prefix arming, the command prompt, or
    /// either chooser. Which one is read back through the accessors.
    OverlaysChanged,
    /// The daemon answered a copy request; the payload is not retained.
    Clipboard {
        target: ClipboardTarget,
        text: String,
    },
    Notice(String),
    Detached,
    Disconnected(String),
}

/// One zz window, as the tab strip needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTab {
    pub id: WindowId,
    pub index: u32,
    pub name: String,
}

/// The attached session's focused window, flattened for the shell. Scoped to
/// the attachment on purpose: panes of other sessions never receive frames.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionView {
    pub session: SessionId,
    pub name: String,
    pub windows: Vec<WindowTab>,
    pub focused_window: WindowId,
    pub layout: LayoutNode,
    pub zoomed_pane: Option<PaneId>,
    pub active_pane: PaneId,
    pub panes: BTreeMap<PaneId, PaneSnapshot>,
}

impl SessionView {
    pub fn terminal_panes(&self) -> impl Iterator<Item = PaneId> + '_ {
        self.panes
            .iter()
            .filter(|(_, pane)| matches!(pane.kind, PaneKindSnapshot::Terminal))
            .map(|(pane, _)| *pane)
    }
}

type Geometry = (u16, u16, u32, u32);

/// The display-free half of the client: one socket, one [`ClientCore`], one
/// reader thread. Everything GTK-shaped lives above it, so the whole protocol
/// path is exercisable from a plain `#[test]`.
pub struct Engine {
    client: Arc<InteractiveClient>,
    core: Arc<Mutex<ClientCore>>,
    frames: Arc<FrameInbox>,
    events: Receiver<EngineEvent>,
    chrome: ChromeKeymap,
    geometry: Mutex<HashMap<PaneId, Geometry>>,
}

impl Engine {
    /// Connect, seed the core with the handshake hello, and attach. An empty
    /// `session` attaches to the daemon's default, which is session "0" on a
    /// freshly booted daemon rather than the newest session.
    pub fn connect(
        endpoint: &Endpoint,
        session: &str,
        color_scheme: TerminalColorScheme,
    ) -> Result<Arc<Self>, String> {
        let client = InteractiveClient::connect_endpoint(endpoint, color_scheme)
            .map_err(|error| error.to_string())?;
        let core = seeded_core(&client);
        let client = Arc::new(client);
        client
            .attach(session.to_owned())
            .map_err(|error| error.to_string())?;

        let frames = Arc::new(FrameInbox::default());
        let (sender, events) = async_channel::unbounded();
        spawn_reader(
            Arc::clone(&client),
            Arc::clone(&core),
            Arc::clone(&frames),
            sender,
        )?;
        Ok(Arc::new(Self {
            client,
            core,
            frames,
            events,
            chrome: ChromeKeymap::for_profile(ChromeProfile::DESKTOP),
            geometry: Mutex::new(HashMap::new()),
        }))
    }

    pub fn events(&self) -> Receiver<EngineEvent> {
        self.events.clone()
    }

    pub fn take_frames(&self) -> Vec<FrameUpdate> {
        self.frames.take()
    }

    pub const fn chrome(&self) -> &ChromeKeymap {
        &self.chrome
    }

    pub fn snapshot(&self) -> Arc<MuxSnapshot> {
        Arc::clone(self.core().snapshot())
    }

    pub fn attached_session(&self) -> Option<SessionId> {
        self.core().attached_session()
    }

    pub fn status(&self) -> StatusLine {
        self.core().status().clone()
    }

    pub fn appearance(&self) -> TerminalAppearance {
        self.core().appearance().cloned().unwrap_or_default()
    }

    pub fn prefix_armed(&self) -> bool {
        self.core().prefix_armed()
    }

    pub fn command_prompt(&self) -> Option<CommandPromptState> {
        self.core().command_prompt().cloned()
    }

    pub fn choose_tree(&self) -> Option<ChooseTreeState> {
        self.core().choose_tree().cloned()
    }

    pub fn choose_buffer(&self) -> Option<ChooseBufferState> {
        self.core().choose_buffer().cloned()
    }

    /// Republish the desktop's light/dark preference; the daemon answers with a
    /// fresh appearance rather than the client recoloring anything itself.
    pub fn set_color_scheme(&self, color_scheme: TerminalColorScheme) {
        if let Err(error) = self.client.set_color_scheme(color_scheme) {
            log::warn!("zz-gtk failed to publish the color scheme: {error}");
        }
    }

    /// A clone of the retained viewport: every visible plane is behind an
    /// `Arc`, so this is a handful of refcount bumps, not a grid copy.
    pub fn viewport(&self, pane: PaneId) -> Option<TerminalViewport> {
        self.core().viewport(pane).cloned()
    }

    pub fn session_view(&self) -> Option<SessionView> {
        let core = self.core();
        let attached = core.attached_session()?;
        let snapshot = core.snapshot();
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == attached)?;
        let focused = snapshot.focused_window_for(session);
        let window = session.windows.iter().find(|window| window.id == focused)?;
        Some(SessionView {
            session: attached,
            name: session.name.clone(),
            windows: session
                .windows
                .iter()
                .map(|window| WindowTab {
                    id: window.id,
                    index: window.index,
                    name: window.name.clone(),
                })
                .collect(),
            focused_window: focused,
            layout: window.layout.clone(),
            zoomed_pane: window.zoomed_pane,
            active_pane: window.active_pane,
            panes: window.panes.clone(),
        })
    }

    /// Publish a pane's cell geometry, skipping a resize the daemon already
    /// has. Widgets re-measure on every allocation, so the filter is what keeps
    /// a window drag from becoming a resize storm.
    pub fn resize_terminal(
        &self,
        pane: PaneId,
        columns: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) {
        if columns == 0 || rows == 0 {
            return;
        }
        let geometry = (columns, rows, cell_width_px, cell_height_px);
        {
            let mut sent = self.geometry.lock().expect("geometry poisoned");
            if sent.get(&pane) == Some(&geometry) {
                return;
            }
            sent.insert(pane, geometry);
        }
        self.send(InputMessage::ResizeTerminal {
            pane,
            columns,
            rows,
            cell_width_px,
            cell_height_px,
        });
    }

    pub fn send_key(&self, pane: PaneId, input: KeyInput, text_follows: bool) {
        self.send(InputMessage::Key {
            pane,
            input,
            text_follows,
        });
    }

    pub fn send_text(&self, pane: PaneId, text: String) {
        if text.is_empty() {
            return;
        }
        self.send(InputMessage::Text { pane, text });
    }

    pub fn select_window(&self, window: WindowId) {
        self.execute(CommandInvocation::new(
            "select-window",
            ["-t", &window.to_string()],
        ));
    }

    pub fn select_pane(&self, pane: PaneId) {
        self.execute(CommandInvocation::new(
            "select-pane",
            ["-t", &pane.to_string()],
        ));
    }

    pub fn kill_pane(&self, pane: PaneId) {
        self.execute(CommandInvocation::new(
            "kill-pane",
            ["-t", &pane.to_string()],
        ));
    }

    pub fn execute(&self, command: CommandInvocation) {
        let name = command.name.clone();
        if let Err(error) = self.client.execute(command) {
            log::warn!("zz-gtk failed to execute {name}: {error}");
        }
    }

    /// Leave the session without disturbing it: the daemon keeps running and
    /// every pane stays alive for the next client.
    pub fn detach(&self) {
        if let Err(error) = self.client.detach() {
            log::warn!("zz-gtk failed to detach: {error}");
        }
    }

    /// Forward one input message untouched. Overlay keys ride this: the daemon
    /// owns chooser and prompt semantics, so the client never resolves them.
    pub fn send(&self, input: InputMessage) {
        if let Err(error) = self.client.send_input(input) {
            log::warn!("zz-gtk failed to send input: {error}");
        }
    }

    fn core(&self) -> MutexGuard<'_, ClientCore> {
        lock_core(&self.core)
    }
}

/// [`InteractiveClient::connect_endpoint`] consumes the hello during the
/// handshake, so it never reaches `recv()`; feeding it by hand is the only way
/// the core learns appearance, options and key tables.
fn seeded_core(client: &InteractiveClient) -> Arc<Mutex<ClientCore>> {
    let mut core = ClientCore::new();
    core.handle_message(ProtocolMessage::ServerHello(client.server_hello().clone()));
    while core.poll_event().is_some() {}
    Arc::new(Mutex::new(core))
}

fn lock_core(core: &Mutex<ClientCore>) -> MutexGuard<'_, ClientCore> {
    core.lock().expect("client core poisoned")
}

/// Reduces one connection: decoded messages in, wire requests straight back
/// out, frames into the coalescing inbox, everything else to the UI in stream
/// order. Event sequence gaps are never checked — the daemon supersedes stale
/// frames under backpressure, so a healthy stream skips numbers.
fn spawn_reader(
    client: Arc<InteractiveClient>,
    core: Arc<Mutex<ClientCore>>,
    frames: Arc<FrameInbox>,
    events: Sender<EngineEvent>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("zz-gtk-protocol".to_owned())
        .spawn(move || {
            loop {
                let message = match client.recv() {
                    Ok(message) => message,
                    Err(error) => {
                        let _ = events.send_blocking(EngineEvent::Disconnected(error.to_string()));
                        break;
                    }
                };
                let forwarded = {
                    let mut core = lock_core(&core);
                    core.handle_message(message);
                    while let Some(Outbound::RequestFull(pane)) = core.poll_outbound() {
                        if let Err(error) = client.request_full(pane) {
                            log::warn!(
                                "zz-gtk failed to request a full viewport for {pane}: {error}"
                            );
                        }
                    }
                    let mut forwarded = Vec::new();
                    while let Some(event) = core.poll_event() {
                        reduce(&core, &client, &frames, event, &mut forwarded);
                    }
                    forwarded
                };
                for event in forwarded {
                    if events.send_blocking(event).is_err() {
                        return;
                    }
                }
            }
        })
        .map(drop)
        .map_err(|error| format!("failed to start the protocol reader: {error}"))
}

fn reduce(
    core: &ClientCore,
    client: &InteractiveClient,
    frames: &FrameInbox,
    event: CoreEvent,
    forwarded: &mut Vec<EngineEvent>,
) {
    match event {
        CoreEvent::ViewportChanged { pane, damage } => {
            if let Some(viewport) = core.viewport(pane)
                && frames.publish(pane, viewport.clone(), damage)
            {
                forwarded.push(EngineEvent::FramesReady);
            }
        }
        CoreEvent::Attached { session } => {
            frames.clear();
            forwarded.push(EngineEvent::Attached(session));
        }
        CoreEvent::PaneRemoved { pane } => {
            frames.forget(pane);
            forwarded.push(EngineEvent::SnapshotChanged);
        }
        CoreEvent::SnapshotChanged => forwarded.push(EngineEvent::SnapshotChanged),
        CoreEvent::StatusChanged => forwarded.push(EngineEvent::StatusChanged),
        CoreEvent::AppearanceChanged => forwarded.push(EngineEvent::AppearanceChanged),
        CoreEvent::PrefixArmed { .. }
        | CoreEvent::CommandPromptChanged
        | CoreEvent::ChooseTreeChanged
        | CoreEvent::ChooseBufferChanged => forwarded.push(EngineEvent::OverlaysChanged),
        CoreEvent::Clipboard { target, text, .. } => {
            forwarded.push(EngineEvent::Clipboard { target, text });
        }
        CoreEvent::ClientMessage { text, .. } => forwarded.push(EngineEvent::Notice(text)),
        CoreEvent::CommandResponse(CommandResponse::Error { error, .. }) => {
            forwarded.push(EngineEvent::Notice(error.to_string()));
        }
        CoreEvent::Detached { .. } | CoreEvent::ServerStopping => {
            forwarded.push(EngineEvent::Detached);
        }
        CoreEvent::AgentCommand { request_id, .. } => {
            reject_gui_request(client, request_id, "agent commands require the zz app");
        }
        CoreEvent::BrowserCommand {
            command: BrowserCommand::Screenshot { request_id, .. },
            ..
        } => reject_gui_request(client, request_id, "browser panes require the zz app"),
        _ => {}
    }
}

fn reject_gui_request(client: &InteractiveClient, request_id: u64, message: &str) {
    if let Err(error) = client.send_gui_response(GuiResponse::Error {
        request_id,
        message: message.to_owned(),
    }) {
        log::warn!("zz-gtk failed to answer a GUI request: {error}");
    }
}
