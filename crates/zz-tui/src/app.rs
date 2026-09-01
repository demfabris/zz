use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{self, Read as _},
    mem,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use zz_client::{ClientCore, CoreEvent, Outbound};
use zz_daemon::{Endpoint, HostEntry, InteractiveClient};
use zz_protocol::{
    BrowserCommand, BrowserDescriptor, ClientExitAction, CommandInvocation, CommandResponse,
    GuiResponse, InputMessage, NEW_SESSION_ATTACH_CAPABILITY, PaneId, PaneKindSnapshot,
    ProtocolMessage, ServerError, ServerHello, TerminalUiCommand,
};
use zz_terminal::{SearchQuery, TerminalColorScheme, TerminalViewAction, TerminalViewport};

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
    tty::{TerminalGuard, TerminalSize},
};

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
}

const KITTY_GATE_PROBING: u8 = 0;
const KITTY_GATE_ENABLED: u8 = 1;
const KITTY_GATE_DISABLED: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KittyProbeState {
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
    const fn new(transport_override: Option<FrameTransport>) -> Self {
        Self {
            state: KittyProbeState::Probing,
            file_pending: true,
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
}

struct ProtocolReader {
    cancelled: Arc<AtomicBool>,
}

impl ProtocolReader {
    fn cancel(&self, client: &InteractiveClient) {
        self.cancelled.store(true, Ordering::Release);
        let _ = client.request_resync();
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
    Request {
        target: Option<String>,
        detach_others: bool,
        read_only: bool,
        client_flags: Option<String>,
    },
    AlreadyAttached {
        session: zz_protocol::SessionId,
        messages: Vec<ProtocolMessage>,
    },
}

pub(crate) fn run(
    initial: InteractiveClient,
    mut endpoint: Endpoint,
    local_endpoint: Endpoint,
    initial_attach: InitialAttach,
    host_label: String,
    local_host_label: String,
    fleet_hosts: Vec<HostEntry>,
    browser_provider: Option<Box<dyn BrowserFrameProvider>>,
) -> Result<(), String> {
    let (attach_target, attach_request, mut read_only, mut client_flags, initial_messages, attempt) =
        match initial_attach {
            InitialAttach::Request {
                target,
                detach_others,
                read_only,
                client_flags,
            } => {
                let attempt = if target.is_some() {
                    AttachAttempt::Explicit
                } else {
                    AttachAttempt::Default
                };
                (
                    target.unwrap_or_default(),
                    Some(detach_others),
                    read_only,
                    client_flags,
                    Vec::new(),
                    attempt,
                )
            }
            InitialAttach::AlreadyAttached { session, messages } => (
                session.to_string(),
                None,
                false,
                None,
                messages,
                AttachAttempt::Explicit,
            ),
        };
    let size = TerminalSize::detect().map_err(|error| error.to_string())?;
    let mut core = seeded_core(initial.server_hello().clone());
    let mut client = Arc::new(initial);
    if let Some(detach_others) = attach_request {
        client
            .attach_session(
                attach_target.clone(),
                detach_others,
                read_only,
                client_flags.as_deref(),
            )
            .map_err(|error| error.to_string())?;
    }

    let escape_time = Arc::new(AtomicU64::new(escape_timeout_ms(
        lock_core(&core).mux_options(),
    )));
    let mut terminal = TerminalGuard::enter(mouse_option_enabled(lock_core(&core).mux_options()))
        .map_err(|error| error.to_string())?;
    let pixel_mouse = terminal.pixel_mouse();
    let key_releases = terminal.kitty_keyboard();
    let mut model = Model::new(
        &lock_core(&core),
        size,
        host_label,
        local_host_label,
        endpoint.clone(),
        local_endpoint,
        fleet_hosts,
    );
    if attach_request.is_some() {
        model.begin_client_focus_attach();
    }
    let mut renderer = Renderer::new();
    let mut browser = BrowserState::new(browser_provider);
    let mut kitty_probe = KittyProbe::new(configured_frame_transport_override());
    renderer.set_frame_transport(kitty_probe.transport());
    browser.set_transport(kitty_probe.transport(), Instant::now());
    let kitty_gate = Arc::new(AtomicU8::new(KITTY_GATE_PROBING));
    let (events, incoming) = mpsc::channel();
    let mut frames = Arc::new(FrameInbox::default());
    let mut kitty_images = Arc::new(KittyImageInbox::default());
    spawn_signal_reader(events.clone())?;
    let mut connection_id = 1;
    let mut protocol_reader = spawn_protocol_reader(
        Arc::clone(&client),
        Arc::clone(&core),
        connection_id,
        initial_messages,
        events.clone(),
        Arc::clone(&frames),
        Arc::clone(&kitty_images),
        Arc::clone(&kitty_gate),
    )?;
    spawn_terminal_reader(events.clone(), Arc::clone(&escape_time))?;

    let mut attempt = attempt;
    let mut creating_default = false;
    let mut remembered_session = None;
    let mut reconnect_available = true;
    renderer
        .paint(&model, true)
        .map_err(|error| error.to_string())?;

    let outcome = loop {
        let now = Instant::now();
        if model.expire_client_message(now) {
            renderer
                .paint(&model, false)
                .map_err(|error| error.to_string())?;
        }
        let now = Instant::now();
        let event = if browser.should_pump(now) {
            None
        } else {
            receive_main_event(&incoming, message_wait(&model, browser.wait(now), now))?
        };
        let Some(event) = event else {
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
            MainEvent::Frames(event_connection) => {
                if event_connection != connection_id {
                    continue;
                }
                for (pane, frame) in frames.take() {
                    if !model.accepts_viewport(pane) {
                        model.viewports.remove(&pane);
                        renderer.forget_pane(pane);
                        continue;
                    }
                    model.viewports.insert(pane, frame.viewport);
                    renderer.note_frame(pane, frame.damage);
                }
                if let Some(sequence) = sync_mouse_modes(&mut model, pixel_mouse) {
                    renderer.queue_control(sequence);
                }
                renderer
                    .paint_frames(&model)
                    .map_err(|error| error.to_string())?;
            }
            MainEvent::KittyImages(event_connection) => {
                if event_connection != connection_id {
                    continue;
                }
                let updates = kitty_images.take();
                if kitty_probe.state == KittyProbeState::Disabled {
                    continue;
                }
                let changed = !updates.is_empty();
                for update in updates {
                    match update {
                        KittyImageUpdate::Ready(image) => renderer.install_kitty_image(image),
                        KittyImageUpdate::Removed { pane, image_ids } => {
                            renderer.remove_kitty_images(pane, &image_ids);
                        }
                    }
                }
                if changed && kitty_probe.state == KittyProbeState::Enabled {
                    renderer
                        .paint(&model, false)
                        .map_err(|error| error.to_string())?;
                }
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
                match input::handle(
                    &mut model,
                    &client,
                    &mut browser,
                    event,
                    pixel_mouse,
                    key_releases,
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
                        model.set_size(size);
                        if size.columns > 0 && size.rows > 0 {
                            client
                                .send_input(InputMessage::ClientTerminalSize {
                                    columns: size.columns,
                                    rows: size.rows,
                                })
                                .map_err(|error| error.to_string())?;
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
                                    &mut protocol_reader,
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
                    InputOutcome::Detach => {
                        break Ok(TuiExit::Detached(attached_session_name(&model)));
                    }
                }
            }
            MainEvent::Terminal(Err(error)) => break Err(error),
            MainEvent::Core { connection, event } => {
                if connection != connection_id {
                    continue;
                }
                if let CoreEvent::Attached { .. } = &*event {
                    {
                        let core = lock_core(&core);
                        read_only = core.attached_read_only();
                        client_flags = (!core.attached_client_flags().is_empty())
                            .then(|| core.attached_client_flags().to_owned());
                    }
                    browser.reset_connection();
                    renderer.reset_kitty_images();
                    reconnect_available = true;
                }
                if matches!(
                    &*event,
                    CoreEvent::MuxOptionsChanged
                        | CoreEvent::HelloReceived
                        | CoreEvent::Attached { .. }
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
                    && previous_popup != model.popup.as_ref().map(|popup| popup.pane)
                    && let Some(pane) = previous_popup
                {
                    frames.remove(pane);
                    renderer.forget_pane(pane);
                }
                match outcome {
                    ProtocolOutcome::None => {}
                    ProtocolOutcome::Repaint => renderer
                        .paint(&model, false)
                        .map_err(|error| error.to_string())?,
                    ProtocolOutcome::RepaintAll => {
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
                    ProtocolOutcome::QueueControl(output) => {
                        renderer.queue_control(output);
                        renderer
                            .paint(&model, false)
                            .map_err(|error| error.to_string())?;
                    }
                    ProtocolOutcome::Exit(reason) => break Ok(reason),
                }
                if let Some(sequence) = sync_mouse_modes(&mut model, pixel_mouse) {
                    renderer.queue_control(sequence);
                    renderer
                        .paint(&model, false)
                        .map_err(|error| error.to_string())?;
                }
            }
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
                    &mut protocol_reader,
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
                    model.set_size(size);
                    if size.columns > 0 && size.rows > 0 {
                        client
                            .send_input(InputMessage::ClientTerminalSize {
                                columns: size.columns,
                                rows: size.rows,
                            })
                            .map_err(|error| error.to_string())?;
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
            }
            MainEvent::Signal => break Ok(TuiExit::Detached(attached_session_name(&model))),
        }
        remembered_session = model.attached_session.or(remembered_session);
    };

    browser.close_all();
    drop(terminal);
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

fn connect(endpoint: &Endpoint) -> Result<InteractiveClient, String> {
    InteractiveClient::connect_endpoint(endpoint, TerminalColorScheme::Dark)
        .map_err(|error| error.to_string())
}

fn prepare_connection(
    endpoint: &Endpoint,
    attach_target: String,
    read_only: bool,
    client_flags: Option<&str>,
) -> Result<PreparedConnection, String> {
    let client = connect(endpoint)?;
    let core = seeded_core(client.server_hello().clone());
    let client = Arc::new(client);
    client
        .attach_session(attach_target, false, read_only, client_flags)
        .map_err(|error| error.to_string())?;
    Ok(PreparedConnection { client, core })
}

/// The handshake hello is consumed by [`InteractiveClient`] before the reader
/// thread exists, so it is fed to the core by hand; draining the resulting
/// events keeps the reader's first drain free of handshake leftovers.
fn seeded_core(hello: ServerHello) -> Arc<Mutex<ClientCore>> {
    let mut core = ClientCore::new();
    core.handle_message(ProtocolMessage::ServerHello(hello));
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
}

/// The pin's `server_client_reset_state`: outer mouse modes follow the option,
/// or the active pane's own request while the option is off.
fn sync_mouse_modes(model: &mut Model, pixel_mouse: bool) -> Option<Vec<u8>> {
    let viewport_tracks_mouse = model.popup.as_ref().map_or_else(
        || {
            model
                .active_viewport()
                .is_some_and(|viewport| viewport.mouse_tracking)
        },
        |popup| {
            model
                .viewports
                .get(&popup.pane)
                .is_some_and(|viewport| viewport.mouse_tracking)
        },
    );
    let desired = model.mouse_option || viewport_tracks_mouse;
    if desired == model.mouse_modes_active {
        return None;
    }
    model.mouse_modes_active = desired;
    Some(if desired {
        crate::tty::mouse_enable_sequence(pixel_mouse)
    } else {
        crate::tty::MOUSE_DISABLE_SEQUENCE.to_vec()
    })
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

fn replace_connection(
    client: &mut Arc<InteractiveClient>,
    core: &mut Arc<Mutex<ClientCore>>,
    protocol_reader: &mut ProtocolReader,
    connection_id: &mut u64,
    connected: PreparedConnection,
    events: &mpsc::Sender<MainEvent>,
    frames: &mut Arc<FrameInbox>,
    kitty_images: &mut Arc<KittyImageInbox>,
    kitty_gate: &Arc<AtomicU8>,
) -> Result<(), String> {
    let next_connection_id = connection_id.wrapping_add(1).max(1);
    let next_frames = Arc::new(FrameInbox::default());
    let next_kitty_images = Arc::new(KittyImageInbox::default());
    let next_reader = spawn_protocol_reader(
        Arc::clone(&connected.client),
        Arc::clone(&connected.core),
        next_connection_id,
        Vec::new(),
        events.clone(),
        Arc::clone(&next_frames),
        Arc::clone(&next_kitty_images),
        Arc::clone(kitty_gate),
    )?;

    protocol_reader.cancel(client);
    kitty_images.clear();
    *client = connected.client;
    *core = connected.core;
    *protocol_reader = next_reader;
    *connection_id = next_connection_id;
    *frames = next_frames;
    *kitty_images = next_kitty_images;
    Ok(())
}

/// Drives one connection's [`ClientCore`]: decoded messages in, wire requests
/// straight back out, frames into the coalescing inbox, everything else to the
/// main loop in stream order.
fn spawn_protocol_reader(
    client: Arc<InteractiveClient>,
    core: Arc<Mutex<ClientCore>>,
    connection: u64,
    initial_messages: Vec<ProtocolMessage>,
    events: mpsc::Sender<MainEvent>,
    frames: Arc<FrameInbox>,
    kitty_images: Arc<KittyImageInbox>,
    kitty_gate: Arc<AtomicU8>,
) -> Result<ProtocolReader, String> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = Arc::clone(&cancelled);
    thread::Builder::new()
        .name("zz-tui-protocol".to_owned())
        .spawn(move || {
            let mut initial_messages = VecDeque::from(initial_messages);
            'reader: loop {
                let message = if let Some(message) = initial_messages.pop_front() {
                    message
                } else {
                    match client.recv() {
                        Ok(message) => message,
                        Err(error) => {
                            if !thread_cancelled.load(Ordering::Acquire) {
                                let _ = events.send(MainEvent::Disconnected {
                                    connection,
                                    error: error.to_string(),
                                });
                            }
                            break;
                        }
                    }
                };
                if thread_cancelled.load(Ordering::Acquire) {
                    break;
                }
                let forwarded = {
                    let mut core = lock_core(&core);
                    core.handle_message(message);
                    while let Some(outbound) = core.poll_outbound() {
                        let Outbound::RequestFull(pane) = outbound;
                        if let Err(error) = client.request_full(pane) {
                            log::warn!("failed to request a full viewport for {pane}: {error}");
                        }
                    }
                    let mut forwarded = Vec::new();
                    while let Some(event) = core.poll_event() {
                        match event {
                            CoreEvent::ViewportChanged { pane, damage } => {
                                if let Some(viewport) = core.viewport(pane) {
                                    frames.publish(
                                        pane,
                                        viewport.clone(),
                                        damage,
                                        connection,
                                        &events,
                                    );
                                }
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
                                    kitty_images.begin(
                                        pane,
                                        image_id,
                                        generation,
                                        width,
                                        height,
                                        total_bytes,
                                    );
                                }
                            }
                            CoreEvent::KittyImageChunk {
                                pane,
                                image_id,
                                generation,
                                bytes,
                            } => {
                                if kitty_gate.load(Ordering::Acquire) != KITTY_GATE_DISABLED {
                                    kitty_images.push_chunk(
                                        pane, image_id, generation, bytes, connection, &events,
                                    );
                                }
                            }
                            CoreEvent::KittyImagesRemoved { pane, image_ids } => {
                                if kitty_gate.load(Ordering::Acquire) != KITTY_GATE_DISABLED {
                                    kitty_images.remove(pane, image_ids, connection, &events);
                                }
                            }
                            CoreEvent::Attached { session } => {
                                frames.clear();
                                kitty_images.clear();
                                forwarded.push(CoreEvent::Attached { session });
                            }
                            CoreEvent::PaneRemoved { pane } => {
                                kitty_images.remove_pane(pane);
                                forwarded.push(CoreEvent::PaneRemoved { pane });
                            }
                            event => forwarded.push(event),
                        }
                    }
                    forwarded
                };
                for event in forwarded {
                    if events
                        .send(MainEvent::Core {
                            connection,
                            event: Box::new(event),
                        })
                        .is_err()
                    {
                        break 'reader;
                    }
                }
            }
        })
        .map_err(|error| format!("failed to start protocol reader: {error}"))?;
    Ok(ProtocolReader { cancelled })
}

fn spawn_terminal_reader(
    events: mpsc::Sender<MainEvent>,
    escape_time: Arc<AtomicU64>,
) -> Result<(), String> {
    let (bytes_sender, bytes_receiver) = mpsc::sync_channel::<Result<Vec<u8>, String>>(16);
    thread::Builder::new()
        .name("zz-tui-stdin".to_owned())
        .spawn(move || {
            let mut stdin = io::stdin().lock();
            let mut buffer = [0_u8; 4096];
            loop {
                match stdin.read(&mut buffer) {
                    Ok(0) => {
                        let _ = bytes_sender.send(Err("terminal input closed".to_owned()));
                        break;
                    }
                    Ok(length) => {
                        if bytes_sender.send(Ok(buffer[..length].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        let _ = bytes_sender.send(Err(error.to_string()));
                        break;
                    }
                }
            }
        })
        .map_err(|error| format!("failed to start terminal input reader: {error}"))?;

    thread::Builder::new()
        .name("zz-tui-input-parser".to_owned())
        .spawn(move || {
            let mut parser = EventParser::default();
            loop {
                let received = if parser.has_pending_escape() {
                    let timeout = Duration::from_millis(escape_time.load(Ordering::Relaxed));
                    match bytes_receiver.recv_timeout(timeout) {
                        Ok(bytes) => Ok(bytes),
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let mut decoded = Vec::new();
                            parser.flush_escape(&mut decoded);
                            if send_terminal_events(&events, decoded).is_err() {
                                break;
                            }
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => Err(mpsc::RecvError),
                    }
                } else {
                    bytes_receiver.recv()
                };
                match received {
                    Ok(Ok(bytes)) => {
                        let mut decoded = Vec::new();
                        parser.push(&bytes, &mut decoded);
                        if send_terminal_events(&events, decoded).is_err() {
                            break;
                        }
                    }
                    Ok(Err(error)) => {
                        let _ = events.send(MainEvent::Terminal(Err(error)));
                        break;
                    }
                    Err(_) => break,
                }
            }
        })
        .map(drop)
        .map_err(|error| format!("failed to start terminal reader: {error}"))
}

fn send_terminal_events(
    events: &mpsc::Sender<MainEvent>,
    decoded: Vec<TerminalEvent>,
) -> Result<(), ()> {
    for event in decoded {
        events.send(MainEvent::Terminal(Ok(event))).map_err(drop)?;
    }
    Ok(())
}

fn spawn_signal_reader(events: mpsc::Sender<MainEvent>) -> Result<(), String> {
    use async_signal::{Signal, Signals};

    let mut signals = Signals::new([Signal::Hup, Signal::Int, Signal::Term, Signal::Winch])
        .map_err(|error| error.to_string())?;
    thread::Builder::new()
        .name("zz-tui-signals".to_owned())
        .spawn(move || {
            use futures_lite::StreamExt as _;

            futures_lite::future::block_on(async {
                while let Some(signal) = signals.next().await {
                    match signal {
                        Ok(Signal::Winch) => {
                            if events.send(MainEvent::Resize).is_err() {
                                break;
                            }
                        }
                        Ok(Signal::Hup | Signal::Int | Signal::Term) => {
                            let _ = events.send(MainEvent::Signal);
                            break;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            let _ = events.send(MainEvent::Terminal(Err(error.to_string())));
                            break;
                        }
                    }
                }
            });
        })
        .map_err(|error| format!("failed to start signal reader: {error}"))?;
    Ok(())
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
        // `SnapshotChanged` is queued right behind this and does the repaint;
        // adopting the snapshot here keeps the new session and the painted
        // layout from ever disagreeing.
        CoreEvent::Attached { session } => {
            *attempt = AttachAttempt::Idle;
            model.attached_session = Some(session);
            model.viewports.clear();
            model.set_command_output(None, None);
            model.set_popup(None);
            model.popup_keys_down.clear();
            model.set_menu(None);
            model.confirm = None;
            model.confirm_reply_pending = false;
            model.client_message = None;
            model.update_snapshot(Arc::clone(lock_core(core).snapshot()));
            if let Some(input) = model.finish_client_focus_attach() {
                client
                    .send_input(input)
                    .map_err(|error| error.to_string())?;
            }
            *creating_default = false;
            Ok(ProtocolOutcome::None)
        }
        CoreEvent::SnapshotChanged => {
            model.update_snapshot(Arc::clone(lock_core(core).snapshot()));
            Ok(ProtocolOutcome::RepaintAll)
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
            model.command_prompt = lock_core(core).command_prompt().cloned();
            Ok(ProtocolOutcome::Repaint)
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
            model.choose_tree = lock_core(core).choose_tree().cloned();
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::ChooseBufferChanged => {
            model.choose_buffer = lock_core(core).choose_buffer().cloned();
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
            model.set_menu(lock_core(core).menu().cloned());
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
            model.viewports.remove(&pane);
            Ok(ProtocolOutcome::RepaintAll)
        }
        CoreEvent::Detached {
            session,
            by: _,
            action,
        } if model.attached_session == Some(session) => {
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
        CoreEvent::Clipboard { target, text, .. } => match clipboard::encode(target, &text) {
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
        | CoreEvent::PrefixCancelled { .. }
        | CoreEvent::Bell { .. }
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

fn receive_main_event(
    incoming: &mpsc::Receiver<MainEvent>,
    wait: BrowserWait,
) -> Result<Option<MainEvent>, String> {
    match wait {
        BrowserWait::Blocking => incoming.recv().map(Some).map_err(|error| error.to_string()),
        BrowserWait::Timeout(timeout) => match incoming.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("main event channel disconnected".to_owned())
            }
        },
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
            let content = entry.rect.content();
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
        client
            .send_input(InputMessage::ResizeTerminal {
                pane,
                columns,
                rows,
                cell_width_px,
                cell_height_px,
            })
            .map_err(|error| error.to_string())?;
        model.last_sent_geometry.insert(pane, geometry);
    }
    if let Some((geometry, message)) = command_output_resize_message(model) {
        client
            .send_input(message)
            .map_err(|error| error.to_string())?;
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
    use zz_protocol::{PopupBorderLines, PopupState};
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
                }],
                viewers: Vec::new(),
            }],
            focused_window: Some(window),
        }));
        (model, pane)
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
                if columns == content.width && rows == content.height
        ));
    }

    #[test]
    fn unrelated_request_zero_error_preserves_pending_attach_recovery() {
        let (mut model, _) = paned_model();
        assert!(model.finish_client_focus_attach().is_some());
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
        model.mouse_modes_active = false;

        assert!(sync_mouse_modes(&mut model, false).is_none());

        model.viewports.insert(pane, tracking_viewport(true));
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(b"\x1b[?1003h\x1b[?1006h".as_slice())
        );
        assert!(model.mouse_modes_active);
        assert!(sync_mouse_modes(&mut model, false).is_none());

        model.viewports.insert(pane, tracking_viewport(false));
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::MOUSE_DISABLE_SEQUENCE)
        );
        assert!(!model.mouse_modes_active);

        model.mouse_option = true;
        assert_eq!(
            sync_mouse_modes(&mut model, true).as_deref(),
            Some(b"\x1b[?1003h\x1b[?1006h\x1b[?1016h".as_slice())
        );
    }

    #[test]
    fn popup_descriptor_owns_outer_mouse_tracking_even_before_its_frame() {
        let (mut model, pane) = paned_model();
        model.mouse_option = false;
        model.mouse_modes_active = true;
        model.viewports.insert(pane, tracking_viewport(true));
        let popup = PaneId(u64::MAX - 1);
        model.popup = Some(popup_state(popup));

        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::MOUSE_DISABLE_SEQUENCE)
        );
        assert!(!model.mouse_modes_active);

        model.viewports.insert(popup, tracking_viewport(true));
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(b"\x1b[?1003h\x1b[?1006h".as_slice())
        );
        assert!(model.mouse_modes_active);

        model.viewports.insert(popup, tracking_viewport(false));
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(crate::tty::MOUSE_DISABLE_SEQUENCE)
        );
        model.popup = None;
        assert_eq!(
            sync_mouse_modes(&mut model, false).as_deref(),
            Some(b"\x1b[?1003h\x1b[?1006h".as_slice())
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
            crate::input::app_mouse_forward_action(&model, event, 5, 0, 40, 0).is_none(),
            "the pane header row is not content: nothing forwards"
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
        let mut probe = KittyProbe::new(None);
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
    fn kitty_probe_disables_when_device_attributes_arrive_first() {
        let mut probe = KittyProbe::new(None);
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
        let mut supported = KittyProbe::new(None);
        let update = supported.observe(&TerminalEvent::KittyGraphicsResponse {
            image_id: FILE_PROBE_IMAGE_ID,
            ok: true,
        });
        assert_eq!(update.transport, Some(FrameTransport::File));
        assert!(update.finish_file_probe);
        assert_eq!(supported.transport(), FrameTransport::File);

        let mut rejected = KittyProbe::new(None);
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
}
