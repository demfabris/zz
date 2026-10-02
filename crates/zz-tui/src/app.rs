use std::{
    collections::{HashMap, HashSet},
    mem,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use zz_client::{ClientCore, CoreEvent, InputEvent, Outbound, PrefixView};
use zz_daemon::{Endpoint, HostEntry, InteractiveClient};
use zz_protocol::{
    BrowserCommand, BrowserDescriptor, ClientExitAction, CommandInvocation, CommandResponse,
    GuiResponse, InputMessage, NEW_SESSION_ATTACH_CAPABILITY, PaneId, PaneKindSnapshot,
    ProtocolMessage, ServerError, ServerHello, TerminalUiCommand,
};
use zz_terminal::{SearchQuery, TerminalViewAction, TerminalViewport};

use crate::{
    browser::{BrowserFrameProvider, BrowserState, BrowserSurface, BrowserWait, SurfaceChanges},
    clipboard::{self, Osc52},
    input::{self, InputOutcome},
    kitty::{
        FILE_PROBE_IMAGE_ID, FrameTransport, KittyImageAssembler, KittyImageData, PROBE_IMAGE_ID,
    },
    render::{FrameDamage, Renderer, merge_damage},
    state::{ClientMessage, HostSwitch, Model},
    terminal_event::{Event as TerminalEvent, EventParser},
    tty::{MouseArming, TerminalGuard, TerminalOptions, TerminalSize},
};

#[cfg(unix)]
mod event_loop;
#[cfg(unix)]
use event_loop::EventLoop;

enum MainEvent {
    Core {
        connection: u64,
        event: Box<CoreEvent>,
    },
    Frames(u64),
    KittyImages(u64),
    Terminal(Result<TerminalEvent, String>),
    Disconnected {
        connection: u64,
        error: String,
    },
    Resize,
    Signal,
    Suspend,
    Resume,
    /// The writer dropped output it could not paint and its block has cleared.
    Repaint,
}

const KITTY_GATE_PROBING: u8 = 0;
const KITTY_GATE_ENABLED: u8 = 1;
const KITTY_GATE_DISABLED: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KittyProbeState {
    Idle,
    Probing,
    Enabled,
    Disabled,
}

struct KittyProbe {
    state: KittyProbeState,
    file_pending: bool,
    transport_override: Option<FrameTransport>,
    transport: FrameTransport,
}

impl KittyProbe {
    const fn new(transport_override: Option<FrameTransport>, probing: bool) -> Self {
        Self {
            state: if probing {
                KittyProbeState::Probing
            } else {
                KittyProbeState::Idle
            },
            file_pending: probing,
            transport_override,
            transport: match transport_override {
                Some(transport) => transport,
                None => FrameTransport::Inline,
            },
        }
    }

    const fn transport(&self) -> FrameTransport {
        self.transport
    }

    fn start(&mut self) -> bool {
        if self.state != KittyProbeState::Idle {
            return false;
        }
        self.state = KittyProbeState::Probing;
        self.file_pending = true;
        true
    }

    fn observe(&mut self, event: &TerminalEvent) -> KittyProbeUpdate {
        let mut update = KittyProbeUpdate::default();
        match event {
            TerminalEvent::KittyGraphicsResponse { image_id, ok }
                if *image_id == PROBE_IMAGE_ID =>
            {
                update.consumed = true;
                if self.state == KittyProbeState::Probing {
                    self.state = if *ok {
                        KittyProbeState::Enabled
                    } else {
                        KittyProbeState::Disabled
                    };
                    update.graphics = Some(*ok);
                }
            }
            TerminalEvent::KittyGraphicsResponse { image_id, ok }
                if *image_id == FILE_PROBE_IMAGE_ID =>
            {
                update.consumed = true;
                self.resolve_file_probe(*ok, &mut update);
            }
            TerminalEvent::DeviceAttributes => {
                update.consumed = true;
                if self.state == KittyProbeState::Probing {
                    self.state = KittyProbeState::Disabled;
                    update.graphics = Some(false);
                }
                self.resolve_file_probe(false, &mut update);
            }
            _ => {}
        }
        update
    }

    fn resolve_file_probe(&mut self, ok: bool, update: &mut KittyProbeUpdate) {
        if !self.file_pending {
            return;
        }
        self.file_pending = false;
        update.finish_file_probe = true;
        let resolved = resolve_frame_transport(ok, self.transport_override);
        if resolved != self.transport {
            self.transport = resolved;
            update.transport = Some(resolved);
        }
    }
}

#[derive(Default)]
struct KittyProbeUpdate {
    consumed: bool,
    graphics: Option<bool>,
    transport: Option<FrameTransport>,
    finish_file_probe: bool,
}

const fn resolve_frame_transport(
    file_supported: bool,
    transport_override: Option<FrameTransport>,
) -> FrameTransport {
    match transport_override {
        Some(transport) => transport,
        None if file_supported => FrameTransport::File,
        None => FrameTransport::Inline,
    }
}

fn configured_frame_transport_override() -> Option<FrameTransport> {
    let value = std::env::var("ZZ_TUI_FRAMES").ok()?;
    let transport = parse_frame_transport_override(&value);
    if transport.is_none() {
        log::warn!("ignoring invalid ZZ_TUI_FRAMES value {value:?}; expected file or inline");
    }
    transport
}

fn parse_frame_transport_override(value: &str) -> Option<FrameTransport> {
    match value.trim().to_ascii_lowercase().as_str() {
        "file" => Some(FrameTransport::File),
        "inline" => Some(FrameTransport::Inline),
        _ => None,
    }
}

#[derive(Default)]
struct FrameState {
    pending: HashMap<PaneId, PendingFrame>,
    wake_pending: bool,
}

struct PendingFrame {
    viewport: TerminalViewport,
    damage: FrameDamage,
}

#[derive(Default)]
struct FrameInbox(Mutex<FrameState>);

impl FrameInbox {
    fn publish(
        &self,
        pane: PaneId,
        viewport: TerminalViewport,
        damage: FrameDamage,
        connection: u64,
        events: &mpsc::Sender<MainEvent>,
    ) {
        let should_wake = {
            let mut state = self.0.lock().expect("frame inbox poisoned");
            state
                .pending
                .entry(pane)
                .and_modify(|pending| {
                    pending.viewport.clone_from(&viewport);
                    merge_damage(&mut pending.damage, damage.clone());
                })
                .or_insert(PendingFrame { viewport, damage });
            if state.wake_pending {
                false
            } else {
                state.wake_pending = true;
                true
            }
        };
        if should_wake {
            let _ = events.send(MainEvent::Frames(connection));
        }
    }

    fn take(&self) -> HashMap<PaneId, PendingFrame> {
        let mut state = self.0.lock().expect("frame inbox poisoned");
        state.wake_pending = false;
        mem::take(&mut state.pending)
    }

    fn recycle(&self, mut completed: HashMap<PaneId, PendingFrame>) {
        debug_assert!(completed.is_empty());
        let mut state = self.0.lock().expect("frame inbox poisoned");
        if completed.capacity() > state.pending.capacity() {
            completed.extend(state.pending.drain());
            state.pending = completed;
        }
    }

    fn clear(&self) {
        let mut state = self.0.lock().expect("frame inbox poisoned");
        state.pending.clear();
        state.wake_pending = false;
    }

    fn remove(&self, pane: PaneId) {
        self.0
            .lock()
            .expect("frame inbox poisoned")
            .pending
            .remove(&pane);
    }
}

enum KittyImageUpdate {
    Reset,
    Ready(KittyImageData),
    Removed { pane: PaneId, image_ids: Vec<u32> },
}

#[derive(Default)]
struct KittyImageState {
    assembler: KittyImageAssembler,
    pending: Vec<KittyImageUpdate>,
    wake_pending: bool,
}

#[derive(Default)]
struct KittyImageInbox(Mutex<KittyImageState>);

impl KittyImageInbox {
    fn begin(
        &self,
        pane: PaneId,
        image_id: u32,
        generation: u64,
        width: u32,
        height: u32,
        total_bytes: u32,
    ) {
        self.0
            .lock()
            .expect("Kitty image inbox poisoned")
            .assembler
            .begin(pane, image_id, generation, width, height, total_bytes);
    }

    fn push_chunk(
        &self,
        pane: PaneId,
        image_id: u32,
        generation: u64,
        bytes: Vec<u8>,
        connection: u64,
        events: &mpsc::Sender<MainEvent>,
    ) {
        let should_wake = {
            let mut state = self.0.lock().expect("Kitty image inbox poisoned");
            let Some(image) = state
                .assembler
                .push_chunk(pane, image_id, generation, bytes)
            else {
                return;
            };
            state.pending.push(KittyImageUpdate::Ready(image));
            if state.wake_pending {
                false
            } else {
                state.wake_pending = true;
                true
            }
        };
        if should_wake {
            let _ = events.send(MainEvent::KittyImages(connection));
        }
    }

    fn remove(
        &self,
        pane: PaneId,
        image_ids: Vec<u32>,
        connection: u64,
        events: &mpsc::Sender<MainEvent>,
    ) {
        if image_ids.is_empty() {
            return;
        }
        let should_wake = {
            let mut state = self.0.lock().expect("Kitty image inbox poisoned");
            state.assembler.remove(pane, &image_ids);
            state
                .pending
                .push(KittyImageUpdate::Removed { pane, image_ids });
            if state.wake_pending {
                false
            } else {
                state.wake_pending = true;
                true
            }
        };
        if should_wake {
            let _ = events.send(MainEvent::KittyImages(connection));
        }
    }

    fn remove_pane(&self, pane: PaneId) {
        let mut state = self.0.lock().expect("Kitty image inbox poisoned");
        state.assembler.remove_pane(pane);
        state.pending.retain(|update| match update {
            KittyImageUpdate::Reset => true,
            KittyImageUpdate::Ready(image) => image.pane != pane,
            KittyImageUpdate::Removed { pane: target, .. } => *target != pane,
        });
    }

    fn take(&self) -> Vec<KittyImageUpdate> {
        let mut state = self.0.lock().expect("Kitty image inbox poisoned");
        state.wake_pending = false;
        mem::take(&mut state.pending)
    }

    fn clear(&self) {
        let mut state = self.0.lock().expect("Kitty image inbox poisoned");
        state.assembler.clear();
        state.pending.clear();
        state.wake_pending = false;
    }

    fn reset_attachment(&self, connection: u64, events: &mpsc::Sender<MainEvent>) {
        let should_wake = {
            let mut state = self.0.lock().expect("Kitty image inbox poisoned");
            state.assembler.clear();
            state.pending.clear();
            state.pending.push(KittyImageUpdate::Reset);
            if state.wake_pending {
                false
            } else {
                state.wake_pending = true;
                true
            }
        };
        if should_wake {
            let _ = events.send(MainEvent::KittyImages(connection));
        }
    }
}

fn apply_kitty_updates(
    renderer: &mut Renderer,
    updates: Vec<KittyImageUpdate>,
    accept_images: bool,
) {
    for update in updates {
        match update {
            KittyImageUpdate::Reset => renderer.reset_kitty_images(),
            KittyImageUpdate::Ready(image) if accept_images => renderer.install_kitty_image(image),
            KittyImageUpdate::Removed { pane, image_ids } if accept_images => {
                renderer.remove_kitty_images(pane, &image_ids);
            }
            _ => {}
        }
    }
}

struct PreparedConnection {
    client: Arc<InteractiveClient>,
    core: Arc<Mutex<ClientCore>>,
}

enum HostSwitchDecision<T> {
    Current,
    Switch { host: HostSwitch, connected: T },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AttachAttempt {
    Idle,
    Default,
    Explicit,
    Remembered,
}

impl AttachAttempt {
    const fn is_pending(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

fn attach_attempt_owns_missing_response(attempt: AttachAttempt, error: &ServerError) -> bool {
    attempt.is_pending()
        && matches!(
            error,
            ServerError::MissingTarget(_) | ServerError::SessionNotFound(_)
        )
}

enum ProtocolOutcome {
    None,
    Repaint,
    RepaintAll,
    QueueControl(Vec<u8>),
    Exit(TuiExit),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PendingPaint {
    None,
    Frames,
    Repaint,
    RepaintAll,
}

const MAX_COALESCED_EVENTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
enum TuiExit {
    Detached(String),
    /// `detach-client -P`, `attach-session -x`, `new-session -X`: the pin
    /// prints a different notice and then hangs up its parent process.
    DetachedHangup(String),
    /// `detach-client -E`: the client process is replaced by `shell -c command`
    /// instead of printing a notice.
    Exec {
        command: String,
        shell: String,
    },
    Exited,
    ServerExited,
    ServerExitedUnexpectedly,
}

impl TuiExit {
    fn notice(&self) -> String {
        match self {
            Self::Detached(session) => format!("[detached (from session {session})]"),
            Self::DetachedHangup(session) => {
                format!("[detached and SIGHUP (from session {session})]")
            }
            Self::Exec { .. } => String::new(),
            Self::Exited => "[exited]".to_owned(),
            Self::ServerExited => "[server exited]".to_owned(),
            Self::ServerExitedUnexpectedly => "[server exited unexpectedly]".to_owned(),
        }
    }

    const fn exit_code(&self) -> u8 {
        match self {
            Self::Detached(_) | Self::DetachedHangup(_) | Self::Exec { .. } | Self::Exited => 0,
            Self::ServerExited | Self::ServerExitedUnexpectedly => 1,
        }
    }
}

pub(crate) enum InitialAttach {
    Connected { messages: Vec<ProtocolMessage> },
}

#[cfg(unix)]
pub(crate) fn run(
    initial: InteractiveClient,
    mut endpoint: Endpoint,
    local_endpoint: Endpoint,
    initial_attach: InitialAttach,
    terminal_options: Option<TerminalOptions>,
    host_label: String,
    local_host_label: String,
    fleet_hosts: Vec<HostEntry>,
    browser_provider: Option<Box<dyn BrowserFrameProvider>>,
) -> Result<(), String> {
    let InitialAttach::Connected {
        messages: initial_messages,
    } = initial_attach;
    let mut read_only = false;
    let mut client_flags = None;
    let attempt = AttachAttempt::Explicit;
    let size = TerminalSize::detect().map_err(|error| error.to_string())?;
    let mut core = seeded_core(initial.server_hello().clone());
    let TerminalOptions {
        extended_keys,
        focus_events,
    } = terminal_options.unwrap_or_default();
    let mut client = Arc::new(initial);
    let escape_time = Arc::new(AtomicU64::new(escape_timeout_ms(
        lock_core(&core).mux_options(),
    )));
    let mut terminal = TerminalGuard::enter(
        if mouse_option_enabled(lock_core(&core).mux_options()) {
            MouseArming::Button
        } else {
            MouseArming::Off
        },
        extended_keys,
        focus_events,
    )
    .map_err(|error| error.to_string())?;
    let pixel_mouse = terminal.pixel_mouse();
    let key_releases = terminal.kitty_keyboard();
    let output = terminal.writer();
    let mut renderer = Renderer::with_writer(std::rc::Rc::clone(&output));
    let mut browser = BrowserState::new(browser_provider);
    let mut kitty_probe = KittyProbe::new(
        configured_frame_transport_override(),
        terminal.kitty_probe_sent(),
    );
    renderer.set_frame_transport(kitty_probe.transport());
    browser.set_transport(kitty_probe.transport(), Instant::now());
    let kitty_gate = Arc::new(AtomicU8::new(KITTY_GATE_PROBING));
    let (events, incoming) = mpsc::channel();
    let mut frames = Arc::new(FrameInbox::default());
    let mut kitty_images = Arc::new(KittyImageInbox::default());
    let mut connection_id = 1;
    for message in initial_messages
        .into_iter()
        .chain(std::iter::from_fn(|| client.take_pending_message()))
    {
        if !forward_protocol_message(
            &core,
            message,
            connection_id,
            &events,
            &frames,
            &kitty_images,
            &kitty_gate,
            |outbound| match outbound {
                Outbound::RequestFull(pane) => client.request_full(pane),
                Outbound::TreeSync => client.request_tree_sync(),
            },
        ) {
            return Err("main event channel disconnected".to_owned());
        }
    }
    let mut model = Model::new(
        &lock_core(&core),
        size,
        host_label,
        local_host_label,
        endpoint.clone(),
        local_endpoint,
        fleet_hosts,
    );
    model.update_snapshot(Arc::clone(lock_core(&core).snapshot()));
    model.begin_client_focus_attach();
    let mut event_loop = EventLoop::new(&client).map_err(|error| error.to_string())?;

    let mut attempt = attempt;
    let mut creating_default = false;
    let mut remembered_session = None;
    let mut reconnect_available = true;
    let mut deferred = None;

    let outcome = loop {
        if browser.wants_graphics() {
            start_kitty_probe(&mut kitty_probe, &mut terminal)?;
        }
        let now = Instant::now();
        if model.expire_client_message(now) {
            renderer
                .paint(&model, false)
                .map_err(|error| error.to_string())?;
        }
        let now = Instant::now();
        let event = if let Some(event) = deferred.take() {
            Some(event)
        } else if browser.should_pump(now) {
            None
        } else {
            event_loop.receive(
                click_wait(&model, message_wait(&model, browser.wait(now), now), now),
                &incoming,
                &events,
                &client,
                &core,
                connection_id,
                &frames,
                &kitty_images,
                &kitty_gate,
                &escape_time,
                &output,
            )?
        };
        let Some(event) = event else {
            let now = Instant::now();
            if model
                .click
                .as_ref()
                .is_some_and(|sequence| sequence.deadline <= now)
            {
                input::expire_click_sequence(&mut model, &client)?;
            }
            if pump_browser_provider(&mut browser, &mut renderer, &model, &client, Instant::now())?
            {
                renderer
                    .paint(&model, false)
                    .map_err(|error| error.to_string())?;
            }
            remembered_session = model.attached_session.or(remembered_session);
            continue;
        };
        match event {
            event @ (MainEvent::Frames(_) | MainEvent::Core { .. } | MainEvent::KittyImages(_)) => {
                let mut paint = PendingPaint::None;
                let mut core_seen = false;
                let mut exit = None;
                let mut handled = 0_usize;
                let mut next = Some(event);
                while let Some(event) = next.take() {
                    match event {
                        MainEvent::Frames(event_connection) => {
                            if event_connection == connection_id {
                                take_frames(&frames, &mut model, &mut renderer);
                                paint = paint.max(PendingPaint::Frames);
                            }
                        }
                        MainEvent::KittyImages(event_connection) => {
                            if event_connection == connection_id {
                                let updates = kitty_images.take();
                                let accept_images = kitty_probe.state != KittyProbeState::Disabled;
                                if accept_images
                                    && updates
                                        .iter()
                                        .any(|update| !matches!(update, KittyImageUpdate::Reset))
                                {
                                    start_kitty_probe(&mut kitty_probe, &mut terminal)?;
                                }
                                let changed = !updates.is_empty();
                                apply_kitty_updates(&mut renderer, updates, accept_images);
                                if changed && kitty_probe.state == KittyProbeState::Enabled {
                                    paint = paint.max(PendingPaint::Repaint);
                                }
                            }
                        }
                        MainEvent::Core { connection, event } => {
                            if connection == connection_id {
                                core_seen = true;
                                if let CoreEvent::Attached { .. } = &*event {
                                    {
                                        let core = lock_core(&core);
                                        read_only = core.attached_read_only();
                                        client_flags = (!core.attached_client_flags().is_empty())
                                            .then(|| core.attached_client_flags().to_owned());
                                    }
                                    browser.reset_connection();
                                    reconnect_available = true;
                                }
                                if matches!(
                                    &*event,
                                    CoreEvent::MuxOptionsChanged
                                        | CoreEvent::HelloReceived
                                        | CoreEvent::Attached { .. }
                                        | CoreEvent::KeyTablesChanged
                                ) {
                                    refresh_terminal_options(&mut model, &core, &escape_time);
                                }
                                let popup_lifecycle_changed = matches!(
                                    &*event,
                                    CoreEvent::PopupChanged | CoreEvent::Attached { .. }
                                );
                                let previous_popup = popup_lifecycle_changed
                                    .then(|| model.popup.as_ref().map(|popup| popup.pane))
                                    .flatten();
                                if let CoreEvent::PaneRemoved { pane } = &*event {
                                    frames.remove(*pane);
                                    renderer.forget_pane(*pane);
                                }
                                let outcome = handle_core_event(
                                    &mut model,
                                    &core,
                                    &client,
                                    *event,
                                    &mut attempt,
                                    &mut creating_default,
                                    &mut browser,
                                )?;
                                if popup_lifecycle_changed
                                    && previous_popup
                                        != model.popup.as_ref().map(|popup| popup.pane)
                                    && let Some(pane) = previous_popup
                                {
                                    frames.remove(pane);
                                    renderer.forget_pane(pane);
                                }
                                match outcome {
                                    ProtocolOutcome::None => {}
                                    ProtocolOutcome::Repaint => {
                                        paint = paint.max(PendingPaint::Repaint);
                                    }
                                    ProtocolOutcome::RepaintAll => {
                                        paint = paint.max(PendingPaint::RepaintAll);
                                    }
                                    ProtocolOutcome::QueueControl(output) => {
                                        renderer.queue_control(output);
                                        paint = paint.max(PendingPaint::Repaint);
                                    }
                                    ProtocolOutcome::Exit(reason) => {
                                        exit = Some(reason);
                                        break;
                                    }
                                }
                            }
                        }
                        other => {
                            deferred = Some(other);
                            break;
                        }
                    }
                    handled += 1;
                    if handled >= MAX_COALESCED_EVENTS {
                        break;
                    }
                    next = next_paint_event(&incoming, &mut deferred);
                }
                if let Some(reason) = exit {
                    break Ok(reason);
                }
                if let Some(sequence) = sync_mouse_modes(&mut model, pixel_mouse) {
                    renderer.queue_control(sequence);
                    paint = paint.max(if core_seen {
                        PendingPaint::Repaint
                    } else {
                        PendingPaint::Frames
                    });
                }
                paint_pending(paint, &mut model, &client, &mut browser, &mut renderer)?;
            }
            MainEvent::Terminal(Ok(event)) => {
                let probe_update = kitty_probe.observe(&event);
                if probe_update.finish_file_probe {
                    terminal.finish_file_probe();
                }
                if let Some(transport) = probe_update.transport {
                    let now = Instant::now();
                    renderer.set_frame_transport(transport);
                    browser.set_transport(transport, now);
                    sync_browser_surfaces(&model, &mut browser, &mut renderer, now);
                    if kitty_probe.state == KittyProbeState::Enabled {
                        renderer
                            .paint(&model, false)
                            .map_err(|error| error.to_string())?;
                    }
                }
                if let Some(enabled) = probe_update.graphics {
                    if enabled {
                        kitty_gate.store(KITTY_GATE_ENABLED, Ordering::Release);
                        terminal.activate_kitty_graphics();
                        renderer.enable_kitty_graphics();
                        browser.enable();
                        sync_browser_surfaces(&model, &mut browser, &mut renderer, Instant::now());
                        renderer
                            .paint(&model, false)
                            .map_err(|error| error.to_string())?;
                    } else {
                        kitty_gate.store(KITTY_GATE_DISABLED, Ordering::Release);
                        kitty_images.clear();
                        browser.disable();
                        renderer.disable_kitty_graphics();
                    }
                }
                if probe_update.consumed {
                    continue;
                }
                let prefix = if let TerminalEvent::Key(event) = &event {
                    let input = input::key_input(*event);
                    let core = lock_core(&core);
                    PrefixView {
                        armed: core.prefix_armed(),
                        claimed: core.claims_prefix_input(&input),
                    }
                } else {
                    PrefixView::default()
                };
                match input::handle(
                    &mut model,
                    &client,
                    &mut browser,
                    event,
                    pixel_mouse,
                    key_releases,
                    prefix,
                )? {
                    InputOutcome::None => {}
                    InputOutcome::Repaint => renderer
                        .paint(&model, false)
                        .map_err(|error| error.to_string())?,
                    InputOutcome::RepaintAll => {
                        send_resizes_and_sync_browser(
                            &mut model,
                            &client,
                            &mut browser,
                            &mut renderer,
                        )?;
                        renderer.invalidate();
                        renderer
                            .paint(&model, true)
                            .map_err(|error| error.to_string())?;
                    }
                    InputOutcome::Resize(size) => {
                        let unchanged = size == model.size;
                        let same_grid =
                            (size.columns, size.rows) == (model.size.columns, model.size.rows);
                        model.set_size(size);
                        if size.columns > 0 && size.rows > 0 && !same_grid {
                            client
                                .send_input(InputMessage::ClientTerminalSizeV2 {
                                    layout_generation: model.layout_generation,
                                    columns: size.columns,
                                    rows: size.rows,
                                })
                                .map_err(|error| error.to_string())?;
                        }
                        if unchanged {
                            continue;
                        }
                        send_resizes_and_sync_browser(
                            &mut model,
                            &client,
                            &mut browser,
                            &mut renderer,
                        )?;
                        renderer.invalidate();
                        renderer
                            .paint(&model, true)
                            .map_err(|error| error.to_string())?;
                    }
                    InputOutcome::AttachRequested => {
                        attempt = AttachAttempt::Explicit;
                        creating_default = false;
                        renderer
                            .paint(&model, false)
                            .map_err(|error| error.to_string())?;
                    }
                    InputOutcome::SwitchHost(host) => {
                        let label = host.label.clone();
                        match prepare_host_switch(&endpoint, host, |target| {
                            prepare_connection(target, String::new(), false, None)
                        }) {
                            Ok(HostSwitchDecision::Current) => {}
                            Err(error) => {
                                model.client_message = Some(ClientMessage::local(format!(
                                    "could not connect to {label}: {error}"
                                )));
                                renderer
                                    .paint(&model, false)
                                    .map_err(|paint| paint.to_string())?;
                            }
                            Ok(HostSwitchDecision::Switch { host, connected }) => {
                                let next_endpoint = host.endpoint.clone();
                                let replacement = replace_connection(
                                    &mut client,
                                    &mut core,
                                    &mut event_loop,
                                    &mut connection_id,
                                    connected,
                                    &events,
                                    &mut frames,
                                    &mut kitty_images,
                                    &kitty_gate,
                                );
                                if let Err(error) = replacement {
                                    model.client_message = Some(ClientMessage::local(format!(
                                        "could not switch to {label}: {error}"
                                    )));
                                    renderer
                                        .paint(&model, false)
                                        .map_err(|paint| paint.to_string())?;
                                } else {
                                    browser.reset_connection();
                                    renderer.reset_kitty_images();
                                    read_only = false;
                                    client_flags = None;
                                    endpoint = next_endpoint;
                                    model.set_connected_host(host, &lock_core(&core));
                                    model.begin_client_focus_attach();
                                    refresh_terminal_options(&mut model, &core, &escape_time);
                                    if let Some(sequence) =
                                        sync_mouse_modes(&mut model, pixel_mouse)
                                    {
                                        renderer.queue_control(sequence);
                                    }
                                    model.client_message =
                                        Some(ClientMessage::local(format!("connected to {label}")));
                                    attempt = AttachAttempt::Default;
                                    creating_default = false;
                                    remembered_session = None;
                                    reconnect_available = true;
                                    renderer.invalidate();
                                    renderer
                                        .paint(&model, true)
                                        .map_err(|paint| paint.to_string())?;
                                }
                            }
                        }
                    }
                }
            }
            MainEvent::Terminal(Err(error)) => break Err(error),
            MainEvent::Disconnected { connection, error } => {
                if connection != connection_id {
                    continue;
                }
                if !reconnect_available {
                    break Ok(TuiExit::ServerExitedUnexpectedly);
                }
                reconnect_available = false;
                log::warn!("zz-tui connection closed: {error}");
                let session = remembered_session.or(model.attached_session);
                let Ok(replacement) = prepare_connection(
                    &endpoint,
                    session.map_or_else(String::new, |session| session.to_string()),
                    read_only,
                    client_flags.as_deref(),
                ) else {
                    break Ok(TuiExit::ServerExitedUnexpectedly);
                };
                attempt = if session.is_some() {
                    AttachAttempt::Remembered
                } else {
                    AttachAttempt::Default
                };
                creating_default = false;
                if replace_connection(
                    &mut client,
                    &mut core,
                    &mut event_loop,
                    &mut connection_id,
                    replacement,
                    &events,
                    &mut frames,
                    &mut kitty_images,
                    &kitty_gate,
                )
                .is_err()
                {
                    break Ok(TuiExit::ServerExitedUnexpectedly);
                }
                browser.reset_connection();
                renderer.reset_kitty_images();
                model.reset_connection(&lock_core(&core));
                model.begin_client_focus_attach();
                refresh_terminal_options(&mut model, &core, &escape_time);
                if let Some(sequence) = sync_mouse_modes(&mut model, pixel_mouse) {
                    renderer.queue_control(sequence);
                }
                model.client_message = Some(ClientMessage::local("reconnected"));
                renderer.invalidate();
                renderer
                    .paint(&model, true)
                    .map_err(|paint| paint.to_string())?;
            }
            MainEvent::Resize => {
                if let Ok(size) = TerminalSize::detect() {
                    let previous = model.size;
                    model.set_size(size);
                    crate::overlay::close_display_panes_on_resize(&model, &client, previous)?;
                    if size.columns > 0 && size.rows > 0 {
                        client
                            .send_input(InputMessage::ClientTerminalSizeV2 {
                                layout_generation: model.layout_generation,
                                columns: size.columns,
                                rows: size.rows,
                            })
                            .map_err(|error| error.to_string())?;
                    }
                    if size != previous {
                        send_resizes_and_sync_browser(
                            &mut model,
                            &client,
                            &mut browser,
                            &mut renderer,
                        )?;
                    }
                    renderer.invalidate();
                    renderer
                        .paint(&model, true)
                        .map_err(|error| error.to_string())?;
                }
            }
            MainEvent::Suspend => {
                client
                    .send_input(InputMessage::ClientSuspendState { suspended: true })
                    .map_err(|error| error.to_string())?;
                renderer.pause(true);
                terminal.suspend();
                #[cfg(unix)]
                rustix::process::kill_process(
                    rustix::process::getpid(),
                    rustix::process::Signal::STOP,
                )
                .map_err(|error| error.to_string())?;
            }
            MainEvent::Resume => {
                client
                    .send_input(InputMessage::ClientSuspendState { suspended: false })
                    .map_err(|error| error.to_string())?;
                #[cfg(unix)]
                terminal
                    .resume(model.mouse_arming, extended_keys, focus_events)
                    .map_err(|error| error.to_string())?;
                renderer.pause(false);
                renderer
                    .paint(&model, true)
                    .map_err(|error| error.to_string())?;
                let _ = events.send(MainEvent::Resize);
            }
            MainEvent::Signal => break Ok(TuiExit::Detached(attached_session_name(&model))),
            MainEvent::Repaint => {
                renderer.invalidate();
                renderer
                    .paint(&model, true)
                    .map_err(|error| error.to_string())?;
            }
        }
        remembered_session = model.attached_session.or(remembered_session);
    };

    browser.close_all();
    renderer.discard_queued_paints();
    drop(terminal);
    let deadline = Instant::now() + Duration::from_secs(1);
    while output.borrow().pending_fd().is_some() && Instant::now() < deadline {
        output
            .borrow_mut()
            .flush()
            .map_err(|error| error.to_string())?;
        std::thread::yield_now();
    }
    match outcome {
        Ok(TuiExit::Exec { command, shell }) => {
            // The pin execs before it would have printed any exit notice, so the
            // shell command inherits the client's terminal and process slot.
            Err(exec_client_command(&shell, &command))
        }
        Ok(exit) => {
            println!("{}", exit.notice());
            let hangup = matches!(exit, TuiExit::DetachedHangup(_));
            if exit.exit_code() == 0 {
                if hangup {
                    hangup_parent();
                }
                Ok(())
            } else {
                std::process::exit(i32::from(exit.exit_code()))
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
pub(crate) fn run(
    _initial: InteractiveClient,
    _endpoint: Endpoint,
    _local_endpoint: Endpoint,
    _initial_attach: InitialAttach,
    _terminal_options: Option<TerminalOptions>,
    _host_label: String,
    _local_host_label: String,
    _fleet_hosts: Vec<HostEntry>,
    _browser_provider: Option<Box<dyn BrowserFrameProvider>>,
) -> Result<(), String> {
    Err("zz-tui currently requires a Unix terminal".to_owned())
}

/// `kill(getppid(), SIGHUP)` guarded the way client.c guards it, so a reparented
/// client never signals init.
#[cfg(unix)]
fn hangup_parent() {
    use rustix::process::{Signal, getppid, kill_process};

    let Some(parent) = getppid() else {
        return;
    };
    if parent.as_raw_nonzero().get() > 1 {
        let _ = kill_process(parent, Signal::HUP);
    }
}

#[cfg(not(unix))]
const fn hangup_parent() {}

fn start_kitty_probe(probe: &mut KittyProbe, terminal: &mut TerminalGuard) -> Result<(), String> {
    if probe.start() {
        terminal
            .probe_kitty_graphics()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn take_frames(frames: &FrameInbox, model: &mut Model, renderer: &mut Renderer) {
    let mut pending = frames.take();
    for (pane, frame) in pending.drain() {
        if !model.accepts_viewport(pane) {
            model.viewports.remove(&pane);
            renderer.forget_pane(pane);
            continue;
        }
        model.viewports.insert(pane, frame.viewport);
        renderer.note_frame(pane, frame.damage);
    }
    frames.recycle(pending);
}

fn paint_pending(
    paint: PendingPaint,
    model: &mut Model,
    client: &InteractiveClient,
    browser: &mut BrowserState,
    renderer: &mut Renderer,
) -> Result<(), String> {
    match paint {
        PendingPaint::None => Ok(()),
        PendingPaint::Frames => renderer
            .paint_frames(model)
            .map_err(|error| error.to_string()),
        PendingPaint::Repaint => renderer
            .paint(model, false)
            .map_err(|error| error.to_string()),
        PendingPaint::RepaintAll => {
            send_resizes_and_sync_browser(model, client, browser, renderer)?;
            renderer.invalidate();
            renderer
                .paint(model, true)
                .map_err(|error| error.to_string())
        }
    }
}

/// Replace this process with `shell -c command`, matching `client_exec`.
#[cfg(unix)]
fn exec_client_command(shell: &str, command: &str) -> String {
    use std::os::unix::process::CommandExt as _;

    let argv0 = std::path::Path::new(shell).file_name().map_or_else(
        || shell.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let error = std::process::Command::new(shell)
        .arg0(argv0)
        .arg("-c")
        .arg(command)
        .env("SHELL", shell)
        .exec();
    format!("{shell}: {error}")
}

#[cfg(not(unix))]
fn exec_client_command(shell: &str, _command: &str) -> String {
    format!("{shell}: detach-client -E needs a Unix client")
}

fn prepare_connection(
    endpoint: &Endpoint,
    attach_target: String,
    read_only: bool,
    client_flags: Option<&str>,
) -> Result<PreparedConnection, String> {
    let mut args = Vec::new();
    if read_only {
        args.push("-r".to_owned());
    }
    if let Some(flags) = client_flags {
        args.extend(["-f".to_owned(), flags.to_owned()]);
    }
    if !attach_target.is_empty() {
        args.extend(["-t".to_owned(), attach_target]);
    }
    let client = InteractiveClient::connect_endpoint_with_attach(
        endpoint,
        zz_protocol::AttachOperation::Commands(vec![zz_protocol::PreparedCommand {
            invocation: CommandInvocation::new("attach-session", args),
            canonical_name: Some("attach-session".to_owned()),
            alias_matched: false,
            result: zz_protocol::PreparedCommandResult::Ready,
        }]),
    )
    .map_err(|error| error.to_string())?;
    let core = seeded_core(client.server_hello().clone());
    let client = Arc::new(client);
    Ok(PreparedConnection { client, core })
}

/// The handshake hello is consumed by [`InteractiveClient`] before the reader
/// thread exists, so it is fed to the core by hand; draining the resulting
/// events keeps the reader's first drain free of handshake leftovers.
fn seeded_core(hello: ServerHello) -> Arc<Mutex<ClientCore>> {
    let mut core = ClientCore::new();
    core.handle_message(ProtocolMessage::ServerHello(Box::new(hello)));
    while core.poll_event().is_some() {}
    Arc::new(Mutex::new(core))
}

fn lock_core(core: &Mutex<ClientCore>) -> MutexGuard<'_, ClientCore> {
    core.lock().expect("client core poisoned")
}

pub(crate) fn mouse_option_enabled(options: &zz_protocol::MuxOptions) -> bool {
    options
        .get(zz_protocol::MuxOptionKey::Mouse)
        .is_some_and(|option| option.value == "on")
}

/// The pin reads `focus-follows-mouse` from the session the client is attached
/// to; the daemon stamps that value into the map every client receives.
pub(crate) fn focus_follows_mouse_enabled(options: &zz_protocol::MuxOptions) -> bool {
    options
        .get(zz_protocol::MuxOptionKey::FocusFollowsMouse)
        .is_some_and(|option| option.value == "on")
}

fn escape_timeout_ms(options: &zz_protocol::MuxOptions) -> u64 {
    options
        .get(zz_protocol::MuxOptionKey::EscapeTime)
        .and_then(|option| option.value.parse::<u64>().ok())
        .unwrap_or(10)
        .max(1)
}

fn refresh_terminal_options(model: &mut Model, core: &Mutex<ClientCore>, escape_time: &AtomicU64) {
    let core = lock_core(core);
    let options = core.mux_options();
    escape_time.store(escape_timeout_ms(options), Ordering::Relaxed);
    model.mouse_option = mouse_option_enabled(options);
    model.focus_follows_mouse = focus_follows_mouse_enabled(options);
    model.mouse_bindings = core.mouse_bindings().keys().into_iter().collect();
    model.copy_mouse_bindings = core.mouse_bindings().copy_keys().into_iter().collect();
}

/// Every mouse key name the ROOT table carries a binding for. The raw TUI only
/// has to know WHETHER a gesture's name is bound before it hands the event to
/// the daemon, which then walks the table stack itself the way
/// `key_bindings_get` does. A gesture whose name the daemon then fails to find
/// would have been swallowed here for nothing, so the client offers exactly
/// the names the daemon's own lookup can reach: root always, and the mode
/// tables below when the pane the pointer landed on holds a mode.
#[cfg(test)]
pub(crate) fn mouse_binding_names(
    tables: &[zz_protocol::KeyTableSnapshot],
) -> std::collections::HashSet<String> {
    zz_protocol::MouseBindings::from_tables(tables)
        .keys()
        .into_iter()
        .collect()
}

fn desired_mouse_arming(model: &Model) -> MouseArming {
    let overlay_any = if let Some(menu) = model.menu.as_ref() {
        Some(menu.mouse_keys)
    } else {
        model.popup.as_ref().map(|popup| {
            model
                .viewports
                .get(&popup.pane)
                .is_some_and(|viewport| viewport.mouse_tracking)
        })
    };
    if !model.mouse_option {
        let tracking = overlay_any.unwrap_or_else(|| {
            model
                .active_viewport()
                .is_some_and(|viewport| viewport.mouse_tracking)
        });
        return if tracking {
            MouseArming::Any
        } else {
            MouseArming::Off
        };
    }
    let any = overlay_any.unwrap_or_else(|| {
        model.layout.panes.iter().any(|entry| {
            model
                .viewports
                .get(&entry.pane)
                .is_some_and(|viewport| viewport.mouse_tracking)
        })
    });
    if any || model.focus_follows_mouse {
        MouseArming::Any
    } else {
        MouseArming::Button
    }
}

fn sync_mouse_modes(model: &mut Model, pixel_mouse: bool) -> Option<Vec<u8>> {
    let desired = desired_mouse_arming(model);
    if desired == model.mouse_arming {
        return None;
    }
    model.mouse_arming = desired;
    Some(crate::tty::mouse_mode_sequence(desired, pixel_mouse))
}

fn prepare_host_switch<T>(
    current_endpoint: &Endpoint,
    host: HostSwitch,
    connect: impl FnOnce(&Endpoint) -> Result<T, String>,
) -> Result<HostSwitchDecision<T>, String> {
    if &host.endpoint == current_endpoint {
        return Ok(HostSwitchDecision::Current);
    }
    let connected = connect(&host.endpoint)?;
    Ok(HostSwitchDecision::Switch { host, connected })
}

#[cfg(unix)]
fn replace_connection(
    client: &mut Arc<InteractiveClient>,
    core: &mut Arc<Mutex<ClientCore>>,
    event_loop: &mut EventLoop,
    connection_id: &mut u64,
    connected: PreparedConnection,
    _events: &mpsc::Sender<MainEvent>,
    frames: &mut Arc<FrameInbox>,
    kitty_images: &mut Arc<KittyImageInbox>,
    _kitty_gate: &Arc<AtomicU8>,
) -> Result<(), String> {
    event_loop
        .replace(&connected.client)
        .map_err(|error| error.to_string())?;
    kitty_images.clear();
    *client = connected.client;
    *core = connected.core;
    *connection_id = connection_id.wrapping_add(1).max(1);
    *frames = Arc::new(FrameInbox::default());
    *kitty_images = Arc::new(KittyImageInbox::default());
    Ok(())
}

/// Drives one connection's [`ClientCore`]: decoded messages in, wire requests
/// straight back out, frames into the coalescing inbox, everything else to the
/// main loop in stream order.
fn forward_protocol_message(
    core: &Mutex<ClientCore>,
    message: ProtocolMessage,
    connection: u64,
    events: &mpsc::Sender<MainEvent>,
    frames: &FrameInbox,
    kitty_images: &KittyImageInbox,
    kitty_gate: &AtomicU8,
    mut send_outbound: impl FnMut(Outbound) -> Result<(), zz_daemon::DaemonError>,
) -> bool {
    let mut core = lock_core(core);
    core.handle_message(message);
    while let Some(outbound) = core.poll_outbound() {
        if let Err(error) = send_outbound(outbound) {
            log::warn!("failed to synchronize the terminal client: {error}");
        }
    }
    while let Some(event) = core.poll_event() {
        let event = match event {
            CoreEvent::ViewportChanged { pane, damage } => {
                if let Some(viewport) = core.viewport(pane) {
                    frames.publish(pane, viewport.clone(), damage, connection, events);
                }
                continue;
            }
            CoreEvent::KittyImageBegin {
                pane,
                image_id,
                generation,
                width,
                height,
                total_bytes,
            } => {
                if kitty_gate.load(Ordering::Acquire) != KITTY_GATE_DISABLED {
                    kitty_images.begin(pane, image_id, generation, width, height, total_bytes);
                }
                continue;
            }
            CoreEvent::KittyImageChunk {
                pane,
                image_id,
                generation,
                bytes,
            } => {
                if kitty_gate.load(Ordering::Acquire) != KITTY_GATE_DISABLED {
                    kitty_images.push_chunk(pane, image_id, generation, bytes, connection, events);
                }
                continue;
            }
            CoreEvent::KittyImagesRemoved { pane, image_ids } => {
                if kitty_gate.load(Ordering::Acquire) != KITTY_GATE_DISABLED {
                    kitty_images.remove(pane, image_ids, connection, events);
                }
                continue;
            }
            CoreEvent::Attached { session } => {
                frames.clear();
                if events
                    .send(MainEvent::Core {
                        connection,
                        event: Box::new(CoreEvent::Attached { session }),
                    })
                    .is_err()
                {
                    return false;
                }
                kitty_images.reset_attachment(connection, events);
                continue;
            }
            CoreEvent::PaneRemoved { pane } => {
                kitty_images.remove_pane(pane);
                CoreEvent::PaneRemoved { pane }
            }
            event => event,
        };
        if events
            .send(MainEvent::Core {
                connection,
                event: Box::new(event),
            })
            .is_err()
        {
            return false;
        }
    }
    true
}

/// Refreshes the [`Model`] caches the event touched and decides how much of the
/// screen that costs. State changes are notifications: the new value is read
/// back from the core, side effects travel in the event itself.
fn handle_core_event(
    model: &mut Model,
    core: &Mutex<ClientCore>,
    client: &InteractiveClient,
    event: CoreEvent,
    attempt: &mut AttachAttempt,
    creating_default: &mut bool,
    browser: &mut BrowserState,
) -> Result<ProtocolOutcome, String> {
    match event {
        CoreEvent::Attached { session } => {
            *attempt = AttachAttempt::Idle;
            model.attached_session = Some(session);
            model.set_command_output(None, None);
            model.set_popup(None);
            model.popup_keys_down.clear();
            model.set_menu(None);
            model.confirm = None;
            model.confirm_reply_pending = false;
            model.client_message = None;
            let (snapshot, viewports, layout_generation) = {
                let core = lock_core(core);
                let snapshot = Arc::clone(core.snapshot());
                let viewports = snapshot
                    .sessions
                    .iter()
                    .flat_map(|session| &session.windows)
                    .flat_map(|window| window.panes.keys())
                    .filter_map(|pane| Some((*pane, core.viewport(*pane)?.clone())))
                    .collect();
                (snapshot, viewports, core.layout_generation())
            };
            update_snapshot(model, snapshot, layout_generation);
            model.viewports = viewports;
            if let Some(input) = model.finish_client_focus_attach() {
                client
                    .send_input(input)
                    .map_err(|error| error.to_string())?;
            }
            *creating_default = false;
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::SnapshotChanged => {
            let (snapshot, layout_generation) = {
                let core = lock_core(core);
                (Arc::clone(core.snapshot()), core.layout_generation())
            };
            refresh_snapshot(model, snapshot, layout_generation, |input| {
                client.send_input(input).map_err(|error| error.to_string())
            })
        }
        CoreEvent::AppearanceChanged => {
            model.appearance = lock_core(core).appearance().cloned().unwrap_or_default();
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::StatusChanged => {
            let status = lock_core(core).status().clone();
            if model.set_status(status) {
                Ok(ProtocolOutcome::RepaintAll)
            } else {
                Ok(ProtocolOutcome::Repaint)
            }
        }
        CoreEvent::PrefixArmed { armed } => {
            model.prefix_armed = armed;
            Ok(ProtocolOutcome::Repaint)
        }
        CoreEvent::CommandPromptChanged => {
            let prompt = lock_core(core).command_prompt().cloned();
            let closed = model.command_prompt.is_some() && prompt.is_none();
            model.command_prompt = prompt;
            Ok(if closed {
                ProtocolOutcome::RepaintAll
            } else {
                ProtocolOutcome::Repaint
            })
        }
        CoreEvent::CommandOutputChanged => {
            let (output_id, output) = {
                let core = lock_core(core);
                let output = core
                    .command_output()
                    .map(|(pane, viewport)| (pane, viewport.clone()));
                (core.command_output_id(), output)
            };
            model.set_command_output(output_id, output);
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::ChooseTreeChanged => {
            let core = lock_core(core);
            model.choose_tree = core.choose_tree().cloned();
            model.chooser_presentation = core.chooser_presentation().cloned();
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::ChooseBufferChanged => {
            let core = lock_core(core);
            model.choose_buffer = core.choose_buffer().cloned();
            model.chooser_presentation = core.chooser_presentation().cloned();
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::DisplayPanesChanged => {
            model.display_panes = lock_core(core).display_panes().cloned();
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::PopupChanged => {
            model.set_popup(lock_core(core).popup().cloned());
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::MenuChanged => {
            model.sync_menu(&lock_core(core));
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::ConfirmChanged => {
            model.confirm = lock_core(core).confirm().cloned();
            model.confirm_reply_pending = false;
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::ClientMessage {
            text,
            duration_ms,
            message_id,
            ..
        } => {
            model.client_message = Some(ClientMessage::timed(
                text,
                message_id,
                duration_ms,
                Instant::now(),
            ));
            Ok(ProtocolOutcome::Repaint)
        }
        CoreEvent::ClientMessageCleared { message_id } => {
            if model.clear_client_message(message_id) {
                Ok(ProtocolOutcome::Repaint)
            } else {
                Ok(ProtocolOutcome::None)
            }
        }
        CoreEvent::PaneRemoved { pane } => {
            model.input_event(InputEvent::PaneRemoved(pane));
            model.viewports.remove(&pane);
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::Detached {
            session,
            by: _,
            action,
        } if model.attached_session == Some(session) => {
            model.input_event(InputEvent::Detached);
            let core = lock_core(core);
            let exit = if let ClientExitAction::Exec { command, shell } = action {
                TuiExit::Exec { command, shell }
            } else if core.last_detach_was_session_destroyed() {
                TuiExit::Exited
            } else if core.last_detach_was_server_stopping() {
                TuiExit::ServerExited
            } else if action.is_parent_hangup() {
                TuiExit::DetachedHangup(attached_session_name(model))
            } else {
                TuiExit::Detached(attached_session_name(model))
            };
            Ok(ProtocolOutcome::Exit(exit))
        }
        CoreEvent::ServerStopping => Ok(ProtocolOutcome::Exit(TuiExit::ServerExited)),
        CoreEvent::AgentCommand { request_id, .. } => {
            client
                .send_gui_response(GuiResponse::Error {
                    request_id,
                    message: "agent commands require the zz app".to_owned(),
                })
                .map_err(|error| error.to_string())?;
            Ok(ProtocolOutcome::None)
        }
        CoreEvent::Clipboard {
            producer,
            target,
            text,
            ..
        } => match clipboard::encode(clipboard::Selection::for_producer(producer, target), &text) {
            Osc52::Empty => Ok(ProtocolOutcome::None),
            Osc52::Encoded(output) => Ok(ProtocolOutcome::QueueControl(output)),
            Osc52::TooLarge => {
                model.client_message =
                    Some(ClientMessage::local("clipboard payload exceeds 1 MiB"));
                Ok(ProtocolOutcome::Repaint)
            }
        },
        CoreEvent::FocusSidebar => {
            if model.focus_sidebar() {
                Ok(ProtocolOutcome::RepaintAll)
            } else {
                Ok(ProtocolOutcome::Repaint)
            }
        }
        CoreEvent::BrowserCommand {
            command: BrowserCommand::Screenshot { request_id, .. },
            ..
        } => {
            client
                .send_gui_response(GuiResponse::Error {
                    request_id,
                    message: "browser screenshots require the zz app".to_owned(),
                })
                .map_err(|error| error.to_string())?;
            Ok(ProtocolOutcome::None)
        }
        CoreEvent::BrowserCommand { pane, command } => {
            browser.command(pane, &command);
            Ok(ProtocolOutcome::None)
        }
        CoreEvent::TerminalUiCommand { pane, command } => {
            if let Some(action) = command_output_ui_action(model, pane, command) {
                client
                    .send_input(InputMessage::CommandOutputView { action })
                    .map_err(|error| error.to_string())?;
                Ok(ProtocolOutcome::Repaint)
            } else {
                model.client_message =
                    Some(ClientMessage::local("terminal search is unsupported here"));
                Ok(ProtocolOutcome::Repaint)
            }
        }
        CoreEvent::CommandResponse(response) => {
            handle_command_response(model, core, client, response, attempt, creating_default)
        }
        CoreEvent::HelloReceived
        | CoreEvent::Detached { .. }
        | CoreEvent::ViewportChanged { .. }
        | CoreEvent::MuxOptionsChanged
        | CoreEvent::KeyTablesChanged
        | CoreEvent::KeyTableChanged
        | CoreEvent::PrefixCancelled { .. }
        | CoreEvent::Bell { .. }
        | CoreEvent::OpenPathPicker { .. }
        | CoreEvent::OpenUri { .. }
        | CoreEvent::HistoryChunk { .. }
        | CoreEvent::KittyImageBegin { .. }
        | CoreEvent::KittyImageChunk { .. }
        | CoreEvent::KittyImagesRemoved { .. }
        // The agent lane needs a transcript reducer the TUI does not have; its
        // panes stay the static card `placeholder_text` paints.
        | CoreEvent::AgentUpdates { .. }
        | CoreEvent::AgentStateChanged { .. }
        | CoreEvent::AgentLagged { .. }
        | CoreEvent::AgentSessions { .. }
        | CoreEvent::Message(_) => Ok(ProtocolOutcome::None),
    }
}

fn command_output_ui_action(
    model: &mut Model,
    pane: PaneId,
    command: TerminalUiCommand,
) -> Option<TerminalViewAction> {
    let active = model
        .command_output
        .as_ref()
        .is_some_and(|(output_pane, _)| *output_pane == pane);
    if !active {
        return None;
    }
    match command {
        TerminalUiCommand::BeginSearch { direction } => {
            let query = SearchQuery {
                direction,
                ..SearchQuery::default()
            };
            model.command_output_search = Some(query.clone());
            model.command_output_swallowed_key = None;
            Some(TerminalViewAction::SearchBegin(query))
        }
    }
}

fn attached_session_name(model: &Model) -> String {
    model
        .attached_session
        .and_then(|attached| {
            model
                .snapshot
                .sessions
                .iter()
                .find(|session| session.id == attached)
        })
        .map(|session| session.name.clone())
        .or_else(|| model.attached_session.map(|session| session.to_string()))
        .unwrap_or_default()
}

fn handle_command_response(
    model: &mut Model,
    core: &Mutex<ClientCore>,
    client: &InteractiveClient,
    response: CommandResponse,
    attempt: &mut AttachAttempt,
    creating_default: &mut bool,
) -> Result<ProtocolOutcome, String> {
    match response {
        CommandResponse::Error {
            request_id: 0,
            error,
            ..
        } if attach_attempt_owns_missing_response(*attempt, &error) => {
            let (ServerError::MissingTarget(target) | ServerError::SessionNotFound(target)) = error
            else {
                unreachable!("attach response guard accepted a different error")
            };
            match *attempt {
                AttachAttempt::Remembered => {
                    if let Err(error) = client.attach("") {
                        recover_client_focus_after_attach_error(model, client)?;
                        *attempt = AttachAttempt::Idle;
                        return Err(error.to_string());
                    }
                    model.begin_client_focus_attach();
                    *attempt = AttachAttempt::Default;
                    Ok(ProtocolOutcome::None)
                }
                AttachAttempt::Default if !*creating_default => {
                    *creating_default = true;
                    let creation = (|| {
                        client.request_resync().map_err(|error| error.to_string())?;
                        client
                            .execute(CommandInvocation::new("new-session", [] as [&str; 0]))
                            .map_err(|error| error.to_string())?;
                        let attaches = lock_core(core)
                            .capabilities()
                            .iter()
                            .any(|capability| capability == NEW_SESSION_ATTACH_CAPABILITY);
                        if !attaches {
                            client
                                .execute(CommandInvocation::new("attach-session", [] as [&str; 0]))
                                .map_err(|error| error.to_string())?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = creation {
                        recover_client_focus_after_attach_error(model, client)?;
                        *attempt = AttachAttempt::Idle;
                        return Err(error);
                    }
                    Ok(ProtocolOutcome::Repaint)
                }
                AttachAttempt::Default => Ok(ProtocolOutcome::None),
                AttachAttempt::Explicit if model.attached_session.is_some() => {
                    recover_client_focus_after_attach_error(model, client)?;
                    *attempt = AttachAttempt::Idle;
                    model.client_message = Some(ClientMessage::local(format!(
                        "session `{target}` was not found"
                    )));
                    Ok(ProtocolOutcome::Repaint)
                }
                AttachAttempt::Explicit => {
                    recover_client_focus_after_attach_error(model, client)?;
                    *attempt = AttachAttempt::Idle;
                    Err(format!("session `{target}` was not found"))
                }
                AttachAttempt::Idle => {
                    unreachable!("attach response guard requires a pending attempt")
                }
            }
        }
        CommandResponse::Error {
            request_id: 0,
            error: ServerError::PaneExited(_) | ServerError::PaneNotAttached(_),
            ..
        } => Ok(ProtocolOutcome::None),
        CommandResponse::Error {
            request_id: 0,
            error: ServerError::InvalidCommand(message),
            ..
        } if attempt.is_pending()
            && model.attached_session.is_none()
            && message == "sessions should be nested with care, unset $TMUX to force" =>
        {
            recover_client_focus_after_attach_error(model, client)?;
            *attempt = AttachAttempt::Idle;
            Err(message)
        }
        CommandResponse::Error { error, .. } => {
            model.client_message = Some(ClientMessage::local(error.tmux_message()));
            Ok(ProtocolOutcome::Repaint)
        }
        CommandResponse::Success { .. } => {
            model.client_message = None;
            Ok(ProtocolOutcome::Repaint)
        }
    }
}

fn recover_client_focus_after_attach_error(
    model: &mut Model,
    client: &InteractiveClient,
) -> Result<(), String> {
    if let Some(input) = model.fail_client_focus_attach() {
        client
            .send_input(input)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Tighten the browser's wait so the loop wakes up when a message's own
/// duration runs out. Producers that carry no daemon deadline — the alert and
/// read-only paths — expire on this timer alone.
fn message_wait(model: &Model, wait: BrowserWait, now: Instant) -> BrowserWait {
    let Some(deadline) = model.client_message_deadline() else {
        return wait;
    };
    let remaining = deadline.saturating_duration_since(now);
    match wait {
        BrowserWait::Blocking => BrowserWait::Timeout(remaining),
        BrowserWait::Timeout(timeout) => BrowserWait::Timeout(timeout.min(remaining)),
    }
}

/// `evtimer_add(&c->click_timer, ...)`: the loop stops blocking long enough to
/// notice a click sequence that has run out of time.
fn click_wait(model: &Model, wait: BrowserWait, now: Instant) -> BrowserWait {
    let Some(sequence) = model.click.as_ref() else {
        return wait;
    };
    let remaining = sequence.deadline.saturating_duration_since(now);
    match wait {
        BrowserWait::Blocking => BrowserWait::Timeout(remaining),
        BrowserWait::Timeout(timeout) => BrowserWait::Timeout(timeout.min(remaining)),
    }
}

fn next_paint_event(
    incoming: &mpsc::Receiver<MainEvent>,
    deferred: &mut Option<MainEvent>,
) -> Option<MainEvent> {
    match incoming.try_recv().ok() {
        Some(
            event @ (MainEvent::Frames(_) | MainEvent::Core { .. } | MainEvent::KittyImages(_)),
        ) => Some(event),
        event => {
            *deferred = event;
            None
        }
    }
}

fn pump_browser_provider(
    browser: &mut BrowserState,
    renderer: &mut Renderer,
    model: &Model,
    client: &InteractiveClient,
    now: Instant,
) -> Result<bool, String> {
    let output = browser.pump(now);
    let changed = !output.frames.is_empty();
    for frame in output.frames {
        let pane = frame.image.pane;
        let transmitted = renderer.install_browser_frame(frame);
        browser.note_transmit_cost(pane, transmitted);
    }
    for (pane, tabs, active) in output.navigations {
        let Some(snapshot) = model.pane_snapshot(pane) else {
            continue;
        };
        let PaneKindSnapshot::Browser(current) = &snapshot.kind else {
            continue;
        };
        let Some(command) = set_browser_tabs_command(pane, current, tabs, active) else {
            continue;
        };
        client
            .execute(command)
            .map(drop)
            .map_err(|error| error.to_string())?;
    }
    Ok(changed)
}

fn set_browser_tabs_command(
    pane: PaneId,
    current: &BrowserDescriptor,
    tabs: Vec<String>,
    active: usize,
) -> Option<CommandInvocation> {
    if tabs.is_empty()
        || active >= tabs.len()
        || (current.tabs == tabs && current.active_tab == active)
    {
        return None;
    }
    let mut args = vec![
        "-t".to_owned(),
        pane.to_string(),
        "-a".to_owned(),
        active.to_string(),
        "--".to_owned(),
    ];
    args.extend(tabs);
    Some(CommandInvocation::new("set-browser-tabs", args))
}

/// Hidpi terminals report cell heights near twice the logical ~16px.
const fn hidpi_scale(cell_height_px: u32) -> f32 {
    if cell_height_px >= 28 { 2.0 } else { 1.0 }
}

fn visible_browser_surfaces(model: &Model, max_surface_bytes: u64) -> Vec<BrowserSurface> {
    model
        .layout
        .panes
        .iter()
        .filter_map(|entry| {
            let snapshot = model.pane_snapshot(entry.pane)?;
            let PaneKindSnapshot::Browser(descriptor) = &snapshot.kind else {
                return None;
            };
            let content = entry.content();
            Some(BrowserSurface {
                pane: entry.pane,
                descriptor: descriptor.clone(),
                cells: (content.width, content.height),
                px: crate::browser::clamp_surface_px(
                    (
                        u32::from(content.width).saturating_mul(model.size.cell_width_px),
                        u32::from(content.height).saturating_mul(model.size.cell_height_px),
                    ),
                    max_surface_bytes,
                ),
                base_scale: hidpi_scale(model.size.cell_height_px),
            })
        })
        .collect()
}

fn sync_browser_surfaces(
    model: &Model,
    browser: &mut BrowserState,
    renderer: &mut Renderer,
    now: Instant,
) {
    let surfaces = visible_browser_surfaces(model, browser.surface_byte_budget());
    let SurfaceChanges { closed, resized } = browser.reconcile_surfaces(surfaces, now);
    for pane in closed {
        renderer.remove_browser_frame(pane);
    }
    for (pane, cells) in resized {
        renderer.resize_browser_frame(pane, cells);
    }
}

fn send_resizes_and_sync_browser(
    model: &mut Model,
    client: &InteractiveClient,
    browser: &mut BrowserState,
    renderer: &mut Renderer,
) -> Result<(), String> {
    send_resizes(model, client)?;
    sync_browser_surfaces(model, browser, renderer, Instant::now());
    Ok(())
}

fn send_resizes(model: &mut Model, client: &InteractiveClient) -> Result<(), String> {
    send_resizes_with(model, |input| {
        client.send_input(input).map_err(|error| error.to_string())
    })
}

fn refresh_snapshot(
    model: &mut Model,
    snapshot: Arc<zz_protocol::MuxSnapshot>,
    layout_generation: u64,
    send: impl FnMut(InputMessage) -> Result<(), String>,
) -> Result<ProtocolOutcome, String> {
    let before = model.paint_structure();
    if update_snapshot(model, snapshot, layout_generation) {
        send_resizes_with(model, send)?;
    }
    Ok(if model.paint_structure() == before {
        ProtocolOutcome::Repaint
    } else {
        ProtocolOutcome::RepaintAll
    })
}

fn update_snapshot(
    model: &mut Model,
    snapshot: Arc<zz_protocol::MuxSnapshot>,
    layout_generation: u64,
) -> bool {
    let generation_changed = model.layout_generation != layout_generation;
    if generation_changed {
        model.last_sent_geometry.clear();
    }
    model.layout_generation = layout_generation;
    model.update_snapshot(snapshot);
    generation_changed
}

fn send_resizes_with(
    model: &mut Model,
    mut send: impl FnMut(InputMessage) -> Result<(), String>,
) -> Result<(), String> {
    let geometries = model.terminal_geometries();
    let visible = geometries
        .iter()
        .map(|(pane, _)| *pane)
        .collect::<HashSet<_>>();
    model
        .last_sent_geometry
        .retain(|pane, _| visible.contains(pane));
    for (pane, geometry @ (columns, rows, cell_width_px, cell_height_px)) in geometries {
        if model.last_sent_geometry.get(&pane) == Some(&geometry) {
            continue;
        }
        send(InputMessage::ResizeTerminalV2 {
            pane,
            columns,
            rows,
            cell_width_px,
            cell_height_px,
            layout_generation: model.layout_generation,
        })?;
        model.last_sent_geometry.insert(pane, geometry);
    }
    if let Some((geometry, message)) = command_output_resize_message(model) {
        send(message)?;
        model.last_sent_command_output_geometry = Some(geometry);
    } else if model.command_output_geometry().is_none() {
        model.last_sent_command_output_geometry = None;
    }
    Ok(())
}

fn command_output_resize_message(model: &Model) -> Option<((u16, u16, u32, u32), InputMessage)> {
    let geometry @ (columns, rows, cell_width_px, cell_height_px) =
        model.command_output_geometry()?;
    (model.last_sent_command_output_geometry != Some(geometry)).then_some((
        geometry,
        InputMessage::ResizeCommandOutput {
            columns,
            rows,
            cell_width_px,
            cell_height_px,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_protocol::{MenuItem, MenuState, PopupBorderLines, PopupState};
    use zz_terminal::SearchDirection;

    fn paned_model() -> (Model, zz_protocol::PaneId) {
        let core = ClientCore::new();
        let endpoint = Endpoint::parse("unix:///tmp/zz-app-mouse-test.sock").expect("endpoint");
        let mut model = Model::new(
            &core,
            crate::tty::TerminalSize {
                columns: 79,
                rows: 24,
                cell_width_px: 8,
                cell_height_px: 16,
            },
            "host".to_owned(),
            "host".to_owned(),
            endpoint.clone(),
            endpoint,
            Vec::new(),
        );
        let session = zz_protocol::SessionId(1);
        let window = zz_protocol::WindowId(1);
        let pane = zz_protocol::PaneId(7);
        model.attached_session = Some(session);
        model.update_snapshot(Arc::new(zz_protocol::MuxSnapshot {
            generation: 1,
            sessions: vec![zz_protocol::SessionSnapshot {
                id: session,
                name: "s".to_owned(),
                active_window: window,
                windows: vec![zz_protocol::WindowSnapshot {
                    id: window,
                    index: 0,
                    name: "w".to_owned(),
                    automatic_rename: true,
                    active_pane: pane,
                    zoomed_pane: None,
                    layout: zz_protocol::LayoutNode::Pane(pane),
                    panes: std::collections::BTreeMap::new(),
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
            focused_window: Some(window),
        }));
        (model, pane)
    }

    #[test]
    fn only_a_snapshot_that_moves_the_frame_asks_for_a_full_repaint() {
        let (mut model, pane) = paned_model();
        let before = model.paint_structure();
        let mut renamed = (*model.snapshot).clone();
        renamed.generation = 2;
        renamed.sessions[0].windows[0].name = "renamed".to_owned();
        renamed.sessions[0].windows[0].status_label = "0:renamed*".to_owned();
        model.update_snapshot(Arc::new(renamed.clone()));
        assert_eq!(model.paint_structure(), before);

        let mut split = renamed;
        split.generation = 3;
        split.sessions[0].windows[0].layout = zz_protocol::LayoutNode::Split {
            id: zz_protocol::SplitId(1),
            axis: zz_protocol::Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(zz_protocol::LayoutNode::Pane(pane)),
            second: Box::new(zz_protocol::LayoutNode::Pane(zz_protocol::PaneId(8))),
        };
        model.update_snapshot(Arc::new(split));
        assert_ne!(model.paint_structure(), before);
    }

    #[test]
    fn a_drained_run_of_events_owes_the_strongest_paint_once() {
        let paints = [
            PendingPaint::Frames,
            PendingPaint::None,
            PendingPaint::RepaintAll,
            PendingPaint::Repaint,
        ];
        assert_eq!(
            paints
                .into_iter()
                .fold(PendingPaint::None, PendingPaint::max),
            PendingPaint::RepaintAll
        );
        assert!(PendingPaint::Frames < PendingPaint::Repaint);
    }

    fn with_row_text(viewport: &TerminalViewport, row: u16, text: &str) -> TerminalViewport {
        let mut viewport = viewport.clone();
        let mut cells = viewport.cells.to_vec();
        let start = usize::from(row) * usize::from(viewport.columns);
        for (offset, glyph) in text.chars().enumerate() {
            cells[start + offset] =
                zz_terminal::PackedCell::new(u32::from(glyph), 0, zz_terminal::CellWidth::Narrow);
        }
        viewport.cells = Arc::from(cells);
        viewport
    }

    fn initial_snapshot(panes: &[PaneId]) -> zz_protocol::MuxSnapshot {
        let (model, _) = paned_model();
        let mut snapshot = (*model.snapshot).clone();
        let window = &mut snapshot.sessions[0].windows[0];
        for pane in panes {
            window.panes.insert(
                *pane,
                zz_protocol::PaneSnapshot {
                    id: *pane,
                    title: "shell".to_owned(),
                    kind: PaneKindSnapshot::Terminal,
                    synchronized_input: false,
                    bell: false,
                    dead: false,
                    dead_status: None,
                    border_colour: None,
                    active_border_colour: None,
                    border_status_text: String::new(),
                    mode: None,
                },
            );
        }
        let split = |id, axis, first, second| zz_protocol::LayoutNode::Split {
            id: zz_protocol::SplitId(id),
            axis,
            ratio: 0.5,
            first: Box::new(first),
            second: Box::new(second),
        };
        window.layout = if panes.len() == 1 {
            zz_protocol::LayoutNode::Pane(panes[0])
        } else {
            split(
                1,
                zz_protocol::Axis::Horizontal,
                split(
                    2,
                    zz_protocol::Axis::Vertical,
                    zz_protocol::LayoutNode::Pane(panes[0]),
                    zz_protocol::LayoutNode::Pane(panes[1]),
                ),
                split(
                    3,
                    zz_protocol::Axis::Vertical,
                    zz_protocol::LayoutNode::Pane(panes[2]),
                    zz_protocol::LayoutNode::Pane(panes[3]),
                ),
            )
        };
        snapshot
    }

    fn initial_event(payload: zz_protocol::EventPayload) -> ProtocolMessage {
        ProtocolMessage::Event(zz_protocol::Event {
            sequence: 0,
            payload,
        })
    }

    fn initial_view(epoch: u64) -> ProtocolMessage {
        initial_event(zz_protocol::EventPayload::ClientView(
            zz_protocol::ClientView {
                session: Some(zz_protocol::SessionId(1)),
                focused_window: Some(zz_protocol::WindowId(1)),
                layout_generation: 42,
                attachment_generation: epoch,
                ..zz_protocol::ClientView::default()
            },
        ))
    }

    fn initial_model(core: &ClientCore) -> Model {
        let endpoint = Endpoint::parse("unix:///tmp/zz-app-first-paint.sock").unwrap();
        Model::new(
            core,
            TerminalSize {
                columns: 79,
                rows: 24,
                cell_width_px: 8,
                cell_height_px: 16,
            },
            "host".to_owned(),
            "host".to_owned(),
            endpoint.clone(),
            endpoint,
            Vec::new(),
        )
    }

    #[test]
    fn a_new_layout_generation_resends_an_unchanged_terminal_geometry() {
        let pane = PaneId(7);
        let mut core = ClientCore::new();
        core.handle_message(initial_event(zz_protocol::EventPayload::Snapshot(
            initial_snapshot(&[pane]),
        )));
        core.handle_message(initial_view(1));
        let mut model = initial_model(&core);
        model.update_snapshot(Arc::clone(core.snapshot()));
        let geometry = model.terminal_geometries();
        assert_eq!(geometry.len(), 1);
        let structure = model.paint_structure();
        let mut sent = Vec::new();
        send_resizes_with(&mut model, |input| {
            sent.push(input);
            Ok(())
        })
        .unwrap();
        assert_eq!(sent.len(), 1);
        while core.poll_event().is_some() {}
        core.handle_message(initial_event(zz_protocol::EventPayload::ClientView(
            zz_protocol::ClientView {
                session: Some(zz_protocol::SessionId(1)),
                focused_window: Some(zz_protocol::WindowId(1)),
                layout_generation: 43,
                attachment_generation: 1,
                ..zz_protocol::ClientView::default()
            },
        )));
        assert!(
            std::iter::from_fn(|| core.poll_event())
                .any(|event| matches!(event, CoreEvent::SnapshotChanged))
        );
        let outcome = refresh_snapshot(
            &mut model,
            Arc::clone(core.snapshot()),
            core.layout_generation(),
            |input| {
                sent.push(input);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(model.terminal_geometries(), geometry);
        assert_eq!(model.paint_structure(), structure);
        assert!(matches!(outcome, ProtocolOutcome::Repaint));
        assert_eq!(sent.len(), 2);
        let (_, (columns, rows, cell_width_px, cell_height_px)) = geometry[0];
        for (input, layout_generation) in sent.iter().zip([42, 43]) {
            assert_eq!(
                input,
                &InputMessage::ResizeTerminalV2 {
                    pane,
                    columns,
                    rows,
                    cell_width_px,
                    cell_height_px,
                    layout_generation,
                }
            );
        }
        refresh_snapshot(
            &mut model,
            Arc::clone(core.snapshot()),
            core.layout_generation(),
            |input| {
                sent.push(input);
                Ok(())
            },
        )
        .unwrap();
        send_resizes_with(&mut model, |input| {
            sent.push(input);
            Ok(())
        })
        .unwrap();
        assert_eq!(sent.len(), 2);
    }

    #[test]
    fn an_attached_refresh_resends_the_same_geometry_with_its_new_generation() {
        let pane = PaneId(7);
        let mut core = ClientCore::new();
        core.handle_message(initial_event(zz_protocol::EventPayload::Snapshot(
            initial_snapshot(&[pane]),
        )));
        core.handle_message(initial_view(1));
        let mut model = initial_model(&core);
        model.update_snapshot(Arc::clone(core.snapshot()));
        let geometry = model.terminal_geometries();
        let structure = model.paint_structure();
        let mut sent = Vec::new();
        send_resizes_with(&mut model, |input| {
            sent.push(input);
            Ok(())
        })
        .unwrap();
        assert_eq!(sent.len(), 1);
        while core.poll_event().is_some() {}
        core.handle_message(initial_event(zz_protocol::EventPayload::ClientView(
            zz_protocol::ClientView {
                session: Some(zz_protocol::SessionId(1)),
                focused_window: Some(zz_protocol::WindowId(1)),
                layout_generation: 43,
                attachment_generation: 2,
                ..zz_protocol::ClientView::default()
            },
        )));
        assert!(matches!(
            core.poll_event(),
            Some(CoreEvent::Attached { session }) if session == zz_protocol::SessionId(1)
        ));
        assert!(update_snapshot(
            &mut model,
            Arc::clone(core.snapshot()),
            core.layout_generation(),
        ));
        send_resizes_with(&mut model, |input| {
            sent.push(input);
            Ok(())
        })
        .unwrap();
        assert_eq!(model.terminal_geometries(), geometry);
        assert_eq!(model.paint_structure(), structure);
        assert_eq!(sent.len(), 2);
        let (_, (columns, rows, cell_width_px, cell_height_px)) = geometry[0];
        for (input, layout_generation) in sent.iter().zip([42, 43]) {
            assert_eq!(
                input,
                &InputMessage::ResizeTerminalV2 {
                    pane,
                    columns,
                    rows,
                    cell_width_px,
                    cell_height_px,
                    layout_generation,
                }
            );
        }
        refresh_snapshot(
            &mut model,
            Arc::clone(core.snapshot()),
            core.layout_generation(),
            |input| {
                sent.push(input);
                Ok(())
            },
        )
        .unwrap();
        send_resizes_with(&mut model, |input| {
            sent.push(input);
            Ok(())
        })
        .unwrap();
        assert_eq!(sent.len(), 2);
    }

    #[test]
    fn initial_batch_has_every_marker_in_the_first_paint() {
        for count in [1, 4] {
            let panes = (7..7 + count).map(PaneId).collect::<Vec<_>>();
            let snapshot = initial_snapshot(&panes);
            let mut layout_model = initial_model(&ClientCore::new());
            layout_model.attached_session = Some(zz_protocol::SessionId(1));
            layout_model.update_snapshot(Arc::new(snapshot.clone()));
            let mut messages = vec![
                initial_event(zz_protocol::EventPayload::Snapshot(snapshot)),
                initial_view(1),
            ];
            for entry in &layout_model.layout.panes {
                let content = entry.content();
                let viewport = with_row_text(
                    &TerminalViewport::blank(
                        content.width,
                        content.height,
                        zz_terminal::SessionStatus::Running,
                    ),
                    0,
                    &format!("INITIAL-{}", entry.pane.0),
                );
                messages.push(initial_event(zz_protocol::EventPayload::TerminalViewport {
                    pane: entry.pane,
                    viewport,
                }));
            }
            let core = Mutex::new(ClientCore::new());
            let frames = FrameInbox::default();
            let images = KittyImageInbox::default();
            let (events, incoming) = mpsc::channel();
            assert!(forward_protocol_message(
                &core,
                ProtocolMessage::Batch(zz_protocol::Batch::from_messages(1, messages).unwrap()),
                1,
                &events,
                &frames,
                &images,
                &AtomicU8::new(KITTY_GATE_PROBING),
                |_| panic!("initial full state must not need another wire request"),
            ));
            let mut model = initial_model(&lock_core(&core));
            model.update_snapshot(Arc::clone(lock_core(&core).snapshot()));
            assert_eq!(model.attached_session, Some(zz_protocol::SessionId(1)));
            assert_eq!(model.layout_generation, 42);
            let ready = incoming.try_iter().collect::<Vec<_>>();
            assert!(ready.len() < MAX_COALESCED_EVENTS);
            assert_eq!(
                ready
                    .iter()
                    .filter(|event| matches!(event, MainEvent::Frames(1)))
                    .count(),
                1
            );
            let (written, output) = mpsc::channel();
            let mut renderer = Renderer::with_sink(Box::new(move |bytes| {
                written.send(bytes.to_vec()).unwrap();
                Ok(())
            }));
            take_frames(&frames, &mut model, &mut renderer);
            renderer.paint(&model, true).unwrap();
            let first =
                String::from_utf8(output.recv_timeout(Duration::from_secs(2)).unwrap()).unwrap();
            for pane in panes {
                assert!(first.contains(&format!("INITIAL-{}", pane.0)), "{first:?}");
            }
        }
    }

    #[test]
    fn held_startup_output_precedes_the_base_frame_in_the_first_physical_paint() {
        let pane = PaneId(7);
        let snapshot = initial_snapshot(&[pane]);
        let mut layout_model = initial_model(&ClientCore::new());
        layout_model.attached_session = Some(zz_protocol::SessionId(1));
        layout_model.update_snapshot(Arc::new(snapshot.clone()));
        let content = layout_model.pane_rect(pane).unwrap().content();
        let blank = TerminalViewport::blank(
            content.width,
            content.height,
            zz_terminal::SessionStatus::Running,
        );
        let direct = "/tmp/root.conf:1: INTERACTIVE_DIRECT";
        let nested = "/tmp/child.conf:1: INTERACTIVE_NESTED";
        let mut actor = with_row_text(&with_row_text(&blank, 0, direct), 1, nested);
        actor.mode = zz_terminal::TerminalMode::View {
            position: 1,
            total: 2,
        };
        let messages = vec![
            initial_event(zz_protocol::EventPayload::Snapshot(snapshot)),
            initial_view(1),
            initial_event(zz_protocol::EventPayload::CommandOutput {
                pane,
                output_id: 9,
                viewport: Some(actor),
            }),
            initial_event(zz_protocol::EventPayload::TerminalViewport {
                pane,
                viewport: with_row_text(&blank, 0, "BASE-SHELL"),
            }),
        ];
        let core = Mutex::new(ClientCore::new());
        let frames = FrameInbox::default();
        let images = KittyImageInbox::default();
        let (events, incoming) = mpsc::channel();
        assert!(forward_protocol_message(
            &core,
            ProtocolMessage::Batch(zz_protocol::Batch::from_messages(1, messages).unwrap()),
            1,
            &events,
            &frames,
            &images,
            &AtomicU8::new(KITTY_GATE_PROBING),
            |_| panic!("held startup state must not need another wire request"),
        ));
        events.send(MainEvent::Resize).unwrap();
        let mut model = initial_model(&lock_core(&core));
        model.update_snapshot(Arc::clone(lock_core(&core).snapshot()));
        assert!(model.command_output.is_none());
        let (written, output) = mpsc::channel();
        let mut renderer = Renderer::with_sink(Box::new(move |bytes| {
            written.send(bytes.to_vec()).unwrap();
            Ok(())
        }));
        let mut deferred = None;
        let mut next = incoming.try_recv().ok();
        let mut paint = PendingPaint::None;
        let mut actions = Vec::new();
        while let Some(event) = next.take() {
            match event {
                MainEvent::Core { event, .. } => match *event {
                    CoreEvent::Attached { .. } => {
                        model.set_command_output(None, None);
                        model.viewports =
                            [(pane, lock_core(&core).viewport(pane).unwrap().clone())]
                                .into_iter()
                                .collect();
                        actions.push("attached");
                        paint = paint.max(PendingPaint::RepaintAll);
                    }
                    CoreEvent::CommandOutputChanged => {
                        let core = lock_core(&core);
                        model.set_command_output(
                            core.command_output_id(),
                            core.command_output()
                                .map(|(pane, frame)| (pane, frame.clone())),
                        );
                        actions.push("output");
                        paint = paint.max(PendingPaint::RepaintAll);
                    }
                    _ => paint = paint.max(PendingPaint::Repaint),
                },
                MainEvent::KittyImages(1) => {
                    apply_kitty_updates(&mut renderer, images.take(), true);
                    actions.push("reset");
                }
                MainEvent::Frames(1) => {
                    take_frames(&frames, &mut model, &mut renderer);
                    actions.push("frames");
                    paint = paint.max(PendingPaint::Frames);
                }
                _ => panic!("unexpected event in startup paint span"),
            }
            next = next_paint_event(&incoming, &mut deferred);
        }
        assert_eq!(actions, ["attached", "reset", "output", "frames"]);
        assert_eq!(paint, PendingPaint::RepaintAll);
        assert!(matches!(deferred, Some(MainEvent::Resize)));
        assert!(incoming.try_recv().is_err());
        assert_eq!(model.command_output_focus(), Some(pane));
        renderer.invalidate();
        renderer.paint(&model, true).unwrap();
        let first =
            String::from_utf8(output.recv_timeout(Duration::from_secs(2)).unwrap()).unwrap();
        assert_eq!(first.matches("\x1b[2J").count(), 1, "{first:?}");
        assert_eq!(first.matches(direct).count(), 1, "{first:?}");
        assert_eq!(first.matches(nested).count(), 1, "{first:?}");
        assert!(first.find(direct).unwrap() < first.find(nested).unwrap());
        assert!(!first.contains("BASE-SHELL"), "{first:?}");
        assert!(output.try_recv().is_err());
    }

    #[test]
    fn initial_drain_paints_metadata_reset_and_frames_once() {
        for count in [1, 4] {
            let panes = (7..7 + count).map(PaneId).collect::<Vec<_>>();
            let snapshot = initial_snapshot(&panes);
            let mut layout_model = initial_model(&ClientCore::new());
            layout_model.attached_session = Some(zz_protocol::SessionId(1));
            layout_model.update_snapshot(Arc::new(snapshot.clone()));
            let mut messages = vec![
                initial_event(zz_protocol::EventPayload::Snapshot(snapshot)),
                initial_view(1),
                initial_event(zz_protocol::EventPayload::AppearanceChanged {
                    appearance: Box::default(),
                    provenance: zz_terminal::AppearanceProvenance::default(),
                }),
                initial_event(zz_protocol::EventPayload::StatusChanged {
                    status: zz_protocol::StatusLine::default(),
                }),
            ];
            for entry in &layout_model.layout.panes {
                let content = entry.content();
                messages.push(initial_event(zz_protocol::EventPayload::TerminalViewport {
                    pane: entry.pane,
                    viewport: with_row_text(
                        &TerminalViewport::blank(
                            content.width,
                            content.height,
                            zz_terminal::SessionStatus::Running,
                        ),
                        0,
                        &format!("DRAIN-{}", entry.pane.0),
                    ),
                }));
            }
            let core = Mutex::new(ClientCore::new());
            let frames = FrameInbox::default();
            let images = KittyImageInbox::default();
            let (events, incoming) = mpsc::channel();
            assert!(forward_protocol_message(
                &core,
                ProtocolMessage::Batch(zz_protocol::Batch::from_messages(1, messages).unwrap()),
                1,
                &events,
                &frames,
                &images,
                &AtomicU8::new(KITTY_GATE_PROBING),
                |_| panic!("initial full state must not need another wire request"),
            ));
            events.send(MainEvent::Resize).unwrap();
            let mut model = initial_model(&lock_core(&core));
            model.update_snapshot(Arc::clone(lock_core(&core).snapshot()));
            let (written, output) = mpsc::channel();
            let mut renderer = Renderer::with_sink(Box::new(move |bytes| {
                written.send(bytes.to_vec()).unwrap();
                Ok(())
            }));
            let mut deferred = None;
            let mut actions = Vec::new();
            let mut paints = Vec::new();
            while let Some(event) = deferred.take().or_else(|| incoming.try_recv().ok()) {
                if matches!(event, MainEvent::KittyImages(1)) {
                    apply_kitty_updates(&mut renderer, images.take(), true);
                    actions.push("reset");
                    continue;
                }
                let mut paint = PendingPaint::None;
                let mut next = Some(event);
                while let Some(event) = next.take() {
                    match event {
                        MainEvent::Core { event, .. } => match *event {
                            CoreEvent::Attached { .. } => {
                                model.viewports = panes
                                    .iter()
                                    .map(|pane| {
                                        (*pane, lock_core(&core).viewport(*pane).unwrap().clone())
                                    })
                                    .collect();
                                actions.push("attached");
                                paint = paint.max(PendingPaint::RepaintAll);
                            }
                            CoreEvent::AppearanceChanged => {
                                paint = paint.max(PendingPaint::RepaintAll);
                            }
                            _ => paint = paint.max(PendingPaint::Repaint),
                        },
                        MainEvent::KittyImages(1) => {
                            apply_kitty_updates(&mut renderer, images.take(), true);
                            actions.push("reset");
                        }
                        MainEvent::Frames(1) => {
                            take_frames(&frames, &mut model, &mut renderer);
                            actions.push("frames");
                            paint = paint.max(PendingPaint::Frames);
                        }
                        _ => panic!("unexpected event in paint span"),
                    }
                    next = next_paint_event(&incoming, &mut deferred);
                }
                match paint {
                    PendingPaint::RepaintAll => {
                        renderer.invalidate();
                        renderer.paint(&model, true).unwrap();
                    }
                    PendingPaint::Repaint => renderer.paint(&model, false).unwrap(),
                    PendingPaint::Frames => renderer.paint_frames(&model).unwrap(),
                    PendingPaint::None => {}
                }
                if paint != PendingPaint::None {
                    paints.push(output.recv_timeout(Duration::from_secs(2)).unwrap());
                }
                if matches!(deferred, Some(MainEvent::Resize)) {
                    break;
                }
            }
            assert_eq!(paints.len(), 1, "panes={count}, actions={actions:?}");
            let first = String::from_utf8(paints.concat()).unwrap();
            assert_eq!(first.matches("\x1b[2J").count(), 1, "{first:?}");
            for pane in panes {
                assert_eq!(first.matches(&format!("DRAIN-{}", pane.0)).count(), 1);
            }
            assert_eq!(actions, ["attached", "reset", "frames"]);
            assert!(matches!(deferred, Some(MainEvent::Resize)));
            assert!(incoming.try_recv().is_err());
        }
    }

    fn placing_viewport(generation: u64) -> TerminalViewport {
        let mut viewport = TerminalViewport::blank(79, 23, zz_terminal::SessionStatus::Running);
        viewport.kitty_placements = Arc::from([zz_terminal::KittyPlacement {
            image_id: 1299,
            image_generation: generation,
            layer: zz_terminal::KittyLayer::AboveText,
            viewport_col: 0,
            viewport_row: 0,
            absolute_row: 0,
            cell_offset_x: 0,
            cell_offset_y: 0,
            grid_cols: 1,
            grid_rows: 1,
            pixel_width: 1,
            pixel_height: 1,
            source_rect: None,
        }]);
        viewport
    }

    #[test]
    fn initial_and_later_batches_reset_before_images_before_placing_frames() {
        let pane = PaneId(7);
        let core = Mutex::new(ClientCore::new());
        let frames = FrameInbox::default();
        let images = KittyImageInbox::default();
        let gate = AtomicU8::new(KITTY_GATE_PROBING);
        let (events, incoming) = mpsc::channel();
        for epoch in [1, 2] {
            let mut messages = vec![
                initial_event(zz_protocol::EventPayload::Snapshot(initial_snapshot(&[
                    pane,
                ]))),
                initial_view(epoch),
            ];
            for image_id in 1000..1300 {
                messages.push(initial_event(zz_protocol::EventPayload::KittyImageBegin {
                    pane,
                    image_id,
                    generation: epoch,
                    width: 1,
                    height: 1,
                    total_bytes: 4,
                }));
                messages.push(initial_event(zz_protocol::EventPayload::KittyImageChunk {
                    pane,
                    image_id,
                    generation: epoch,
                    bytes: vec![1, 2, 3, 255],
                }));
            }
            let viewport = placing_viewport(epoch);
            messages.push(initial_event(zz_protocol::EventPayload::TerminalViewport {
                pane,
                viewport,
            }));
            messages.push(ProtocolMessage::CommandResponse(CommandResponse::Error {
                request_id: 99,
                error: ServerError::InvalidCommand("tail-error".to_owned()),
                output: zz_protocol::RawText::default(),
            }));
            assert!(forward_protocol_message(
                &core,
                ProtocolMessage::Batch(zz_protocol::Batch::from_messages(epoch, messages).unwrap()),
                1,
                &events,
                &frames,
                &images,
                &gate,
                |_| panic!("initial full state must not need another wire request"),
            ));
            let ready = incoming.try_iter().collect::<Vec<_>>();
            let attached = ready.iter().position(|event| matches!(event, MainEvent::Core { event, .. } if matches!(**event, CoreEvent::Attached { .. }))).unwrap();
            let delivered = ready
                .iter()
                .position(|event| matches!(event, MainEvent::KittyImages(1)))
                .unwrap();
            let placing = ready
                .iter()
                .position(|event| matches!(event, MainEvent::Frames(1)))
                .unwrap();
            let tail = ready.iter().position(|event| matches!(event, MainEvent::Core { event, .. } if matches!(**event, CoreEvent::CommandResponse(CommandResponse::Error { request_id: 99, .. })))).unwrap();
            assert!(attached < delivered && delivered < placing && placing < tail);
            assert!(ready.len() < MAX_COALESCED_EVENTS);
            let state = images.0.lock().unwrap();
            assert_eq!(state.pending.len(), 301);
            assert!(state.pending.iter().all(
                |image| matches!(image, KittyImageUpdate::Reset) || matches!(image, KittyImageUpdate::Ready(image) if image.generation == epoch)
            ));
            drop(state);
            let state = frames.0.lock().unwrap();
            assert_eq!(state.pending.len(), 1);
            assert_eq!(
                state.pending[&pane].viewport.kitty_placements[0].image_generation,
                epoch
            );
            drop(state);
            images.take();
            frames.take();
        }
    }

    fn delayed_main_reattach_keeps_the_latest_image(legacy: bool) {
        let pane = PaneId(7);
        let core = Mutex::new(ClientCore::new());
        let frames = FrameInbox::default();
        let images = KittyImageInbox::default();
        let gate = AtomicU8::new(KITTY_GATE_ENABLED);
        let (events, incoming) = mpsc::channel();
        for epoch in [1, 2] {
            let snapshot = initial_snapshot(&[pane]);
            let mut messages = if legacy {
                vec![ProtocolMessage::Attached {
                    session: zz_protocol::SessionId(1),
                    snapshot,
                    read_only: false,
                    client_flags: String::new(),
                }]
            } else {
                vec![
                    initial_event(zz_protocol::EventPayload::Snapshot(snapshot)),
                    initial_view(epoch),
                ]
            };
            messages.extend([
                initial_event(zz_protocol::EventPayload::KittyImageBegin {
                    pane,
                    image_id: 1299,
                    generation: epoch,
                    width: 1,
                    height: 1,
                    total_bytes: 4,
                }),
                initial_event(zz_protocol::EventPayload::KittyImageChunk {
                    pane,
                    image_id: 1299,
                    generation: epoch,
                    bytes: vec![1, 2, 3, 255],
                }),
                initial_event(zz_protocol::EventPayload::TerminalViewport {
                    pane,
                    viewport: placing_viewport(epoch),
                }),
            ]);
            if !legacy {
                messages = vec![ProtocolMessage::Batch(
                    zz_protocol::Batch::from_messages(epoch, messages).unwrap(),
                )];
            }
            for message in messages {
                assert!(forward_protocol_message(
                    &core,
                    message,
                    1,
                    &events,
                    &frames,
                    &images,
                    &gate,
                    |_| panic!("retained full state must not need another wire request"),
                ));
            }
        }
        let mut model = initial_model(&lock_core(&core));
        model.update_snapshot(Arc::clone(lock_core(&core).snapshot()));
        let (written, output) = mpsc::channel();
        let mut renderer = Renderer::with_sink(Box::new(move |bytes| {
            written.send(bytes.to_vec()).unwrap();
            Ok(())
        }));
        renderer.enable_kitty_graphics();
        let mut final_paint = String::new();
        for event in incoming.try_iter() {
            match event {
                MainEvent::Core { event, .. } if matches!(*event, CoreEvent::Attached { .. }) => {
                    model.update_snapshot(Arc::clone(lock_core(&core).snapshot()));
                    renderer.invalidate();
                }
                MainEvent::KittyImages(1) => {
                    apply_kitty_updates(&mut renderer, images.take(), true);
                }
                MainEvent::Frames(1) => {
                    take_frames(&frames, &mut model, &mut renderer);
                    renderer.paint(&model, true).unwrap();
                    final_paint =
                        String::from_utf8(output.recv_timeout(Duration::from_secs(2)).unwrap())
                            .unwrap();
                }
                _ => {}
            }
        }
        assert!(
            final_paint.contains("\x1b_Ga=p"),
            "legacy={legacy} latest placement lost: {final_paint:?}"
        );
    }

    #[test]
    fn delayed_main_compact_reattach_keeps_the_latest_image() {
        delayed_main_reattach_keeps_the_latest_image(false);
    }

    #[test]
    fn delayed_main_legacy_reattach_keeps_the_latest_image() {
        delayed_main_reattach_keeps_the_latest_image(true);
    }

    #[test]
    fn image_free_attachments_deliver_reset_even_when_graphics_are_rejected() {
        let pane = PaneId(7);
        for legacy in [false, true] {
            let snapshot = initial_snapshot(&[pane]);
            let (mut model, _) = paned_model();
            model.update_snapshot(Arc::new(snapshot.clone()));
            model.viewports.insert(pane, placing_viewport(1));
            let (written, output) = mpsc::channel();
            let mut renderer = Renderer::with_sink(Box::new(move |bytes| {
                written.send(bytes.to_vec()).unwrap();
                Ok(())
            }));
            renderer.enable_kitty_graphics();
            renderer.install_kitty_image(KittyImageData {
                pane,
                image_id: 1299,
                generation: 1,
                width: 1,
                height: 1,
                bytes: vec![1, 2, 3, 255],
            });
            renderer.paint(&model, true).unwrap();
            let previous =
                String::from_utf8(output.recv_timeout(Duration::from_secs(2)).unwrap()).unwrap();
            assert!(previous.contains("\x1b_Ga=p"));
            let core = Mutex::new(ClientCore::new());
            let frames = FrameInbox::default();
            let images = KittyImageInbox::default();
            let (events, incoming) = mpsc::channel();
            let message = if legacy {
                ProtocolMessage::Attached {
                    session: zz_protocol::SessionId(1),
                    snapshot,
                    read_only: false,
                    client_flags: String::new(),
                }
            } else {
                ProtocolMessage::Batch(
                    zz_protocol::Batch::from_messages(
                        1,
                        vec![
                            initial_event(zz_protocol::EventPayload::Snapshot(snapshot)),
                            initial_view(1),
                        ],
                    )
                    .unwrap(),
                )
            };
            assert!(forward_protocol_message(
                &core,
                message,
                1,
                &events,
                &frames,
                &images,
                &AtomicU8::new(KITTY_GATE_DISABLED),
                |_| panic!("attach must not need another wire request"),
            ));
            let ready = incoming.try_iter().collect::<Vec<_>>();
            let attached = ready.iter().position(|event| matches!(event, MainEvent::Core { event, .. } if matches!(**event, CoreEvent::Attached { .. }))).unwrap();
            let delivered = ready
                .iter()
                .position(|event| matches!(event, MainEvent::KittyImages(1)))
                .unwrap();
            assert!(attached < delivered);
            let updates = images.take();
            assert!(matches!(updates.as_slice(), [KittyImageUpdate::Reset]));
            apply_kitty_updates(&mut renderer, updates, false);
            renderer.invalidate();
            renderer.paint(&model, true).unwrap();
            let reset =
                String::from_utf8(output.recv_timeout(Duration::from_secs(2)).unwrap()).unwrap();
            assert!(
                reset.contains("\x1b_Ga=d,d=I"),
                "legacy={legacy}: {reset:?}"
            );
            assert!(!reset.contains("\x1b_Ga=p"), "legacy={legacy}: {reset:?}");
        }
    }

    #[test]
    fn a_drained_run_of_frames_paints_every_row_any_of_them_changed() {
        let (mut model, pane) = paned_model();
        let mut snapshot = (*model.snapshot).clone();
        snapshot.generation = 2;
        snapshot.sessions[0].windows[0].panes.insert(
            pane,
            zz_protocol::PaneSnapshot {
                id: pane,
                title: "shell".to_owned(),
                kind: zz_protocol::PaneKindSnapshot::Terminal,
                synchronized_input: false,
                bell: false,
                dead: false,
                dead_status: None,
                border_colour: None,
                active_border_colour: None,
                border_status_text: String::new(),
                mode: None,
            },
        );
        model.update_snapshot(Arc::new(snapshot));
        let content = model.layout.panes[0].content();
        let prompt = with_row_text(
            &TerminalViewport::blank(
                content.width,
                content.height,
                zz_terminal::SessionStatus::Running,
            ),
            1,
            "$",
        );
        model.viewports.insert(pane, prompt.clone());
        let (send, receive) = mpsc::channel();
        let mut renderer = Renderer::with_sink(Box::new(move |bytes| {
            send.send(bytes.to_vec()).expect("paint receiver");
            Ok(())
        }));
        let wait = Duration::from_secs(5);
        renderer.paint(&model, true).expect("attach paint");
        receive.recv_timeout(wait).expect("attach paint written");

        let inbox = FrameInbox::default();
        let (events, _incoming) = mpsc::channel();
        let typed = with_row_text(&prompt, 1, "$ printf 'MARK-%s' split");
        inbox.publish(pane, typed.clone(), FrameDamage::Rows(vec![1]), 7, &events);
        let capacity = inbox.0.lock().unwrap().pending.capacity();
        take_frames(&inbox, &mut model, &mut renderer);
        assert_eq!(inbox.0.lock().unwrap().pending.capacity(), capacity);
        let answered = with_row_text(&with_row_text(&typed, 2, "MARK-split"), 3, "$");
        inbox.publish(pane, answered, FrameDamage::Rows(vec![2, 3]), 7, &events);
        assert_eq!(inbox.0.lock().unwrap().pending.capacity(), capacity);
        take_frames(&inbox, &mut model, &mut renderer);
        assert_eq!(inbox.0.lock().unwrap().pending.capacity(), capacity);
        renderer.paint_frames(&model).expect("drained paint");

        let painted = String::from_utf8(receive.recv_timeout(wait).expect("drained paint written"))
            .expect("paint is UTF-8");
        assert!(
            painted.contains(" printf 'MARK-%s' split"),
            "the row only the first frame changed reaches the tty: {painted:?}"
        );
        assert!(
            painted.contains(&format!("\x1b[{};{}H", content.y + 2, content.x + 2)),
            "the unchanged prompt cell is preserved: {painted:?}"
        );
        assert!(painted.contains("MARK-split"), "{painted:?}");
    }

    #[test]
    #[cfg(unix)]
    fn event_loop_delivers_suspend_and_resume() {
        let (events, incoming) = mpsc::channel();
        let signals = event_loop::SignalInbox::new().unwrap();
        rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::TSTP)
            .unwrap();
        signals.wait_for_signal(&events).unwrap();
        assert!(matches!(
            incoming.recv_timeout(Duration::from_secs(2)).unwrap(),
            MainEvent::Suspend
        ));
        rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::CONT)
            .unwrap();
        signals.wait_for_signal(&events).unwrap();
        assert!(matches!(
            incoming.recv_timeout(Duration::from_secs(2)).unwrap(),
            MainEvent::Resume
        ));
    }

    #[test]
    fn command_output_begin_search_starts_the_matching_local_prompt() {
        let (mut model, pane) = paned_model();
        model.set_command_output(
            Some(1),
            Some((
                pane,
                TerminalViewport::blank(79, 22, zz_terminal::SessionStatus::Running),
            )),
        );
        model.command_output_swallowed_key = Some(zz_terminal::KeyCode::Escape);

        let action = command_output_ui_action(
            &mut model,
            pane,
            TerminalUiCommand::BeginSearch {
                direction: SearchDirection::Backward,
            },
        );
        assert!(matches!(
            action,
            Some(TerminalViewAction::SearchBegin(ref query))
                if query.direction == SearchDirection::Backward && query.text.is_empty()
        ));
        assert_eq!(
            model
                .command_output_search
                .as_ref()
                .map(|query| query.direction),
            Some(SearchDirection::Backward)
        );
        assert_eq!(model.command_output_swallowed_key, None);

        assert_eq!(
            command_output_ui_action(
                &mut model,
                PaneId(pane.0 + 1),
                TerminalUiCommand::BeginSearch {
                    direction: SearchDirection::Forward,
                },
            ),
            None
        );
        assert_eq!(
            model
                .command_output_search
                .as_ref()
                .map(|query| query.direction),
            Some(SearchDirection::Backward)
        );
    }

    #[test]
    fn command_output_resize_matches_rendered_content_and_tracks_geometry() {
        let (mut model, pane) = paned_model();
        let viewport = TerminalViewport::blank(79, 22, zz_terminal::SessionStatus::Running);
        assert_eq!(command_output_resize_message(&model), None);

        model.set_command_output(Some(1), Some((pane, viewport.clone())));
        let content = model.command_output_content_rect();
        let (geometry, message) = command_output_resize_message(&model).expect("open resize");
        assert_eq!(geometry.0, content.width);
        assert_eq!(geometry.1, content.height);
        assert!(matches!(
            message,
            InputMessage::ResizeCommandOutput {
                columns,
                rows,
                cell_width_px: 8,
                cell_height_px: 16,
            } if columns == content.width && rows == content.height
        ));

        model.last_sent_command_output_geometry = Some(geometry);
        assert_eq!(command_output_resize_message(&model), None);

        let mut size = model.size;
        size.columns = 91;
        size.rows = 30;
        model.set_size(size);
        let resized_content = model.command_output_content_rect();
        let (resized_geometry, resized_message) =
            command_output_resize_message(&model).expect("terminal resize");
        assert_eq!(resized_geometry.0, resized_content.width);
        assert_eq!(resized_geometry.1, resized_content.height);
        assert!(matches!(
            resized_message,
            InputMessage::ResizeCommandOutput { columns: 91, rows, .. }
                if rows == resized_content.height
        ));

        model.last_sent_command_output_geometry = Some(resized_geometry);
        assert!(model.set_status(zz_protocol::StatusLine {
            rows: vec!["one".to_owned(), "two".to_owned()],
            position: zz_protocol::StatusPosition::Bottom,
            ..zz_protocol::StatusLine::default()
        }));
        let status_content = model.command_output_content_rect();
        let (status_geometry, status_message) =
            command_output_resize_message(&model).expect("status resize");
        assert_eq!(status_geometry.1, status_content.height);
        assert!(matches!(
            status_message,
            InputMessage::ResizeCommandOutput { rows, .. } if rows == status_content.height
        ));

        model.last_sent_command_output_geometry = Some(status_geometry);
        model.set_command_output(None, None);
        assert_eq!(model.last_sent_command_output_geometry, None);
        assert_eq!(command_output_resize_message(&model), None);

        model.set_command_output(Some(2), Some((pane, viewport)));
        assert!(command_output_resize_message(&model).is_some());
    }

    #[test]
    fn same_output_frames_dedupe_but_an_identical_replacement_resizes() {
        let (mut model, pane) = paned_model();
        let content = model.command_output_content_rect();
        let mut frame = TerminalViewport::blank(
            content.width,
            content.height,
            zz_terminal::SessionStatus::Running,
        );
        model.set_command_output(Some(1), Some((pane, frame.clone())));
        let (geometry, _) = command_output_resize_message(&model).expect("open resize");
        model.last_sent_command_output_geometry = Some(geometry);

        frame.generation = frame.generation.saturating_add(1);
        model.set_command_output(Some(1), Some((pane, frame.clone())));
        assert_eq!(command_output_resize_message(&model), None);

        model.set_command_output(Some(2), Some((pane, frame)));
        let (replacement_geometry, replacement) =
            command_output_resize_message(&model).expect("replacement resize");
        assert_eq!(replacement_geometry, geometry);
        assert!(matches!(
            replacement,
            InputMessage::ResizeCommandOutput { columns, rows, .. }
                if columns == geometry.0 && rows == geometry.1
        ));
    }

    #[test]
    fn unrelated_request_zero_error_preserves_pending_attach_recovery() {
        let (mut model, _) = paned_model();
        assert_eq!(model.finish_client_focus_attach(), None);
        model.begin_client_focus_attach();
        assert_eq!(model.client_focus_changed(false), None);
        let mut attempt = AttachAttempt::Explicit;

        let unrelated = ServerError::InvalidCommand("unrelated input error".to_owned());
        assert!(!attach_attempt_owns_missing_response(attempt, &unrelated));
        assert!(attempt.is_pending());
        assert!(model.client_focus_attach_pending());

        let missing = ServerError::SessionNotFound("missing".to_owned());
        assert!(attach_attempt_owns_missing_response(attempt, &missing));
        let recovered = model.fail_client_focus_attach();
        attempt = AttachAttempt::Idle;
        assert_eq!(
            recovered,
            Some(InputMessage::ClientFocus { focused: false })
        );
        assert!(!attempt.is_pending());
        assert!(!attach_attempt_owns_missing_response(attempt, &missing));
    }

    fn tracking_viewport(tracking: bool) -> zz_terminal::TerminalViewport {
        let mut viewport =
            zz_terminal::TerminalViewport::blank(79, 22, zz_terminal::SessionStatus::Running);
        viewport.mouse_tracking = tracking;
        viewport
    }

    fn menu_state(mouse_keys: bool) -> MenuState {
        MenuState {
            left: 4,
            top: 3,
            width: 20,
            height: 3,
            client_columns: 80,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 16,
            title: "Menu".to_owned(),
            style: "default".to_owned(),
            selected_style: "reverse".to_owned(),
            border_style: "default".to_owned(),
            border_lines: PopupBorderLines::Single,
            items: vec![Some(MenuItem {
                name: "First".to_owned(),
                key: Some("f".to_owned()),
                annotation: Some("f".to_owned()),
                enabled: true,
            })],
            selected: None,
            stay_open: false,
            mouse_keys,
        }
    }

    fn popup_state(pane: PaneId) -> PopupState {
        PopupState {
            pane,
            left: 4,
            top: 3,
            width: 20,
            height: 8,
            client_columns: 79,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 16,
            title: "Popup".to_owned(),
            style: "default".to_owned(),
            border_style: "default".to_owned(),
            border_lines: PopupBorderLines::Single,
            close_on_exit: false,
            close_on_exit_zero: false,
            close_on_any_key: false,
            dead: false,
        }
    }

    #[test]
    fn app_requested_mouse_lights_the_outer_modes_while_the_option_is_off() {
        let (mut model, pane) = paned_model();
        model.mouse_option = false;
        model.mouse_arming = MouseArming::Off;

        assert!(sync_mouse_modes(&mut model, false).is_none());

        model.viewports.insert(pane, tracking_viewport(true));
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::mouse_mode_sequence(MouseArming::Any, false).as_slice())
        );
        assert_eq!(model.mouse_arming, MouseArming::Any);
        assert!(sync_mouse_modes(&mut model, false).is_none());

        model.viewports.insert(pane, tracking_viewport(false));
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::mouse_mode_sequence(MouseArming::Off, false).as_slice())
        );
        assert_eq!(model.mouse_arming, MouseArming::Off);

        model.mouse_option = true;
        assert_eq!(
            sync_mouse_modes(&mut model, true).as_deref(),
            Some(crate::tty::mouse_mode_sequence(MouseArming::Button, true).as_slice())
        );
    }

    /// `menu.c` `menu_prepare` raises `MODE_MOUSE_ALL|MODE_MOUSE_BUTTON` on the
    /// menu's own screen only when the menu is not `MENU_NOMOUSE`, which
    /// `cmd-display-menu.c` sets for every menu raised without `-M` and
    /// without an invoking mouse event. So the common menu leaves the arming
    /// to the `mouse` option and only `-M` pins any-event tracking up.
    #[test]
    fn a_nomouse_menu_leaves_the_arming_to_the_mouse_option() {
        let (mut model, pane) = paned_model();
        model.viewports.insert(pane, tracking_viewport(false));

        model.mouse_option = true;
        model.menu = Some(menu_state(false));
        assert_eq!(desired_mouse_arming(&model), MouseArming::Button);
        model.menu = Some(menu_state(true));
        assert_eq!(desired_mouse_arming(&model), MouseArming::Any);

        model.mouse_option = false;
        model.menu = Some(menu_state(false));
        assert_eq!(desired_mouse_arming(&model), MouseArming::Off);
        model.menu = Some(menu_state(true));
        assert_eq!(desired_mouse_arming(&model), MouseArming::Any);

        model.mouse_option = true;
        model.focus_follows_mouse = true;
        model.menu = Some(menu_state(false));
        assert_eq!(desired_mouse_arming(&model), MouseArming::Any);
    }

    #[test]
    fn popup_descriptor_owns_outer_mouse_tracking_even_before_its_frame() {
        let (mut model, pane) = paned_model();
        model.mouse_option = false;
        model.mouse_arming = MouseArming::Any;
        model.viewports.insert(pane, tracking_viewport(true));
        let popup = PaneId(u64::MAX - 1);
        model.popup = Some(popup_state(popup));

        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::mouse_mode_sequence(MouseArming::Off, false).as_slice())
        );
        assert_eq!(model.mouse_arming, MouseArming::Off);

        model.viewports.insert(popup, tracking_viewport(true));
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::mouse_mode_sequence(MouseArming::Any, false).as_slice())
        );
        assert_eq!(model.mouse_arming, MouseArming::Any);

        model.viewports.insert(popup, tracking_viewport(false));
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::mouse_mode_sequence(MouseArming::Off, false).as_slice())
        );
        model.popup = None;
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::mouse_mode_sequence(MouseArming::Any, false).as_slice())
        );
    }

    #[test]
    fn mouse_off_forwards_only_to_a_tracking_pane_and_skips_chrome() {
        let (mut model, pane) = paned_model();
        model.mouse_option = false;
        let event = crate::terminal_event::MouseEvent {
            kind: crate::terminal_event::MouseEventKind::Down(
                crate::terminal_event::MouseButton::Left,
            ),
            column: 5,
            row: 2,
            modifiers: crate::terminal_event::KeyModifiers::NONE,
        };

        assert!(
            crate::input::app_mouse_forward_action(&model, event, 5, 2, 40, 32).is_none(),
            "no viewport yet: nothing forwards"
        );

        model.viewports.insert(pane, tracking_viewport(false));
        assert!(
            crate::input::app_mouse_forward_action(&model, event, 5, 2, 40, 32).is_none(),
            "a pane that did not request mouse receives nothing"
        );

        model.viewports.insert(pane, tracking_viewport(true));
        let (target, action) = crate::input::app_mouse_forward_action(&model, event, 5, 2, 40, 32)
            .expect("tracking pane receives the event");
        assert_eq!(target, pane);
        assert!(matches!(
            action,
            zz_terminal::TerminalViewAction::Mouse(input)
                if !input.force_selection()
        ));

        assert!(
            crate::input::app_mouse_forward_action(&model, event, 5, 0, 40, 0).is_some(),
            "pane-border-status off reserves no row, so the pane's first row is content"
        );

        let mut snapshot = (*model.snapshot).clone();
        snapshot.sessions[0].windows[0].pane_border_status = zz_protocol::PaneBorderStatus::Top;
        model.update_snapshot(Arc::new(snapshot));
        assert!(
            crate::input::app_mouse_forward_action(&model, event, 5, 0, 40, 0).is_none(),
            "pane-border-status top spends the pane's first row on the status row"
        );
        assert!(
            crate::input::app_mouse_forward_action(&model, event, 5, 1, 40, 16).is_some(),
            "the row below it is content again"
        );
    }

    #[test]
    fn escape_timeout_honors_the_pinned_default_and_live_values() {
        let mut options = zz_protocol::MuxOptions::default();
        assert_eq!(escape_timeout_ms(&options), 10);
        options.set(
            zz_protocol::MuxOptionKey::EscapeTime,
            "0",
            zz_protocol::MuxOptionSource::RuntimeCommand,
        );
        assert_eq!(escape_timeout_ms(&options), 1);
        options.set(
            zz_protocol::MuxOptionKey::EscapeTime,
            "50",
            zz_protocol::MuxOptionSource::RuntimeCommand,
        );
        assert_eq!(escape_timeout_ms(&options), 50);
        options.set(
            zz_protocol::MuxOptionKey::EscapeTime,
            "bogus",
            zz_protocol::MuxOptionSource::RuntimeCommand,
        );
        assert_eq!(escape_timeout_ms(&options), 10);
    }

    #[test]
    fn mouse_gate_follows_the_effective_option_value() {
        let mut options = zz_protocol::MuxOptions::default();
        assert!(mouse_option_enabled(&options));
        options.set(
            zz_protocol::MuxOptionKey::Mouse,
            "off",
            zz_protocol::MuxOptionSource::RuntimeCommand,
        );
        assert!(!mouse_option_enabled(&options));
    }

    #[test]
    fn tui_exit_notices_and_codes_match_tmux() {
        for (exit, notice, code) in [
            (
                TuiExit::Detached("work".to_owned()),
                "[detached (from session work)]",
                0,
            ),
            (TuiExit::Exited, "[exited]", 0),
            (TuiExit::ServerExited, "[server exited]", 1),
            (
                TuiExit::ServerExitedUnexpectedly,
                "[server exited unexpectedly]",
                1,
            ),
        ] {
            assert_eq!(exit.notice(), notice);
            assert_eq!(exit.exit_code(), code);
        }
    }

    #[test]
    fn frame_inbox_keeps_only_the_latest_viewport_per_pane() {
        let inbox = FrameInbox::default();
        let (events, incoming) = mpsc::channel();
        let first = TerminalViewport::blank(80, 24, zz_terminal::SessionStatus::Running);
        let second = TerminalViewport::blank(120, 40, zz_terminal::SessionStatus::Running);

        inbox.publish(PaneId(1), first, FrameDamage::Rows(vec![1]), 7, &events);
        inbox.publish(PaneId(1), second, FrameDamage::Rows(vec![2]), 7, &events);

        assert!(matches!(incoming.recv().unwrap(), MainEvent::Frames(7)));
        assert!(incoming.try_recv().is_err());
        let pending = inbox.take();
        assert_eq!(pending[&PaneId(1)].viewport.columns, 120);
        assert_eq!(pending[&PaneId(1)].damage, FrameDamage::Rows(vec![1, 2]));
    }

    #[test]
    fn frame_capacity_return_keeps_new_publication_and_attachment_reset() {
        for reset in [false, true] {
            let inbox = FrameInbox::default();
            let (events, incoming) = mpsc::channel();
            let first = TerminalViewport::blank(80, 24, zz_terminal::SessionStatus::Running);
            for pane in 1..=16 {
                inbox.publish(PaneId(pane), first.clone(), FrameDamage::All, 7, &events);
            }
            assert!(matches!(incoming.recv().unwrap(), MainEvent::Frames(7)));
            let mut completed = inbox.take();
            let capacity = completed.capacity();
            completed.clear();
            if reset {
                inbox.clear();
            }
            inbox.publish(PaneId(1), first, FrameDamage::Rows(vec![1]), 7, &events);
            inbox.publish(
                PaneId(1),
                TerminalViewport::blank(120, 40, zz_terminal::SessionStatus::Running),
                FrameDamage::Rows(vec![2]),
                7,
                &events,
            );
            inbox.recycle(completed);
            {
                let state = inbox.0.lock().unwrap();
                assert_eq!(state.pending.capacity(), capacity);
                assert_eq!(state.pending.len(), 1);
                assert!(state.wake_pending);
            }
            assert!(matches!(incoming.recv().unwrap(), MainEvent::Frames(7)));
            assert!(incoming.try_recv().is_err());
            let mut pending = inbox.take();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[&PaneId(1)].viewport.columns, 120);
            assert_eq!(pending[&PaneId(1)].damage, FrameDamage::Rows(vec![1, 2]));
            pending.clear();
            inbox.recycle(pending);
            let state = inbox.0.lock().unwrap();
            assert!(state.pending.is_empty());
            assert_eq!(state.pending.capacity(), capacity);
            assert!(!state.wake_pending);
        }
    }

    #[test]
    fn frame_inbox_removes_a_retired_synthetic_pane_before_delivery() {
        let inbox = FrameInbox::default();
        let (events, incoming) = mpsc::channel();
        inbox.publish(
            PaneId(1),
            TerminalViewport::blank(20, 8, zz_terminal::SessionStatus::Running),
            FrameDamage::All,
            7,
            &events,
        );
        inbox.publish(
            PaneId(2),
            TerminalViewport::blank(30, 10, zz_terminal::SessionStatus::Running),
            FrameDamage::All,
            7,
            &events,
        );
        inbox.remove(PaneId(1));

        assert!(matches!(incoming.recv().unwrap(), MainEvent::Frames(7)));
        let pending = inbox.take();
        assert!(!pending.contains_key(&PaneId(1)));
        assert!(pending.contains_key(&PaneId(2)));
    }

    #[test]
    fn failed_connect_first_host_switch_keeps_the_current_endpoint() {
        let current = Endpoint::parse("unix:///tmp/zz.sock").unwrap();
        let original = current.clone();
        let target = HostSwitch {
            label: "box".to_owned(),
            endpoint: Endpoint::parse("ssh://box").unwrap(),
        };

        let result: Result<HostSwitchDecision<()>, String> =
            prepare_host_switch(&current, target, |_| Err("offline".to_owned()));

        assert!(matches!(result, Err(error) if error == "offline"));
        assert_eq!(current, original);
    }

    #[test]
    fn kitty_probe_enables_on_a_matching_ok_response() {
        let mut probe = KittyProbe::new(None, true);
        let update = probe.observe(&TerminalEvent::KittyGraphicsResponse {
            image_id: PROBE_IMAGE_ID,
            ok: true,
        });
        assert_eq!(update.graphics, Some(true));
        assert!(update.consumed);
        assert_eq!(probe.state, KittyProbeState::Enabled);
        let fence = probe.observe(&TerminalEvent::DeviceAttributes);
        assert_eq!(fence.graphics, None);
        assert!(fence.finish_file_probe);
    }

    #[test]
    fn an_unprobed_terminal_is_asked_only_once_something_needs_graphics() {
        let mut probe = KittyProbe::new(None, false);
        let reply = probe.observe(&TerminalEvent::DeviceAttributes);
        assert!(reply.consumed);
        assert_eq!(reply.graphics, None);
        assert!(!reply.finish_file_probe);
        assert_eq!(probe.state, KittyProbeState::Idle);

        assert!(probe.start());
        assert!(!probe.start());
        let fence = probe.observe(&TerminalEvent::DeviceAttributes);
        assert_eq!(fence.graphics, Some(false));
        assert!(fence.finish_file_probe);
        assert_eq!(probe.state, KittyProbeState::Disabled);
        assert!(!probe.start());
    }

    #[test]
    fn kitty_probe_disables_when_device_attributes_arrive_first() {
        let mut probe = KittyProbe::new(None, true);
        let fence = probe.observe(&TerminalEvent::DeviceAttributes);
        assert_eq!(fence.graphics, Some(false));
        assert!(fence.finish_file_probe);
        assert_eq!(probe.state, KittyProbeState::Disabled);
        let late = probe.observe(&TerminalEvent::KittyGraphicsResponse {
            image_id: PROBE_IMAGE_ID,
            ok: true,
        });
        assert_eq!(late.graphics, None);
        assert!(late.consumed);
    }

    #[test]
    fn file_probe_selects_file_only_on_ok_and_honors_the_override() {
        let mut supported = KittyProbe::new(None, true);
        let update = supported.observe(&TerminalEvent::KittyGraphicsResponse {
            image_id: FILE_PROBE_IMAGE_ID,
            ok: true,
        });
        assert_eq!(update.transport, Some(FrameTransport::File));
        assert!(update.finish_file_probe);
        assert_eq!(supported.transport(), FrameTransport::File);

        let mut rejected = KittyProbe::new(None, true);
        let update = rejected.observe(&TerminalEvent::KittyGraphicsResponse {
            image_id: FILE_PROBE_IMAGE_ID,
            ok: false,
        });
        assert_eq!(update.transport, None);
        assert_eq!(rejected.transport(), FrameTransport::Inline);

        assert_eq!(
            resolve_frame_transport(true, Some(FrameTransport::Inline)),
            FrameTransport::Inline
        );
        assert_eq!(
            resolve_frame_transport(false, Some(FrameTransport::File)),
            FrameTransport::File
        );
        assert_eq!(
            parse_frame_transport_override(" file "),
            Some(FrameTransport::File)
        );
        assert_eq!(
            parse_frame_transport_override("INLINE"),
            Some(FrameTransport::Inline)
        );
        assert_eq!(parse_frame_transport_override("auto"), None);
    }

    #[test]
    fn provider_navigation_publishes_the_full_tab_descriptor_once_changed() {
        let current = BrowserDescriptor {
            tabs: vec!["https://one".to_owned()],
            active_tab: 0,
            profile: "default".to_owned(),
        };
        assert!(
            set_browser_tabs_command(PaneId(3), &current, vec!["https://one".to_owned()], 0,)
                .is_none()
        );

        let command = set_browser_tabs_command(
            PaneId(3),
            &current,
            vec!["https://one".to_owned(), "https://two".to_owned()],
            1,
        )
        .unwrap();
        assert_eq!(command.name, "set-browser-tabs");
        assert_eq!(
            command.args,
            ["-t", "%3", "-a", "1", "--", "https://one", "https://two",]
        );
        assert!(set_browser_tabs_command(PaneId(3), &current, Vec::new(), 0).is_none());
        assert!(
            set_browser_tabs_command(PaneId(3), &current, vec!["https://two".to_owned()], 1)
                .is_none()
        );
    }

    #[test]
    fn only_the_root_tables_mouse_names_are_collected() {
        let binding = |key: &str| zz_protocol::KeyBindingSnapshot {
            key: key.to_owned(),
            commands: Vec::new(),
            repeat: false,
            note: None,
        };
        let tables = vec![
            zz_protocol::KeyTableSnapshot {
                name: "root".to_owned(),
                bindings: vec![
                    binding("MouseDown1Pane"),
                    binding("M-MouseDrag1Border"),
                    binding("WheelUpStatus"),
                    binding("C-b"),
                    binding("F9"),
                ],
            },
            zz_protocol::KeyTableSnapshot {
                name: "copy-mode".to_owned(),
                bindings: vec![binding("MouseDrag1Pane")],
            },
        ];
        let names = mouse_binding_names(&tables);
        assert!(names.contains("MouseDown1Pane"));
        assert!(names.contains("M-MouseDrag1Border"));
        assert!(names.contains("WheelUpStatus"));
        assert!(!names.contains("C-b"));
        assert!(!names.contains("F9"));
        assert!(!names.contains("MouseDrag1Pane"));
    }
}
