#![allow(clippy::missing_safety_doc)]

use std::{
    collections::VecDeque,
    ffi::{CStr, c_char, c_int, c_void},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    thread,
};

#[cfg(target_os = "ios")]
use zeroize::{Zeroize, Zeroizing};
use zz_client::{
    AgentAttentionEdge, AgentAttentionStatus, ClientCore, CoreEvent, NormalizedPaneRect, Outbound,
    ViewportDamage, agent_attention_status, pane_rects,
};
#[cfg(target_os = "ios")]
use zz_daemon::{AskpassPromptKind, AskpassReply, SshPrompts};
use zz_daemon::{DaemonError, Endpoint, EndpointError, InteractiveClient};
use zz_protocol::{
    AgentConnectionPhase, AgentPaneWire, AgentSessionOpKind, CommandInvocation, CommandResponse,
    InputMessage, KeyBindingSnapshot, MuxSnapshot, PaneId, PaneKindSnapshot, PaneSnapshot,
    ProtocolMessage, SessionId, SessionSnapshot, WindowSnapshot,
};
use zz_terminal::{
    CellWidth, ClipboardTarget, CopyModeAction, CursorStyle, Glyph, KeyAction, KeyCode, KeyInput,
    Modifiers, PackedStyle, PointerCellEvent, TerminalColorScheme, TerminalViewAction,
    TerminalViewport,
};

const EVENT_DAMAGE_ALL: u32 = 1;
const EVENT_AGENT_REQUEST: u32 = 1 << 1;
const EVENT_AGENT_DONE: u32 = 1 << 2;
const EVENT_AGENT_FAILED: u32 = 1 << 3;

/// How many unread command replies the queue keeps before it drops the
/// oldest, so a shell that only fires commands never grows without bound.
const MAX_QUEUED_COMMAND_REPLIES: usize = 64;

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
    AgentStateChanged = 11,
    Clipboard = 12,
    /// The daemon armed or cleared the prefix sequence; reread it with the
    /// `zz_prefix_snapshot_*` family. `flags` is 1 while armed.
    PrefixArmed = 13,
    /// The published key tables changed; reread them with the
    /// `zz_prefix_snapshot_*` family.
    KeyTablesChanged = 14,
    /// A `command-prompt` overlay opened or closed on the daemon.
    CommandPromptChanged = 15,
    /// A `choose-buffer` overlay opened or closed on the daemon.
    ChooseBufferChanged = 16,
    /// A `display-panes` overlay opened or closed on the daemon.
    DisplayPanesChanged = 17,
    /// One coalesced agent transcript batch arrived; pop it with
    /// `zz_client_agent_updates_next`. `pane` names the pane.
    AgentUpdates = 18,
    /// The daemon cleared a pane's agent lane; catch up with
    /// `zz_client_agent_lagged_next`, then replay from the shell's cursor.
    AgentLagged = 19,
    /// An agent session-list reply arrived; pop it with
    /// `zz_client_agent_sessions_next`. `pane` names the pane.
    AgentSessions = 20,
    /// An executed command answered; pop the reply with
    /// `zz_client_command_reply_next` and match its request id against the one
    /// `zz_client_execute_request` returned.
    CommandReply = 21,
}

