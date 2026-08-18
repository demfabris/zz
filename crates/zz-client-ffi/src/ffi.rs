#![allow(clippy::missing_safety_doc)]

use std::{
    collections::VecDeque,
    ffi::{CStr, c_char, c_int},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::Path,
    sync::{Arc, Mutex, PoisonError},
    thread,
};

use zz_client::{ClientCore, CoreEvent, Outbound, ViewportDamage};
use zz_daemon::InteractiveClient;
use zz_protocol::{
    CommandInvocation, InputMessage, MuxSnapshot, PaneId, PaneKindSnapshot, PaneSnapshot,
    ProtocolMessage, SessionId, SessionSnapshot, WindowSnapshot,
};
use zz_terminal::{
    CellWidth, CursorStyle, Glyph, KeyAction, KeyCode, KeyInput, Modifiers, PackedStyle,
    TerminalViewAction, TerminalViewport,
};

const EVENT_DAMAGE_ALL: u32 = 1;

/// Event kinds mirrored in `include/zz-client.h`; values are ABI.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZzEventKind {
    Hello = 0,
    Attached = 1,
    SnapshotChanged = 2,
    ViewportChanged = 3,
    PaneRemoved = 4,
    StatusChanged = 5,
    Detached = 6,
    ServerStopping = 7,
    Other = 8,
    AppearanceChanged = 9,
    Disconnected = 10,
}

