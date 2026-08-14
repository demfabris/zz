mod frames;
mod history;
mod reader;

pub use frames::{FrameInbox, FrameUpdate, merge_damage};
pub use history::{
    HistoryChunk, HistoryRing, HistoryRow, MAX_HISTORY_ROWS, local_scroll_gate, max_scroll_offset,
};

use std::{
    collections::{BTreeMap, HashMap},
    mem,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use async_channel::Receiver;
use zz_client::{ChromeKeymap, ChromeProfile, ClientCore};
use zz_daemon::{Endpoint, InteractiveClient};
use zz_protocol::{
    ChooseBufferState, ChooseTreeState, CommandInvocation, CommandPromptState, ConfigOverrideEntry,
    DisplayPanesState, InputMessage, KeyBindingSnapshot, LayoutNode, MuxOptionKey, MuxOptions,
    MuxSnapshot, NEW_SESSION_ATTACH_CAPABILITY, PaneId, PaneKindSnapshot, PaneSnapshot,
    ProtocolMessage, SPLIT_RATIO_BASIS, SessionId, SplitId, StatusLine, WindowId, canonical_key,
};
use zz_terminal::{
    AppearanceProvenance, ClipboardTarget, KeyInput, SearchDirection, TerminalAppearance,
    TerminalColorScheme, TerminalViewport,
};

/// tmux keeps a split from swallowing its neighbour whole; the desktop clamps a
/// divider drag to the same band before it reaches the wire.
pub const MIN_SPLIT_RATIO: f32 = 0.1;
pub const MAX_SPLIT_RATIO: f32 = 0.9;

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
    /// The daemon republished its mux options, effective values and all. The
    /// only honest source for what an override actually did.
    MuxOptionsChanged,
    /// A daemon-owned overlay moved: prefix arming, the command prompt, either
    /// chooser, or display-panes. Which one is read back through the accessors.
    OverlaysChanged,
    /// The daemon asked for the session tree, the way `focus-sidebar` and
    /// `choose-tree -s|-w` do.
    FocusSidebar,
    /// The frozen command-output view opened, changed or closed; read it back
    /// with [`Engine::command_output`].
    CommandOutputChanged,
    /// The daemon asked this client to open its search prompt — `C-b /` and the
    /// copy-mode search bindings arrive this way, never as a key.
    BeginSearch {
        pane: PaneId,
        direction: SearchDirection,
    },
    /// Scrollback rows landed in `pane`'s ring; a local scroll can repaint.
    HistoryChanged(PaneId),
    /// A link was activated in `pane`. The client opens it; nothing is retained.
    OpenUri {
        pane: PaneId,
        uri: String,
    },
    /// The daemon answered a copy request; the payload is not retained.
    Clipboard {
        target: ClipboardTarget,
        text: String,
    },
    Notice(String),
    /// The connection dropped and the engine is retrying, counting from one.
    /// Every accessor keeps answering with the last state the daemon sent, so
    /// the UI can leave the frozen frames on screen instead of tearing down.
    Reconnecting {
        attempt: u32,
    },
    /// A new connection is live and the remembered session is being re-attached;
    /// [`EngineEvent::Attached`] follows once the daemon agrees.
    Reconnected,
    Detached,
    /// The connection is gone for good — the retry window elapsed, or the
    /// daemon refused. Nothing else follows.
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

/// The display-free half of the client: one [`ClientCore`], one reader thread,
/// and whichever socket is currently live. Everything GTK-shaped lives above
/// it, so the whole protocol path is exercisable from a plain `#[test]`.
pub struct Engine {
    link: Arc<Link>,
    events: Receiver<EngineEvent>,
    chrome: ChromeKeymap,
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
        client
            .attach(session.to_owned())
            .map_err(|error| error.to_string())?;

        let link = Arc::new(Link {
            endpoint: endpoint.clone(),
            color_scheme: Mutex::new(color_scheme),
            client: Mutex::new(Arc::new(client)),
            core: Mutex::new(core),
            frames: FrameInbox::default(),
            geometry: Mutex::new(HashMap::new()),
            replay: Mutex::new(Vec::new()),
            remembered_reattach: AtomicBool::new(false),
            history: Mutex::new(HashMap::new()),
        });
        let (sender, events) = async_channel::unbounded();
        reader::spawn(Arc::clone(&link), sender)?;
        Ok(Arc::new(Self {
            link,
            events,
            chrome: ChromeKeymap::for_profile(ChromeProfile::DESKTOP),
        }))
    }

    pub fn events(&self) -> Receiver<EngineEvent> {
        self.events.clone()
    }

    pub fn take_frames(&self) -> Vec<FrameUpdate> {
        self.link.frames.take()
    }

    pub const fn chrome(&self) -> &ChromeKeymap {
        &self.chrome
    }

    pub fn snapshot(&self) -> Arc<MuxSnapshot> {
        Arc::clone(self.link.core().snapshot())
    }

    pub fn attached_session(&self) -> Option<SessionId> {
        self.link.core().attached_session()
    }

    pub fn status(&self) -> StatusLine {
        self.link.core().status().clone()
    }

    pub fn appearance(&self) -> TerminalAppearance {
        self.link.core().appearance().cloned().unwrap_or_default()
    }

    /// Where this client is attached, for the About page.
    pub fn endpoint(&self) -> String {
        self.link.endpoint.to_string()
    }

    /// What the daemon advertised in its handshake.
    pub fn capabilities(&self) -> Vec<String> {
        self.link.core().capabilities().to_vec()
    }

    /// Per-key provenance for the appearance the daemon resolved: whether a
    /// value came from a theme file, a Ghostty donor, an override, or nothing.
    pub fn appearance_provenance(&self) -> AppearanceProvenance {
        self.link.core().appearance_provenance().clone()
    }

    /// The daemon's complete mux option state, effective values plus the layer
    /// that last wrote each one.
    pub fn mux_options(&self) -> MuxOptions {
        self.link.core().mux_options().clone()
    }

    /// Whether this daemon accepts `SetConfigOverrides` at all. A skewed or
    /// older daemon keeps the client's daemon-owned keys local rather than
    /// having them silently dropped on the far side.
    pub fn supports_config_overrides(&self) -> bool {
        self.link
            .core()
            .capabilities()
            .iter()
            .any(|capability| capability == "config-overrides-v1")
    }

    /// Publish the daemon-owned half of `zz/config`. The vector is the file's
    /// own order with duplicates intact: the daemon applies last-writer per key
    /// and cumulative keys need every occurrence.
    pub fn set_config_overrides(&self, entries: Vec<ConfigOverrideEntry>) {
        if let Err(error) = self.link.client().set_config_overrides(entries) {
            log::warn!("zz-gtk failed to send configuration overrides: {error}");
        }
    }

    pub fn prefix_armed(&self) -> bool {
        self.link.core().prefix_armed()
    }

    /// The daemon's prefix chord in tmux grammar, or `None` until it publishes
    /// the option.
    pub fn prefix_key(&self) -> Option<String> {
        self.link
            .core()
            .mux_options()
            .get(MuxOptionKey::Prefix)
            .map(|option| canonical_key(&option.value))
    }

    /// The daemon-published `prefix` table, or empty before the hello. Keys are
    /// tmux-grammar strings; commands carry canonical names.
    pub fn prefix_bindings(&self) -> Vec<KeyBindingSnapshot> {
        self.link.core().prefix_bindings().to_vec()
    }

    pub fn command_prompt(&self) -> Option<CommandPromptState> {
        self.link.core().command_prompt().cloned()
    }

    pub fn choose_tree(&self) -> Option<ChooseTreeState> {
        self.link.core().choose_tree().cloned()
    }

    pub fn choose_buffer(&self) -> Option<ChooseBufferState> {
        self.link.core().choose_buffer().cloned()
    }

    pub fn display_panes(&self) -> Option<DisplayPanesState> {
        self.link.core().display_panes().cloned()
    }

    /// The frozen `list-keys`-style output view, if the daemon opened one for
    /// this client. The pane is the one it is anchored to, not a pane of its own.
    pub fn command_output(&self) -> Option<(PaneId, TerminalViewport)> {
        self.link
            .core()
            .command_output()
            .map(|(pane, viewport)| (pane, viewport.clone()))
    }

    /// Rows the ring can paint for the window starting at `target`, oldest
    /// first, alongside the live viewport top they stop at. Absent rows are
    /// `None`, which the painter shows as an unfilled band.
    pub fn history_window(&self, pane: PaneId, target: u32, rows: u16) -> Vec<Option<HistoryRow>> {
        let history = self.link.history();
        let ring = history.get(&pane).map(|entry| &entry.ring);
        (0..u32::from(rows))
            .map(|row| {
                ring.and_then(|ring| ring.row(target.saturating_add(row)))
                    .cloned()
            })
            .collect()
    }

    /// How far back the ring currently reaches, after retiring rows the pane
    /// has outrun. Called at scroll time, never per frame.
    pub fn history_rows(&self, pane: PaneId, viewport: &TerminalViewport) -> usize {
        let mut history = self.link.history();
        let entry = history.entry(pane).or_default();
        entry.ring.observe(viewport);
        entry.ring.len()
    }

    /// Ask for the next slice of scrollback that would let `target` be painted
    /// locally. One request per pane is outstanding at a time; the reader
    /// chains the next one as each chunk lands.
    pub fn request_history(&self, pane: PaneId, target: u32) {
        self.link.request_history(pane, target);
    }
}