/// One drained event; `pane` is zero when the kind carries no pane.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZzAgentPhase {
    #[default]
    Starting = 0,
    Ready = 1,
    Running = 2,
    AwaitingPermission = 3,
    Failed = 4,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZzAgentAttention {
    #[default]
    Idle = 0,
    Working = 1,
    NeedsInput = 2,
    Failed = 3,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZzAgentPermissionKind {
    #[default]
    Unknown = 0,
    AllowOnce = 1,
    AllowAlways = 2,
    RejectOnce = 3,
    RejectAlways = 4,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZzConnectFailure {
    #[default]
    None = 0,
    Retryable = 1,
    Authentication = 2,
    HostKey = 3,
    Configuration = 4,
    Incompatible = 5,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZzSshPromptKind {
    #[default]
    Secret = 0,
    HostKey = 1,
    Confirmation = 2,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZzSshPromptReply {
    #[default]
    Cancel = 0,
    Answer = 1,
    TrustOnce = 2,
    TrustAndSave = 3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ZzSshPrompt {
    pub kind: ZzSshPromptKind,
    pub title: ZzBytes,
    pub message: ZzBytes,
    pub echo: bool,
}

pub type ZzSshPromptCallback = unsafe extern "C" fn(
    context: *mut c_void,
    prompt: *const ZzSshPrompt,
    response: *mut c_char,
    response_capacity: usize,
) -> ZzSshPromptReply;

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
        Self::from_bytes(value.as_bytes())
    }

    fn from_bytes(value: &[u8]) -> Self {
        Self {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ZzPaneRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<NormalizedPaneRect> for ZzPaneRect {
    fn from(rect: NormalizedPaneRect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
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
    queues: EventQueues,
    wake_read: UnixStream,
    reader: Option<thread::JoinHandle<()>>,
}

/// Every queue the reader thread fans core events into. One struct keeps
/// `queue_event` and `spawn_reader` under the argument-count lint.
#[derive(Clone, Default)]
struct EventQueues {
    events: Arc<Mutex<VecDeque<ZzEvent>>>,
    clipboards: Arc<Mutex<VecDeque<ZzClipboard>>>,
    agent_batches: Arc<Mutex<VecDeque<ZzAgentBatch>>>,
    agent_lagged: Arc<Mutex<VecDeque<ZzAgentLag>>>,
    agent_sessions: Arc<Mutex<VecDeque<ZzAgentSessionsReply>>>,
    command_replies: Arc<Mutex<VecDeque<ZzCommandReply>>>,
}

/// One coalesced agent transcript batch, caller-owned once popped with
/// `zz_client_agent_updates_next`. `items` are the daemon's JSON stream
/// items in order; `first_seq` numbers the first one.
pub struct ZzAgentBatch {
    pane: u64,
    first_seq: u64,
    items: Vec<Vec<u8>>,
}

struct ZzAgentLag {
    pane: u64,
    next_seq: u64,
}

/// One agent session-list reply, caller-owned once popped with
/// `zz_client_agent_sessions_next`. `result` is the daemon's JSON reply:
/// a `sessionsListed` payload on success, a `sessionListFailed` one after a
/// rejected list request.
pub struct ZzAgentSessionsReply {
    pane: u64,
    request_id: u64,
    result: String,
}

/// One executed command's answer, caller-owned once popped with
/// `zz_client_command_reply_next`. `output` is the reply text the daemon
/// prints for that verb (`show-last-output`, `display-message -p`,
/// `list-sessions`, …); `error` holds the rendered server error when the
/// command failed, and is empty otherwise.
pub struct ZzCommandReply {
    request_id: u64,
    ok: bool,
    exit_code: u8,
    output: String,
    error: String,
}

impl ZzCommandReply {
    fn new(response: &CommandResponse) -> Self {
        match response {
            CommandResponse::Success {
                request_id,
                output,
                exit_code,
                ..
            } => Self {
                request_id: *request_id,
                ok: true,
                exit_code: *exit_code,
                output: output.to_string(),
                error: String::new(),
            },
            CommandResponse::Error {
                request_id,
                error,
                output,
            } => Self {
                request_id: *request_id,
                ok: false,
                exit_code: 1,
                output: output.to_string(),
                error: error.to_string(),
            },
        }
    }
}

pub struct ZzMuxSnapshot {
    snapshot: Arc<MuxSnapshot>,
    attached: Option<SessionId>,
}

pub struct ZzAgentState {
    wire: AgentPaneWire,
    permission: Option<ZzAgentPermission>,
}

struct ZzAgentPermission {
    title: String,
    options: Vec<ZzAgentPermissionOption>,
}

struct ZzAgentPermissionOption {
    id: String,
    name: String,
    kind: ZzAgentPermissionKind,
}

pub struct ZzClipboard {
    pane: u64,
    request_id: u64,
    text: String,
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

fn window_at(snapshot: &ZzMuxSnapshot, session: usize, window: usize) -> Option<&WindowSnapshot> {
    session_at(snapshot, session)?.windows.get(window)
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

fn window_pane_at(
    snapshot: &ZzMuxSnapshot,
    session: usize,
    window: usize,
    pane: usize,
) -> Option<(&WindowSnapshot, &PaneSnapshot)> {
    let window = window_at(snapshot, session, window)?;
    let mut panes = Vec::with_capacity(window.panes.len());
    window.layout.panes(&mut panes);
    let pane = panes.get(pane)?;
    Some((window, window.panes.get(pane)?))
}

fn window_pane_rect(window: &WindowSnapshot, pane: PaneId) -> Option<NormalizedPaneRect> {
    if let Some(zoomed) = window.zoomed_pane {
        return (zoomed == pane).then_some(NormalizedPaneRect::FULL);
    }
    pane_rects(&window.layout)
        .into_iter()
        .find_map(|(candidate, rect)| (candidate == pane).then_some(rect))
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

fn agent_state(wire: AgentPaneWire) -> ZzAgentState {
    let permission = wire
        .pending_permission
        .as_ref()
        .and_then(|permission| serde_json::from_str::<serde_json::Value>(&permission.payload).ok())
        .and_then(|payload| {
            let tool_call = payload.get("toolCall")?;
            let title = tool_call
                .get("title")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    tool_call
                        .get("toolCallId")
                        .and_then(serde_json::Value::as_str)
                })
                .unwrap_or("Tool approval")
                .to_owned();
            let options = payload
                .get("options")?
                .as_array()?
                .iter()
                .take(32)
                .filter_map(|option| {
                    let id = option.get("optionId")?.as_str()?.to_owned();
                    let name = option
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(&id)
                        .to_owned();
                    let kind = match option
                        .get("kind")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                    {
                        "allow_once" => ZzAgentPermissionKind::AllowOnce,
                        "allow_always" => ZzAgentPermissionKind::AllowAlways,
                        "reject_once" => ZzAgentPermissionKind::RejectOnce,
                        "reject_always" => ZzAgentPermissionKind::RejectAlways,
                        _ => ZzAgentPermissionKind::Unknown,
                    };
                    Some(ZzAgentPermissionOption { id, name, kind })
                })
                .collect();
            Some(ZzAgentPermission { title, options })
        });
    ZzAgentState { wire, permission }
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

fn queue_event(queues: &EventQueues, event: &CoreEvent) {
    let EventQueues {
        events,
        clipboards,
        agent_batches,
        agent_lagged,
        agent_sessions,
        command_replies,
    } = queues;
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
        CoreEvent::AgentStateChanged { pane, attention } => {
            let flags = match attention {
                Some(AgentAttentionEdge::Request) => EVENT_AGENT_REQUEST,
                Some(AgentAttentionEdge::Done) => EVENT_AGENT_DONE,
                Some(AgentAttentionEdge::Failed) => EVENT_AGENT_FAILED,
                None => 0,
            };
            (ZzEventKind::AgentStateChanged, flags, pane.0, 0, 0)
        }
        CoreEvent::Clipboard {
            pane,
            request_id,
            text,
            ..
        } => {
            lock(clipboards).push_back(ZzClipboard {
                pane: pane.0,
                request_id: *request_id,
                text: text.clone(),
            });
            (ZzEventKind::Clipboard, 0, pane.0, 0, 0)
        }
        CoreEvent::AgentUpdates {
            pane,
            first_seq,
            items,
        } => {
            lock(agent_batches).push_back(ZzAgentBatch {
                pane: pane.0,
                first_seq: *first_seq,
                items: items.clone(),
            });
            (ZzEventKind::AgentUpdates, 0, pane.0, 0, 0)
        }
        CoreEvent::AgentLagged { pane, next_seq } => {
            lock(agent_lagged).push_back(ZzAgentLag {
                pane: pane.0,
                next_seq: *next_seq,
            });
            (ZzEventKind::AgentLagged, 0, pane.0, 0, 0)
        }
        CoreEvent::AgentSessions {
            pane,
            request_id,
            result,
        } => {
            lock(agent_sessions).push_back(ZzAgentSessionsReply {
                pane: pane.0,
                request_id: *request_id,
                result: result.clone(),
            });
            (ZzEventKind::AgentSessions, 0, pane.0, 0, 0)
        }
        CoreEvent::CommandResponse(response) => {
            let mut queue = lock(command_replies);
            while queue.len() >= MAX_QUEUED_COMMAND_REPLIES {
                queue.pop_front();
            }
            queue.push_back(ZzCommandReply::new(response));
            drop(queue);
            (ZzEventKind::CommandReply, 0, 0, 0, 0)
        }
        CoreEvent::PrefixArmed { armed } => (ZzEventKind::PrefixArmed, u32::from(*armed), 0, 0, 0),
        CoreEvent::KeyTablesChanged => (ZzEventKind::KeyTablesChanged, 0, 0, 0, 0),
        CoreEvent::CommandPromptChanged => (ZzEventKind::CommandPromptChanged, 0, 0, 0, 0),
        CoreEvent::ChooseBufferChanged => (ZzEventKind::ChooseBufferChanged, 0, 0, 0, 0),
        CoreEvent::DisplayPanesChanged => (ZzEventKind::DisplayPanesChanged, 0, 0, 0, 0),
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
    queues: &EventQueues,
    wake_write: UnixStream,
) -> std::io::Result<thread::JoinHandle<()>> {
    let client = Arc::clone(client);
    let core = Arc::clone(core);
    let queues = queues.clone();
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
                    queue_event(&queues, &event);
                    queued = true;
                }
                drop(guard);
                if queued {
                    let _ = wake_event_fd(&wake_write);
                }
            }
            lock(&queues.events).push_back(ZzEvent {
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

fn start_client(client: InteractiveClient) -> Result<*mut ZzClient, String> {
    let (wake_read, wake_write) = UnixStream::pair().map_err(|error| error.to_string())?;
    wake_read
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    wake_write
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let client = Arc::new(client);
    let core = Arc::new(Mutex::new(ClientCore::new()));
    lock(&core).handle_message(ProtocolMessage::ServerHello(client.server_hello().clone()));
    let queues = EventQueues::default();
    let mut queued = false;
    {
        let mut guard = lock(&core);
        while let Some(event) = guard.poll_event() {
            queue_event(&queues, &event);
            queued = true;
        }
    }
    if queued {
        wake_event_fd(&wake_write).map_err(|error| error.to_string())?;
    }
    let reader =
        spawn_reader(&client, &core, &queues, wake_write).map_err(|error| error.to_string())?;
    Ok(Box::into_raw(Box::new(ZzClient {
        client,
        core,
        queues,
        wake_read,
        reader: Some(reader),
    })))
}

unsafe fn write_c_string(value: &str, buffer: *mut c_char, capacity: usize) -> usize {
    if buffer.is_null() || capacity == 0 {
        return value.len();
    }
    let mut written = value.len().min(capacity - 1);
    while !value.is_char_boundary(written) {
        written -= 1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), buffer.cast::<u8>(), written);
        *buffer.add(written) = 0;
    }
    value.len()
}

struct ConnectFailure {
    kind: ZzConnectFailure,
    message: String,
}

impl ConnectFailure {
    fn configuration(message: impl Into<String>) -> Self {
        Self {
            kind: ZzConnectFailure::Configuration,
            message: message.into(),
        }
    }
}

fn classify_connect_error(error: &DaemonError) -> ZzConnectFailure {
    if matches!(error, DaemonError::IncompatibleDaemon { .. }) {
        return ZzConnectFailure::Incompatible;
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if let Some(endpoint) = current.downcast_ref::<EndpointError>() {
            return match endpoint {
                EndpointError::HostKeyRejected { .. } => ZzConnectFailure::HostKey,
                EndpointError::AuthenticationFailed { .. } => ZzConnectFailure::Authentication,
                EndpointError::UriParse { .. }
                | EndpointError::RemoteBinaryMissing { .. }
                | EndpointError::InvalidRemoteSocket(_)
                | EndpointError::UnsupportedPlatform => ZzConnectFailure::Configuration,
                EndpointError::RemoteProtocolMismatch { .. }
                | EndpointError::RemoteProtocolUnknown { .. } => ZzConnectFailure::Incompatible,
                EndpointError::SshSpawn { .. }
                | EndpointError::ProbeFailure { .. }
                | EndpointError::SshFailed { .. }
                | EndpointError::RemoteDaemonUnavailable { .. }
                | EndpointError::ForwardExited { .. }
                | EndpointError::ForwardTimeout { .. }
                | EndpointError::ForwardIo { .. } => ZzConnectFailure::Retryable,
            };
        }
        if matches!(
            current.downcast_ref::<zz_protocol::ProtocolError>(),
            Some(zz_protocol::ProtocolError::VersionMismatch { .. })
        ) {
            return ZzConnectFailure::Incompatible;
        }
        source = current.source();
    }
    ZzConnectFailure::Retryable
}

fn connect_endpoint(
    endpoint: &str,
    prompts: Option<zz_daemon::SshPrompts>,
) -> Result<*mut ZzClient, ConnectFailure> {
    let endpoint = Endpoint::parse(endpoint)
        .map_err(|error| ConnectFailure::configuration(error.to_string()))?;
    let client = InteractiveClient::connect_terminal_surface_endpoint_with_prompts(
        &endpoint,
        TerminalColorScheme::Dark,
        prompts,
    )
    .map_err(|error| ConnectFailure {
        kind: classify_connect_error(&error),
        message: error.to_string(),
    })?;
    start_client(client).map_err(|message| ConnectFailure {
        kind: ZzConnectFailure::Retryable,
        message,
    })
}

#[cfg(target_os = "ios")]
unsafe fn password_prompts(password: *const c_char) -> Result<Option<SshPrompts>, &'static str> {
    let password = if password.is_null() {
        None
    } else {
        let password = unsafe { CStr::from_ptr(password) }
            .to_str()
            .map_err(|_| "Password must be valid UTF-8.")?;
        Some(Zeroizing::new(password.to_owned()))
    };
    Ok(Some(SshPrompts::new(
        Path::new("").to_owned(),
        move |prompt| match prompt.kind() {
            AskpassPromptKind::HostKey => AskpassReply::answer("save"),
            AskpassPromptKind::Secret | AskpassPromptKind::AgentConfirm => {
                password.as_ref().map_or(AskpassReply::Cancel, |password| {
                    AskpassReply::answer(password.as_str())
                })
            }
        },
    )))
}

#[cfg(not(target_os = "ios"))]
unsafe fn password_prompts(
    password: *const c_char,
) -> Result<Option<zz_daemon::SshPrompts>, &'static str> {
    if password.is_null() {
        Ok(None)
    } else {
        Err("Password authentication through this API is supported only on iOS.")
    }
}

#[cfg(target_os = "ios")]
fn interactive_prompts(
    callback: Option<ZzSshPromptCallback>,
    context: *mut c_void,
) -> Option<SshPrompts> {
    let callback = callback?;
    let context = context as usize;
    Some(SshPrompts::new(Path::new("").to_owned(), move |prompt| {
        let (kind, title) = match prompt.kind() {
            AskpassPromptKind::Secret => (
                ZzSshPromptKind::Secret,
                if prompt.echo() {
                    "SSH challenge"
                } else {
                    "SSH authentication"
                },
            ),
            AskpassPromptKind::HostKey => (ZzSshPromptKind::HostKey, "Verify SSH host"),
            AskpassPromptKind::AgentConfirm => {
                (ZzSshPromptKind::Confirmation, "Confirm SSH request")
            }
        };
        let value = ZzSshPrompt {
            kind,
            title: ZzBytes::new(title),
            message: ZzBytes::new(prompt.text()),
            echo: prompt.echo(),
        };
        let mut response: [c_char; 4096] = [0; 4096];
        let reply = unsafe {
            callback(
                context as *mut c_void,
                &value,
                response.as_mut_ptr(),
                response.len(),
            )
        };
        let answer = match reply {
            ZzSshPromptReply::Cancel => AskpassReply::Cancel,
            ZzSshPromptReply::TrustOnce => AskpassReply::answer("once"),
            ZzSshPromptReply::TrustAndSave => AskpassReply::answer("save"),
            ZzSshPromptReply::Answer => {
                let length = response
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(response.len());
                let bytes = response[..length]
                    .iter()
                    .map(|byte| byte.to_ne_bytes()[0])
                    .collect::<Vec<_>>();
                String::from_utf8(bytes).map_or(AskpassReply::Cancel, AskpassReply::answer)
            }
        };
        response.zeroize();
        answer
    }))
}

#[cfg(not(target_os = "ios"))]
fn interactive_prompts(
    _callback: Option<ZzSshPromptCallback>,
    _context: *mut c_void,
) -> Option<zz_daemon::SshPrompts> {
    None
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
    start_client(client).unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_connect_endpoint(
    endpoint: *const c_char,
    password: *const c_char,
    error: *mut c_char,
    error_capacity: usize,
) -> *mut ZzClient {
    unsafe {
        write_c_string("", error, error_capacity);
    }
    let result = (|| {
        if endpoint.is_null() {
            return Err("Endpoint is required.".to_owned());
        }
        let endpoint = unsafe { CStr::from_ptr(endpoint) }
            .to_str()
            .map_err(|_| "Endpoint must be valid UTF-8.".to_owned())?;
        let prompts = unsafe { password_prompts(password) }.map_err(str::to_owned)?;
        connect_endpoint(endpoint, prompts).map_err(|error| error.message)
    })();
    match result {
        Ok(client) => client,
        Err(message) => {
            unsafe {
                write_c_string(&message, error, error_capacity);
            }
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_connect_endpoint_interactive(
    endpoint: *const c_char,
    callback: Option<ZzSshPromptCallback>,
    context: *mut c_void,
    failure: *mut ZzConnectFailure,
    error: *mut c_char,
    error_capacity: usize,
) -> *mut ZzClient {
    unsafe {
        write_c_string("", error, error_capacity);
        if let Some(failure) = failure.as_mut() {
            *failure = ZzConnectFailure::None;
        }
    }
    let result = (|| {
        if endpoint.is_null() {
            return Err(ConnectFailure::configuration("Endpoint is required."));
        }
        let endpoint = unsafe { CStr::from_ptr(endpoint) }
            .to_str()
            .map_err(|_| ConnectFailure::configuration("Endpoint must be valid UTF-8."))?;
        connect_endpoint(endpoint, interactive_prompts(callback, context))
    })();
    match result {
        Ok(client) => client,
        Err(connect_error) => {
            unsafe {
                if let Some(failure) = failure.as_mut() {
                    *failure = connect_error.kind;
                }
                write_c_string(&connect_error.message, error, error_capacity);
            }
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_ssh_public_key(buffer: *mut c_char, capacity: usize) -> usize {
    #[cfg(target_os = "ios")]
    {
        match zz_daemon::ios_ssh_public_key() {
            Ok(public_key) => unsafe { write_c_string(&public_key, buffer, capacity) },
            Err(_) => unsafe { write_c_string("", buffer, capacity) },
        }
    }
    #[cfg(not(target_os = "ios"))]
    unsafe {
        write_c_string("", buffer, capacity)
    }
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_set_terminal_preview(
    client: *mut ZzClient,
    enabled: bool,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    client.client.set_terminal_preview(enabled).is_ok()
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

/// Paste text into a terminal pane. The text never reaches the key tables, so
/// a pasted prefix byte stays a byte; the daemon encodes it exactly the way
/// the desktop does, turning newlines into carriage returns and adding
/// bracketed-paste markers only when the pane's program enabled DECSET 2004.
/// Use this for clipboard and drag-and-drop text; [`zz_client_send_text`] is
/// for typing.
///
/// # Safety
///
/// `client` must be a live handle; `text` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_paste(
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
        .send_input(InputMessage::TerminalView {
            pane: PaneId(pane),
            action: TerminalViewAction::Paste(text.to_owned()),
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

unsafe fn command_invocation(
    name: *const c_char,
    args: *const *const c_char,
    args_len: usize,
) -> Option<CommandInvocation> {
    let name = unsafe { c_string(name) }?;
    let mut arguments = Vec::with_capacity(args_len);
    for index in 0..args_len {
        if args.is_null() {
            return None;
        }
        arguments.push(unsafe { c_string(*args.add(index)) }?);
    }
    Some(CommandInvocation::new(name, arguments))
}

/// Execute a tmux-style command (`name` plus arguments) on the daemon,
/// discarding whatever it replies. Use [`zz_client_execute_request`] when the
/// reply text matters.
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
    unsafe { zz_client_execute_request(client, name, args, args_len) != 0 }
}

/// Execute a tmux-style command and return the request id its reply will
/// carry; zero when the request could not be sent. The daemon answers every
/// command, so a `ZZ_EVENT_COMMAND_REPLY` event follows; pop the reply with
/// [`zz_client_command_reply_next`] and match `zz_command_reply_request_id`
/// against the id returned here.
///
/// # Safety
///
/// `client` must be a live handle; `name` a valid NUL-terminated string;
/// `args` must point to `args_len` valid NUL-terminated strings (may be null
/// when `args_len` is zero).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_execute_request(
    client: *mut ZzClient,
    name: *const c_char,
    args: *const *const c_char,
    args_len: usize,
) -> u64 {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return 0;
    };
    let Some(command) = (unsafe { command_invocation(name, args, args_len) }) else {
        return 0;
    };
    client.client.execute(command).unwrap_or(0)
}

/// Close the command-output view a printing command opened for this client.
/// The daemon puts such a client on the pane's copy-mode key table and
/// swallows its terminal input until the view is gone, so a shell that never
/// renders the view has to close it explicitly. This asks for the view's own
/// cancel rather than a key, which is what makes it work under
/// `mode-keys vi`, where Escape is bound to `clear-selection` and leaves the
/// view open. Harmless when no view is open.
///
/// # Safety
///
/// `client` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_cancel_command_output(client: *mut ZzClient) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    client
        .client
        .send_input(InputMessage::CommandOutputView {
            action: TerminalViewAction::CopyMode(CopyModeAction::Cancel),
        })
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
pub unsafe extern "C" fn zz_client_terminal_selection(
    client: *mut ZzClient,
    pane: u64,
    phase: u32,
    column: u16,
    row: u16,
    click_count: u8,
    rectangle: bool,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    let pointer = PointerCellEvent {
        column,
        row,
        click_count,
        rectangle,
    };
    let action = match phase {
        0 => TerminalViewAction::SelectionPress(pointer),
        1 => TerminalViewAction::SelectionDrag(pointer),
        2 => TerminalViewAction::SelectionRelease(pointer),
        _ => return false,
    };
    client
        .client
        .send_input(InputMessage::TerminalView {
            pane: PaneId(pane),
            action,
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_copy_selection(
    client: *mut ZzClient,
    pane: u64,
    request_id: u64,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    client
        .client
        .send_input(InputMessage::TerminalView {
            pane: PaneId(pane),
            action: TerminalViewAction::CopySelection {
                request_id,
                target: ClipboardTarget::Clipboard,
            },
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_set_focused(client: *mut ZzClient, focused: bool) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    client
        .client
        .send_input(InputMessage::ClientFocus { focused })
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
pub unsafe extern "C" fn zz_snapshot_session_window_count(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
) -> usize {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| session_at(snapshot, session))
        .map_or(0, |session| session.windows.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_id(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
) -> u64 {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_at(snapshot, session, window))
        .map_or(0, |window| window.id.0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_index(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
) -> u32 {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_at(snapshot, session, window))
        .map_or(0, |window| window.index)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_name(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
) -> ZzBytes {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_at(snapshot, session, window))
        .map_or(ZzBytes::EMPTY, |window| ZzBytes::new(&window.name))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_is_current(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
) -> bool {
    let Some(snapshot) = (unsafe { snapshot.as_ref() }) else {
        return false;
    };
    let Some(session) = session_at(snapshot, session) else {
        return false;
    };
    session
        .windows
        .get(window)
        .is_some_and(|window| snapshot.snapshot.focused_window_for(session) == window.id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_active_pane(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
) -> u64 {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_at(snapshot, session, window))
        .map_or(0, |window| window.active_pane.0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_zoomed_pane(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
    out: *mut u64,
) -> bool {
    let (Some(snapshot), false) = (unsafe { snapshot.as_ref() }, out.is_null()) else {
        return false;
    };
    let Some(pane) = window_at(snapshot, session, window).and_then(|window| window.zoomed_pane)
    else {
        return false;
    };
    unsafe { out.write(pane.0) };
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_pane_count(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
) -> usize {
    let Some(window) =
        (unsafe { snapshot.as_ref() }).and_then(|snapshot| window_at(snapshot, session, window))
    else {
        return 0;
    };
    let mut panes = Vec::with_capacity(window.panes.len());
    window.layout.panes(&mut panes);
    panes.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_pane_id(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
    pane: usize,
) -> u64 {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_pane_at(snapshot, session, window, pane))
        .map_or(0, |(_, pane)| pane.id.0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_pane_title(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
    pane: usize,
) -> ZzBytes {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_pane_at(snapshot, session, window, pane))
        .map_or(ZzBytes::EMPTY, |(_, pane)| ZzBytes::new(&pane.title))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_pane_kind(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
    pane: usize,
) -> ZzPaneKind {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_pane_at(snapshot, session, window, pane))
        .map_or(ZzPaneKind::Picker, |(_, pane)| pane_kind(&pane.kind))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_pane_is_active(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
    pane: usize,
) -> bool {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_pane_at(snapshot, session, window, pane))
        .is_some_and(|(window, pane)| window.active_pane == pane.id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_pane_has_bell(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
    pane: usize,
) -> bool {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| window_pane_at(snapshot, session, window, pane))
        .is_some_and(|(_, pane)| pane.bell)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_session_window_pane_rect(
    snapshot: *const ZzMuxSnapshot,
    session: usize,
    window: usize,
    pane: usize,
    out: *mut ZzPaneRect,
) -> bool {
    let (Some(snapshot), false) = (unsafe { snapshot.as_ref() }, out.is_null()) else {
        return false;
    };
    let Some((window, pane)) = window_pane_at(snapshot, session, window, pane) else {
        return false;
    };
    let Some(rect) = window_pane_rect(window, pane.id) else {
        return false;
    };
    unsafe { out.write(rect.into()) };
    true
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

/// An owned copy of the prefix arming plus the published `prefix` table's
/// bindings, so Swift can read them after the core lock is released.
pub struct ZzPrefixSnapshot {
    armed: bool,
    bindings: Vec<KeyBindingSnapshot>,
    summaries: Vec<String>,
}

fn prefix_binding_at(snapshot: &ZzPrefixSnapshot, binding: usize) -> Option<&KeyBindingSnapshot> {
    snapshot.bindings.get(binding)
}

/// One binding's command line for help surfaces: the first command's name
/// plus its arguments (`split-window -h`), or empty when unbound.
fn prefix_command_summary(binding: &KeyBindingSnapshot) -> String {
    let Some(first) = binding.commands.first() else {
        return String::new();
    };
    if first.args.is_empty() {
        return first.name.clone();
    }
    let mut summary = first.name.clone();
    for argument in &first.args {
        summary.push(' ');
        summary.push_str(argument);
    }
    summary
}

/// Acquire the prefix arming plus the published `prefix` table's bindings.
/// Returns null for a null client. Free with [`zz_prefix_snapshot_release`].
///
/// # Safety
///
/// `client` must be a live handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_prefix_snapshot_acquire(
    client: *const ZzClient,
) -> *mut ZzPrefixSnapshot {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let core = lock(&client.core);
    let bindings = core.prefix_bindings().to_vec();
    let summaries = bindings.iter().map(prefix_command_summary).collect();
    Box::into_raw(Box::new(ZzPrefixSnapshot {
        armed: core.prefix_armed(),
        bindings,
        summaries,
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_prefix_snapshot_release(snapshot: *mut ZzPrefixSnapshot) {
    if !snapshot.is_null() {
        drop(unsafe { Box::from_raw(snapshot) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_prefix_snapshot_armed(snapshot: *const ZzPrefixSnapshot) -> bool {
    unsafe { snapshot.as_ref() }.is_some_and(|snapshot| snapshot.armed)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_prefix_binding_count(snapshot: *const ZzPrefixSnapshot) -> usize {
    unsafe { snapshot.as_ref() }.map_or(0, |snapshot| snapshot.bindings.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_prefix_binding_key(
    snapshot: *const ZzPrefixSnapshot,
    binding: usize,
) -> ZzBytes {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| prefix_binding_at(snapshot, binding))
        .map_or(ZzBytes::EMPTY, |binding| ZzBytes::new(&binding.key))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_prefix_binding_repeat(
    snapshot: *const ZzPrefixSnapshot,
    binding: usize,
) -> bool {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| prefix_binding_at(snapshot, binding))
        .is_some_and(|binding| binding.repeat)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_prefix_binding_note(
    snapshot: *const ZzPrefixSnapshot,
    binding: usize,
) -> ZzBytes {
    unsafe { snapshot.as_ref() }
        .and_then(|snapshot| prefix_binding_at(snapshot, binding))
        .and_then(|binding| binding.note.as_deref())
        .map_or(ZzBytes::EMPTY, ZzBytes::new)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_prefix_binding_summary(
    snapshot: *const ZzPrefixSnapshot,
    binding: usize,
) -> ZzBytes {
    let Some(snapshot) = (unsafe { snapshot.as_ref() }) else {
        return ZzBytes::EMPTY;
    };
    snapshot
        .summaries
        .get(binding)
        .map_or(ZzBytes::EMPTY, |summary| ZzBytes::new(summary))
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_state_acquire(
    client: *const ZzClient,
    pane: u64,
) -> *mut ZzAgentState {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    lock(&client.core)
        .agent_state(PaneId(pane))
        .cloned()
        .map_or(std::ptr::null_mut(), |state| {
            Box::into_raw(Box::new(agent_state(state)))
        })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_state_release(state: *mut ZzAgentState) {
    if !state.is_null() {
        drop(unsafe { Box::from_raw(state) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_state_phase(state: *const ZzAgentState) -> ZzAgentPhase {
    let Some(state) = (unsafe { state.as_ref() }) else {
        return ZzAgentPhase::Starting;
    };
    match &state.wire.phase {
        AgentConnectionPhase::Starting => ZzAgentPhase::Starting,
        AgentConnectionPhase::Ready => ZzAgentPhase::Ready,
        AgentConnectionPhase::Running => ZzAgentPhase::Running,
        AgentConnectionPhase::AwaitingPermission => ZzAgentPhase::AwaitingPermission,
        AgentConnectionPhase::Failed { .. } => ZzAgentPhase::Failed,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_attention_status(state: *const ZzAgentState) -> ZzAgentAttention {
    let Some(state) = (unsafe { state.as_ref() }) else {
        return ZzAgentAttention::Idle;
    };
    match agent_attention_status(&state.wire) {
        AgentAttentionStatus::Idle => ZzAgentAttention::Idle,
        AgentAttentionStatus::Working => ZzAgentAttention::Working,
        AgentAttentionStatus::NeedsInput => ZzAgentAttention::NeedsInput,
        AgentAttentionStatus::Failed => ZzAgentAttention::Failed,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_queued_prompts(state: *const ZzAgentState) -> u32 {
    unsafe { state.as_ref() }.map_or(0, |state| state.wire.queued_prompts)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_session_id(state: *const ZzAgentState) -> ZzBytes {
    unsafe { state.as_ref() }
        .and_then(|state| state.wire.session_id.as_deref())
        .map_or(ZzBytes::EMPTY, ZzBytes::new)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_title(state: *const ZzAgentState) -> ZzBytes {
    unsafe { state.as_ref() }
        .and_then(|state| state.wire.title.as_deref())
        .map_or(ZzBytes::EMPTY, ZzBytes::new)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_error(state: *const ZzAgentState) -> ZzBytes {
    let Some(state) = (unsafe { state.as_ref() }) else {
        return ZzBytes::EMPTY;
    };
    state
        .wire
        .error
        .as_deref()
        .or(match &state.wire.phase {
            AgentConnectionPhase::Failed { message } => Some(message.as_str()),
            _ => None,
        })
        .map_or(ZzBytes::EMPTY, ZzBytes::new)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_has_permission(state: *const ZzAgentState) -> bool {
    unsafe { state.as_ref() }.is_some_and(|state| state.wire.pending_permission.is_some())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_permission_request_id(state: *const ZzAgentState) -> u64 {
    unsafe { state.as_ref() }
        .and_then(|state| state.wire.pending_permission.as_ref())
        .map_or(0, |permission| permission.request_id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_permission_payload(state: *const ZzAgentState) -> ZzBytes {
    unsafe { state.as_ref() }
        .and_then(|state| state.wire.pending_permission.as_ref())
        .map_or(ZzBytes::EMPTY, |permission| {
            ZzBytes::new(&permission.payload)
        })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_permission_title(state: *const ZzAgentState) -> ZzBytes {
    unsafe { state.as_ref() }
        .and_then(|state| state.permission.as_ref())
        .map_or(ZzBytes::EMPTY, |permission| ZzBytes::new(&permission.title))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_permission_option_count(state: *const ZzAgentState) -> usize {
    unsafe { state.as_ref() }
        .and_then(|state| state.permission.as_ref())
        .map_or(0, |permission| permission.options.len())
}

fn permission_option(state: &ZzAgentState, option: usize) -> Option<&ZzAgentPermissionOption> {
    state.permission.as_ref()?.options.get(option)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_permission_option_id(
    state: *const ZzAgentState,
    option: usize,
) -> ZzBytes {
    unsafe { state.as_ref() }
        .and_then(|state| permission_option(state, option))
        .map_or(ZzBytes::EMPTY, |option| ZzBytes::new(&option.id))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_permission_option_name(
    state: *const ZzAgentState,
    option: usize,
) -> ZzBytes {
    unsafe { state.as_ref() }
        .and_then(|state| permission_option(state, option))
        .map_or(ZzBytes::EMPTY, |option| ZzBytes::new(&option.name))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_permission_option_kind(
    state: *const ZzAgentState,
    option: usize,
) -> ZzAgentPermissionKind {
    unsafe { state.as_ref() }
        .and_then(|state| permission_option(state, option))
        .map_or(ZzAgentPermissionKind::Unknown, |option| option.kind)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_has_git(state: *const ZzAgentState) -> bool {
    unsafe { state.as_ref() }.is_some_and(|state| state.wire.git.is_some())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_git_branch(state: *const ZzAgentState) -> ZzBytes {
    unsafe { state.as_ref() }
        .and_then(|state| state.wire.git.as_ref())
        .and_then(|git| git.branch.as_deref())
        .map_or(ZzBytes::EMPTY, ZzBytes::new)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_git_changed_files(state: *const ZzAgentState) -> u32 {
    unsafe { state.as_ref() }
        .and_then(|state| state.wire.git.as_ref())
        .map_or(0, |git| git.changed_files)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_git_additions(state: *const ZzAgentState) -> u32 {
    unsafe { state.as_ref() }
        .and_then(|state| state.wire.git.as_ref())
        .map_or(0, |git| git.additions)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_git_deletions(state: *const ZzAgentState) -> u32 {
    unsafe { state.as_ref() }
        .and_then(|state| state.wire.git.as_ref())
        .map_or(0, |git| git.deletions)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_respond_permission(
    client: *mut ZzClient,
    pane: u64,
    request_id: u64,
    option_id: *const c_char,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    let option_id = if option_id.is_null() {
        None
    } else {
        let Ok(option_id) = unsafe { CStr::from_ptr(option_id) }.to_str() else {
            return false;
        };
        Some(option_id.to_owned())
    };
    client
        .client
        .agent_respond_permission(PaneId(pane), request_id, option_id)
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_cancel(client: *mut ZzClient, pane: u64) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    client.client.agent_cancel(PaneId(pane)).is_ok()
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
    let Some(event) = lock(&client.queues.events).pop_front() else {
        return false;
    };
    unsafe { out.write(event) };
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_clipboard_next(client: *mut ZzClient) -> *mut ZzClipboard {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    lock(&client.queues.clipboards)
        .pop_front()
        .map_or(std::ptr::null_mut(), |clipboard| {
            Box::into_raw(Box::new(clipboard))
        })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_clipboard_release(clipboard: *mut ZzClipboard) {
    if !clipboard.is_null() {
        drop(unsafe { Box::from_raw(clipboard) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_clipboard_pane(clipboard: *const ZzClipboard) -> u64 {
    unsafe { clipboard.as_ref() }.map_or(0, |clipboard| clipboard.pane)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_clipboard_request_id(clipboard: *const ZzClipboard) -> u64 {
    unsafe { clipboard.as_ref() }.map_or(0, |clipboard| clipboard.request_id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_clipboard_text(clipboard: *const ZzClipboard) -> ZzBytes {
    unsafe { clipboard.as_ref() }.map_or(ZzBytes::EMPTY, |clipboard| ZzBytes::new(&clipboard.text))
}

/// Pop the oldest queued command reply, or null when none is queued. Replies
/// arrive in the order the daemon answers, one per executed command. The
/// reply is caller-owned: read it with the `zz_command_reply_*` accessors,
/// then free it with [`zz_command_reply_release`].
///
/// # Safety
///
/// `client` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_command_reply_next(
    client: *mut ZzClient,
) -> *mut ZzCommandReply {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    lock(&client.queues.command_replies)
        .pop_front()
        .map_or(std::ptr::null_mut(), |reply| Box::into_raw(Box::new(reply)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_command_reply_release(reply: *mut ZzCommandReply) {
    if !reply.is_null() {
        drop(unsafe { Box::from_raw(reply) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_command_reply_request_id(reply: *const ZzCommandReply) -> u64 {
    unsafe { reply.as_ref() }.map_or(0, |reply| reply.request_id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_command_reply_ok(reply: *const ZzCommandReply) -> bool {
    unsafe { reply.as_ref() }.is_some_and(|reply| reply.ok)
}

/// The command's exit code: zero on success, and one for a rejected command,
/// which the wire reports as an error rather than an exit status.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_command_reply_exit_code(reply: *const ZzCommandReply) -> u8 {
    unsafe { reply.as_ref() }.map_or(0, |reply| reply.exit_code)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_command_reply_output(reply: *const ZzCommandReply) -> ZzBytes {
    unsafe { reply.as_ref() }.map_or(ZzBytes::EMPTY, |reply| ZzBytes::new(&reply.output))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_command_reply_error(reply: *const ZzCommandReply) -> ZzBytes {
    unsafe { reply.as_ref() }.map_or(ZzBytes::EMPTY, |reply| ZzBytes::new(&reply.error))
}

/// Pop the oldest queued agent transcript batch, or null when none is queued.
/// Batches arrive in journal order per pane; each item is one daemon JSON
/// stream item and `zz_agent_updates_first_seq` numbers the first one. The
/// batch is caller-owned: read it with the `zz_agent_updates_*` accessors,
/// then free it with [`zz_agent_updates_release`].
///
/// # Safety
///
/// `client` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_updates_next(client: *mut ZzClient) -> *mut ZzAgentBatch {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    lock(&client.queues.agent_batches)
        .pop_front()
        .map_or(std::ptr::null_mut(), |batch| Box::into_raw(Box::new(batch)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_updates_release(updates: *mut ZzAgentBatch) {
    if !updates.is_null() {
        drop(unsafe { Box::from_raw(updates) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_updates_pane(updates: *const ZzAgentBatch) -> u64 {
    unsafe { updates.as_ref() }.map_or(0, |updates| updates.pane)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_updates_first_seq(updates: *const ZzAgentBatch) -> u64 {
    unsafe { updates.as_ref() }.map_or(0, |updates| updates.first_seq)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_updates_item_count(updates: *const ZzAgentBatch) -> usize {
    unsafe { updates.as_ref() }.map_or(0, |updates| updates.items.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_updates_item_bytes(
    updates: *const ZzAgentBatch,
    index: usize,
) -> ZzBytes {
    unsafe { updates.as_ref() }.map_or(ZzBytes::EMPTY, |updates| {
        updates
            .items
            .get(index)
            .map_or(ZzBytes::EMPTY, |item| ZzBytes::from_bytes(item))
    })
}

/// Pop the oldest queued agent-lane overflow notice. The daemon cleared the
/// pane's lane from `next_seq_out`; answer with [`zz_client_agent_replay`]
/// from the shell's cursor.
///
/// # Safety
///
/// `client` must be a live handle; both out pointers must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_lagged_next(
    client: *mut ZzClient,
    pane_out: *mut u64,
    next_seq_out: *mut u64,
) -> bool {
    let (Some(client), false) = (
        unsafe { client.as_ref() },
        pane_out.is_null() || next_seq_out.is_null(),
    ) else {
        return false;
    };
    let Some(lagged) = lock(&client.queues.agent_lagged).pop_front() else {
        return false;
    };
    unsafe {
        pane_out.write(lagged.pane);
        next_seq_out.write(lagged.next_seq);
    }
    true
}

/// Ask the daemon to replay a pane's agent stream from `from_seq`,
/// inclusively, then tail it. Send on a journal gap, a lane overflow, and
/// when a pane's view goes live without a cursor.
///
/// # Safety
///
/// `client` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_replay(
    client: *mut ZzClient,
    pane: u64,
    from_seq: u64,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    client.client.agent_replay(PaneId(pane), from_seq).is_ok()
}

unsafe fn c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Pop the oldest queued agent session-list reply, or null when none is
/// queued. Read it with the `zz_agent_sessions_*` accessors, then free it
/// with [`zz_agent_sessions_release`].
///
/// # Safety
///
/// `client` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_sessions_next(
    client: *mut ZzClient,
) -> *mut ZzAgentSessionsReply {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    lock(&client.queues.agent_sessions)
        .pop_front()
        .map_or(std::ptr::null_mut(), |reply| Box::into_raw(Box::new(reply)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_sessions_release(reply: *mut ZzAgentSessionsReply) {
    if !reply.is_null() {
        drop(unsafe { Box::from_raw(reply) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_sessions_pane(reply: *const ZzAgentSessionsReply) -> u64 {
    unsafe { reply.as_ref() }.map_or(0, |reply| reply.pane)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_sessions_request_id(reply: *const ZzAgentSessionsReply) -> u64 {
    unsafe { reply.as_ref() }.map_or(0, |reply| reply.request_id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_sessions_result(reply: *const ZzAgentSessionsReply) -> ZzBytes {
    unsafe { reply.as_ref() }.map_or(ZzBytes::EMPTY, |reply| ZzBytes::new(&reply.result))
}

/// The pane's raw session-config JSON blob: an array of ACP
/// `SessionConfigOption` values (`model`, `thoughtLevel`, and `mode`
/// categories drive the pickers). Empty when the adapter published none.
///
/// # Safety
///
/// `state` must be a live handle; the bytes are borrowed from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_config_options(state: *const ZzAgentState) -> ZzBytes {
    unsafe { state.as_ref() }.map_or(ZzBytes::EMPTY, |state| {
        ZzBytes::new(&state.wire.config_options)
    })
}

/// The pane's raw legacy session-mode JSON blob (`SessionModeState`), used
/// only when the adapter publishes no config options. Empty when absent.
///
/// # Safety
///
/// `state` must be a live handle; the bytes are borrowed from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_modes(state: *const ZzAgentState) -> ZzBytes {
    unsafe { state.as_ref() }.map_or(ZzBytes::EMPTY, |state| ZzBytes::new(&state.wire.modes))
}

/// Set one session config option (model, effort, permission mode) by id.
///
/// # Safety
///
/// `client`, `option_id`, and `value` must be live; the strings must be UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_set_config_option(
    client: *mut ZzClient,
    pane: u64,
    option_id: *const c_char,
    value: *const c_char,
) -> bool {
    let (Some(client), Some(option_id), Some(value)) = (
        unsafe { client.as_ref() },
        unsafe { c_string(option_id) },
        unsafe { c_string(value) },
    ) else {
        return false;
    };
    client
        .client
        .agent_set_config_option(PaneId(pane), option_id, value)
        .is_ok()
}

/// Set the pane's legacy session mode by id. Adapters with config options
/// ignore this; it exists for adapters that only publish `modes`.
///
/// # Safety
///
/// `client` and `mode_id` must be live; `mode_id` must be UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_set_mode(
    client: *mut ZzClient,
    pane: u64,
    mode_id: *const c_char,
) -> bool {
    let (Some(client), Some(mode_id)) = (unsafe { client.as_ref() }, unsafe { c_string(mode_id) })
    else {
        return false;
    };
    client.client.agent_set_mode(PaneId(pane), mode_id).is_ok()
}

/// Ask the daemon to list the pane's agent sessions across every project.
/// The answer arrives as `ZZ_EVENT_AGENT_SESSIONS`.
///
/// # Safety
///
/// `client` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_list_sessions(client: *mut ZzClient, pane: u64) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    client
        .client
        .agent_session_op(
            PaneId(pane),
            AgentSessionOpKind::List {
                cwd: None,
                cursor: None,
                replace: true,
            },
        )
        .is_ok()
}

/// Start a new agent session in the pane with `cwd` as its working
/// directory. The path must be absolute.
///
/// # Safety
///
/// `client` and `cwd` must be live; `cwd` must be UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_new_session(
    client: *mut ZzClient,
    pane: u64,
    cwd: *const c_char,
) -> bool {
    let (Some(client), Some(cwd)) = (unsafe { client.as_ref() }, unsafe { c_string(cwd) }) else {
        return false;
    };
    client
        .client
        .agent_session_op(
            PaneId(pane),
            AgentSessionOpKind::New {
                cwd: PathBuf::from(cwd),
            },
        )
        .is_ok()
}

/// Switch the pane to a listed agent session. `additional_directories_json`
/// is a JSON array of absolute paths and may be null for none.
///
/// # Safety
///
/// `client`, `session_id`, and `cwd` must be live and UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_switch_session(
    client: *mut ZzClient,
    pane: u64,
    session_id: *const c_char,
    cwd: *const c_char,
    additional_directories_json: *const c_char,
) -> bool {
    let (Some(client), Some(session_id), Some(cwd)) = (
        unsafe { client.as_ref() },
        unsafe { c_string(session_id) },
        unsafe { c_string(cwd) },
    ) else {
        return false;
    };
    let encoded = unsafe { c_string(additional_directories_json) }.unwrap_or_default();
    let additional_directories = if encoded.is_empty() {
        Vec::new()
    } else {
        let Ok(directories) = serde_json::from_str::<Vec<String>>(&encoded) else {
            return false;
        };
        directories.into_iter().map(PathBuf::from).collect()
    };
    client
        .client
        .agent_session_op(
            PaneId(pane),
            AgentSessionOpKind::Switch {
                session_id,
                cwd: PathBuf::from(cwd),
                additional_directories,
            },
        )
        .is_ok()
}

/// Delete a listed agent session by id.
///
/// # Safety
///
/// `client` and `session_id` must be live; `session_id` must be UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_agent_delete_session(
    client: *mut ZzClient,
    pane: u64,
    session_id: *const c_char,
) -> bool {
    let (Some(client), Some(session_id)) =
        (unsafe { client.as_ref() }, unsafe { c_string(session_id) })
    else {
        return false;
    };
    client
        .client
        .agent_session_op(PaneId(pane), AgentSessionOpKind::Delete { session_id })
        .is_ok()
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
    use std::{collections::BTreeMap, sync::Arc};

    use zz_protocol::{Axis, LayoutNode, SplitId, WindowId};
    use zz_terminal::{GRAPHEME_TABLE_BIT, PackedCell, SessionStatus, TerminalDictionary};

    use super::*;

    fn decode(viewport: &TerminalViewport, capacity: usize) -> Vec<u8> {
        let viewport = ZzViewport(viewport.clone());
        let mut output: Vec<c_char> = vec![0; capacity];
        let length =
            unsafe { zz_viewport_row_text(&viewport, 0, output.as_mut_ptr(), output.len()) };
        output[..length]
            .iter()
            .map(|byte| byte.to_ne_bytes()[0])
            .collect()
    }

    fn terminal_pane(id: u64) -> PaneSnapshot {
        PaneSnapshot {
            id: PaneId(id),
            title: String::new(),
            kind: PaneKindSnapshot::Terminal,
            synchronized_input: false,
            bell: false,
            dead: false,
            dead_status: None,
            border_colour: None,
            active_border_colour: None,
            border_status_text: String::new(),
        }
    }

    #[test]
    fn zoomed_window_only_exposes_the_zoomed_pane_rect() {
        let first = terminal_pane(1);
        let second = terminal_pane(2);
        let window = WindowSnapshot {
            id: WindowId(1),
            index: 0,
            name: String::new(),
            automatic_rename: false,
            active_pane: second.id,
            zoomed_pane: Some(second.id),
            layout: LayoutNode::Split {
                id: SplitId(1),
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(first.id)),
                second: Box::new(LayoutNode::Pane(second.id)),
            },
            panes: BTreeMap::from([(first.id, first), (second.id, second)]),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
            status_label: String::new(),
            activity: false,
            pane_border_status: zz_protocol::PaneBorderStatus::Off,
            pane_border_lines: zz_protocol::PaneBorderLines::Single,
            pane_border_indicators: zz_protocol::PaneBorderIndicators::Colour,
            pane_order: Vec::new(),
            pane_z_order: Vec::new(),
        };

        assert_eq!(window_pane_rect(&window, PaneId(1)), None);
        assert_eq!(
            window_pane_rect(&window, PaneId(2)),
            Some(NormalizedPaneRect::FULL)
        );

        let snapshot = ZzMuxSnapshot {
            snapshot: Arc::new(MuxSnapshot {
                generation: 1,
                sessions: vec![SessionSnapshot {
                    id: SessionId(1),
                    name: String::new(),
                    active_window: window.id,
                    windows: vec![window],
                    viewers: Vec::new(),
                }],
                focused_window: None,
            }),
            attached: Some(SessionId(1)),
        };
        let mut rect = ZzPaneRect::default();

        assert_eq!(
            unsafe { zz_snapshot_session_window_pane_count(&snapshot, 0, 0) },
            2
        );
        let mut zoomed = 0;
        assert!(unsafe { zz_snapshot_session_window_zoomed_pane(&snapshot, 0, 0, &mut zoomed) });
        assert_eq!(zoomed, 2);
        assert!(!unsafe { zz_snapshot_session_window_pane_rect(&snapshot, 0, 0, 0, &mut rect) });
        assert!(unsafe { zz_snapshot_session_window_pane_rect(&snapshot, 0, 0, 1, &mut rect) });
        assert_eq!(rect, ZzPaneRect::from(NormalizedPaneRect::FULL));
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

    #[test]
    fn agent_permission_payload_becomes_typed_c_state() {
        let state = agent_state(AgentPaneWire {
            phase: AgentConnectionPhase::AwaitingPermission,
            pending_permission: Some(zz_protocol::AgentPermissionWire {
                request_id: 41,
                payload: serde_json::json!({
                    "toolCall": {
                        "toolCallId": "tool-1",
                        "title": "Run cargo test"
                    },
                    "options": [
                        {
                            "optionId": "allow-once",
                            "name": "Allow once",
                            "kind": "allow_once"
                        },
                        {
                            "optionId": "reject-once",
                            "name": "Reject",
                            "kind": "reject_once"
                        }
                    ]
                })
                .to_string(),
            }),
            ..AgentPaneWire::default()
        });

        assert_eq!(
            state
                .permission
                .as_ref()
                .map(|permission| permission.title.as_str()),
            Some("Run cargo test")
        );
        assert_eq!(state.permission.as_ref().unwrap().options.len(), 2);
        assert_eq!(
            state.permission.as_ref().unwrap().options[0].kind,
            ZzAgentPermissionKind::AllowOnce
        );
    }

    #[test]
    fn attention_edges_keep_their_ffi_flags() {
        let queues = EventQueues::default();
        queue_event(
            &queues,
            &CoreEvent::AgentStateChanged {
                pane: PaneId(9),
                attention: Some(AgentAttentionEdge::Request),
            },
        );

        assert_eq!(
            lock(&queues.events).pop_front(),
            Some(ZzEvent {
                kind: ZzEventKind::AgentStateChanged,
                flags: EVENT_AGENT_REQUEST,
                pane: 9,
                row_start: 0,
                row_end: 0,
            })
        );
    }

    #[test]
    fn agent_update_batches_keep_order_and_their_event_kind() {
        let queues = EventQueues::default();
        for (pane, first_seq) in [(4_u64, 1_u64), (4_u64, 3_u64)] {
            queue_event(
                &queues,
                &CoreEvent::AgentUpdates {
                    pane: PaneId(pane),
                    first_seq,
                    items: vec![vec![7_u8, 8_u8]],
                },
            );
        }

        let kinds: Vec<ZzEvent> = std::iter::from_fn(|| lock(&queues.events).pop_front()).collect();
        assert_eq!(
            kinds
                .iter()
                .map(|event| (event.kind, event.pane))
                .collect::<Vec<_>>(),
            vec![
                (ZzEventKind::AgentUpdates, 4),
                (ZzEventKind::AgentUpdates, 4),
            ]
        );

        let batches = lock(&queues.agent_batches);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].pane, 4);
        assert_eq!(batches[0].first_seq, 1);
        assert_eq!(batches[0].items, vec![vec![7_u8, 8_u8]]);
        assert_eq!(batches[1].first_seq, 3);
    }

    #[test]
    fn agent_lag_notices_keep_their_pane_and_sequence() {
        let queues = EventQueues::default();
        queue_event(
            &queues,
            &CoreEvent::AgentLagged {
                pane: PaneId(6),
                next_seq: 41,
            },
        );

        assert_eq!(
            lock(&queues.events).pop_front(),
            Some(ZzEvent {
                kind: ZzEventKind::AgentLagged,
                flags: 0,
                pane: 6,
                row_start: 0,
                row_end: 0,
            })
        );
        let lagged = lock(&queues.agent_lagged);
        assert_eq!(lagged.len(), 1);
        assert_eq!(lagged[0].pane, 6);
        assert_eq!(lagged[0].next_seq, 41);
    }

    #[test]
    fn agent_session_replies_keep_pane_request_and_result() {
        let queues = EventQueues::default();
        queue_event(
            &queues,
            &CoreEvent::AgentSessions {
                pane: PaneId(5),
                request_id: 7,
                result: "{\"item\":\"sessionsListed\"}".to_owned(),
            },
        );

        assert_eq!(
            lock(&queues.events).pop_front(),
            Some(ZzEvent {
                kind: ZzEventKind::AgentSessions,
                flags: 0,
                pane: 5,
                row_start: 0,
                row_end: 0,
            })
        );
        let replies = lock(&queues.agent_sessions);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].pane, 5);
        assert_eq!(replies[0].request_id, 7);
        assert_eq!(replies[0].result, "{\"item\":\"sessionsListed\"}");
    }

    fn prefix_binding(
        key: &str,
        name: &str,
        args: &[&str],
        repeat: bool,
        note: Option<&str>,
    ) -> KeyBindingSnapshot {
        KeyBindingSnapshot {
            key: key.to_owned(),
            commands: vec![CommandInvocation::new(name, args.iter().copied())],
            repeat,
            note: note.map(str::to_owned),
        }
    }

    fn zz_str(bytes: ZzBytes) -> String {
        assert!(bytes.len == 0 || !bytes.ptr.is_null());
        if bytes.len == 0 {
            return String::new();
        }
        String::from_utf8(unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len).to_vec() })
            .unwrap()
    }

    #[test]
    fn prefix_command_summary_joins_first_command_and_args() {
        let bound = prefix_binding("c", "new-window", &["-n", "work"], false, None);
        assert_eq!(prefix_command_summary(&bound), "new-window -n work");
        let bare = prefix_binding("x", "kill-pane", &[], false, None);
        assert_eq!(prefix_command_summary(&bare), "kill-pane");
        let unbound = KeyBindingSnapshot {
            key: "y".to_owned(),
            commands: Vec::new(),
            repeat: false,
            note: None,
        };
        assert_eq!(prefix_command_summary(&unbound), "");
    }

    #[test]
    fn prefix_snapshot_reads_are_null_safe_and_bounded() {
        assert!(unsafe { zz_prefix_snapshot_acquire(std::ptr::null()) }.is_null());
        assert!(!unsafe { zz_prefix_snapshot_armed(std::ptr::null()) });
        assert_eq!(unsafe { zz_prefix_binding_count(std::ptr::null()) }, 0);
        assert_eq!(unsafe { zz_prefix_binding_key(std::ptr::null(), 0) }.len, 0);
        assert!(!unsafe { zz_prefix_binding_repeat(std::ptr::null(), 0) });
        assert_eq!(
            unsafe { zz_prefix_binding_note(std::ptr::null(), 0) }.len,
            0
        );
        assert_eq!(
            unsafe { zz_prefix_binding_summary(std::ptr::null(), 0) }.len,
            0
        );
        unsafe { zz_prefix_snapshot_release(std::ptr::null_mut()) };
    }

    #[test]
    fn prefix_snapshot_exposes_key_note_repeat_and_summary() {
        let snapshot = Box::into_raw(Box::new(ZzPrefixSnapshot {
            armed: true,
            bindings: vec![
                prefix_binding("c", "new-window", &[], false, None),
                prefix_binding("%", "split-window", &["-h"], true, Some("Split vertically")),
            ],
            summaries: vec!["new-window".to_owned(), "split-window -h".to_owned()],
        }));
        assert!(unsafe { zz_prefix_snapshot_armed(snapshot) });
        assert_eq!(unsafe { zz_prefix_binding_count(snapshot) }, 2);
        assert_eq!(unsafe { zz_str(zz_prefix_binding_key(snapshot, 0)) }, "c");
        assert_eq!(
            unsafe { zz_str(zz_prefix_binding_summary(snapshot, 1)) },
            "split-window -h"
        );
        assert!(unsafe { zz_prefix_binding_repeat(snapshot, 1) });
        assert!(!unsafe { zz_prefix_binding_repeat(snapshot, 0) });
        assert_eq!(
            unsafe { zz_str(zz_prefix_binding_note(snapshot, 1)) },
            "Split vertically"
        );
        assert_eq!(unsafe { zz_prefix_binding_key(snapshot, 9) }.len, 0);
        assert_eq!(unsafe { zz_prefix_binding_summary(snapshot, 9) }.len, 0);
        unsafe { zz_prefix_snapshot_release(snapshot) };
    }

    #[test]
    fn command_replies_carry_output_and_error_text_to_the_abi() {
        let queues = EventQueues::default();
        queue_event(
            &queues,
            &CoreEvent::CommandResponse(CommandResponse::Success {
                request_id: 3,
                output: "%1: last output".into(),
                exit_code: 0,
                stderr: String::new(),
            }),
        );
        queue_event(
            &queues,
            &CoreEvent::CommandResponse(CommandResponse::Error {
                request_id: 4,
                error: zz_protocol::ServerError::MissingTarget("current pane".to_owned()),
                output: Default::default(),
            }),
        );

        let queued: Vec<ZzEvent> =
            std::iter::from_fn(|| lock(&queues.events).pop_front()).collect();
        assert_eq!(
            queued.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![ZzEventKind::CommandReply, ZzEventKind::CommandReply]
        );

        let replies = lock(&queues.command_replies);
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0].request_id, 3);
        assert!(replies[0].ok);
        assert_eq!(replies[0].output, "%1: last output");
        assert!(replies[0].error.is_empty());
        assert_eq!(replies[1].request_id, 4);
        assert!(!replies[1].ok);
        assert_eq!(replies[1].exit_code, 1);
        assert_eq!(replies[1].error, "target not found: current pane");
    }

    #[test]
    fn unread_command_replies_drop_the_oldest_at_the_cap() {
        let queues = EventQueues::default();
        for request_id in 0..u64::try_from(MAX_QUEUED_COMMAND_REPLIES).unwrap() + 2 {
            queue_event(
                &queues,
                &CoreEvent::CommandResponse(CommandResponse::Success {
                    request_id,
                    output: Default::default(),
                    exit_code: 0,
                    stderr: String::new(),
                }),
            );
        }

        let replies = lock(&queues.command_replies);
        assert_eq!(replies.len(), MAX_QUEUED_COMMAND_REPLIES);
        assert_eq!(replies[0].request_id, 2);
    }

    #[test]
    fn command_reply_reads_are_null_safe_and_release_owns_the_handle() {
        assert_eq!(unsafe { zz_command_reply_request_id(std::ptr::null()) }, 0);
        assert!(!unsafe { zz_command_reply_ok(std::ptr::null()) });
        assert_eq!(unsafe { zz_command_reply_exit_code(std::ptr::null()) }, 0);
        assert_eq!(unsafe { zz_command_reply_output(std::ptr::null()) }.len, 0);
        assert_eq!(unsafe { zz_command_reply_error(std::ptr::null()) }.len, 0);
        unsafe { zz_command_reply_release(std::ptr::null_mut()) };

        let reply = Box::into_raw(Box::new(ZzCommandReply::new(&CommandResponse::Success {
            request_id: 11,
            output: "ok".into(),
            exit_code: 0,
            stderr: String::new(),
        })));
        assert_eq!(unsafe { zz_command_reply_request_id(reply) }, 11);
        assert!(unsafe { zz_command_reply_ok(reply) });
        assert_eq!(unsafe { zz_str(zz_command_reply_output(reply)) }, "ok");
        assert_eq!(unsafe { zz_command_reply_error(reply) }.len, 0);
        unsafe { zz_command_reply_release(reply) };
    }

    #[test]
    fn overlay_core_events_reach_their_own_ffi_kinds() {
        let queues = EventQueues::default();
        for event in [
            CoreEvent::PrefixArmed { armed: true },
            CoreEvent::PrefixArmed { armed: false },
            CoreEvent::KeyTablesChanged,
            CoreEvent::CommandPromptChanged,
            CoreEvent::ChooseBufferChanged,
            CoreEvent::DisplayPanesChanged,
        ] {
            queue_event(&queues, &event);
        }
        let queued: Vec<ZzEvent> =
            std::iter::from_fn(|| lock(&queues.events).pop_front()).collect();
        assert_eq!(
            queued
                .iter()
                .map(|event| (event.kind, event.flags))
                .collect::<Vec<_>>(),
            vec![
                (ZzEventKind::PrefixArmed, 1),
                (ZzEventKind::PrefixArmed, 0),
                (ZzEventKind::KeyTablesChanged, 0),
                (ZzEventKind::CommandPromptChanged, 0),
                (ZzEventKind::ChooseBufferChanged, 0),
                (ZzEventKind::DisplayPanesChanged, 0),
            ]
        );
    }
}
