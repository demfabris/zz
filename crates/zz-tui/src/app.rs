use std::{
    collections::{HashMap, HashSet},
    io::{self, Read as _},
    mem,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use zz_daemon::{Endpoint, HostEntry, InteractiveClient};
use zz_protocol::{
    BrowserCommand, BrowserDescriptor, CommandInvocation, CommandResponse, Event, EventPayload,
    GuiResponse, InputMessage, NEW_SESSION_ATTACH_CAPABILITY, PaneId, PaneKindSnapshot,
    ProtocolMessage, ServerError, ServerHello,
};
use zz_terminal::{TerminalColorScheme, TerminalViewport, TerminalViewportPatch};

use crate::{
    browser::{BrowserFrameProvider, BrowserState, BrowserSurface, BrowserWait, SurfaceChanges},
    clipboard::{self, Osc52},
    input::{self, InputOutcome},
    kitty::{
        FILE_PROBE_IMAGE_ID, FrameTransport, KittyImageAssembler, KittyImageData, PROBE_IMAGE_ID,
    },
    render::{FrameDamage, Renderer},
    state::{HostSwitch, Model},
    terminal_event::{Event as TerminalEvent, EventParser},
    tty::{TerminalGuard, TerminalSize},
};

enum MainEvent {
    Protocol {
        connection: u64,
        message: Box<ProtocolMessage>,
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
                    pending.damage.merge(damage.clone());
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
    hello: ServerHello,
}

enum HostSwitchDecision<T> {
    Current,
    Switch { host: HostSwitch, connected: T },
}

#[derive(Clone, Debug)]
enum AttachAttempt {
    Default,
    Explicit,
    Remembered,
}

enum ProtocolOutcome {
    None,
    Repaint,
    RepaintAll,
    QueueControl(Vec<u8>),
    Exit(String),
}

pub(crate) fn run(
    initial: InteractiveClient,
    mut endpoint: Endpoint,
    local_endpoint: Endpoint,
    target: Option<&str>,
    host_label: String,
    local_host_label: String,
    fleet_hosts: Vec<HostEntry>,
    browser_provider: Option<Box<dyn BrowserFrameProvider>>,
) -> Result<(), String> {
    let size = TerminalSize::detect().map_err(|error| error.to_string())?;
    let hello = initial.server_hello().clone();
    let mut client = Arc::new(initial);
    let attach_target = target.unwrap_or_default().to_owned();
    client
        .attach(attach_target)
        .map_err(|error| error.to_string())?;

    let mut terminal = TerminalGuard::enter().map_err(|error| error.to_string())?;
    let pixel_mouse = terminal.pixel_mouse();
    let mut model = Model::new(
        &hello,
        size,
        host_label,
        local_host_label,
        endpoint.clone(),
        local_endpoint,
        fleet_hosts,
    );
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
        connection_id,
        events.clone(),
        Arc::clone(&frames),
        Arc::clone(&kitty_images),
        Arc::clone(&kitty_gate),
    )?;
    spawn_terminal_reader(events.clone())?;

    let mut attempt = if target.is_some() {
        AttachAttempt::Explicit
    } else {
        AttachAttempt::Default
    };
    let mut creating_default = false;
    let mut remembered_session = None;
    let mut reconnect_available = true;
    renderer
        .paint(&model, true)
        .map_err(|error| error.to_string())?;

    let outcome = loop {
        let now = Instant::now();
        let event = if browser.should_pump(now) {
            None
        } else {
            receive_main_event(&incoming, browser.wait(now))?
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
                    model.viewports.insert(pane, frame.viewport);
                    renderer.note_frame(pane, frame.damage);
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
                match input::handle(&mut model, &client, &mut browser, event, pixel_mouse)? {
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
                    InputOutcome::SwitchHost(host) => {
                        let label = host.label.clone();
                        match prepare_host_switch(&endpoint, host, |target| {
                            prepare_connection(target, String::new())
                        }) {
                            Ok(HostSwitchDecision::Current) => {}
                            Err(error) => {
                                model.client_message =
                                    Some(format!("could not connect to {label}: {error}"));
                                renderer
                                    .paint(&model, false)
                                    .map_err(|paint| paint.to_string())?;
                            }
                            Ok(HostSwitchDecision::Switch { host, connected }) => {
                                let next_endpoint = host.endpoint.clone();
                                let replacement = replace_connection(
                                    &mut client,
                                    &mut protocol_reader,
                                    &mut connection_id,
                                    connected,
                                    &events,
                                    &mut frames,
                                    &mut kitty_images,
                                    &kitty_gate,
                                );
                                match replacement {
                                    Err(error) => {
                                        model.client_message =
                                            Some(format!("could not switch to {label}: {error}"));
                                        renderer
                                            .paint(&model, false)
                                            .map_err(|paint| paint.to_string())?;
                                    }
                                    Ok(hello) => {
                                        browser.reset_connection();
                                        renderer.reset_kitty_images();
                                        endpoint = next_endpoint;
                                        model.set_connected_host(host, &hello);
                                        model.client_message =
                                            Some(format!("connected to {label}"));
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
                    InputOutcome::Detach => break Ok("detached".to_owned()),
                }
            }
            MainEvent::Terminal(Err(error)) => break Err(error),
            MainEvent::Protocol {
                connection,
                message,
            } => {
                if connection != connection_id {
                    continue;
                }
                if matches!(&*message, ProtocolMessage::Attached { .. }) {
                    browser.reset_connection();
                    renderer.reset_kitty_images();
                }
                if let ProtocolMessage::Event(Event {
                    payload: EventPayload::PaneRemoved(pane),
                    ..
                }) = &*message
                {
                    renderer.remove_kitty_pane(*pane);
                }
                if matches!(*message, ProtocolMessage::Attached { .. }) {
                    reconnect_available = true;
                }
                match handle_protocol(
                    &mut model,
                    &client,
                    *message,
                    &mut attempt,
                    &mut creating_default,
                    &mut browser,
                )? {
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
            }
            MainEvent::Disconnected { connection, error } => {
                if connection != connection_id {
                    continue;
                }
                if !reconnect_available {
                    break Err(format!("connection closed after reconnect: {error}"));
                }
                reconnect_available = false;
                log::warn!("zz-tui connection closed: {error}");
                let session = remembered_session.or(model.attached_session);
                let replacement = prepare_connection(
                    &endpoint,
                    session.map_or_else(String::new, |session| session.to_string()),
                )
                .map_err(|reconnect| {
                    format!("connection closed ({error}); reconnect failed: {reconnect}")
                })?;
                attempt = if session.is_some() {
                    AttachAttempt::Remembered
                } else {
                    AttachAttempt::Default
                };
                creating_default = false;
                let hello = replace_connection(
                    &mut client,
                    &mut protocol_reader,
                    &mut connection_id,
                    replacement,
                    &events,
                    &mut frames,
                    &mut kitty_images,
                    &kitty_gate,
                )?;
                browser.reset_connection();
                renderer.reset_kitty_images();
                model.reset_connection(&hello);
                model.client_message = Some("reconnected".to_owned());
                renderer.invalidate();
                renderer
                    .paint(&model, true)
                    .map_err(|paint| paint.to_string())?;
            }
            MainEvent::Resize => {
                if let Ok(size) = TerminalSize::detect() {
                    model.set_size(size);
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
            MainEvent::Signal => break Ok("detached".to_owned()),
        }
        remembered_session = model.attached_session.or(remembered_session);
    };

    browser.close_all();
    drop(terminal);
    match outcome {
        Ok(message) => {
            println!("{message}");
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn connect(endpoint: &Endpoint) -> Result<InteractiveClient, String> {
    InteractiveClient::connect_endpoint(endpoint, TerminalColorScheme::Dark)
        .map_err(|error| error.to_string())
}

fn prepare_connection(
    endpoint: &Endpoint,
    attach_target: String,
) -> Result<PreparedConnection, String> {
    let client = connect(endpoint)?;
    let hello = client.server_hello().clone();
    let client = Arc::new(client);
    client
        .attach(attach_target)
        .map_err(|error| error.to_string())?;
    Ok(PreparedConnection { client, hello })
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
    protocol_reader: &mut ProtocolReader,
    connection_id: &mut u64,
    connected: PreparedConnection,
    events: &mpsc::Sender<MainEvent>,
    frames: &mut Arc<FrameInbox>,
    kitty_images: &mut Arc<KittyImageInbox>,
    kitty_gate: &Arc<AtomicU8>,
) -> Result<ServerHello, String> {
    let next_connection_id = connection_id.wrapping_add(1).max(1);
    let next_frames = Arc::new(FrameInbox::default());
    let next_kitty_images = Arc::new(KittyImageInbox::default());
    let next_reader = spawn_protocol_reader(
        Arc::clone(&connected.client),
        next_connection_id,
        events.clone(),
        Arc::clone(&next_frames),
        Arc::clone(&next_kitty_images),
        Arc::clone(kitty_gate),
    )?;

    protocol_reader.cancel(client);
    kitty_images.clear();
    *client = connected.client;
    *protocol_reader = next_reader;
    *connection_id = next_connection_id;
    *frames = next_frames;
    *kitty_images = next_kitty_images;
    Ok(connected.hello)
}

fn spawn_protocol_reader(
    client: Arc<InteractiveClient>,
    connection: u64,
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
            let mut retained = HashMap::<PaneId, TerminalViewport>::new();
            let mut full_pending = HashSet::<PaneId>::new();
            loop {
                let message = match client.recv() {
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
                };
                if thread_cancelled.load(Ordering::Acquire) {
                    break;
                }
                match message {
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::TerminalViewport { pane, viewport },
                        ..
                    }) => {
                        full_pending.remove(&pane);
                        retained.insert(pane, viewport.clone());
                        frames.publish(pane, viewport, FrameDamage::All, connection, &events);
                    }
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::TerminalPatch { pane, patch },
                        ..
                    }) => {
                        let damage = retained
                            .get(&pane)
                            .map_or(FrameDamage::All, |viewport| patch_damage(viewport, &patch));
                        let applied = retained
                            .get_mut(&pane)
                            .is_some_and(|viewport| viewport.apply_patch(patch).is_ok());
                        if applied {
                            if let Some(viewport) = retained.get(&pane) {
                                frames.publish(pane, viewport.clone(), damage, connection, &events);
                            }
                        } else if full_pending.insert(pane)
                            && let Err(error) = client.request_full(pane)
                        {
                            full_pending.remove(&pane);
                            log::warn!("failed to request a full viewport for {pane}: {error}");
                        }
                    }
                    ProtocolMessage::Attached { session, snapshot } => {
                        full_pending.clear();
                        retained.clear();
                        frames.clear();
                        kitty_images.clear();
                        if events
                            .send(MainEvent::Protocol {
                                connection,
                                message: Box::new(ProtocolMessage::Attached { session, snapshot }),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    ProtocolMessage::Event(Event {
                        sequence,
                        payload: EventPayload::Snapshot(snapshot),
                    }) => {
                        full_pending.clear();
                        if events
                            .send(MainEvent::Protocol {
                                connection,
                                message: Box::new(ProtocolMessage::Event(Event {
                                    sequence,
                                    payload: EventPayload::Snapshot(snapshot),
                                })),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    ProtocolMessage::Event(Event {
                        sequence,
                        payload: EventPayload::PaneRemoved(pane),
                    }) => {
                        retained.remove(&pane);
                        full_pending.remove(&pane);
                        kitty_images.remove_pane(pane);
                        if events
                            .send(MainEvent::Protocol {
                                connection,
                                message: Box::new(ProtocolMessage::Event(Event {
                                    sequence,
                                    payload: EventPayload::PaneRemoved(pane),
                                })),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    ProtocolMessage::Event(Event {
                        payload:
                            EventPayload::KittyImageBegin {
                                pane,
                                image_id,
                                generation,
                                width,
                                height,
                                total_bytes,
                            },
                        ..
                    }) => {
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
                    ProtocolMessage::Event(Event {
                        payload:
                            EventPayload::KittyImageChunk {
                                pane,
                                image_id,
                                generation,
                                bytes,
                            },
                        ..
                    }) => {
                        if kitty_gate.load(Ordering::Acquire) != KITTY_GATE_DISABLED {
                            kitty_images
                                .push_chunk(pane, image_id, generation, bytes, connection, &events);
                        }
                    }
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::KittyImagesRemoved { pane, image_ids },
                        ..
                    }) => {
                        if kitty_gate.load(Ordering::Acquire) != KITTY_GATE_DISABLED {
                            kitty_images.remove(pane, image_ids, connection, &events);
                        }
                    }
                    message => {
                        if events
                            .send(MainEvent::Protocol {
                                connection,
                                message: Box::new(message),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        })
        .map_err(|error| format!("failed to start protocol reader: {error}"))?;
    Ok(ProtocolReader { cancelled })
}

fn patch_damage(previous: &TerminalViewport, patch: &TerminalViewportPatch) -> FrameDamage {
    if patch.scroll != 0
        || patch.foreground != previous.foreground
        || patch.background != previous.background
    {
        return FrameDamage::All;
    }
    let mut rows = patch.changed_rows.row_indices().to_vec();
    rows.extend(previous.overlays.iter().map(|overlay| overlay.row));
    rows.extend(patch.overlays.iter().map(|overlay| overlay.row));
    rows.sort_unstable();
    rows.dedup();
    FrameDamage::Rows(rows)
}

fn spawn_terminal_reader(events: mpsc::Sender<MainEvent>) -> Result<(), String> {
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
                    match bytes_receiver.recv_timeout(Duration::from_millis(25)) {
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

fn handle_protocol(
    model: &mut Model,
    client: &InteractiveClient,
    message: ProtocolMessage,
    attempt: &mut AttachAttempt,
    creating_default: &mut bool,
    browser: &mut BrowserState,
) -> Result<ProtocolOutcome, String> {
    match message {
        ProtocolMessage::Attached { session, snapshot } => {
            model.attached_session = Some(session);
            model.viewports.clear();
            model.client_message = None;
            model.update_snapshot(snapshot);
            *creating_default = false;
            Ok(ProtocolOutcome::RepaintAll)
        }
        ProtocolMessage::CommandResponse(CommandResponse::Error {
            request_id: 0,
            error: ServerError::MissingTarget(target),
        }) => match attempt {
            AttachAttempt::Remembered => {
                *attempt = AttachAttempt::Default;
                client.attach("").map_err(|error| error.to_string())?;
                Ok(ProtocolOutcome::None)
            }
            AttachAttempt::Default if !*creating_default => {
                *creating_default = true;
                client.request_resync().map_err(|error| error.to_string())?;
                client
                    .execute(CommandInvocation::new("new-session", [] as [&str; 0]))
                    .map_err(|error| error.to_string())?;
                if !model
                    .capabilities
                    .iter()
                    .any(|capability| capability == NEW_SESSION_ATTACH_CAPABILITY)
                {
                    client
                        .execute(CommandInvocation::new("attach-session", [] as [&str; 0]))
                        .map_err(|error| error.to_string())?;
                }
                Ok(ProtocolOutcome::Repaint)
            }
            AttachAttempt::Default => Ok(ProtocolOutcome::None),
            AttachAttempt::Explicit => Err(format!("session `{target}` was not found")),
        },
        ProtocolMessage::CommandResponse(CommandResponse::Error {
            request_id: 0,
            error: ServerError::PaneExited(_) | ServerError::PaneNotAttached(_),
        }) => Ok(ProtocolOutcome::None),
        ProtocolMessage::CommandResponse(CommandResponse::Error { error, .. }) => {
            model.client_message = Some(error.to_string());
            Ok(ProtocolOutcome::Repaint)
        }
        ProtocolMessage::CommandResponse(CommandResponse::Success { .. }) => {
            model.client_message = None;
            Ok(ProtocolOutcome::Repaint)
        }
        ProtocolMessage::Event(event) => handle_event(model, client, browser, event),
        _ => Ok(ProtocolOutcome::None),
    }
}

fn handle_event(
    model: &mut Model,
    client: &InteractiveClient,
    browser: &mut BrowserState,
    event: Event,
) -> Result<ProtocolOutcome, String> {
    match event.payload {
        EventPayload::Snapshot(snapshot) => {
            model.update_snapshot(snapshot);
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::AppearanceChanged { appearance, .. } => {
            model.appearance = *appearance;
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::StatusChanged { status } => {
            model.status = status;
            Ok(ProtocolOutcome::Repaint)
        }
        EventPayload::PrefixArmed { armed } => {
            model.prefix_armed = armed;
            Ok(ProtocolOutcome::Repaint)
        }
        EventPayload::CommandPrompt { state } => {
            model.command_prompt = state;
            Ok(ProtocolOutcome::Repaint)
        }
        EventPayload::CommandOutput { pane, viewport } => {
            model.command_output = viewport.map(|viewport| (pane, viewport));
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::ChooseTree { state } => {
            model.choose_tree = state;
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::ChooseTreeUpdate { search, selected } => {
            if let Some(state) = &mut model.choose_tree {
                state.search = search;
                state.selected = selected;
            }
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::ChooseBuffer { state } => {
            model.choose_buffer = state;
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::ChooseBufferUpdate { search, selected } => {
            if let Some(state) = &mut model.choose_buffer {
                state.search = search;
                state.selected = selected;
            }
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::DisplayPanes { state } => {
            model.display_panes = state;
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::ClientMessage { text, .. } => {
            model.client_message = Some(text);
            Ok(ProtocolOutcome::Repaint)
        }
        EventPayload::PaneRemoved(pane) => {
            model.viewports.remove(&pane);
            Ok(ProtocolOutcome::RepaintAll)
        }
        EventPayload::Detached { session, by } if model.attached_session == Some(session) => Ok(
            ProtocolOutcome::Exit(
                by.map_or_else(|| "detached".to_owned(), |by| format!("detached by {by}")),
            ),
        ),
        EventPayload::ServerStopping => Ok(ProtocolOutcome::Exit("zz daemon stopped".to_owned())),
        EventPayload::AgentCommand { request_id, .. } => {
            client
                .send_gui_response(GuiResponse::Error {
                    request_id,
                    message: "agent commands require the zz app".to_owned(),
                })
                .map_err(|error| error.to_string())?;
            Ok(ProtocolOutcome::None)
        }
        EventPayload::Clipboard { target, text, .. } => match clipboard::encode(target, &text) {
            Osc52::Empty => Ok(ProtocolOutcome::None),
            Osc52::Encoded(output) => Ok(ProtocolOutcome::QueueControl(output)),
            Osc52::TooLarge => {
                model.client_message = Some("clipboard payload exceeds 1 MiB".to_owned());
                Ok(ProtocolOutcome::Repaint)
            }
        },
        EventPayload::FocusSidebar => {
            if model.focus_sidebar() {
                Ok(ProtocolOutcome::RepaintAll)
            } else {
                Ok(ProtocolOutcome::Repaint)
            }
        }
        EventPayload::BrowserCommand {
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
        EventPayload::BrowserCommand { pane, command } => {
            browser.command(pane, &command);
            Ok(ProtocolOutcome::None)
        }
        EventPayload::TerminalUiCommand { .. } => {
            model.client_message = Some("terminal search is unsupported here".to_owned());
            Ok(ProtocolOutcome::Repaint)
        }
        EventPayload::TerminalViewport { pane, viewport } => {
            model.viewports.insert(pane, viewport);
            Ok(ProtocolOutcome::Repaint)
        }
        EventPayload::Detached { .. }
        | EventPayload::TerminalPatch { .. }
        | EventPayload::MuxOptionsChanged { .. }
        | EventPayload::PrefixBindingsChanged { .. }
        | EventPayload::OpenUri { .. }
        | EventPayload::HistoryChunk { .. }
        | EventPayload::Bell { .. }
        | EventPayload::KittyImageBegin { .. }
        | EventPayload::KittyImageChunk { .. }
        | EventPayload::KittyImagesRemoved { .. } => Ok(ProtocolOutcome::None),
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