/// One drained event; `pane` is zero when the kind carries no pane.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ZzEvent {
    pub kind: ZzEventKind,
    pub flags: u32,
    pub pane: u64,
    pub row_start: u16,
    pub row_end: u16,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZzPaneKind {
    #[default]
    Picker = 0,
    Terminal = 1,
    Browser = 2,
    Agent = 3,
    Editor = 4,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ZzBytes {
    pub ptr: *const u8,
    pub len: usize,
}

impl ZzBytes {
    const EMPTY: Self = Self {
        ptr: std::ptr::null(),
        len: 0,
    };

    fn new(value: &str) -> Self {
        Self {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ZzCursor {
    pub color: u32,
    pub column: u16,
    pub row: u16,
    pub style: u8,
    pub visible: u8,
    pub blinking: u8,
    pub wide_tail: u8,
}

/// An attached client: connection, reader thread, reduced core, event queue,
/// and the wake fd the caller's main loop polls.
pub struct ZzClient {
    client: Arc<InteractiveClient>,
    core: Arc<Mutex<ClientCore>>,
    events: Arc<Mutex<VecDeque<ZzEvent>>>,
    wake_read: UnixStream,
    reader: Option<thread::JoinHandle<()>>,
}

pub struct ZzMuxSnapshot {
    snapshot: Arc<MuxSnapshot>,
    attached: Option<SessionId>,
}

/// A caller-owned viewport snapshot; cheap to acquire (shared immutable
/// planes), stable until released.
pub struct ZzViewport(TerminalViewport);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn session_at(snapshot: &ZzMuxSnapshot, index: usize) -> Option<&SessionSnapshot> {
    snapshot.snapshot.sessions.get(index)
}

fn active_window_at(snapshot: &ZzMuxSnapshot, session: usize) -> Option<&WindowSnapshot> {
    let session = session_at(snapshot, session)?;
    let window = snapshot.snapshot.focused_window_for(session);
    session
        .windows
        .iter()
        .find(|candidate| candidate.id == window)
}

fn pane_at(
    snapshot: &ZzMuxSnapshot,
    session: usize,
    pane: usize,
) -> Option<(&WindowSnapshot, &PaneSnapshot)> {
    let window = active_window_at(snapshot, session)?;
    let mut panes = Vec::with_capacity(window.panes.len());
    window.layout.panes(&mut panes);
    let pane = panes.get(pane)?;
    Some((window, window.panes.get(pane)?))
}

fn pane_kind(kind: &PaneKindSnapshot) -> ZzPaneKind {
    match kind {
        PaneKindSnapshot::Picker => ZzPaneKind::Picker,
        PaneKindSnapshot::Terminal => ZzPaneKind::Terminal,
        PaneKindSnapshot::Browser(_) => ZzPaneKind::Browser,
        PaneKindSnapshot::Agent(_) => ZzPaneKind::Agent,
        PaneKindSnapshot::Editor(_) => ZzPaneKind::Editor,
    }
}

fn key_code(code: u32, codepoint: u32, function: u8) -> Option<KeyCode> {
    Some(match code {
        0 => KeyCode::Character(char::from_u32(codepoint)?),
        1 => KeyCode::Backspace,
        2 => KeyCode::Enter,
        3 => KeyCode::Tab,
        4 => KeyCode::Escape,
        5 => KeyCode::Delete,
        6 => KeyCode::Insert,
        7 => KeyCode::Home,
        8 => KeyCode::End,
        9 => KeyCode::PageUp,
        10 => KeyCode::PageDown,
        11 => KeyCode::ArrowUp,
        12 => KeyCode::ArrowDown,
        13 => KeyCode::ArrowLeft,
        14 => KeyCode::ArrowRight,
        15 => KeyCode::Function(function),
        16 => KeyCode::Unidentified,
        _ => return None,
    })
}

fn key_action(action: u32) -> Option<KeyAction> {
    match action {
        0 => Some(KeyAction::Press),
        1 => Some(KeyAction::Repeat),
        2 => Some(KeyAction::Release),
        _ => None,
    }
}

fn queue_event(events: &Mutex<VecDeque<ZzEvent>>, event: &CoreEvent) {
    let (kind, flags, pane, row_start, row_end) = match event {
        CoreEvent::HelloReceived => (ZzEventKind::Hello, 0, 0, 0, 0),
        CoreEvent::Attached { .. } => (ZzEventKind::Attached, 0, 0, 0, 0),
        CoreEvent::SnapshotChanged => (ZzEventKind::SnapshotChanged, 0, 0, 0, 0),
        CoreEvent::ViewportChanged { pane, damage } => match damage {
            ViewportDamage::All => (
                ZzEventKind::ViewportChanged,
                EVENT_DAMAGE_ALL,
                pane.0,
                0,
                u16::MAX,
            ),
            ViewportDamage::Rows(rows) => (
                ZzEventKind::ViewportChanged,
                0,
                pane.0,
                rows.iter().copied().min().unwrap_or(0),
                rows.iter().copied().max().unwrap_or(0),
            ),
        },
        CoreEvent::PaneRemoved { pane } => (ZzEventKind::PaneRemoved, 0, pane.0, 0, 0),
        CoreEvent::StatusChanged => (ZzEventKind::StatusChanged, 0, 0, 0, 0),
        CoreEvent::AppearanceChanged => (ZzEventKind::AppearanceChanged, 0, 0, 0, 0),
        CoreEvent::Detached { .. } => (ZzEventKind::Detached, 0, 0, 0, 0),
        CoreEvent::ServerStopping => (ZzEventKind::ServerStopping, 0, 0, 0, 0),
        _ => (ZzEventKind::Other, 0, 0, 0, 0),
    };
    lock(events).push_back(ZzEvent {
        kind,
        flags,
        pane,
        row_start,
        row_end,
    });
}

fn wake_event_fd(wake_write: &UnixStream) -> std::io::Result<()> {
    loop {
        match rustix::io::write(wake_write, &[1]) {
            Ok(1) | Err(rustix::io::Errno::AGAIN) => return Ok(()),
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "event fd accepted no wake byte",
                ));
            }
            Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn spawn_reader(
    client: &Arc<InteractiveClient>,
    core: &Arc<Mutex<ClientCore>>,
    events: &Arc<Mutex<VecDeque<ZzEvent>>>,
    wake_write: UnixStream,
) -> std::io::Result<thread::JoinHandle<()>> {
    let client = Arc::clone(client);
    let core = Arc::clone(core);
    let events = Arc::clone(events);
    thread::Builder::new()
        .name("zz-client-ffi-reader".to_owned())
        .spawn(move || {
            while let Ok(message) = client.recv() {
                let mut guard = lock(&core);
                guard.handle_message(message);
                while let Some(outbound) = guard.poll_outbound() {
                    match outbound {
                        Outbound::RequestFull(pane) => {
                            let _ = client.request_full(pane);
                        }
                    }
                }
                let mut queued = false;
                while let Some(event) = guard.poll_event() {
                    queue_event(&events, &event);
                    queued = true;
                }
                drop(guard);
                if queued {
                    let _ = wake_event_fd(&wake_write);
                }
            }
            lock(&events).push_back(ZzEvent {
                kind: ZzEventKind::Disconnected,
                flags: 0,
                pane: 0,
                row_start: 0,
                row_end: 0,
            });
            let _ = wake_event_fd(&wake_write);
        })
}

impl Drop for ZzClient {
    fn drop(&mut self) {
        let _ = self.client.shutdown();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Connect to a zz daemon socket and start the reader thread.
///
/// # Safety
///
/// `socket_path` must be a valid NUL-terminated string. Returns null when the
/// path is invalid or the daemon is unreachable; the caller owns the handle
/// and must release it with [`zz_client_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_connect(socket_path: *const c_char) -> *mut ZzClient {
    if socket_path.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(path) = unsafe { CStr::from_ptr(socket_path) }.to_str() else {
        return std::ptr::null_mut();
    };
    let Ok(client) = InteractiveClient::connect(Path::new(path)) else {
        return std::ptr::null_mut();
    };
    let Ok((wake_read, wake_write)) = UnixStream::pair() else {
        return std::ptr::null_mut();
    };
    if wake_read.set_nonblocking(true).is_err() || wake_write.set_nonblocking(true).is_err() {
        return std::ptr::null_mut();
    }
    let client = Arc::new(client);
    let core = Arc::new(Mutex::new(ClientCore::new()));
    lock(&core).handle_message(ProtocolMessage::ServerHello(client.server_hello().clone()));
    let events = Arc::new(Mutex::new(VecDeque::new()));
    let mut queued = false;
    {
        let mut guard = lock(&core);
        while let Some(event) = guard.poll_event() {
            queue_event(&events, &event);
            queued = true;
        }
    }
    if queued && wake_event_fd(&wake_write).is_err() {
        return std::ptr::null_mut();
    }
    let Ok(reader) = spawn_reader(&client, &core, &events, wake_write) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(ZzClient {
        client,
        core,
        events,
        wake_read,
        reader: Some(reader),
    }))
}

/// Release a handle returned by [`zz_client_connect`].
///
/// # Safety
///
/// `client` must be a pointer previously returned by [`zz_client_connect`]
/// and must not be used afterwards. Null is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_free(client: *mut ZzClient) {
    if !client.is_null() {
        drop(unsafe { Box::from_raw(client) });
    }
}

/// The pollable wake fd: readable whenever new events are queued. Read it dry,
/// then drain [`zz_client_next_event`] until it returns false.
///
/// # Safety
///
/// `client` must be a live handle from [`zz_client_connect`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_event_fd(client: *const ZzClient) -> c_int {
    unsafe { client.as_ref() }.map_or(-1, |client| client.wake_read.as_raw_fd())
}

/// Attach this connection to a session by name or target.
///
/// # Safety
///
/// `client` must be a live handle; `session` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_attach(client: *mut ZzClient, session: *const c_char) -> bool {
    let (Some(client), false) = (unsafe { client.as_mut() }, session.is_null()) else {
        return false;
    };
    let Ok(session) = unsafe { CStr::from_ptr(session) }.to_str() else {
        return false;
    };
    client.client.attach(session).is_ok()
}

/// Send literal text to a pane; the daemon routes it through its key tables.
///
/// # Safety
///
/// `client` must be a live handle; `text` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_send_text(
    client: *mut ZzClient,
    pane: u64,
    text: *const c_char,
) -> bool {
    let (Some(client), false) = (unsafe { client.as_mut() }, text.is_null()) else {
        return false;
    };
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return false;
    };
    client
        .client
        .send_input(InputMessage::Text {
            pane: PaneId(pane),
            text: text.to_owned(),
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_send_key(
    client: *mut ZzClient,
    pane: u64,
    code: u32,
    codepoint: u32,
    function: u8,
    action: u32,
    modifiers: u8,
    text: *const c_char,
    text_follows: bool,
) -> bool {
    let Some(client) = (unsafe { client.as_mut() }) else {
        return false;
    };
    let (Some(key), Some(action), Some(modifiers)) = (
        key_code(code, codepoint, function),
        key_action(action),
        Modifiers::from_bits(modifiers),
    ) else {
        return false;
    };
    let text = if text.is_null() {
        None
    } else {
        let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
            return false;
        };
        Some(text.to_owned().into_boxed_str())
    };
    client
        .client
        .send_input(InputMessage::Key {
            pane: PaneId(pane),
            input: KeyInput {
                action,
                key,
                modifiers,
                text,
                unshifted_codepoint: None,
            },
            text_follows,
        })
        .is_ok()
}

/// Execute a tmux-style command (`name` plus arguments) on the daemon.
///
/// # Safety
///
/// `client` must be a live handle; `name` a valid NUL-terminated string;
/// `args` must point to `args_len` valid NUL-terminated strings (may be null
/// when `args_len` is zero).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_execute(
    client: *mut ZzClient,
    name: *const c_char,
    args: *const *const c_char,
    args_len: usize,
) -> bool {
    let (Some(client), false) = (unsafe { client.as_mut() }, name.is_null()) else {
        return false;
    };
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return false;
    };
    let mut arguments = Vec::with_capacity(args_len);
    for index in 0..args_len {
        if args.is_null() {
            return false;
        }
        let argument = unsafe { *args.add(index) };
        if argument.is_null() {
            return false;
        }
        let Ok(argument) = unsafe { CStr::from_ptr(argument) }.to_str() else {
            return false;
        };
        arguments.push(argument.to_owned());
    }
    client
        .client
        .execute(CommandInvocation::new(name, arguments))
        .is_ok()
}