/// Split-divider drags land here: the daemon owns the layout, so the client
/// previews a ratio locally and commits exactly one message on release.
impl Engine {
    pub fn resize_split(&self, window: WindowId, split: SplitId, ratio: f32) {
        let clamped =
            if ratio.is_finite() { ratio } else { 0.5 }.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO);
        self.send(InputMessage::ResizeSplit {
            window,
            split,
            ratio_basis_points: (clamped * f32::from(SPLIT_RATIO_BASIS)).round() as u16,
        });
    }

    /// Republish the desktop's light/dark preference; the daemon answers with a
    /// fresh appearance rather than the client recoloring anything itself. The
    /// preference is remembered so a reconnect dials with the current scheme.
    pub fn set_color_scheme(&self, color_scheme: TerminalColorScheme) {
        self.link.set_color_scheme(color_scheme);
    }

    /// A clone of the retained viewport: every visible plane is behind an
    /// `Arc`, so this is a handful of refcount bumps, not a grid copy.
    pub fn viewport(&self, pane: PaneId) -> Option<TerminalViewport> {
        self.link.core().viewport(pane).cloned()
    }

    pub fn session_view(&self) -> Option<SessionView> {
        self.link.session_view()
    }

    /// Publish a pane's cell geometry, skipping a resize the daemon already
    /// has; true when it reached the wire. Widgets re-measure on every
    /// allocation, so the filter is what keeps a window drag from becoming a
    /// resize storm.
    pub fn resize_terminal(
        &self,
        pane: PaneId,
        columns: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> bool {
        if columns == 0 || rows == 0 {
            return false;
        }
        self.link
            .publish_geometry(pane, (columns, rows, cell_width_px, cell_height_px))
    }

    pub fn send_key(&self, pane: PaneId, input: KeyInput, text_follows: bool) {
        self.link.send(InputMessage::Key {
            pane,
            input,
            text_follows,
        });
    }

    pub fn send_text(&self, pane: PaneId, text: String) {
        if text.is_empty() {
            return;
        }
        self.link.send(InputMessage::Text { pane, text });
    }

    /// Move this client to another session. The daemon answers with a fresh
    /// snapshot and an [`EngineEvent::Attached`]; nothing is assumed here.
    pub fn attach_session(&self, session: SessionId) {
        if let Err(error) = self.link.client().attach(session.to_string()) {
            log::warn!("zz-gtk failed to attach to {session}: {error}");
        }
    }

    /// Create a session and land in it. A daemon that advertises
    /// `new-session-attach-v1` attaches the client itself; an older one needs
    /// the follow-up attach, which is why the capability is consulted rather
    /// than assumed.
    pub fn new_session(&self) {
        self.execute(CommandInvocation::new("new-session", [] as [&str; 0]));
        let attaches = self
            .link
            .core()
            .capabilities()
            .iter()
            .any(|capability| capability == NEW_SESSION_ATTACH_CAPABILITY);
        if !attaches {
            self.execute(CommandInvocation::new("attach-session", [] as [&str; 0]));
        }
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

    /// Forward one input message untouched. Overlay keys ride this: the daemon
    /// owns chooser and prompt semantics, so the client never resolves them.
    pub fn send(&self, input: InputMessage) {
        self.link.send(input);
    }

    pub fn execute(&self, command: CommandInvocation) {
        let name = command.name.clone();
        if let Err(error) = self.link.client().execute(command) {
            log::warn!("zz-gtk failed to execute {name}: {error}");
        }
    }

    /// Leave the session without disturbing it: the daemon keeps running and
    /// every pane stays alive for the next client.
    pub fn detach(&self) {
        if let Err(error) = self.link.client().detach() {
            log::warn!("zz-gtk failed to detach: {error}");
        }
    }
}

/// The state a connection does not own. It outlives any single socket, which is
/// what a reconnect needs: the reader swaps the client underneath it while the
/// core keeps the viewports the widgets are still painting.
struct Link {
    endpoint: Endpoint,
    color_scheme: Mutex<TerminalColorScheme>,
    client: Mutex<Arc<InteractiveClient>>,
    core: Mutex<ClientCore>,
    frames: FrameInbox,
    geometry: Mutex<HashMap<PaneId, Geometry>>,
    replay: Mutex<Vec<(PaneId, Geometry)>>,
    remembered_reattach: AtomicBool,
    /// Scrollback the client keeps for local scrolling, one ring per pane.
    /// Deliberately off the frame path: rows arrive only through
    /// `HistoryRequest`, so nothing here runs while frames do.
    history: Mutex<HashMap<PaneId, PaneHistory>>,
}

/// A pane's ring plus the offset the client is still trying to reach with it.
#[derive(Default)]
struct PaneHistory {
    ring: HistoryRing,
    /// The oldest offset a scroll asked for; kept while requests are in flight
    /// so the reader can chain chunk after chunk until it is covered.
    wanted: Option<u32>,
    requesting: bool,
}

impl Link {
    fn client(&self) -> Arc<InteractiveClient> {
        Arc::clone(&self.client.lock().expect("client slot poisoned"))
    }

    fn core(&self) -> MutexGuard<'_, ClientCore> {
        self.core.lock().expect("client core poisoned")
    }

    fn history(&self) -> MutexGuard<'_, HashMap<PaneId, PaneHistory>> {
        self.history.lock().expect("pane history poisoned")
    }

    /// Issue the next backfill for `pane` unless one is already outstanding.
    /// `target` only ever moves the goal further back, so a fast scroll does not
    /// shorten a walk already under way.
    fn request_history(&self, pane: PaneId, target: u32) {
        let request = {
            let mut history = self.history();
            let entry = history.entry(pane).or_default();
            entry.wanted = Some(entry.wanted.map_or(target, |wanted| wanted.min(target)));
            if entry.requesting {
                None
            } else {
                entry.ring.next_request(target, MAX_HISTORY_ROWS)
            }
        };
        let Some((start, count)) = request else {
            return;
        };
        self.history().entry(pane).or_default().requesting = true;
        if let Err(error) = self.client().request_history(pane, start, count) {
            log::warn!("zz-gtk failed to request scrollback for {pane}: {error}");
            let mut history = self.history();
            let entry = history.entry(pane).or_default();
            entry.requesting = false;
            entry.wanted = None;
        }
    }

    /// Fold one chunk in and say whether the ring grew. The viewport it is
    /// checked against is the one live when the chunk landed: a chunk the pane
    /// has already outrun describes a scrollback that has moved.
    fn absorb_history(
        &self,
        pane: PaneId,
        chunk: HistoryChunk,
        viewport: &TerminalViewport,
    ) -> bool {
        let mut history = self.history();
        let entry = history.entry(pane).or_default();
        entry.requesting = false;
        let absorbed = entry.ring.absorb(chunk, viewport);
        if !absorbed {
            entry.wanted = None;
        }
        absorbed
    }

    /// The next slice of a walk that has not reached its goal yet.
    fn next_history_request(&self, pane: PaneId) -> Option<(u32, u32)> {
        let mut history = self.history();
        let entry = history.get_mut(&pane)?;
        let wanted = entry.wanted?;
        let next = entry.ring.next_request(wanted, MAX_HISTORY_ROWS);
        if next.is_none() {
            entry.wanted = None;
        } else {
            entry.requesting = true;
        }
        next
    }

    fn forget_history(&self, pane: PaneId) {
        self.history().remove(&pane);
    }

    fn clear_history(&self) {
        self.history().clear();
    }

    fn send(&self, input: InputMessage) {
        if let Err(error) = self.client().send_input(input) {
            log::warn!("zz-gtk failed to send input: {error}");
        }
    }

    fn set_color_scheme(&self, color_scheme: TerminalColorScheme) {
        *self.color_scheme.lock().expect("color scheme poisoned") = color_scheme;
        if let Err(error) = self.client().set_color_scheme(color_scheme) {
            log::warn!("zz-gtk failed to publish the color scheme: {error}");
        }
    }

    fn publish_geometry(&self, pane: PaneId, geometry: Geometry) -> bool {
        {
            let mut sent = self.geometry.lock().expect("geometry poisoned");
            if sent.get(&pane) == Some(&geometry) {
                return false;
            }
            sent.insert(pane, geometry);
        }
        let (columns, rows, cell_width_px, cell_height_px) = geometry;
        self.send(InputMessage::ResizeTerminal {
            pane,
            columns,
            rows,
            cell_width_px,
            cell_height_px,
        });
        true
    }

    /// Re-establish the connection without disturbing what the widgets show.
    /// Only the hello's settings are adopted — the full
    /// [`ClientCore::handle_message`] reset would clear the attachment and blank
    /// the workspace for a whole round trip — and the remembered session is
    /// re-attached by id, exactly as the desktop does.
    fn dial(&self) -> Result<(), String> {
        let color_scheme = *self.color_scheme.lock().expect("color scheme poisoned");
        let client = InteractiveClient::connect_endpoint(&self.endpoint, color_scheme)
            .map_err(|error| error.to_string())?;
        let session = {
            let mut core = self.core();
            core.adopt_hello(client.server_hello().clone());
            core.attached_session()
        };
        client
            .attach(session.map_or_else(String::new, |session| session.to_string()))
            .map_err(|error| error.to_string())?;
        self.remembered_reattach
            .store(session.is_some(), Ordering::Relaxed);
        let stale = mem::take(&mut *self.geometry.lock().expect("geometry poisoned"));
        *self.replay.lock().expect("geometry replay poisoned") = stale.into_iter().collect();
        *self.client.lock().expect("client slot poisoned") = Arc::new(client);
        Ok(())
    }

    /// A reconnected daemon knows nothing about pane geometry, and widgets only
    /// re-measure when GTK re-allocates them, so the sizes the UI last asked for
    /// are replayed for every pane the re-attached session still has. Anything
    /// the UI published in the meantime wins: [`Self::publish_geometry`] skips
    /// what the fresh connection already carries.
    fn replay_geometry(&self) {
        let replay = mem::take(&mut *self.replay.lock().expect("geometry replay poisoned"));
        if replay.is_empty() {
            return;
        }
        let Some(view) = self.session_view() else {
            return;
        };
        let panes: Vec<PaneId> = view.terminal_panes().collect();
        for (pane, geometry) in replay {
            if panes.contains(&pane) {
                self.publish_geometry(pane, geometry);
            }
        }
    }

    /// The remembered session can be gone from a restarted daemon. The desktop
    /// answers that one `MissingTarget` by falling back to the default session
    /// instead of surfacing an error, and so does this; true when the fallback
    /// consumed the response.
    fn retry_default_attach(&self, client: &InteractiveClient) -> bool {
        if !self.remembered_reattach.swap(false, Ordering::Relaxed) {
            return false;
        }
        if let Err(error) = client.attach("") {
            log::warn!("zz-gtk failed to fall back to the default session: {error}");
            return false;
        }
        true
    }

    fn session_view(&self) -> Option<SessionView> {
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
}

/// [`InteractiveClient::connect_endpoint`] consumes the hello during the
/// handshake, so it never reaches `recv()`; feeding it by hand is the only way
/// the core learns appearance, options and key tables.
fn seeded_core(client: &InteractiveClient) -> ClientCore {
    let mut core = ClientCore::new();
    core.handle_message(ProtocolMessage::ServerHello(client.server_hello().clone()));
    while core.poll_event().is_some() {}
    core
}
