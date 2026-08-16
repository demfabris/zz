//! GPUI-side client for the daemon-owned multiplexer.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    io,
    path::PathBuf,
    sync::Arc,
    thread,
    time::Duration,
};

use gpui::{ClipboardItem, Context, EventEmitter, Image, RenderImage};
use image::{Frame as ImageFrame, ImageBuffer, Rgba};
use parking_lot::RwLock;
use zz_browser::{diagnostic_url, normalize_url};
use zz_client::{ClientCore, CoreEvent, Outbound};
use zz_daemon::{
    AskpassPrompt, AskpassReply, DaemonError, Endpoint, EndpointError, InteractiveClient,
    terminate_incompatible_daemon,
};
use zz_protocol::{
    AgentCommand, BrowserCommand, ChooseBufferState, ChooseTreeState, ClientMessageKind,
    CommandInvocation, CommandPromptState, CommandResponse, DisplayPanesState, Event, EventPayload,
    GuiResponse, InputMessage, KeyBindingSnapshot, LayoutNode, MuxOptionKey, MuxSnapshot,
    NEW_SESSION_ATTACH_CAPABILITY, PROTOCOL_VERSION, PaneId, PaneKindSnapshot, PastedImageFormat,
    ProtocolError, ProtocolMessage, ServerError, ServerHello, SessionId, StatusLine,
    TerminalUiCommand, WindowSnapshot,
};
use zz_terminal::{
    AppearanceProvenance, ClipboardTarget, GRAPHEME_TABLE_BIT, IMAGE_PLACEHOLDER_SCHEME,
    PackedCell, ScrollbarState, TerminalAppearance, TerminalColorScheme, TerminalDictionary,
    TerminalDiffScratch, TerminalViewport, TerminalViewportPatch,
};

use crate::{
    diagnostics,
    mux::hosts::{HostId, HostRegistry, HostState},
    terminal::view::localize_terminal_font_families,
};

/// One client-to-daemon agent request, named so the shell states its intent
/// once and the transport picks the matching [`InteractiveClient`] helper.
#[cfg(feature = "agent-pane")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentRequest {
    Prompt {
        text: String,
        images: Vec<zz_protocol::AgentImage>,
    },
    Cancel,
    Unqueue,
    RespondPermission {
        request_id: u64,
        option_id: Option<String>,
    },
    SetConfigOption {
        option_id: String,
        value: String,
    },
    SetMode {
        mode_id: String,
    },
    Authenticate {
        method_id: String,
    },
    SessionOp {
        op: zz_protocol::AgentSessionOpKind,
    },
    Replay {
        from_seq: u64,
    },
    AcknowledgePromptRestore {
        reclaim_id: u64,
    },
}

/// What one drain hands the agent controller.
#[cfg(feature = "agent-pane")]
#[derive(Default)]
pub(crate) struct AgentEvents {
    pub(crate) items: Vec<(PaneId, Vec<zz_daemon::AgentStreamItem>)>,
    pub(crate) states: Vec<(PaneId, zz_protocol::AgentPaneWire)>,
    pub(crate) sessions: Vec<(PaneId, u64, String)>,
}

#[cfg(feature = "agent-pane")]
impl AgentEvents {
    fn is_empty(&self) -> bool {
        self.items.is_empty() && self.states.is_empty() && self.sessions.is_empty()
    }
}

const MAX_PENDING_DECODED_MESSAGES: usize = 1;
const MAX_HISTORY_ROWS: usize = 10_000;
const MAX_HISTORY_CHUNK_ROWS: u32 = 512;
const HISTORY_BACKFILL_QUIET: Duration = Duration::from_millis(100);
const MAX_PANE_IMAGE_SNAPSHOTS: usize = 8;
const MAX_TRACKED_COMMANDS: usize = 32;
const TERMINAL_FONT_SIZE_STEP_POINTS: f32 = 1.0;
const MIN_TERMINAL_FONT_SIZE_POINTS: f32 = 1.0;
const MAX_TERMINAL_FONT_SIZE_POINTS: f32 = 256.0;

fn new_session_commands(host: HostId, capabilities: &[String]) -> Vec<CommandInvocation> {
    if host != HostId::LOCAL {
        return vec![CommandInvocation::new("new-session", ["-d"])];
    }
    let mut commands = vec![CommandInvocation::new("new-session", [] as [&str; 0])];
    if !capabilities
        .iter()
        .any(|capability| capability == NEW_SESSION_ATTACH_CAPABILITY)
    {
        commands.push(CommandInvocation::new("attach-session", [] as [&str; 0]));
    }
    commands
}

fn snapshot_contains_pane(snapshot: &MuxSnapshot, pane: PaneId) -> bool {
    snapshot
        .sessions
        .iter()
        .flat_map(|session| &session.windows)
        .any(|window| window.panes.contains_key(&pane))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalFontSizeAdjustment {
    Increase,
    Decrease,
}

impl TerminalFontSizeAdjustment {
    const fn delta_points(self) -> f32 {
        match self {
            Self::Increase => TERMINAL_FONT_SIZE_STEP_POINTS,
            Self::Decrease => -TERMINAL_FONT_SIZE_STEP_POINTS,
        }
    }
}

const DIAGNOSTIC_TARGET: &str = "zz::diagnostics::mux";
const MAX_UNATTACHED_RECONNECT_ATTEMPTS: u32 = 3;

fn reconnect_delay(attempt: u32) -> Duration {
    Duration::from_secs(match attempt {
        0 | 1 => 1,
        2 => 2,
        3 => 4,
        4 => 8,
        5 => 16,
        _ => 30,
    })
}

fn decoded_message_channel() -> (
    async_channel::Sender<ProtocolMessage>,
    async_channel::Receiver<ProtocolMessage>,
) {
    async_channel::bounded(MAX_PENDING_DECODED_MESSAGES)
}

fn connect_error_state(error: &DaemonError) -> HostState {
    if let DaemonError::IncompatibleDaemon { daemon, client } = error {
        return daemon.map_or_else(
            || HostState::Unreachable {
                reason: error.to_string(),
            },
            |daemon| HostState::Incompatible {
                local: *client,
                remote: daemon,
            },
        );
    }
    let mut ssh_reason = None;
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if ssh_reason.is_none()
            && let Some(endpoint_error) = current.downcast_ref::<EndpointError>()
        {
            if let EndpointError::RemoteProtocolMismatch { daemon, client, .. } = endpoint_error {
                return HostState::Incompatible {
                    local: *client,
                    remote: *daemon,
                };
            }
            ssh_reason = endpoint_error.ssh_reason();
        }
        if let Some(ProtocolError::VersionMismatch { received, .. }) =
            current.downcast_ref::<ProtocolError>()
        {
            return HostState::Incompatible {
                local: PROTOCOL_VERSION,
                remote: *received,
            };
        }
        if let Some(ServerError::ProtocolMismatch { server, .. }) =
            current.downcast_ref::<ServerError>()
        {
            return HostState::Incompatible {
                local: PROTOCOL_VERSION,
                remote: *server,
            };
        }
        if let Some(error) = current.downcast_ref::<io::Error>()
            && let Some(inner) = error.get_ref()
        {
            source = Some(inner);
            continue;
        }
        source = current.source();
    }
    HostState::Unreachable {
        reason: ssh_reason.unwrap_or_else(|| error.to_string()),
    }
}

#[cfg(test)]
fn connect_result_state<T>(result: &Result<T, DaemonError>) -> HostState {
    match result {
        Ok(_) => HostState::Connected,
        Err(error) => connect_error_state(error),
    }
}