/// Report a terminal pane's geometry to the daemon.
///
/// # Safety
///
/// `client` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_resize_terminal(
    client: *mut ZzClient,
    pane: u64,
    columns: u16,
    rows: u16,
    cell_width_px: u32,
    cell_height_px: u32,
) -> bool {
    let Some(client) = (unsafe { client.as_mut() }) else {
        return false;
    };
    client
        .client
        .send_input(InputMessage::ResizeTerminal {
            pane: PaneId(pane),
            columns,
            rows,
            cell_width_px,
            cell_height_px,
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_scroll_lines(
    client: *mut ZzClient,
    pane: u64,
    lines: i32,
) -> bool {
    let Some(client) = (unsafe { client.as_mut() }) else {
        return false;
    };
    client
        .client
        .send_input(InputMessage::TerminalView {
            pane: PaneId(pane),
            action: TerminalViewAction::ScrollLines(lines),
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_focus_terminal(
    client: *mut ZzClient,
    pane: u64,
    focused: bool,
) -> bool {
    let Some(client) = (unsafe { client.as_mut() }) else {
        return false;
    };
    client
        .client
        .send_input(InputMessage::TerminalView {
            pane: PaneId(pane),
            action: TerminalViewAction::Focus(focused),
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_snapshot_acquire(client: *const ZzClient) -> *mut ZzMuxSnapshot {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let core = lock(&client.core);
    Box::into_raw(Box::new(ZzMuxSnapshot {
        snapshot: Arc::clone(core.snapshot()),
        attached: core.attached_session(),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_release(snapshot: *mut ZzMuxSnapshot) {
    if !snapshot.is_null() {
        drop(unsafe { Box::from_raw(snapshot) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_generation(snapshot: *const ZzMuxSnapshot) -> u64 {
    unsafe { snapshot.as_ref() }.map_or(0, |snapshot| snapshot.snapshot.generation)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_count(snapshot: *const ZzMuxSnapshot) -> usize {
    unsafe { snapshot.as_ref() }.map_or(0, |snapshot| snapshot.snapshot.sessions.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_id(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
) -> u64 {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| session_at(snapshot, session))
        .map_or(0, |session| session.id.0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_name(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
) -> ZzBytes {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| session_at(snapshot, session))
        .map_or(ZzBytes::EMPTY, |session| ZzBytes::new(&session.name))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_is_attached(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
) -> bool {
    let Some(snapshot) = (unsafe { snapshot.as_ref() }) else {
        return false;
    };
    session_at(snapshot, session).is_some_and(|session| snapshot.attached == Some(session.id))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_active_window(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
) -> u64 {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| active_window_at(snapshot, session))
        .map_or(0, |window| window.id.0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_pane_count(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
) -> usize {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| active_window_at(snapshot, session))
        .map_or(0, |window| window.panes.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_pane_id(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    pane: usize,
) -> u64 {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| pane_at(snapshot, session, pane))
        .map_or(0, |(_, pane)| pane.id.0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_pane_title(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    pane: usize,
) -> ZzBytes {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| pane_at(snapshot, session, pane))
        .map_or(ZzBytes::EMPTY, |(_, pane)| ZzBytes::new(&pane.title))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_pane_kind(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    pane: usize,
) -> ZzPaneKind {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| pane_at(snapshot, session, pane))
        .map_or(ZzPaneKind::Picker, |(_, pane)| pane_kind(&pane.kind))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_pane_is_active(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    pane: usize,
) -> bool {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| pane_at(snapshot, session, pane))
        .is_some_and(|(window, pane)| window.active_pane == pane.id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_pane_has_bell(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    pane: usize,
) -> bool {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| pane_at(snapshot, session, pane))
        .is_some_and(|(_, pane)| pane.bell)
}

/// Write up to `capacity` terminal pane ids from the attached session into
/// `out`; returns how many exist (which may exceed `capacity`). Empty until
/// [`zz_client_attach`] succeeds and a snapshot arrives.
///
/// # Safety
///
/// `client` must be a live handle; `out` must point to `capacity` writable
/// `uint64_t`s (may be null when `capacity` is zero).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_terminal_panes(
    client: *const ZzClient,
    out: *mut u64,
    capacity: usize,
) -> usize {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return 0;
    };
    let core = lock(&client.core);
    let attached = core.attached_session();
    let mut written = 0;
    let mut total = 0;
    for session in &core.snapshot().sessions {
        if attached != Some(session.id) {
            continue;
        }
        for window in &session.windows {
            for (pane, snapshot) in &window.panes {
                if !matches!(snapshot.kind, PaneKindSnapshot::Terminal) {
                    continue;
                }
                if written < capacity && !out.is_null() {
                    unsafe { out.add(written).write(pane.0) };
                    written += 1;
                }
                total += 1;
            }
        }
    }
    total
}

/// Pop the next queued event into `out`; false when the queue is empty.
///
/// # Safety
///
/// `client` must be a live handle; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_next_event(client: *mut ZzClient, out: *mut ZzEvent) -> bool {
    let (Some(client), false) = (unsafe { client.as_ref() }, out.is_null()) else {
        return false;
    };
    let mut buffer = [0_u8; 64];
    while matches!(rustix::io::read(&client.wake_read, &mut buffer), Ok(count) if count > 0) {}
    let Some(event) = lock(&client.events).pop_front() else {
        return false;
    };
    unsafe { out.write(event) };
    true
}

/// Acquire the retained viewport for a pane, or null when none is held. The
/// snapshot is caller-owned and stable until [`zz_viewport_release`].
///
/// # Safety
///
/// `client` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_viewport_acquire(
    client: *const ZzClient,
    pane: u64,
) -> *mut ZzViewport {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let core = lock(&client.core);
    core.viewport(PaneId(pane))
        .map_or(std::ptr::null_mut(), |viewport| {
            Box::into_raw(Box::new(ZzViewport(viewport.clone())))
        })
}

/// Release a viewport snapshot from [`zz_client_viewport_acquire`].
///
/// # Safety
///
/// `viewport` must be a pointer previously returned by
/// [`zz_client_viewport_acquire`] and must not be used afterwards. Null is
/// ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_release(viewport: *mut ZzViewport) {
    if !viewport.is_null() {
        drop(unsafe { Box::from_raw(viewport) });
    }
}

/// # Safety
///
/// `viewport` must be a live snapshot from [`zz_client_viewport_acquire`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_columns(viewport: *const ZzViewport) -> u16 {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.columns)
}

/// # Safety
///
/// `viewport` must be a live snapshot from [`zz_client_viewport_acquire`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_rows(viewport: *const ZzViewport) -> u16 {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.rows)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_generation(viewport: *const ZzViewport) -> u64 {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.generation)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_view_generation(viewport: *const ZzViewport) -> u64 {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.view_generation)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_dictionary_generation(viewport: *const ZzViewport) -> u32 {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.dictionary_generation)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_foreground(viewport: *const ZzViewport) -> u32 {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.foreground.packed())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_background(viewport: *const ZzViewport) -> u32 {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.background.packed())
}

/// The row-major cell plane (`rows * columns` entries of `zz_cell`). Valid
/// until the snapshot is released.
///
/// # Safety
///
/// `viewport` must be a live snapshot from [`zz_client_viewport_acquire`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_cells(
    viewport: *const ZzViewport,
) -> *const zz_terminal::PackedCell {
    unsafe { viewport.as_ref() }.map_or(std::ptr::null(), |viewport| viewport.0.cells.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_styles(viewport: *const ZzViewport) -> *const PackedStyle {
    unsafe { viewport.as_ref() }.map_or(std::ptr::null(), |viewport| {
        viewport.0.dictionary.styles.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_style_count(viewport: *const ZzViewport) -> usize {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.dictionary.styles.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_grapheme_offsets(viewport: *const ZzViewport) -> *const u32 {
    unsafe { viewport.as_ref() }.map_or(std::ptr::null(), |viewport| {
        viewport.0.dictionary.grapheme_offsets.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_grapheme_offset_count(viewport: *const ZzViewport) -> usize {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.dictionary.grapheme_offsets.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_grapheme_bytes(viewport: *const ZzViewport) -> *const u8 {
    unsafe { viewport.as_ref() }.map_or(std::ptr::null(), |viewport| {
        viewport.0.dictionary.grapheme_bytes.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_grapheme_byte_count(viewport: *const ZzViewport) -> usize {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.dictionary.grapheme_bytes.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_cursor(
    viewport: *const ZzViewport,
    out: *mut ZzCursor,
) -> bool {
    let (Some(viewport), false) = (unsafe { viewport.as_ref() }, out.is_null()) else {
        return false;
    };
    let Some(cursor) = viewport.0.cursor else {
        return false;
    };
    let style = match cursor.style() {
        CursorStyle::Bar => 0,
        CursorStyle::Block => 1,
        CursorStyle::Underline => 2,
        CursorStyle::BlockHollow => 3,
    };
    unsafe {
        out.write(ZzCursor {
            color: cursor.color().packed(),
            column: cursor.column(),
            row: cursor.row(),
            style,
            visible: u8::from(cursor.visible()),
            blinking: u8::from(cursor.blinking()),
            wide_tail: u8::from(cursor.at_wide_tail()),
        });
    }
    true
}

/// Decode one viewport row as UTF-8 text into `buf` (NUL-terminated when
/// `capacity` is nonzero); returns the bytes written excluding the NUL.
///
/// # Safety
///
/// `viewport` must be a live snapshot; `buf` must point to `capacity`
/// writable bytes (may be null when `capacity` is zero).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_row_text(
    viewport: *const ZzViewport,
    row: u16,
    buf: *mut c_char,
    capacity: usize,
) -> usize {
    let Some(viewport) = (unsafe { viewport.as_ref() }) else {
        return 0;
    };
    let viewport = &viewport.0;
    if buf.is_null() || capacity == 0 || row >= viewport.rows {
        return 0;
    }
    let output = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), capacity) };
    let mut length = 0;
    for cell in viewport.row(row).unwrap_or_default() {
        if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
            continue;
        }
        let mut scalar = [0; 4];
        let bytes = match viewport.glyph(*cell) {
            Glyph::Empty => b" ".as_slice(),
            Glyph::Scalar(value) => value.encode_utf8(&mut scalar).as_bytes(),
            Glyph::Grapheme(value) => value.as_bytes(),
        };
        let end = length + bytes.len();
        if end >= capacity {
            break;
        }
        output[length..end].copy_from_slice(bytes);
        length = end;
    }
    output[length] = 0;
    length
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zz_terminal::{GRAPHEME_TABLE_BIT, PackedCell, SessionStatus, TerminalDictionary};

    use super::*;

    fn decode(viewport: &TerminalViewport, capacity: usize) -> Vec<u8> {
        let viewport = ZzViewport(viewport.clone());
        let mut output = vec![0_i8; capacity];
        let length =
            unsafe { zz_viewport_row_text(&viewport, 0, output.as_mut_ptr(), output.len()) };
        output[..length].iter().map(|byte| *byte as u8).collect()
    }

    #[test]
    fn row_text_resolves_graphemes_and_omits_spacer_cells() {
        let grapheme = "e\u{301}";
        let mut viewport = TerminalViewport::blank(5, 1, SessionStatus::Running);
        viewport.cells = Arc::from([
            PackedCell::EMPTY,
            PackedCell::new(GRAPHEME_TABLE_BIT, 0, CellWidth::Wide),
            PackedCell::new(0, 0, CellWidth::SpacerTail),
            PackedCell::new(u32::from('界'), 0, CellWidth::Wide),
            PackedCell::new(0, 0, CellWidth::SpacerTail),
        ]);
        viewport.dictionary = Arc::new(TerminalDictionary::from_shared(
            Arc::clone(&viewport.dictionary.styles),
            Arc::from([0, u32::try_from(grapheme.len()).unwrap()]),
            Arc::from(grapheme.as_bytes()),
        ));

        assert_eq!(
            String::from_utf8(decode(&viewport, 32)).unwrap(),
            " e\u{301}界"
        );
    }

    #[test]
    fn row_text_never_splits_a_utf8_scalar() {
        let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        viewport.cells = Arc::from([PackedCell::new(u32::from('界'), 0, CellWidth::Narrow)]);

        assert!(decode(&viewport, 3).is_empty());
        assert_eq!(String::from_utf8(decode(&viewport, 4)).unwrap(), "界");
    }
}