fn reconnect_timer_is_current(connection: &HostConnection, generation: u64, attempt: u32) -> bool {
    connection.reconnect_generation == generation
        && connection.state == (HostState::Reconnecting { attempt })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OpenUriRoute {
    Embedded { pane: PaneId, url: String },
    PastedImage { pane: PaneId, number: u32 },
    External,
}

#[derive(Default)]
struct PaneImageSnapshots {
    images: BTreeMap<u32, Arc<Image>>,
    order: VecDeque<u32>,
    revision: u64,
}

impl PaneImageSnapshots {
    fn insert(&mut self, number: u32, image: Arc<Image>) {
        self.images.insert(number, image);
        self.order.retain(|stored| *stored != number);
        self.order.push_back(number);
        while self.images.len() > MAX_PANE_IMAGE_SNAPSHOTS {
            if let Some(oldest) = self.order.pop_front() {
                self.images.remove(&oldest);
            }
        }
        self.revision = self.revision.wrapping_add(1).max(1);
    }

    fn get(&self, number: u32) -> Option<Arc<Image>> {
        self.images.get(&number).cloned()
    }

    fn remove(&mut self, number: u32) {
        if self.images.remove(&number).is_some() {
            self.order.retain(|stored| *stored != number);
            self.revision = self.revision.wrapping_add(1).max(1);
        }
    }
}

/// A pasted image a terminal pane asked to reopen.
pub(crate) struct AttachmentPreviewRequest {
    pub(crate) image: Arc<Image>,
}

/// A question ssh asked while dialling `host`, waiting on an answer. Dropping
/// `reply` unanswered cancels the prompt.
pub(crate) struct SshPromptRequest {
    pub(crate) host: HostId,
    /// The ssh destination, so several hosts dialling at once stay apart.
    pub(crate) label: String,
    pub(crate) prompt: AskpassPrompt,
    pub(crate) reply: async_channel::Sender<AskpassReply>,
}

const AUTH_DECLINED_REASON: &str = "Authentication was cancelled.\nPick Reconnect when you are \
                                    ready to sign in again.";

fn ssh_destination_label(endpoint: &zz_daemon::SshEndpoint) -> String {
    let mut label = String::new();
    if let Some(user) = &endpoint.user {
        label.push_str(user);
        label.push('@');
    }
    if endpoint.host.contains(':') {
        label.push('[');
        label.push_str(&endpoint.host);
        label.push(']');
    } else {
        label.push_str(&endpoint.host);
    }
    if let Some(port) = endpoint.port {
        label.push(':');
        label.push_str(&port.to_string());
    }
    label
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BrowserTargetScore {
    tree_distance: usize,
    layout_distance: usize,
    before_source: bool,
    layout_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BrowserTarget {
    score: BrowserTargetScore,
    pane: PaneId,
}

fn open_uri_route(
    snapshot: &MuxSnapshot,
    attached_session: Option<SessionId>,
    source: PaneId,
    uri: &str,
) -> OpenUriRoute {
    if let Some(number) = pasted_image_number(uri) {
        return OpenUriRoute::PastedImage {
            pane: source,
            number,
        };
    }
    let Ok(url) = normalize_url(uri) else {
        return OpenUriRoute::External;
    };
    if url.starts_with("file:") {
        return OpenUriRoute::External;
    }
    let Some(pane) = embedded_browser_target(snapshot, attached_session, source) else {
        return OpenUriRoute::External;
    };
    OpenUriRoute::Embedded { pane, url }
}

fn pasted_image_number(uri: &str) -> Option<u32> {
    uri.strip_prefix(IMAGE_PLACEHOLDER_SCHEME)?
        .strip_prefix("://")?
        .parse()
        .ok()
}

const fn gpui_image_format(format: PastedImageFormat) -> gpui::ImageFormat {
    match format {
        PastedImageFormat::Png => gpui::ImageFormat::Png,
        PastedImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        PastedImageFormat::Gif => gpui::ImageFormat::Gif,
        PastedImageFormat::Webp => gpui::ImageFormat::Webp,
    }
}

fn embedded_browser_target(
    snapshot: &MuxSnapshot,
    attached_session: Option<SessionId>,
    source: PaneId,
) -> Option<PaneId> {
    let session = snapshot
        .sessions
        .iter()
        .find(|session| Some(session.id) == attached_session)?;
    let window = session
        .windows
        .iter()
        .find(|window| window.layout.contains(source))?;
    nearest_browser_in_window(window, source)
}

fn nearest_browser_in_window(window: &WindowSnapshot, source: PaneId) -> Option<PaneId> {
    let mut source_path = Vec::new();
    let mut next_index = 0;
    let (source_path, source_index) =
        pane_path_and_index(&window.layout, source, &mut source_path, &mut next_index)?;

    let mut path = Vec::new();
    let mut next_index = 0;
    let mut best = None;
    visit_browser_panes(
        &window.layout,
        window,
        &source_path,
        source_index,
        &mut path,
        &mut next_index,
        &mut best,
    );
    best.map(|candidate| candidate.pane)
}

fn pane_path_and_index(
    node: &LayoutNode,
    target: PaneId,
    path: &mut Vec<bool>,
    next_index: &mut usize,
) -> Option<(Vec<bool>, usize)> {
    match node {
        LayoutNode::Pane(pane) => {
            let index = *next_index;
            *next_index += 1;
            (*pane == target).then(|| (path.clone(), index))
        }
        LayoutNode::Split { first, second, .. } => {
            path.push(false);
            let found = pane_path_and_index(first, target, path, next_index);
            path.pop();
            if found.is_some() {
                return found;
            }
            path.push(true);
            let found = pane_path_and_index(second, target, path, next_index);
            path.pop();
            found
        }
    }
}

fn visit_browser_panes(
    node: &LayoutNode,
    window: &WindowSnapshot,
    source_path: &[bool],
    source_index: usize,
    path: &mut Vec<bool>,
    next_index: &mut usize,
    best: &mut Option<BrowserTarget>,
) {
    match node {
        LayoutNode::Pane(pane) => {
            let index = *next_index;
            *next_index += 1;
            if !matches!(
                window.panes.get(pane).map(|pane| &pane.kind),
                Some(PaneKindSnapshot::Browser(_))
            ) {
                return;
            }
            let shared_depth = source_path
                .iter()
                .zip(path.iter())
                .take_while(|(source, candidate)| source == candidate)
                .count();
            let tree_distance = source_path.len() + path.len() - 2 * shared_depth;
            let score = BrowserTargetScore {
                tree_distance,
                layout_distance: index.abs_diff(source_index),
                before_source: index < source_index,
                layout_index: index,
            };
            if best
                .as_ref()
                .is_none_or(|candidate| score < candidate.score)
            {
                *best = Some(BrowserTarget { score, pane: *pane });
            }
        }
        LayoutNode::Split { first, second, .. } => {
            path.push(false);
            visit_browser_panes(
                first,
                window,
                source_path,
                source_index,
                path,
                next_index,
                best,
            );
            path.pop();
            path.push(true);
            visit_browser_panes(
                second,
                window,
                source_path,
                source_index,
                path,
                next_index,
                best,
            );
            path.pop();
        }
    }
}

#[derive(Clone)]
pub(crate) struct HistoryRow {
    pub(crate) cells: Box<[PackedCell]>,
    pub(crate) dictionary: Arc<TerminalDictionary>,
    pub(crate) revision: u64,
}

#[derive(Default)]
pub(crate) struct HistoryRing {
    pub(crate) rows: VecDeque<HistoryRow>,
}

impl HistoryRing {
    fn push_back(&mut self, row: HistoryRow) {
        self.rows.push_back(row);
    }

    fn enforce_cap(&mut self) {
        while self.rows.len() > MAX_HISTORY_ROWS {
            self.rows.pop_front();
        }
    }

    fn prepend(
        &mut self,
        rows: Vec<Vec<PackedCell>>,
        dictionary: TerminalDictionary,
        next_row_revision: &mut u64,
    ) {
        let dictionary = Arc::new(dictionary);
        for cells in rows.into_iter().rev() {
            self.rows.push_front(HistoryRow {
                cells: cells.into_boxed_slice(),
                dictionary: Arc::clone(&dictionary),
                revision: allocate_row_revision(next_row_revision),
            });
        }
        while self.rows.len() > MAX_HISTORY_ROWS {
            self.rows.pop_front();
        }
    }

    fn clear(&mut self) {
        self.rows.clear();
    }

    fn len(&self) -> usize {
        self.rows.len()
    }
}

pub(crate) struct RetainedTerminalViewport {
    pub(crate) viewport: TerminalViewport,
    pub(crate) history: HistoryRing,
    pub(crate) history_scrollbar: ScrollbarState,
    /// Bumped on every live-patch ring mutation or drop. A `HistoryChunk` applies
    /// only when this still matches the value snapshotted at request time.
    pub(crate) history_mutations: u64,
    /// Bumped only when the ring is dropped, which is what retires the
    /// local-scroll overlay.
    pub(crate) history_invalidations: u64,
    pub(crate) row_revisions: Box<[u64]>,
    pub(crate) row_revision_epoch: u64,
    pub(crate) revision_scratch: Vec<u16>,
}

struct KittyCachedImage {
    generation: u64,
    image: Arc<RenderImage>,
}

/// Pane-local GPU image ownership shared by the mux client and terminal view.
/// Replaced textures wait in `retired` until a paint-capable Window can call
/// `drop_image`.
#[derive(Default)]
pub(crate) struct KittyImageCache {
    images: HashMap<u32, KittyCachedImage>,
    retired: Vec<Arc<RenderImage>>,
    revision: u64,
}

impl KittyImageCache {
    pub(crate) fn image(&self, image_id: u32, generation: u64) -> Option<Arc<RenderImage>> {
        self.images
            .get(&image_id)
            .filter(|cached| cached.generation == generation)
            .map(|cached| Arc::clone(&cached.image))
    }

    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn take_retired(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.retired)
    }

    fn contains(&self, image_id: u32, generation: u64) -> bool {
        self.images
            .get(&image_id)
            .is_some_and(|cached| cached.generation == generation)
    }

    fn insert(
        &mut self,
        image_id: u32,
        generation: u64,
        image: Arc<RenderImage>,
        retain_replaced: bool,
    ) {
        if self.contains(image_id, generation) {
            return;
        }
        if let Some(previous) = self
            .images
            .insert(image_id, KittyCachedImage { generation, image })
            && retain_replaced
        {
            self.retired.push(previous.image);
        }
        self.revision = self.revision.wrapping_add(1).max(1);
    }

    fn remove(&mut self, image_id: u32, retain_removed: bool) {
        if let Some(previous) = self.images.remove(&image_id) {
            if retain_removed {
                self.retired.push(previous.image);
            }
            self.revision = self.revision.wrapping_add(1).max(1);
        }
    }

    fn clear(&mut self) {
        if self.images.is_empty() {
            return;
        }
        self.retired
            .extend(self.images.drain().map(|(_, cached)| cached.image));
        self.revision = self.revision.wrapping_add(1).max(1);
    }
}

struct KittyImageAssembly {
    width: u32,
    height: u32,
    total_bytes: usize,
    bytes: Vec<u8>,
}

struct PastedImageAssembly {
    format: PastedImageFormat,
    total_bytes: usize,
    bytes: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct CommandOutputModel {
    pub(crate) pane: PaneId,
    pub(crate) retained: Arc<RwLock<RetainedTerminalViewport>>,
}

pub(crate) struct ClientNotification {
    pub(crate) kind: ClientMessageKind,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StaleDaemonInfo {
    pub(crate) daemon: Option<u16>,
}

#[cfg(test)]
struct FakeConnectedHost {
    hello: ServerHello,
    attached_session: std::cell::Cell<Option<SessionId>>,
    attached_default: std::cell::Cell<bool>,
    commands: std::cell::RefCell<Vec<CommandInvocation>>,
    history_requests: std::cell::RefCell<Vec<(PaneId, u32, u32)>>,
    next_request_id: std::cell::Cell<u64>,
}

#[cfg(test)]
impl FakeConnectedHost {
    fn new(hello: ServerHello) -> Self {
        Self {
            hello,
            attached_session: std::cell::Cell::new(None),
            attached_default: std::cell::Cell::new(false),
            commands: std::cell::RefCell::new(Vec::new()),
            history_requests: std::cell::RefCell::new(Vec::new()),
            next_request_id: std::cell::Cell::new(1),
        }
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "the fake mirrors the connected client's fallible signature"
    )]
    fn attach(&self, session: Option<SessionId>) -> Result<(), DaemonError> {
        match session {
            Some(session) => self.attached_session.set(Some(session)),
            None => self.attached_default.set(true),
        }
        Ok(())
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "the fake mirrors the connected client's fallible signature"
    )]
    fn execute(&self, command: CommandInvocation) -> Result<u64, DaemonError> {
        self.commands.borrow_mut().push(command);
        let request_id = self.next_request_id.get();
        self.next_request_id.set(request_id.saturating_add(1));
        Ok(request_id)
    }

    fn request_history(&self, pane: PaneId, start: u32, count: u32) {
        self.history_requests
            .borrow_mut()
            .push((pane, start, count));
    }
}

#[derive(Clone, Copy)]
struct PendingHistoryRequest {
    mutations: u64,
    prefetch_target: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReconnectAttachState {
    RememberedSession,
    DefaultSession,
}

struct HostConnection {
    client: Option<Arc<InteractiveClient>>,
    #[cfg(test)]
    fake_client: Option<Arc<FakeConnectedHost>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    resync_pending: bool,
    full_requests_pending: BTreeSet<PaneId>,
    history_requests_pending: BTreeMap<PaneId, PendingHistoryRequest>,
    history_backfill_deferred: BTreeMap<PaneId, u64>,
    snapshot: Option<Arc<MuxSnapshot>>,
    appearance: Option<(TerminalAppearance, AppearanceProvenance)>,
    state: HostState,
    route: Arc<RwLock<Option<HostId>>>,
    last_attached_session: Option<SessionId>,
    reconnect_generation: u64,
    reconnect_attempt_in_flight: Option<u32>,
    reconnect_attach: Option<ReconnectAttachState>,
    in_flight_commands: RwLock<VecDeque<(u64, String)>>,
    ssh_auth_declined: bool,
}

impl HostConnection {
    fn disconnected(host: HostId) -> Self {
        Self::disconnected_with_route(Arc::new(RwLock::new(Some(host))))
    }

    fn disconnected_with_route(route: Arc<RwLock<Option<HostId>>>) -> Self {
        Self {
            client: None,
            #[cfg(test)]
            fake_client: None,
            reader_thread: None,
            resync_pending: false,
            full_requests_pending: BTreeSet::new(),
            history_requests_pending: BTreeMap::new(),
            history_backfill_deferred: BTreeMap::new(),
            snapshot: None,
            appearance: None,
            state: HostState::Disconnected,
            route,
            last_attached_session: None,
            reconnect_generation: 0,
            reconnect_attempt_in_flight: None,
            reconnect_attach: None,
            in_flight_commands: RwLock::new(VecDeque::new()),
            ssh_auth_declined: false,
        }
    }

    fn connected(
        client: Arc<InteractiveClient>,
        host: HostId,
        cx: &mut Context<MuxClient>,
    ) -> Result<Self, io::Error> {
        Self::connected_with_route(client, Arc::new(RwLock::new(Some(host))), cx)
    }

    fn connected_with_route(
        client: Arc<InteractiveClient>,
        route: Arc<RwLock<Option<HostId>>>,
        cx: &mut Context<MuxClient>,
    ) -> Result<Self, io::Error> {
        let appearance = Some((
            client.server_hello().appearance.clone(),
            client.server_hello().appearance_provenance.clone(),
        ));
        let receiver = Arc::clone(&client);
        let (messages, incoming) = decoded_message_channel();
        let reader_thread = thread::Builder::new()
            .name("zz-mux-reader".to_owned())
            .spawn(move || {
                while let Ok(message) = receiver.recv() {
                    if messages.send_blocking(message).is_err() {
                        break;
                    }
                }
            })?;
        let message_route = Arc::clone(&route);
        cx.spawn(async move |this, cx| {
            while let Ok(message) = incoming.recv().await {
                let applied = this.update(cx, |client, cx| {
                    let Some(host) = *message_route.read() else {
                        return false;
                    };
                    client.handle_message(host, message, cx);
                    true
                });
                if !matches!(applied, Ok(true)) {
                    break;
                }
            }
            let _ = this.update(cx, |client, cx| {
                if let Some(host) = *message_route.read() {
                    client.handle_host_disconnected(host, cx);
                }
            });
        })
        .detach();
        Ok(Self {
            client: Some(client),
            #[cfg(test)]
            fake_client: None,
            reader_thread: Some(reader_thread),
            resync_pending: false,
            full_requests_pending: BTreeSet::new(),
            history_requests_pending: BTreeMap::new(),
            history_backfill_deferred: BTreeMap::new(),
            snapshot: None,
            appearance,
            state: HostState::Connected,
            route,
            last_attached_session: None,
            reconnect_generation: 0,
            reconnect_attempt_in_flight: None,
            reconnect_attach: None,
            in_flight_commands: RwLock::new(VecDeque::new()),
            ssh_auth_declined: false,
        })
    }

    fn track_command(&self, request_id: u64, name: String) {
        let mut in_flight = self.in_flight_commands.write();
        while in_flight.len() >= MAX_TRACKED_COMMANDS {
            in_flight.pop_front();
        }
        in_flight.push_back((request_id, name));
    }

    fn take_command(&self, request_id: u64) -> Option<String> {
        let mut in_flight = self.in_flight_commands.write();
        let index = in_flight.iter().position(|(id, _)| *id == request_id)?;
        in_flight.remove(index).map(|(_, name)| name)
    }

    fn reroute(&self, host: HostId) {
        *self.route.write() = Some(host);
    }

    fn clear_route(&mut self) {
        self.bump_reconnect_generation();
        self.reconnect_attempt_in_flight = None;
        self.reconnect_attach = None;
        *self.route.write() = None;
    }

    fn bump_reconnect_generation(&mut self) -> u64 {
        self.reconnect_generation = self.reconnect_generation.wrapping_add(1).max(1);
        self.reconnect_generation
    }

    fn is_connected(&self) -> bool {
        let has_client = self.client.is_some();
        #[cfg(test)]
        let has_client = has_client || self.fake_client.is_some();
        debug_assert_eq!(self.state == HostState::Connected, has_client);
        debug_assert_eq!(self.reader_thread.is_some(), self.client.is_some());
        has_client
    }

    fn reconnect_active(&self) -> bool {
        matches!(self.state, HostState::Reconnecting { .. })
            || self.reconnect_attempt_in_flight.is_some()
            || self.reconnect_attach.is_some()
    }

    fn current_hello(&self) -> Option<ServerHello> {
        let hello = self
            .client
            .as_ref()
            .map(|client| client.server_hello().clone());
        #[cfg(test)]
        let hello = hello.or_else(|| self.fake_client.as_ref().map(|client| client.hello.clone()));
        let mut hello = hello?;
        if let Some((appearance, provenance)) = &self.appearance {
            hello.appearance.clone_from(appearance);
            hello.appearance_provenance.clone_from(provenance);
        }
        Some(hello)
    }

    #[cfg(feature = "agent-pane")]
    pub(crate) fn client_instance_id(&self) -> Option<zz_protocol::ClientInstanceId> {
        self.current_hello().map(|hello| hello.client_instance_id)
    }
}

pub struct MuxClient {
    /// Protocol reduction for the attached connection: snapshot, options, key
    /// tables, status, prefix arming, attachment and the overlay models. It
    /// outlives connection churn on purpose — a reconnecting host keeps
    /// rendering its last frame until the new `ServerHello` resets the core.
    core: ClientCore,
    registry: HostRegistry,
    connections: HashMap<HostId, HostConnection>,
    attached_host: HostId,
    color_scheme: TerminalColorScheme,
    appearance: Arc<TerminalAppearance>,
    status_revision: u64,
    terminal_font_size_offset_points: f32,
    #[cfg(test)]
    input_sink: Option<std::rc::Rc<std::cell::RefCell<Vec<InputMessage>>>>,
    #[cfg(all(test, feature = "agent-pane"))]
    agent_sink: Option<std::rc::Rc<std::cell::RefCell<Vec<(PaneId, AgentRequest)>>>>,
    #[cfg(all(test, feature = "agent-pane"))]
    agent_client_instance_id: Option<zz_protocol::ClientInstanceId>,
    attached_snapshot_pending: bool,
    viewports: BTreeMap<PaneId, Arc<RwLock<RetainedTerminalViewport>>>,
    kitty_images: BTreeMap<PaneId, Arc<RwLock<KittyImageCache>>>,
    kitty_image_assemblies: BTreeMap<(PaneId, u32, u64), KittyImageAssembly>,
    browser_commands: BTreeMap<PaneId, Vec<BrowserCommand>>,
    agent_commands: BTreeMap<PaneId, Vec<(u64, AgentCommand)>>,
    /// Per-pane replay cursor: the highest agent stream seq handed to the
    /// shell. A replay overlaps the live tail on purpose, so this is what makes
    /// the stream idempotent.
    #[cfg(feature = "agent-pane")]
    agent_cursors: BTreeMap<PaneId, u64>,
    /// Panes with a replay asked for and not yet landed. A hole is re-requested
    /// once rather than once per batch: every batch that arrives across the
    /// hole before the replay lands is another sighting of the same gap.
    #[cfg(feature = "agent-pane")]
    agent_replays_pending: BTreeSet<PaneId>,
    #[cfg(feature = "agent-pane")]
    agent_events: AgentEvents,
    screenshot_requests: Vec<(PaneId, u64, String)>,
    terminal_commands: BTreeMap<PaneId, Vec<TerminalUiCommand>>,
    pane_images: BTreeMap<PaneId, PaneImageSnapshots>,
    pasted_image_assemblies: BTreeMap<(PaneId, u32), PastedImageAssembly>,
    pending_pasted_image_previews: BTreeSet<(PaneId, u32)>,
    pending_commands_revision: u64,
    command_output: Option<CommandOutputModel>,
    command_prompt_revision: u64,
    choose_tree_revision: u64,
    choose_buffer_revision: u64,
    display_panes_revision: u64,
    sidebar_focus_revision: u64,
    bell_revision: u64,
    error: Option<Arc<str>>,
    stale_daemon: Option<StaleDaemonInfo>,
    error_after_next_attach: Option<Arc<str>>,
    shutting_down: bool,
    next_row_revision: u64,
    command_output_diff: TerminalDiffScratch,
}

impl MuxClient {
    pub(crate) fn new(
        client: Result<InteractiveClient, DaemonError>,
        local_socket_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let color_scheme = client.as_ref().map_or(TerminalColorScheme::Dark, |client| {
            client.server_hello().appearance.color_scheme
        });
        Self::new_inner(client, local_socket_path, color_scheme, cx)
    }

    pub fn new_with_color_scheme(
        client: Result<InteractiveClient, DaemonError>,
        local_socket_path: PathBuf,
        color_scheme: TerminalColorScheme,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut state = Self::new(client, local_socket_path, cx);
        state.color_scheme = color_scheme;
        state
    }

    fn new_inner(
        client: Result<InteractiveClient, DaemonError>,
        local_socket_path: PathBuf,
        color_scheme: TerminalColorScheme,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_global::<crate::config::FleetHosts>(|client, cx| {
            client.reconcile_hosts(cx);
        })
        .detach();
        let local_host_policy = crate::profile::profile(cx).local_host;
        let registry = HostRegistry::new(
            local_socket_path,
            &crate::config::fleet_hosts(cx),
            local_host_policy,
        );
        let has_local = registry.get(HostId::LOCAL).is_some();
        let mut connections: HashMap<HostId, HostConnection> = registry
            .iter()
            .map(|(host, _)| (host, HostConnection::disconnected(host)))
            .collect();
        if !has_local {
            connections.insert(HostId::LOCAL, HostConnection::disconnected(HostId::LOCAL));
        }
        let mut state = Self {
            core: ClientCore::new(),
            registry,
            connections,
            attached_host: HostId::LOCAL,
            color_scheme,
            appearance: Arc::new(TerminalAppearance::default()),
            status_revision: 0,
            terminal_font_size_offset_points: 0.0,
            #[cfg(test)]
            input_sink: None,
            #[cfg(all(test, feature = "agent-pane"))]
            agent_sink: None,
            #[cfg(all(test, feature = "agent-pane"))]
            agent_client_instance_id: None,
            attached_snapshot_pending: false,
            viewports: BTreeMap::new(),
            kitty_images: BTreeMap::new(),
            kitty_image_assemblies: BTreeMap::new(),
            browser_commands: BTreeMap::new(),
            agent_commands: BTreeMap::new(),
            #[cfg(feature = "agent-pane")]
            agent_cursors: BTreeMap::new(),
            #[cfg(feature = "agent-pane")]
            agent_replays_pending: BTreeSet::new(),
            #[cfg(feature = "agent-pane")]
            agent_events: AgentEvents::default(),
            screenshot_requests: Vec::new(),
            terminal_commands: BTreeMap::new(),
            pane_images: BTreeMap::new(),
            pasted_image_assemblies: BTreeMap::new(),
            pending_pasted_image_previews: BTreeSet::new(),
            pending_commands_revision: 0,
            command_output: None,
            command_prompt_revision: 0,
            choose_tree_revision: 0,
            choose_buffer_revision: 0,
            display_panes_revision: 0,
            sidebar_focus_revision: 0,
            bell_revision: 0,
            error: None,
            stale_daemon: None,
            error_after_next_attach: None,
            shutting_down: false,
            next_row_revision: 1,
            command_output_diff: TerminalDiffScratch::default(),
        };
        if has_local {
            match client {
                Ok(client) => {
                    if let Err(error) = state.install_initial_local_connection(client, cx) {
                        state.error = Some(Arc::from(error));
                        return state;
                    }
                }
                Err(error) => {
                    if let DaemonError::IncompatibleDaemon { daemon, .. } = &error {
                        state.stale_daemon = Some(StaleDaemonInfo { daemon: *daemon });
                    }
                    state.error = Some(Arc::from(error.to_string()));
                }
            }
        } else if let Ok(client) = client {
            let _ = client.detach();
        }
        if !has_local
            || state
                .connections
                .get(&HostId::LOCAL)
                .is_some_and(HostConnection::is_connected)
        {
            state.ensure_all_connected(cx);
        }
        state
    }

    fn install_initial_local_connection(
        &mut self,
        client: InteractiveClient,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.ingest_server_hello(client.server_hello().clone(), cx);
        let client = Arc::new(client);
        crate::config::register_config_override_client(&client, false, cx);
        client.attach("").map_err(|error| error.to_string())?;
        let connection = HostConnection::connected(client, HostId::LOCAL, cx)
            .map_err(|error| error.to_string())?;
        self.connections.insert(HostId::LOCAL, connection);
        Ok(())
    }

    /// Adopt a connection's handshake as the attached client's truth: options,
    /// key tables and status land in the core, the localized appearance stays
    /// here because font resolution needs the GPUI text system.
    ///
    /// Settings only — a reconnect ingests the new hello while the old frame is
    /// still on screen, and must not blank it. Clearing the attachment is
    /// `clear_cross_host_state`'s job, and only a host switch does it.
    fn ingest_server_hello(&mut self, hello: ServerHello, cx: &mut Context<Self>) {
        self.core.adopt_hello(hello);
        let provenance = self.core.appearance_provenance().clone();
        let mut appearance = self.core.appearance().cloned().unwrap_or_default();
        let requested_primary_font = appearance.font_families.first().cloned();
        localize_terminal_font_families(
            &mut appearance,
            &provenance,
            &cx.text_system().all_font_names(),
        );
        self.terminal_font_size_offset_points =
            apply_terminal_font_size_offset(&mut appearance, self.terminal_font_size_offset_points);
        self.appearance = Arc::new(appearance);
        log::info!(
            target: "zz::diagnostics::appearance",
            "resolved appearance hash={} requested_primary_font={requested_primary_font:?} primary_font={:?} fallback_count={} feature_count={} font_size_points={} padding_left={} padding_right={} padding_top={} padding_bottom={} minimum_contrast={} background_opacity={} blink_policy={:?} blink_interval_ms={}",
            self.appearance.stable_hash(),
            self.appearance.font_families.first(),
            self.appearance.font_families.len().saturating_sub(1),
            self.appearance.font_features.len(),
            self.appearance.font_size_points,
            self.appearance.padding_left,
            self.appearance.padding_right,
            self.appearance.padding_top,
            self.appearance.padding_bottom,
            self.appearance.minimum_contrast,
            self.appearance.background_opacity,
            self.appearance.cursor_blink_policy,
            self.appearance.cursor_blink_interval_ms,
        );
        crate::theme::set_terminal_appearance(Arc::clone(&self.appearance), cx);
    }

    /// Drain the core's queues without acting on them, for state a test stands
    /// up directly. Left queued, they would replay against the next message.
    #[cfg(test)]
    fn discard_core_effects(&mut self) {
        while self.core.poll_outbound().is_some() {}
        while self.core.poll_event().is_some() {}
    }

    fn attached_connection(&self) -> &HostConnection {
        debug_assert!(self.connections.contains_key(&self.attached_host));
        &self.connections[&self.attached_host]
    }

    fn attached_connection_mut(&mut self) -> &mut HostConnection {
        let attached_host = self.attached_host;
        debug_assert!(self.connections.contains_key(&attached_host));
        self.connections.get_mut(&attached_host).unwrap()
    }

    /// The attached ssh host and the loopback SOCKS port its forward opened, or
    /// `None` when the attached host is local (or has no live port).
    #[cfg_attr(target_os = "ios", allow(dead_code))]
    pub(crate) fn attached_ssh_egress(&self) -> Option<(String, u16)> {
        let entry = self.registry.get(self.attached_host)?;
        let Endpoint::Ssh(endpoint) = &entry.endpoint else {
            return None;
        };
        let port = self.attached_connection().client.as_ref()?.socks_port()?;
        Some((ssh_destination_label(endpoint), port))
    }

    fn history_trickle_budget(&self) -> usize {
        self.core
            .mux_options()
            .get(MuxOptionKey::HistoryTrickle)
            .and_then(|option| option.value.parse::<usize>().ok())
            .unwrap_or_default()
            .min(MAX_HISTORY_ROWS)
    }

    fn request_history_backfill(&mut self, pane: PaneId) {
        self.request_history(pane, None);
    }

    fn defer_history_backfill(
        &mut self,
        pane: PaneId,
        history_mutations: u64,
        cx: &mut Context<Self>,
    ) {
        let host = self.attached_host;
        let should_arm = self
            .attached_connection_mut()
            .history_backfill_deferred
            .insert(pane, history_mutations)
            .is_none();
        if !should_arm {
            return;
        }

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(HISTORY_BACKFILL_QUIET).await;
                let rearm = this.update(cx, |client, _cx| {
                    if client.attached_host != host {
                        return false;
                    }
                    let Some(recorded_mutations) = client
                        .connections
                        .get(&host)
                        .and_then(|connection| connection.history_backfill_deferred.get(&pane))
                        .copied()
                    else {
                        return false;
                    };
                    let Some(current_mutations) = client
                        .viewports
                        .get(&pane)
                        .map(|retained| retained.read().history_mutations)
                    else {
                        return false;
                    };

                    if current_mutations == recorded_mutations {
                        let Some(connection) = client.connections.get_mut(&host) else {
                            return false;
                        };
                        connection.history_backfill_deferred.remove(&pane);
                        client.request_history_backfill(pane);
                        false
                    } else {
                        let Some(recorded_mutations) =
                            client.connections.get_mut(&host).and_then(|connection| {
                                connection.history_backfill_deferred.get_mut(&pane)
                            })
                        else {
                            return false;
                        };
                        *recorded_mutations = current_mutations;
                        true
                    }
                });
                if !matches!(rearm, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn request_history_prefetch(&mut self, pane: PaneId, target_offset: u32) {
        self.request_history(pane, Some(target_offset));
    }

    fn request_history(&mut self, pane: PaneId, prefetch_target: Option<u32>) {
        if prefetch_target.is_none()
            && self
                .attached_connection()
                .history_backfill_deferred
                .contains_key(&pane)
        {
            return;
        }
        if self
            .attached_connection()
            .history_requests_pending
            .contains_key(&pane)
        {
            if let Some(target) = prefetch_target {
                let pending = self
                    .attached_connection_mut()
                    .history_requests_pending
                    .get_mut(&pane)
                    .expect("pending history request was checked above");
                pending.prefetch_target = Some(
                    pending
                        .prefetch_target
                        .map_or(target, |previous| previous.min(target)),
                );
            }
            return;
        }

        let budget =
            prefetch_target.map_or_else(|| self.history_trickle_budget(), |_| MAX_HISTORY_ROWS);
        if budget == 0 {
            return;
        }
        let Some(retained) = self.viewports.get(&pane).cloned() else {
            return;
        };
        let retained = retained.read();
        let retained_rows = retained.history.len();
        if retained_rows >= budget {
            return;
        }
        let Ok(retained_rows_u32) = u32::try_from(retained_rows) else {
            return;
        };
        let Some(front) = retained
            .history_scrollbar
            .offset
            .checked_sub(retained_rows_u32)
        else {
            return;
        };
        let desired = prefetch_target.map_or(front, |target| {
            front.saturating_sub(target.saturating_sub(retained.viewport.scrollbar.len))
        });
        let count = desired
            .min(MAX_HISTORY_CHUNK_ROWS)
            .min(u32::try_from(budget - retained_rows).unwrap_or(u32::MAX));
        if count == 0 {
            return;
        }
        let start = front - count;
        let mutations = retained.history_mutations;
        drop(retained);

        let connection = self.attached_connection_mut();
        connection.history_requests_pending.insert(
            pane,
            PendingHistoryRequest {
                mutations,
                prefetch_target,
            },
        );
        if let Some(client) = &connection.client {
            if let Err(error) = client.request_history(pane, start, count) {
                connection.history_requests_pending.remove(&pane);
                connection.history_backfill_deferred.remove(&pane);
                log::warn!("failed to request terminal history for {pane}: {error}");
            }
            return;
        }
        #[cfg(test)]
        if let Some(client) = &connection.fake_client {
            client.request_history(pane, start, count);
            return;
        }
        connection.history_requests_pending.remove(&pane);
        connection.history_backfill_deferred.remove(&pane);
    }

    fn reconcile_hosts(&mut self, cx: &mut Context<Self>) {
        let configured = crate::config::fleet_hosts(cx);
        let has_retained = self
            .registry
            .iter()
            .any(|(host, _)| self.registry.is_retained(host));
        if self.registry.configured() == configured.as_slice()
            && (!has_retained || self.registry.is_retained(self.attached_host))
        {
            return;
        }

        let old_attached_host = self.attached_host;
        let attached_entry = self.registry.get(old_attached_host).cloned();
        let local_socket_path = self.registry.local_socket_path().to_path_buf();
        let mut next_registry = HostRegistry::new(
            local_socket_path,
            &configured,
            crate::profile::profile(cx).local_host,
        );
        let attached_remote_vanished = old_attached_host != HostId::LOCAL
            && attached_entry
                .as_ref()
                .is_some_and(|attached| !configured.iter().any(|host| host.name == attached.name));
        let removed_active_reconnect = attached_remote_vanished
            && self
                .connections
                .get(&old_attached_host)
                .is_some_and(HostConnection::reconnect_active);
        if attached_remote_vanished
            && !removed_active_reconnect
            && let Some(attached) = &attached_entry
        {
            log::warn!(
                "attached fleet host {} was removed from configuration; keeping its live connection until detach",
                attached.name,
            );
            next_registry.push_retained(attached.clone());
        }

        let old_registry = std::mem::replace(&mut self.registry, next_registry);
        let mut old_connections = std::mem::take(&mut self.connections);
        let mut next_connections = HashMap::with_capacity(self.registry.iter().count());
        for (host, entry) in self.registry.iter() {
            let connection = if host == HostId::LOCAL {
                old_connections
                    .remove(&HostId::LOCAL)
                    .unwrap_or_else(|| HostConnection::disconnected(host))
            } else if let Some((old_host, old_entry)) = old_registry.get_by_name(&entry.name)
                && old_entry == entry
            {
                old_connections
                    .remove(&old_host)
                    .unwrap_or_else(|| HostConnection::disconnected(host))
            } else {
                HostConnection::disconnected(host)
            };
            connection.reroute(host);
            next_connections.insert(host, connection);
        }
        if self.registry.get(HostId::LOCAL).is_none() {
            let connection = old_connections
                .remove(&HostId::LOCAL)
                .unwrap_or_else(|| HostConnection::disconnected(HostId::LOCAL));
            next_connections.insert(HostId::LOCAL, connection);
        }
        for connection in old_connections.values_mut() {
            connection.clear_route();
        }
        self.connections = next_connections;
        self.attached_host = if old_attached_host == HostId::LOCAL {
            HostId::LOCAL
        } else {
            attached_entry
                .as_ref()
                .and_then(|attached| self.registry.get_by_name(&attached.name))
                .map_or(HostId::LOCAL, |(host, _)| host)
        };
        if removed_active_reconnect {
            self.give_up_removed_attached_host(
                attached_entry.as_ref().map_or("remote", |host| &host.name),
                cx,
            );
        }
        cx.notify();
        self.ensure_all_connected(cx);
    }

    pub(crate) fn host_states<'a>(
        &'a mut self,
        cx: &mut Context<Self>,
    ) -> impl Iterator<Item = (HostId, &'a str, &'a HostState)> + 'a {
        self.reconcile_hosts(cx);
        let connections = &self.connections;
        self.registry.iter().map(move |(host, entry)| {
            let state = &connections
                .get(&host)
                .expect("every registered host has a connection slot")
                .state;
            (host, entry.name.as_str(), state)
        })
    }

    pub const fn attached_host(&self) -> HostId {
        self.attached_host
    }

    #[cfg(feature = "agent-pane")]
    pub(crate) fn client_instance_id(&self) -> Option<zz_protocol::ClientInstanceId> {
        #[cfg(test)]
        if self.agent_client_instance_id.is_some() {
            return self.agent_client_instance_id;
        }
        self.attached_connection().client_instance_id()
    }

    #[must_use]
    pub fn first_host(&self) -> Option<HostId> {
        self.registry.iter().next().map(|(host, _)| host)
    }

    #[must_use]
    pub fn has_hosts(&self) -> bool {
        self.first_host().is_some()
    }

    pub fn fleet_hosts(
        &self,
    ) -> impl Iterator<Item = (HostId, &str, &HostState, Option<&MuxSnapshot>)> {
        let connections = &self.connections;
        let attached_host = self.attached_host;
        let attached_snapshot =
            (!self.attached_snapshot_pending).then(|| self.core.snapshot().as_ref());
        self.registry.iter().map(move |(host, entry)| {
            let connection = connections
                .get(&host)
                .expect("every registered host has a connection slot");
            (
                host,
                entry.name.as_str(),
                &connection.state,
                match attached_snapshot {
                    Some(snapshot) if host == attached_host => Some(snapshot),
                    _ => connection.snapshot.as_deref(),
                },
            )
        })
    }

    pub(crate) fn ensure_connected(&mut self, host: HostId, cx: &mut Context<Self>) {
        let requested_name = self.registry.get(host).map(|entry| entry.name.clone());
        self.reconcile_hosts(cx);
        let Some((host, endpoint)) = requested_name
            .as_deref()
            .and_then(|name| self.registry.get_by_name(name))
            .map(|(host, entry)| (host, entry.endpoint.clone()))
        else {
            log::warn!("cannot connect unknown fleet host {host:?}");
            return;
        };
        let connection = self
            .connections
            .get_mut(&host)
            .expect("every registered host has a connection slot");
        if matches!(
            connection.state,
            HostState::Connecting | HostState::Connected
        ) {
            return;
        }
        if connection.ssh_auth_declined {
            return;
        }
        let reconnect_attempt = match connection.state {
            HostState::Reconnecting { attempt } => {
                connection.bump_reconnect_generation();
                Some(attempt)
            }
            _ => None,
        };
        connection.client = None;
        connection.reader_thread = None;
        connection.resync_pending = false;
        connection.full_requests_pending.clear();
        connection.history_requests_pending.clear();
        connection.history_backfill_deferred.clear();
        if host != HostId::LOCAL {
            connection.snapshot = None;
        }
        connection.state = HostState::Connecting;
        connection.reconnect_attempt_in_flight = reconnect_attempt;
        connection.reconnect_attach = None;
        let result_route = Arc::clone(&connection.route);
        cx.notify();

        let color_scheme = self.color_scheme;
        let prompts = self.ssh_prompts(host, &endpoint, cx);
        let (results, incoming) = async_channel::bounded(1);
        let connect_thread = thread::Builder::new()
            .name("zz-host-connect".to_owned())
            .spawn(move || {
                let result = InteractiveClient::connect_endpoint_with_prompts(
                    &endpoint,
                    color_scheme,
                    prompts,
                );
                let _ = results.send_blocking(result);
            });
        if let Err(error) = connect_thread {
            self.handle_connect_result(
                host,
                Err(DaemonError::Thread(format!(
                    "failed to start host connection thread: {error}"
                ))),
                cx,
            );
            return;
        }
        cx.spawn(async move |this, cx| {
            let Ok(result) = incoming.recv().await else {
                return;
            };
            let _ = this.update(cx, |client, cx| {
                if let Some(host) = *result_route.read() {
                    client.handle_connect_result(host, result, cx);
                }
            });
        })
        .detach();
    }

    /// Retry every host the backoff ladder has waiting. iOS calls this on
    /// foregrounding, where suspension froze the ladder's timers.
    #[cfg(target_os = "ios")]
    pub fn retry_stalled_hosts(&mut self, cx: &mut Context<Self>) {
        let stalled: Vec<HostId> = self
            .connections
            .iter()
            .filter(|(_, connection)| {
                matches!(
                    connection.state,
                    HostState::Reconnecting { .. } | HostState::Unreachable { .. }
                )
            })
            .map(|(host, _)| *host)
            .collect();
        for host in stalled {
            self.retry_host_now(host, cx);
        }
    }

    pub fn retry_host_now(&mut self, host: HostId, cx: &mut Context<Self>) {
        if let Some(connection) = self.connections.get_mut(&host) {
            connection.ssh_auth_declined = false;
            if matches!(connection.state, HostState::Unreachable { .. }) {
                connection.state = HostState::Reconnecting { attempt: 1 };
            }
        }
        self.ensure_connected(host, cx);
    }

    pub(crate) fn note_ssh_auth_declined(&mut self, host: HostId, cx: &mut Context<Self>) {
        let Some(connection) = self.connections.get_mut(&host) else {
            return;
        };
        connection.ssh_auth_declined = true;
        cx.notify();
    }

    fn ssh_prompts(
        &self,
        host: HostId,
        endpoint: &Endpoint,
        cx: &mut Context<Self>,
    ) -> Option<zz_daemon::SshPrompts> {
        let Endpoint::Ssh(ssh) = endpoint else {
            return None;
        };
        let helper = std::env::current_exe().ok()?;
        let label = ssh_destination_label(ssh);
        let route = Arc::clone(&self.connections.get(&host)?.route);

        let (requests, incoming) =
            async_channel::bounded::<(AskpassPrompt, async_channel::Sender<AskpassReply>)>(1);
        cx.spawn(async move |this, cx| {
            while let Ok((prompt, reply)) = incoming.recv().await {
                let emitted = this.update(cx, |_, cx| {
                    let Some(host) = *route.read() else {
                        return false;
                    };
                    cx.emit(SshPromptRequest {
                        host,
                        label: label.clone(),
                        prompt,
                        reply,
                    });
                    true
                });
                if !matches!(emitted, Ok(true)) {
                    break;
                }
            }
        })
        .detach();

        Some(zz_daemon::SshPrompts::new(
            helper,
            move |prompt: &AskpassPrompt| {
                let (reply, answers) = async_channel::bounded(1);
                if requests.send_blocking((prompt.clone(), reply)).is_err() {
                    return AskpassReply::Cancel;
                }
                answers.recv_blocking().unwrap_or(AskpassReply::Cancel)
            },
        ))
    }

    pub(crate) fn ensure_all_connected(&mut self, cx: &mut Context<Self>) {
        let hosts = self
            .host_states(cx)
            .filter_map(|(host, _, state)| {
                (host != HostId::LOCAL && !matches!(state, HostState::Reconnecting { .. }))
                    .then_some(host)
            })
            .collect::<Vec<_>>();
        for host in hosts {
            self.ensure_connected(host, cx);
        }
    }

    fn arm_reconnect(&mut self, host: HostId, attempt: u32, cx: &mut Context<Self>) {
        let Some(connection) = self.connections.get_mut(&host) else {
            return;
        };
        if connection.state != (HostState::Reconnecting { attempt }) {
            return;
        }

        let generation = connection.bump_reconnect_generation();
        let route = Arc::clone(&connection.route);
        let delay = reconnect_delay(attempt);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |client, cx| {
                let routed_host = *route.read();
                let Some(host) = routed_host else {
                    return;
                };
                let Some(connection) = client.connections.get(&host) else {
                    return;
                };
                if !reconnect_timer_is_current(connection, generation, attempt) {
                    return;
                }
                client.ensure_connected(host, cx);
            });
        })
        .detach();
    }

    fn cancel_reconnect(&mut self, host: HostId) {
        let Some(connection) = self.connections.get_mut(&host) else {
            return;
        };
        if !connection.reconnect_active() {
            return;
        }
        connection.bump_reconnect_generation();
        connection.reconnect_attempt_in_flight = None;
        connection.reconnect_attach = None;
        if !connection.is_connected() {
            connection.state = HostState::Unreachable {
                reason: "connection lost".to_owned(),
            };
        }
    }

    fn handle_connect_result(
        &mut self,
        host: HostId,
        result: Result<InteractiveClient, DaemonError>,
        cx: &mut Context<Self>,
    ) {
        let Some(connection) = self.connections.get(&host) else {
            log::debug!("discarding connection result for removed fleet host {host:?}");
            return;
        };
        if connection.state != HostState::Connecting {
            log::debug!("discarding stale connection result for fleet host {host:?}");
            return;
        }

        let route = Arc::clone(&connection.route);
        let last_attached_session = connection.last_attached_session;
        let snapshot = connection.snapshot.as_ref().map(Arc::clone);
        let reconnect_generation = connection.reconnect_generation;
        let reconnect_attempt = connection.reconnect_attempt_in_flight;
        match result {
            Ok(client) => {
                if let Err(error) = client.request_resync() {
                    log::warn!("failed to request initial fleet snapshot: {error}");
                }
                match HostConnection::connected_with_route(Arc::new(client), Arc::clone(&route), cx)
                {
                    Ok(mut connection) => {
                        connection.last_attached_session = last_attached_session;
                        connection.snapshot = snapshot;
                        connection.reconnect_generation = reconnect_generation;
                        self.connections.insert(host, connection);
                        if reconnect_attempt.is_some() && self.attached_host == host {
                            self.reattach_after_reconnect(host, cx);
                        }
                        cx.notify();
                    }
                    Err(error) => {
                        self.finish_connect_failure(
                            host,
                            HostState::Unreachable {
                                reason: error.to_string(),
                            },
                            route,
                            last_attached_session,
                            snapshot,
                            reconnect_generation,
                            reconnect_attempt,
                            cx,
                        );
                    }
                }
            }
            Err(error) => {
                let state = connect_error_state(&error);
                log::warn!(
                    "fleet host {name} is {label}: {error}",
                    name = self
                        .registry
                        .get(host)
                        .map_or("<unknown>", |entry| entry.name.as_str()),
                    label = state.label(),
                );
                self.finish_connect_failure(
                    host,
                    state,
                    route,
                    last_attached_session,
                    snapshot,
                    reconnect_generation,
                    reconnect_attempt,
                    cx,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_connect_failure(
        &mut self,
        host: HostId,
        state: HostState,
        route: Arc<RwLock<Option<HostId>>>,
        last_attached_session: Option<SessionId>,
        snapshot: Option<Arc<MuxSnapshot>>,
        reconnect_generation: u64,
        reconnect_attempt: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        let declined = self
            .connections
            .get(&host)
            .is_some_and(|connection| connection.ssh_auth_declined);
        let mut connection = HostConnection::disconnected_with_route(route);
        connection.last_attached_session = last_attached_session;
        connection.snapshot = snapshot;
        connection.reconnect_generation = reconnect_generation;
        connection.ssh_auth_declined = declined;
        let next_attempt = reconnect_attempt.filter(|_| !declined).and_then(|attempt| {
            (self.attached_host == host || attempt < MAX_UNATTACHED_RECONNECT_ATTEMPTS)
                .then(|| attempt.saturating_add(1))
        });
        if declined {
            connection.state = HostState::Unreachable {
                reason: AUTH_DECLINED_REASON.to_owned(),
            };
        } else if let Some(attempt) = next_attempt {
            connection.state = HostState::Reconnecting { attempt };
        } else {
            connection.state = state;
        }
        self.connections.insert(host, connection);
        if let Some(attempt) = next_attempt {
            self.arm_reconnect(host, attempt, cx);
        }
        cx.notify();
    }

    fn reattach_after_reconnect(&mut self, host: HostId, cx: &mut Context<Self>) {
        if host != self.attached_host {
            return;
        }
        let Some(connection) = self.connections.get(&host) else {
            return;
        };
        let client = connection.client.as_ref().map(Arc::clone);
        #[cfg(test)]
        let fake_client = connection.fake_client.as_ref().map(Arc::clone);
        let hello = connection
            .current_hello()
            .expect("a connected host has an interactive client");
        let session = connection.last_attached_session;

        self.ingest_server_hello(hello, cx);
        self.status_revision = self.status_revision.saturating_add(1);
        self.error = None;
        self.error_after_next_attach = None;
        self.connections
            .get_mut(&host)
            .expect("reconnected host still has a connection slot")
            .reconnect_attach = Some(if session.is_some() {
            ReconnectAttachState::RememberedSession
        } else {
            ReconnectAttachState::DefaultSession
        });
        let result = if let Some(client) = client {
            crate::config::register_config_override_client(&client, true, cx);
            client.attach(session.map_or_else(String::new, |session| session.to_string()))
        } else {
            #[cfg(test)]
            {
                fake_client
                    .expect("a connected test host has a fake client")
                    .attach(session)
            }
            #[cfg(not(test))]
            unreachable!("a connected host has an interactive client");
        };
        if let Err(error) = result {
            self.connections
                .get_mut(&host)
                .expect("reconnected host still has a connection slot")
                .reconnect_attach = None;
            log::warn!("failed to re-attach after reconnect: {error}");
        }
    }

    fn retry_default_after_missing_session(&mut self) -> bool {
        let host = self.attached_host;
        let Some(connection) = self.connections.get(&host) else {
            return false;
        };
        if connection.reconnect_attach != Some(ReconnectAttachState::RememberedSession) {
            return false;
        }
        let client = connection.client.as_ref().map(Arc::clone);
        #[cfg(test)]
        let fake_client = connection.fake_client.as_ref().map(Arc::clone);
        self.connections
            .get_mut(&host)
            .expect("attached host still has a connection slot")
            .reconnect_attach = Some(ReconnectAttachState::DefaultSession);
        self.error = None;
        let result = if let Some(client) = client {
            client.attach("")
        } else {
            #[cfg(test)]
            {
                fake_client
                    .expect("a connected test host has a fake client")
                    .attach(None)
            }
            #[cfg(not(test))]
            unreachable!("a connected host has an interactive client");
        };
        if let Err(error) = result {
            log::warn!("failed to fall back to the default session after reconnect: {error}");
            if let Some(connection) = self.connections.get_mut(&host) {
                connection.reconnect_attach = None;
            }
        }
        true
    }

    fn handle_host_disconnected(&mut self, host: HostId, cx: &mut Context<Self>) {
        let Some(connection) = self.connections.get(&host) else {
            log::debug!("discarding disconnect for removed fleet host {host:?}");
            return;
        };
        let route = Arc::clone(&connection.route);
        let name = self
            .registry
            .get(host)
            .map_or_else(|| format!("{host:?}"), |entry| entry.name.clone());
        let was_attached = host == self.attached_host;
        let last_attached_session = if was_attached {
            self.core
                .attached_session()
                .or(connection.last_attached_session)
        } else {
            connection.last_attached_session
        };
        let reconnect_generation = connection.reconnect_generation;
        let mut disconnected = HostConnection::disconnected_with_route(route);
        disconnected.last_attached_session = last_attached_session;
        disconnected.reconnect_generation = reconnect_generation;

        if self.shutting_down {
            disconnected.state = HostState::Unreachable {
                reason: "connection lost".to_owned(),
            };
            self.connections.insert(host, disconnected);
            cx.notify();
            return;
        }

        if host == HostId::LOCAL {
            disconnected.snapshot = connection.snapshot.as_ref().map(Arc::clone);
        }

        disconnected.state = HostState::Reconnecting { attempt: 1 };
        self.connections.insert(host, disconnected);
        if was_attached {
            self.error_after_next_attach = None;
            self.reset_session_state(cx);
            if host == HostId::LOCAL {
                self.error = Some(Arc::from(format!("fleet host {name} disconnected")));
            } else {
                self.error = None;
                Self::emit_notification(
                    ClientMessageKind::Warning,
                    format!("connection to {name} lost: reconnecting…"),
                    cx,
                );
            }
        }
        self.arm_reconnect(host, 1, cx);
        cx.notify();
    }

    #[must_use]
    pub fn snapshot(&self) -> Arc<MuxSnapshot> {
        Arc::clone(self.core.snapshot())
    }

    /// The rendered tmux status line for this client, expanded by the daemon.
    #[must_use]
    pub(crate) const fn status(&self) -> &StatusLine {
        self.core.status()
    }

    /// Bumped on every status change, so a view can observe the status without
    /// comparing its text.
    #[must_use]
    pub(crate) const fn status_revision(&self) -> u64 {
        self.status_revision
    }

    /// The daemon-published prefix in the form a keystroke is compared
    /// against, or `None` while disconnected.
    #[must_use]
    pub(crate) fn canonical_prefix(&self) -> Option<String> {
        self.attached_connection().client.as_ref()?;
        self.core
            .mux_options()
            .get(MuxOptionKey::Prefix)
            .map(|option| zz_protocol::canonical_key(&option.value))
    }

    /// Whether the daemon reported this client's prefix sequence as armed.
    #[must_use]
    pub(crate) const fn prefix_armed(&self) -> bool {
        self.core.prefix_armed()
    }

    /// The daemon-published prefix key table, or empty while disconnected.
    /// Keys are tmux-grammar strings; commands carry canonical names.
    #[must_use]
    pub(crate) fn prefix_bindings(&self) -> &[KeyBindingSnapshot] {
        if self.attached_connection().client.is_none() {
            return &[];
        }
        self.core.prefix_bindings()
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.attached_connection().is_connected()
    }

    /// Collect what the views send to panes, so a test can assert delivery.
    #[cfg(test)]
    pub(crate) fn record_input_for_test(
        &mut self,
    ) -> std::rc::Rc<std::cell::RefCell<Vec<InputMessage>>> {
        let sink = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        self.input_sink = Some(std::rc::Rc::clone(&sink));
        sink
    }

    /// Collect what the agent controller sends, so a test can assert the shape
    /// that reaches the wire.
    #[cfg(all(test, feature = "agent-pane"))]
    pub(crate) fn record_agent_requests_for_test(
        &mut self,
    ) -> std::rc::Rc<std::cell::RefCell<Vec<(PaneId, AgentRequest)>>> {
        let sink = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        self.agent_sink = Some(std::rc::Rc::clone(&sink));
        sink
    }

    #[cfg(all(test, feature = "agent-pane"))]
    pub(crate) fn set_agent_client_instance_id_for_test(
        &mut self,
        client_instance_id: zz_protocol::ClientInstanceId,
    ) {
        self.agent_client_instance_id = Some(client_instance_id);
    }

    /// Reduce one event payload into the core without running its effects, so
    /// a test can stand a piece of daemon state up directly.
    #[cfg(test)]
    fn seed_core(&mut self, payload: EventPayload) {
        self.core.handle_message(ProtocolMessage::Event(Event {
            sequence: 0,
            payload,
        }));
        self.discard_core_effects();
    }

    #[cfg(test)]
    pub(crate) fn set_prefix_armed_for_test(&mut self, armed: bool, cx: &mut Context<Self>) {
        self.seed_core(EventPayload::PrefixArmed { armed });
        cx.notify();
    }

    /// Stand in for a daemon `Attached`/snapshot round trip in a view test.
    #[cfg(test)]
    pub(crate) fn attach_snapshot_for_test(
        &mut self,
        session: SessionId,
        snapshot: MuxSnapshot,
        cx: &mut Context<Self>,
    ) {
        self.core
            .handle_message(ProtocolMessage::Attached { session, snapshot });
        self.discard_core_effects();
        cx.notify();
    }

    /// What a cross-host attach leaves behind before the new daemon answers:
    /// the machine has moved and the outgoing snapshot is gone.
    #[cfg(test)]
    pub(crate) fn attach_host_for_test(&mut self, name: &str, cx: &mut Context<Self>) {
        let (host, _) = self
            .registry
            .get_by_name(name)
            .expect("a fleet host registered for the test");
        self.attached_host = host;
        self.core.clear_attachment();
        cx.notify();
    }

    #[must_use]
    pub(crate) fn appearance(&self) -> Arc<TerminalAppearance> {
        Arc::clone(&self.appearance)
    }

    pub(crate) fn adjust_terminal_font_size(
        &mut self,
        adjustment: TerminalFontSizeAdjustment,
        cx: &mut Context<Self>,
    ) {
        let previous = self.appearance.font_size_points;
        let next = adjusted_terminal_font_size(previous, adjustment);
        let applied_delta = next - previous;
        if applied_delta.abs() < f32::EPSILON {
            return;
        }

        self.terminal_font_size_offset_points += applied_delta;
        let mut appearance = self.appearance.as_ref().clone();
        appearance.font_size_points = next;
        self.appearance = Arc::new(appearance);
        log::info!(
            target: "zz::diagnostics::appearance",
            "terminal font size adjustment={adjustment:?} previous_points={previous} points={next} offset_points={}",
            self.terminal_font_size_offset_points,
        );
        cx.notify();
    }

    #[must_use]
    pub(crate) fn viewport(&self, pane: PaneId) -> Option<Arc<RwLock<RetainedTerminalViewport>>> {
        self.viewports.get(&pane).cloned()
    }

    #[must_use]
    pub(crate) fn kitty_images(&self, pane: PaneId) -> Option<Arc<RwLock<KittyImageCache>>> {
        self.kitty_images.get(&pane).cloned()
    }

    #[must_use]
    pub(crate) fn command_output(&self) -> Option<CommandOutputModel> {
        self.command_output.clone()
    }

    #[must_use]
    pub(crate) fn error(&self) -> Option<Arc<str>> {
        self.error.clone()
    }

    #[must_use]
    pub(crate) fn stale_daemon(&self) -> Option<StaleDaemonInfo> {
        self.stale_daemon
    }

    pub(crate) fn dismiss_stale_daemon(&mut self, cx: &mut Context<Self>) {
        if self.stale_daemon.take().is_none() {
            return;
        }
        let message = self.error.as_deref().map_or_else(
            || "the running zz daemon is incompatible with this zz".to_owned(),
            str::to_owned,
        );
        self.error = Some(Arc::from(format!(
            "{message}\nRun 'zz kill-server' to restart it manually."
        )));
        cx.notify();
    }

    pub(crate) fn restart_stale_daemon(&mut self, automatic: bool, cx: &mut Context<Self>) {
        let Some(stale) = self.stale_daemon.take() else {
            return;
        };
        let socket_path = self.registry.local_socket_path().to_path_buf();
        let color_scheme = self.color_scheme;
        self.error = Some(Arc::from("restarting stale zz daemon…"));
        cx.notify();

        let (results, incoming) = async_channel::bounded(1);
        let restart_thread = thread::Builder::new()
            .name("zz-stale-daemon-restart".to_owned())
            .spawn(move || {
                let result = terminate_incompatible_daemon(&socket_path)
                    .map_err(|error| error.to_string())
                    .and_then(|_| {
                        crate::connect_interactive_client(&socket_path, color_scheme)
                            .map_err(|error| error.to_string())
                    });
                let _ = results.send_blocking(result);
            });
        if let Err(error) = restart_thread {
            self.error = Some(Arc::from(format!(
                "could not start the daemon restart: {error}"
            )));
            cx.notify();
            return;
        }

        cx.spawn(async move |this, cx| {
            let Ok(result) = incoming.recv().await else {
                return;
            };
            let _ = this.update(cx, |client, cx| match result {
                Ok(connection) => match client.install_initial_local_connection(connection, cx) {
                    Ok(()) => {
                        client.error = None;
                        client.ensure_all_connected(cx);
                        if automatic {
                            let message = stale.daemon.map_or_else(
                                || {
                                    "restarted stale zz daemon (previous version unknown)"
                                        .to_owned()
                                },
                                |daemon| format!("restarted stale zz daemon (was v{daemon})"),
                            );
                            Self::emit_notification(ClientMessageKind::Success, message, cx);
                        }
                        cx.notify();
                    }
                    Err(error) => {
                        client.error = Some(Arc::from(format!(
                            "could not attach after restarting the zz daemon: {error}"
                        )));
                        cx.notify();
                    }
                },
                Err(error) => {
                    client.error = Some(Arc::from(format!(
                        "could not restart the zz daemon: {error}"
                    )));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn emit_notification(
        kind: ClientMessageKind,
        text: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        cx.emit(ClientNotification {
            kind,
            text: text.into(),
        });
    }

    #[must_use]
    pub(crate) fn command_prompt(&self) -> Option<&CommandPromptState> {
        self.core.command_prompt()
    }

    #[must_use]
    pub(crate) fn command_prompt_revision(&self) -> u64 {
        self.command_prompt_revision
    }

    #[must_use]
    pub(crate) fn choose_tree(&self) -> Option<&ChooseTreeState> {
        self.core.choose_tree()
    }

    #[must_use]
    pub(crate) fn choose_tree_revision(&self) -> u64 {
        self.choose_tree_revision
    }

    #[must_use]
    pub(crate) fn choose_buffer(&self) -> Option<&ChooseBufferState> {
        self.core.choose_buffer()
    }

    #[must_use]
    pub(crate) fn choose_buffer_revision(&self) -> u64 {
        self.choose_buffer_revision
    }

    #[must_use]
    pub(crate) fn display_panes(&self) -> Option<&DisplayPanesState> {
        self.core.display_panes()
    }

    #[must_use]
    pub(crate) fn display_panes_revision(&self) -> u64 {
        self.display_panes_revision
    }

    #[must_use]
    pub const fn sidebar_focus_revision(&self) -> u64 {
        self.sidebar_focus_revision
    }

    #[must_use]
    pub(crate) const fn bell_revision(&self) -> u64 {
        self.bell_revision
    }

    #[must_use]
    pub fn attached_session(&self) -> Option<SessionId> {
        self.core.attached_session()
    }

    pub(crate) fn send_input(&self, input: InputMessage) -> bool {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        log::trace!(target: "zz::diagnostics::mux", "send_input begin input={input:#?}");
        #[cfg(test)]
        if let Some(sink) = &self.input_sink {
            sink.borrow_mut().push(input);
            return true;
        }
        let sent = if let Some(client) = &self.attached_connection().client {
            if let Err(error) = client.send_input(input) {
                log::warn!("failed to send mux input: {error}");
                false
            } else {
                true
            }
        } else {
            log::trace!(target: "zz::diagnostics::mux", "send_input skipped: no client");
            false
        };
        log::trace!(
            target: "zz::diagnostics::mux",
            "send_input end elapsed_us={}",
            diagnostics::elapsed_us(started)
        );
        sent
    }

    /// Ship a pasted image to the daemon that owns `pane`, which lands it on
    /// its own host and pastes the resulting path into the pane. Returns
    /// whether the upload reached the wire.
    pub(crate) fn send_paste_upload(
        &self,
        upload_id: u64,
        pane: PaneId,
        extension: String,
        bytes: &[u8],
    ) -> bool {
        let Some(client) = &self.attached_connection().client else {
            log::trace!(target: "zz::diagnostics::mux", "paste upload skipped: no client");
            return false;
        };
        if let Err(error) = client.send_paste_upload(upload_id, pane, extension, bytes) {
            log::warn!("failed to upload a pasted image: {error}");
            return false;
        }
        true
    }

    /// Ship normalized image bytes to the daemon before forwarding the paste key.
    pub(crate) fn record_pasted_image(
        &self,
        upload_id: u64,
        pane: PaneId,
        extension: String,
        bytes: &[u8],
    ) -> bool {
        let Some(client) = &self.attached_connection().client else {
            log::trace!(target: "zz::diagnostics::mux", "pasted image record skipped: no client");
            return false;
        };
        if let Err(error) = client.record_pasted_image(upload_id, pane, extension, bytes) {
            log::warn!("failed to record a pasted image: {error}");
            return false;
        }
        true
    }

    pub fn execute(&self, command: CommandInvocation) {
        self.execute_on_host(self.attached_host, command);
    }

    pub fn execute_on_host(&self, host: HostId, command: CommandInvocation) {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        log::trace!(
            target: "zz::diagnostics::mux",
            "execute begin host={host:?} command={command:#?}",
        );
        let Some(connection) = self.connections.get(&host) else {
            log::warn!("cannot send mux command to unknown fleet host {host:?}");
            return;
        };
        let name = command.name.clone();
        if let Some(client) = &connection.client {
            match client.execute(command) {
                Ok(request_id) => connection.track_command(request_id, name),
                Err(error) => log::warn!("failed to send mux command: {error}"),
            }
        } else {
            #[cfg(test)]
            if let Some(client) = &connection.fake_client {
                match client.execute(command) {
                    Ok(request_id) => connection.track_command(request_id, name),
                    Err(error) => log::warn!("failed to send mux command: {error}"),
                }
                return;
            }
            log::trace!(
                target: "zz::diagnostics::mux",
                "execute skipped: no client for host={host:?}",
            );
        }
        log::trace!(
            target: "zz::diagnostics::mux",
            "execute end elapsed_us={}",
            diagnostics::elapsed_us(started)
        );
    }

    pub fn new_session(&self, host: HostId) {
        let Some(connection) = self.connections.get(&host) else {
            log::warn!("cannot create a session on unknown fleet host {host:?}");
            return;
        };
        let capabilities = connection.client.as_ref().map_or(&[][..], |client| {
            client.server_hello().capabilities.as_slice()
        });
        #[cfg(test)]
        let capabilities = connection
            .fake_client
            .as_ref()
            .map_or(capabilities, |client| client.hello.capabilities.as_slice());
        for command in new_session_commands(host, capabilities) {
            self.execute_on_host(host, command);
        }
    }

    pub fn set_color_scheme(&mut self, color_scheme: TerminalColorScheme) {
        self.color_scheme = color_scheme;
        if let Some(client) = &self.attached_connection().client
            && let Err(error) = client.set_color_scheme(color_scheme)
        {
            log::warn!("failed to report system color scheme: {error}");
        }
    }

    pub(crate) fn attach(&self, session: SessionId) {
        if let Some(client) = &self.attached_connection().client
            && let Err(error) = client.attach(session.to_string())
        {
            log::warn!("failed to attach mux session: {error}");
        }
    }

    pub fn attach_to_host(
        &mut self,
        host: HostId,
        session: SessionId,
        cx: &mut Context<Self>,
    ) -> bool {
        self.attach_to_host_target(host, Some(session), cx)
    }

    /// Switch to a machine and let its daemon select the default session. The
    /// sidebar uses this for machine rows, which name no session of their own.
    pub fn attach_to_host_default(&mut self, host: HostId, cx: &mut Context<Self>) -> bool {
        self.attach_to_host_target(host, None, cx)
    }

    /// Step off `host` so that removing it from the fleet actually ends its
    /// connection: [`Self::reconcile_hosts`] retains an attached host even
    /// after its configuration entry vanishes.
    pub(crate) fn release_host(&mut self, host: HostId, cx: &mut Context<Self>) {
        if self.attached_host == host {
            if self.registry.get(HostId::LOCAL).is_some() {
                self.attach_to_host_default(HostId::LOCAL, cx);
            } else {
                let outgoing_snapshot = Arc::clone(self.core.snapshot());
                self.cancel_reconnect(host);
                self.attached_connection_mut().snapshot = Some(outgoing_snapshot);
                if let Some(client) = &self.attached_connection().client {
                    let _ = client.detach();
                }
                self.reset_session_state(cx);
                self.clear_cross_host_state();
                self.attached_host = HostId::LOCAL;
                self.error = None;
                self.error_after_next_attach = None;
                cx.notify();
            }
        }
    }

    fn attach_to_host_target(
        &mut self,
        host: HostId,
        session: Option<SessionId>,
        cx: &mut Context<Self>,
    ) -> bool {
        if host == self.attached_host {
            if let Some(session) = session {
                self.attach(session);
            }
            return true;
        }

        let requested_name = self.registry.get(host).map(|entry| entry.name.clone());
        self.reconcile_hosts(cx);
        let Some((host, name)) = requested_name
            .as_deref()
            .and_then(|name| self.registry.get_by_name(name))
            .map(|(host, entry)| (host, entry.name.clone()))
        else {
            log::warn!("cannot attach to unknown fleet host {host:?}");
            return false;
        };
        let Some(connection) = self.connections.get(&host) else {
            log::warn!("cannot attach to fleet host {name}: no connection slot");
            return false;
        };
        if !connection.is_connected() {
            log::warn!(
                "cannot attach to fleet host {name}: host is {}",
                connection.state.label(),
            );
            return false;
        }
        let client = connection.client.as_ref().map(Arc::clone);
        #[cfg(test)]
        let fake_client = connection.fake_client.as_ref().map(Arc::clone);
        let hello = connection
            .current_hello()
            .expect("a connected host has an interactive client");

        let outgoing_snapshot = Arc::clone(self.core.snapshot());
        self.cancel_reconnect(self.attached_host);
        self.attached_connection_mut().snapshot = Some(outgoing_snapshot);
        if let Some(current) = &self.attached_connection().client {
            let _ = current.detach();
        }
        self.reset_session_state(cx);

        self.clear_cross_host_state();

        self.error = None;
        self.error_after_next_attach = None;
        self.attached_host = host;
        self.ingest_server_hello(hello, cx);
        self.status_revision = self.status_revision.saturating_add(1);
        let attach_result = if let Some(client) = client {
            crate::config::register_config_override_client(&client, host != HostId::LOCAL, cx);
            client.attach(session.map_or_else(String::new, |session| session.to_string()))
        } else {
            #[cfg(test)]
            {
                fake_client
                    .expect("a connected test host has a fake client")
                    .attach(session)
            }
            #[cfg(not(test))]
            unreachable!("a connected host has an interactive client");
        };
        if let Err(error) = attach_result {
            log::warn!("failed to attach mux session on fleet host {name}: {error}");
        }
        self.reconcile_hosts(cx);
        cx.notify();
        true
    }

    fn give_up_removed_attached_host(&mut self, name: &str, cx: &mut Context<Self>) {
        debug_assert_eq!(self.attached_host, HostId::LOCAL);
        self.reset_session_state(cx);
        self.clear_cross_host_state();
        let banner = Arc::<str>::from(format!("fleet host {name} disconnected"));
        self.error = Some(Arc::clone(&banner));
        self.error_after_next_attach = None;

        let connection = self.attached_connection();
        if !connection.is_connected() {
            return;
        }
        let client = connection.client.as_ref().map(Arc::clone);
        #[cfg(test)]
        let fake_client = connection.fake_client.as_ref().map(Arc::clone);
        let hello = connection
            .current_hello()
            .expect("a connected host has an interactive client");
        self.ingest_server_hello(hello, cx);
        self.status_revision = self.status_revision.saturating_add(1);
        let result = if let Some(client) = client {
            crate::config::register_config_override_client(&client, false, cx);
            client.attach("")
        } else {
            #[cfg(test)]
            {
                fake_client
                    .expect("a connected test host has a fake client")
                    .attach(None)
            }
            #[cfg(not(test))]
            unreachable!("a connected host has an interactive client");
        };
        if let Err(error) = result {
            log::warn!("failed to fall back after removing fleet host {name}: {error}");
        } else {
            self.error_after_next_attach = Some(banner);
        }
    }

    fn clear_cross_host_state(&mut self) {
        self.core.clear_attachment();
        self.attached_snapshot_pending = true;
        self.viewports.clear();
        self.clear_all_kitty_images();
        self.browser_commands.clear();
        self.agent_commands.clear();
        self.clear_agent_streams();
        self.screenshot_requests.clear();
        self.terminal_commands.clear();
        self.clear_all_pasted_images();
        self.pending_commands_revision = self.pending_commands_revision.wrapping_add(1).max(1);
        self.command_output = None;
        self.command_output_diff.invalidate();
    }

    fn reset_session_state(&mut self, _cx: &mut Context<Self>) {
        let connection = self.attached_connection_mut();
        connection.resync_pending = false;
        connection.full_requests_pending.clear();
        connection.history_requests_pending.clear();
        connection.history_backfill_deferred.clear();
        self.core.reset_session();
        self.command_prompt_revision = self.command_prompt_revision.wrapping_add(1).max(1);
        self.command_output = None;
        self.choose_tree_revision = self.choose_tree_revision.wrapping_add(1).max(1);
        self.choose_buffer_revision = self.choose_buffer_revision.wrapping_add(1).max(1);
        self.display_panes_revision = self.display_panes_revision.wrapping_add(1).max(1);
        self.clear_all_kitty_images();
        self.clear_all_pasted_images();
    }

    fn kitty_image_cache(&mut self, pane: PaneId) -> Arc<RwLock<KittyImageCache>> {
        self.kitty_images
            .entry(pane)
            .or_insert_with(|| Arc::new(RwLock::new(KittyImageCache::default())))
            .clone()
    }

    fn begin_kitty_image(
        &mut self,
        pane: PaneId,
        image_id: u32,
        generation: u64,
        width: u32,
        height: u32,
        total_bytes: u32,
    ) {
        let cache = self.kitty_image_cache(pane);
        if cache.read().contains(image_id, generation) {
            self.kitty_image_assemblies
                .retain(|(target, id, _), _| *target != pane || *id != image_id);
            return;
        }
        self.kitty_image_assemblies
            .retain(|(target, id, _), _| *target != pane || *id != image_id);
        let total_bytes = usize::try_from(total_bytes).unwrap_or(usize::MAX);
        self.kitty_image_assemblies.insert(
            (pane, image_id, generation),
            KittyImageAssembly {
                width,
                height,
                total_bytes,
                bytes: Vec::with_capacity(total_bytes),
            },
        );
    }

    fn push_kitty_image_chunk(
        &mut self,
        pane: PaneId,
        image_id: u32,
        generation: u64,
        bytes: &[u8],
    ) {
        let key = (pane, image_id, generation);
        let complete = {
            let Some(assembly) = self.kitty_image_assemblies.get_mut(&key) else {
                return;
            };
            if assembly
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|next| next > assembly.total_bytes)
            {
                log::warn!(
                    "discarding over-declared Kitty image {image_id} generation {generation} for {pane}"
                );
                self.kitty_image_assemblies.remove(&key);
                return;
            }
            assembly.bytes.extend_from_slice(bytes);
            assembly.bytes.len() == assembly.total_bytes
        };
        if !complete {
            return;
        }
        let assembly = self
            .kitty_image_assemblies
            .remove(&key)
            .expect("completed Kitty assembly remains present");
        let cache = self.kitty_image_cache(pane);
        let retain_replaced = Arc::strong_count(&cache) > 2;
        let mut cache = cache.write();
        if cache.contains(image_id, generation) {
            return;
        }
        let expected = usize::try_from(assembly.width)
            .ok()
            .and_then(|width| {
                usize::try_from(assembly.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4));
        if expected != Some(assembly.bytes.len()) {
            log::warn!(
                "discarding malformed Kitty image {image_id} generation {generation} for {pane}"
            );
            return;
        }
        // GPUI reads these bytes as premultiplied BGRA; `Rgba` names the
        // four-byte storage layout, not the channel order.
        let Some(buffer) = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
            assembly.width,
            assembly.height,
            assembly.bytes,
        ) else {
            return;
        };
        cache.insert(
            image_id,
            generation,
            Arc::new(RenderImage::new(vec![ImageFrame::new(buffer)])),
            retain_replaced,
        );
    }

    fn remove_kitty_images(&mut self, pane: PaneId, image_ids: &[u32]) {
        self.kitty_image_assemblies
            .retain(|(target, id, _), _| *target != pane || !image_ids.contains(id));
        if let Some(cache) = self.kitty_images.get(&pane) {
            let retain_removed = Arc::strong_count(cache) > 1;
            let mut cache = cache.write();
            for image_id in image_ids {
                cache.remove(*image_id, retain_removed);
            }
        }
    }

    fn clear_kitty_images(&mut self, pane: PaneId) {
        self.kitty_image_assemblies
            .retain(|(target, _, _), _| *target != pane);
        if let Some(cache) = self.kitty_images.remove(&pane) {
            cache.write().clear();
        }
    }

    fn clear_all_kitty_images(&mut self) {
        self.kitty_image_assemblies.clear();
        for cache in std::mem::take(&mut self.kitty_images).into_values() {
            cache.write().clear();
        }
    }

    fn clear_all_pasted_images(&mut self) {
        self.pane_images.clear();
        self.pasted_image_assemblies.clear();
        self.pending_pasted_image_previews.clear();
    }

    pub(crate) fn take_browser_commands(&mut self, pane: PaneId) -> Vec<BrowserCommand> {
        self.browser_commands.remove(&pane).unwrap_or_default()
    }

    pub(crate) fn take_terminal_commands(&mut self, pane: PaneId) -> Vec<TerminalUiCommand> {
        self.terminal_commands.remove(&pane).unwrap_or_default()
    }

    pub(crate) fn pasted_image(&self, pane: PaneId, number: u32) -> Option<Arc<Image>> {
        self.pane_images
            .get(&pane)
            .and_then(|images| images.get(number))
    }

    pub(crate) fn pasted_image_revision(&self, pane: PaneId) -> u64 {
        self.pane_images
            .get(&pane)
            .map_or(0, |images| images.revision)
    }

    pub(crate) fn prefetch_pasted_image(&self, pane: PaneId, number: u32) -> bool {
        let Some(client) = &self.attached_connection().client else {
            return false;
        };
        if let Err(error) = client.fetch_pasted_image(pane, number) {
            log::warn!("failed to fetch pasted image #{number} for {pane}: {error}");
            return false;
        }
        true
    }

    pub(crate) fn open_pasted_image(&mut self, pane: PaneId, number: u32, cx: &mut Context<Self>) {
        if let Some(image) = self.pasted_image(pane, number) {
            cx.emit(AttachmentPreviewRequest { image });
            return;
        }
        self.pending_pasted_image_previews.insert((pane, number));
        if !self.prefetch_pasted_image(pane, number) {
            self.pending_pasted_image_previews.remove(&(pane, number));
        }
    }

    fn begin_pasted_image(
        &mut self,
        pane: PaneId,
        number: u32,
        format: PastedImageFormat,
        total_bytes: u32,
    ) {
        let total_bytes = usize::try_from(total_bytes).unwrap_or(usize::MAX);
        self.pasted_image_assemblies.remove(&(pane, number));
        if let Some(images) = self.pane_images.get_mut(&pane) {
            images.remove(number);
        }
        self.pasted_image_assemblies.insert(
            (pane, number),
            PastedImageAssembly {
                format,
                total_bytes,
                bytes: Vec::with_capacity(total_bytes),
            },
        );
    }

    fn push_pasted_image_chunk(
        &mut self,
        pane: PaneId,
        number: u32,
        bytes: &[u8],
        cx: &mut Context<Self>,
    ) {
        let key = (pane, number);
        let complete = {
            let Some(assembly) = self.pasted_image_assemblies.get_mut(&key) else {
                return;
            };
            if assembly
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|next| next > assembly.total_bytes)
            {
                self.pasted_image_assemblies.remove(&key);
                self.pending_pasted_image_previews.remove(&key);
                return;
            }
            assembly.bytes.extend_from_slice(bytes);
            assembly.bytes.len() == assembly.total_bytes
        };
        if !complete {
            return;
        }
        let assembly = self
            .pasted_image_assemblies
            .remove(&key)
            .expect("the completed pasted-image assembly remains present");
        let image = Arc::new(Image::from_bytes(
            gpui_image_format(assembly.format),
            assembly.bytes,
        ));
        self.pane_images
            .entry(pane)
            .or_default()
            .insert(number, Arc::clone(&image));
        if self.pending_pasted_image_previews.remove(&key) {
            cx.emit(AttachmentPreviewRequest { image });
        }
    }

    fn pasted_image_unavailable(&mut self, pane: PaneId, number: u32) {
        self.pasted_image_assemblies.remove(&(pane, number));
        self.pending_pasted_image_previews.remove(&(pane, number));
        if let Some(images) = self.pane_images.get_mut(&pane) {
            images.remove(number);
        }
    }

    /// Buffer one coalesced agent batch, dropping what the shell already
    /// applied: a replay deliberately overlaps the live tail, so the per-pane
    /// cursor is what makes the stream idempotent. A batch that starts past the
    /// cursor is a hole in the journal the shell cannot fill by waiting, so the
    /// pane re-requests from where it is instead of buffering across it — once,
    /// because the batches that keep arriving across the hole are the same gap
    /// seen again, and the outstanding replay is what closes it. A batch that
    /// reaches the cursor without a gap is the proof it closed.
    #[cfg(feature = "agent-pane")]
    fn apply_agent_updates(&mut self, pane: PaneId, first_seq: u64, items: Vec<Vec<u8>>) {
        let cursor = self.agent_cursors.entry(pane).or_default();
        let mut accepted = Vec::new();
        let mut gapped = false;
        for (index, blob) in items.into_iter().enumerate() {
            let positional = first_seq.saturating_add(index as u64);
            let item = match serde_json::from_slice::<zz_daemon::AgentStreamItem>(&blob) {
                Ok(item) => item,
                Err(error) => {
                    log::warn!(
                        target: "zz::diagnostics::mux",
                        "dropping undecodable agent item pane={pane} seq={positional}: {error}"
                    );
                    *cursor = (*cursor).max(positional);
                    continue;
                }
            };
            if item.seq > cursor.saturating_add(1)
                && matches!(
                    &item.payload,
                    zz_daemon::AgentStreamPayload::SessionReset { restoring: true }
                )
            {
                *cursor = item.seq.saturating_sub(1);
            }
            if item.seq <= *cursor {
                continue;
            }
            if item.seq > cursor.saturating_add(1) {
                gapped = true;
                break;
            }
            *cursor = item.seq;
            accepted.push(item);
        }
        if !accepted.is_empty() {
            self.agent_events.items.push((pane, accepted));
        }
        if gapped {
            if !self.agent_replays_pending.contains(&pane) {
                self.request_agent_replay_from_cursor(pane);
            }
        } else {
            self.agent_replays_pending.remove(&pane);
        }
    }

    #[cfg(not(feature = "agent-pane"))]
    fn apply_agent_updates(&mut self, _pane: PaneId, _first_seq: u64, _items: Vec<Vec<u8>>) {}

    /// Ask the daemon to replay this pane's stream from where the shell is.
    /// Sent on a lane overflow, a journal gap, and when a pane's view goes live.
    /// The daemon serves `from_seq` inclusively, so asking from the cursor
    /// re-sends one applied item rather than risking a skipped one. An ask that
    /// reaches the wire is recorded as outstanding, which is what keeps a hole
    /// from asking again on every batch until the replay lands; a caller that
    /// asks explicitly is a lifecycle event, not a hole, so it always sends.
    #[cfg(feature = "agent-pane")]
    pub(crate) fn request_agent_replay_from_cursor(&mut self, pane: PaneId) -> bool {
        let from_seq = self.agent_cursors.get(&pane).copied().unwrap_or_default();
        let sent = self.send_agent_request(pane, AgentRequest::Replay { from_seq });
        if sent {
            self.agent_replays_pending.insert(pane);
        }
        sent
    }

    #[cfg(not(feature = "agent-pane"))]
    fn request_agent_replay_from_cursor(&mut self, _pane: PaneId) {}

    /// Forget every pane's replay cursor. The core clears its agent state at
    /// the same two moments, so the next attach replays from seq 0 rather than
    /// filtering the new stream against the old connection's cursor.
    #[cfg(feature = "agent-pane")]
    fn clear_agent_streams(&mut self) {
        self.agent_cursors.clear();
        self.agent_replays_pending.clear();
        self.agent_events = AgentEvents::default();
    }

    #[cfg(not(feature = "agent-pane"))]
    fn clear_agent_streams(&mut self) {}

    #[cfg(feature = "agent-pane")]
    pub(crate) fn take_agent_events_for(&mut self, panes: &BTreeSet<PaneId>) -> AgentEvents {
        let pending = std::mem::take(&mut self.agent_events);
        let mut ready = AgentEvents::default();
        for (pane, items) in pending.items {
            if panes.contains(&pane) {
                ready.items.push((pane, items));
            } else {
                self.agent_events.items.push((pane, items));
            }
        }
        for (pane, state) in pending.states {
            if panes.contains(&pane) {
                ready.states.push((pane, state));
            } else {
                self.agent_events.states.push((pane, state));
            }
        }
        for (pane, request_id, result) in pending.sessions {
            if panes.contains(&pane) {
                ready.sessions.push((pane, request_id, result));
            } else {
                self.agent_events.sessions.push((pane, request_id, result));
            }
        }
        ready
    }

    #[cfg(feature = "agent-pane")]
    #[must_use]
    pub(crate) fn has_agent_events(&self) -> bool {
        !self.agent_events.is_empty()
    }

    /// Ship one agent request to the daemon that owns `pane`. Returns whether
    /// it reached the wire, which is what the composer reports as "not
    /// connected".
    #[cfg(feature = "agent-pane")]
    pub(crate) fn send_agent_request(&self, pane: PaneId, request: AgentRequest) -> bool {
        #[cfg(test)]
        if let Some(sink) = &self.agent_sink {
            sink.borrow_mut().push((pane, request));
            return true;
        }
        let Some(client) = &self.attached_connection().client else {
            log::trace!(target: "zz::diagnostics::mux", "agent request skipped: no client");
            return false;
        };
        let sent = match request {
            AgentRequest::Prompt { text, images } => client.agent_prompt(pane, text, images),
            AgentRequest::Cancel => client.agent_cancel(pane),
            AgentRequest::Unqueue => client.agent_unqueue(pane),
            AgentRequest::RespondPermission {
                request_id,
                option_id,
            } => client.agent_respond_permission(pane, request_id, option_id),
            AgentRequest::SetConfigOption { option_id, value } => {
                client.agent_set_config_option(pane, option_id, value)
            }
            AgentRequest::SetMode { mode_id } => client.agent_set_mode(pane, mode_id),
            AgentRequest::Authenticate { method_id } => client.agent_authenticate(pane, method_id),
            AgentRequest::SessionOp { op } => client.agent_session_op(pane, op),
            AgentRequest::Replay { from_seq } => client.agent_replay(pane, from_seq),
            AgentRequest::AcknowledgePromptRestore { reclaim_id } => {
                client.agent_acknowledge_prompt_restore(pane, reclaim_id)
            }
        };
        if let Err(error) = sent {
            log::warn!("failed to send an agent request for {pane}: {error}");
            return false;
        }
        true
    }

    /// Every queued `agent-send` payload, in arrival order per pane.
    pub(crate) fn take_agent_commands(&mut self) -> Vec<(PaneId, u64, AgentCommand)> {
        std::mem::take(&mut self.agent_commands)
            .into_iter()
            .flat_map(|(pane, commands)| {
                commands
                    .into_iter()
                    .map(move |(request_id, command)| (pane, request_id, command))
            })
            .collect()
    }

    pub(crate) fn take_screenshot_requests(&mut self) -> Vec<(PaneId, u64, String)> {
        std::mem::take(&mut self.screenshot_requests)
    }

    /// Whether any daemon-issued GUI request is waiting for an answer.
    #[must_use]
    pub(crate) fn has_gui_requests(&self) -> bool {
        !self.agent_commands.is_empty() || !self.screenshot_requests.is_empty()
    }

    /// Answer a daemon request for GUI-owned work. A CLI client is blocked on
    /// this, so the reply goes out even when the work failed.
    pub(crate) fn respond_to_request(&self, response: GuiResponse) {
        if let Some(client) = &self.attached_connection().client
            && let Err(error) = client.send_gui_response(response)
        {
            log::warn!("failed to answer a daemon GUI request: {error}");
        }
    }

    #[must_use]
    pub(crate) const fn pending_commands_revision(&self) -> u64 {
        self.pending_commands_revision
    }

    pub fn detach(&mut self) {
        self.shutting_down = true;
        if let Some(client) = &self.attached_connection().client {
            let _ = client.detach();
        }
    }

    pub(crate) fn log_diagnostic_snapshot(&self, reason: &str) {
        log::info!(
            target: "zz::diagnostics::mux_state",
            "snapshot reason={reason} connected={} host_state={} appearance_hash={} terminal_font_size_points={} terminal_font_size_offset_points={} attached_session={:?} mux_generation={} sessions={} retained_viewports={} browser_command_panes={} terminal_command_panes={} command_output={} command_prompt={} choose_tree={} choose_buffer={} display_panes={} next_row_revision={} resync_pending={} error={:?}",
            self.attached_connection().client.is_some(),
            self.attached_connection().state.label(),
            self.appearance.stable_hash(),
            self.appearance.font_size_points,
            self.terminal_font_size_offset_points,
            self.core.attached_session(),
            self.core.snapshot().generation,
            self.core.snapshot().sessions.len(),
            self.viewports.len(),
            self.browser_commands.len(),
            self.terminal_commands.len(),
            self.command_output.is_some(),
            self.core.command_prompt().is_some(),
            self.core.choose_tree().is_some(),
            self.core.choose_buffer().is_some(),
            self.core.display_panes().is_some(),
            self.next_row_revision,
            self.attached_connection().resync_pending,
            self.error,
        );
        log::trace!(
            target: "zz::diagnostics::mux_state",
            "snapshot reason={reason} mux={:#?} browser_commands={:#?} terminal_commands={:#?} command_prompt={:#?} choose_tree={:#?} choose_buffer={:#?} display_panes={:#?}",
            self.core.snapshot(),
            self.browser_commands,
            self.terminal_commands,
            self.core.command_prompt(),
            self.core.choose_tree(),
            self.core.choose_buffer(),
            self.core.display_panes(),
        );
        for (pane, retained) in &self.viewports {
            let retained_state = retained.read();
            let viewport = &retained_state.viewport;
            log::info!(
                target: "zz::diagnostics::terminal_state",
                "snapshot reason={reason} pane={pane} retained_strong_count={} generation={} view_generation={} dictionary_generation={} columns={} rows={} cell_count={} cell_bytes={} cell_arc_strong_count={} dictionary_arc_strong_count={} styles={} graphemes={} grapheme_bytes={} overlays={} row_revisions={} row_revision_epoch={} revision_scratch_len={} revision_scratch_capacity={} title={:?} cursor={:?} scrollbar={:?} mode={:?} search={:?} unseen_output={} kitty_keyboard={} mouse_tracking={} status={:?}",
                Arc::strong_count(retained),
                viewport.generation,
                viewport.view_generation,
                viewport.dictionary_generation,
                viewport.columns,
                viewport.rows,
                viewport.cells.len(),
                std::mem::size_of_val(viewport.cells.as_ref()),
                Arc::strong_count(&viewport.cells),
                Arc::strong_count(&viewport.dictionary),
                viewport.dictionary.styles.len(),
                viewport.dictionary.grapheme_offsets.len(),
                viewport.dictionary.grapheme_bytes.len(),
                viewport.overlays.len(),
                retained_state.row_revisions.len(),
                retained_state.row_revision_epoch,
                retained_state.revision_scratch.len(),
                retained_state.revision_scratch.capacity(),
                viewport.title(),
                viewport.cursor,
                viewport.scrollbar,
                viewport.mode,
                viewport.search,
                viewport.unseen_output,
                viewport.kitty_keyboard,
                viewport.mouse_tracking,
                viewport.status,
            );
            log::trace!(
                target: "zz::diagnostics::terminal_state",
                "snapshot reason={reason} pane={pane} viewport={viewport:#?} row_revisions={:#?}",
                retained_state.row_revisions,
            );
        }
        if let Some(output) = &self.command_output {
            let retained = output.retained.read();
            log::trace!(
                target: "zz::diagnostics::terminal_state",
                "snapshot reason={reason} command_output_pane={} viewport={:#?} row_revisions={:#?}",
                output.pane,
                retained.viewport,
                retained.row_revisions,
            );
        }
    }

    fn report_command_failure(
        &self,
        host: HostId,
        request_id: u64,
        error: &ServerError,
        cx: &mut Context<Self>,
    ) -> bool {
        if request_id == 0 {
            return false;
        }
        let tracked = self
            .connections
            .get(&host)
            .and_then(|connection| connection.take_command(request_id));
        match tracked {
            Some(name)
                if matches!(
                    error,
                    ServerError::PaneExited(_) | ServerError::PaneNotAttached(_)
                ) =>
            {
                log::debug!(
                    target: "zz::diagnostics::mux",
                    "command_raced_teardown host={host:?} command={name} error={error}"
                );
            }
            Some(name) => {
                log::warn!(
                    target: "zz::diagnostics::mux",
                    "command_failed host={host:?} command={name} error={error}"
                );
                Self::emit_notification(ClientMessageKind::Error, format!("{name}: {error}"), cx);
            }
            None => log::warn!(
                target: "zz::diagnostics::mux",
                "untracked_command_failed host={host:?} request_id={request_id} error={error}"
            ),
        }
        true
    }

    fn handle_message(&mut self, host: HostId, message: ProtocolMessage, cx: &mut Context<Self>) {
        if host != self.attached_host {
            match message {
                ProtocolMessage::Event(event) => match event.payload {
                    EventPayload::Snapshot(snapshot) => {
                        if let Some(connection) = self.connections.get_mut(&host) {
                            connection.snapshot = Some(Arc::new(snapshot));
                            cx.notify();
                        }
                    }
                    EventPayload::Bell { pane } => {
                        self.record_bell(host, pane);
                        cx.notify();
                    }
                    EventPayload::AppearanceChanged {
                        appearance,
                        provenance,
                    } => {
                        if let Some(connection) = self.connections.get_mut(&host) {
                            connection.appearance = Some((*appearance, provenance));
                        }
                    }
                    _ => {}
                },
                ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Error {
                    request_id,
                    error,
                }) => {
                    if !self.report_command_failure(host, request_id, &error, cx) {
                        log::warn!(
                            target: "zz::diagnostics::mux",
                            "unsolicited_error host={host:?} error={error}"
                        );
                    }
                }
                ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Success {
                    request_id,
                    ..
                }) => {
                    if let Some(connection) = self.connections.get(&host) {
                        connection.take_command(request_id);
                    }
                }
                _ => {}
            }
            return;
        }
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        log::trace!(
            target: "zz::diagnostics::mux",
            "handle_message begin message={message:#?}"
        );
        match message {
            // The terminal frame path never reaches the core: the retained
            // viewport carries the history ring, row revisions and diff
            // scratch the painter reads, so delegating it would keep a second
            // copy of every grid and re-apply every patch on the hot path.
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalViewport { pane, viewport },
                ..
            }) => self.apply_terminal_viewport(pane, viewport),
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalPatch { pane, patch },
                ..
            }) => self.apply_terminal_patch(pane, patch),
            ProtocolMessage::Event(Event {
                payload: EventPayload::CommandOutput { pane, viewport },
                ..
            }) => self.apply_command_output(pane, viewport),
            message => {
                // `Detached` clears the attachment during reduction, so the
                // session it names has to be compared against what the core
                // held before.
                let attached_before = self.core.attached_session();
                self.core.handle_message(message);
                while let Some(outbound) = self.core.poll_outbound() {
                    match outbound {
                        Outbound::RequestFull(pane) => self.request_full_viewport(pane),
                    }
                }
                let mut attaching = false;
                while let Some(event) = self.core.poll_event() {
                    self.apply_core_event(host, event, attached_before, &mut attaching, cx);
                }
            }
        }
        log::trace!(
            target: "zz::diagnostics::mux",
            "handle_message end elapsed_us={} mux_generation={} viewports={} resync_pending={}",
            diagnostics::elapsed_us(started),
            self.core.snapshot().generation,
            self.viewports.len(),
            self.attached_connection().resync_pending,
        );
        cx.notify();
    }

    /// Run the GPUI half of one reduced change: revisions, toasts, clipboard
    /// and pane bookkeeping. The state itself already landed in the core.
    ///
    /// `attaching` records that this message also attached, so the snapshot it
    /// carries does not re-request history for viewports the outgoing session
    /// left behind — an `Attached` never did that.
    fn apply_core_event(
        &mut self,
        host: HostId,
        event: CoreEvent,
        attached_before: Option<SessionId>,
        attaching: &mut bool,
        cx: &mut Context<Self>,
    ) {
        match event {
            CoreEvent::Attached { session } => {
                *attaching = true;
                self.finish_attach(host, session, cx);
            }
            CoreEvent::SnapshotChanged => {
                self.attached_snapshot_pending = false;
                let connection = self.attached_connection_mut();
                connection.resync_pending = false;
                connection.full_requests_pending.clear();
                connection.history_requests_pending.clear();
                connection.history_backfill_deferred.clear();
                if !*attaching {
                    self.backfill_retained_history();
                }
            }
            CoreEvent::AppearanceChanged => self.adopt_core_appearance(cx),
            CoreEvent::MuxOptionsChanged => self.backfill_retained_history(),
            CoreEvent::StatusChanged => {
                self.status_revision = self.status_revision.saturating_add(1);
            }
            CoreEvent::PrefixArmed { armed } => log::info!(
                target: "zz::diagnostics::input",
                "prefix_armed_received armed={armed}"
            ),
            CoreEvent::CommandPromptChanged => {
                self.command_prompt_revision = self.command_prompt_revision.wrapping_add(1).max(1);
            }
            CoreEvent::ChooseTreeChanged => {
                self.choose_tree_revision = self.choose_tree_revision.wrapping_add(1).max(1);
            }
            CoreEvent::ChooseBufferChanged => {
                self.choose_buffer_revision = self.choose_buffer_revision.wrapping_add(1).max(1);
            }
            CoreEvent::DisplayPanesChanged => {
                self.display_panes_revision = self.display_panes_revision.wrapping_add(1).max(1);
            }
            CoreEvent::PaneRemoved { pane } => self.forget_pane(pane),
            CoreEvent::Bell { pane } => self.record_bell(host, pane),
            CoreEvent::FocusSidebar => {
                self.sidebar_focus_revision = self.sidebar_focus_revision.wrapping_add(1).max(1);
                log::info!(
                    target: "zz::diagnostics::input",
                    "focus_sidebar_received revision={}",
                    self.sidebar_focus_revision
                );
            }
            CoreEvent::Detached { session, by } => {
                if attached_before == Some(session) {
                    self.reset_session_state(cx);
                    Self::emit_notification(
                        ClientMessageKind::Warning,
                        by.map_or_else(
                            || "session ended".to_owned(),
                            |device| format!("detached by {device}"),
                        ),
                        cx,
                    );
                }
            }
            CoreEvent::ServerStopping => {
                self.error_after_next_attach = None;
                self.reset_session_state(cx);
                if host == HostId::LOCAL {
                    self.error = Some(Arc::from("zz daemon stopped"));
                } else {
                    self.error = None;
                }
            }
            CoreEvent::CommandResponse(response) => {
                self.handle_command_response(host, response, cx);
            }
            CoreEvent::ClientMessage { kind, text, .. } => Self::emit_notification(kind, text, cx),
            CoreEvent::Clipboard { target, text, .. } => {
                if !text.is_empty() {
                    let item = ClipboardItem::new_string(text);
                    match target {
                        ClipboardTarget::Clipboard => cx.write_to_clipboard(item),
                        ClipboardTarget::Primary => {
                            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                            cx.write_to_primary(item);
                            #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
                            cx.write_to_clipboard(item);
                        }
                    }
                }
            }
            CoreEvent::OpenUri { pane, uri } => self.route_open_uri(pane, &uri, cx),
            CoreEvent::AgentCommand {
                pane,
                request_id,
                command,
            } => {
                self.agent_commands
                    .entry(pane)
                    .or_default()
                    .push((request_id, command));
                self.pending_commands_revision =
                    self.pending_commands_revision.wrapping_add(1).max(1);
            }
            CoreEvent::BrowserCommand { pane, command } => {
                if let BrowserCommand::Screenshot { request_id, path } = command {
                    self.screenshot_requests.push((pane, request_id, path));
                } else {
                    self.browser_commands.entry(pane).or_default().push(command);
                }
                self.pending_commands_revision =
                    self.pending_commands_revision.wrapping_add(1).max(1);
            }
            CoreEvent::TerminalUiCommand { pane, command } => {
                self.terminal_commands
                    .entry(pane)
                    .or_default()
                    .push(command);
                self.pending_commands_revision =
                    self.pending_commands_revision.wrapping_add(1).max(1);
            }
            CoreEvent::HistoryChunk {
                pane,
                start,
                total,
                offset,
                columns,
                rows,
                dictionary,
            } => self.apply_history_chunk_event(
                pane, start, total, offset, columns, rows, dictionary, cx,
            ),
            CoreEvent::KittyImageBegin {
                pane,
                image_id,
                generation,
                width,
                height,
                total_bytes,
            } => self.begin_kitty_image(pane, image_id, generation, width, height, total_bytes),
            CoreEvent::KittyImageChunk {
                pane,
                image_id,
                generation,
                bytes,
            } => self.push_kitty_image_chunk(pane, image_id, generation, &bytes),
            CoreEvent::KittyImagesRemoved { pane, image_ids } => {
                self.remove_kitty_images(pane, &image_ids);
            }
            CoreEvent::AgentUpdates {
                pane,
                first_seq,
                items,
            } => self.apply_agent_updates(pane, first_seq, items),
            CoreEvent::AgentStateChanged { pane } => {
                #[cfg(feature = "agent-pane")]
                if let Some(state) = self.core.agent_state(pane).cloned() {
                    self.agent_events.states.push((pane, state));
                    // The daemon publishes state to every attached client and
                    // pushes it again on resync, but never pushes the stream:
                    // a pane the shell has no cursor for is one it has to ask
                    // for, which is what makes a reattach replay-then-tail.
                    if let std::collections::btree_map::Entry::Vacant(slot) =
                        self.agent_cursors.entry(pane)
                    {
                        slot.insert(0);
                        self.request_agent_replay_from_cursor(pane);
                    }
                }
                #[cfg(not(feature = "agent-pane"))]
                let _ = pane;
            }
            // An overflow is a hole the daemon reports rather than one the shell
            // infers, so it recovers the same way: from the shell's own cursor,
            // which is at or behind the sequence the daemon dropped from.
            CoreEvent::AgentLagged { pane, next_seq } => {
                log::warn!(
                    target: "zz::diagnostics::mux",
                    "agent lane cleared for {pane} from {next_seq}; replaying from the shell's cursor"
                );
                self.request_agent_replay_from_cursor(pane);
            }
            CoreEvent::AgentSessions {
                pane,
                request_id,
                result,
            } => {
                #[cfg(feature = "agent-pane")]
                self.agent_events.sessions.push((pane, request_id, result));
                #[cfg(not(feature = "agent-pane"))]
                let _ = (pane, request_id, result);
            }
            CoreEvent::Message(message) => self.handle_unreduced_message(*message, cx),
            // Key tables are read straight off the core; the handshake is
            // ingested rather than received; the frame path never reaches the
            // core, so its two viewport events cannot fire here.
            CoreEvent::KeyTablesChanged
            | CoreEvent::HelloReceived
            | CoreEvent::ViewportChanged { .. }
            | CoreEvent::CommandOutputChanged => {}
        }
    }

    fn finish_attach(&mut self, host: HostId, session: SessionId, cx: &mut Context<Self>) {
        self.clear_all_kitty_images();
        self.clear_all_pasted_images();
        self.clear_agent_streams();
        self.attached_snapshot_pending = false;
        let connection = self.attached_connection_mut();
        connection.last_attached_session = Some(session);
        let reconnected = connection.reconnect_attach.take().is_some();
        connection.resync_pending = false;
        connection.full_requests_pending.clear();
        connection.history_requests_pending.clear();
        connection.history_backfill_deferred.clear();
        self.error = self.error_after_next_attach.take();
        if reconnected {
            let name = self
                .registry
                .get(host)
                .map_or_else(|| format!("{host:?}"), |entry| entry.name.clone());
            Self::emit_notification(
                ClientMessageKind::Success,
                format!("reconnected to {name}"),
                cx,
            );
        }
    }

    fn backfill_retained_history(&mut self) {
        let panes = self.viewports.keys().copied().collect::<Vec<_>>();
        for pane in panes {
            self.request_history_backfill(pane);
        }
    }

    /// Re-derive the rendered appearance after the core reduced a reload. The
    /// core keeps the daemon's value; localization and the client-local font
    /// size offset are GPUI-side and stay here.
    fn adopt_core_appearance(&mut self, cx: &mut Context<Self>) {
        let previous_hash = self.appearance.stable_hash();
        let provenance = self.core.appearance_provenance().clone();
        let mut appearance = self.core.appearance().cloned().unwrap_or_default();
        let configured_font_size = appearance.font_size_points;
        let requested_primary_font = appearance.font_families.first().cloned();
        self.attached_connection_mut().appearance = Some((appearance.clone(), provenance.clone()));
        localize_terminal_font_families(
            &mut appearance,
            &provenance,
            &cx.text_system().all_font_names(),
        );
        self.terminal_font_size_offset_points =
            apply_terminal_font_size_offset(&mut appearance, self.terminal_font_size_offset_points);
        self.appearance = Arc::new(appearance);
        crate::theme::set_terminal_appearance(Arc::clone(&self.appearance), cx);
        self.command_output_diff.invalidate();
        log::info!(
            target: "zz::diagnostics::appearance",
            "reloaded appearance previous_hash={previous_hash} hash={} scheme={} requested_primary_font={requested_primary_font:?} primary_font={:?} configured_font_size_points={configured_font_size} effective_font_size_points={} font_size_offset_points={}",
            self.appearance.stable_hash(),
            self.appearance.color_scheme.as_str(),
            self.appearance.font_families.first(),
            self.appearance.font_size_points,
            self.terminal_font_size_offset_points,
        );
    }

    fn forget_pane(&mut self, pane: PaneId) {
        self.viewports.remove(&pane);
        self.clear_kitty_images(pane);
        let connection = self.attached_connection_mut();
        connection.full_requests_pending.remove(&pane);
        connection.history_requests_pending.remove(&pane);
        connection.history_backfill_deferred.remove(&pane);
        self.browser_commands.remove(&pane);
        self.terminal_commands.remove(&pane);
        #[cfg(feature = "agent-pane")]
        self.agent_cursors.remove(&pane);
        #[cfg(feature = "agent-pane")]
        self.agent_replays_pending.remove(&pane);
        self.pane_images.remove(&pane);
        self.pasted_image_assemblies
            .retain(|(target, _), _| *target != pane);
        self.pending_pasted_image_previews
            .retain(|(target, _)| *target != pane);
        for (request_id, _) in self.agent_commands.remove(&pane).unwrap_or_default() {
            self.respond_to_request(GuiResponse::Error {
                request_id,
                message: format!("{pane} was closed"),
            });
        }
        let mut removed_screenshots = Vec::new();
        self.screenshot_requests.retain(|(target, request_id, _)| {
            let keep = *target != pane;
            if !keep {
                removed_screenshots.push(*request_id);
            }
            keep
        });
        for request_id in removed_screenshots {
            self.respond_to_request(GuiResponse::Error {
                request_id,
                message: format!("{pane} was closed"),
            });
        }
    }

    fn route_open_uri(&mut self, pane: PaneId, uri: &str, cx: &mut Context<Self>) {
        let route = open_uri_route(
            self.core.snapshot(),
            self.core.attached_session(),
            pane,
            uri,
        );
        match route {
            OpenUriRoute::Embedded { pane: browser, url } => {
                log::debug!(
                    "routing terminal link from {pane} to embedded browser {browser}: {}",
                    diagnostic_url(&url)
                );
                self.browser_commands
                    .entry(browser)
                    .or_default()
                    .push(BrowserCommand::Navigate(url));
                self.pending_commands_revision =
                    self.pending_commands_revision.wrapping_add(1).max(1);
            }
            OpenUriRoute::PastedImage { pane, number } => self.open_pasted_image(pane, number, cx),
            OpenUriRoute::External => cx.open_url(uri),
        }
    }

    fn handle_command_response(
        &mut self,
        host: HostId,
        response: CommandResponse,
        cx: &mut Context<Self>,
    ) {
        match response {
            CommandResponse::Error {
                request_id: 0,
                error: ServerError::MissingTarget(_),
            } if self.retry_default_after_missing_session() => {}
            CommandResponse::Error {
                request_id: 0,
                error: ServerError::MissingTarget(_),
            } if self.core.attached_session().is_none()
                && self.core.snapshot().sessions.is_empty() =>
            {
                self.error = None;
                let connection = self.attached_connection_mut();
                if !connection.resync_pending
                    && let Some(client) = &connection.client
                {
                    if let Err(error) = client.request_resync() {
                        self.error = Some(Arc::from(error.to_string()));
                    } else {
                        connection.resync_pending = true;
                    }
                }
            }
            CommandResponse::Error {
                request_id: 0,
                error: ServerError::PaneExited(pane) | ServerError::PaneNotAttached(pane),
            } => log::debug!("ignoring stale input for detached pane {pane}"),
            CommandResponse::Error { request_id, error } => {
                if !self.report_command_failure(host, request_id, &error, cx) {
                    self.attached_connection_mut().reconnect_attach = None;
                    self.error = Some(Arc::from(error.to_string()));
                }
            }
            CommandResponse::Success { request_id, .. } => {
                self.attached_connection().take_command(request_id);
                self.error = None;
            }
        }
    }

    /// Inbound messages the core does not reduce. Pasted-image previews are
    /// assembled here because they end as GPUI `Image`s, not protocol state.
    fn handle_unreduced_message(&mut self, message: ProtocolMessage, cx: &mut Context<Self>) {
        match message {
            ProtocolMessage::PastedImageBegin {
                pane,
                number,
                format,
                total_bytes,
            } => self.begin_pasted_image(pane, number, format, total_bytes),
            ProtocolMessage::PastedImageChunk {
                pane,
                number,
                bytes,
            } => self.push_pasted_image_chunk(pane, number, &bytes, cx),
            ProtocolMessage::PastedImageUnavailable { pane, number } => {
                self.pasted_image_unavailable(pane, number);
            }
            _ => {}
        }
    }

    fn apply_terminal_viewport(&mut self, pane: PaneId, viewport: TerminalViewport) {
        self.kitty_image_cache(pane);
        self.viewports.insert(
            pane,
            Arc::new(RwLock::new(new_retained_viewport(
                viewport,
                &mut self.next_row_revision,
            ))),
        );
        let connection = self.attached_connection_mut();
        connection.full_requests_pending.remove(&pane);
        connection.history_requests_pending.remove(&pane);
        connection.history_backfill_deferred.remove(&pane);
        self.request_history_backfill(pane);
    }

    fn apply_terminal_patch(&mut self, pane: PaneId, patch: TerminalViewportPatch) {
        self.kitty_image_cache(pane);
        let retained = self.viewports.get(&pane).cloned();
        let request_missing =
            retained.is_none() && snapshot_contains_pane(self.core.snapshot(), pane);
        let request_failed_patch = retained.is_some();
        let apply_result = if let Some(retained) = retained {
            let mut retained = retained.write();
            if retained.row_revisions.len() == usize::from(patch.rows) {
                apply_retained_patch(&mut retained, patch, &mut self.next_row_revision).map_err(
                    |error| {
                        log::warn!("rejected terminal patch for {pane}: {error}");
                    },
                )
            } else {
                log::warn!("terminal row revisions are out of sync for {pane}");
                Err(())
            }
        } else {
            log::warn!("received terminal patch for missing pane {pane}");
            Err(())
        };
        if apply_result.is_err() && (request_failed_patch || request_missing) {
            self.request_full_viewport(pane);
        } else if apply_result.is_ok() {
            self.request_history_backfill(pane);
        }
    }

    fn request_full_viewport(&mut self, pane: PaneId) {
        let connection = self.attached_connection_mut();
        if connection.full_requests_pending.insert(pane)
            && let Some(client) = &connection.client
            && let Err(error) = client.request_full(pane)
        {
            connection.full_requests_pending.remove(&pane);
            log::warn!("failed to request a full terminal viewport for {pane}: {error}");
        }
    }

    fn apply_command_output(&mut self, pane: PaneId, viewport: Option<TerminalViewport>) {
        let Some(viewport) = viewport else {
            let output_pane = self
                .command_output
                .as_ref()
                .map_or(pane, |output| output.pane);
            self.command_output = None;
            self.command_output_diff.invalidate();
            self.terminal_commands.remove(&output_pane);
            return;
        };
        let Some(output) = self
            .command_output
            .as_ref()
            .filter(|output| output.pane == pane)
        else {
            self.command_output_diff.invalidate();
            self.command_output = Some(CommandOutputModel {
                pane,
                retained: Arc::new(RwLock::new(new_retained_viewport(
                    viewport,
                    &mut self.next_row_revision,
                ))),
            });
            return;
        };
        let mut retained = output.retained.write();
        let patch = TerminalViewport::diff_with_scratch(
            &retained.viewport,
            &viewport,
            &mut self.command_output_diff,
        );
        if let Some(patch) = patch {
            if let Err(error) =
                apply_retained_patch(&mut retained, patch, &mut self.next_row_revision)
            {
                log::warn!("could not retain command-output rows for {pane}: {error}");
                replace_retained_viewport(&mut retained, viewport, &mut self.next_row_revision);
                self.command_output_diff.invalidate();
            } else {
                self.command_output_diff
                    .remember_applied(&retained.viewport);
            }
        } else {
            replace_retained_viewport(&mut retained, viewport, &mut self.next_row_revision);
            self.command_output_diff.invalidate();
        }
    }

    fn apply_history_chunk_event(
        &mut self,
        pane: PaneId,
        start: u32,
        total: u32,
        offset: u32,
        columns: u16,
        rows: Vec<Vec<PackedCell>>,
        dictionary: TerminalDictionary,
        cx: &mut Context<Self>,
    ) {
        let pending = self
            .attached_connection_mut()
            .history_requests_pending
            .remove(&pane);
        let mut deferred_mutations = None;
        if let Some(retained) = self.viewports.get(&pane) {
            let mut retained = retained.write();
            if pending.map(|request| request.mutations) == Some(retained.history_mutations) {
                apply_history_chunk(
                    &mut retained,
                    start,
                    total,
                    offset,
                    columns,
                    rows,
                    dictionary,
                    &mut self.next_row_revision,
                );
            } else if matches!(
                pending,
                Some(PendingHistoryRequest {
                    prefetch_target: None,
                    ..
                })
            ) {
                deferred_mutations = Some(retained.history_mutations);
            }
        }
        if let Some(target) = pending.and_then(|request| request.prefetch_target) {
            self.request_history_prefetch(pane, target);
        } else if let Some(mutations) = deferred_mutations {
            self.defer_history_backfill(pane, mutations, cx);
        } else {
            self.request_history_backfill(pane);
        }
    }

    fn record_bell(&mut self, host: HostId, pane: PaneId) {
        log::debug!(target: "zz::diagnostics::mux", "bell rang host={host:?} pane={pane}");
        self.bell_revision = self.bell_revision.wrapping_add(1).max(1);
    }
}

impl EventEmitter<ClientNotification> for MuxClient {}

impl EventEmitter<AttachmentPreviewRequest> for MuxClient {}

impl EventEmitter<SshPromptRequest> for MuxClient {}

fn adjusted_terminal_font_size(current: f32, adjustment: TerminalFontSizeAdjustment) -> f32 {
    (current + adjustment.delta_points())
        .clamp(MIN_TERMINAL_FONT_SIZE_POINTS, MAX_TERMINAL_FONT_SIZE_POINTS)
}

fn apply_terminal_font_size_offset(appearance: &mut TerminalAppearance, offset_points: f32) -> f32 {
    let configured = appearance.font_size_points;
    let effective = (configured + offset_points)
        .clamp(MIN_TERMINAL_FONT_SIZE_POINTS, MAX_TERMINAL_FONT_SIZE_POINTS);
    appearance.font_size_points = effective;
    effective - configured
}

fn history_chunk_is_valid(
    columns: u16,
    rows: &[Vec<PackedCell>],
    dictionary: &TerminalDictionary,
) -> bool {
    if rows.is_empty()
        || rows.len() > usize::try_from(MAX_HISTORY_CHUNK_ROWS).unwrap_or(usize::MAX)
        || rows.iter().any(|row| row.len() != usize::from(columns))
        || dictionary.grapheme_offsets.first() != Some(&0)
        || dictionary
            .grapheme_offsets
            .windows(2)
            .any(|offsets| offsets[0] > offsets[1])
        || usize::try_from(dictionary.grapheme_offsets.last().copied().unwrap_or(0))
            .unwrap_or(usize::MAX)
            != dictionary.grapheme_bytes.len()
    {
        return false;
    }
    for offsets in dictionary.grapheme_offsets.windows(2) {
        let Some(bytes) = dictionary
            .grapheme_bytes
            .get(offsets[0] as usize..offsets[1] as usize)
        else {
            return false;
        };
        if std::str::from_utf8(bytes).is_err() {
            return false;
        }
    }
    rows.iter().flatten().all(|cell| {
        if usize::from(cell.style_id()) >= dictionary.styles.len() {
            return false;
        }
        let glyph = cell.glyph();
        if glyph == 0 {
            return true;
        }
        if glyph & GRAPHEME_TABLE_BIT == 0 {
            return char::from_u32(glyph).is_some();
        }
        let index = usize::try_from(glyph & !GRAPHEME_TABLE_BIT).unwrap_or(usize::MAX);
        index.saturating_add(1) < dictionary.grapheme_offsets.len()
    })
}

fn apply_history_chunk(
    retained: &mut RetainedTerminalViewport,
    start: u32,
    total: u32,
    offset: u32,
    columns: u16,
    rows: Vec<Vec<PackedCell>>,
    dictionary: TerminalDictionary,
    next_row_revision: &mut u64,
) -> bool {
    let Ok(retained_rows) = u32::try_from(retained.history.len()) else {
        return false;
    };
    let Some(front) = retained.history_scrollbar.offset.checked_sub(retained_rows) else {
        return false;
    };
    let Some(end) = start.checked_add(u32::try_from(rows.len()).unwrap_or(u32::MAX)) else {
        return false;
    };
    if columns != retained.viewport.columns
        || total != retained.history_scrollbar.total
        || offset != retained.history_scrollbar.offset
        || end != front
        || !history_chunk_is_valid(columns, &rows, &dictionary)
    {
        return false;
    }
    retained
        .history
        .prepend(rows, dictionary, next_row_revision);
    retained.row_revision_epoch = allocate_row_revision(next_row_revision);
    true
}

fn allocate_row_revisions(counter: &mut u64, rows: usize) -> Vec<u64> {
    (0..rows).map(|_| allocate_row_revision(counter)).collect()
}

fn allocate_row_revision(counter: &mut u64) -> u64 {
    let revision = *counter;
    *counter = counter.wrapping_add(1).max(1);
    revision
}

fn new_retained_viewport(
    viewport: TerminalViewport,
    next_row_revision: &mut u64,
) -> RetainedTerminalViewport {
    let row_revisions = allocate_row_revisions(next_row_revision, usize::from(viewport.rows));
    let history_scrollbar = viewport.scrollbar;
    RetainedTerminalViewport {
        viewport,
        history: HistoryRing::default(),
        history_scrollbar,
        history_mutations: 0,
        history_invalidations: 0,
        row_revisions: row_revisions.into_boxed_slice(),
        row_revision_epoch: allocate_row_revision(next_row_revision),
        revision_scratch: Vec::new(),
    }
}

fn drop_retained_history(retained: &mut RetainedTerminalViewport) {
    retained.history.clear();
    retained.history_mutations = retained.history_mutations.wrapping_add(1);
    retained.history_invalidations = retained.history_invalidations.wrapping_add(1);
}

fn shift_rows(revisions: &mut [u64], scroll: i16) {
    let rows = revisions.len();
    let shift = isize::from(scroll);
    if shift > 0 {
        let shift = shift.unsigned_abs();
        revisions.copy_within(0..rows - shift, shift);
    } else if shift < 0 {
        let shift = shift.unsigned_abs();
        revisions.copy_within(shift..rows, 0);
    }
}

fn apply_retained_patch(
    retained: &mut RetainedTerminalViewport,
    patch: TerminalViewportPatch,
    next_row_revision: &mut u64,
) -> Result<(), zz_terminal::PatchError> {
    let scroll = patch.scroll;
    let previous_scrollbar = retained.history_scrollbar;
    let next_scrollbar = patch.scrollbar;
    let rows = usize::from(patch.rows);
    let shift = usize::from(scroll.unsigned_abs());
    let total_delta = next_scrollbar.total.checked_sub(previous_scrollbar.total);
    let offset_forward = next_scrollbar.offset.checked_sub(previous_scrollbar.offset);
    let offset_reverse = previous_scrollbar.offset.checked_sub(next_scrollbar.offset);
    let full_row_replacement = scroll == 0
        && rows != 0
        && patch.changed_rows.len() == rows
        && patch.generation != patch.base_generation;
    let mut invalidate_history =
        patch.columns != retained.viewport.columns || total_delta.is_none() || full_row_replacement;
    let mut departing_rows = Vec::new();

    if !invalidate_history {
        match scroll.cmp(&0) {
            std::cmp::Ordering::Less => {
                let shift_u32 = u32::try_from(shift).unwrap_or(u32::MAX);
                invalidate_history = total_delta.is_none_or(|delta| delta > shift_u32)
                    || offset_forward.is_none_or(|delta| delta > shift_u32);
                if !invalidate_history {
                    let dictionary = Arc::clone(&retained.viewport.dictionary);
                    for row in 0..shift {
                        let Some(cells) = u16::try_from(row)
                            .ok()
                            .and_then(|row| retained.viewport.row(row))
                        else {
                            invalidate_history = true;
                            departing_rows.clear();
                            break;
                        };
                        departing_rows.push(HistoryRow {
                            cells: Box::from(cells),
                            dictionary: Arc::clone(&dictionary),
                            revision: allocate_row_revision(next_row_revision),
                        });
                    }
                }
            }
            std::cmp::Ordering::Greater => {
                let shift_u32 = u32::try_from(shift).unwrap_or(u32::MAX);
                invalidate_history = total_delta != Some(0)
                    || offset_reverse.is_none_or(|delta| delta > shift_u32)
                    || retained.history.len() < shift;
            }
            std::cmp::Ordering::Equal => {
                invalidate_history = previous_scrollbar.offset != next_scrollbar.offset;
            }
        }
    }
    retained.revision_scratch.clear();
    retained
        .revision_scratch
        .extend_from_slice(patch.changed_rows.row_indices());
    if let Err(error) = retained.viewport.apply_patch(patch) {
        retained.revision_scratch.clear();
        return Err(error);
    }
    if invalidate_history {
        drop_retained_history(retained);
    } else if scroll < 0 {
        for row in departing_rows {
            retained.history.push_back(row);
        }
        let advanced = next_scrollbar
            .offset
            .saturating_sub(retained.history_scrollbar.offset);
        let evicted = shift.saturating_sub(usize::try_from(advanced).unwrap_or(0));
        for _ in 0..evicted {
            retained.history.rows.pop_front();
        }
        retained.history.enforce_cap();
        retained.history_mutations = retained.history_mutations.wrapping_add(1);
    } else if scroll > 0 {
        for _ in 0..shift {
            retained.history.rows.pop_back();
        }
        retained.history_mutations = retained.history_mutations.wrapping_add(1);
    }
    retained.history_scrollbar = next_scrollbar;
    if scroll != 0 || !retained.revision_scratch.is_empty() {
        shift_rows(&mut retained.row_revisions, scroll);
        for row in retained.revision_scratch.iter().copied() {
            retained.row_revisions[usize::from(row)] = allocate_row_revision(next_row_revision);
        }
        retained.row_revision_epoch = allocate_row_revision(next_row_revision);
    }
    retained.revision_scratch.clear();
    Ok(())
}

fn replace_retained_viewport(
    retained: &mut RetainedTerminalViewport,
    viewport: TerminalViewport,
    next_row_revision: &mut u64,
) {
    let rows = usize::from(viewport.rows);
    if retained.row_revisions.len() == rows {
        for revision in &mut retained.row_revisions {
            *revision = allocate_row_revision(next_row_revision);
        }
    } else {
        retained.row_revisions = allocate_row_revisions(next_row_revision, rows).into_boxed_slice();
    }
    retained.row_revision_epoch = allocate_row_revision(next_row_revision);
    retained.revision_scratch.clear();
    drop_retained_history(retained);
    retained.history_scrollbar = viewport.scrollbar;
    retained.viewport = viewport;
}

#[cfg(test)]
#[allow(
    clippy::arc_with_non_send_sync,
    reason = "the fake host is Cell-based and fills a slot typed for the real Arc'd client"
)]
mod tests {
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        rc::Rc,
        time::{Duration, Instant},
    };

    #[cfg(unix)]
    use std::{
        path::PathBuf,
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
    };

    use gpui::{AppContext as _, TestAppContext};
    use zz_protocol::{
        Axis, BrowserDescriptor, MuxOptions, PaneSnapshot, SessionSnapshot, SplitId, WindowId,
        WindowSnapshot,
    };
    #[cfg(unix)]
    use zz_protocol::{read_protocol_message, write_protocol_message};
    use zz_terminal::{
        AppearanceProvenance, CellWidth, OverlayKind, OverlaySpan, PackedCell, SessionStatus,
    };

    use super::*;

    #[cfg(unix)]
    static TEST_SOCKET_ID: AtomicU64 = AtomicU64::new(1);
    #[cfg(unix)]
    const M3_DAEMON_CHILD_SOCKET: &str = "ZZ_M3_DAEMON_CHILD_SOCKET";
    #[cfg(unix)]
    const M3_DAEMON_SANDBOX_BLOCKED_EXIT: i32 = 77;

    fn test_host(name: &str, endpoint: &str) -> crate::config::HostEntry {
        crate::config::HostEntry {
            name: name.to_owned(),
            endpoint: zz_daemon::Endpoint::parse(endpoint).expect("test endpoint"),
        }
    }

    fn test_server_hello() -> ServerHello {
        ServerHello {
            protocol_version: PROTOCOL_VERSION,
            server_id: 1,
            client_id: zz_protocol::ClientId(1),
            client_instance_id: zz_protocol::ClientInstanceId(1),
            capabilities: Vec::new(),
            appearance: TerminalAppearance::default(),
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: MuxOptions::default(),
            status: StatusLine::default(),
            key_tables: Vec::new(),
        }
    }

    /// Reduce a snapshot into the core the way a daemon event would, for a
    /// test that needs one standing before it drives the client.
    fn seed_snapshot(mux: &mut MuxClient, snapshot: MuxSnapshot) {
        mux.seed_core(EventPayload::Snapshot(snapshot));
    }

    /// Reduce an attachment into the core, session and snapshot together.
    fn seed_attachment(mux: &mut MuxClient, session: SessionId, snapshot: MuxSnapshot) {
        mux.core
            .handle_message(ProtocolMessage::Attached { session, snapshot });
        mux.discard_core_effects();
    }

    fn seed_choose_tree(mux: &mut MuxClient) {
        mux.seed_core(EventPayload::ChooseTree {
            state: Some(ChooseTreeState {
                items: Vec::new(),
                search: None,
                selected: 0,
                kind: zz_protocol::ChooseTreeKind::Windows,
            }),
        });
    }

    fn install_fake_connection(mux: &mut MuxClient, host: HostId) -> Arc<FakeConnectedHost> {
        let fake = Arc::new(FakeConnectedHost::new(test_server_hello()));
        let connection = mux.connections.get_mut(&host).expect("host connection");
        connection.client = None;
        connection.fake_client = Some(Arc::clone(&fake));
        connection.reader_thread = None;
        connection.state = HostState::Connected;
        fake
    }

    fn record_notifications(
        mux: &gpui::Entity<MuxClient>,
        cx: &mut gpui::App,
    ) -> (
        gpui::Entity<()>,
        Rc<RefCell<Vec<(ClientMessageKind, String)>>>,
    ) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&events);
        let mux = mux.clone();
        let sink = cx.new(|cx| {
            cx.subscribe(&mux, move |(), _, event: &ClientNotification, _| {
                observed.borrow_mut().push((event.kind, event.text.clone()));
            })
            .detach();
        });
        (sink, events)
    }

    #[cfg(unix)]
    struct RunningTestDaemon {
        socket: PathBuf,
        command: Option<zz_daemon::CommandClient>,
        thread: Option<thread::JoinHandle<Result<(), DaemonError>>>,
    }

    #[cfg(unix)]
    impl RunningTestDaemon {
        fn start() -> Option<Self> {
            let socket = PathBuf::from(format!(
                "/tmp/zz-it-{}-{}.sock",
                std::process::id(),
                TEST_SOCKET_ID.fetch_add(1, Ordering::Relaxed),
            ));
            let _ = std::fs::remove_file(&socket);
            let daemon = zz_daemon::Daemon::new(&socket).without_user_config();
            let thread = thread::spawn(move || daemon.run_foreground());
            let deadline = Instant::now() + Duration::from_secs(30);
            let command = loop {
                match zz_daemon::CommandClient::connect(&socket) {
                    Ok(client) => break client,
                    Err(_) if thread.is_finished() => {
                        let result = thread.join().expect("join failed test daemon");
                        if matches!(
                            &result,
                            Err(DaemonError::Io(error))
                                if error.kind() == io::ErrorKind::PermissionDenied
                        ) {
                            return None;
                        }
                        panic!("test daemon exited during startup: {result:?}");
                    }
                    Err(error) if Instant::now() >= deadline => {
                        panic!("daemon did not start: {error}")
                    }
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            };
            Some(Self {
                socket,
                command: Some(command),
                thread: Some(thread),
            })
        }

        fn connect_interactive(&self) -> InteractiveClient {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                match InteractiveClient::connect(&self.socket) {
                    Ok(client) => return client,
                    Err(error) if Instant::now() >= deadline => {
                        panic!("daemon did not accept an interactive client: {error}")
                    }
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
        }

        fn stop(&mut self) {
            if self.thread.is_none() {
                return;
            }
            self.command
                .as_mut()
                .expect("running daemon command client")
                .execute(CommandInvocation::new("kill-server", [] as [&str; 0]))
                .expect("stop test daemon");
            self.command = None;
            self.thread
                .take()
                .expect("running daemon thread")
                .join()
                .expect("join test daemon")
                .expect("test daemon exits cleanly");
            let _ = std::fs::remove_file(&self.socket);
        }
    }

    #[cfg(unix)]
    impl Drop for RunningTestDaemon {
        fn drop(&mut self) {
            if self.thread.is_some() {
                if let Some(command) = &mut self.command {
                    let _ = command.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
                }
                self.command = None;
                if let Some(thread) = self.thread.take() {
                    let _ = thread.join();
                }
            }
            let _ = std::fs::remove_file(&self.socket);
        }
    }

    #[cfg(unix)]
    struct RunningProcessTestDaemon {
        socket: PathBuf,
        test_name: &'static str,
        child: Option<std::process::Child>,
    }

    #[cfg(unix)]
    impl RunningProcessTestDaemon {
        fn start(socket: PathBuf, test_name: &'static str) -> Option<Self> {
            let mut daemon = Self {
                socket,
                test_name,
                child: None,
            };
            daemon.restart().then_some(daemon)
        }

        fn restart(&mut self) -> bool {
            self.stop();
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg(self.test_name)
                .arg("--nocapture")
                .env(M3_DAEMON_CHILD_SOCKET, &self.socket)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn process-backed test daemon");
            self.child = Some(child);

            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                match zz_daemon::CommandClient::connect(&self.socket) {
                    Ok(_) => return true,
                    Err(_) => {
                        if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                            let sandbox_blocked =
                                status.code() == Some(M3_DAEMON_SANDBOX_BLOCKED_EXIT);
                            self.stop();
                            if sandbox_blocked {
                                return false;
                            }
                            panic!("process-backed test daemon exited during startup");
                        }
                    }
                }
                if Instant::now() >= deadline {
                    let Err(error) = zz_daemon::CommandClient::connect(&self.socket) else {
                        return true;
                    };
                    panic!("process-backed test daemon did not start: {error}");
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn stop(&mut self) {
            let Some(mut child) = self.child.take() else {
                let _ = std::fs::remove_file(&self.socket);
                return;
            };
            if child.try_wait().expect("inspect test daemon").is_none() {
                child.kill().expect("kill process-backed test daemon");
            }
            child.wait().expect("reap process-backed test daemon");
            let _ = std::fs::remove_file(&self.socket);
        }
    }

    #[cfg(unix)]
    impl Drop for RunningProcessTestDaemon {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn wait_for_mux(
        cx: &mut TestAppContext,
        mux: &gpui::Entity<MuxClient>,
        description: &str,
        mut predicate: impl FnMut(&MuxClient) -> bool,
    ) {
        const DEADLINE: Duration = Duration::from_mins(1);

        let deadline = Instant::now() + DEADLINE;
        loop {
            cx.run_until_parked();
            if cx.update(|cx| predicate(mux.read(cx))) {
                return;
            }
            if Instant::now() >= deadline {
                let details = cx.update(|cx| {
                    let mux = mux.read(cx);
                    let hosts: Vec<String> = mux
                        .connections
                        .iter()
                        .map(|(host, connection)| {
                            format!(
                                "{host:?}: {:?} reconnect_in_flight={:?} snapshot_sessions={:?}",
                                connection.state,
                                connection.reconnect_attempt_in_flight,
                                connection
                                    .snapshot
                                    .as_ref()
                                    .map(|snapshot| snapshot.sessions.len()),
                            )
                        })
                        .collect();
                    format!("error={:?}; {}", mux.error, hosts.join("; "))
                });
                panic!("timed out waiting for {description}: {details}");
            }
            cx.executor().advance_clock(Duration::from_millis(100));
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn connected_test_client() -> Option<(
        InteractiveClient,
        thread::JoinHandle<Result<ProtocolMessage, ProtocolError>>,
    )> {
        use std::os::unix::net::UnixListener;

        let socket = std::env::temp_dir().join(format!(
            "z3b-{}-{}.sock",
            std::process::id(),
            TEST_SOCKET_ID.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = std::fs::remove_file(&socket);
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return None,
            Err(error) => panic!("bind test protocol socket: {error}"),
        };
        let server_socket = socket.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test client");
            assert!(matches!(
                read_protocol_message(&mut stream).expect("read ClientHello"),
                ProtocolMessage::ClientHello(_)
            ));
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::ServerHello(test_server_hello()),
            )
            .expect("write ServerHello");
            let resync = read_protocol_message(&mut stream);
            drop(stream);
            let _ = std::fs::remove_file(server_socket);
            resync
        });
        let client = InteractiveClient::connect_endpoint(
            &zz_daemon::Endpoint::Local(socket),
            TerminalColorScheme::Dark,
        )
        .expect("connect test client");
        Some((client, server))
    }

    #[test]
    fn reconnect_backoff_caps_at_thirty_seconds() {
        assert_eq!(
            (1..=8).map(reconnect_delay).collect::<Vec<_>>(),
            [
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
                Duration::from_secs(16),
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(30),
            ]
        );
    }

    #[test]
    fn stale_reconnect_generation_does_not_match_the_armed_timer() {
        let mut connection = HostConnection::disconnected(HostId::LOCAL);
        connection.state = HostState::Reconnecting { attempt: 2 };
        let armed = connection.bump_reconnect_generation();
        assert!(reconnect_timer_is_current(&connection, armed, 2));

        connection.bump_reconnect_generation();
        assert!(!reconnect_timer_is_current(&connection, armed, 2));
    }

    #[test]
    fn ssh_dial_failures_reach_the_host_row_as_advice() {
        for (error, expected) in [
            (
                EndpointError::RemoteBinaryMissing {
                    target: "ssh://desk".to_owned(),
                },
                "zz is not installed on ssh://desk",
            ),
            (
                EndpointError::SshFailed {
                    target: "ssh://desk".to_owned(),
                    reason: "ssh rejected the login".to_owned(),
                },
                "ssh rejected the login",
            ),
            (
                EndpointError::RemoteDaemonUnavailable {
                    target: "ssh://desk".to_owned(),
                },
                "never appeared",
            ),
        ] {
            let state = connect_result_state(&Err::<(), _>(DaemonError::from(error)));
            let HostState::Unreachable { reason } = state else {
                panic!("ssh dial failures belong in an unreachable row, got {state:?}");
            };
            assert!(
                reason.contains(expected),
                "{reason:?} should say {expected:?}"
            );
        }
    }

    #[test]
    fn classified_daemon_versions_map_without_inventing_v0() {
        assert_eq!(
            connect_result_state(&Err::<(), _>(DaemonError::IncompatibleDaemon {
                daemon: Some(41),
                client: PROTOCOL_VERSION,
            })),
            HostState::Incompatible {
                local: PROTOCOL_VERSION,
                remote: 41,
            }
        );
        let state = connect_result_state(&Err::<(), _>(DaemonError::IncompatibleDaemon {
            daemon: None,
            client: PROTOCOL_VERSION,
        }));
        let HostState::Unreachable { reason } = state else {
            panic!("an unknown daemon protocol must remain readable, got {state:?}");
        };
        assert!(reason.contains("older than this zz"));
        assert!(!reason.contains("v0"));

        assert_eq!(
            connect_result_state(&Err::<(), _>(DaemonError::from(
                EndpointError::RemoteProtocolMismatch {
                    target: "host".to_owned(),
                    daemon: 40,
                    client: PROTOCOL_VERSION,
                }
            ))),
            HostState::Incompatible {
                local: PROTOCOL_VERSION,
                remote: 40,
            }
        );

        let state = connect_result_state(&Err::<(), _>(DaemonError::from(
            EndpointError::RemoteProtocolUnknown {
                target: "host".to_owned(),
            },
        )));
        assert_eq!(
            state,
            HostState::Unreachable {
                reason: "zz on host predates protocol version reporting; update it, then reconnect"
                    .to_owned(),
            }
        );
    }

    #[gpui::test]
    fn initial_stale_daemon_state_is_structured_and_dismissal_adds_the_manual_hint(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::IncompatibleDaemon {
                        daemon: Some(41),
                        client: PROTOCOL_VERSION,
                    }),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            assert_eq!(
                mux.read(cx).stale_daemon(),
                Some(StaleDaemonInfo { daemon: Some(41) })
            );
            mux.update(cx, super::MuxClient::dismiss_stale_daemon);
            assert!(mux.read(cx).stale_daemon().is_none());
            let error = mux.read(cx).error().unwrap();
            assert!(error.contains("daemon speaks protocol v41"));
            assert!(error.contains("Run 'zz kill-server' to restart it manually."));
        });
    }

    #[gpui::test]
    fn handle_connect_result_maps_success_unreachable_and_version_skew(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        assert_eq!(
            connect_result_state(&Ok::<(), DaemonError>(())),
            HostState::Connected
        );
        #[cfg(unix)]
        let (connected, resync_reader) = connected_test_client()
            .map_or((None, None), |(client, reader)| {
                (Some(client), Some(reader))
            });
        #[cfg(not(unix))]
        let connected = None;
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-step3b-remote.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux
                .read(cx)
                .registry
                .get_by_name("remote")
                .expect("remote host")
                .0;

            mux.update(cx, |mux, cx| {
                if let Some(connected) = connected {
                    mux.connections.get_mut(&remote).unwrap().state = HostState::Connecting;
                    mux.handle_connect_result(remote, Ok(connected), cx);
                    assert_eq!(
                        mux.connections.get(&remote).unwrap().state,
                        HostState::Connected
                    );
                }

                mux.connections.get_mut(&remote).unwrap().state = HostState::Connecting;
                mux.handle_connect_result(
                    remote,
                    Err(DaemonError::Thread("remote offline".to_owned())),
                    cx,
                );
                assert_eq!(
                    mux.connections.get(&remote).unwrap().state.label(),
                    "unreachable"
                );
                let HostState::Unreachable { reason } =
                    &mux.connections.get(&remote).unwrap().state
                else {
                    panic!("generic failure should be unreachable");
                };
                assert!(reason.contains("remote offline"));

                mux.connections.get_mut(&remote).unwrap().state = HostState::Connecting;
                mux.handle_connect_result(
                    remote,
                    Err(DaemonError::Protocol(ProtocolError::VersionMismatch {
                        expected: PROTOCOL_VERSION,
                        received: 41,
                    })),
                    cx,
                );
                assert_eq!(
                    mux.connections.get(&remote).unwrap().state,
                    HostState::Incompatible {
                        local: PROTOCOL_VERSION,
                        remote: 41,
                    }
                );

                mux.connections.get_mut(&remote).unwrap().state = HostState::Connecting;
                mux.handle_connect_result(
                    remote,
                    Err(DaemonError::Io(io::Error::other(
                        ProtocolError::VersionMismatch {
                            expected: PROTOCOL_VERSION,
                            received: 42,
                        },
                    ))),
                    cx,
                );
                assert_eq!(
                    mux.connections.get(&remote).unwrap().state,
                    HostState::Incompatible {
                        local: PROTOCOL_VERSION,
                        remote: 42,
                    }
                );
            });
        });
        #[cfg(unix)]
        if let Some(resync_reader) = resync_reader {
            assert!(matches!(
                resync_reader
                    .join()
                    .expect("join test protocol server")
                    .expect("read initial resync"),
                ProtocolMessage::Resync
            ));
        }
    }

    #[gpui::test]
    fn non_attached_host_disconnect_starts_a_capped_reconnect_loop(cx: &mut TestAppContext) {
        let (mux, remote, _sink, notifications) = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-step5-remote.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("local unavailable".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            mux.update(cx, |mux, _| {
                install_fake_connection(mux, remote);
                mux.error = Some(Arc::from("keep this error"));
                seed_snapshot(
                    mux,
                    MuxSnapshot {
                        generation: 7,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );
            });
            let (sink, notifications) = record_notifications(&mux, cx);
            mux.update(cx, |mux, cx| {
                mux.handle_host_disconnected(remote, cx);
            });
            (mux, remote, sink, notifications)
        });

        assert!(notifications.borrow().is_empty());
        cx.update(|cx| {
            let mux = mux.read(cx);
            assert_eq!(
                mux.connections.get(&remote).unwrap().state,
                HostState::Reconnecting { attempt: 1 }
            );
            assert_eq!(mux.attached_host, HostId::LOCAL);
            assert_eq!(mux.error.as_deref(), Some("keep this error"));
            assert_eq!(mux.core.snapshot().generation, 7);
            assert!(mux.error_after_next_attach.is_none());
        });
    }

    #[gpui::test]
    fn attached_remote_disconnect_freezes_the_frame_and_remembers_the_session(
        cx: &mut TestAppContext,
    ) {
        let (_mux, _sink, notifications) = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-step5-remote.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("initial error".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            let (sink, notifications) = record_notifications(&mux, cx);
            mux.update(cx, |mux, cx| {
                let local = install_fake_connection(mux, HostId::LOCAL);
                install_fake_connection(mux, remote);
                mux.attached_host = remote;
                seed_attachment(
                    mux,
                    SessionId(9),
                    MuxSnapshot {
                        generation: 7,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );
                let snapshot = Arc::clone(mux.core.snapshot());
                let pane = PaneId(23);
                let viewport = Arc::new(RwLock::new(new_retained_viewport(
                    TerminalViewport::blank(2, 2, SessionStatus::Running),
                    &mut mux.next_row_revision,
                )));
                mux.viewports.insert(pane, Arc::clone(&viewport));
                seed_choose_tree(mux);
                mux.error = Some(Arc::from("stale daemon error"));

                mux.handle_host_disconnected(remote, cx);

                assert_eq!(
                    mux.connections.get(&remote).unwrap().state,
                    HostState::Reconnecting { attempt: 1 }
                );
                let connection = mux.connections.get(&remote).unwrap();
                assert_eq!(connection.last_attached_session, Some(SessionId(9)));
                assert!(connection.reconnect_generation > 0);
                assert_eq!(mux.attached_host, remote);
                assert_eq!(mux.core.attached_session(), Some(SessionId(9)));
                assert!(Arc::ptr_eq(mux.core.snapshot(), &snapshot));
                assert!(Arc::ptr_eq(
                    mux.viewports.get(&pane).expect("frozen viewport"),
                    &viewport
                ));
                assert!(mux.core.choose_tree().is_none());
                assert!(mux.error.is_none());
                assert!(mux.error_after_next_attach.is_none());
                assert!(
                    !local.attached_default.get(),
                    "an involuntary remote disconnect must not fall back to local"
                );
            });
            (mux, sink, notifications)
        });
        assert_eq!(
            &*notifications.borrow(),
            &[(
                ClientMessageKind::Warning,
                "connection to remote lost: reconnecting…".to_owned(),
            )]
        );
    }

    #[gpui::test]
    fn attached_remote_disconnect_reconnects_even_when_local_is_down(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-step5-remote.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("local unavailable".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, remote);
                mux.attached_host = remote;
                seed_attachment(
                    mux,
                    SessionId(9),
                    MuxSnapshot {
                        generation: 7,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );
                seed_choose_tree(mux);
                mux.error = None;

                mux.handle_host_disconnected(remote, cx);

                assert_eq!(
                    mux.connections.get(&remote).unwrap().state,
                    HostState::Reconnecting { attempt: 1 }
                );
                assert_eq!(mux.attached_host, remote);
                assert_eq!(mux.core.attached_session(), Some(SessionId(9)));
                assert_eq!(mux.core.snapshot().generation, 7);
                assert!(mux.core.choose_tree().is_none());
                assert!(mux.error.is_none());
                assert!(mux.error_after_next_attach.is_none());
            });
        });
    }

    #[gpui::test]
    fn reconnect_reingests_hello_and_reattaches_the_remembered_session(cx: &mut TestAppContext) {
        let (mux, remote, fake, _sink, notifications) = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-reconnect-fake.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("local unavailable".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            let (sink, notifications) = record_notifications(&mux, cx);
            let mut hello = test_server_hello();
            hello.status.left = "fresh reconnect hello".to_owned();
            let fake = Arc::new(FakeConnectedHost::new(hello));
            mux.update(cx, |mux, _| {
                let connection = mux.connections.get_mut(&remote).unwrap();
                connection.state = HostState::Connected;
                connection.fake_client = Some(Arc::clone(&fake));
                connection.last_attached_session = Some(SessionId(9));
                mux.attached_host = remote;
                seed_attachment(
                    mux,
                    SessionId(9),
                    MuxSnapshot {
                        generation: 7,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );
                mux.seed_core(EventPayload::StatusChanged {
                    status: StatusLine {
                        left: "stale".to_owned(),
                        ..StatusLine::default()
                    },
                });
            });
            (mux, remote, fake, sink, notifications)
        });
        cx.update(|cx| {
            mux.update(cx, |mux, cx| {
                mux.reattach_after_reconnect(remote, cx);

                assert_eq!(fake.attached_session.get(), Some(SessionId(9)));
                assert!(!fake.attached_default.get());
                assert_eq!(mux.core.status().left, "fresh reconnect hello");
                assert_eq!(
                    mux.connections.get(&remote).unwrap().reconnect_attach,
                    Some(ReconnectAttachState::RememberedSession)
                );
                assert_eq!(
                    (mux.core.snapshot().generation, mux.core.attached_session()),
                    (7, Some(SessionId(9))),
                    "re-ingesting the hello must not blank the frame the daemon has \
                     not re-attached yet",
                );

                mux.handle_message(
                    remote,
                    ProtocolMessage::Attached {
                        session: SessionId(9),
                        snapshot: MuxSnapshot {
                            generation: 12,
                            focused_window: None,
                            sessions: Vec::new(),
                        },
                    },
                    cx,
                );
                let connection = mux.connections.get(&remote).unwrap();
                assert_eq!(connection.last_attached_session, Some(SessionId(9)));
                assert!(connection.reconnect_attach.is_none());
                assert_eq!(mux.core.snapshot().generation, 12);
            });
        });
        cx.run_until_parked();
        assert_eq!(
            &*notifications.borrow(),
            &[(
                ClientMessageKind::Success,
                "reconnected to remote".to_owned(),
            )]
        );
    }

    #[gpui::test]
    fn missing_remembered_session_falls_back_to_default_without_an_error_banner(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-reconnect-missing.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("local unavailable".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            let fake = Arc::new(FakeConnectedHost::new(test_server_hello()));
            mux.update(cx, |mux, cx| {
                let connection = mux.connections.get_mut(&remote).unwrap();
                connection.state = HostState::Connected;
                connection.fake_client = Some(Arc::clone(&fake));
                connection.last_attached_session = Some(SessionId(9));
                mux.attached_host = remote;
                seed_attachment(mux, SessionId(9), MuxSnapshot::default());
                mux.error = None;
                mux.reattach_after_reconnect(remote, cx);

                mux.handle_message(
                    remote,
                    ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Error {
                        request_id: 0,
                        error: ServerError::MissingTarget("9".to_owned()),
                    }),
                    cx,
                );

                assert_eq!(fake.attached_session.get(), Some(SessionId(9)));
                assert!(fake.attached_default.get());
                assert_eq!(
                    mux.connections.get(&remote).unwrap().reconnect_attach,
                    Some(ReconnectAttachState::DefaultSession)
                );
                assert!(mux.error.is_none());
                assert!(mux.error_after_next_attach.is_none());
            });
        });
    }

    #[gpui::test]
    fn non_attached_reconnect_stops_after_three_failures_and_manual_retry_rearms(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let mux = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host(
                    "remote",
                    "unix:///tmp/zz-reconnect-capped-does-not-exist.sock",
                )],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("local unavailable".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, remote);
                mux.handle_host_disconnected(remote, cx);

                for attempt in 1..=MAX_UNATTACHED_RECONNECT_ATTEMPTS {
                    let connection = mux.connections.get_mut(&remote).unwrap();
                    assert_eq!(connection.state, HostState::Reconnecting { attempt });
                    connection.bump_reconnect_generation();
                    connection.state = HostState::Connecting;
                    connection.reconnect_attempt_in_flight = Some(attempt);
                    mux.handle_connect_result(
                        remote,
                        Err(DaemonError::Thread(format!("failure {attempt}"))),
                        cx,
                    );
                }

                let HostState::Unreachable { reason } =
                    &mux.connections.get(&remote).unwrap().state
                else {
                    panic!("the third background reconnect must stop at unreachable");
                };
                assert!(reason.contains("failure 3"));
                mux.retry_host_now(remote, cx);
                let connection = mux.connections.get(&remote).unwrap();
                assert_eq!(connection.state, HostState::Connecting);
                assert_eq!(connection.reconnect_attempt_in_flight, Some(1));
            });
            mux
        });
        std::thread::sleep(Duration::from_millis(50));
        cx.run_until_parked();
        drop(mux);
    }

    #[gpui::test]
    fn a_dismissed_ssh_prompt_parks_the_host_until_reconnect_is_picked(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let mux = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host(
                    "remote",
                    "unix:///tmp/zz-askpass-declined-does-not-exist.sock",
                )],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("local unavailable".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, remote);
                mux.handle_host_disconnected(remote, cx);

                let connection = mux.connections.get_mut(&remote).unwrap();
                assert_eq!(connection.state, HostState::Reconnecting { attempt: 1 });
                connection.bump_reconnect_generation();
                connection.state = HostState::Connecting;
                connection.reconnect_attempt_in_flight = Some(1);

                mux.note_ssh_auth_declined(remote, cx);
                mux.handle_connect_result(
                    remote,
                    Err(DaemonError::Thread("Permission denied".to_owned())),
                    cx,
                );

                let HostState::Unreachable { reason } =
                    &mux.connections.get(&remote).unwrap().state
                else {
                    panic!("a dismissed prompt must park the host, not schedule another rung");
                };
                assert!(reason.contains("Authentication was cancelled"), "{reason}");

                mux.ensure_connected(remote, cx);
                assert!(matches!(
                    mux.connections.get(&remote).unwrap().state,
                    HostState::Unreachable { .. }
                ));

                mux.retry_host_now(remote, cx);
                assert_eq!(
                    mux.connections.get(&remote).unwrap().state,
                    HostState::Connecting,
                );
            });
            mux
        });
        std::thread::sleep(Duration::from_millis(50));
        cx.run_until_parked();
        drop(mux);
    }

    #[test]
    fn the_prompt_dialog_names_the_ssh_destination() {
        let label = |uri: &str| {
            let Endpoint::Ssh(endpoint) = Endpoint::parse(uri).expect(uri) else {
                panic!("{uri} is an ssh endpoint");
            };
            ssh_destination_label(&endpoint)
        };
        assert_eq!(label("ssh://fabrico@xps"), "fabrico@xps");
        assert_eq!(label("ssh://xps"), "xps");
        assert_eq!(label("ssh://fabrico@xps:2222"), "fabrico@xps:2222");
        assert_eq!(label("ssh://[fe80::1]:2222"), "[fe80::1]:2222");
    }

    #[gpui::test]
    fn attaching_elsewhere_cancels_the_pending_reconnect_generation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-reconnect-cancel.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            mux.update(cx, |mux, cx| {
                let local = install_fake_connection(mux, HostId::LOCAL);
                install_fake_connection(mux, remote);
                mux.attached_host = remote;
                seed_attachment(mux, SessionId(9), MuxSnapshot::default());
                mux.handle_host_disconnected(remote, cx);
                let armed_generation = mux.connections.get(&remote).unwrap().reconnect_generation;

                assert!(mux.attach_to_host_target(HostId::LOCAL, None, cx));

                let connection = mux.connections.get(&remote).unwrap();
                assert_eq!(
                    connection.state,
                    HostState::Unreachable {
                        reason: "connection lost".to_owned(),
                    }
                );
                assert!(!reconnect_timer_is_current(connection, armed_generation, 1));
                assert!(local.attached_default.get());
                assert_eq!(mux.attached_host, HostId::LOCAL);
            });
        });
    }

    #[gpui::test]
    fn attached_remote_reconnect_never_gives_up(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-reconnect-give-up.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, HostId::LOCAL);
                install_fake_connection(mux, remote);
                mux.attached_host = remote;
                seed_attachment(mux, SessionId(9), MuxSnapshot::default());
                mux.handle_host_disconnected(remote, cx);

                for attempt in 1..=(MAX_UNATTACHED_RECONNECT_ATTEMPTS + 2) {
                    let connection = mux.connections.get_mut(&remote).unwrap();
                    assert_eq!(connection.state, HostState::Reconnecting { attempt });
                    connection.bump_reconnect_generation();
                    connection.state = HostState::Connecting;
                    connection.reconnect_attempt_in_flight = Some(attempt);
                    mux.handle_connect_result(
                        remote,
                        Err(DaemonError::Thread(format!("failure {attempt}"))),
                        cx,
                    );
                }

                assert_eq!(mux.attached_host, remote);
                assert_eq!(
                    mux.connections[&remote].state,
                    HostState::Reconnecting {
                        attempt: MAX_UNATTACHED_RECONNECT_ATTEMPTS + 3
                    }
                );
            });
        });
    }

    #[gpui::test]
    fn abrupt_local_disconnect_preserves_snapshot_and_reconnects_unless_shutting_down(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("initial error".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, HostId::LOCAL);
                let cached = Arc::new(MuxSnapshot {
                    generation: 7,
                    focused_window: None,
                    sessions: Vec::new(),
                });
                mux.connections.get_mut(&HostId::LOCAL).unwrap().snapshot =
                    Some(Arc::clone(&cached));
                mux.error = None;

                mux.handle_host_disconnected(HostId::LOCAL, cx);

                let connection = mux.connections.get(&HostId::LOCAL).unwrap();
                assert_eq!(connection.state, HostState::Reconnecting { attempt: 1 });
                assert!(
                    connection
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| Arc::ptr_eq(snapshot, &cached))
                );
                assert!(connection.reconnect_generation > 0);
                assert_eq!(mux.attached_host, HostId::LOCAL);
                assert_eq!(mux.error.as_deref(), Some("fleet host local disconnected"));

                install_fake_connection(mux, HostId::LOCAL);
                mux.connections.get_mut(&HostId::LOCAL).unwrap().snapshot =
                    Some(Arc::clone(&cached));
                let reconnect_generation = mux.connections[&HostId::LOCAL].reconnect_generation;
                mux.detach();
                mux.handle_host_disconnected(HostId::LOCAL, cx);

                let connection = mux.connections.get(&HostId::LOCAL).unwrap();
                assert_eq!(
                    connection.state,
                    HostState::Unreachable {
                        reason: "connection lost".to_owned(),
                    }
                );
                assert!(connection.snapshot.is_none());
                assert_eq!(connection.reconnect_generation, reconnect_generation);
            });
        });
    }

    #[cfg(unix)]
    #[gpui::test]
    fn attached_remote_stream_eof_reconnects_and_reattaches_the_same_session(
        cx: &mut TestAppContext,
    ) {
        use std::os::unix::net::UnixListener;

        cx.executor().allow_parking();
        let socket = std::env::temp_dir().join(format!(
            "z5-eof-{}-{}.sock",
            std::process::id(),
            TEST_SOCKET_ID.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = std::fs::remove_file(&socket);
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("bind fake remote socket: {error}"),
        };
        let remote_session = SessionId(41);
        let remote_snapshot = MuxSnapshot {
            generation: 2,
            focused_window: None,
            sessions: vec![SessionSnapshot {
                id: remote_session,
                name: "remote-default".to_owned(),
                active_window: WindowId(1),
                windows: Vec::new(),
                viewers: Vec::new(),
            }],
        };
        let (close_remote, close_remote_receiver) = mpsc::channel();
        let (remote_dropped, remote_dropped_receiver) = mpsc::channel();
        let (respawn_remote, respawn_remote_receiver) = mpsc::channel();
        let (finish_remote, finish_remote_receiver) = mpsc::channel();
        let server_socket = socket.clone();
        let remote_endpoint = format!("unix://{}", socket.display());
        let server_snapshot = remote_snapshot.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept fake remote client");
            assert!(matches!(
                read_protocol_message(&mut stream).expect("read ClientHello"),
                ProtocolMessage::ClientHello(_)
            ));
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::ServerHello(test_server_hello()),
            )
            .expect("write ServerHello");
            assert!(matches!(
                read_protocol_message(&mut stream).expect("read initial Resync"),
                ProtocolMessage::Resync
            ));
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::Snapshot(server_snapshot.clone()),
                }),
            )
            .expect("write connect-time snapshot");

            loop {
                match read_protocol_message(&mut stream).expect("read remote attach") {
                    ProtocolMessage::Attach { session } => {
                        assert_eq!(session, remote_session.to_string());
                        break;
                    }
                    ProtocolMessage::SetColorScheme(_)
                    | ProtocolMessage::SetConfigOverrides { .. } => {}
                    message => panic!("unexpected message before remote attach: {message:?}"),
                }
            }
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::Attached {
                    session: remote_session,
                    snapshot: server_snapshot.clone(),
                },
            )
            .expect("write remote Attached");
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 2,
                    payload: EventPayload::TerminalViewport {
                        pane: PaneId(77),
                        viewport: TerminalViewport::blank(2, 2, SessionStatus::Running),
                    },
                }),
            )
            .expect("write first remote frame");

            close_remote_receiver
                .recv()
                .expect("wait to close attached remote stream");
            drop(stream);
            drop(listener);
            let _ = std::fs::remove_file(&server_socket);
            remote_dropped.send(()).expect("report remote drop");
            respawn_remote_receiver
                .recv()
                .expect("wait to respawn remote");

            let listener = UnixListener::bind(&server_socket).expect("rebind fake remote socket");
            let (mut stream, _) = listener.accept().expect("accept reconnected remote client");
            assert!(matches!(
                read_protocol_message(&mut stream).expect("read reconnect ClientHello"),
                ProtocolMessage::ClientHello(_)
            ));
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::ServerHello(test_server_hello()),
            )
            .expect("write reconnect ServerHello");
            assert!(matches!(
                read_protocol_message(&mut stream).expect("read reconnect Resync"),
                ProtocolMessage::Resync
            ));
            let mut reconnected_snapshot = server_snapshot;
            reconnected_snapshot.generation = 3;
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 3,
                    payload: EventPayload::Snapshot(reconnected_snapshot.clone()),
                }),
            )
            .expect("write reconnect snapshot");
            loop {
                match read_protocol_message(&mut stream).expect("read reconnect attach") {
                    ProtocolMessage::Attach { session } => {
                        assert_eq!(session, remote_session.to_string());
                        break;
                    }
                    ProtocolMessage::SetColorScheme(_)
                    | ProtocolMessage::SetConfigOverrides { .. } => {}
                    message => panic!("unexpected reconnect message before attach: {message:?}"),
                }
            }
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::Attached {
                    session: remote_session,
                    snapshot: reconnected_snapshot,
                },
            )
            .expect("write reconnect Attached");
            write_protocol_message(
                &mut stream,
                &ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 4,
                    payload: EventPayload::TerminalViewport {
                        pane: PaneId(77),
                        viewport: TerminalViewport::blank(3, 2, SessionStatus::Running),
                    },
                }),
            )
            .expect("write frame after reconnect");
            finish_remote_receiver
                .recv()
                .expect("wait to finish reconnected remote");
            drop(stream);
            drop(listener);
            let _ = std::fs::remove_file(&server_socket);
        });
        let remote_client = InteractiveClient::connect_endpoint(
            &zz_daemon::Endpoint::Local(socket),
            TerminalColorScheme::Dark,
        )
        .expect("connect fake remote client");

        let (mux, remote, local) = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", &remote_endpoint)],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("local test transport".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            let local = mux.update(cx, |mux, cx| {
                let local = install_fake_connection(mux, HostId::LOCAL);
                mux.connections.get_mut(&remote).unwrap().state = HostState::Connecting;
                mux.handle_connect_result(remote, Ok(remote_client), cx);
                local
            });
            (mux, remote, local)
        });

        wait_for_mux(cx, &mux, "fake remote connect-time snapshot", |mux| {
            mux.connections.get(&remote).is_some_and(|connection| {
                connection.state == HostState::Connected
                    && connection.snapshot.as_deref() == Some(&remote_snapshot)
            })
        });
        cx.update(|cx| {
            mux.update(cx, |mux, cx| {
                mux.attach_to_host(remote, remote_session, cx);
            });
        });
        wait_for_mux(cx, &mux, "fake remote Attached response", |mux| {
            mux.attached_host == remote
                && mux.core.attached_session() == Some(remote_session)
                && mux
                    .viewports
                    .get(&PaneId(77))
                    .is_some_and(|viewport| viewport.read().viewport.columns == 2)
        });
        let (frozen_snapshot, frozen_viewport) = cx.update(|cx| {
            let mux = mux.read(cx);
            (
                Arc::clone(mux.core.snapshot()),
                Arc::clone(mux.viewports.get(&PaneId(77)).expect("first remote frame")),
            )
        });

        close_remote
            .send(())
            .expect("close attached fake remote stream");
        remote_dropped_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("remote socket dropped");
        wait_for_mux(cx, &mux, "frozen reconnect state", |mux| {
            mux.attached_host == remote
                && mux.core.attached_session() == Some(remote_session)
                && mux.error.is_none()
                && !local.attached_default.get()
                && Arc::ptr_eq(mux.core.snapshot(), &frozen_snapshot)
                && mux
                    .viewports
                    .get(&PaneId(77))
                    .is_some_and(|viewport| Arc::ptr_eq(viewport, &frozen_viewport))
                && mux.connections.get(&remote).is_some_and(|connection| {
                    connection.state == (HostState::Reconnecting { attempt: 1 })
                        && connection.last_attached_session == Some(remote_session)
                })
        });
        respawn_remote.send(()).expect("respawn fake remote");
        wait_for_mux(cx, &mux, "same-session reconnect frame", |mux| {
            mux.attached_host == remote
                && mux.core.attached_session() == Some(remote_session)
                && mux.core.snapshot().generation == 3
                && mux
                    .viewports
                    .get(&PaneId(77))
                    .is_some_and(|viewport| viewport.read().viewport.columns == 3)
                && mux.connections.get(&remote).is_some_and(|connection| {
                    connection.state == HostState::Connected
                        && connection.last_attached_session == Some(remote_session)
                        && connection.reconnect_attach.is_none()
                })
        });
        finish_remote.send(()).expect("finish fake remote");
        server.join().expect("join fake remote server");
    }

    #[cfg(unix)]
    #[gpui::test]
    fn attached_remote_daemon_process_restart_reconnects_and_receives_new_frames(
        cx: &mut TestAppContext,
    ) {
        const TEST_NAME: &str = "mux::client::tests::\
            attached_remote_daemon_process_restart_reconnects_and_receives_new_frames";
        if let Some(socket) = std::env::var_os(M3_DAEMON_CHILD_SOCKET) {
            cx.executor().allow_parking();
            let result = zz_daemon::Daemon::new(PathBuf::from(socket))
                .without_user_config()
                .run_foreground();
            match result {
                Err(DaemonError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied => {
                    std::process::exit(M3_DAEMON_SANDBOX_BLOCKED_EXIT);
                }
                result => result.expect("run process-backed test daemon"),
            }
            return;
        }

        cx.executor().allow_parking();
        let socket = PathBuf::from(format!(
            "/tmp/zz-m3-process-{}-{}.sock",
            std::process::id(),
            TEST_SOCKET_ID.fetch_add(1, Ordering::Relaxed),
        ));
        let Some(mut remote_daemon) = RunningProcessTestDaemon::start(socket.clone(), TEST_NAME)
        else {
            return;
        };
        let remote_endpoint = format!("unix://{}", socket.display());
        let (mux, remote) = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", &remote_endpoint)],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("local unavailable".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            mux.update(cx, |mux, cx| mux.ensure_connected(remote, cx));
            (mux, remote)
        });

        wait_for_mux(
            cx,
            &mux,
            "process-backed remote connection and snapshot",
            |mux| {
                mux.connections.get(&remote).is_some_and(|connection| {
                    connection.state == HostState::Connected
                        && connection
                            .snapshot
                            .as_ref()
                            .is_some_and(|snapshot| !snapshot.sessions.is_empty())
                })
            },
        );
        let (session, session_name) = cx.update(|cx| {
            let mux = mux.read(cx);
            let session = mux.connections[&remote]
                .snapshot
                .as_ref()
                .unwrap()
                .sessions
                .first()
                .unwrap();
            (session.id, session.name.clone())
        });
        cx.update(|cx| {
            mux.update(cx, |mux, cx| {
                assert!(mux.attach_to_host(remote, session, cx));
            });
        });
        wait_for_mux(cx, &mux, "first process-backed remote frame", |mux| {
            mux.attached_host == remote
                && mux.core.attached_session() == Some(session)
                && !mux.viewports.is_empty()
        });
        let pane = cx.update(|cx| {
            let mux = mux.read(cx);
            *mux.viewports.first_key_value().unwrap().0
        });

        remote_daemon.stop();
        wait_for_mux(cx, &mux, "process-backed daemon reconnect state", |mux| {
            mux.attached_host == remote
                && mux.core.attached_session() == Some(session)
                && mux.error.is_none()
                && mux
                    .core
                    .snapshot()
                    .sessions
                    .iter()
                    .any(|candidate| candidate.id == session)
                && mux.viewports.contains_key(&pane)
                && mux.connections.get(&remote).is_some_and(|connection| {
                    matches!(connection.state, HostState::Reconnecting { .. })
                        && connection.last_attached_session == Some(session)
                })
        });
        let (frozen_snapshot, frozen_viewport) = cx.update(|cx| {
            let mux = mux.read(cx);
            (
                Arc::clone(mux.core.snapshot()),
                Arc::clone(mux.viewports.get(&pane).unwrap()),
            )
        });

        assert!(
            remote_daemon.restart(),
            "a daemon restart should remain permitted after its first successful start"
        );
        wait_for_mux(
            cx,
            &mux,
            "process-backed same-session reattach and fresh frame",
            |mux| {
                mux.attached_host == remote
                    && mux.core.attached_session() == Some(session)
                    && mux
                        .core
                        .snapshot()
                        .sessions
                        .iter()
                        .any(|candidate| candidate.id == session && candidate.name == session_name)
                    && !Arc::ptr_eq(mux.core.snapshot(), &frozen_snapshot)
                    && mux
                        .viewports
                        .get(&pane)
                        .is_some_and(|viewport| !Arc::ptr_eq(viewport, &frozen_viewport))
                    && mux.connections.get(&remote).is_some_and(|connection| {
                        connection.state == HostState::Connected
                            && connection.last_attached_session == Some(session)
                            && connection.reconnect_attach.is_none()
                    })
            },
        );

        drop(mux);
        remote_daemon.stop();
    }

    #[cfg(unix)]
    #[gpui::test]
    fn fleet_attach_two_real_daemons_keeps_remote_frames_on_server_stopping(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let Some(mut local_daemon) = RunningTestDaemon::start() else {
            return;
        };
        let Some(mut remote_daemon) = RunningTestDaemon::start() else {
            return;
        };
        let local_client = local_daemon.connect_interactive();
        let local_socket = local_daemon.socket.clone();
        let remote_endpoint = format!("unix://{}", remote_daemon.socket.display());
        let (mux, remote) = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", &remote_endpoint)],
                cx,
            );
            let mux = cx.new(|cx| MuxClient::new(Ok(local_client), local_socket.clone(), cx));
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            mux.update(cx, |mux, cx| mux.ensure_connected(remote, cx));
            (mux, remote)
        });

        wait_for_mux(
            cx,
            &mux,
            "remote connection and connect-time snapshot",
            |mux| {
                let connection = mux.connections.get(&remote).unwrap();
                connection.state == HostState::Connected
                    && connection
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| !snapshot.sessions.is_empty())
            },
        );

        let remote_session = cx.update(|cx| {
            let mux = mux.read(cx);
            mux.connections[&remote]
                .snapshot
                .as_ref()
                .expect("connect-time remote snapshot")
                .sessions
                .first()
                .expect("remote default session")
                .id
        });

        cx.update(|cx| {
            mux.update(cx, |mux, cx| {
                mux.attach_to_host(remote, remote_session, cx);
            });
        });
        wait_for_mux(cx, &mux, "remote Attached response", |mux| {
            mux.attached_host == remote && mux.core.attached_session() == Some(remote_session)
        });
        cx.update(|cx| {
            let mux = mux.read(cx);
            let local = &mux.connections[&HostId::LOCAL];
            assert_eq!(local.state, HostState::Connected);
            assert_eq!(local.reconnect_attempt_in_flight, None);
        });
        cx.update(|cx| {
            mux.update(cx, |mux, _| seed_choose_tree(mux));
        });

        remote_daemon.stop();
        wait_for_mux(cx, &mux, "remote ServerStopping event", |mux| {
            mux.core.choose_tree().is_none()
                && mux.error.is_none()
                && mux.attached_host == remote
                && mux.core.attached_session() == Some(remote_session)
        });
        local_daemon.stop();
    }

    #[gpui::test]
    fn non_attached_hosts_cache_snapshots_and_ignore_attached_only_events(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-step4-remote.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;

            mux.update(cx, |mux, cx| {
                seed_snapshot(
                    mux,
                    MuxSnapshot {
                        generation: 3,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );
                seed_choose_tree(mux);
                let revision = mux.choose_tree_revision;

                mux.handle_message(
                    remote,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 9,
                        payload: EventPayload::Snapshot(MuxSnapshot {
                            generation: 11,
                            focused_window: None,
                            sessions: Vec::new(),
                        }),
                    }),
                    cx,
                );

                assert_eq!(mux.core.snapshot().generation, 3);
                assert_eq!(
                    mux.connections
                        .get(&remote)
                        .unwrap()
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .generation,
                    11
                );
                assert_eq!(
                    mux.choose_tree_revision, revision,
                    "remote snapshots now refresh the sidebar without perturbing the chooser",
                );

                mux.sidebar_focus_revision = 4;
                mux.handle_message(
                    remote,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 10,
                        payload: EventPayload::FocusSidebar,
                    }),
                    cx,
                );
                mux.handle_message(
                    remote,
                    ProtocolMessage::Attached {
                        session: SessionId(99),
                        snapshot: MuxSnapshot {
                            generation: 12,
                            focused_window: None,
                            sessions: Vec::new(),
                        },
                    },
                    cx,
                );

                assert_eq!(mux.sidebar_focus_revision, 4);
                assert_eq!(mux.core.attached_session(), None);
                assert_eq!(
                    mux.connections
                        .get(&remote)
                        .unwrap()
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .generation,
                    11
                );
            });
        });
    }

    #[gpui::test]
    fn ensure_connected_is_a_noop_while_connecting_or_connected(cx: &mut TestAppContext) {
        let mux = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host(
                    "remote",
                    "unix:///tmp/zz-step3b-does-not-exist.sock",
                )],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux
                .read(cx)
                .registry
                .get_by_name("remote")
                .expect("remote host")
                .0;
            mux.update(cx, |mux, cx| {
                mux.connections.get_mut(&remote).unwrap().state = HostState::Connecting;
                mux.ensure_connected(remote, cx);
            });
            (mux, remote)
        });

        std::thread::sleep(Duration::from_millis(50));
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                mux.0.read(cx).connections.get(&mux.1).unwrap().state,
                HostState::Connecting
            );
            mux.0.update(cx, |mux_client, cx| {
                mux_client.connections.get_mut(&mux.1).unwrap().state = HostState::Connected;
                mux_client.ensure_connected(mux.1, cx);
                assert_eq!(
                    mux_client.connections.get(&mux.1).unwrap().state,
                    HostState::Connected
                );
            });
        });
    }

    #[gpui::test]
    fn a_published_fleet_host_registers_and_starts_connecting(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let mux = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            })
        });
        cx.update(|cx| assert!(mux.read(cx).registry.get_by_name("studio").is_none()));

        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("studio", "unix:///tmp/zz-published-studio.sock")],
                cx,
            );
        });

        cx.update(|cx| {
            let mux = mux.read(cx);
            let studio = mux
                .registry
                .get_by_name("studio")
                .expect("publishing a fleet host registers it")
                .0;
            assert_ne!(
                mux.connections.get(&studio).unwrap().state,
                HostState::Disconnected,
                "a newly published host dials without waiting for a restart"
            );
        });
    }

    #[gpui::test]
    fn host_state_access_reconciles_changed_fleet_hosts_by_name(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (mux, server_id) = cx.update(|cx| {
            let desktop = test_host(
                "desktop",
                "unix:///tmp/zz-reconcile-desktop-does-not-exist.sock",
            );
            let server = test_host(
                "server",
                "unix:///tmp/zz-reconcile-server-does-not-exist.sock",
            );
            crate::config::set_fleet_hosts_for_test(vec![desktop, server.clone()], cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });

            mux.update(cx, |mux, _| {
                mux.connections.get_mut(&HostId::LOCAL).unwrap().state = HostState::Unreachable {
                    reason: "local kept".to_owned(),
                };
                let desktop = mux.registry.get_by_name("desktop").unwrap().0;
                mux.connections.get_mut(&desktop).unwrap().state = HostState::Unreachable {
                    reason: "desktop dropped".to_owned(),
                };
                let server = mux.registry.get_by_name("server").unwrap().0;
                mux.connections.get_mut(&server).unwrap().state = HostState::Unreachable {
                    reason: "server kept".to_owned(),
                };
            });
            let (desktop_route, server_route) = {
                let mux = mux.read(cx);
                let desktop = mux.registry.get_by_name("desktop").unwrap().0;
                let server = mux.registry.get_by_name("server").unwrap().0;
                (
                    Arc::clone(&mux.connections.get(&desktop).unwrap().route),
                    Arc::clone(&mux.connections.get(&server).unwrap().route),
                )
            };

            crate::config::set_fleet_hosts_for_test(vec![server], cx);
            mux.update(cx, |mux, cx| {
                let states = mux
                    .host_states(cx)
                    .map(|(host, name, state)| (host, name.to_owned(), state.clone()))
                    .collect::<Vec<_>>();
                assert_eq!(
                    states,
                    [
                        (
                            HostId::LOCAL,
                            "local".to_owned(),
                            HostState::Unreachable {
                                reason: "local kept".to_owned(),
                            },
                        ),
                        (
                            mux.registry.get_by_name("server").unwrap().0,
                            "server".to_owned(),
                            HostState::Connecting,
                        ),
                    ]
                );
                assert!(mux.registry.get_by_name("desktop").is_none());
                assert_eq!(mux.connections.len(), 2);
                assert_eq!(*desktop_route.read(), None);
                assert_eq!(
                    *server_route.read(),
                    Some(mux.registry.get_by_name("server").unwrap().0)
                );
            });
            let server_id = mux.read(cx).registry.get_by_name("server").unwrap().0;
            (mux, server_id)
        });
        wait_for_mux(cx, &mux, "config-reconciled connection attempt", |mux| {
            matches!(
                mux.connections
                    .get(&server_id)
                    .map(|connection| &connection.state),
                Some(HostState::Unreachable { .. })
            )
        });
    }

    #[gpui::test]
    fn reconcile_retains_a_removed_attached_host_until_it_is_no_longer_attached(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(vec![test_host("remote", "ssh://remote")], cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            let route = Arc::clone(&mux.read(cx).connections.get(&remote).unwrap().route);
            mux.update(cx, |mux, _| {
                mux.connections.get_mut(&remote).unwrap().state = HostState::Connected;
                mux.attached_host = remote;
            });

            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            mux.update(cx, |mux, cx| {
                let states = mux
                    .host_states(cx)
                    .map(|(_, name, state)| (name.to_owned(), state.clone()))
                    .collect::<Vec<_>>();
                assert_eq!(
                    states,
                    [
                        ("local".to_owned(), HostState::Disconnected),
                        ("remote".to_owned(), HostState::Connected),
                    ]
                );
                assert!(mux.registry.is_retained(mux.attached_host));

                mux.attached_host = HostId::LOCAL;
                assert_eq!(mux.host_states(cx).count(), 1);
                assert!(mux.registry.get_by_name("remote").is_none());
                assert_eq!(*route.read(), None);
            });
        });
    }

    #[cfg(feature = "agent-pane")]
    fn agent_blob(seq: u64) -> Vec<u8> {
        serde_json::to_vec(&zz_daemon::AgentStreamItem {
            seq,
            payload: zz_daemon::AgentStreamPayload::PromptAccepted { turn_id: seq },
        })
        .expect("a stream item encodes")
    }

    #[cfg(feature = "agent-pane")]
    fn agent_reset(seq: u64) -> Vec<u8> {
        serde_json::to_vec(&zz_daemon::AgentStreamItem {
            seq,
            payload: zz_daemon::AgentStreamPayload::SessionReset { restoring: true },
        })
        .expect("a reset item encodes")
    }

    #[cfg(feature = "agent-pane")]
    fn applied_agent_seqs(mux: &mut MuxClient, pane: PaneId) -> Vec<u64> {
        mux.take_agent_events_for(&BTreeSet::from([pane]))
            .items
            .into_iter()
            .flat_map(|(_, items)| items.into_iter().map(|item| item.seq))
            .collect()
    }

    #[cfg(feature = "agent-pane")]
    #[gpui::test]
    fn the_agent_stream_is_filtered_by_seq_and_a_gap_asks_for_a_replay(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            mux.update(cx, |mux, cx| {
                let pane = PaneId(3);
                let sink = mux.record_agent_requests_for_test();
                let blob = agent_blob;
                let reset = agent_reset;
                let applied = |mux: &mut MuxClient| applied_agent_seqs(mux, pane);

                mux.apply_agent_updates(pane, 1, vec![blob(1), blob(2)]);
                assert_eq!(applied(mux), [1, 2]);

                mux.apply_agent_updates(pane, 1, vec![blob(1), blob(2), blob(3)]);
                assert_eq!(
                    applied(mux),
                    [3],
                    "a replay overlaps the live tail and only the new item survives"
                );

                mux.apply_agent_updates(pane, 9, vec![blob(9)]);
                assert!(
                    applied(mux).is_empty(),
                    "a batch past the cursor is a hole, not something to buffer across"
                );
                assert_eq!(mux.agent_cursors.get(&pane), Some(&3));
                assert_eq!(
                    &*sink.borrow(),
                    &[(pane, AgentRequest::Replay { from_seq: 3 })]
                );

                sink.borrow_mut().clear();
                mux.request_agent_replay_from_cursor(pane);
                assert_eq!(
                    &*sink.borrow(),
                    &[(pane, AgentRequest::Replay { from_seq: 3 })]
                );

                sink.borrow_mut().clear();
                mux.clear_agent_streams();
                mux.request_agent_replay_from_cursor(pane);
                assert_eq!(
                    &*sink.borrow(),
                    &[(pane, AgentRequest::Replay { from_seq: 0 })],
                    "an attach forgets the cursor so the stream replays from the top"
                );

                sink.borrow_mut().clear();
                mux.apply_agent_updates(pane, 40, vec![reset(40), blob(41)]);
                assert_eq!(applied(mux), [40, 41]);
                assert_eq!(mux.agent_cursors.get(&pane), Some(&41));
                assert!(sink.borrow().is_empty());

                let mut attaching = false;
                mux.apply_core_event(
                    HostId::LOCAL,
                    CoreEvent::AgentLagged { pane, next_seq: 42 },
                    None,
                    &mut attaching,
                    cx,
                );
                assert_eq!(
                    &*sink.borrow(),
                    &[(pane, AgentRequest::Replay { from_seq: 41 })],
                    "an overflow replays from the shell's cursor, not the daemon's drop point"
                );
            });
        });
    }

    #[cfg(feature = "agent-pane")]
    #[gpui::test]
    fn a_hole_asks_for_one_replay_until_the_stream_lands_again(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            mux.update(cx, |mux, _| {
                let pane = PaneId(3);
                let sink = mux.record_agent_requests_for_test();
                let applied = |mux: &mut MuxClient| applied_agent_seqs(mux, pane);

                mux.apply_agent_updates(pane, 1, vec![agent_blob(1), agent_blob(2)]);
                assert_eq!(applied(mux), [1, 2]);

                mux.apply_agent_updates(pane, 7, vec![agent_blob(7)]);
                mux.apply_agent_updates(pane, 8, vec![agent_blob(8)]);
                mux.apply_agent_updates(pane, 9, vec![agent_blob(9), agent_blob(10)]);
                assert!(
                    applied(mux).is_empty(),
                    "nothing across the hole is applied while the replay is outstanding"
                );
                assert_eq!(
                    &*sink.borrow(),
                    &[(pane, AgentRequest::Replay { from_seq: 2 })],
                    "three sightings of one hole are one ask"
                );

                mux.apply_agent_updates(pane, 2, vec![agent_blob(2), agent_blob(3)]);
                assert_eq!(
                    applied(mux),
                    [3],
                    "the replay overlaps the cursor and lands the item the hole hid"
                );
                assert_eq!(
                    sink.borrow().len(),
                    1,
                    "landing the replay is not itself a reason to ask again"
                );

                mux.apply_agent_updates(pane, 20, vec![agent_blob(20)]);
                assert!(applied(mux).is_empty());
                assert_eq!(
                    &*sink.borrow(),
                    &[
                        (pane, AgentRequest::Replay { from_seq: 2 }),
                        (pane, AgentRequest::Replay { from_seq: 3 })
                    ],
                    "a hole after the replay landed is a new hole, asked from the new cursor"
                );
            });
        });
    }

    #[cfg(feature = "agent-pane")]
    #[gpui::test]
    fn a_journal_floor_replay_clears_the_outstanding_ask(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            mux.update(cx, |mux, _| {
                let pane = PaneId(3);
                let sink = mux.record_agent_requests_for_test();

                mux.apply_agent_updates(pane, 1, vec![agent_blob(1), agent_blob(2)]);
                mux.apply_agent_updates(pane, 7, vec![agent_blob(7)]);
                assert_eq!(sink.borrow().len(), 1);
                let _ = applied_agent_seqs(mux, pane);

                mux.apply_agent_updates(pane, 40, vec![agent_reset(40), agent_blob(41)]);
                assert_eq!(
                    applied_agent_seqs(mux, pane),
                    [40, 41],
                    "the daemon's journal-floor replay is stamped past the hole and still applies"
                );
                assert!(!mux.agent_replays_pending.contains(&pane));

                mux.apply_agent_updates(pane, 50, vec![agent_blob(50)]);
                assert_eq!(
                    sink.borrow().last(),
                    Some(&(pane, AgentRequest::Replay { from_seq: 41 })),
                    "the next hole asks again once the reset landed"
                );
            });
        });
    }

    #[cfg(feature = "agent-pane")]
    #[gpui::test]
    fn a_reattach_forgets_that_a_pane_was_waiting_on_a_replay(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            mux.update(cx, |mux, _| {
                let pane = PaneId(3);
                let sink = mux.record_agent_requests_for_test();

                mux.apply_agent_updates(pane, 1, vec![agent_blob(1)]);
                mux.apply_agent_updates(pane, 5, vec![agent_blob(5)]);
                assert!(mux.agent_replays_pending.contains(&pane));

                mux.clear_agent_streams();
                assert!(mux.agent_replays_pending.is_empty());

                sink.borrow_mut().clear();
                mux.apply_agent_updates(pane, 5, vec![agent_blob(5)]);
                assert_eq!(
                    &*sink.borrow(),
                    &[(pane, AgentRequest::Replay { from_seq: 0 })],
                    "a fresh connection asks from the top rather than staying silent"
                );

                mux.forget_pane(pane);
                assert!(mux.agent_replays_pending.is_empty());
            });
        });
    }

    #[cfg(feature = "agent-pane")]
    #[gpui::test]
    fn an_undecodable_agent_item_is_consumed_rather_than_left_as_a_hole(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            mux.update(cx, |mux, _| {
                let pane = PaneId(3);
                let sink = mux.record_agent_requests_for_test();

                mux.apply_agent_updates(
                    pane,
                    1,
                    vec![
                        agent_blob(1),
                        b"{\"seq\":2,\"payload\":{}}".to_vec(),
                        agent_blob(3),
                    ],
                );

                assert_eq!(
                    applied_agent_seqs(mux, pane),
                    [1, 3],
                    "the item after the undecodable one is still applied"
                );
                assert_eq!(mux.agent_cursors.get(&pane), Some(&3));
                assert!(
                    sink.borrow().is_empty(),
                    "a skipped seq counts as consumed, so it is not a gap to replay"
                );
            });
        });
    }

    #[gpui::test]
    fn releasing_the_attached_host_lets_its_removal_end_the_connection(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(vec![test_host("remote", "ssh://remote")], cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            let (local, route) = mux.update(cx, |mux, cx| {
                let local = install_fake_connection(mux, HostId::LOCAL);
                install_fake_connection(mux, remote);
                mux.attached_host = remote;
                seed_attachment(mux, SessionId(9), MuxSnapshot::default());
                let route = Arc::clone(&mux.connections.get(&remote).unwrap().route);
                mux.release_host(HostId::LOCAL, cx);
                assert_eq!(mux.attached_host, remote);
                mux.release_host(remote, cx);
                (local, route)
            });
            assert!(local.attached_default.get());

            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            mux.update(cx, |mux, cx| {
                assert_eq!(mux.host_states(cx).count(), 1);
                assert!(mux.registry.get_by_name("remote").is_none());
                assert_eq!(mux.attached_host, HostId::LOCAL);
                assert!(
                    mux.registry
                        .iter()
                        .all(|(host, _)| !mux.registry.is_retained(host)),
                    "nothing is kept alive behind the tree's back"
                );
                assert_eq!(*route.read(), None);
            });
        });
    }

    #[gpui::test]
    fn removing_an_attached_reconnecting_host_cancels_its_route_and_gives_up_to_local(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-removed-reconnect.sock")],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            let (local, route) = mux.update(cx, |mux, cx| {
                let local = install_fake_connection(mux, HostId::LOCAL);
                install_fake_connection(mux, remote);
                mux.attached_host = remote;
                seed_attachment(mux, SessionId(9), MuxSnapshot::default());
                let route = Arc::clone(&mux.connections.get(&remote).unwrap().route);
                mux.handle_host_disconnected(remote, cx);
                (local, route)
            });

            crate::config::set_fleet_hosts_for_test(Vec::new(), cx);
            mux.update(cx, |mux, cx| {
                assert_eq!(mux.host_states(cx).count(), 1);
                assert!(mux.registry.get_by_name("remote").is_none());
                assert_eq!(mux.attached_host, HostId::LOCAL);
                assert!(local.attached_default.get());
                assert!(mux.core.snapshot().sessions.is_empty());
                assert!(mux.core.attached_session().is_none());
                assert_eq!(mux.error.as_deref(), Some("fleet host remote disconnected"));
                assert_eq!(*route.read(), None);
            });
        });
    }

    #[gpui::test]
    fn attach_to_non_connected_host_is_a_noop(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(vec![test_host("remote", "ssh://remote")], cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("initial error".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux
                .read(cx)
                .registry
                .get_by_name("remote")
                .expect("remote host")
                .0;
            mux.update(cx, |mux, cx| {
                mux.connections.get_mut(&remote).unwrap().state = HostState::Unreachable {
                    reason: "offline".to_owned(),
                };
                seed_attachment(
                    mux,
                    SessionId(7),
                    MuxSnapshot {
                        generation: 9,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );
                let error = mux.error.clone();

                mux.attach_to_host(remote, SessionId(11), cx);

                assert_eq!(mux.attached_host, HostId::LOCAL);
                assert_eq!(mux.core.attached_session(), Some(SessionId(7)));
                assert_eq!(mux.core.snapshot().generation, 9);
                assert_eq!(mux.error, error);
            });
        });
    }

    #[gpui::test]
    fn compact_machine_row_attach_asks_the_daemon_for_its_default_session(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(vec![test_host("remote", "ssh://remote")], cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("initial error".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;
            let fake_remote = Arc::new(FakeConnectedHost::new(test_server_hello()));

            mux.update(cx, |mux, cx| {
                let connection = mux.connections.get_mut(&remote).unwrap();
                connection.state = HostState::Connected;
                connection.fake_client = Some(Arc::clone(&fake_remote));

                assert!(mux.attach_to_host_default(remote, cx));
                assert_eq!(mux.attached_host, remote);
                assert!(fake_remote.attached_default.get());
                assert_eq!(fake_remote.attached_session.get(), None);
            });
        });
    }

    #[gpui::test]
    fn attaching_remote_preserves_local_snapshot_and_connection(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host(
                    "remote",
                    "unix:///tmp/zz-attach-redial-remote.sock",
                )],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    std::path::PathBuf::from("/tmp/zz-attach-redial-local-does-not-exist.sock"),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;

            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, HostId::LOCAL);
                let remote_client = install_fake_connection(mux, remote);
                seed_attachment(
                    mux,
                    SessionId(7),
                    MuxSnapshot {
                        generation: 19,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );
                let local_snapshot = Arc::clone(mux.core.snapshot());

                assert!(mux.attach_to_host(remote, SessionId(11), cx));

                assert_eq!(mux.attached_host, remote);
                assert_eq!(remote_client.attached_session.get(), Some(SessionId(11)));
                let local = &mux.connections[&HostId::LOCAL];
                assert_eq!(local.state, HostState::Connected);
                assert_eq!(local.reconnect_attempt_in_flight, None);
                assert!(
                    local
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| Arc::ptr_eq(snapshot, &local_snapshot))
                );
            });
        });
    }

    #[gpui::test]
    fn attach_to_connected_host_clears_old_pane_state_before_remote_attach(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(vec![test_host("remote", "ssh://remote")], cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("initial error".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux
                .read(cx)
                .registry
                .get_by_name("remote")
                .expect("remote host")
                .0;
            let fake_remote = Arc::new(FakeConnectedHost::new(test_server_hello()));

            mux.update(cx, |mux, cx| {
                let connection = mux.connections.get_mut(&remote).expect("remote slot");
                connection.state = HostState::Connected;
                connection.fake_client = Some(Arc::clone(&fake_remote));

                let pane = PaneId(23);
                let retained = Arc::new(RwLock::new(new_retained_viewport(
                    TerminalViewport::blank(1, 1, SessionStatus::Running),
                    &mut mux.next_row_revision,
                )));
                seed_attachment(
                    mux,
                    SessionId(7),
                    MuxSnapshot {
                        generation: 9,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );
                mux.viewports.insert(pane, Arc::clone(&retained));
                mux.browser_commands
                    .entry(pane)
                    .or_default()
                    .push(BrowserCommand::Reload);
                mux.agent_commands.entry(pane).or_default().push((
                    31,
                    AgentCommand::ComposerAppend {
                        text: "old host".to_owned(),
                    },
                ));
                mux.screenshot_requests
                    .push((pane, 32, "/tmp/old-host.png".to_owned()));
                mux.terminal_commands.entry(pane).or_default().push(
                    TerminalUiCommand::BeginSearch {
                        direction: zz_terminal::SearchDirection::Forward,
                    },
                );
                mux.pane_images.insert(pane, PaneImageSnapshots::default());
                mux.pending_commands_revision = 17;
                mux.command_output = Some(CommandOutputModel { pane, retained });

                mux.attach_to_host(remote, SessionId(11), cx);

                assert_eq!(fake_remote.attached_session.get(), Some(SessionId(11)));
                assert_eq!(mux.attached_host, remote);
                assert_eq!(mux.core.attached_session(), None);
                assert_eq!(mux.core.snapshot().generation, 0);
                assert!(mux.core.snapshot().sessions.is_empty());
                assert!(mux.viewports.is_empty());
                assert!(mux.browser_commands.is_empty());
                assert!(mux.agent_commands.is_empty());
                assert!(mux.screenshot_requests.is_empty());
                assert!(mux.terminal_commands.is_empty());
                assert!(mux.pane_images.is_empty());
                assert!(mux.command_output.is_none());
                assert_eq!(mux.pending_commands_revision, 18);
            });
        });
    }

    #[gpui::test]
    fn a_switched_to_host_projects_its_cache_until_its_daemon_attaches(cx: &mut TestAppContext) {
        fn projected(mux: &MuxClient) -> Vec<(HostId, Option<u64>)> {
            mux.fleet_hosts()
                .map(|(host, _, _, snapshot)| (host, snapshot.map(|snapshot| snapshot.generation)))
                .collect()
        }

        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(vec![test_host("remote", "ssh://remote")], cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    std::path::PathBuf::from("/tmp/zz-fleet-switch-local.sock"),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;

            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, HostId::LOCAL);
                install_fake_connection(mux, remote);
                mux.connections.get_mut(&remote).unwrap().snapshot = Some(Arc::new(MuxSnapshot {
                    generation: 31,
                    focused_window: None,
                    sessions: vec![SessionSnapshot {
                        id: SessionId(11),
                        name: "builds".to_owned(),
                        active_window: WindowId(1),
                        windows: Vec::new(),
                        viewers: Vec::new(),
                    }],
                }));
                seed_attachment(
                    mux,
                    SessionId(7),
                    MuxSnapshot {
                        generation: 19,
                        focused_window: None,
                        sessions: Vec::new(),
                    },
                );

                assert!(mux.attach_to_host(remote, SessionId(11), cx));

                assert_eq!(
                    projected(mux),
                    vec![(HostId::LOCAL, Some(19)), (remote, Some(31))],
                );

                mux.handle_message(
                    remote,
                    ProtocolMessage::Attached {
                        session: SessionId(11),
                        snapshot: MuxSnapshot {
                            generation: 41,
                            focused_window: None,
                            sessions: Vec::new(),
                        },
                    },
                    cx,
                );

                assert_eq!(
                    projected(mux),
                    vec![(HostId::LOCAL, Some(19)), (remote, Some(41))],
                );
                assert!(
                    mux.fleet_hosts()
                        .any(|(host, _, _, snapshot)| host == remote
                            && snapshot.is_some_and(|snapshot| snapshot.sessions.is_empty()))
                );
            });
        });
    }

    #[gpui::test]
    fn switching_hosts_uses_the_latest_background_appearance(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host(
                    "remote",
                    "unix:///tmp/zz-background-appearance.sock",
                )],
                cx,
            );
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = mux.read(cx).registry.get_by_name("remote").unwrap().0;

            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, HostId::LOCAL);
                install_fake_connection(mux, remote);
                let appearance = TerminalAppearance {
                    font_size_points: 27.0,
                    ..TerminalAppearance::default()
                };
                mux.handle_message(
                    remote,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 1,
                        payload: EventPayload::AppearanceChanged {
                            appearance: Box::new(appearance),
                            provenance: AppearanceProvenance::default(),
                        },
                    }),
                    cx,
                );

                assert!(mux.attach_to_host(remote, SessionId(9), cx));
                assert_eq!(mux.appearance.font_size_points, 27.0);
            });
        });
    }

    #[gpui::test]
    fn snapshot_handles_share_one_allocation_until_replaced(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let client = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let first = client.read(cx).snapshot();
            let same = client.read(cx).snapshot();
            assert!(Arc::ptr_eq(&first, &same));

            client.update(cx, |client, cx| {
                client.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Attached {
                        session: SessionId(7),
                        snapshot: MuxSnapshot {
                            generation: 42,
                            focused_window: None,
                            sessions: Vec::new(),
                        },
                    },
                    cx,
                );
            });

            let replacement = client.read(cx).snapshot();
            assert!(!Arc::ptr_eq(&first, &replacement));
            assert_eq!(first.generation, 0);
            assert_eq!(replacement.generation, 42);
        });
    }

    #[gpui::test]
    fn focus_sidebar_events_advance_the_client_revision(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let client = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            assert_eq!(client.read(cx).sidebar_focus_revision(), 0);

            for (sequence, expected) in [(1, 1), (2, 2)] {
                client.update(cx, |client, cx| {
                    client.handle_message(
                        HostId::LOCAL,
                        ProtocolMessage::Event(zz_protocol::Event {
                            sequence,
                            payload: EventPayload::FocusSidebar,
                        }),
                        cx,
                    );
                });
                assert_eq!(client.read(cx).sidebar_focus_revision(), expected);
            }
        });
    }

    #[gpui::test]
    fn bell_events_from_attached_and_background_hosts_advance_the_client_revision(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host("remote", "unix:///tmp/zz-bell-remote.sock")],
                cx,
            );
            let client = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let remote = client.read(cx).registry.get_by_name("remote").unwrap().0;
            assert_eq!(client.read(cx).bell_revision(), 0);

            for (host, sequence, expected) in
                [(HostId::LOCAL, 1, 1), (remote, 2, 2), (HostId::LOCAL, 3, 3)]
            {
                client.update(cx, |client, cx| {
                    client.handle_message(
                        host,
                        ProtocolMessage::Event(zz_protocol::Event {
                            sequence,
                            payload: EventPayload::Bell { pane: PaneId(3) },
                        }),
                        cx,
                    );
                });
                assert_eq!(client.read(cx).bell_revision(), expected);
            }
        });
    }

    fn assert_points_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < f32::EPSILON,
            "expected {expected} points, got {actual}"
        );
    }

    #[test]
    fn terminal_font_adjustments_step_and_clamp_in_points() {
        assert_points_eq(
            adjusted_terminal_font_size(13.0, TerminalFontSizeAdjustment::Increase),
            14.0,
        );
        assert_points_eq(
            adjusted_terminal_font_size(13.0, TerminalFontSizeAdjustment::Decrease),
            12.0,
        );
        assert_points_eq(
            adjusted_terminal_font_size(
                MAX_TERMINAL_FONT_SIZE_POINTS,
                TerminalFontSizeAdjustment::Increase,
            ),
            MAX_TERMINAL_FONT_SIZE_POINTS,
        );
        assert_points_eq(
            adjusted_terminal_font_size(
                MIN_TERMINAL_FONT_SIZE_POINTS,
                TerminalFontSizeAdjustment::Decrease,
            ),
            MIN_TERMINAL_FONT_SIZE_POINTS,
        );
    }

    #[test]
    fn terminal_font_offset_survives_reload_and_normalizes_at_bounds() {
        let mut appearance = TerminalAppearance {
            font_size_points: 12.0,
            ..TerminalAppearance::default()
        };
        let offset = apply_terminal_font_size_offset(&mut appearance, 3.0);
        assert_points_eq(appearance.font_size_points, 15.0);
        assert_points_eq(offset, 3.0);

        let mut clamped = TerminalAppearance {
            font_size_points: 255.0,
            ..TerminalAppearance::default()
        };
        let offset = apply_terminal_font_size_offset(&mut clamped, 3.0);
        assert_points_eq(clamped.font_size_points, MAX_TERMINAL_FONT_SIZE_POINTS);
        assert_points_eq(offset, 1.0);
    }

    #[test]
    fn decoded_message_bridge_applies_backpressure_after_one_pending_message() {
        let (messages, incoming) = decoded_message_channel();
        messages
            .try_send(ProtocolMessage::Detach)
            .expect("first decoded message");
        assert!(matches!(
            messages.try_send(ProtocolMessage::Resync),
            Err(async_channel::TrySendError::Full(ProtocolMessage::Resync))
        ));
        assert_eq!(
            incoming.try_recv().expect("pending decoded message"),
            ProtocolMessage::Detach
        );
    }

    #[test]
    fn new_session_falls_back_to_an_explicit_attach_for_older_daemons() {
        assert_eq!(
            new_session_commands(HostId::LOCAL, &[]),
            vec![
                CommandInvocation::new("new-session", [] as [&str; 0]),
                CommandInvocation::new("attach-session", [] as [&str; 0]),
            ]
        );
        assert_eq!(
            new_session_commands(HostId::LOCAL, &[NEW_SESSION_ATTACH_CAPABILITY.to_owned()]),
            vec![CommandInvocation::new("new-session", [] as [&str; 0],)]
        );
    }

    #[gpui::test]
    fn new_session_routes_to_the_requested_host_and_keeps_remote_creation_detached(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(vec![test_host("remote", "ssh://remote")], cx);
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let (local_client, remote_client) = mux.update(cx, |mux, _cx| {
                let remote = mux.registry.get_by_name("remote").unwrap().0;
                let local_client = install_fake_connection(mux, HostId::LOCAL);
                let remote_client = install_fake_connection(mux, remote);

                mux.new_session(HostId::LOCAL);
                mux.new_session(remote);
                (local_client, remote_client)
            });

            assert_eq!(
                local_client.commands.borrow().as_slice(),
                [
                    CommandInvocation::new("new-session", [] as [&str; 0]),
                    CommandInvocation::new("attach-session", [] as [&str; 0]),
                ]
            );
            assert_eq!(
                remote_client.commands.borrow().as_slice(),
                [CommandInvocation::new("new-session", ["-d"])]
            );
        });
    }

    #[gpui::test]
    fn an_empty_daemon_attach_miss_recovers_through_resync(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let client = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            client.update(cx, |client, cx| {
                client.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Error {
                        request_id: 0,
                        error: ServerError::MissingTarget(String::new()),
                    }),
                    cx,
                );
                client.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 1,
                        payload: EventPayload::Snapshot(MuxSnapshot {
                            generation: 7,
                            focused_window: None,
                            sessions: Vec::new(),
                        }),
                    }),
                    cx,
                );
            });

            assert!(client.read(cx).error().is_none());
            assert_eq!(client.read(cx).snapshot().generation, 7);
            assert!(client.read(cx).snapshot().sessions.is_empty());
        });
    }

    #[gpui::test]
    fn late_input_for_an_exited_last_pane_does_not_mask_the_empty_workspace(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            for late_error in [
                ServerError::PaneExited(PaneId(0)),
                ServerError::PaneNotAttached(PaneId(0)),
            ] {
                for error_before_snapshot in [true, false] {
                    let client = cx.new(|cx| {
                        MuxClient::new(
                            Err(DaemonError::Thread("test client".to_owned())),
                            zz_daemon::default_socket_path(),
                            cx,
                        )
                    });
                    client.update(cx, |client, cx| {
                        client.error = None;
                        let late_input = || {
                            ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Error {
                                request_id: 0,
                                error: late_error.clone(),
                            })
                        };
                        let empty_snapshot = || {
                            ProtocolMessage::Event(zz_protocol::Event {
                                sequence: 1,
                                payload: EventPayload::Snapshot(MuxSnapshot {
                                    generation: 8,
                                    focused_window: None,
                                    sessions: Vec::new(),
                                }),
                            })
                        };

                        if error_before_snapshot {
                            client.handle_message(HostId::LOCAL, late_input(), cx);
                            client.handle_message(HostId::LOCAL, empty_snapshot(), cx);
                        } else {
                            client.handle_message(HostId::LOCAL, empty_snapshot(), cx);
                            client.handle_message(HostId::LOCAL, late_input(), cx);
                        }
                    });

                    assert!(client.read(cx).error().is_none(), "{late_error:?}");
                    assert_eq!(client.read(cx).snapshot().generation, 8);
                    assert!(client.read(cx).snapshot().sessions.is_empty());
                }
            }
        });
    }

    fn tracked_commands(mux: &MuxClient, host: HostId) -> Vec<(u64, String)> {
        mux.connections
            .get(&host)
            .expect("host connection")
            .in_flight_commands
            .read()
            .iter()
            .cloned()
            .collect()
    }

    fn test_mux(cx: &mut gpui::App) -> gpui::Entity<MuxClient> {
        cx.new(|cx| {
            MuxClient::new(
                Err(DaemonError::Thread("test client".to_owned())),
                zz_daemon::default_socket_path(),
                cx,
            )
        })
    }

    #[gpui::test]
    fn kitty_image_chunks_assemble_replace_remove_and_deduplicate(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mux = test_mux(cx);
            let pane = PaneId(7);
            mux.update(cx, |mux, _| {
                mux.begin_kitty_image(pane, 4, 1, 2, 1, 8);
                mux.push_kitty_image_chunk(pane, 4, 1, &[0, 1, 2]);
                assert!(
                    mux.kitty_images(pane)
                        .expect("pane cache")
                        .read()
                        .image(4, 1)
                        .is_none()
                );
                mux.push_kitty_image_chunk(pane, 4, 1, &[3, 4, 5, 6, 7]);
            });
            let cache = mux.read(cx).kitty_images(pane).expect("pane cache");
            let first = cache.read().image(4, 1).expect("assembled image");
            assert_eq!(first.as_bytes(0), Some([0, 1, 2, 3, 4, 5, 6, 7].as_slice()));

            mux.update(cx, |mux, _| {
                mux.begin_kitty_image(pane, 4, 1, 2, 1, 8);
                mux.push_kitty_image_chunk(pane, 4, 1, &[9; 8]);
            });
            let unchanged = cache.read().image(4, 1).expect("unchanged image");
            assert!(Arc::ptr_eq(&first, &unchanged));

            mux.update(cx, |mux, _| {
                mux.begin_kitty_image(pane, 4, 2, 1, 1, 4);
                mux.push_kitty_image_chunk(pane, 4, 2, &[8, 9, 10, 11]);
            });
            let replacement = cache.read().image(4, 2).expect("replacement image");
            assert!(!Arc::ptr_eq(&first, &replacement));
            assert_eq!(cache.write().take_retired().len(), 1);

            mux.update(cx, |mux, _| {
                mux.begin_kitty_image(pane, 4, 3, 1, 1, 4);
                mux.push_kitty_image_chunk(pane, 4, 3, &[0; 5]);
            });
            assert!(cache.read().image(4, 3).is_none());
            assert!(
                !mux.read(cx)
                    .kitty_image_assemblies
                    .contains_key(&(pane, 4, 3))
            );

            mux.update(cx, |mux, _| mux.remove_kitty_images(pane, &[4]));
            assert!(cache.read().image(4, 2).is_none());
        });
    }

    #[gpui::test]
    fn a_rejected_command_toasts_instead_of_setting_the_connection_error(cx: &mut TestAppContext) {
        let (_mux, _sink, notifications) = cx.update(|cx| {
            let mux = test_mux(cx);
            let (sink, notifications) = record_notifications(&mux, cx);
            mux.update(cx, |mux, cx| {
                let fake = install_fake_connection(mux, HostId::LOCAL);
                mux.error = None;
                mux.execute(CommandInvocation::new(
                    "swap-pane",
                    ["-s", "%1", "-t", "%2"],
                ));
                let request_id = fake.next_request_id.get() - 1;
                assert_eq!(
                    tracked_commands(mux, HostId::LOCAL),
                    [(request_id, "swap-pane".to_owned())]
                );

                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Error {
                        request_id,
                        error: ServerError::MissingTarget("%2".to_owned()),
                    }),
                    cx,
                );

                assert!(mux.error.is_none());
                assert!(tracked_commands(mux, HostId::LOCAL).is_empty());
            });
            (mux, sink, notifications)
        });

        assert_eq!(
            &*notifications.borrow(),
            &[(
                ClientMessageKind::Error,
                "swap-pane: target not found: %2".to_owned(),
            )]
        );
    }

    #[gpui::test]
    fn a_close_racing_the_panes_own_exit_stays_silent(cx: &mut TestAppContext) {
        let (_mux, _sink, notifications) = cx.update(|cx| {
            let mux = test_mux(cx);
            let (sink, notifications) = record_notifications(&mux, cx);
            mux.update(cx, |mux, cx| {
                for late_error in [
                    ServerError::PaneExited(PaneId(0)),
                    ServerError::PaneNotAttached(PaneId(0)),
                ] {
                    let fake = install_fake_connection(mux, HostId::LOCAL);
                    mux.error = None;
                    mux.execute(CommandInvocation::new("kill-pane", ["-t", "%0"]));
                    let request_id = fake.next_request_id.get() - 1;

                    mux.handle_message(
                        HostId::LOCAL,
                        ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Error {
                            request_id,
                            error: late_error.clone(),
                        }),
                        cx,
                    );

                    assert!(mux.error.is_none(), "{late_error:?}");
                    assert!(tracked_commands(mux, HostId::LOCAL).is_empty());
                }
            });
            (mux, sink, notifications)
        });

        assert!(notifications.borrow().is_empty());
    }

    #[gpui::test]
    fn command_tracking_prunes_on_success_and_stays_bounded(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mux = test_mux(cx);
            mux.update(cx, |mux, cx| {
                install_fake_connection(mux, HostId::LOCAL);
                mux.execute(CommandInvocation::new("select-pane", ["-t", "%1"]));
                mux.execute(CommandInvocation::new("resize-pane", ["-t", "%1"]));
                assert_eq!(
                    tracked_commands(mux, HostId::LOCAL),
                    [(1, "select-pane".to_owned()), (2, "resize-pane".to_owned())]
                );

                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Success {
                        request_id: 1,
                        output: String::new(),
                    }),
                    cx,
                );
                assert_eq!(
                    tracked_commands(mux, HostId::LOCAL),
                    [(2, "resize-pane".to_owned())]
                );

                for _ in 0..MAX_TRACKED_COMMANDS {
                    mux.execute(CommandInvocation::new("select-pane", ["-t", "%1"]));
                }
                let tracked = tracked_commands(mux, HostId::LOCAL);
                assert_eq!(tracked.len(), MAX_TRACKED_COMMANDS);
                assert_eq!(tracked.first().map(|(id, _)| *id), Some(3));
            });
        });
    }

    #[gpui::test]
    fn a_rejected_command_on_a_non_attached_host_is_reported(cx: &mut TestAppContext) {
        let (_mux, _sink, notifications) = cx.update(|cx| {
            crate::config::set_fleet_hosts_for_test(
                vec![test_host(
                    "remote",
                    "unix:///tmp/zz-fleet-command-error.sock",
                )],
                cx,
            );
            let mux = test_mux(cx);
            let (sink, notifications) = record_notifications(&mux, cx);
            mux.update(cx, |mux, cx| {
                let remote = mux.registry.get_by_name("remote").unwrap().0;
                let fake = install_fake_connection(mux, remote);
                mux.error = None;
                mux.execute_on_host(remote, CommandInvocation::new("kill-session", ["-t", "9"]));
                let request_id = fake.next_request_id.get() - 1;

                mux.handle_message(
                    remote,
                    ProtocolMessage::CommandResponse(zz_protocol::CommandResponse::Error {
                        request_id,
                        error: ServerError::MissingTarget("9".to_owned()),
                    }),
                    cx,
                );

                assert!(mux.error.is_none());
                assert!(tracked_commands(mux, remote).is_empty());
            });
            (mux, sink, notifications)
        });

        assert_eq!(
            &*notifications.borrow(),
            &[(
                ClientMessageKind::Error,
                "kill-session: target not found: 9".to_owned(),
            )]
        );
    }

    fn routing_pane(id: u64, browser: bool) -> PaneSnapshot {
        let id = PaneId(id);
        PaneSnapshot {
            id,
            title: id.to_string(),
            kind: if browser {
                PaneKindSnapshot::Browser(BrowserDescriptor::single(
                    "about:blank".to_owned(),
                    "zz-default".to_owned(),
                ))
            } else {
                PaneKindSnapshot::Terminal
            },
            synchronized_input: false,
            bell: false,
        }
    }

    fn routing_window(id: u64, layout: LayoutNode, panes: &[PaneSnapshot]) -> WindowSnapshot {
        let panes = panes
            .iter()
            .cloned()
            .map(|pane| (pane.id, pane))
            .collect::<BTreeMap<_, _>>();
        WindowSnapshot {
            id: WindowId(id),
            index: u32::try_from(id).expect("small fixture window id"),
            name: format!("window-{id}"),
            active_pane: panes.keys().next().copied().expect("fixture pane"),
            zoomed_pane: None,
            layout,
            panes,
        }
    }

    fn routing_snapshot(windows: Vec<WindowSnapshot>) -> MuxSnapshot {
        MuxSnapshot {
            generation: 1,
            focused_window: None,
            sessions: vec![SessionSnapshot {
                id: SessionId(7),
                name: "routing".to_owned(),
                active_window: windows.first().expect("fixture window").id,
                windows,
                viewers: Vec::new(),
            }],
        }
    }

    #[test]
    fn web_uri_routes_to_topologically_nearest_browser_in_the_source_window() {
        let layout = LayoutNode::Split {
            id: SplitId(10),
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Split {
                id: SplitId(11),
                axis: Axis::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(PaneId(1))),
                second: Box::new(LayoutNode::Pane(PaneId(2))),
            }),
            second: Box::new(LayoutNode::Split {
                id: SplitId(12),
                axis: Axis::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(PaneId(3))),
                second: Box::new(LayoutNode::Pane(PaneId(4))),
            }),
        };
        let window = routing_window(
            1,
            layout,
            &[
                routing_pane(1, true),
                routing_pane(2, false),
                routing_pane(3, true),
                routing_pane(4, false),
            ],
        );
        let snapshot = routing_snapshot(vec![window]);

        assert_eq!(
            open_uri_route(
                &snapshot,
                Some(SessionId(7)),
                PaneId(2),
                "https://example.com/docs?q=1"
            ),
            OpenUriRoute::Embedded {
                pane: PaneId(1),
                url: "https://example.com/docs?q=1".to_owned(),
            }
        );
    }

    #[test]
    fn uri_routing_falls_back_for_other_windows_and_unsupported_schemes() {
        let terminal_window =
            routing_window(1, LayoutNode::Pane(PaneId(2)), &[routing_pane(2, false)]);
        let browser_window =
            routing_window(2, LayoutNode::Pane(PaneId(3)), &[routing_pane(3, true)]);
        let snapshot = routing_snapshot(vec![terminal_window, browser_window]);

        assert_eq!(
            open_uri_route(
                &snapshot,
                Some(SessionId(7)),
                PaneId(2),
                "https://example.com"
            ),
            OpenUriRoute::External
        );

        let side_by_side = routing_window(
            3,
            LayoutNode::Split {
                id: SplitId(13),
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(PaneId(2))),
                second: Box::new(LayoutNode::Pane(PaneId(3))),
            },
            &[routing_pane(2, false), routing_pane(3, true)],
        );
        let snapshot = routing_snapshot(vec![side_by_side]);
        assert_eq!(
            open_uri_route(
                &snapshot,
                Some(SessionId(7)),
                PaneId(2),
                "mailto:hello@example.com"
            ),
            OpenUriRoute::External
        );
    }

    #[test]
    fn image_placeholders_route_back_to_their_own_pane() {
        let snapshot = routing_snapshot(vec![routing_window(
            1,
            LayoutNode::Split {
                id: SplitId(11),
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(PaneId(1))),
                second: Box::new(LayoutNode::Pane(PaneId(2))),
            },
            &[routing_pane(1, true), routing_pane(2, false)],
        )]);

        assert_eq!(
            open_uri_route(&snapshot, Some(SessionId(7)), PaneId(2), "zz-image://3"),
            OpenUriRoute::PastedImage {
                pane: PaneId(2),
                number: 3,
            }
        );
        for uri in ["zz-image://", "zz-image://x", "zz-image:3", "https://3"] {
            assert!(
                !matches!(
                    open_uri_route(&snapshot, Some(SessionId(7)), PaneId(2), uri),
                    OpenUriRoute::PastedImage { .. }
                ),
                "{uri} is not an image placeholder"
            );
        }
    }

    #[test]
    fn pane_image_snapshots_use_daemon_observed_numbers() {
        let mut snapshots = PaneImageSnapshots::default();
        snapshots.insert(
            12,
            Arc::new(Image::from_bytes(gpui::ImageFormat::Png, vec![12])),
        );
        snapshots.insert(
            1,
            Arc::new(Image::from_bytes(gpui::ImageFormat::Png, vec![1])),
        );

        assert_eq!(snapshots.get(12).expect("observed number").bytes, [12]);
        assert_eq!(snapshots.get(1).expect("restarted numbering").bytes, [1]);
        assert!(snapshots.get(2).is_none());

        let mut capped = PaneImageSnapshots::default();
        for number in 0..=u32::try_from(MAX_PANE_IMAGE_SNAPSHOTS).unwrap() {
            capped.insert(
                number,
                Arc::new(Image::from_bytes(
                    gpui::ImageFormat::Png,
                    vec![u8::try_from(number).unwrap()],
                )),
            );
        }
        assert!(capped.get(0).is_none());
        assert!(capped.get(1).is_some());
        assert_eq!(capped.images.len(), MAX_PANE_IMAGE_SNAPSHOTS);
    }

    #[gpui::test]
    fn pasted_image_chunks_populate_replace_and_invalidate_the_daemon_number(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            let mux = test_mux(cx);
            mux.update(cx, |mux, cx| {
                let pane = PaneId(9);
                mux.begin_pasted_image(pane, 12, PastedImageFormat::Png, 4);
                mux.push_pasted_image_chunk(pane, 12, &[1, 2], cx);
                assert!(mux.pasted_image(pane, 12).is_none());
                mux.push_pasted_image_chunk(pane, 12, &[3, 4], cx);
                assert_eq!(
                    mux.pasted_image(pane, 12).expect("assembled").bytes,
                    [1, 2, 3, 4]
                );

                mux.begin_pasted_image(pane, 12, PastedImageFormat::Webp, 2);
                assert!(mux.pasted_image(pane, 12).is_none());
                mux.push_pasted_image_chunk(pane, 12, &[8, 9], cx);
                assert_eq!(
                    mux.pasted_image(pane, 12).expect("replacement").bytes,
                    [8, 9]
                );
                mux.pasted_image_unavailable(pane, 12);
                assert!(mux.pasted_image(pane, 12).is_none());

                mux.pending_pasted_image_previews.insert((pane, 13));
                mux.begin_pasted_image(pane, 13, PastedImageFormat::Png, 2);
                mux.push_pasted_image_chunk(pane, 13, &[1, 2, 3], cx);
                assert!(!mux.pasted_image_assemblies.contains_key(&(pane, 13)));
                assert!(!mux.pending_pasted_image_previews.contains(&(pane, 13)));
            });
        });
    }

    fn history_fixture_viewport(
        row_ids: &[u32],
        generation: u64,
        total: u32,
        offset: u32,
    ) -> TerminalViewport {
        let mut viewport = TerminalViewport::blank(
            1,
            u16::try_from(row_ids.len()).expect("small fixture"),
            SessionStatus::Running,
        );
        viewport.generation = generation;
        viewport.view_generation = generation;
        viewport.scrollbar = ScrollbarState {
            total,
            offset,
            len: u32::try_from(row_ids.len()).expect("small fixture"),
        };
        for (cell, id) in Arc::make_mut(&mut viewport.cells).iter_mut().zip(row_ids) {
            *cell = PackedCell::new(0xe000 + *id, 0, CellWidth::Narrow);
        }
        viewport
    }

    fn retained_history_ids(retained: &RetainedTerminalViewport) -> Vec<u32> {
        retained
            .history
            .rows
            .iter()
            .map(|row| row.cells[0].glyph() - 0xe000)
            .collect()
    }

    fn chunk_rows(ids: &[u32]) -> Vec<Vec<PackedCell>> {
        ids.iter()
            .map(|id| vec![PackedCell::new(0xe000 + *id, 0, CellWidth::Narrow)])
            .collect()
    }

    fn history_backfill_debounce_fixture(
        cx: &mut gpui::App,
        pane: PaneId,
    ) -> (
        gpui::Entity<MuxClient>,
        Arc<FakeConnectedHost>,
        TerminalViewport,
    ) {
        let mux = cx.new(|cx| {
            MuxClient::new(
                Err(DaemonError::Thread("fixture".to_owned())),
                zz_daemon::default_socket_path(),
                cx,
            )
        });
        let fake = mux.update(cx, |mux, _| {
            let fake = install_fake_connection(mux, HostId::LOCAL);
            let mut options = MuxOptions::default();
            options.set(
                MuxOptionKey::HistoryTrickle,
                "600",
                zz_protocol::MuxOptionSource::RuntimeCommand,
            );
            mux.seed_core(EventPayload::MuxOptionsChanged { options });
            fake
        });
        let viewport = history_fixture_viewport(&[1_200, 1_201, 1_202], 1, 1_203, 1_200);
        mux.update(cx, |mux, cx| {
            mux.handle_message(
                HostId::LOCAL,
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::TerminalViewport {
                        pane,
                        viewport: viewport.clone(),
                    },
                }),
                cx,
            );
        });
        assert_eq!(&*fake.history_requests.borrow(), &[(pane, 688, 512)]);
        (mux, fake, viewport)
    }

    fn apply_history_output_scroll(
        mux: &gpui::Entity<MuxClient>,
        cx: &mut gpui::App,
        pane: PaneId,
        previous: &TerminalViewport,
        generation: u64,
    ) -> TerminalViewport {
        let generation_u32 = u32::try_from(generation).expect("small fixture generation");
        let first = 1_199 + generation_u32;
        let next =
            history_fixture_viewport(&[first, first + 1, first + 2], generation, first + 3, first);
        let patch = TerminalViewport::diff(previous, &next).expect("compatible output scroll");
        assert_eq!(patch.scroll, -1);
        mux.update(cx, |mux, cx| {
            mux.handle_message(
                HostId::LOCAL,
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: generation,
                    payload: EventPayload::TerminalPatch { pane, patch },
                }),
                cx,
            );
        });
        next
    }

    fn defer_initial_history_backfill(
        mux: &gpui::Entity<MuxClient>,
        cx: &mut gpui::App,
        pane: PaneId,
        initial: &TerminalViewport,
    ) -> TerminalViewport {
        let current = apply_history_output_scroll(mux, cx, pane, initial, 2);
        mux.update(cx, |mux, cx| {
            mux.handle_message(
                HostId::LOCAL,
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 3,
                    payload: EventPayload::HistoryChunk {
                        pane,
                        start: 688,
                        total: 1_203,
                        offset: 1_200,
                        columns: 1,
                        rows: chunk_rows(&vec![0; 512]),
                        dictionary: initial.dictionary.as_ref().clone(),
                    },
                }),
                cx,
            );
        });

        let mux = mux.read(cx);
        assert_eq!(
            retained_history_ids(&mux.viewports[&pane].read()),
            vec![1_200]
        );
        assert_eq!(
            mux.attached_connection()
                .history_backfill_deferred
                .get(&pane),
            Some(&1)
        );
        current
    }

    #[test]
    fn history_ring_matches_seeded_scrolls_eviction_and_invalidation() {
        for seed in [1_u32, 7, 42] {
            let mut next_revision = 1;
            let mut next_id = 11_u32;
            let mut history = vec![0, 1, 2];
            let mut live = (3..11).collect::<Vec<_>>();
            let initial = history_fixture_viewport(&live, 1, 11, 3);
            let dictionary = initial.dictionary.as_ref().clone();
            let mut retained = new_retained_viewport(initial, &mut next_revision);
            assert!(apply_history_chunk(
                &mut retained,
                0,
                11,
                3,
                1,
                chunk_rows(&history),
                dictionary,
                &mut next_revision,
            ));

            let mut random = seed;
            for generation in 2..82 {
                random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let shift = usize::try_from(random % 3 + 1).expect("small shift");
                history.extend_from_slice(&live[..shift]);
                if history.len() > 17 {
                    history.drain(..history.len() - 17);
                }
                live.drain(..shift);
                for _ in 0..shift {
                    live.push(next_id);
                    next_id += 1;
                }
                let next = history_fixture_viewport(
                    &live,
                    generation,
                    u32::try_from(history.len() + live.len()).expect("small fixture"),
                    u32::try_from(history.len()).expect("small fixture"),
                );
                let patch = TerminalViewport::diff(&retained.viewport, &next)
                    .expect("compatible scrolling viewport");
                assert_eq!(patch.scroll, -i16::try_from(shift).expect("small shift"));
                apply_retained_patch(&mut retained, patch, &mut next_revision)
                    .expect("apply output scroll");
                assert_eq!(retained_history_ids(&retained), history, "seed {seed}");
            }

            let reverse = 2_usize;
            let reverse_offset = history.len() - reverse;
            let reverse_rows = history[reverse_offset..]
                .iter()
                .chain(live[..live.len() - reverse].iter())
                .copied()
                .collect::<Vec<_>>();
            let reverse_viewport = history_fixture_viewport(
                &reverse_rows,
                82,
                u32::try_from(history.len() + live.len()).expect("small fixture"),
                u32::try_from(reverse_offset).expect("small fixture"),
            );
            let patch = TerminalViewport::diff(&retained.viewport, &reverse_viewport)
                .expect("compatible reverse scroll");
            assert_eq!(patch.scroll, i16::try_from(reverse).expect("small reverse"));
            apply_retained_patch(&mut retained, patch, &mut next_revision)
                .expect("apply reverse scroll");
            assert_eq!(
                retained_history_ids(&retained),
                history[..reverse_offset],
                "seed {seed} reverse"
            );

            let refill_dictionary = reverse_viewport.dictionary.as_ref().clone();
            replace_retained_viewport(&mut retained, reverse_viewport, &mut next_revision);
            assert!(retained.history.rows.is_empty());
            assert!(apply_history_chunk(
                &mut retained,
                0,
                u32::try_from(history.len() + live.len()).expect("small fixture"),
                u32::try_from(reverse_offset).expect("small fixture"),
                1,
                chunk_rows(&history[..reverse_offset]),
                refill_dictionary,
                &mut next_revision,
            ));

            let mut cleared = retained.viewport.clone();
            cleared.generation += 1;
            cleared.view_generation += 1;
            cleared.scrollbar = ScrollbarState {
                total: u32::try_from(live.len()).expect("small fixture"),
                offset: 0,
                len: u32::try_from(live.len()).expect("small fixture"),
            };
            let patch =
                TerminalViewport::diff(&retained.viewport, &cleared).expect("metadata clear patch");
            apply_retained_patch(&mut retained, patch, &mut next_revision)
                .expect("apply history clear");
            assert!(retained.history.rows.is_empty());

            retained.history.rows.push_back(HistoryRow {
                cells: chunk_rows(&[999]).remove(0).into_boxed_slice(),
                dictionary: Arc::clone(&retained.viewport.dictionary),
                revision: allocate_row_revision(&mut next_revision),
            });
            let wider = TerminalViewport::blank(2, 8, SessionStatus::Running);
            replace_retained_viewport(&mut retained, wider, &mut next_revision);
            assert!(retained.history.rows.is_empty());
        }
    }

    #[gpui::test]
    fn discarded_history_backfill_defers_and_suppresses_patch_retries(cx: &mut TestAppContext) {
        let pane = PaneId(74);
        let (mux, fake, initial) = cx.update(|cx| history_backfill_debounce_fixture(cx, pane));

        cx.update(|cx| {
            let current = defer_initial_history_backfill(&mux, cx, pane, &initial);
            apply_history_output_scroll(&mux, cx, pane, &current, 3);

            let mux = mux.read(cx);
            let retained = mux.viewports[&pane].read();
            assert_eq!(retained_history_ids(&retained), vec![1_200, 1_201]);
            assert_eq!(retained.history_mutations, 2);
            assert_eq!(
                mux.attached_connection()
                    .history_backfill_deferred
                    .get(&pane),
                Some(&1)
            );
        });

        assert_eq!(&*fake.history_requests.borrow(), &[(pane, 688, 512)]);
    }

    #[gpui::test]
    fn quiet_history_backfill_debounce_resumes_and_applies_chunk(cx: &mut TestAppContext) {
        let pane = PaneId(75);
        let (mux, fake, initial) = cx.update(|cx| history_backfill_debounce_fixture(cx, pane));
        let current = cx.update(|cx| defer_initial_history_backfill(&mux, cx, pane, &initial));

        cx.run_until_parked();
        cx.executor()
            .advance_clock(HISTORY_BACKFILL_QUIET + Duration::from_millis(1));
        cx.run_until_parked();

        assert_eq!(
            &*fake.history_requests.borrow(),
            &[(pane, 688, 512), (pane, 688, 512)]
        );
        cx.update(|cx| {
            assert!(
                !mux.read(cx)
                    .attached_connection()
                    .history_backfill_deferred
                    .contains_key(&pane)
            );
            let ids = (688..1_200).collect::<Vec<_>>();
            mux.update(cx, |mux, cx| {
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 4,
                        payload: EventPayload::HistoryChunk {
                            pane,
                            start: 688,
                            total: 1_204,
                            offset: 1_201,
                            columns: 1,
                            rows: chunk_rows(&ids),
                            dictionary: current.dictionary.as_ref().clone(),
                        },
                    }),
                    cx,
                );
            });

            let mux = mux.read(cx);
            let retained = mux.viewports[&pane].read();
            let retained_ids = retained_history_ids(&retained);
            assert_eq!(retained_ids.len(), 513);
            assert_eq!(retained_ids.first(), Some(&688));
            assert_eq!(retained_ids.last(), Some(&1_200));
        });
    }

    #[gpui::test]
    fn history_backfill_debounce_rearms_while_mutations_continue(cx: &mut TestAppContext) {
        let pane = PaneId(76);
        let (mux, fake, initial) = cx.update(|cx| history_backfill_debounce_fixture(cx, pane));
        let current = cx.update(|cx| defer_initial_history_backfill(&mux, cx, pane, &initial));
        cx.run_until_parked();

        let current = cx.update(|cx| apply_history_output_scroll(&mux, cx, pane, &current, 3));
        cx.executor()
            .advance_clock(HISTORY_BACKFILL_QUIET + Duration::from_millis(1));
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                mux.read(cx)
                    .attached_connection()
                    .history_backfill_deferred
                    .get(&pane),
                Some(&2)
            );
        });
        assert_eq!(fake.history_requests.borrow().len(), 1);

        cx.update(|cx| {
            apply_history_output_scroll(&mux, cx, pane, &current, 4);
        });
        cx.executor()
            .advance_clock(HISTORY_BACKFILL_QUIET + Duration::from_millis(1));
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                mux.read(cx)
                    .attached_connection()
                    .history_backfill_deferred
                    .get(&pane),
                Some(&3)
            );
        });
        assert_eq!(fake.history_requests.borrow().len(), 1);

        cx.executor()
            .advance_clock(HISTORY_BACKFILL_QUIET + Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(
            &*fake.history_requests.borrow(),
            &[(pane, 688, 512), (pane, 688, 512)]
        );
    }

    #[gpui::test]
    fn history_prefetch_bypasses_deferred_backfill(cx: &mut TestAppContext) {
        let pane = PaneId(77);
        let (mux, fake, initial) = cx.update(|cx| history_backfill_debounce_fixture(cx, pane));
        cx.update(|cx| {
            defer_initial_history_backfill(&mux, cx, pane, &initial);
            mux.update(cx, |mux, _| {
                assert!(
                    mux.attached_connection()
                        .history_backfill_deferred
                        .contains_key(&pane)
                );
                mux.request_history_prefetch(pane, 1_199);
                assert_eq!(
                    mux.attached_connection()
                        .history_requests_pending
                        .get(&pane)
                        .and_then(|request| request.prefetch_target),
                    Some(1_199)
                );
                assert!(
                    mux.attached_connection()
                        .history_backfill_deferred
                        .contains_key(&pane)
                );
            });
        });

        assert_eq!(
            &*fake.history_requests.borrow(),
            &[(pane, 688, 512), (pane, 1_196, 4)]
        );
    }

    #[gpui::test]
    fn history_trickle_requests_nearest_chunks_until_budget_and_zero_disables(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let fake = mux.update(cx, |mux, _| {
                let fake = install_fake_connection(mux, HostId::LOCAL);
                let mut options = MuxOptions::default();
                options.set(
                    MuxOptionKey::HistoryTrickle,
                    "600",
                    zz_protocol::MuxOptionSource::RuntimeCommand,
                );
                mux.seed_core(EventPayload::MuxOptionsChanged { options });
                fake
            });
            let pane = PaneId(71);
            let mut viewport = TerminalViewport::blank(1, 3, SessionStatus::Running);
            viewport.scrollbar = ScrollbarState {
                total: 1_203,
                offset: 1_200,
                len: 3,
            };
            let dictionary = viewport.dictionary.as_ref().clone();

            mux.update(cx, |mux, cx| {
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 1,
                        payload: EventPayload::TerminalViewport { pane, viewport },
                    }),
                    cx,
                );
            });
            assert_eq!(&*fake.history_requests.borrow(), &[(pane, 688, 512)]);

            mux.update(cx, |mux, cx| {
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 2,
                        payload: EventPayload::HistoryChunk {
                            pane,
                            start: 688,
                            total: 1_202,
                            offset: 1_200,
                            columns: 1,
                            rows: chunk_rows(&vec![0; 512]),
                            dictionary: dictionary.clone(),
                        },
                    }),
                    cx,
                );
            });
            assert_eq!(
                &*fake.history_requests.borrow(),
                &[(pane, 688, 512), (pane, 688, 512)]
            );

            mux.update(cx, |mux, cx| {
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 3,
                        payload: EventPayload::HistoryChunk {
                            pane,
                            start: 688,
                            total: 1_203,
                            offset: 1_200,
                            columns: 1,
                            rows: chunk_rows(&vec![1; 512]),
                            dictionary: dictionary.clone(),
                        },
                    }),
                    cx,
                );
            });
            assert_eq!(
                &*fake.history_requests.borrow(),
                &[(pane, 688, 512), (pane, 688, 512), (pane, 600, 88)]
            );

            mux.update(cx, |mux, cx| {
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 4,
                        payload: EventPayload::HistoryChunk {
                            pane,
                            start: 600,
                            total: 1_203,
                            offset: 1_200,
                            columns: 1,
                            rows: chunk_rows(&vec![2; 88]),
                            dictionary,
                        },
                    }),
                    cx,
                );
            });
            assert_eq!(fake.history_requests.borrow().len(), 3);
            assert_eq!(mux.read(cx).viewports[&pane].read().history.len(), 600);

            mux.update(cx, |mux, cx| {
                let mut options = MuxOptions::default();
                options.set(
                    MuxOptionKey::HistoryTrickle,
                    "0",
                    zz_protocol::MuxOptionSource::RuntimeCommand,
                );
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 5,
                        payload: EventPayload::MuxOptionsChanged { options },
                    }),
                    cx,
                );
                let disabled = PaneId(72);
                let mut viewport = TerminalViewport::blank(1, 3, SessionStatus::Running);
                viewport.scrollbar = ScrollbarState {
                    total: 103,
                    offset: 100,
                    len: 3,
                };
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 6,
                        payload: EventPayload::TerminalViewport {
                            pane: disabled,
                            viewport,
                        },
                    }),
                    cx,
                );
            });
            assert_eq!(fake.history_requests.borrow().len(), 3);
        });
    }

    #[gpui::test]
    fn local_scroll_prefetch_is_bounded_and_coalesces_while_in_flight(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("fixture".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let fake = mux.update(cx, |mux, _| {
                let fake = install_fake_connection(mux, HostId::LOCAL);
                let mut options = MuxOptions::default();
                options.set(
                    MuxOptionKey::HistoryTrickle,
                    "0",
                    zz_protocol::MuxOptionSource::RuntimeCommand,
                );
                mux.seed_core(EventPayload::MuxOptionsChanged { options });
                fake
            });
            let pane = PaneId(73);
            let mut viewport = TerminalViewport::blank(1, 3, SessionStatus::Running);
            viewport.scrollbar = ScrollbarState {
                total: 1_203,
                offset: 1_200,
                len: 3,
            };
            let dictionary = viewport.dictionary.as_ref().clone();
            mux.update(cx, |mux, cx| {
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 1,
                        payload: EventPayload::TerminalViewport { pane, viewport },
                    }),
                    cx,
                );
                mux.request_history_prefetch(pane, 1_199);
                mux.request_history_prefetch(pane, 1_198);
            });
            assert_eq!(&*fake.history_requests.borrow(), &[(pane, 1_196, 4)]);

            mux.update(cx, |mux, cx| {
                mux.handle_message(
                    HostId::LOCAL,
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 2,
                        payload: EventPayload::HistoryChunk {
                            pane,
                            start: 1_196,
                            total: 1_203,
                            offset: 1_200,
                            columns: 1,
                            rows: chunk_rows(&[1, 2, 3, 4]),
                            dictionary,
                        },
                    }),
                    cx,
                );
            });

            assert_eq!(
                &*fake.history_requests.borrow(),
                &[(pane, 1_196, 4), (pane, 1_195, 1)]
            );
            assert!(
                fake.history_requests
                    .borrow()
                    .iter()
                    .all(|(_, _, count)| *count <= MAX_HISTORY_CHUNK_ROWS)
            );
            let retained = mux.read(cx).viewports[&pane].read();
            assert_eq!(retained.history.len(), 4);
            let revisions = retained
                .history
                .rows
                .iter()
                .map(|row| row.revision)
                .collect::<BTreeSet<_>>();
            assert_eq!(revisions.len(), retained.history.len());
        });
    }

    #[test]
    fn retained_command_output_scroll_reuses_unchanged_row_revisions() {
        let mut previous = TerminalViewport::blank(1, 3, SessionStatus::Running);
        previous.generation = 1;
        for (cell, glyph) in Arc::make_mut(&mut previous.cells)
            .iter_mut()
            .zip(['a', 'b', 'c'])
        {
            *cell = PackedCell::new(u32::from(glyph), 0, CellWidth::Narrow);
        }
        let mut current = previous.clone();
        current.generation = 2;
        current.view_generation = 2;
        let cells = Arc::make_mut(&mut current.cells);
        cells[0] = cells[1];
        cells[1] = cells[2];
        cells[2] = PackedCell::new(u32::from('d'), 0, CellWidth::Narrow);
        let patch = TerminalViewport::diff(&previous, &current).expect("compatible output frame");
        assert_eq!(patch.scroll, -1);

        let history_scrollbar = previous.scrollbar;
        let mut retained = RetainedTerminalViewport {
            viewport: previous,
            history: HistoryRing::default(),
            history_scrollbar,
            history_mutations: 0,
            history_invalidations: 0,
            row_revisions: Box::new([10, 11, 12]),
            row_revision_epoch: 9,
            revision_scratch: Vec::new(),
        };
        let revision_address = retained.row_revisions.as_ptr();
        let mut next_revision = 20;
        apply_retained_patch(&mut retained, patch, &mut next_revision).expect("apply output patch");

        assert_eq!(retained.viewport, current);
        assert_eq!(&*retained.row_revisions, &[11, 12, 21]);
        assert_eq!(retained.row_revisions.as_ptr(), revision_address);
        assert_eq!(retained.row_revision_epoch, 22);
        assert_eq!(next_revision, 23);
        assert!(retained.revision_scratch.is_empty());
        assert!(retained.revision_scratch.capacity() > 0);

        let scratch_address = retained.revision_scratch.as_ptr();
        let mut next = current.clone();
        next.generation = 3;
        next.view_generation = 3;
        let cells = Arc::make_mut(&mut next.cells);
        cells[0] = cells[1];
        cells[1] = cells[2];
        cells[2] = PackedCell::new(u32::from('e'), 0, CellWidth::Narrow);
        let patch = TerminalViewport::diff(&current, &next).expect("second output patch");
        apply_retained_patch(&mut retained, patch, &mut next_revision)
            .expect("apply second output patch");

        assert_eq!(retained.viewport, next);
        assert_eq!(&*retained.row_revisions, &[12, 21, 24]);
        assert_eq!(retained.row_revisions.as_ptr(), revision_address);
        assert_eq!(retained.revision_scratch.as_ptr(), scratch_address);
        assert_eq!(retained.row_revision_epoch, 25);
        assert_eq!(next_revision, 26);

        let mut rejected = next.clone();
        rejected.generation = 4;
        rejected.view_generation = 4;
        Arc::make_mut(&mut rejected.cells)[2] =
            PackedCell::new(u32::from('f'), 0, CellWidth::Narrow);
        let mut patch = TerminalViewport::diff(&next, &rejected).expect("rejected patch");
        patch.base_generation = u64::MAX;
        let revisions_before = retained.row_revisions.to_vec();
        let epoch_before = retained.row_revision_epoch;
        assert!(apply_retained_patch(&mut retained, patch, &mut next_revision).is_err());
        assert_eq!(&*retained.row_revisions, revisions_before);
        assert_eq!(retained.row_revision_epoch, epoch_before);
        assert!(retained.revision_scratch.is_empty());

        let mut metadata = next.clone();
        metadata.view_generation = 4;
        metadata.overlays = Arc::from([OverlaySpan::new(0, 0, 1, OverlayKind::Selection)]);
        let patch = TerminalViewport::diff(&next, &metadata).expect("metadata patch");
        assert!(patch.changed_rows.is_empty());
        apply_retained_patch(&mut retained, patch, &mut next_revision)
            .expect("apply metadata patch");
        assert_eq!(retained.viewport, metadata);
        assert_eq!(retained.row_revisions.as_ptr(), revision_address);
        assert_eq!(retained.row_revision_epoch, epoch_before);
        assert_eq!(next_revision, 26);
    }
}
