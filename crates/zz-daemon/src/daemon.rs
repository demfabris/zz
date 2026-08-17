use std::{
    borrow::Cow,
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::OsStr,
    fmt::Write as _,
    fs,
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use parking_lot::{Condvar, Mutex};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use zz_mux::{
    DEFAULT_BUFFER_LIMIT, DetachScope, Execution, ExecutionContext, KeyDecision, KeyEngine,
    KeyTables, MuxEffect, MuxEngine, MuxState, PaneKind, PaneRuntimeFacts, canonical_command,
    expand_format_values, parse_config,
};
use zz_protocol::{
    AgentCommand, BrowserCommand, ChooseBufferAction, ChooseBufferItem, ChooseBufferSearchState,
    ChooseBufferState, ChooseTreeAction, ChooseTreeItem, ChooseTreeKind, ChooseTreePaneKind,
    ChooseTreeSearchState, ChooseTreeState, ChooseTreeTarget, ClientHello, ClientId,
    ClientInstanceId, ClientKind, ClientMessageKind, CommandInvocation, CommandPromptAction,
    CommandPromptKind, CommandPromptState, CommandRequest, CommandResponse, ConfigOverrideEntry,
    DisplayPanesAction, DisplayPanesState, Event, EventPayload, GuiResponse, InputMessage,
    MAX_AGENT_SEND_BYTES, MAX_CHOOSE_BUFFER_QUERY_BYTES, MAX_CHOOSE_TREE_QUERY_BYTES, MuxOptionKey,
    MuxOptionSource, MuxOptions, MuxSnapshot, NEW_SESSION_ATTACH_CAPABILITY, PROTOCOL_VERSION,
    PaneId, PaneIndicator, PaneKindSnapshot, PasteUploadPurpose, PastedImageFormat, ProtocolError,
    ProtocolMessage, SPLIT_RATIO_BASIS, ServerError, ServerHello, SessionId, SessionViewer,
    SplitId, StatusLine, WindowId, encode_protocol_message_into,
    encode_terminal_viewport_event_into, read_protocol_message_into,
};
use zz_terminal::{
    AppearanceConfigDisposition, AppearanceLoad, AppearanceProvenance, CaptureBoundary,
    CaptureOptions, ClipboardTarget, LastCommandCapture, PasteBufferAction, TerminalAppearance,
    TerminalCaptureError, TerminalColorScheme, TerminalDiffScratch, TerminalEvent, TerminalMode,
    TerminalSession, TerminalSpawn, TerminalViewId, TerminalViewport, WordSeparators,
    apply_appearance_overrides, prepare_paste_buffer,
};

#[cfg(feature = "agent")]
use zz_protocol::{
    AgentPaneWire, AgentProvider, AgentSessionOpKind, MAX_AGENT_SESSION_DIRECTORIES,
    MAX_AGENT_UPDATES_BYTES, MAX_GUI_TEXT_BYTES,
};

#[cfg(feature = "agent")]
use crate::agent::{
    environment::AgentWorkspaceEnvironment,
    fanout::{AgentPublisher, AgentRequestReply, AgentRuntime, is_default_agent_title},
    host::{AgentPaneSpec, HostCommand},
    runtime::{AgentSpawnConfig, load_persistent_journal},
    stream::{AgentImage, AgentPrompt, AgentSessionSummary},
};
use crate::{
    DaemonError, diagnostic_elapsed_us, diagnostic_timer,
    keys::{choose_buffer_key_action, choose_tree_key_action, input_key_name, send_tokens},
    lifecycle::DaemonIdentityGuard,
    paths::{default_mux_config, home_directory, is_default_mux_config},
    shell_process,
    status::{
        BufferFormatFacts, ClientFormatFacts, DaemonFormatHooks, FormatHookFacts,
        MessageFormatFacts, StatusRenderer, StatusRequest, host_names, status_context,
    },
    transport::{LocalTransport, Transport, TransportListener, TransportStream},
};

#[cfg(unix)]
const ACCEPT_WAIT_TIMEOUT: Duration = Duration::from_millis(100);
#[cfg(windows)]
const ACCEPT_WAIT_TIMEOUT: Duration = Duration::from_millis(20);
const DIAGNOSTIC_STATE_INTERVAL: Duration = Duration::from_secs(5);
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_CONFIG_DEPTH: usize = 16;
const MAX_RELIABLE_MESSAGES: usize = 256;
const MAX_KITTY_IMAGE_CHUNK_BYTES: usize = 1024 * 1024;
const MAX_PENDING_TERMINALS: usize = 128;
/// What one pane's agent lane may hold before it is cleared in favour of a
/// lag marker. A slow client degrades to replay instead of being closed.
#[cfg(feature = "agent")]
const MAX_PENDING_AGENT_BYTES: usize = MAX_AGENT_UPDATES_BYTES + MAX_GUI_TEXT_BYTES;
#[cfg(feature = "agent")]
const MAX_PENDING_AGENT_REPLAY_BYTES: usize = 5 * MAX_AGENT_UPDATES_BYTES + MAX_GUI_TEXT_BYTES;
const MAX_OUTBOUND_BYTES: usize = 72 * 1024 * 1024;
const MAX_HISTORY_CHUNK_ROWS: u32 = 512;
const MAX_RECYCLED_FRAME_BUFFERS: usize = 8;
const MAX_RECYCLED_FRAME_CAPACITY: usize = 8 * 1024 * 1024;
const MAX_COPY_PIPE_PROCESSES: usize = 8;
const MAX_COPY_PIPE_BYTES: usize = 32 * 1024 * 1024;
const MAX_COPY_PIPE_COMMAND_BYTES: usize = 8 * 1024;
const COPY_PIPE_TIMEOUT: Duration = Duration::from_secs(30);
const COPY_PIPE_POLL_INTERVAL: Duration = Duration::from_millis(20);
const GUI_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONTEXT_PATH_BYTES: usize = 4 * 1024;
const MAX_COMMAND_PROMPT_HISTORY: usize = 100;
const MAX_COMMAND_PROMPT_HISTORY_SNAPSHOT_ITEMS: usize = 20;
const MAX_COMMAND_PROMPT_HISTORY_SNAPSHOT_BYTES: usize = 32 * 1024;
const MAX_COMMAND_PROMPT_OUTPUT_BYTES: usize = 1024 * 1024;
const COMMAND_PROMPT_OUTPUT_TRUNCATED: &str = "… output truncated";
const MAX_CHOOSE_TREE_ITEMS: usize = 4_096;
const MAX_CHOOSE_TREE_ITEM_BYTES: usize = 512;
const CHOOSE_TREE_PAGE_ROWS: usize = 10;
const MAX_CHOOSE_BUFFER_ITEMS: usize = 4_096;
const MAX_CHOOSE_BUFFER_NAME_BYTES: usize = 512;
const MAX_CHOOSE_BUFFER_PREVIEW_BYTES: usize = 256;
const CHOOSE_BUFFER_PAGE_ROWS: usize = 10;
const MAX_PASTE_BUFFER_BYTES: usize = 32 * 1024 * 1024;
const MAX_PASTE_BUFFER_NAME_BYTES: usize = 4 * 1024;
const MAX_PASTE_BUFFER_SAMPLE_BYTES: usize = 40;
const MAX_DISPLAY_PANE_INDICATORS: usize = 4_096;
const MAX_CONCURRENT_PASTE_UPLOADS: usize = 2;
const PASTE_UPLOAD_RETENTION: usize = 8;
const MAX_PASTED_IMAGES_PER_PANE: usize = 8;
const MAX_PASTED_IMAGE_BYTES_PER_PANE: usize = 24 * 1024 * 1024;

fn terminal_status_should_close(status: &zz_terminal::SessionStatus) -> bool {
    matches!(
        status,
        zz_terminal::SessionStatus::Exited(_) | zz_terminal::SessionStatus::Failed(_)
    )
}

fn daemon_color_scheme() -> TerminalColorScheme {
    match std::env::var("ZZ_COLOR_SCHEME").as_deref() {
        Ok("light") => TerminalColorScheme::Light,
        _ => TerminalColorScheme::Dark,
    }
}

fn mode_keys_from_environment(visual: Option<&OsStr>, editor: Option<&OsStr>) -> &'static str {
    let Some(editor) = visual.or(editor) else {
        return "emacs";
    };
    let editor = editor.to_string_lossy();
    let basename = editor.rsplit('/').next().unwrap_or(&editor);
    if basename.contains("vi") {
        "vi"
    } else {
        "emacs"
    }
}

fn daemon_environment() -> Vec<(String, String)> {
    let mut environment = std::env::vars_os()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if let Ok(cwd) = std::env::current_dir() {
        let cwd = environment
            .get("PWD")
            .filter(|pwd| !pwd.is_empty())
            .filter(|pwd| {
                fs::canonicalize(pwd)
                    .ok()
                    .zip(fs::canonicalize(&cwd).ok())
                    .is_some_and(|(pwd, cwd)| pwd == cwd)
            })
            .cloned()
            .unwrap_or_else(|| cwd.to_string_lossy().into_owned());
        environment.insert("PWD".to_owned(), cwd);
    }
    environment.into_iter().collect()
}

fn terminal_environment_for_session(
    engine: &MuxEngine,
    session: SessionId,
) -> Result<Vec<(String, Option<String>)>, ServerError> {
    let mut environment = engine.environment_for_session(session)?;
    environment.retain(|(name, _)| name != "TERM");
    Ok(environment)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn terminal_working_directory(terminal: &TerminalSession) -> Option<PathBuf> {
    let process_id = terminal.foreground_process_id().filter(|pid| *pid != 0)?;
    let process_id = Pid::from_u32(process_id);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[process_id]),
        ProcessRefreshKind::new().with_cwd(sysinfo::UpdateKind::Always),
    );
    system
        .process(process_id)
        .and_then(|process| process.cwd())
        .map(Path::to_path_buf)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn terminal_working_directory(_terminal: &TerminalSession) -> Option<PathBuf> {
    None
}

fn terminal_current_command(terminal: &TerminalSession) -> String {
    let Some(process_id) = terminal
        .foreground_process_id()
        .filter(|pid| *pid != 0)
        .map(Pid::from_u32)
    else {
        return String::new();
    };
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[process_id]),
        ProcessRefreshKind::new(),
    );
    system
        .process(process_id)
        .map(|process| process.name().to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn command_log_line(command: &CommandInvocation) -> String {
    let mut line = canonical_command(&command.name).to_owned();
    for argument in &command.args {
        line.push(' ');
        line.push_str(argument);
    }
    line
}

fn push_server_message(inner: &mut ServerState, text: String) -> u64 {
    let number = inner.next_message_number;
    inner.next_message_number = inner.next_message_number.wrapping_add(1);
    inner.message_log.push_back(ServerMessage {
        number,
        time: SystemTime::now(),
        text,
    });
    let limit = inner.engine.message_limit();
    while inner.message_log.len() > limit {
        inner.message_log.pop_front();
    }
    number
}

fn daemon_uid() -> String {
    #[cfg(unix)]
    {
        rustix::process::getuid().as_raw().to_string()
    }
    #[cfg(not(unix))]
    {
        String::new()
    }
}

fn daemon_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}

fn resolve_appearance(
    color_scheme: TerminalColorScheme,
    entries: &[ConfigOverrideEntry],
) -> AppearanceLoad {
    apply_appearance_overrides(AppearanceLoad::defaults_for(color_scheme), entries)
}

fn log_appearance_load(reason: &str, load: &AppearanceLoad) {
    log::info!(
        target: "zz_daemon::diagnostics::appearance",
        "loaded appearance reason={reason} root={:?} scheme={} supported={} unsupported={} invalid={} diagnostics_dropped={} bytes={} hash={} appearance={:?}",
        load.root,
        load.appearance.color_scheme.as_str(),
        load.supported,
        load.unsupported,
        load.invalid,
        load.diagnostics_dropped,
        load.bytes_read,
        load.appearance.stable_hash(),
        load.appearance,
    );
    for diagnostic in &load.diagnostics {
        match diagnostic.disposition {
            AppearanceConfigDisposition::Invalid => log::warn!(
                target: "zz_daemon::diagnostics::appearance",
                "config reason={reason} path={} line={} key={:?} disposition={:?} message={}",
                diagnostic.path.display(),
                diagnostic.line,
                diagnostic.key,
                diagnostic.disposition,
                diagnostic.message,
            ),
            _ => log::debug!(
                target: "zz_daemon::diagnostics::appearance",
                "config reason={reason} path={} line={} key={:?} disposition={:?} message={}",
                diagnostic.path.display(),
                diagnostic.line,
                diagnostic.key,
                diagnostic.disposition,
                diagnostic.message,
            ),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MuxOverrideApplyReport {
    applied: usize,
    diagnostics: Vec<String>,
}

fn partition_config_overrides(
    entries: &[ConfigOverrideEntry],
) -> (Vec<ConfigOverrideEntry>, Vec<ConfigOverrideEntry>) {
    let mut appearance_entries = Vec::new();
    let mut mux_entries = Vec::new();
    for entry in entries {
        if zz_terminal::AppearanceConfigKey::from_config_key(&entry.0).is_some() {
            appearance_entries.push(entry.clone());
        } else if MuxOptionKey::from_config_key(&entry.0).is_some() {
            mux_entries.push(entry.clone());
        } else {
            log::warn!(
                target: "zz_daemon::diagnostics::mux_config",
                "ignored unsupported configuration override key={:?}",
                entry.0,
            );
        }
    }
    (appearance_entries, mux_entries)
}

#[derive(Clone, Debug)]
pub struct Daemon {
    socket_path: PathBuf,
    load_user_config: bool,
}

impl Daemon {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            load_user_config: true,
        }
    }

    /// Skip the user's `zz/mux.conf`, for hermetic embedding and tests.
    #[must_use]
    pub fn without_user_config(mut self) -> Self {
        self.load_user_config = false;
        self
    }

    /// Run the persistent listener on the current thread until shutdown.
    pub fn run_foreground(&self) -> Result<(), DaemonError> {
        prepare_socket(&self.socket_path)?;
        let listener = LocalTransport::bind(&self.socket_path).map_err(|error| {
            if error.kind() == ErrorKind::AddrInUse {
                DaemonError::AlreadyRunning(self.socket_path.clone())
            } else {
                DaemonError::Io(error)
            }
        })?;
        restrict_socket_permissions(&self.socket_path)?;
        listener.set_nonblocking(true)?;
        let _socket_guard = SocketGuard::new(self.socket_path.clone());
        let _identity_guard = DaemonIdentityGuard::install(&self.socket_path)?;

        self.run_foreground_listener::<LocalTransport>(&listener, self.socket_path.display())
    }

    fn run_foreground_listener<T: Transport>(
        &self,
        listener: &T::Listener,
        endpoint: impl std::fmt::Display,
    ) -> Result<(), DaemonError> {
        let color_scheme = daemon_color_scheme();
        let load = AppearanceLoad::defaults_for(color_scheme);
        log_appearance_load("startup", &load);
        let (appearance, provenance) = (Arc::new(load.appearance), load.provenance);
        let shared = Arc::new(Shared::configured(
            server_id(),
            appearance,
            provenance,
            self.load_user_config,
            paste_upload_directory(&self.socket_path),
            self.socket_path.clone(),
        ));
        shared.initialize(self.load_user_config)?;
        shared.start_diagnostic_sampler()?;
        shared.start_status_sampler()?;
        shared.log_diagnostic_snapshot("startup");
        #[cfg(unix)]
        let _signal_guard = DaemonSignalGuard::install(&shared)?;
        log::info!("zz daemon listening at {endpoint}");

        let accept_result = accept_connections::<T>(listener, &shared);
        if accept_result.is_err() {
            shared.request_shutdown();
        }
        accept_result?;
        shared.publish(EventPayload::ServerStopping);
        // The adapter children are told to close and joined before the socket
        // goes; what refuses to settle is the acp crate's problem, not ours.
        #[cfg(feature = "agent")]
        shared.shutdown_agents();
        shared.log_diagnostic_snapshot("shutdown");
        log::info!("zz daemon stopped");
        log::logger().flush();
        Ok(())
    }
}

#[cfg(unix)]
struct DaemonSignalGuard {
    stop: async_channel::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl DaemonSignalGuard {
    fn install(shared: &Arc<Shared>) -> Result<Self, DaemonError> {
        use async_signal::Signal;

        let mut signals = async_signal::Signals::new([Signal::Term, Signal::Int])?;
        let (stop, stopped) = async_channel::bounded(1);
        let shared = Arc::downgrade(shared);
        let thread = thread::Builder::new()
            .name("zz-daemon-signals".to_owned())
            .spawn(move || {
                use futures_lite::{StreamExt as _, future};

                let signalled = future::block_on(future::race(
                    async {
                        let _ = signals.next().await;
                        true
                    },
                    async {
                        let _ = stopped.recv().await;
                        false
                    },
                ));
                if signalled && let Some(shared) = shared.upgrade() {
                    shared.request_shutdown();
                }
            })
            .map_err(|error| DaemonError::Thread(error.to_string()))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

#[cfg(unix)]
impl Drop for DaemonSignalGuard {
    fn drop(&mut self) {
        let _ = self.stop.send_blocking(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn accept_connections<T: Transport>(
    listener: &T::Listener,
    shared: &Arc<Shared>,
) -> Result<(), DaemonError> {
    while !shared.stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok(stream) => {
                let shared = Arc::clone(shared);
                thread::Builder::new()
                    .name("zz-client".to_owned())
                    .spawn(move || {
                        if let Err(error) = handle_connection(stream, &shared) {
                            log::debug!("client disconnected: {error}");
                        }
                    })
                    .map_err(|error| DaemonError::Thread(error.to_string()))?;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                listener.wait_for_incoming(ACCEPT_WAIT_TIMEOUT)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(unix)]
struct SocketGuard(PathBuf);

#[cfg(windows)]
struct SocketGuard;

impl SocketGuard {
    fn new(path: PathBuf) -> Self {
        #[cfg(unix)]
        {
            Self(path)
        }
        #[cfg(windows)]
        {
            let _ = path;
            Self
        }
    }
}

#[cfg(unix)]
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(unix)]
fn paste_upload_directory(socket_path: &Path) -> PathBuf {
    socket_path
        .parent()
        .map_or_else(|| PathBuf::from("paste"), |parent| parent.join("paste"))
}

#[cfg(not(unix))]
fn paste_upload_directory(_socket_path: &Path) -> PathBuf {
    std::env::temp_dir().join("zz").join("paste")
}

fn write_paste_upload(
    directory: &Path,
    file_name: &str,
    bytes: &[u8],
) -> Result<PathBuf, std::io::Error> {
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    fs::set_permissions(
        directory,
        <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
    )?;
    let path = directory.join(file_name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(path)
}

fn prune_paste_uploads(directory: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut uploads = Vec::new();
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        uploads.push((metadata.modified().unwrap_or(UNIX_EPOCH), entry.path()));
    }
    if uploads.len() <= keep {
        return;
    }
    uploads.sort_unstable_by_key(|(modified, _)| Reverse(*modified));
    for (_, path) in uploads.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
}

struct OutboundMailbox {
    state: Mutex<OutboundState>,
    ready: Condvar,
}

#[derive(Default)]
struct OutboundState {
    reliable: VecDeque<Vec<u8>>,
    command_output: Option<Vec<u8>>,
    agent: BTreeMap<PaneId, PendingAgent>,
    agent_order: VecDeque<PaneId>,
    terminals: BTreeMap<PaneId, PendingTerminal>,
    delivered_terminals: BTreeMap<PaneId, TerminalGeneration>,
    delivered_images: BTreeMap<PaneId, BTreeMap<u32, u64>>,
    delivered_pasted_images: BTreeMap<PaneId, BTreeMap<u32, u64>>,
    terminal_order: VecDeque<PaneId>,
    recycled_frames: Vec<Vec<u8>>,
    recycled_capacity: usize,
    queued_bytes: usize,
    closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalEnqueue {
    Queued,
    NeedsFull,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KittyImageEnqueue {
    Queued,
    AlreadyDelivered,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PastedImageEnqueue {
    Queued,
    AlreadyDelivered,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalGeneration {
    content: u64,
    view: u64,
    dictionary: u32,
    columns: u16,
    rows: u16,
}

struct PendingTerminal {
    encoded: Vec<u8>,
    current: TerminalGeneration,
}

/// One pane's undelivered agent frames. Unlike a terminal viewport an agent
/// batch cannot be coalesced away — every item is transcript — so the lane
/// queues them and drops the whole pane when it outgrows its share.
#[derive(Default)]
struct PendingAgent {
    /// Encoded frames, each tagged with the first sequence it carries: what a
    /// lagged client must replay from.
    frames: VecDeque<(u64, Vec<u8>)>,
    bytes: usize,
}

#[derive(Clone, Copy)]
struct TerminalTransition {
    base: Option<TerminalGeneration>,
    current: TerminalGeneration,
}

enum TerminalFanout {
    Full,
    Patch(zz_terminal::TerminalViewportPatch),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalGeometry {
    columns: u16,
    rows: u16,
    cell_width_px: u32,
    cell_height_px: u32,
}

fn viewport_generation(viewport: &TerminalViewport) -> TerminalGeneration {
    TerminalGeneration {
        content: viewport.generation,
        view: viewport.view_generation,
        dictionary: viewport.dictionary_generation,
        columns: viewport.columns,
        rows: viewport.rows,
    }
}

fn terminal_transition(pane: PaneId, message: &ProtocolMessage) -> Option<TerminalTransition> {
    let ProtocolMessage::Event(Event { payload, .. }) = message else {
        return None;
    };
    match payload {
        EventPayload::TerminalViewport {
            pane: target,
            viewport,
        } if *target == pane => Some(TerminalTransition {
            base: None,
            current: viewport_generation(viewport),
        }),
        EventPayload::TerminalPatch {
            pane: target,
            patch,
        } if *target == pane => Some(TerminalTransition {
            base: Some(TerminalGeneration {
                content: patch.base_generation,
                view: patch.base_view_generation,
                dictionary: patch.dictionary_generation,
                columns: patch.columns,
                rows: patch.rows,
            }),
            current: TerminalGeneration {
                content: patch.generation,
                view: patch.view_generation,
                dictionary: patch.dictionary_generation,
                columns: patch.columns,
                rows: patch.rows,
            },
        }),
        _ => None,
    }
}

impl OutboundMailbox {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(OutboundState::default()),
            ready: Condvar::new(),
        })
    }

    fn encode_message(&self, message: &ProtocolMessage) -> Result<Vec<u8>, ProtocolError> {
        self.encode_with(|frame| encode_protocol_message_into(message, frame))
    }

    fn encode_with(
        &self,
        encode: impl FnOnce(&mut Vec<u8>) -> Result<(), ProtocolError>,
    ) -> Result<Vec<u8>, ProtocolError> {
        let mut frame = {
            let mut state = self.state.lock();
            take_recycled_frame(&mut state)
        };
        if let Err(error) = encode(&mut frame) {
            self.recycle_frame(frame);
            return Err(error);
        }
        Ok(frame)
    }

    fn recycle_frame(&self, frame: Vec<u8>) {
        let mut state = self.state.lock();
        recycle_outbound_frame(&mut state, frame);
    }

    #[must_use]
    fn enqueue_reliable(&self, message: &ProtocolMessage) -> bool {
        let removed_pane = match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::PaneRemoved(pane),
                ..
            }) => Some(*pane),
            _ => None,
        };
        let removed_images = match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::KittyImagesRemoved { pane, image_ids },
                ..
            }) => Some((*pane, image_ids.as_slice())),
            _ => None,
        };
        let clears_command_output = matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::CommandOutput { viewport: None, .. },
                ..
            })
        );
        let Ok(encoded) = self.encode_message(message) else {
            log::error!("failed to encode outbound control message");
            return false;
        };
        let mut state = self.state.lock();
        if state.closed {
            return false;
        }
        if let Some(pane) = removed_pane {
            remove_pending_terminal(&mut state, pane);
            clear_pending_agent(&mut state, pane);
            state.delivered_terminals.remove(&pane);
            state.delivered_images.remove(&pane);
            state.delivered_pasted_images.remove(&pane);
        }
        if let Some((pane, image_ids)) = removed_images
            && let Some(delivered) = state.delivered_images.get_mut(&pane)
        {
            for image_id in image_ids {
                delivered.remove(image_id);
            }
            if delivered.is_empty() {
                state.delivered_images.remove(&pane);
            }
        }
        if clears_command_output && let Some(frame) = state.command_output.take() {
            state.queued_bytes = state.queued_bytes.saturating_sub(frame.len());
            recycle_outbound_frame(&mut state, frame);
        }
        if state.reliable.len() >= MAX_RELIABLE_MESSAGES
            || !reserve_outbound_bytes(&mut state, encoded.len(), 0)
        {
            close_outbound(&mut state);
            self.ready.notify_all();
            return false;
        }
        state.queued_bytes += encoded.len();
        state.reliable.push_back(encoded);
        drop(state);
        self.ready.notify_one();
        true
    }

    fn enqueue_kitty_image(
        &self,
        pane: PaneId,
        image_id: u32,
        generation: u64,
        frames: &[Vec<u8>],
    ) -> KittyImageEnqueue {
        let mut state = self.state.lock();
        if state.closed {
            return KittyImageEnqueue::Closed;
        }
        if state
            .delivered_images
            .get(&pane)
            .and_then(|images| images.get(&image_id))
            == Some(&generation)
        {
            return KittyImageEnqueue::AlreadyDelivered;
        }
        let Some(frame_bytes) = frames
            .iter()
            .try_fold(0_usize, |total, frame| total.checked_add(frame.len()))
        else {
            close_outbound(&mut state);
            self.ready.notify_all();
            return KittyImageEnqueue::Closed;
        };
        if state.reliable.len().saturating_add(frames.len()) > MAX_RELIABLE_MESSAGES
            || !reserve_outbound_bytes(&mut state, frame_bytes, 0)
        {
            close_outbound(&mut state);
            self.ready.notify_all();
            return KittyImageEnqueue::Closed;
        }
        for frame in frames {
            state.queued_bytes += frame.len();
            state.reliable.push_back(frame.clone());
        }
        state
            .delivered_images
            .entry(pane)
            .or_default()
            .insert(image_id, generation);
        drop(state);
        self.ready.notify_one();
        KittyImageEnqueue::Queued
    }

    fn enqueue_kitty_images_removed(&self, pane: PaneId, image_ids: &[u32]) {
        let image_ids = {
            let state = self.state.lock();
            let Some(delivered) = state.delivered_images.get(&pane) else {
                return;
            };
            image_ids
                .iter()
                .copied()
                .filter(|image_id| delivered.contains_key(image_id))
                .collect::<Vec<_>>()
        };
        if image_ids.is_empty() {
            return;
        }
        let message = ProtocolMessage::Event(Event {
            sequence: Shared::next_sequence(),
            payload: EventPayload::KittyImagesRemoved { pane, image_ids },
        });
        let _ = self.enqueue_reliable(&message);
    }

    fn enqueue_pasted_image(
        &self,
        pane: PaneId,
        number: u32,
        token: u64,
        frames: &[Vec<u8>],
    ) -> PastedImageEnqueue {
        let mut state = self.state.lock();
        if state.closed {
            return PastedImageEnqueue::Closed;
        }
        if state
            .delivered_pasted_images
            .get(&pane)
            .and_then(|images| images.get(&number))
            == Some(&token)
        {
            return PastedImageEnqueue::AlreadyDelivered;
        }
        let Some(frame_bytes) = frames
            .iter()
            .try_fold(0_usize, |total, frame| total.checked_add(frame.len()))
        else {
            close_outbound(&mut state);
            self.ready.notify_all();
            return PastedImageEnqueue::Closed;
        };
        if state.reliable.len().saturating_add(frames.len()) > MAX_RELIABLE_MESSAGES
            || !reserve_outbound_bytes(&mut state, frame_bytes, 0)
        {
            close_outbound(&mut state);
            self.ready.notify_all();
            return PastedImageEnqueue::Closed;
        }
        for frame in frames {
            state.queued_bytes += frame.len();
            state.reliable.push_back(frame.clone());
        }
        state
            .delivered_pasted_images
            .entry(pane)
            .or_default()
            .insert(number, token);
        drop(state);
        self.ready.notify_one();
        PastedImageEnqueue::Queued
    }

    /// Queue one encoded `AgentUpdates` frame. Overflow clears the pane's lane
    /// and answers with a lag marker on the reliable lane instead of closing
    /// the connection: the journal makes the stream recoverable.
    #[cfg(feature = "agent")]
    fn enqueue_agent(&self, pane: PaneId, first_seq: u64, encoded: Vec<u8>) -> bool {
        let lagged_from = {
            let mut state = self.state.lock();
            if state.closed {
                return false;
            }
            let (queued_bytes, queued_first) =
                state.agent.get(&pane).map_or((0, first_seq), |queued| {
                    (
                        queued.bytes,
                        queued.frames.front().map_or(first_seq, |(seq, _)| *seq),
                    )
                });
            if queued_bytes.saturating_add(encoded.len()) > MAX_PENDING_AGENT_BYTES
                || !reserve_outbound_bytes(&mut state, encoded.len(), 0)
            {
                clear_pending_agent(&mut state, pane);
                Some(queued_first)
            } else {
                let fresh = queued_bytes == 0;
                state.queued_bytes += encoded.len();
                let queued = state.agent.entry(pane).or_default();
                queued.bytes = queued.bytes.saturating_add(encoded.len());
                queued.frames.push_back((first_seq, encoded));
                if fresh {
                    state.agent_order.push_back(pane);
                }
                None
            }
        };
        let Some(next_seq) = lagged_from else {
            self.ready.notify_one();
            return true;
        };
        log::warn!(
            target: "zz_daemon::diagnostics::outbound",
            "agent lane for {pane} overflowed; asking for a replay from {next_seq}",
        );
        self.enqueue_reliable(&Shared::event(EventPayload::AgentLagged { pane, next_seq }))
    }

    #[cfg(feature = "agent")]
    fn enqueue_agent_replay(&self, pane: PaneId, frames: Vec<(u64, Vec<u8>)>) -> bool {
        let Some(bytes) = frames
            .iter()
            .try_fold(0_usize, |total, (_, frame)| total.checked_add(frame.len()))
        else {
            return false;
        };
        let mut state = self.state.lock();
        if state.closed {
            return false;
        }
        let replaced = state.agent.get(&pane).map_or(0, |queued| queued.bytes);
        if bytes > MAX_PENDING_AGENT_REPLAY_BYTES
            || !reserve_outbound_bytes(&mut state, bytes, replaced)
        {
            close_outbound(&mut state);
            drop(state);
            self.ready.notify_all();
            return false;
        }
        clear_pending_agent(&mut state, pane);
        if frames.is_empty() {
            return true;
        }
        state.queued_bytes = state.queued_bytes.saturating_add(bytes);
        let queued = state.agent.entry(pane).or_default();
        queued.bytes = bytes;
        queued.frames.extend(frames);
        state.agent_order.push_back(pane);
        drop(state);
        self.ready.notify_one();
        true
    }

    #[cfg(feature = "agent")]
    fn cancel_agent(&self, pane: PaneId) {
        let mut state = self.state.lock();
        clear_pending_agent(&mut state, pane);
    }

    fn enqueue_terminal(&self, pane: PaneId, message: &ProtocolMessage) -> TerminalEnqueue {
        let Some(transition) = terminal_transition(pane, message) else {
            log::error!("refusing a non-terminal update in the terminal mailbox for {pane}");
            return TerminalEnqueue::Closed;
        };
        self.enqueue_terminal_with(pane, transition, |frame| {
            encode_protocol_message_into(message, frame)
        })
    }

    fn enqueue_terminal_viewport(
        &self,
        pane: PaneId,
        sequence: u64,
        viewport: &TerminalViewport,
    ) -> TerminalEnqueue {
        self.enqueue_terminal_with(
            pane,
            TerminalTransition {
                base: None,
                current: viewport_generation(viewport),
            },
            |frame| encode_terminal_viewport_event_into(pane, sequence, viewport, frame),
        )
    }

    fn enqueue_terminal_with(
        &self,
        pane: PaneId,
        transition: TerminalTransition,
        encode: impl FnOnce(&mut Vec<u8>) -> Result<(), ProtocolError>,
    ) -> TerminalEnqueue {
        {
            let state = self.state.lock();
            if state.closed {
                return TerminalEnqueue::Closed;
            }
            if state.terminals.contains_key(&pane) {
                return TerminalEnqueue::NeedsFull;
            }
            if transition.base.is_some()
                && transition.base != state.delivered_terminals.get(&pane).copied()
            {
                return TerminalEnqueue::NeedsFull;
            }
        }
        let Ok(encoded) = self.encode_with(encode) else {
            log::error!("failed to encode terminal update for {pane}");
            return TerminalEnqueue::Closed;
        };
        let mut state = self.state.lock();
        if state.closed {
            return TerminalEnqueue::Closed;
        }
        if state.terminals.contains_key(&pane) {
            recycle_outbound_frame(&mut state, encoded);
            return TerminalEnqueue::NeedsFull;
        }
        if transition.base.is_some()
            && transition.base != state.delivered_terminals.get(&pane).copied()
        {
            recycle_outbound_frame(&mut state, encoded);
            return TerminalEnqueue::NeedsFull;
        }
        if state.terminals.len() >= MAX_PENDING_TERMINALS
            || !reserve_outbound_bytes(&mut state, encoded.len(), 0)
        {
            close_outbound(&mut state);
            self.ready.notify_all();
            return TerminalEnqueue::Closed;
        }
        state.queued_bytes += encoded.len();
        state.terminals.insert(
            pane,
            PendingTerminal {
                encoded,
                current: transition.current,
            },
        );
        state.terminal_order.push_back(pane);
        drop(state);
        self.ready.notify_one();
        TerminalEnqueue::Queued
    }

    fn replace_terminal(&self, pane: PaneId, message: &ProtocolMessage) -> bool {
        let Some(transition) = terminal_transition(pane, message) else {
            log::error!("refusing a non-terminal update in the terminal mailbox for {pane}");
            return false;
        };
        self.replace_terminal_with(pane, transition, |frame| {
            encode_protocol_message_into(message, frame)
        })
    }

    fn replace_terminal_viewport(
        &self,
        pane: PaneId,
        sequence: u64,
        viewport: &TerminalViewport,
    ) -> bool {
        self.replace_terminal_with(
            pane,
            TerminalTransition {
                base: None,
                current: viewport_generation(viewport),
            },
            |frame| encode_terminal_viewport_event_into(pane, sequence, viewport, frame),
        )
    }

    fn replace_terminal_with(
        &self,
        pane: PaneId,
        transition: TerminalTransition,
        encode: impl FnOnce(&mut Vec<u8>) -> Result<(), ProtocolError>,
    ) -> bool {
        let Ok(encoded) = self.encode_with(encode) else {
            log::error!("failed to encode coalesced terminal update for {pane}");
            return false;
        };
        let mut state = self.state.lock();
        if state.closed {
            return false;
        }
        let replaced_len = state
            .terminals
            .get(&pane)
            .map_or(0, |pending| pending.encoded.len());
        if replaced_len == 0 && state.terminals.len() >= MAX_PENDING_TERMINALS
            || !reserve_outbound_bytes(&mut state, encoded.len(), replaced_len)
        {
            close_outbound(&mut state);
            self.ready.notify_all();
            return false;
        }
        state.queued_bytes = state
            .queued_bytes
            .saturating_sub(replaced_len)
            .saturating_add(encoded.len());
        let replaced = state.terminals.insert(
            pane,
            PendingTerminal {
                encoded,
                current: transition.current,
            },
        );
        if let Some(replaced) = replaced {
            recycle_outbound_frame(&mut state, replaced.encoded);
        } else {
            state.terminal_order.push_back(pane);
        }
        drop(state);
        self.ready.notify_one();
        true
    }

    fn replace_command_output(&self, message: &ProtocolMessage) -> bool {
        let Ok(encoded) = self.encode_message(message) else {
            log::error!("failed to encode command-output viewport");
            return false;
        };
        self.replace_encoded_command_output(encoded)
    }

    fn replace_encoded_command_output(&self, encoded: Vec<u8>) -> bool {
        let mut state = self.state.lock();
        if state.closed {
            return false;
        }
        let replaced_len = state.command_output.as_ref().map_or(0, Vec::len);
        if !reserve_outbound_bytes(&mut state, encoded.len(), replaced_len) {
            close_outbound(&mut state);
            self.ready.notify_all();
            return false;
        }
        state.queued_bytes = state
            .queued_bytes
            .saturating_sub(replaced_len)
            .saturating_add(encoded.len());
        let replaced = state.command_output.replace(encoded);
        if let Some(replaced) = replaced {
            recycle_outbound_frame(&mut state, replaced);
        }
        drop(state);
        self.ready.notify_one();
        true
    }

    fn recv(&self) -> Option<Vec<u8>> {
        let mut state = self.state.lock();
        loop {
            if let Some(frame) = state.reliable.pop_front() {
                state.queued_bytes = state.queued_bytes.saturating_sub(frame.len());
                return Some(frame);
            }
            if let Some(frame) = state.command_output.take() {
                state.queued_bytes = state.queued_bytes.saturating_sub(frame.len());
                return Some(frame);
            }
            // One frame per pane per turn: a chatty agent never starves the
            // pane beside it, and terminals still drain behind both.
            while let Some(pane) = state.agent_order.pop_front() {
                let Some(queued) = state.agent.get_mut(&pane) else {
                    continue;
                };
                let Some((_, frame)) = queued.frames.pop_front() else {
                    state.agent.remove(&pane);
                    continue;
                };
                queued.bytes = queued.bytes.saturating_sub(frame.len());
                if queued.frames.is_empty() {
                    state.agent.remove(&pane);
                } else {
                    state.agent_order.push_back(pane);
                }
                state.queued_bytes = state.queued_bytes.saturating_sub(frame.len());
                return Some(frame);
            }
            while let Some(pane) = state.terminal_order.pop_front() {
                if let Some(pending) = state.terminals.remove(&pane) {
                    state.queued_bytes = state.queued_bytes.saturating_sub(pending.encoded.len());
                    state.delivered_terminals.insert(pane, pending.current);
                    return Some(pending.encoded);
                }
            }
            if state.closed {
                return None;
            }
            self.ready.wait(&mut state);
        }
    }

    fn close(&self) {
        let mut state = self.state.lock();
        close_outbound(&mut state);
        drop(state);
        self.ready.notify_all();
    }

    #[cfg(test)]
    fn cancel_terminal(&self, pane: PaneId) {
        let mut state = self.state.lock();
        remove_pending_terminal(&mut state, pane);
        state.delivered_terminals.remove(&pane);
        state.delivered_images.remove(&pane);
        state.delivered_pasted_images.remove(&pane);
    }

    fn suspend_terminal(&self, pane: PaneId) {
        let mut state = self.state.lock();
        remove_pending_terminal(&mut state, pane);
        state.delivered_terminals.remove(&pane);
    }

    fn reset_kitty_images(&self) {
        self.state.lock().delivered_images.clear();
    }

    fn reset_pasted_images(&self) {
        self.state.lock().delivered_pasted_images.clear();
    }

    fn log_diagnostic_snapshot(&self, client: ClientId, reason: &str) {
        let state = self.state.lock();
        log::info!(
            target: "zz_daemon::diagnostics::outbound",
            "snapshot reason={reason} client={client} reliable_messages={} command_output_bytes={} agent_panes={} agent_frames={} terminal_messages={} delivered_terminals={} delivered_image_panes={} terminal_order={} recycled_frames={} recycled_capacity={} queued_bytes={} closed={}",
            state.reliable.len(),
            state.command_output.as_ref().map_or(0, Vec::len),
            state.agent.len(),
            state.agent.values().map(|queued| queued.frames.len()).sum::<usize>(),
            state.terminals.len(),
            state.delivered_terminals.len(),
            state.delivered_images.len(),
            state.terminal_order.len(),
            state.recycled_frames.len(),
            state.recycled_capacity,
            state.queued_bytes,
            state.closed,
        );
        log::trace!(
            target: "zz_daemon::diagnostics::outbound",
            "snapshot reason={reason} client={client} reliable_frame_lengths={:?} command_output_capacity={:?} terminals={:#?} delivered_terminals={:#?} terminal_order={:#?} recycled_frame_capacities={:?}",
            state.reliable.iter().map(Vec::len).collect::<Vec<_>>(),
            state.command_output.as_ref().map(Vec::capacity),
            state.terminals.iter().map(|(pane, pending)| (*pane, pending.encoded.len(), pending.encoded.capacity(), pending.current)).collect::<Vec<_>>(),
            state.delivered_terminals,
            state.terminal_order,
            state.recycled_frames.iter().map(Vec::capacity).collect::<Vec<_>>(),
        );
    }
}

fn take_recycled_frame(state: &mut OutboundState) -> Vec<u8> {
    let Some(frame) = state.recycled_frames.pop() else {
        return Vec::new();
    };
    state.recycled_capacity = state.recycled_capacity.saturating_sub(frame.capacity());
    frame
}

fn recycle_outbound_frame(state: &mut OutboundState, mut frame: Vec<u8>) {
    let capacity = frame.capacity();
    if state.closed
        || capacity == 0
        || state.recycled_frames.len() >= MAX_RECYCLED_FRAME_BUFFERS
        || state
            .recycled_capacity
            .checked_add(capacity)
            .is_none_or(|total| total > MAX_RECYCLED_FRAME_CAPACITY)
    {
        return;
    }
    frame.clear();
    state.recycled_capacity += capacity;
    state.recycled_frames.push(frame);
}

fn clear_pending_agent(state: &mut OutboundState, pane: PaneId) {
    if let Some(queued) = state.agent.remove(&pane) {
        state.queued_bytes = state.queued_bytes.saturating_sub(queued.bytes);
        for (_, frame) in queued.frames {
            recycle_outbound_frame(state, frame);
        }
    }
    state.agent_order.retain(|queued| *queued != pane);
}

fn remove_pending_terminal(state: &mut OutboundState, pane: PaneId) {
    if let Some(pending) = state.terminals.remove(&pane) {
        state.queued_bytes = state.queued_bytes.saturating_sub(pending.encoded.len());
        recycle_outbound_frame(state, pending.encoded);
    }
    state.terminal_order.retain(|queued| *queued != pane);
}

fn reserve_outbound_bytes(state: &mut OutboundState, incoming: usize, replaced: usize) -> bool {
    state
        .queued_bytes
        .saturating_sub(replaced)
        .checked_add(incoming)
        .is_some_and(|total| total <= MAX_OUTBOUND_BYTES)
}

fn close_outbound(state: &mut OutboundState) {
    state.closed = true;
    state.reliable.clear();
    state.command_output = None;
    state.agent.clear();
    state.agent_order.clear();
    state.terminals.clear();
    state.delivered_terminals.clear();
    state.delivered_images.clear();
    state.delivered_pasted_images.clear();
    state.terminal_order.clear();
    state.recycled_frames.clear();
    state.recycled_capacity = 0;
    state.queued_bytes = 0;
}

#[derive(Clone, Copy)]
enum DaemonCommandDispatch {
    CapturePane,
    AgentSend,
    SendLastOutput,
    CaptureBrowser,
    DebugMarker,
    Tools,
    Buffer,
    ListClients,
    ShowMessages,
    RefreshClient,
}

const DAEMON_COMMAND_DISPATCHES: &[(&str, DaemonCommandDispatch)] = &[
    ("capture-pane", DaemonCommandDispatch::CapturePane),
    ("capturep", DaemonCommandDispatch::CapturePane),
    ("agent-send", DaemonCommandDispatch::AgentSend),
    ("send-last-output", DaemonCommandDispatch::SendLastOutput),
    ("capture-browser", DaemonCommandDispatch::CaptureBrowser),
    ("debug-marker", DaemonCommandDispatch::DebugMarker),
    ("tools", DaemonCommandDispatch::Tools),
    ("set-buffer", DaemonCommandDispatch::Buffer),
    ("setb", DaemonCommandDispatch::Buffer),
    ("show-buffer", DaemonCommandDispatch::Buffer),
    ("showb", DaemonCommandDispatch::Buffer),
    ("list-buffers", DaemonCommandDispatch::Buffer),
    ("lsb", DaemonCommandDispatch::Buffer),
    ("load-buffer", DaemonCommandDispatch::Buffer),
    ("loadb", DaemonCommandDispatch::Buffer),
    ("save-buffer", DaemonCommandDispatch::Buffer),
    ("saveb", DaemonCommandDispatch::Buffer),
    ("delete-buffer", DaemonCommandDispatch::Buffer),
    ("deleteb", DaemonCommandDispatch::Buffer),
    ("paste-buffer", DaemonCommandDispatch::Buffer),
    ("pasteb", DaemonCommandDispatch::Buffer),
    ("list-clients", DaemonCommandDispatch::ListClients),
    ("lsc", DaemonCommandDispatch::ListClients),
    ("show-messages", DaemonCommandDispatch::ShowMessages),
    ("showmsgs", DaemonCommandDispatch::ShowMessages),
    ("refresh-client", DaemonCommandDispatch::RefreshClient),
    ("refresh", DaemonCommandDispatch::RefreshClient),
];

fn daemon_command_dispatch(name: &str) -> Option<DaemonCommandDispatch> {
    DAEMON_COMMAND_DISPATCHES
        .iter()
        .find_map(|(candidate, dispatch)| (*candidate == name).then_some(*dispatch))
}

struct Shared {
    inner: Mutex<ServerState>,
    /// Built on the first agent pane rather than at startup: a daemon that
    /// never opens one never touches the journal directory.
    #[cfg(feature = "agent")]
    agent: Mutex<Option<Arc<crate::agent::fanout::AgentRuntime>>>,
    #[cfg(feature = "agent")]
    agent_effects: Mutex<()>,
    #[cfg(feature = "agent")]
    agent_stopped: AtomicBool,
    kitty_image_frames: Mutex<BTreeMap<KittyImageKey, Arc<[Vec<u8>]>>>,
    pasted_images: Mutex<BTreeMap<PaneId, PanePastedImages>>,
    status: Mutex<StatusRenderer>,
    display_panes_deadline_tx: crossbeam_channel::Sender<DisplayPanesDeadlineCommand>,
    display_panes_deadline_rx:
        Mutex<Option<crossbeam_channel::Receiver<DisplayPanesDeadlineCommand>>>,
    stopping: AtomicBool,
    exit_empty_armed: AtomicBool,
    server_id: u64,
    load_user_config: bool,
    paste_directory: PathBuf,
    socket_path: PathBuf,
}

#[derive(Clone, Copy)]
struct DisplayPanesDeadline {
    client: ClientId,
    token: u64,
    deadline: Instant,
}

enum DisplayPanesDeadlineCommand {
    Schedule(DisplayPanesDeadline),
    Cancel { client: ClientId, token: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct KittyImageKey {
    pane: PaneId,
    image_id: u32,
    generation: u64,
}

#[derive(Debug)]
struct PendingPastedImage {
    token: u64,
    format: PastedImageFormat,
    bytes: Arc<[u8]>,
}

#[derive(Debug)]
struct StoredPastedImage {
    token: u64,
    format: PastedImageFormat,
    bytes: Arc<[u8]>,
    frames: Option<Arc<[Vec<u8>]>>,
}

#[derive(Default, Debug)]
struct PanePastedImages {
    pending: VecDeque<PendingPastedImage>,
    images: BTreeMap<u32, StoredPastedImage>,
    order: VecDeque<(u32, u64)>,
    pending_bytes: usize,
    stored_bytes: usize,
}

struct PastedImageAdmission {
    evicted_numbers: Vec<u32>,
    retained: bool,
}

impl PanePastedImages {
    fn push_pending(&mut self, pending: PendingPastedImage) -> PastedImageAdmission {
        let token = pending.token;
        self.pending_bytes = self.pending_bytes.saturating_add(pending.bytes.len());
        self.pending.push_back(pending);
        let evicted_numbers = self.enforce_limits();
        PastedImageAdmission {
            evicted_numbers,
            retained: self.pending.iter().any(|pending| pending.token == token),
        }
    }

    fn bind(&mut self, token: u64, number: u32) -> Option<Vec<u32>> {
        let position = self
            .pending
            .iter()
            .position(|pending| pending.token == token)?;
        let pending = self.pending.remove(position)?;
        self.pending_bytes = self.pending_bytes.saturating_sub(pending.bytes.len());
        if let Some(replaced) = self.images.remove(&number) {
            self.stored_bytes = self.stored_bytes.saturating_sub(replaced.bytes.len());
            self.order.retain(|(stored_number, stored_token)| {
                *stored_number != number || *stored_token != replaced.token
            });
        }
        self.stored_bytes = self.stored_bytes.saturating_add(pending.bytes.len());
        self.order.push_back((number, token));
        self.images.insert(
            number,
            StoredPastedImage {
                token,
                format: pending.format,
                bytes: pending.bytes,
                frames: None,
            },
        );
        Some(self.enforce_limits())
    }

    fn enforce_limits(&mut self) -> Vec<u32> {
        let mut evicted = Vec::new();
        while self.images.len().saturating_add(self.pending.len()) > MAX_PASTED_IMAGES_PER_PANE
            || self.stored_bytes.saturating_add(self.pending_bytes)
                > MAX_PASTED_IMAGE_BYTES_PER_PANE
        {
            while self.order.front().is_some_and(|(number, token)| {
                self.images
                    .get(number)
                    .is_none_or(|image| image.token != *token)
            }) {
                self.order.pop_front();
            }
            if let Some((evicted_number, evicted_token)) = self.order.pop_front() {
                if let Some(image) = self.images.get(&evicted_number)
                    && image.token == evicted_token
                {
                    let image = self
                        .images
                        .remove(&evicted_number)
                        .expect("the pasted image was present");
                    self.stored_bytes = self.stored_bytes.saturating_sub(image.bytes.len());
                    evicted.push(evicted_number);
                }
            } else if let Some(pending) = self.pending.pop_back() {
                self.pending_bytes = self.pending_bytes.saturating_sub(pending.bytes.len());
            } else {
                break;
            }
        }
        evicted
    }

    fn expire(&mut self, token: u64) {
        if let Some(position) = self
            .pending
            .iter()
            .position(|pending| pending.token == token)
            && let Some(pending) = self.pending.remove(position)
        {
            self.pending_bytes = self.pending_bytes.saturating_sub(pending.bytes.len());
        }
    }
}

fn pasted_image_frames(
    pane: PaneId,
    number: u32,
    image: &mut StoredPastedImage,
) -> Option<Arc<[Vec<u8>]>> {
    if let Some(frames) = &image.frames {
        return Some(Arc::clone(frames));
    }
    let total_bytes = u32::try_from(image.bytes.len()).ok()?;
    let mut frames = Vec::with_capacity(
        1 + image
            .bytes
            .len()
            .div_ceil(zz_protocol::MAX_PASTE_UPLOAD_CHUNK_BYTES),
    );
    let begin = ProtocolMessage::PastedImageBegin {
        pane,
        number,
        format: image.format,
        total_bytes,
    };
    let mut encoded = Vec::new();
    encode_protocol_message_into(&begin, &mut encoded).ok()?;
    frames.push(encoded);
    for chunk in image
        .bytes
        .chunks(zz_protocol::MAX_PASTE_UPLOAD_CHUNK_BYTES)
    {
        let message = ProtocolMessage::PastedImageChunk {
            pane,
            number,
            bytes: chunk.to_vec(),
        };
        let mut encoded = Vec::new();
        encode_protocol_message_into(&message, &mut encoded).ok()?;
        frames.push(encoded);
    }
    let frames: Arc<[Vec<u8>]> = frames.into();
    image.frames = Some(Arc::clone(&frames));
    Some(frames)
}

struct ClientRegistrationGuard<'a> {
    shared: &'a Shared,
    client: ClientId,
    armed: bool,
}

impl<'a> ClientRegistrationGuard<'a> {
    fn new(shared: &'a Shared, client: ClientId) -> Self {
        Self {
            shared,
            client,
            armed: true,
        }
    }

    fn unregister(&mut self) {
        if std::mem::take(&mut self.armed) {
            self.shared.unregister(self.client);
        }
    }
}

impl Drop for ClientRegistrationGuard<'_> {
    fn drop(&mut self) {
        self.unregister();
    }
}

impl Shared {
    #[cfg(test)]
    fn with_appearance(server_id: u64, appearance: Arc<TerminalAppearance>) -> Self {
        Self::configured_with_boot_environment(
            server_id,
            appearance,
            AppearanceProvenance::default(),
            false,
            std::env::temp_dir().join("zz-test-paste"),
            std::env::temp_dir().join("zz-test.sock"),
            "emacs",
            std::iter::empty::<(String, String)>(),
        )
    }

    fn configured(
        server_id: u64,
        appearance: Arc<TerminalAppearance>,
        appearance_provenance: AppearanceProvenance,
        load_user_config: bool,
        paste_directory: PathBuf,
        socket_path: PathBuf,
    ) -> Self {
        let visual = std::env::var_os("VISUAL");
        let editor = std::env::var_os("EDITOR");
        Self::configured_with_boot_environment(
            server_id,
            appearance,
            appearance_provenance,
            load_user_config,
            paste_directory,
            socket_path,
            mode_keys_from_environment(visual.as_deref(), editor.as_deref()),
            daemon_environment(),
        )
    }

    fn configured_with_boot_environment<I, K, V>(
        server_id: u64,
        appearance: Arc<TerminalAppearance>,
        appearance_provenance: AppearanceProvenance,
        load_user_config: bool,
        paste_directory: PathBuf,
        socket_path: PathBuf,
        default_mode_keys: &str,
        environment: I,
    ) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut state = ServerState {
            active_color_scheme: appearance.color_scheme,
            appearance,
            appearance_provenance,
            ..ServerState::default()
        };
        state
            .engine
            .set_default_mode_keys(default_mode_keys)
            .expect("daemon mode-keys default is valid");
        state.engine.seed_global_environment(environment);
        let (host, host_short) = host_names();
        state.engine.set_format_server_context(
            host.clone(),
            host_short.clone(),
            socket_path.display().to_string(),
            unix_timestamp(),
        );
        state
            .engine
            .set_format_server_identity(std::process::id(), daemon_uid(), daemon_user());
        for option in MuxOptionKey::ALL {
            let value = state.engine.mux_option_value(option);
            state
                .mux_options
                .set(option, value.clone(), MuxOptionSource::Default);
            state
                .mux_option_underlay
                .set(option, value, MuxOptionSource::Default);
        }
        let (display_panes_deadline_tx, display_panes_deadline_rx) = crossbeam_channel::unbounded();
        Self {
            inner: Mutex::new(state),
            #[cfg(feature = "agent")]
            agent: Mutex::new(None),
            #[cfg(feature = "agent")]
            agent_effects: Mutex::new(()),
            #[cfg(feature = "agent")]
            agent_stopped: AtomicBool::new(false),
            kitty_image_frames: Mutex::new(BTreeMap::new()),
            pasted_images: Mutex::new(BTreeMap::new()),
            status: Mutex::new(StatusRenderer::default()),
            display_panes_deadline_tx,
            display_panes_deadline_rx: Mutex::new(Some(display_panes_deadline_rx)),
            stopping: AtomicBool::new(false),
            exit_empty_armed: AtomicBool::new(false),
            server_id,
            load_user_config,
            paste_directory,
            socket_path,
        }
    }

    #[cfg(test)]
    fn new(server_id: u64) -> Self {
        Self::with_appearance(server_id, Arc::new(TerminalAppearance::default()))
    }

    fn initialize(self: &Arc<Self>, load_user_config: bool) -> Result<(), DaemonError> {
        self.start_display_panes_deadline_dispatcher()?;
        let mut context = ExecutionContext::default();
        if let Some(config) = load_user_config
            .then(default_mux_config)
            .flatten()
            .filter(|path| path.is_file())
        {
            self.load_config_file(&config, &mut context, 0)?;
        }
        self.apply_stored_mux_config_overrides("startup-mux-replay");
        if self.inner.lock().engine.state.sessions.is_empty() {
            self.execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "0"]),
            )?;
        }
        self.exit_empty_armed.store(true, Ordering::Release);
        // Building the runtime is what warms the adapter cache, so a daemon
        // that has agent panes enabled pays the npx download before the first
        // pane asks for it.
        #[cfg(feature = "agent")]
        {
            let agent_panes_enabled = self.inner.lock().engine.experimental_agent_pane();
            if agent_panes_enabled {
                let _ = self.agent_runtime();
            }
        }
        self.request_shutdown_if_empty(&self.inner.lock());
        Ok(())
    }

    fn start_display_panes_deadline_dispatcher(self: &Arc<Self>) -> Result<(), DaemonError> {
        let Some(receiver) = self.display_panes_deadline_rx.lock().take() else {
            return Ok(());
        };
        let shared = Arc::downgrade(self);
        let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
        thread::Builder::new()
            .name("zz-display-panes".to_owned())
            .spawn(move || {
                if ready_tx.send(()).is_err() {
                    return;
                }
                let mut deadlines = BTreeMap::<ClientId, DisplayPanesDeadline>::new();
                loop {
                    let next = deadlines
                        .values()
                        .min_by_key(|deadline| deadline.deadline)
                        .copied();
                    let command = if let Some(next) = next {
                        let now = Instant::now();
                        if next.deadline <= now {
                            deadlines.remove(&next.client);
                            let Some(shared) = shared.upgrade() else {
                                return;
                            };
                            shared.expire_display_panes(next, now);
                            continue;
                        }
                        match receiver.recv_deadline(next.deadline) {
                            Ok(command) => command,
                            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                                deadlines.remove(&next.client);
                                let Some(shared) = shared.upgrade() else {
                                    return;
                                };
                                shared.expire_display_panes(next, Instant::now());
                                continue;
                            }
                            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                        }
                    } else {
                        let Ok(command) = receiver.recv() else {
                            return;
                        };
                        command
                    };
                    match command {
                        DisplayPanesDeadlineCommand::Schedule(deadline) => {
                            let Some(shared) = shared.upgrade() else {
                                return;
                            };
                            if shared
                                .inner
                                .lock()
                                .display_panes
                                .get(&deadline.client)
                                .is_some_and(|overlay| {
                                    overlay.token == deadline.token
                                        && overlay.deadline == Some(deadline.deadline)
                                })
                            {
                                deadlines.insert(deadline.client, deadline);
                            }
                        }
                        DisplayPanesDeadlineCommand::Cancel { client, token } => {
                            if deadlines
                                .get(&client)
                                .is_some_and(|deadline| deadline.token == token)
                            {
                                deadlines.remove(&client);
                            }
                        }
                    }
                }
            })
            .map_err(|error| DaemonError::Thread(error.to_string()))?;
        ready_rx
            .recv()
            .map_err(|error| DaemonError::Thread(error.to_string()))
    }

    fn request_shutdown(&self) {
        self.stopping.store(true, Ordering::Release);
    }

    fn request_shutdown_if_empty(&self, state: &ServerState) {
        if self.exit_empty_armed.load(Ordering::Acquire)
            && state.engine.state.sessions.is_empty()
            && state.subscribers.is_empty()
        {
            self.request_shutdown();
        }
    }

    fn refresh_status(&self, refresh: bool) {
        let requests = {
            let mut inner = self.inner.lock();
            inner.engine.set_format_now(unix_timestamp());
            let formats = inner.engine.status_formats().clone();
            let snapshot = inner.engine.state.snapshot();
            let facts = format_hook_facts(&inner);
            inner
                .subscribers
                .keys()
                .copied()
                .map(|client| {
                    let attached = client_attached_session(&inner, client);
                    StatusRequest {
                        client,
                        formats: formats.clone(),
                        context: status_context(
                            &snapshot,
                            &inner.engine,
                            attached,
                            client_focused_window_for_attachment(&inner, client),
                        ),
                        facts: facts.clone(),
                    }
                })
                .collect::<Vec<_>>()
        };
        if requests.is_empty() {
            return;
        }
        let changed = self.status.lock().render_changed(&requests, refresh);
        for (client, status) in changed {
            self.publish_to_client(client, EventPayload::StatusChanged { status });
        }
    }

    fn start_status_sampler(self: &Arc<Self>) -> Result<(), DaemonError> {
        let shared = Arc::downgrade(self);
        thread::Builder::new()
            .name("zz-daemon-status".to_owned())
            .spawn(move || {
                let mut due = Instant::now();
                loop {
                    thread::sleep(STATUS_POLL_INTERVAL);
                    let Some(shared) = shared.upgrade() else {
                        break;
                    };
                    if shared.stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let interval = shared.inner.lock().engine.status_formats().interval;
                    if interval.is_zero() {
                        due = Instant::now();
                        continue;
                    }
                    let now = Instant::now();
                    if now < due {
                        continue;
                    }
                    due = now + interval;
                    shared.refresh_status(true);
                }
            })
            .map_err(|error| DaemonError::Thread(error.to_string()))?;
        Ok(())
    }

    fn start_diagnostic_sampler(self: &Arc<Self>) -> Result<(), DaemonError> {
        if !log::log_enabled!(target: "zz_daemon::diagnostics::state", log::Level::Trace) {
            return Ok(());
        }
        let shared = Arc::downgrade(self);
        thread::Builder::new()
            .name("zz-daemon-diagnostics".to_owned())
            .spawn(move || {
                loop {
                    thread::sleep(DIAGNOSTIC_STATE_INTERVAL);
                    let Some(shared) = shared.upgrade() else {
                        break;
                    };
                    shared.log_diagnostic_snapshot("periodic");
                    if shared.stopping.load(Ordering::Acquire) {
                        break;
                    }
                }
            })
            .map_err(|error| DaemonError::Thread(error.to_string()))?;
        Ok(())
    }

    fn log_diagnostic_snapshot(&self, reason: &str) {
        if !log::log_enabled!(target: "zz_daemon::diagnostics::state", log::Level::Trace) {
            return;
        }
        let DiagnosticSample {
            mux,
            terminals,
            command_outputs,
            subscribers,
            attached,
            visible_terminals,
            key_clients,
            copy_sessions,
            swallowed_keys,
            suppressed_text,
            command_prompts,
            choose_trees,
            choose_buffers,
            display_panes,
            command_history,
            paste_buffers,
            active_copy_pipes,
        } = DiagnosticSample::capture(&self.inner.lock());
        let attachment_count = attached.values().map(BTreeSet::len).sum::<usize>();
        log::info!(
            target: "zz_daemon::diagnostics::state",
            "snapshot reason={reason} server_id={} stopping={} mux_generation={} sessions={} terminals={} command_outputs={} subscribers={} attachments={} visible_terminal_clients={} key_clients={} copy_sessions={} swallowed_key_clients={} suppressed_text_clients={} command_prompts={} choose_trees={} choose_buffers={} display_panes={} command_history={} paste_buffers={} active_copy_pipes={}",
            self.server_id,
            self.stopping.load(Ordering::Acquire),
            mux.generation,
            mux.sessions.len(),
            terminals.len(),
            command_outputs.len(),
            subscribers.len(),
            attachment_count,
            visible_terminals.len(),
            key_clients.len(),
            copy_sessions.len(),
            swallowed_keys.len(),
            suppressed_text.len(),
            command_prompts.len(),
            choose_trees.len(),
            choose_buffers.len(),
            display_panes.len(),
            command_history.len(),
            paste_buffers.len(),
            active_copy_pipes,
        );
        log::trace!(
            target: "zz_daemon::diagnostics::state",
            "snapshot reason={reason} mux={mux:#?} attached={attached:#?} visible_terminals={visible_terminals:#?} key_clients={key_clients:#?} copy_sessions={copy_sessions:#?} swallowed_keys={swallowed_keys:#?} suppressed_text={suppressed_text:#?} command_prompt_clients={command_prompts:#?} choose_tree_clients={choose_trees:#?} choose_buffer_clients={choose_buffers:#?} display_panes_clients={display_panes:#?} command_history={command_history:#?} paste_buffers={paste_buffers:#?}",
        );
        for (pane, terminal) in terminals {
            let viewport = terminal.latest_viewport();
            log::info!(
                target: "zz_daemon::diagnostics::terminal_state",
                "snapshot reason={reason} pane={pane} terminal_strong_count={} diagnostics={:?}",
                Arc::strong_count(&terminal),
                terminal.diagnostics(),
            );
            log::trace!(
                target: "zz_daemon::diagnostics::terminal_state",
                "snapshot reason={reason} pane={pane} viewport={viewport:#?}"
            );
        }
        for (client, pane, terminal) in command_outputs {
            let viewport = terminal.latest_viewport();
            log::info!(
                target: "zz_daemon::diagnostics::terminal_state",
                "snapshot reason={reason} command_output_client={client} pane={pane} terminal_strong_count={} diagnostics={:?}",
                Arc::strong_count(&terminal),
                terminal.diagnostics(),
            );
            log::trace!(
                target: "zz_daemon::diagnostics::terminal_state",
                "snapshot reason={reason} command_output_client={client} pane={pane} viewport={viewport:#?}"
            );
        }
        for (client, subscriber) in subscribers {
            subscriber.log_diagnostic_snapshot(client, reason);
        }
    }

    fn register(
        &self,
        kind: ClientKind,
        client_instance_id: ClientInstanceId,
        device_name: Option<String>,
        color_scheme: Option<TerminalColorScheme>,
    ) -> (ClientId, ServerHello) {
        let mut inner = self.inner.lock();
        inner.engine.set_format_now(unix_timestamp());
        let client = ClientId(inner.next_client_id);
        inner.next_client_id = inner.next_client_id.saturating_add(1);
        inner.client_instances.insert(client, client_instance_id);
        if let Some(device_name) = device_name {
            inner.client_names.insert(client, device_name);
        }
        if kind == ClientKind::Interactive {
            let color_scheme = color_scheme.unwrap_or(inner.active_color_scheme);
            inner.client_color_schemes.insert(client, color_scheme);
            inner.active_color_scheme = color_scheme;
        }
        let capabilities = vec![
            "mux-v1".to_owned(),
            "terminal-viewport-v3".to_owned(),
            "terminal-row-patches".to_owned(),
            "terminal-visible-window-subscriptions".to_owned(),
            "terminal-native-selection".to_owned(),
            "terminal-async-regex-search".to_owned(),
            "terminal-osc8-links".to_owned(),
            "terminal-appearance-v2".to_owned(),
            "terminal-appearance-reload".to_owned(),
            "config-overrides-v1".to_owned(),
            "terminal-copy-pipe".to_owned(),
            "native-synchronize-panes".to_owned(),
            "native-command-prompt".to_owned(),
            "native-command-output-view".to_owned(),
            "native-choose-tree".to_owned(),
            "native-choose-buffer".to_owned(),
            "native-display-panes".to_owned(),
            "native-split-resize".to_owned(),
            "native-pane-swap".to_owned(),
            "native-pane-relocation".to_owned(),
            "native-preset-layouts".to_owned(),
            "native-pane-rotation".to_owned(),
            "browser-panes".to_owned(),
            "tmux-config-subset".to_owned(),
            NEW_SESSION_ATTACH_CAPABILITY.to_owned(),
        ];
        let hello = ServerHello {
            protocol_version: PROTOCOL_VERSION,
            server_id: self.server_id,
            client_id: client,
            client_instance_id,
            capabilities,
            appearance: (*inner.appearance).clone(),
            appearance_provenance: inner.appearance_provenance.clone(),
            mux_options: inner.mux_options.clone(),
            status: StatusLine::default(),
            key_tables: inner.engine.keys.snapshot(),
        };
        let attached = client_attached_session(&inner, client);
        let request = StatusRequest {
            client,
            formats: inner.engine.status_formats().clone(),
            context: status_context(
                &inner.engine.state.snapshot(),
                &inner.engine,
                attached,
                client_focused_window_for_attachment(&inner, client),
            ),
            facts: format_hook_facts(&inner),
        };
        drop(inner);
        let mut hello = hello;
        hello.status = self.status.lock().render_initial(&request);
        (client, hello)
    }

    fn subscribe(&self, client: ClientId, outbound: Arc<OutboundMailbox>) {
        self.inner.lock().subscribers.insert(client, outbound);
    }

    fn client_instance_id(&self, client: ClientId) -> Option<ClientInstanceId> {
        self.inner.lock().client_instances.get(&client).copied()
    }

    #[cfg(test)]
    fn register_subscribed(
        &self,
        kind: ClientKind,
        device_name: Option<String>,
        color_scheme: Option<TerminalColorScheme>,
        outbound: Arc<OutboundMailbox>,
    ) -> (ClientId, ServerHello) {
        let (client, hello) =
            self.register(kind, ClientInstanceId::default(), device_name, color_scheme);
        if kind == ClientKind::Interactive {
            self.subscribe(client, outbound);
        }
        (client, hello)
    }

    fn unregister(&self, client: ClientId) {
        self.detach(client);
        self.fail_gui_requests_for(client);
        self.status.lock().forget(client);
        let (terminals, command_output) = {
            let mut inner = self.inner.lock();
            inner.subscribers.remove(&client);
            inner.client_color_schemes.remove(&client);
            inner.client_names.remove(&client);
            inner.client_instances.remove(&client);
            inner.key_engines.remove(&client);
            inner.copy_sessions.remove(&client);
            inner.prefix_armed.remove(&client);
            inner.swallowed_keys.remove(&client);
            inner.suppressed_text.remove(&client);
            inner.command_prompts.remove(&client);
            inner.choose_trees.remove(&client);
            inner.choose_buffers.remove(&client);
            let _ = take_display_panes(&mut inner, client);
            inner.focused_windows.remove(&client);
            inner.client_terminal_input_sequences.remove(&client);
            inner
                .paste_uploads
                .retain(|(uploader, _), _| *uploader != client);
            (
                inner.terminals.values().cloned().collect::<Vec<_>>(),
                inner.command_outputs.remove(&client),
            )
        };
        let view = TerminalViewId(client.0);
        if let Some(command_output) = command_output {
            command_output.terminal.view_action(
                view,
                zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel),
            );
        }
        for terminal in terminals {
            terminal.release_view(view);
        }
        self.request_shutdown_if_empty(&self.inner.lock());
    }

    fn execute_command_request(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        request_id: u64,
        command: &CommandInvocation,
    ) -> CommandResponse {
        let client_name = {
            let mut inner = self.inner.lock();
            let client_name = inner
                .client_names
                .get(&client)
                .cloned()
                .unwrap_or_else(|| format!("client-{}", client.0));
            let command = command_log_line(command);
            push_server_message(&mut inner, format!("{client_name} command: {command}"));
            client_name
        };
        match self.execute(client, kind, context, command) {
            Ok(execution) => {
                if kind == ClientKind::Interactive
                    && !execution.output.is_empty()
                    && let Err(error) = self.open_command_output(
                        client,
                        context.pane,
                        command.name.clone(),
                        &execution.output,
                    )
                {
                    self.publish_to_client(
                        client,
                        EventPayload::ClientMessage {
                            pane: context.pane,
                            kind: ClientMessageKind::Error,
                            text: error.to_string(),
                        },
                    );
                }
                CommandResponse::Success {
                    request_id,
                    output: execution.output,
                }
            }
            Err(error) => {
                let error = daemon_server_error(error);
                let mut inner = self.inner.lock();
                push_server_message(&mut inner, format!("{client_name} message: {error}"));
                CommandResponse::Error { request_id, error }
            }
        }
    }

    fn execute(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        command: &CommandInvocation,
    ) -> Result<Execution, DaemonError> {
        self.execute_with_mux_source(
            client,
            kind,
            context,
            command,
            MuxOptionSource::RuntimeCommand,
        )
    }

    fn execute_with_mux_source(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        command: &CommandInvocation,
        mux_source: MuxOptionSource,
    ) -> Result<Execution, DaemonError> {
        let preempted = zz_mux::CommandSpec::DAEMON_COMMAND_NAMES
            .contains(&command.name.as_str())
            .then(|| {
                match daemon_command_dispatch(&command.name)
                    .expect("daemon command catalog and dispatch must agree")
                {
                    DaemonCommandDispatch::CapturePane => self.capture_pane(context, &command.args),
                    DaemonCommandDispatch::AgentSend => self.agent_send(context, &command.args),
                    DaemonCommandDispatch::SendLastOutput => {
                        self.send_last_output(context, &command.args)
                    }
                    DaemonCommandDispatch::CaptureBrowser => {
                        self.capture_browser(context, &command.args)
                    }
                    DaemonCommandDispatch::DebugMarker => {
                        Ok(debug_marker(client, context, &command.args))
                    }
                    DaemonCommandDispatch::Tools => Ok(workspace_tools_catalog()),
                    DaemonCommandDispatch::Buffer => {
                        self.buffer_command(context, &command.name, &command.args)
                    }
                    DaemonCommandDispatch::ListClients => {
                        self.list_clients(context, &command.name, &command.args)
                    }
                    DaemonCommandDispatch::ShowMessages => {
                        self.show_messages(&command.name, &command.args)
                    }
                    DaemonCommandDispatch::RefreshClient => {
                        self.refresh_client(client, kind, &command.name, &command.args)
                    }
                }
            });
        if let Some(result) = preempted {
            if result.is_ok() {
                self.inner.lock().engine.repair_context(context);
            }
            return result;
        }
        let generation = self.inner.lock().engine.state.generation();
        let result = self.execute_with_mux_source_inner(client, kind, context, command, mux_source);
        let publish_snapshot = {
            let inner = self.inner.lock();
            let current = inner.engine.state.generation();
            current != generation && inner.last_published_mux_generation != current
        };
        if publish_snapshot {
            self.publish_snapshot();
        }
        result
    }

    fn execute_with_mux_source_inner(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        command: &CommandInvocation,
        mux_source: MuxOptionSource,
    ) -> Result<Execution, DaemonError> {
        let mut terminals_to_watch = Vec::new();
        let mut client_events = Vec::new();
        let mut direct_events = Vec::new();
        let mut source_files = Vec::new();
        let mut removed_panes = Vec::new();
        let mut agent_panes_opened = Vec::new();
        #[cfg(feature = "agent")]
        let mut agent_panes_restarted = Vec::new();
        let mut relocated_terminal_views = Vec::new();
        let mut retired_command_outputs = Vec::new();
        let mut deferred_terminal_commands = Vec::new();
        let mut unfocused_copy_mode_exits = Vec::new();
        let mut cleared_bells = Vec::new();
        let mut display_panes_deadline = None;
        let mut attach = None;
        let mut detach = None;
        let mut detached_session = None;
        let mut reload_config = false;
        let mut snapshot_changed = false;
        let mut mux_options_changed = false;
        #[cfg(feature = "agent")]
        let mut agent_options_changed = false;
        let mut status_formats_changed = false;

        let (execution, mux_options_event) = {
            let mut inner = self.inner.lock();
            let active_windows_before = inner
                .engine
                .state
                .sessions
                .iter()
                .map(|(session, state)| (*session, state.active_window))
                .collect::<BTreeMap<_, _>>();
            let active_panes_before = inner
                .engine
                .state
                .windows
                .iter()
                .map(|(window, state)| (*window, state.active_pane))
                .collect::<BTreeMap<_, _>>();
            let belled_panes_before = inner
                .engine
                .state
                .windows
                .values()
                .flat_map(|window| window.panes.values())
                .filter(|pane| pane.bell)
                .map(|pane| pane.id)
                .collect::<BTreeSet<_>>();
            let mut focused_windows_before = BTreeMap::new();
            for (session, clients) in &inner.attached {
                let Some(state) = inner.engine.state.sessions.get(session) else {
                    continue;
                };
                for attached_client in clients {
                    focused_windows_before.insert(
                        (*session, *attached_client),
                        client_focused_window(&inner, *attached_client, state),
                    );
                }
            }
            let facts = format_hook_facts(&inner);
            let mut hooks = DaemonFormatHooks::command(&facts);
            inner.engine.set_format_now(unix_timestamp());
            let execution = inner
                .engine
                .execute_with_format_hooks(context, command, &mut hooks)?;
            let selected_panes = inner
                .engine
                .state
                .windows
                .iter()
                .filter_map(|(window, state)| {
                    active_panes_before
                        .get(window)
                        .is_some_and(|previous| *previous != state.active_pane)
                        .then_some(state.active_pane)
                })
                .collect::<Vec<_>>();
            for pane in selected_panes {
                if inner.engine.state.set_pane_bell(pane, false) {
                    snapshot_changed = true;
                }
            }
            for pane in belled_panes_before {
                if inner.engine.state.pane(pane).is_none_or(|pane| !pane.bell) {
                    snapshot_changed = true;
                    if let Some(terminal) = inner.terminals.get(&pane) {
                        cleared_bells.push(Arc::clone(terminal));
                    }
                }
            }
            let changed_windows = inner
                .engine
                .state
                .sessions
                .iter()
                .filter_map(|(session, state)| {
                    active_windows_before
                        .get(session)
                        .is_some_and(|previous| *previous != state.active_window)
                        .then_some((*session, state.active_window))
                })
                .collect::<Vec<_>>();
            for (session, focused_window) in changed_windows {
                let attached = inner.attached.get(&session).cloned().unwrap_or_default();
                if attached.contains(&client) {
                    for attached_client in attached {
                        let focus = if attached_client == client {
                            focused_window
                        } else {
                            focused_windows_before
                                .get(&(session, attached_client))
                                .copied()
                                .unwrap_or(focused_window)
                        };
                        inner.focused_windows.insert(attached_client, focus);
                    }
                } else {
                    for attached_client in attached {
                        inner
                            .focused_windows
                            .insert(attached_client, focused_window);
                    }
                }
            }
            for effect in &execution.effects {
                match effect {
                    MuxEffect::PaneCreated {
                        pane,
                        kind: PaneKindSnapshot::Terminal,
                        inherit_cwd_from,
                        cwd,
                        command,
                    }
                    | MuxEffect::PaneMaterialized {
                        pane,
                        kind: PaneKindSnapshot::Terminal,
                        inherit_cwd_from,
                        cwd,
                        command,
                    } => {
                        let history_limit = inner.engine.history_limit_for_pane(*pane)?;
                        let word_separators =
                            WordSeparators::new(inner.engine.word_separators_for_pane(*pane)?);
                        let working_directory = match cwd.as_deref().map(PathBuf::from) {
                            Some(path) if path.is_dir() => Some(path),
                            Some(_) => std::env::var_os("HOME")
                                .map(PathBuf::from)
                                .filter(|path| path.is_dir())
                                .or_else(|| Some(PathBuf::from("/"))),
                            None => inherit_cwd_from
                                .and_then(|source| inner.terminals.get(&source))
                                .and_then(|terminal| terminal_working_directory(terminal))
                                .or_else(|| std::env::current_dir().ok()),
                        };
                        let start_path = working_directory
                            .as_deref()
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let appearance = Arc::clone(&inner.appearance);
                        let pane_session = inner
                            .engine
                            .state
                            .window_for_pane(*pane)
                            .map(|window| inner.engine.state.windows[&window].session)
                            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
                        let mut env =
                            terminal_environment_for_session(&inner.engine, pane_session)?;
                        env.extend([
                            ("ZZ_PANE".to_owned(), Some(pane.to_string())),
                            (
                                "ZZ_SOCKET".to_owned(),
                                Some(self.socket_path.display().to_string()),
                            ),
                            ("ZZ_SESSION".to_owned(), Some(pane_session.to_string())),
                        ]);
                        if let Some(path) = &working_directory {
                            env.push(("PWD".to_owned(), Some(path.to_string_lossy().into_owned())));
                        }
                        let spawn = TerminalSpawn {
                            working_directory: working_directory.clone(),
                            command: command.clone(),
                            terminal_type: Some(
                                inner.engine.default_terminal_for_spawn().to_owned(),
                            ),
                            env,
                        };
                        let session = Arc::new(TerminalSession::spawn(
                            history_limit,
                            appearance,
                            spawn.clone(),
                        ));
                        let current_path = terminal_working_directory(&session)
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        deferred_terminal_commands.push(
                            DeferredTerminalCommand::SetWordSeparators {
                                terminal: Arc::clone(&session),
                                separators: word_separators,
                            },
                        );
                        inner.terminals.insert(*pane, Arc::clone(&session));
                        inner.terminal_spawns.insert(*pane, spawn);
                        inner.engine.set_pane_runtime_facts_with_hooks(
                            *pane,
                            PaneRuntimeFacts {
                                current_path,
                                start_path,
                                ..PaneRuntimeFacts::default()
                            },
                            &mut hooks,
                        );
                        let attached_clients = inner
                            .engine
                            .state
                            .window_for_pane(*pane)
                            .map(|window| inner.engine.state.windows[&window].session)
                            .and_then(|session| inner.attached.get(&session))
                            .cloned()
                            .unwrap_or_default();
                        for client in attached_clients {
                            deferred_terminal_commands.push(DeferredTerminalCommand::AttachView {
                                terminal: Arc::clone(&session),
                                view: TerminalViewId(client.0),
                            });
                        }
                        terminals_to_watch.push((*pane, session));
                    }
                    MuxEffect::PaneRespawned {
                        pane,
                        cwd,
                        command,
                        environment,
                        empty,
                    } => {
                        let previous = inner.terminal_spawns.get(pane).cloned().unwrap_or_default();
                        let history_limit = inner.engine.history_limit_for_pane(*pane)?;
                        let word_separators =
                            WordSeparators::new(inner.engine.word_separators_for_pane(*pane)?);
                        let working_directory = match cwd.as_deref().map(PathBuf::from) {
                            Some(path) => {
                                let path = if path.is_absolute() {
                                    path
                                } else if let Some(base) = &previous.working_directory {
                                    base.join(&path)
                                } else {
                                    path
                                };
                                if path.is_dir() {
                                    Some(path)
                                } else {
                                    std::env::var_os("HOME")
                                        .map(PathBuf::from)
                                        .filter(|path| path.is_dir())
                                        .or_else(|| Some(PathBuf::from("/")))
                                }
                            }
                            None => previous
                                .working_directory
                                .clone()
                                .or_else(|| std::env::current_dir().ok()),
                        };
                        let start_path = working_directory
                            .as_deref()
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let appearance = Arc::clone(&inner.appearance);
                        let pane_session = inner
                            .engine
                            .state
                            .window_for_pane(*pane)
                            .map(|window| inner.engine.state.windows[&window].session)
                            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
                        let mut env =
                            terminal_environment_for_session(&inner.engine, pane_session)?;
                        env.extend([
                            ("ZZ_PANE".to_owned(), Some(pane.to_string())),
                            (
                                "ZZ_SOCKET".to_owned(),
                                Some(self.socket_path.display().to_string()),
                            ),
                            ("ZZ_SESSION".to_owned(), Some(pane_session.to_string())),
                        ]);
                        env.extend(
                            environment
                                .iter()
                                .map(|(name, value)| (name.clone(), Some(value.clone()))),
                        );
                        if let Some(path) = &working_directory {
                            env.push(("PWD".to_owned(), Some(path.to_string_lossy().into_owned())));
                        }
                        let spawn = TerminalSpawn {
                            working_directory: working_directory.clone(),
                            command: command.clone().or(previous.command),
                            terminal_type: Some(
                                inner.engine.default_terminal_for_spawn().to_owned(),
                            ),
                            env,
                        };
                        let session = Arc::new(if *empty {
                            TerminalSession::spawn_empty_with_appearance(history_limit, appearance)
                        } else {
                            TerminalSession::spawn(history_limit, appearance, spawn.clone())
                        });
                        let current_path = if *empty {
                            start_path.clone()
                        } else {
                            terminal_working_directory(&session)
                                .map(|path| path.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        };
                        deferred_terminal_commands.push(
                            DeferredTerminalCommand::SetWordSeparators {
                                terminal: Arc::clone(&session),
                                separators: word_separators,
                            },
                        );
                        inner.terminals.insert(*pane, Arc::clone(&session));
                        inner.terminal_spawns.insert(*pane, spawn);
                        inner.engine.set_pane_runtime_facts_with_hooks(
                            *pane,
                            PaneRuntimeFacts {
                                current_path,
                                start_path,
                                ..PaneRuntimeFacts::default()
                            },
                            &mut hooks,
                        );
                        let attached_clients = inner
                            .attached
                            .get(&pane_session)
                            .cloned()
                            .unwrap_or_default();
                        for client in attached_clients {
                            deferred_terminal_commands.push(DeferredTerminalCommand::AttachView {
                                terminal: Arc::clone(&session),
                                view: TerminalViewId(client.0),
                            });
                        }
                        if let Some((_, geometry)) = terminal_resize_for_pane(&inner, *pane) {
                            deferred_terminal_commands.push(DeferredTerminalCommand::Resize {
                                terminal: Arc::clone(&session),
                                geometry,
                            });
                        }
                        terminals_to_watch.push((*pane, session));
                    }
                    MuxEffect::PaneCreated {
                        kind: PaneKindSnapshot::Browser(_) | PaneKindSnapshot::Picker,
                        ..
                    }
                    | MuxEffect::PaneMaterialized {
                        kind: PaneKindSnapshot::Browser(_) | PaneKindSnapshot::Picker,
                        ..
                    } => {}
                    MuxEffect::PaneCreated {
                        pane,
                        kind: PaneKindSnapshot::Agent(descriptor),
                        inherit_cwd_from,
                        ..
                    }
                    | MuxEffect::PaneMaterialized {
                        pane,
                        kind: PaneKindSnapshot::Agent(descriptor),
                        inherit_cwd_from,
                        ..
                    } => {
                        let working_directory = descriptor
                            .cwd
                            .clone()
                            .or_else(|| {
                                inherit_cwd_from
                                    .and_then(|source| inner.terminals.get(&source))
                                    .and_then(|terminal| terminal_working_directory(terminal))
                            })
                            .or_else(|| std::env::current_dir().ok());
                        inner
                            .engine
                            .state
                            .update_agent_cwd(*pane, working_directory)?;
                        agent_panes_opened.push(*pane);
                    }
                    MuxEffect::AgentPaneRestart { pane } => {
                        #[cfg(feature = "agent")]
                        agent_panes_restarted.push(*pane);
                        #[cfg(not(feature = "agent"))]
                        let _ = pane;
                    }
                    MuxEffect::PaneCreated {
                        pane,
                        kind: PaneKindSnapshot::Editor(_),
                        inherit_cwd_from,
                        ..
                    }
                    | MuxEffect::PaneMaterialized {
                        pane,
                        kind: PaneKindSnapshot::Editor(_),
                        inherit_cwd_from,
                        ..
                    } => {
                        let working_directory = inherit_cwd_from
                            .and_then(|source| inner.terminals.get(&source))
                            .and_then(|terminal| terminal_working_directory(terminal))
                            .or_else(|| std::env::current_dir().ok());
                        if let Some(working_directory) = working_directory {
                            inner.engine.state.update_editor_cwd(
                                *pane,
                                working_directory.to_string_lossy().into_owned(),
                            )?;
                        }
                    }
                    MuxEffect::PanesRemoved(panes) => {
                        let clients = inner
                            .command_outputs
                            .iter()
                            .filter_map(|(client, output)| {
                                panes.contains(&output.pane).then_some(*client)
                            })
                            .collect::<Vec<_>>();
                        for client in clients {
                            if let Some(output) = take_command_output(&mut inner, client) {
                                retired_command_outputs.push((client, output));
                            }
                        }
                        for pane in panes {
                            inner.terminals.remove(pane);
                            inner.terminal_spawns.remove(pane);
                            inner.terminal_geometries.remove(pane);
                            inner.paste_uploads.retain(|_, upload| upload.pane != *pane);
                            removed_panes.push(*pane);
                        }
                    }
                    MuxEffect::PaneRelocated { pane, from, to } => {
                        if let Some(terminal) = inner.terminals.get(pane).cloned() {
                            deferred_terminal_commands.push(
                                DeferredTerminalCommand::SetWordSeparators {
                                    terminal: Arc::clone(&terminal),
                                    separators: WordSeparators::new(
                                        inner.engine.word_separators_for_session(*to),
                                    ),
                                },
                            );
                            relocated_terminal_views.push((
                                terminal,
                                inner.attached.get(from).cloned().unwrap_or_default(),
                                inner.attached.get(to).cloned().unwrap_or_default(),
                            ));
                        }
                    }
                    MuxEffect::SendKeys { pane, keys } => {
                        let sinks = resolve_input_sinks(&inner, *pane)?;
                        let mut terminals = Vec::new();
                        for sink in sinks {
                            match sink {
                                PaneSink::Terminal(terminal) => {
                                    terminals.push(terminal);
                                }
                                PaneSink::Browser(target) => {
                                    client_events.push(EventPayload::BrowserCommand {
                                        pane: target,
                                        command: BrowserCommand::SendKeys(keys.clone()),
                                    });
                                }
                            }
                        }
                        if !terminals.is_empty() {
                            deferred_terminal_commands.push(DeferredTerminalCommand::SendTokens {
                                terminals,
                                keys: keys.clone(),
                            });
                        }
                    }
                    MuxEffect::CopyModeRepeat { pane, count } => {
                        if inner
                            .copy_sessions
                            .get(&client)
                            .is_some_and(|session| session.pane == *pane)
                        {
                            inner
                                .key_engines
                                .entry(client)
                                .or_default()
                                .set_repeat_count(*count);
                        }
                    }
                    MuxEffect::TerminalView { pane, action } => {
                        let command_output = inner
                            .command_outputs
                            .get(&client)
                            .map(|output| Arc::clone(&output.terminal));
                        if let Some(terminal) = command_output {
                            deferred_terminal_commands.push(DeferredTerminalCommand::ViewAction {
                                terminal,
                                view: TerminalViewId(client.0),
                                action: action.clone(),
                            });
                            continue;
                        }
                        let terminal = inner
                            .terminals
                            .get(pane)
                            .cloned()
                            .ok_or(ServerError::PaneExited(*pane))?;
                        let targets: Vec<ClientId> =
                            if client_is_attached_to_pane(&inner, client, *pane) {
                                vec![client]
                            } else {
                                attached_clients_for_pane(&inner, *pane)
                                    .map(|clients| clients.iter().copied().collect())
                                    .unwrap_or_default()
                            };
                        if targets.is_empty() {
                            return Err(ServerError::PaneNotAttached(*pane).into());
                        }
                        for target in targets {
                            deferred_terminal_commands.push(DeferredTerminalCommand::ViewAction {
                                terminal: Arc::clone(&terminal),
                                view: TerminalViewId(target.0),
                                action: action.clone(),
                            });
                            if terminal_view_action_enters_copy_mode(action) {
                                enter_copy_session(&mut inner, target, *pane)?;
                            } else if terminal_view_action_exits_copy_mode(action) {
                                exit_copy_session(&mut inner, target);
                            } else if terminal_view_action_arms_scroll_exit(action)
                                && let Some(session) = inner.copy_sessions.get_mut(&target)
                                && session.pane == *pane
                            {
                                session.scroll_exit = true;
                            }
                        }
                    }
                    MuxEffect::TerminalUi { pane, command } => {
                        if let Some(output) = inner.command_outputs.get(&client) {
                            client_events.push(EventPayload::TerminalUiCommand {
                                pane: output.pane,
                                command: *command,
                            });
                        } else if !inner.terminals.contains_key(pane) {
                            return Err(ServerError::PaneExited(*pane).into());
                        } else {
                            client_events.push(EventPayload::TerminalUiCommand {
                                pane: *pane,
                                command: *command,
                            });
                        }
                    }
                    MuxEffect::FocusSidebar { pane } => {
                        if kind != ClientKind::Interactive
                            || !inner.subscribers.contains_key(&client)
                        {
                            return Err(ServerError::InvalidCommand(
                                "focus-sidebar requires an interactive client".to_owned(),
                            )
                            .into());
                        }
                        if !client_is_attached_to_pane(&inner, client, *pane) {
                            return Err(ServerError::PaneNotAttached(*pane).into());
                        }
                        dismiss_overlays(
                            &mut inner,
                            client,
                            None,
                            &mut direct_events,
                            &mut retired_command_outputs,
                        );
                        inner.swallowed_keys.remove(&client);
                        direct_events.push(EventPayload::FocusSidebar);
                    }
                    MuxEffect::CommandPrompt {
                        prompt,
                        input,
                        template,
                    } => {
                        if kind != ClientKind::Interactive
                            || !inner.subscribers.contains_key(&client)
                        {
                            return Err(ServerError::InvalidCommand(
                                "command-prompt requires an interactive client".to_owned(),
                            )
                            .into());
                        }
                        dismiss_overlays(
                            &mut inner,
                            client,
                            Some(Overlay::CommandPrompt),
                            &mut direct_events,
                            &mut retired_command_outputs,
                        );
                        if !inner.command_prompts.contains_key(&client) {
                            let prompt =
                                CommandPrompt::new(prompt.clone(), input.clone(), template.clone());
                            let state = prompt.state(&inner.command_history);
                            inner.command_prompts.insert(client, prompt);
                            direct_events.push(EventPayload::CommandPrompt { state: Some(state) });
                        }
                    }
                    MuxEffect::ChooseTree {
                        pane,
                        kind: tree_kind,
                    } => {
                        if kind != ClientKind::Interactive
                            || !inner.subscribers.contains_key(&client)
                        {
                            return Err(ServerError::InvalidCommand(
                                "choose-tree requires an interactive client".to_owned(),
                            )
                            .into());
                        }
                        let attached_session = client_attached_session(&inner, client);
                        let chooser = ChooseTreeSession::new(
                            *tree_kind,
                            *pane,
                            &inner.engine.state,
                            attached_session,
                        )?;
                        if attached_session != Some(chooser.source_session) {
                            return Err(ServerError::PaneNotAttached(*pane).into());
                        }
                        dismiss_overlays(
                            &mut inner,
                            client,
                            Some(Overlay::ChooseTree),
                            &mut direct_events,
                            &mut retired_command_outputs,
                        );
                        inner.swallowed_keys.remove(&client);
                        inner.suppressed_text.remove(&client);
                        let state = chooser.rendered.clone();
                        inner.choose_trees.insert(client, chooser);
                        direct_events.push(EventPayload::ChooseTree { state: Some(state) });
                    }
                    MuxEffect::ChooseBuffer { pane } => {
                        if kind != ClientKind::Interactive
                            || !inner.subscribers.contains_key(&client)
                        {
                            return Err(ServerError::InvalidCommand(
                                "choose-buffer requires an interactive client".to_owned(),
                            )
                            .into());
                        }
                        let attached_session = client_attached_session(&inner, client);
                        let Some(chooser) = ChooseBufferSession::new(
                            *pane,
                            &inner.engine.state,
                            &inner.paste_buffers,
                        )?
                        else {
                            continue;
                        };
                        if attached_session != Some(chooser.source_session) {
                            return Err(ServerError::PaneNotAttached(*pane).into());
                        }
                        dismiss_overlays(
                            &mut inner,
                            client,
                            Some(Overlay::ChooseBuffer),
                            &mut direct_events,
                            &mut retired_command_outputs,
                        );
                        inner.swallowed_keys.remove(&client);
                        inner.suppressed_text.remove(&client);
                        let state = chooser.rendered.clone();
                        inner.choose_buffers.insert(client, chooser);
                        direct_events.push(EventPayload::ChooseBuffer { state: Some(state) });
                    }
                    MuxEffect::DisplayPanes { pane, duration_ms } => {
                        if kind != ClientKind::Interactive
                            || !inner.subscribers.contains_key(&client)
                        {
                            return Err(ServerError::InvalidCommand(
                                "display-panes requires an interactive client".to_owned(),
                            )
                            .into());
                        }
                        let (source_session, source_window, state) =
                            build_display_panes_state(&inner.engine, *pane, *duration_ms)?;
                        if client_attached_session(&inner, client) != Some(source_session) {
                            return Err(ServerError::PaneNotAttached(*pane).into());
                        }
                        dismiss_overlays(
                            &mut inner,
                            client,
                            Some(Overlay::DisplayPanes),
                            &mut direct_events,
                            &mut retired_command_outputs,
                        );
                        let _ = take_display_panes(&mut inner, client);
                        inner.swallowed_keys.remove(&client);
                        inner.suppressed_text.remove(&client);
                        inner.next_display_panes_token =
                            inner.next_display_panes_token.wrapping_add(1).max(1);
                        let token = inner.next_display_panes_token;
                        let deadline = (*duration_ms != 0).then(|| {
                            Instant::now() + Duration::from_millis(u64::from(*duration_ms))
                        });
                        inner.display_panes.insert(
                            client,
                            DisplayPanesSession {
                                token,
                                source_pane: *pane,
                                source_session,
                                source_window,
                                state: state.clone(),
                                deadline,
                                cancel: deadline.map(|_| self.display_panes_deadline_tx.clone()),
                            },
                        );
                        if let Some(deadline) = deadline {
                            display_panes_deadline = Some(DisplayPanesDeadline {
                                client,
                                token,
                                deadline,
                            });
                        }
                        direct_events.push(EventPayload::DisplayPanes { state: Some(state) });
                    }
                    MuxEffect::DisplayMessage {
                        pane,
                        text,
                        duration_ms,
                    } => {
                        push_server_message(&mut inner, text.clone());
                        direct_events.push(EventPayload::TimedClientMessage {
                            pane: *pane,
                            kind: ClientMessageKind::Info,
                            text: text.clone(),
                            duration_ms: *duration_ms,
                        });
                    }
                    MuxEffect::BufferLimitChanged(limit) => {
                        inner.automatic_paste_buffer_limit = AutomaticPasteBufferLimit(*limit);
                    }
                    MuxEffect::WordSeparatorsChanged { session } => {
                        for (pane, terminal) in &inner.terminals {
                            let Some(window) = inner.engine.state.window_for_pane(*pane) else {
                                continue;
                            };
                            let pane_session = inner.engine.state.windows[&window].session;
                            if session.is_some_and(|target| target != pane_session) {
                                continue;
                            }
                            deferred_terminal_commands.push(
                                DeferredTerminalCommand::SetWordSeparators {
                                    terminal: Arc::clone(terminal),
                                    separators: WordSeparators::new(
                                        inner.engine.word_separators_for_session(pane_session),
                                    ),
                                },
                            );
                        }
                        for output in inner.command_outputs.values() {
                            let Some(window) = inner.engine.state.window_for_pane(output.pane)
                            else {
                                continue;
                            };
                            let output_session = inner.engine.state.windows[&window].session;
                            if session.is_some_and(|target| target != output_session) {
                                continue;
                            }
                            deferred_terminal_commands.push(
                                DeferredTerminalCommand::SetWordSeparators {
                                    terminal: Arc::clone(&output.terminal),
                                    separators: WordSeparators::new(
                                        inner.engine.word_separators_for_session(output_session),
                                    ),
                                },
                            );
                        }
                    }
                    MuxEffect::ModeKeysChanged { window } => {
                        retarget_copy_mode_tables(&mut inner, *window);
                    }
                    MuxEffect::MuxOptionChanged { option } => {
                        let value = inner.engine.mux_option_value(*option);
                        mux_options_changed |=
                            inner.mux_options.set(*option, value.clone(), mux_source);
                        if mux_source != MuxOptionSource::Override {
                            inner.mux_option_underlay.set(*option, value, mux_source);
                        }
                        #[cfg(feature = "agent")]
                        {
                            agent_options_changed |= matches!(
                                option,
                                MuxOptionKey::AgentCommand
                                    | MuxOptionKey::AgentClaudeCodeCommand
                                    | MuxOptionKey::AgentAutoApprove
                            );
                        }
                    }
                    MuxEffect::StatusFormatsChanged => status_formats_changed = true,
                    MuxEffect::Attach {
                        session,
                        detach_others,
                    } => attach = Some((*session, *detach_others)),
                    MuxEffect::Detach(scope) => {
                        detach = Some(*scope);
                        detached_session = client_attached_session(&inner, client);
                    }
                    MuxEffect::SourceFile { path, quiet } => {
                        source_files.push((path.clone(), *quiet));
                    }
                    MuxEffect::ReloadConfig => reload_config = true,
                    MuxEffect::KillServer => self.request_shutdown(),
                    MuxEffect::SnapshotChanged => snapshot_changed = true,
                }
            }
            self.request_shutdown_if_empty(&inner);
            if snapshot_changed {
                unfocused_copy_mode_exits = unfocused_copy_sessions(&mut inner);
                let clients = inner
                    .command_outputs
                    .iter()
                    .filter_map(|(client, output)| {
                        (client_context_pane(&inner, *client) != Some(output.pane))
                            .then_some(*client)
                    })
                    .collect::<Vec<_>>();
                for client in clients {
                    if let Some(output) = take_command_output(&mut inner, client) {
                        retired_command_outputs.push((client, output));
                    }
                }
            }
            let mux_options_event = mux_options_changed.then(|| inner.mux_options.clone());
            (execution, mux_options_event)
        };

        for command in deferred_terminal_commands {
            command.run();
        }
        for terminal in cleared_bells {
            terminal.clear_bell();
        }
        for (client, terminal) in unfocused_copy_mode_exits {
            terminal.view_action(
                TerminalViewId(client.0),
                zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel),
            );
        }
        for (client, output) in retired_command_outputs {
            Self::retire_command_output(client, output);
        }
        for (terminal, previous, next) in relocated_terminal_views {
            for client in previous.difference(&next) {
                terminal.detach_view(TerminalViewId(client.0));
            }
            for client in next.difference(&previous) {
                terminal.attach_view(TerminalViewId(client.0));
            }
        }
        match detach {
            None => {}
            Some(DetachScope::Client) => {
                if let Some(session) = detached_session {
                    self.publish_to_client(client, EventPayload::Detached { session, by: None });
                }
                self.detach(client);
            }
            Some(DetachScope::Others) => self.evict_clients(None, client),
            Some(DetachScope::Session(session)) => {
                self.evict_clients(Some(session), client);
                if detached_session == Some(session) {
                    self.publish_to_client(client, EventPayload::Detached { session, by: None });
                    self.detach(client);
                }
            }
        }
        if let Some((session, detach_others)) = attach {
            if kind == ClientKind::Interactive {
                let mut snapshot = self.attach(client, session)?;
                if detach_others {
                    self.evict_clients(Some(session), client);
                    let inner = self.inner.lock();
                    snapshot = inner.engine.state.snapshot();
                    let presence = snapshot_presence(&inner);
                    stamp_snapshot_for_client(&inner, client, &mut snapshot, &presence);
                }
                let outbound = self.inner.lock().subscribers.get(&client).cloned();
                if let Some(outbound) = outbound {
                    outbound.reset_kitty_images();
                    outbound.reset_pasted_images();
                    let _ =
                        outbound.enqueue_reliable(&ProtocolMessage::Attached { session, snapshot });
                    self.send_resync(client, &outbound);
                }
                self.publish_snapshot();
            } else if detach_others {
                self.evict_clients(Some(session), client);
                self.publish_snapshot();
            }
        }
        for (pane, terminal) in terminals_to_watch {
            self.watch_terminal(pane, &terminal)?;
        }
        for event in client_events {
            let pane = match &event {
                EventPayload::BrowserCommand { pane, .. }
                | EventPayload::TerminalUiCommand { pane, .. } => *pane,
                _ => continue,
            };
            self.publish_for_pane(pane, &event);
        }
        for event in direct_events {
            self.publish_to_client(client, event);
        }
        if let Some(deadline) = display_panes_deadline {
            self.display_panes_deadline_tx
                .send(DisplayPanesDeadlineCommand::Schedule(deadline))
                .map_err(|_| {
                    DaemonError::Thread("display-panes deadline dispatcher stopped".to_owned())
                })?;
        }
        #[cfg(feature = "agent")]
        {
            if agent_options_changed {
                self.reconfigure_agents();
            }
            self.close_agent_panes(&removed_panes);
            for pane in agent_panes_opened {
                self.open_agent_pane(pane);
            }
            for pane in agent_panes_restarted {
                self.restart_agent_pane(pane);
            }
        }
        for pane in removed_panes {
            self.publish(EventPayload::PaneRemoved(pane));
        }
        if let Some(options) = mux_options_event {
            self.publish(EventPayload::MuxOptionsChanged { options });
        }
        if snapshot_changed {
            self.publish_snapshot();
        } else if status_formats_changed {
            self.refresh_status(false);
        }
        let mut source_file_error = None;
        for (path, quiet) in source_files {
            if path == "-" {
                self.publish_to_client(
                    client,
                    EventPayload::ClientMessage {
                        pane: context.pane,
                        kind: ClientMessageKind::Warning,
                        text: "source-file from standard input is not supported".to_owned(),
                    },
                );
                continue;
            }
            let path = expand_path(&path);
            let matches = source_glob_matches(&path);
            for error in &matches.errors {
                self.publish_to_client(
                    client,
                    EventPayload::ClientMessage {
                        pane: context.pane,
                        kind: ClientMessageKind::Warning,
                        text: format!("source-file glob error for {}: {error}", path.display()),
                    },
                );
            }
            if matches.paths.is_empty() && matches.errors.is_empty() {
                if !quiet {
                    self.publish_to_client(
                        client,
                        EventPayload::ClientMessage {
                            pane: context.pane,
                            kind: ClientMessageKind::Warning,
                            text: format!("no such file: {}", path.display()),
                        },
                    );
                }
                continue;
            }
            for path in matches.paths {
                if is_default_mux_config(&path) {
                    reload_config = true;
                } else {
                    let mut report = ConfigLoadReport::default();
                    if let Err(error) =
                        self.load_config_file_with_report(&path, context, 0, &mut report)
                    {
                        if source_file_error.is_none() {
                            source_file_error = Some(error);
                        }
                        continue;
                    }
                    self.apply_stored_mux_config_overrides("source-file-replay");
                    if let Some(summary) = report.summary() {
                        self.publish_to_client(
                            client,
                            EventPayload::ClientMessage {
                                pane: context.pane,
                                kind: ClientMessageKind::Warning,
                                text: summary,
                            },
                        );
                    }
                }
            }
        }
        if reload_config
            && let Err(error) = self.reload_user_config(client, context)
            && source_file_error.is_none()
        {
            source_file_error = Some(error);
        }
        self.publish_key_tables_if_changed();
        source_file_error.map_or(Ok(execution), Err)
    }

    fn publish_key_tables_if_changed(&self) {
        let tables = {
            let mut inner = self.inner.lock();
            let tables = inner.engine.keys.snapshot();
            if tables == inner.key_tables {
                return;
            }
            inner.key_tables.clone_from(&tables);
            tables
        };
        self.publish(EventPayload::KeyTablesChanged { tables });
    }

    fn capture_pane(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, DaemonError> {
        let parsed = parse_capture_pane_args(args)?;
        let (pane, terminal) = {
            let inner = self.inner.lock();
            let pane = inner.engine.resolve_pane(
                parsed.target.as_deref(),
                context.window,
                context.pane,
            )?;
            let terminal = inner
                .terminals
                .get(&pane)
                .cloned()
                .ok_or(ServerError::PaneExited(pane))?;
            (pane, terminal)
        };
        let output = match terminal.capture(parsed.options) {
            Ok(output) => output,
            Err(TerminalCaptureError::AlternateUnavailable) if parsed.quiet => String::new(),
            Err(TerminalCaptureError::ActorStopped) => {
                return Err(ServerError::PaneExited(pane).into());
            }
            Err(TerminalCaptureError::TimedOut) => {
                return Err(ServerError::Internal("terminal capture timed out".to_owned()).into());
            }
            Err(TerminalCaptureError::Failed(error)) => {
                return Err(
                    ServerError::Internal(format!("terminal capture failed: {error}")).into(),
                );
            }
            Err(error) => return Err(ServerError::InvalidCommand(error.to_string()).into()),
        };
        if let Some(buffer_name) = parsed.buffer_name.as_deref() {
            insert_paste_buffer(
                &mut self.inner.lock(),
                Some(buffer_name),
                "buffer",
                output.into_bytes(),
            )?;
            return Ok(Execution::default());
        }
        Ok(Execution {
            output,
            effects: Vec::new(),
        })
    }

    fn agent_send(
        self: &Arc<Self>,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, DaemonError> {
        let parsed = parse_agent_send_args(args)?;
        let payload = parsed.payload()?;
        let pane = self.resolve_agent_pane(context, parsed.target.as_deref())?;
        self.deliver_to_agent(pane, payload, parsed.submit)
    }

    fn send_last_output(
        self: &Arc<Self>,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, DaemonError> {
        let target = parse_target_only_args("send-last-output", args)?;
        let (pane, terminal) = {
            let inner = self.inner.lock();
            let pane =
                inner
                    .engine
                    .resolve_pane(target.as_deref(), context.window, context.pane)?;
            if !matches!(
                inner.engine.state.pane(pane).map(|pane| &pane.kind),
                Some(PaneKind::Terminal)
            ) {
                return Err(
                    ServerError::InvalidTarget(format!("{pane} is not a terminal pane")).into(),
                );
            }
            let terminal = inner
                .terminals
                .get(&pane)
                .cloned()
                .ok_or(ServerError::PaneExited(pane))?;
            (pane, terminal)
        };
        let capture = match terminal.capture_last_command() {
            Ok(capture) => capture,
            Err(TerminalCaptureError::NoSemanticMarks) => {
                return Err(ServerError::InvalidCommand(format!(
                    "{pane} has no shell-integration marks; send-last-output needs a shell that \
                     emits OSC 133 prompt marks (ghostty, kitty, wezterm, or starship shell \
                     integration all do)"
                ))
                .into());
            }
            Err(TerminalCaptureError::ActorStopped) => {
                return Err(ServerError::PaneExited(pane).into());
            }
            Err(TerminalCaptureError::TimedOut) => {
                return Err(ServerError::Internal("terminal capture timed out".to_owned()).into());
            }
            Err(error) => return Err(ServerError::Internal(error.to_string()).into()),
        };
        if capture.command.trim().is_empty() {
            return Err(ServerError::InvalidCommand(format!(
                "{pane} has not completed a command yet"
            ))
            .into());
        }
        let agent = self
            .inner
            .lock()
            .engine
            .state
            .recent_agent_pane(pane)
            .ok_or_else(|| {
                ServerError::MissingTarget(format!("no agent pane in the window holding {pane}"))
            })?;
        self.deliver_to_agent(agent, last_command_block(pane, &capture), false)?;
        Ok(Execution {
            output: format!("sent the last command from {pane} to {agent}"),
            effects: Vec::new(),
        })
    }

    fn capture_browser(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, DaemonError> {
        let parsed = parse_capture_browser_args(args)?;
        let output = parsed.output.ok_or_else(|| {
            ServerError::InvalidCommand("capture-browser needs an output path (-o)".to_owned())
        })?;
        let path = expand_path(&output);
        if !path.is_absolute() {
            return Err(ServerError::InvalidCommand(format!(
                "capture-browser output path must be absolute: {output}"
            ))
            .into());
        }
        let path = path
            .to_str()
            .ok_or_else(|| {
                ServerError::InvalidCommand(
                    "capture-browser output path is not valid UTF-8".to_owned(),
                )
            })?
            .to_owned();
        let pane = {
            let inner = self.inner.lock();
            let pane = inner.engine.resolve_pane(
                parsed.target.as_deref(),
                context.window,
                context.pane,
            )?;
            if !matches!(
                inner.engine.state.pane(pane).map(|pane| &pane.kind),
                Some(PaneKind::Browser(_))
            ) {
                return Err(
                    ServerError::InvalidTarget(format!("{pane} is not a browser pane")).into(),
                );
            }
            pane
        };
        let output = self.request_from_gui(pane, |request_id| EventPayload::BrowserCommand {
            pane,
            command: BrowserCommand::Screenshot { request_id, path },
        })?;
        Ok(Execution {
            output,
            effects: Vec::new(),
        })
    }

    fn resolve_agent_pane(
        &self,
        context: &ExecutionContext,
        target: Option<&str>,
    ) -> Result<PaneId, DaemonError> {
        let inner = self.inner.lock();
        let pane = inner
            .engine
            .resolve_pane(target, context.window, context.pane)?;
        match inner.engine.state.pane(pane).map(|pane| &pane.kind) {
            Some(PaneKind::Agent(_)) => Ok(pane),
            Some(_) => inner.engine.state.recent_agent_pane(pane).ok_or_else(|| {
                ServerError::MissingTarget(format!("no agent pane in the window holding {pane}"))
                    .into()
            }),
            None => Err(ServerError::MissingTarget(pane.to_string()).into()),
        }
    }

    fn deliver_to_agent(
        self: &Arc<Self>,
        pane: PaneId,
        text: String,
        submit: bool,
    ) -> Result<Execution, DaemonError> {
        // A submitted prompt is the daemon's own business now; only the
        // composer draft still belongs to whichever GUI owns the pane.
        #[cfg(feature = "agent")]
        if submit {
            if !self.submit_agent_prompt(pane, text) {
                return Err(ServerError::PaneExited(pane).into());
            }
            return Ok(Execution::default());
        }
        let command = if submit {
            AgentCommand::Prompt { text }
        } else {
            AgentCommand::ComposerAppend { text }
        };
        let output = self.request_from_gui(pane, move |request_id| EventPayload::AgentCommand {
            pane,
            request_id,
            command,
        })?;
        Ok(Execution {
            output,
            effects: Vec::new(),
        })
    }

    fn list_clients(
        &self,
        context: &ExecutionContext,
        name: &str,
        args: &[String],
    ) -> Result<Execution, DaemonError> {
        let parsed = parse_buffer_command_args(name, args, &['F', 't'], &[])?;
        require_no_positionals(name, &parsed)?;
        let mut inner = self.inner.lock();
        let target = parsed
            .value('t')
            .map(|target| {
                inner
                    .engine
                    .state
                    .resolve_session(Some(target), context.session)
            })
            .transpose()?;
        inner.engine.set_format_now(unix_timestamp());
        let mut clients = inner
            .attached
            .iter()
            .flat_map(|(session, clients)| {
                clients
                    .iter()
                    .copied()
                    .map(|client| (client, *session))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        clients.sort_by(|(left, _), (right, _)| {
            let left_name = inner
                .client_names
                .get(left)
                .cloned()
                .unwrap_or_else(|| format!("device-{}", left.0));
            let right_name = inner
                .client_names
                .get(right)
                .cloned()
                .unwrap_or_else(|| format!("device-{}", right.0));
            left_name.cmp(&right_name).then(left.0.cmp(&right.0))
        });
        let format = parsed.value('F').unwrap_or(
            "#{client_name}: #{session_name} [#{client_width}x#{client_height} #{client_termname}] #{?#{!=:#{client_uid},#{uid}},[user #{?client_user,#{client_user},#{client_uid},}] ,}#{?client_flags,(,}#{client_flags}#{?client_flags,),}",
        );
        let mut output = Vec::new();
        for (line, (client, session_id)) in clients.into_iter().enumerate() {
            if target.is_some_and(|target| target != session_id) {
                continue;
            }
            let session = inner
                .engine
                .state
                .sessions
                .get(&session_id)
                .ok_or_else(|| ServerError::MissingTarget(session_id.to_string()))?;
            let focused = client_focused_window(&inner, client, session);
            let format_context =
                inner
                    .engine
                    .format_status_context(Some(session_id), Some(focused), None);
            let facts = FormatHookFacts {
                client: Some(ClientFormatFacts {
                    name: inner
                        .client_names
                        .get(&client)
                        .cloned()
                        .unwrap_or_else(|| format!("device-{}", client.0)),
                    session: session.name.clone(),
                    width: 0,
                    height: 0,
                    termname: String::new(),
                    uid: format_context.uid.clone(),
                    user: format_context.user.clone(),
                    flags: String::new(),
                    theme: inner
                        .client_color_schemes
                        .get(&client)
                        .copied()
                        .unwrap_or_default()
                        .as_str()
                        .to_owned(),
                    line,
                }),
                ..FormatHookFacts::default()
            };
            let mut hooks = DaemonFormatHooks::command(&facts);
            output.push(expand_format_values(format, &format_context, &mut hooks));
        }
        Ok(Execution {
            output: output.join("\n"),
            effects: Vec::new(),
        })
    }

    fn show_messages(&self, name: &str, args: &[String]) -> Result<Execution, DaemonError> {
        let parsed = parse_buffer_command_args(name, args, &[], &[])?;
        require_no_positionals(name, &parsed)?;
        let mut inner = self.inner.lock();
        inner.engine.set_format_now(unix_timestamp());
        let context = inner.engine.format_status_context(None, None, None);
        let output = inner
            .message_log
            .iter()
            .rev()
            .map(|message| {
                let facts = FormatHookFacts {
                    message: Some(MessageFormatFacts {
                        number: message.number,
                        text: message.text.clone(),
                        time: message.time,
                    }),
                    ..FormatHookFacts::default()
                };
                let mut hooks = DaemonFormatHooks::command(&facts);
                expand_format_values("#{t/p:message_time}: #{message_text}", &context, &mut hooks)
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Execution {
            output,
            effects: Vec::new(),
        })
    }

    fn refresh_client(
        &self,
        client: ClientId,
        _kind: ClientKind,
        name: &str,
        args: &[String],
    ) -> Result<Execution, DaemonError> {
        let parsed = parse_buffer_command_args(
            name,
            args,
            &['A', 'B', 'C', 'F', 'f', 'r', 't'],
            &['c', 'D', 'l', 'L', 'R', 'S', 'U'],
        )?;
        if parsed.positional.len() > 1 {
            return Err(ServerError::InvalidCommand(
                "refresh-client accepts at most one adjustment".to_owned(),
            )
            .into());
        }
        if client_attached_session(&self.inner.lock(), client).is_none() {
            return Err(ServerError::InvalidCommand("no current client".to_owned()).into());
        }
        Err(
            ServerError::UnsupportedCommand("refresh-client interactive behavior".to_owned())
                .into(),
        )
    }

    fn buffer_command(
        &self,
        context: &ExecutionContext,
        name: &str,
        args: &[String],
    ) -> Result<Execution, DaemonError> {
        match name {
            "set-buffer" | "setb" => {
                let parsed = parse_buffer_command_args(name, args, &['b'], &['a'])?;
                let [data] = parsed.positional.as_slice() else {
                    return Err(ServerError::InvalidCommand(
                        "set-buffer requires exactly one data argument".to_owned(),
                    )
                    .into());
                };
                if data.is_empty() {
                    return Ok(Execution::default());
                }
                let requested_name = parsed.value('b');
                validate_paste_buffer_size(data.len())?;
                if let Some(name) = requested_name {
                    validate_paste_buffer_name(name)?;
                }
                let mut data = data.as_bytes().to_vec();
                let mut inner = self.inner.lock();
                if parsed.has('a')
                    && let Some(name) = requested_name
                    && let Some(buffer) = inner
                        .paste_buffers
                        .iter()
                        .find(|buffer| buffer.name == name)
                {
                    let combined = buffer.data.len().saturating_add(data.len());
                    validate_paste_buffer_size(combined)?;
                    let mut appended = Vec::with_capacity(combined);
                    appended.extend_from_slice(&buffer.data);
                    appended.append(&mut data);
                    data = appended;
                }
                insert_paste_buffer(&mut inner, requested_name, "buffer", data)?;
                drop(inner);
                self.refresh_choose_buffers();
                Ok(Execution::default())
            }
            "show-buffer" | "showb" => {
                let parsed = parse_buffer_command_args(name, args, &['b'], &[])?;
                require_no_positionals(name, &parsed)?;
                let (buffer_name, data, utf8) = {
                    let inner = self.inner.lock();
                    let buffer = resolve_buffer(&inner, parsed.value('b'))?;
                    (buffer.name.clone(), Arc::clone(&buffer.data), buffer.utf8)
                };
                if !utf8 {
                    return Err(ServerError::InvalidCommand(format!(
                        "buffer {buffer_name} contains non-UTF-8 bytes; use save-buffer"
                    ))
                    .into());
                }
                let output = String::from_utf8(data.as_ref().to_vec())
                    .expect("paste-buffer UTF-8 validity is cached at insertion");
                Ok(Execution {
                    output,
                    effects: Vec::new(),
                })
            }
            "list-buffers" | "lsb" => {
                let parsed = parse_buffer_command_args(name, args, &['F'], &[])?;
                require_no_positionals(name, &parsed)?;
                let mut inner = self.inner.lock();
                if let Some(format) = parsed.value('F') {
                    inner.engine.set_format_now(unix_timestamp());
                    let context = inner.engine.format_status_context(None, None, None);
                    let output = inner
                        .paste_buffers
                        .iter()
                        .map(|buffer| {
                            let facts = FormatHookFacts {
                                buffer: Some(buffer_format_facts(buffer)),
                                ..FormatHookFacts::default()
                            };
                            let mut hooks = DaemonFormatHooks::command(&facts);
                            expand_format_values(format, &context, &mut hooks)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    return Ok(Execution {
                        output,
                        effects: Vec::new(),
                    });
                }
                let now = SystemTime::now();
                let output = inner
                    .paste_buffers
                    .iter()
                    .map(|buffer| {
                        let age = now
                            .duration_since(buffer.created)
                            .unwrap_or_default()
                            .as_secs();
                        format!(
                            "{}: {} bytes: \"{}\" ({}s ago)",
                            buffer.name,
                            buffer.data.len(),
                            bounded_buffer_sample(&buffer.data, MAX_PASTE_BUFFER_SAMPLE_BYTES),
                            age
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(Execution {
                    output,
                    effects: Vec::new(),
                })
            }
            "load-buffer" | "loadb" => {
                let parsed = parse_buffer_command_args(name, args, &['b'], &[])?;
                let path = require_one_positional(name, &parsed)?;
                if path == "-" {
                    return Err(ServerError::UnsupportedCommand(
                        "load-buffer from standard input".to_owned(),
                    )
                    .into());
                }
                let path = expand_path(path);
                let data = read_paste_buffer_file(&path)?;
                if data.is_empty() {
                    return Ok(Execution::default());
                }
                if let Some(name) = parsed.value('b') {
                    validate_paste_buffer_name(name)?;
                }
                let mut inner = self.inner.lock();
                insert_paste_buffer(&mut inner, parsed.value('b'), "buffer", data)?;
                drop(inner);
                self.refresh_choose_buffers();
                Ok(Execution::default())
            }
            "save-buffer" | "saveb" => {
                let parsed = parse_buffer_command_args(name, args, &['b'], &['a'])?;
                let path = require_one_positional(name, &parsed)?;
                if path == "-" {
                    return Err(ServerError::UnsupportedCommand(
                        "save-buffer to standard output".to_owned(),
                    )
                    .into());
                }
                let data = {
                    let inner = self.inner.lock();
                    Arc::clone(&resolve_buffer(&inner, parsed.value('b'))?.data)
                };
                write_paste_buffer_file(&expand_path(path), &data, parsed.has('a'))?;
                Ok(Execution::default())
            }
            "delete-buffer" | "deleteb" => {
                let parsed = parse_buffer_command_args(name, args, &['b'], &[])?;
                require_no_positionals(name, &parsed)?;
                let mut inner = self.inner.lock();
                let name = resolve_buffer(&inner, parsed.value('b'))?.name.clone();
                inner.paste_buffers.retain(|buffer| buffer.name != name);
                drop(inner);
                self.refresh_choose_buffers();
                Ok(Execution::default())
            }
            "paste-buffer" | "pasteb" => {
                let parsed =
                    parse_buffer_command_args(name, args, &['b', 's', 't'], &['d', 'p', 'r', 'S'])?;
                require_no_positionals(name, &parsed)?;
                let pane = self.inner.lock().engine.resolve_pane(
                    parsed.value('t'),
                    context.window,
                    context.pane,
                )?;
                let separator = parsed.value('s').map_or_else(
                    || {
                        if parsed.has('r') {
                            b"\n".as_slice()
                        } else {
                            b"\r".as_slice()
                        }
                    },
                    str::as_bytes,
                );
                self.paste_buffer_to_pane(
                    pane,
                    PasteBufferPaste {
                        requested_name: parsed.value('b'),
                        delete: parsed.has('d'),
                        bracketed: parsed.has('p'),
                        separator,
                        literal: parsed.has('S'),
                        expected_client: None,
                    },
                )?;
                Ok(Execution::default())
            }
            _ => Err(ServerError::UnsupportedCommand(name.to_owned()).into()),
        }
    }

    fn paste_buffer_to_pane(
        &self,
        pane: PaneId,
        request: PasteBufferPaste<'_>,
    ) -> Result<(), DaemonError> {
        let PasteBufferPaste {
            requested_name,
            delete,
            bracketed,
            separator,
            literal,
            expected_client,
        } = request;
        let (client, sinks, buffer_name, data) = {
            let inner = self.inner.lock();
            let Some(buffer) = find_buffer(&inner, requested_name) else {
                if let Some(name) = requested_name {
                    return Err(ServerError::MissingTarget(name.to_owned()).into());
                }
                return Ok(());
            };
            let data = Arc::clone(&buffer.data);
            let buffer_name = buffer.name.clone();
            if expected_client
                .is_some_and(|expected| !client_is_attached_to_pane(&inner, expected, pane))
            {
                return Err(ServerError::PaneNotAttached(pane).into());
            }
            let sinks = resolve_input_sinks(&inner, pane)?;
            (expected_client, sinks, buffer_name, data)
        };
        let mut prepared = prepare_paste_buffer(&data, separator, literal)
            .map_err(|error| ServerError::InvalidCommand(error.to_string()))?;
        let has_terminal = sinks
            .iter()
            .any(|sink| matches!(sink, PaneSink::Terminal(_)));
        let browser_text = sinks
            .iter()
            .any(|sink| matches!(sink, PaneSink::Browser(_)))
            .then(|| {
                let bytes = if has_terminal {
                    prepared.clone()
                } else {
                    std::mem::take(&mut prepared)
                };
                String::from_utf8(bytes)
            })
            .transpose()
            .map_err(|_| {
                ServerError::InvalidCommand(format!(
                    "buffer {buffer_name} is not valid UTF-8 for a browser pane"
                ))
            })?;
        let prepared = has_terminal.then(|| Arc::<[u8]>::from(prepared));
        for sink in sinks {
            match sink {
                PaneSink::Terminal(terminal) => {
                    terminal.paste_prepared_bytes(
                        client.map(|client| TerminalViewId(client.0)),
                        Arc::clone(
                            prepared
                                .as_ref()
                                .expect("terminal paste retains prepared bytes"),
                        ),
                        bracketed,
                    );
                }
                PaneSink::Browser(target) => self.publish_for_pane(
                    target,
                    &EventPayload::BrowserCommand {
                        pane: target,
                        command: BrowserCommand::SendKeys(vec![zz_protocol::KeyToken::Literal(
                            browser_text
                                .as_ref()
                                .expect("browser text is validated before dispatch")
                                .clone(),
                        )]),
                    },
                ),
            }
        }
        let removed = delete && {
            let mut inner = self.inner.lock();
            let before = inner.paste_buffers.len();
            inner
                .paste_buffers
                .retain(|buffer| buffer.name != buffer_name || !Arc::ptr_eq(&buffer.data, &data));
            inner.paste_buffers.len() != before
        };
        if removed {
            self.refresh_choose_buffers();
        }
        Ok(())
    }

    fn attach(&self, client: ClientId, session: SessionId) -> Result<MuxSnapshot, ServerError> {
        let mut inner = self.inner.lock();
        if !inner.engine.state.sessions.contains_key(&session) {
            return Err(ServerError::MissingTarget(session.to_string()));
        }
        let previous_sessions = inner
            .attached
            .iter()
            .filter_map(|(attached_session, clients)| {
                (clients.contains(&client) && *attached_session != session)
                    .then_some(*attached_session)
            })
            .collect::<Vec<_>>();
        let switches_session = !previous_sessions.is_empty();
        let previous = previous_sessions
            .iter()
            .flat_map(|attached_session| session_terminals(&inner, *attached_session))
            .collect::<Vec<_>>();
        let command_output = switches_session
            .then(|| take_command_output(&mut inner, client))
            .flatten();
        let choose_tree_closed = switches_session && inner.choose_trees.remove(&client).is_some();
        let choose_buffer_closed =
            switches_session && inner.choose_buffers.remove(&client).is_some();
        let display_panes_closed =
            switches_session && take_display_panes(&mut inner, client).is_some();
        let mut affected_panes = inner
            .visible_terminals
            .get(&client)
            .cloned()
            .unwrap_or_default();
        if switches_session {
            affected_panes.extend(remove_client_terminal_geometries(&mut inner, client));
            inner.focused_windows.remove(&client);
        }
        for (attached_session, clients) in &mut inner.attached {
            if *attached_session != session {
                clients.remove(&client);
            }
        }
        inner.attached.retain(|_, clients| !clients.is_empty());
        inner.attached.entry(session).or_default().insert(client);
        inner.engine.mark_session_active(session);
        let visible = visible_terminal_panes(&inner, client, session);
        affected_panes.extend(visible.iter().copied());
        inner.visible_terminals.insert(client, visible);
        #[cfg(feature = "agent")]
        {
            let visible = visible_agent_panes(&inner, client, session);
            inner.visible_agents.insert(client, visible);
        }
        let terminals = session_terminals(&inner, session);
        let unfocused_copy_mode_exits = unfocused_copy_sessions(&mut inner);
        let resizes = terminal_resizes_for_panes(&inner, &affected_panes);
        let mut snapshot = inner.engine.state.snapshot();
        let presence = snapshot_presence(&inner);
        stamp_snapshot_for_client(&inner, client, &mut snapshot, &presence);
        drop(inner);
        if let Some(output) = command_output {
            Self::retire_command_output(client, output);
        }
        if choose_tree_closed {
            self.publish_to_client(client, EventPayload::ChooseTree { state: None });
        }
        if choose_buffer_closed {
            self.publish_to_client(client, EventPayload::ChooseBuffer { state: None });
        }
        if display_panes_closed {
            self.publish_to_client(client, EventPayload::DisplayPanes { state: None });
        }
        let view = TerminalViewId(client.0);
        for (frozen_client, terminal) in unfocused_copy_mode_exits {
            terminal.view_action(
                TerminalViewId(frozen_client.0),
                zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel),
            );
        }
        for terminal in previous {
            terminal.detach_view(view);
        }
        for terminal in terminals {
            terminal.attach_view(view);
        }
        apply_terminal_resizes(resizes);
        Ok(snapshot)
    }

    fn attach_target(
        &self,
        client: ClientId,
        target: &str,
    ) -> Result<(SessionId, MuxSnapshot), ServerError> {
        let session = {
            let inner = self.inner.lock();
            inner.engine.state.resolve_session(
                (!target.is_empty()).then_some(target),
                inner
                    .engine
                    .state
                    .default_context()
                    .map(|context| context.0),
            )?
        };
        let snapshot = self.attach(client, session)?;
        Ok((session, snapshot))
    }

    fn detach(&self, client: ClientId) {
        if self.detach_client_state(client) {
            self.publish_snapshot();
        }
    }

    /// Detach every attached client but `stealer`, either across the server or
    /// on one session.
    fn evict_clients(self: &Arc<Self>, session: Option<SessionId>, stealer: ClientId) {
        let (victims, by) = {
            let inner = self.inner.lock();
            let victims = inner
                .attached
                .iter()
                .filter(|(attached, _)| session.is_none_or(|wanted| wanted == **attached))
                .flat_map(|(attached, clients)| {
                    clients.iter().map(move |client| (*attached, *client))
                })
                .filter(|(_, victim)| *victim != stealer)
                .collect::<Vec<_>>();
            let by = inner
                .client_names
                .get(&stealer)
                .cloned()
                .unwrap_or_else(|| format!("device-{}", stealer.0));
            (victims, by)
        };
        let evicted = !victims.is_empty();
        for (session, victim) in victims {
            self.publish_to_client(
                victim,
                EventPayload::Detached {
                    session,
                    by: Some(by.clone()),
                },
            );
            let _ = self.detach_client_state(victim);
        }
        if evicted {
            self.publish_snapshot();
        }
    }

    fn detach_client_state(&self, client: ClientId) -> bool {
        let mut inner = self.inner.lock();
        let sessions = inner
            .attached
            .iter()
            .filter_map(|(session, clients)| clients.contains(&client).then_some(*session))
            .collect::<Vec<_>>();
        let was_attached = !sessions.is_empty();
        let terminals = sessions
            .iter()
            .flat_map(|session| session_terminals(&inner, *session))
            .collect::<Vec<_>>();
        let mut affected_panes = inner
            .visible_terminals
            .get(&client)
            .cloned()
            .unwrap_or_default();
        affected_panes.extend(remove_client_terminal_geometries(&mut inner, client));
        for clients in inner.attached.values_mut() {
            clients.remove(&client);
        }
        inner.attached.retain(|_, clients| !clients.is_empty());
        inner.visible_terminals.remove(&client);
        inner.visible_agents.remove(&client);
        inner.focused_windows.remove(&client);
        inner.client_terminal_input_sequences.remove(&client);
        inner.key_engines.remove(&client);
        let copy_session = inner
            .copy_sessions
            .remove(&client)
            .and_then(|session| inner.terminals.get(&session.pane).cloned());
        let prefix_was_armed = inner.prefix_armed.remove(&client);
        inner.swallowed_keys.remove(&client);
        inner.suppressed_text.remove(&client);
        inner.command_prompts.remove(&client);
        inner.choose_trees.remove(&client);
        inner.choose_buffers.remove(&client);
        let _ = take_display_panes(&mut inner, client);
        let command_output = take_command_output(&mut inner, client);
        let resizes = terminal_resizes_for_panes(&inner, &affected_panes);
        drop(inner);
        self.fail_gui_requests_for(client);
        let view = TerminalViewId(client.0);
        if let Some(command_output) = command_output {
            Self::retire_command_output(client, command_output);
        }
        if let Some(terminal) = copy_session {
            terminal.view_action(
                view,
                zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel),
            );
        }
        for terminal in terminals {
            terminal.detach_view(view);
        }
        apply_terminal_resizes(resizes);
        if prefix_was_armed {
            self.publish_to_client(client, EventPayload::PrefixArmed { armed: false });
        }
        was_attached
    }

    fn input(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        input: InputMessage,
    ) -> Result<(), DaemonError> {
        let started = diagnostic_timer();
        log::trace!(
            target: "zz_daemon::diagnostics::input",
            "dispatch begin client={client} kind={kind:?} context={context:#?} input={input:#?}"
        );
        let generation = self.inner.lock().engine.state.generation();
        let resize_split = matches!(&input, InputMessage::ResizeSplit { .. });
        let result = (|| -> Result<(), DaemonError> {
            match input {
                InputMessage::Text { pane, text } => {
                    self.reject_unattached_input(client, pane)?;
                    self.note_terminal_input(client, pane);
                    self.input_text(client, kind, context, pane, &text)?;
                }
                InputMessage::Key {
                    pane,
                    input,
                    text_follows,
                } => {
                    self.reject_unattached_input(client, pane)?;
                    self.note_terminal_input(client, pane);
                    self.input_key(client, kind, context, pane, input, text_follows)?;
                }
                InputMessage::BrowserSurfaceText { pane, text } => {
                    self.reject_invalid_browser_surface_input(client, pane)?;
                    self.note_terminal_input(client, pane);
                    self.input_browser_surface_text(client, pane, &text)?;
                }
                InputMessage::BrowserSurfaceKey {
                    pane,
                    input,
                    text_follows,
                } => {
                    self.reject_invalid_browser_surface_input(client, pane)?;
                    self.note_terminal_input(client, pane);
                    self.input_browser_surface_key(
                        client,
                        kind,
                        context,
                        pane,
                        input,
                        text_follows,
                    )?;
                }
                InputMessage::ResizeTerminal {
                    pane,
                    columns,
                    rows,
                    cell_width_px,
                    cell_height_px,
                } => {
                    let resize = {
                        let mut inner = self.inner.lock();
                        if !inner.terminals.contains_key(&pane) {
                            return Err(ServerError::PaneExited(pane).into());
                        }
                        if !client_is_attached_to_pane(&inner, client, pane) {
                            return Err(ServerError::PaneNotAttached(pane).into());
                        }
                        inner.terminal_geometries.entry(pane).or_default().insert(
                            client,
                            TerminalGeometry {
                                columns,
                                rows,
                                cell_width_px,
                                cell_height_px,
                            },
                        );
                        let resize = terminal_resize_for_pane(&inner, pane);
                        if let Some((_, geometry)) = &resize {
                            inner
                                .engine
                                .set_pane_geometry(pane, geometry.columns, geometry.rows);
                        }
                        resize
                    };
                    if let Some((terminal, geometry)) = resize {
                        terminal.resize(
                            geometry.columns,
                            geometry.rows,
                            geometry.cell_width_px,
                            geometry.cell_height_px,
                        );
                    }
                }
                InputMessage::TerminalView {
                    pane,
                    action: zz_terminal::TerminalViewAction::Paste(text),
                } => {
                    let modal_active = {
                        let inner = self.inner.lock();
                        inner.choose_trees.contains_key(&client)
                            || inner.choose_buffers.contains_key(&client)
                            || inner.display_panes.contains_key(&client)
                    };
                    if !modal_active {
                        self.note_terminal_input(client, pane);
                        self.input_paste(client, pane, &text)?;
                    }
                }
                InputMessage::TerminalView { pane, action } => {
                    let terminal = {
                        let inner = self.inner.lock();
                        if inner.choose_trees.contains_key(&client)
                            || inner.choose_buffers.contains_key(&client)
                            || inner.display_panes.contains_key(&client)
                        {
                            None
                        } else {
                            if !client_is_attached_to_pane(&inner, client, pane) {
                                return Err(ServerError::PaneNotAttached(pane).into());
                            }
                            Some(
                                inner
                                    .terminals
                                    .get(&pane)
                                    .cloned()
                                    .ok_or(ServerError::PaneExited(pane))?,
                            )
                        }
                    };
                    if let Some(terminal) = terminal {
                        if terminal_view_action_is_input(&action) {
                            self.note_terminal_input(client, pane);
                        }
                        sync_copy_session_for_view_action(
                            &mut self.inner.lock(),
                            client,
                            pane,
                            &action,
                        )?;
                        terminal.view_action(TerminalViewId(client.0), action);
                    }
                }
                InputMessage::ResizeCommandOutput {
                    columns,
                    rows,
                    cell_width_px,
                    cell_height_px,
                } => {
                    let terminal = self
                        .inner
                        .lock()
                        .command_outputs
                        .get(&client)
                        .map(|output| Arc::clone(&output.terminal));
                    if let Some(terminal) = terminal {
                        terminal.resize(columns, rows, cell_width_px, cell_height_px);
                    }
                }
                InputMessage::CommandOutputView { action } => {
                    let terminal = self
                        .inner
                        .lock()
                        .command_outputs
                        .get(&client)
                        .map(|output| Arc::clone(&output.terminal));
                    if let Some(terminal) = terminal {
                        terminal.view_action(TerminalViewId(client.0), action);
                    }
                }
                InputMessage::ChooseTree { action } => {
                    self.input_choose_tree(client, kind, context, action)?;
                }
                InputMessage::ChooseBuffer { action } => {
                    self.input_choose_buffer(client, kind, action)?;
                }
                InputMessage::DisplayPanes { action } => {
                    self.input_display_panes(client, kind, context, action)?;
                }
                InputMessage::CommandPrompt { action } => {
                    self.input_command_prompt_action(client, kind, context, action)?;
                }
                InputMessage::ResizeSplit {
                    window,
                    split,
                    ratio_basis_points,
                } => {
                    self.input_resize_split(client, kind, window, split, ratio_basis_points)?;
                }
            }
            Ok(())
        })();
        let publish_snapshot = {
            let inner = self.inner.lock();
            let current = inner.engine.state.generation();
            resize_split && result.is_ok()
                || current != generation && inner.last_published_mux_generation != current
        };
        if publish_snapshot {
            self.publish_snapshot();
        }
        log::trace!(
            target: "zz_daemon::diagnostics::input",
            "dispatch end client={client} success={} elapsed_us={} context={context:#?}",
            result.is_ok(),
            diagnostic_elapsed_us(started),
        );
        result
    }

    fn reject_unattached_input(&self, client: ClientId, pane: PaneId) -> Result<(), ServerError> {
        if client_attached_session(&self.inner.lock(), client).is_some() {
            Ok(())
        } else {
            Err(ServerError::PaneNotAttached(pane))
        }
    }

    fn reject_invalid_browser_surface_input(
        &self,
        client: ClientId,
        pane: PaneId,
    ) -> Result<(), ServerError> {
        let inner = self.inner.lock();
        if !client_is_attached_to_pane(&inner, client, pane) {
            return Err(ServerError::PaneNotAttached(pane));
        }
        match inner.engine.state.pane(pane).map(|pane| &pane.kind) {
            Some(PaneKind::Browser(_)) => Ok(()),
            Some(_) => Err(ServerError::InvalidTarget(format!(
                "{pane} is not a browser pane"
            ))),
            None => Err(ServerError::PaneExited(pane)),
        }
    }

    fn note_terminal_input(&self, client: ClientId, pane: PaneId) {
        let resizes = {
            let mut inner = self.inner.lock();
            let session = client_is_attached_to_pane(&inner, client, pane)
                .then(|| inner.engine.state.window_for_pane(pane))
                .flatten()
                .and_then(|window| inner.engine.state.windows.get(&window))
                .map(|window| window.session);
            if let Some(session) = session {
                inner.engine.mark_session_active(session);
            }
            terminal_resizes_after_client_input(&mut inner, client, pane)
        };
        apply_terminal_resizes(resizes);
        if self.clear_pane_bell(pane) {
            self.publish_snapshot();
        }
    }

    fn input_resize_split(
        &self,
        client: ClientId,
        kind: ClientKind,
        window: WindowId,
        split: SplitId,
        ratio_basis_points: u16,
    ) -> Result<(), DaemonError> {
        if kind != ClientKind::Interactive {
            return Err(ServerError::InvalidCommand(
                "split dragging requires an interactive client".to_owned(),
            )
            .into());
        }
        if ratio_basis_points > SPLIT_RATIO_BASIS {
            return Err(ServerError::InvalidCommand(format!(
                "split ratio must be between 0 and {SPLIT_RATIO_BASIS}"
            ))
            .into());
        }
        {
            let mut inner = self.inner.lock();
            if !inner.subscribers.contains_key(&client) {
                return Err(ServerError::InvalidCommand(
                    "split dragging requires a subscribed client".to_owned(),
                )
                .into());
            }
            let attached = client_attached_session(&inner, client).ok_or_else(|| {
                ServerError::InvalidTarget("client is not attached to a session".to_owned())
            })?;
            let active_window = inner
                .engine
                .state
                .sessions
                .get(&attached)
                .map(|session| client_focused_window(&inner, client, session))
                .ok_or_else(|| {
                    ServerError::InvalidTarget("attached session no longer exists".to_owned())
                })?;
            let target = inner
                .engine
                .state
                .windows
                .get(&window)
                .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
            if target.session != attached || active_window != window {
                return Err(ServerError::InvalidTarget(format!(
                    "window {window} is not active for the attached client"
                ))
                .into());
            }
            if target.zoomed_pane.is_some() {
                return Err(ServerError::InvalidCommand(
                    "cannot drag a split while its window is zoomed".to_owned(),
                )
                .into());
            }
            if !target.layout.project().contains_split(split) {
                return Err(ServerError::MissingTarget(split.to_string()).into());
            }
            inner.engine.state.resize_split(
                window,
                split,
                f32::from(ratio_basis_points) / f32::from(SPLIT_RATIO_BASIS),
            )?;
        }
        Ok(())
    }

    fn input_text(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        pane: PaneId,
        text: &str,
    ) -> Result<(), DaemonError> {
        let filtered_text = self.filter_suppressed_text(client, text);
        let text = filtered_text.as_ref();
        {
            let inner = self.inner.lock();
            if inner.choose_trees.contains_key(&client)
                || inner.choose_buffers.contains_key(&client)
                || inner.display_panes.contains_key(&client)
            {
                return Ok(());
            }
        }
        if text.is_empty() || self.input_command_prompt_text(client, text) {
            return Ok(());
        }
        let command_output_active = self.inner.lock().command_outputs.contains_key(&client);
        let mut sinks = None;
        let mut pass_start = None;
        for (offset, character) in text.char_indices() {
            let mut encoded = [0_u8; 4];
            let decision = self.key_decision(client, character.encode_utf8(&mut encoded), false);
            match decision {
                KeyDecision::Pass => {
                    if command_output_active
                        || self.inner.lock().command_outputs.contains_key(&client)
                    {
                        if let Some(start) = pass_start.take() {
                            self.dispatch_input_text(
                                client,
                                pane,
                                &mut sinks,
                                &text[start..offset],
                            )?;
                        }
                        continue;
                    }
                    if pass_start.is_none() {
                        pass_start = Some(offset);
                    }
                }
                KeyDecision::Prefix | KeyDecision::Ignore => {
                    if let Some(start) = pass_start.take() {
                        self.dispatch_input_text(client, pane, &mut sinks, &text[start..offset])?;
                    }
                }
                KeyDecision::Commands(commands) => {
                    if let Some(start) = pass_start.take() {
                        self.dispatch_input_text(client, pane, &mut sinks, &text[start..offset])?;
                    }
                    self.execute_key_commands(client, kind, context, pane, &commands)?;
                    sinks = None;
                    if self.inner.lock().command_prompts.contains_key(&client) {
                        let remaining = &text[offset + character.len_utf8()..];
                        if !remaining.is_empty() {
                            self.input_command_prompt_text(client, remaining);
                        }
                        break;
                    }
                }
            }
        }
        if let Some(start) = pass_start {
            self.dispatch_input_text(client, pane, &mut sinks, &text[start..])?;
        }
        self.sync_prefix_armed(client);
        Ok(())
    }

    fn input_browser_surface_text(
        &self,
        client: ClientId,
        pane: PaneId,
        text: &str,
    ) -> Result<(), DaemonError> {
        let blocked = {
            let inner = self.inner.lock();
            inner.choose_trees.contains_key(&client)
                || inner.choose_buffers.contains_key(&client)
                || inner.display_panes.contains_key(&client)
                || inner.command_outputs.contains_key(&client)
        };
        if blocked || text.is_empty() || self.input_command_prompt_text(client, text) {
            return Ok(());
        }
        self.dispatch_input_text(client, pane, &mut None, text)
            .map_err(Into::into)
    }

    fn dispatch_input_text(
        &self,
        client: ClientId,
        pane: PaneId,
        sinks: &mut Option<Vec<PaneSink>>,
        text: &str,
    ) -> Result<(), ServerError> {
        if text.is_empty() {
            return Ok(());
        }
        let sinks = if let Some(sinks) = sinks.as_ref() {
            sinks
        } else {
            sinks.insert(self.input_sinks(client, pane)?)
        };
        let terminal_text = sinks
            .iter()
            .any(|sink| matches!(sink, PaneSink::Terminal(_)))
            .then(|| Arc::<str>::from(text));
        for sink in sinks {
            match sink {
                PaneSink::Terminal(terminal) => terminal.send_text_for_view(
                    TerminalViewId(client.0),
                    Arc::clone(
                        terminal_text
                            .as_ref()
                            .expect("terminal text is allocated when a terminal sink exists"),
                    ),
                ),
                PaneSink::Browser(target) => self.publish_for_pane(
                    *target,
                    &EventPayload::BrowserCommand {
                        pane: *target,
                        command: BrowserCommand::SendKeys(vec![zz_protocol::KeyToken::Literal(
                            text.to_owned(),
                        )]),
                    },
                ),
            }
        }
        Ok(())
    }

    fn input_key(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        pane: PaneId,
        input: zz_terminal::KeyInput,
        text_follows: bool,
    ) -> Result<(), DaemonError> {
        let modal_active = {
            let inner = self.inner.lock();
            inner.choose_trees.contains_key(&client)
                || inner.choose_buffers.contains_key(&client)
                || inner.display_panes.contains_key(&client)
        };
        if modal_active {
            if input.action == zz_terminal::KeyAction::Release {
                let _ = self.key_decision(client, &input_key_name(&input), true);
            }
            return Ok(());
        }
        if self.input_command_prompt_key(client, kind, context, &input, text_follows) {
            return Ok(());
        }
        if input.action != zz_terminal::KeyAction::Release
            && self.inner.lock().engine.dead_pane_dismisses_on_key(pane)
        {
            let target = pane.to_string();
            self.execute(
                client,
                kind,
                context,
                &CommandInvocation::new("kill-pane", ["-t", target.as_str()]),
            )?;
            return Ok(());
        }
        let key = input_key_name(&input);
        let decision = self.key_decision(
            client,
            &key,
            input.action == zz_terminal::KeyAction::Release,
        );
        if decision != KeyDecision::Pass
            && input.action != zz_terminal::KeyAction::Release
            && text_follows
            && input.modifiers == zz_terminal::Modifiers::default()
            && let zz_terminal::KeyCode::Character(character) = input.key
        {
            *self
                .inner
                .lock()
                .suppressed_text
                .entry(client)
                .or_default()
                .entry(character)
                .or_default() += 1;
        }
        let result = match decision {
            KeyDecision::Pass => {
                if self.inner.lock().command_outputs.contains_key(&client) {
                    return Ok(());
                }
                self.dispatch_input_key(client, pane, input)
                    .map_err(Into::into)
            }
            KeyDecision::Prefix | KeyDecision::Ignore => Ok(()),
            KeyDecision::Commands(commands) => {
                self.execute_key_commands(client, kind, context, pane, &commands)
            }
        };
        self.sync_prefix_armed(client);
        result
    }

    fn input_browser_surface_key(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        pane: PaneId,
        input: zz_terminal::KeyInput,
        text_follows: bool,
    ) -> Result<(), DaemonError> {
        let blocked = {
            let inner = self.inner.lock();
            inner.choose_trees.contains_key(&client)
                || inner.choose_buffers.contains_key(&client)
                || inner.display_panes.contains_key(&client)
        };
        if blocked || self.input_command_prompt_key(client, kind, context, &input, text_follows) {
            return Ok(());
        }
        if self.inner.lock().command_outputs.contains_key(&client) {
            return Ok(());
        }
        self.dispatch_input_key(client, pane, input)
            .map_err(Into::into)
    }

    fn dispatch_input_key(
        &self,
        client: ClientId,
        pane: PaneId,
        input: zz_terminal::KeyInput,
    ) -> Result<(), ServerError> {
        let mut sinks = self.input_sinks(client, pane)?.into_iter().peekable();
        let mut owned_input = Some(input);
        while let Some(sink) = sinks.next() {
            let sink_input = if sinks.peek().is_none() {
                owned_input.take().expect("last sink owns key input")
            } else {
                owned_input
                    .as_ref()
                    .expect("key input is retained until the last sink")
                    .clone()
            };
            match sink {
                PaneSink::Terminal(terminal) => {
                    terminal.send_key_for_view(TerminalViewId(client.0), sink_input);
                }
                PaneSink::Browser(target) => self.publish_for_pane(
                    target,
                    &EventPayload::BrowserCommand {
                        pane: target,
                        command: BrowserCommand::Key(sink_input),
                    },
                ),
            }
        }
        Ok(())
    }

    fn input_choose_tree(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        action: ChooseTreeAction,
    ) -> Result<(), DaemonError> {
        if kind != ClientKind::Interactive {
            return Err(ServerError::InvalidCommand(
                "choose-tree input requires an interactive client".to_owned(),
            )
            .into());
        }
        let (result, state, delta) = {
            let mut inner = self.inner.lock();
            let Some(mut chooser) = inner.choose_trees.remove(&client) else {
                return Ok(());
            };
            let action = match action {
                ChooseTreeAction::Key(input) => {
                    let searching = chooser.search.is_some();
                    let Some(action) =
                        choose_tree_key_action(&inner.engine.keys, &input, searching)
                    else {
                        inner.choose_trees.insert(client, chooser);
                        return Ok(());
                    };
                    action
                }
                action => action,
            };
            let attached_session = client_attached_session(&inner, client);
            let result = match chooser.apply(action, &inner.engine.state, attached_session) {
                Ok(result) => result,
                Err(error) => {
                    inner.choose_trees.insert(client, chooser);
                    return Err(error.into());
                }
            };
            let state = (result == ChooseTreeResult::Updated(ChooseTreeUpdateKind::Full))
                .then(|| chooser.rendered.clone());
            let delta = (result == ChooseTreeResult::Updated(ChooseTreeUpdateKind::Delta))
                .then(|| (chooser.rendered.selected, chooser.rendered.search.clone()));
            if matches!(result, ChooseTreeResult::Updated(_)) {
                inner.choose_trees.insert(client, chooser);
            }
            (result, state, delta)
        };

        match result {
            ChooseTreeResult::Updated(ChooseTreeUpdateKind::Full) => {
                self.publish_to_client(client, EventPayload::ChooseTree { state });
            }
            ChooseTreeResult::Updated(ChooseTreeUpdateKind::Delta) => {
                let (selected, search) = delta.expect("updated chooser retains cursor state");
                self.publish_to_client(client, EventPayload::ChooseTreeUpdate { search, selected });
            }
            ChooseTreeResult::Close => {
                self.publish_to_client(client, EventPayload::ChooseTree { state: None });
            }
            ChooseTreeResult::Activate(target) => {
                self.publish_to_client(client, EventPayload::ChooseTree { state: None });
                self.activate_choose_tree_target(client, kind, context, target)?;
            }
        }
        Ok(())
    }

    fn input_choose_buffer(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        action: ChooseBufferAction,
    ) -> Result<(), DaemonError> {
        if kind != ClientKind::Interactive {
            return Err(ServerError::InvalidCommand(
                "choose-buffer input requires an interactive client".to_owned(),
            )
            .into());
        }

        let outcome = {
            let mut inner = self.inner.lock();
            let Some(mut chooser) = inner.choose_buffers.remove(&client) else {
                return Ok(());
            };
            let action = match action {
                ChooseBufferAction::Key(input) => {
                    let searching = chooser.search.is_some();
                    let Some(action) =
                        choose_buffer_key_action(&inner.engine.keys, &input, searching)
                    else {
                        inner.choose_buffers.insert(client, chooser);
                        return Ok(());
                    };
                    action
                }
                action => action,
            };
            let attached_session = client_attached_session(&inner, client);
            let source_session = inner
                .engine
                .state
                .window_for_pane(chooser.source_pane)
                .map(|window| inner.engine.state.windows[&window].session);
            if attached_session != Some(chooser.source_session)
                || source_session != Some(chooser.source_session)
            {
                ChooseBufferInputOutcome::Close
            } else {
                let result = match chooser.apply(action, &inner.paste_buffers) {
                    Ok(result) => result,
                    Err(error) => {
                        inner.choose_buffers.insert(client, chooser);
                        return Err(error.into());
                    }
                };
                match result {
                    ChooseBufferResult::Updated => {
                        let search = chooser.rendered.search.clone();
                        let selected = chooser.rendered.selected;
                        inner.choose_buffers.insert(client, chooser);
                        ChooseBufferInputOutcome::Delta { search, selected }
                    }
                    ChooseBufferResult::Delete(name) => {
                        let deleted_index = usize::try_from(chooser.rendered.selected)
                            .unwrap_or(usize::MAX)
                            .min(inner.paste_buffers.len().saturating_sub(1));
                        inner.paste_buffers.retain(|buffer| buffer.name != name);
                        chooser.selected = inner
                            .paste_buffers
                            .get(deleted_index)
                            .or_else(|| inner.paste_buffers.last())
                            .map(|buffer| buffer.name.clone());
                        chooser.rebuild(&inner.paste_buffers);
                        if chooser.rendered.items.is_empty() {
                            ChooseBufferInputOutcome::Full(None)
                        } else {
                            let state = chooser.rendered.clone();
                            inner.choose_buffers.insert(client, chooser);
                            ChooseBufferInputOutcome::Full(Some(state))
                        }
                    }
                    ChooseBufferResult::Paste(name) => ChooseBufferInputOutcome::Paste {
                        pane: chooser.source_pane,
                        name,
                    },
                    ChooseBufferResult::Close => ChooseBufferInputOutcome::Close,
                }
            }
        };

        match outcome {
            ChooseBufferInputOutcome::Delta { search, selected } => {
                self.publish_to_client(
                    client,
                    EventPayload::ChooseBufferUpdate { search, selected },
                );
            }
            ChooseBufferInputOutcome::Full(state) => {
                self.publish_to_client(client, EventPayload::ChooseBuffer { state });
                self.refresh_choose_buffers_except(Some(client));
            }
            ChooseBufferInputOutcome::Paste { pane, name } => {
                self.publish_to_client(client, EventPayload::ChooseBuffer { state: None });
                self.paste_buffer_to_pane(
                    pane,
                    PasteBufferPaste {
                        requested_name: Some(&name),
                        delete: false,
                        bracketed: true,
                        separator: b"\r",
                        literal: false,
                        expected_client: Some(client),
                    },
                )?;
            }
            ChooseBufferInputOutcome::Close => {
                self.publish_to_client(client, EventPayload::ChooseBuffer { state: None });
            }
        }
        Ok(())
    }

    fn input_display_panes(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        action: DisplayPanesAction,
    ) -> Result<(), DaemonError> {
        if kind != ClientKind::Interactive {
            return Err(ServerError::InvalidCommand(
                "display-panes input requires an interactive client".to_owned(),
            )
            .into());
        }
        if matches!(
            &action,
            DisplayPanesAction::Key(input) if input.action == zz_terminal::KeyAction::Release
        ) {
            return Ok(());
        }

        let Some(overlay) = ({
            let mut inner = self.inner.lock();
            take_display_panes(&mut inner, client)
        }) else {
            return Ok(());
        };
        self.publish_to_client(client, EventPayload::DisplayPanes { state: None });

        let attached = client_attached_session(&self.inner.lock(), client);
        if attached != Some(overlay.source_session) {
            return Ok(());
        }
        match action {
            DisplayPanesAction::Close => {
                self.inner
                    .lock()
                    .swallowed_keys
                    .entry(client)
                    .or_default()
                    .insert("Escape".to_owned());
            }
            DisplayPanesAction::Select(pane) => {
                if overlay
                    .state
                    .indicators
                    .iter()
                    .any(|indicator| indicator.pane == pane)
                {
                    self.select_display_pane(client, kind, context, pane)?;
                }
            }
            DisplayPanesAction::Key(input) => {
                let selected = display_panes_selection_key(&input).and_then(|key| {
                    overlay
                        .state
                        .indicators
                        .iter()
                        .find(|indicator| indicator.select_key == key)
                        .map(|indicator| indicator.pane)
                });
                if let Some(pane) = selected {
                    self.inner
                        .lock()
                        .swallowed_keys
                        .entry(client)
                        .or_default()
                        .insert(input_key_name(&input).into_string());
                    self.select_display_pane(client, kind, context, pane)?;
                } else if self
                    .inner
                    .lock()
                    .engine
                    .state
                    .window_for_pane(overlay.source_pane)
                    .is_some()
                {
                    self.input_key(client, kind, context, overlay.source_pane, input, false)?;
                }
            }
        }
        Ok(())
    }

    fn select_display_pane(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        pane: PaneId,
    ) -> Result<(), DaemonError> {
        let zoomed = {
            let inner = self.inner.lock();
            let window = inner
                .engine
                .state
                .window_for_pane(pane)
                .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
            inner.engine.state.windows[&window].zoomed_pane.is_some()
        };
        if zoomed {
            self.execute_gesture(
                client,
                kind,
                context,
                "display_panes_select",
                &CommandInvocation::new("resize-pane", ["-Z", "-t", &pane.to_string()]),
            )?;
        }
        self.execute_gesture(
            client,
            kind,
            context,
            "display_panes_select",
            &CommandInvocation::new("select-pane", ["-t", &pane.to_string()]),
        )
    }

    fn execute_gesture(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        gesture: &str,
        command: &CommandInvocation,
    ) -> Result<(), DaemonError> {
        let Err(error) = self.execute(client, kind, context, command) else {
            return Ok(());
        };
        log::warn!(
            target: "zz_daemon::diagnostics::input",
            "gesture_command_failed client={client} gesture={gesture} command={} args={:?} error={error}",
            command.name,
            command.args,
        );
        self.publish_to_client(
            client,
            EventPayload::ClientMessage {
                pane: context.pane,
                kind: ClientMessageKind::Error,
                text: error.to_string(),
            },
        );
        Err(error)
    }

    fn expire_display_panes(&self, scheduled: DisplayPanesDeadline, now: Instant) -> bool {
        let mut inner = self.inner.lock();
        let due = inner
            .display_panes
            .get(&scheduled.client)
            .is_some_and(|overlay| {
                overlay.token == scheduled.token
                    && overlay.deadline == Some(scheduled.deadline)
                    && scheduled.deadline <= now
            });
        if !due {
            return false;
        }
        inner.display_panes.remove(&scheduled.client);
        if let Some(outbound) = inner.subscribers.get(&scheduled.client) {
            Self::send_event(outbound, EventPayload::DisplayPanes { state: None });
        }
        true
    }

    fn activate_choose_tree_target(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        target: ChooseTreeTarget,
    ) -> Result<(), DaemonError> {
        let (selection, session) = {
            let inner = self.inner.lock();
            let (selection, session) = match target {
                ChooseTreeTarget::Session(session) => (None, session),
                ChooseTreeTarget::Window(window) => {
                    let session = inner
                        .engine
                        .state
                        .windows
                        .get(&window)
                        .map(|window| window.session)
                        .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
                    (
                        Some(CommandInvocation::new(
                            "select-window",
                            ["-t", &window.to_string()],
                        )),
                        session,
                    )
                }
                ChooseTreeTarget::Pane(pane) => {
                    let window = inner
                        .engine
                        .state
                        .window_for_pane(pane)
                        .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
                    let session = inner.engine.state.windows[&window].session;
                    (
                        Some(CommandInvocation::new(
                            "select-pane",
                            ["-t", &pane.to_string()],
                        )),
                        session,
                    )
                }
            };
            (selection, session)
        };
        if let Some(selection) = selection {
            self.execute_gesture(client, kind, context, "choose_tree_activate", &selection)?;
        }
        if client_attached_session(&self.inner.lock(), client) != Some(session) {
            self.execute_gesture(
                client,
                kind,
                context,
                "choose_tree_activate",
                &CommandInvocation::new("attach-session", ["-t", &session.to_string()]),
            )?;
        }
        Ok(())
    }

    fn execute_key_commands(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        pane: PaneId,
        commands: &[CommandInvocation],
    ) -> Result<(), DaemonError> {
        let mut output = String::new();
        let mut output_truncated = false;
        for command in commands {
            let execution = match self.execute(client, kind, context, command) {
                Ok(execution) => execution,
                Err(error) => {
                    log::warn!(
                        target: "zz_daemon::diagnostics::input",
                        "key_command_failed client={client} command={} args={:?} error={error}",
                        command.name,
                        command.args,
                    );
                    return Err(error);
                }
            };
            if !execution.output.is_empty() && !output_truncated {
                output_truncated = append_command_prompt_output(&mut output, &execution.output);
            }
        }
        if output_truncated {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(COMMAND_PROMPT_OUTPUT_TRUNCATED);
        }
        if kind == ClientKind::Interactive && !output.is_empty() {
            let title = commands.first().map_or_else(
                || "command output".to_owned(),
                |command| {
                    if commands.len() == 1 {
                        command.name.clone()
                    } else {
                        "command output".to_owned()
                    }
                },
            );
            self.open_command_output(client, Some(pane), title, &output)?;
        }
        Ok(())
    }

    fn input_command_prompt_text(&self, client: ClientId, text: &str) -> bool {
        let result = {
            let mut inner = self.inner.lock();
            let history = command_prompt_history_snapshot(&inner.command_history);
            let Some(prompt) = inner.command_prompts.get_mut(&client) else {
                return false;
            };
            if prompt.insert(text) {
                Ok(prompt.state(&history))
            } else {
                Err(())
            }
        };
        match result {
            Ok(state) => {
                self.publish_to_client(client, EventPayload::CommandPrompt { state: Some(state) });
            }
            Err(()) => {
                self.publish_to_client(
                    client,
                    EventPayload::ClientMessage {
                        pane: None,
                        kind: ClientMessageKind::Warning,
                        text: format!(
                            "command prompt input exceeds {} bytes",
                            zz_protocol::MAX_COMMAND_PROMPT_BYTES
                        ),
                    },
                );
            }
        }
        true
    }

    fn input_command_prompt_key(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        input: &zz_terminal::KeyInput,
        text_follows: bool,
    ) -> bool {
        let (event, submission, limit_exceeded) = {
            let mut inner = self.inner.lock();
            let Some(mut prompt) = inner.command_prompts.remove(&client) else {
                return false;
            };
            let action =
                command_prompt_key(&mut prompt, input, text_follows, &inner.command_history);
            match action {
                PromptKeyAction::Handled => {
                    inner.command_prompts.insert(client, prompt);
                    (None, None, false)
                }
                PromptKeyAction::Updated => {
                    let state = prompt.state(&inner.command_history);
                    inner.command_prompts.insert(client, prompt);
                    (Some(Some(state)), None, false)
                }
                PromptKeyAction::LimitExceeded => {
                    inner.command_prompts.insert(client, prompt);
                    (None, None, true)
                }
                PromptKeyAction::Close => (Some(None), None, false),
                PromptKeyAction::Submit => {
                    if !prompt.input.is_empty()
                        && inner.command_history.last() != Some(&prompt.input)
                    {
                        inner.command_history.push(prompt.input.clone());
                        if inner.command_history.len() > MAX_COMMAND_PROMPT_HISTORY {
                            inner.command_history.remove(0);
                        }
                    }
                    let submission = (!prompt.input.is_empty() || prompt.template.is_some())
                        .then_some(CommandPromptSubmission {
                            input: prompt.input,
                            template: prompt.template,
                        });
                    (Some(None), submission, false)
                }
            }
        };

        if let Some(state) = event {
            self.publish_to_client(client, EventPayload::CommandPrompt { state });
        }
        if limit_exceeded {
            self.publish_to_client(
                client,
                EventPayload::ClientMessage {
                    pane: context.pane,
                    kind: ClientMessageKind::Warning,
                    text: format!(
                        "command prompt input exceeds {} bytes",
                        zz_protocol::MAX_COMMAND_PROMPT_BYTES
                    ),
                },
            );
        }
        if let Some(submission) = submission {
            self.submit_command_prompt(client, kind, context, &submission);
        }
        true
    }

    fn input_command_prompt_action(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        action: CommandPromptAction,
    ) -> Result<(), DaemonError> {
        if kind != ClientKind::Interactive {
            return Err(ServerError::InvalidCommand(
                "command prompt actions require an interactive client".to_owned(),
            )
            .into());
        }

        match action {
            CommandPromptAction::Update { input, cursor } => {
                let mut inner = self.inner.lock();
                if let Some(prompt) = inner.command_prompts.get_mut(&client) {
                    prompt.replace_input(input, cursor)?;
                }
            }
            CommandPromptAction::Submit { input } => {
                if input.len() > zz_protocol::MAX_COMMAND_PROMPT_BYTES {
                    return Err(ServerError::InvalidCommand(format!(
                        "command prompt input exceeds {} bytes",
                        zz_protocol::MAX_COMMAND_PROMPT_BYTES
                    ))
                    .into());
                }
                let submission = {
                    let mut inner = self.inner.lock();
                    let Some(mut prompt) = inner.command_prompts.remove(&client) else {
                        return Ok(());
                    };
                    let cursor = u32::try_from(input.chars().count()).unwrap_or(u32::MAX);
                    prompt.replace_input(input, cursor)?;
                    if !prompt.input.is_empty()
                        && inner.command_history.last() != Some(&prompt.input)
                    {
                        inner.command_history.push(prompt.input.clone());
                        if inner.command_history.len() > MAX_COMMAND_PROMPT_HISTORY {
                            inner.command_history.remove(0);
                        }
                    }
                    (!prompt.input.is_empty() || prompt.template.is_some()).then_some(
                        CommandPromptSubmission {
                            input: prompt.input,
                            template: prompt.template,
                        },
                    )
                };
                self.publish_to_client(client, EventPayload::CommandPrompt { state: None });
                if let Some(submission) = submission {
                    self.submit_command_prompt(client, kind, context, &submission);
                }
            }
            CommandPromptAction::Close => {
                if self.inner.lock().command_prompts.remove(&client).is_some() {
                    self.publish_to_client(client, EventPayload::CommandPrompt { state: None });
                }
            }
        }
        Ok(())
    }

    fn submit_command_prompt(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        submission: &CommandPromptSubmission,
    ) {
        let source = submission
            .template
            .as_deref()
            .unwrap_or(submission.input.as_str());
        let mut parsed = parse_config("<command-prompt>", source);
        if let Some(diagnostic) = parsed.diagnostics.first() {
            self.publish_to_client(
                client,
                EventPayload::ClientMessage {
                    pane: context.pane,
                    kind: ClientMessageKind::Error,
                    text: format!(
                        "{}:{}:{}: {}",
                        diagnostic.source, diagnostic.line, diagnostic.column, diagnostic.message
                    ),
                },
            );
            return;
        }
        if submission.template.is_some() {
            for command in &mut parsed.commands {
                command.name = command.name.replace("%%", &submission.input);
                for argument in &mut command.args {
                    *argument = argument.replace("%%", &submission.input);
                }
            }
        }

        let mut output = String::new();
        let mut output_truncated = false;
        for command in parsed.commands {
            match self.execute(client, kind, context, &command) {
                Ok(execution) if !execution.output.is_empty() && !output_truncated => {
                    output_truncated = append_command_prompt_output(&mut output, &execution.output);
                }
                Ok(_) => {}
                Err(error) => {
                    self.publish_to_client(
                        client,
                        EventPayload::ClientMessage {
                            pane: context.pane,
                            kind: ClientMessageKind::Error,
                            text: error.to_string(),
                        },
                    );
                    return;
                }
            }
        }
        if output_truncated {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(COMMAND_PROMPT_OUTPUT_TRUNCATED);
        }
        if !output.is_empty()
            && let Err(error) =
                self.open_command_output(client, context.pane, "command output".to_owned(), &output)
        {
            self.publish_to_client(
                client,
                EventPayload::ClientMessage {
                    pane: context.pane,
                    kind: ClientMessageKind::Error,
                    text: error.to_string(),
                },
            );
        }
    }

    fn input_paste(&self, client: ClientId, pane: PaneId, text: &str) -> Result<(), ServerError> {
        for sink in self.input_sinks(client, pane)? {
            match sink {
                PaneSink::Terminal(terminal) => terminal.view_action(
                    TerminalViewId(client.0),
                    zz_terminal::TerminalViewAction::Paste(text.to_owned()),
                ),
                PaneSink::Browser(target) => self.publish_for_pane(
                    target,
                    &EventPayload::BrowserCommand {
                        pane: target,
                        command: BrowserCommand::SendKeys(vec![zz_protocol::KeyToken::Literal(
                            text.to_owned(),
                        )]),
                    },
                ),
            }
        }
        Ok(())
    }

    fn begin_paste_upload(
        &self,
        client: ClientId,
        kind: ClientKind,
        upload_id: u64,
        pane: PaneId,
        purpose: PasteUploadPurpose,
        extension: String,
        total_bytes: u32,
    ) {
        if kind != ClientKind::Interactive {
            return;
        }
        if !zz_protocol::paste_upload_extension_is_valid(&extension)
            || purpose == PasteUploadPurpose::RecordPastedImage
                && PastedImageFormat::from_extension(&extension).is_none()
            || total_bytes == 0
            || total_bytes > zz_protocol::MAX_PASTE_UPLOAD_BYTES
        {
            self.reject_paste_upload(
                client,
                Some(pane),
                "the paste upload is malformed".to_owned(),
            );
            return;
        }
        let total_bytes = total_bytes as usize;
        let mut inner = self.inner.lock();
        if !client_is_attached_to_pane(&inner, client, pane) {
            drop(inner);
            self.reject_paste_upload(
                client,
                Some(pane),
                format!("cannot paste an image into {pane}: the pane is not attached"),
            );
            return;
        }
        let in_flight = inner
            .paste_uploads
            .keys()
            .filter(|(owner, id)| *owner == client && *id != upload_id)
            .count();
        if in_flight >= MAX_CONCURRENT_PASTE_UPLOADS {
            drop(inner);
            self.reject_paste_upload(
                client,
                Some(pane),
                format!("too many image pastes in flight (limit {MAX_CONCURRENT_PASTE_UPLOADS})"),
            );
            return;
        }
        inner.paste_uploads.insert(
            (client, upload_id),
            PasteUpload {
                pane,
                purpose,
                extension,
                total_bytes,
                bytes: Vec::with_capacity(total_bytes),
            },
        );
    }

    fn extend_paste_upload(&self, client: ClientId, upload_id: u64, bytes: &[u8]) {
        let key = (client, upload_id);
        let finished = {
            let mut inner = self.inner.lock();
            let Some(upload) = inner.paste_uploads.get(&key) else {
                return;
            };
            if upload.bytes.len().saturating_add(bytes.len()) > upload.total_bytes {
                let pane = upload.pane;
                inner.paste_uploads.remove(&key);
                drop(inner);
                self.reject_paste_upload(
                    client,
                    Some(pane),
                    "the pasted image sent more bytes than it declared".to_owned(),
                );
                return;
            }
            let upload = inner
                .paste_uploads
                .get_mut(&key)
                .expect("the upload was present a statement ago");
            upload.bytes.extend_from_slice(bytes);
            if upload.bytes.len() < upload.total_bytes {
                return;
            }
            inner.paste_uploads.remove(&key)
        };
        let Some(upload) = finished else {
            return;
        };
        self.finish_paste_upload(client, upload_id, upload);
    }

    fn finish_paste_upload(&self, client: ClientId, upload_id: u64, upload: PasteUpload) {
        if upload.purpose == PasteUploadPurpose::RecordPastedImage {
            self.record_pasted_image(client, upload);
            return;
        }
        let path = match write_paste_upload(
            &self.paste_directory,
            &format!("paste-{client}-{upload_id}.{}", upload.extension),
            &upload.bytes,
        ) {
            Ok(path) => path,
            Err(error) => {
                self.reject_paste_upload(
                    client,
                    Some(upload.pane),
                    format!("could not save the pasted image: {error}"),
                );
                return;
            }
        };
        prune_paste_uploads(&self.paste_directory, PASTE_UPLOAD_RETENTION);
        let Some(path) = path.to_str() else {
            self.reject_paste_upload(
                client,
                Some(upload.pane),
                "the pasted image landed on a path that is not valid UTF-8".to_owned(),
            );
            return;
        };
        // Record image PastePath bytes so remote placeholders can bind. Stage them before
        // input_paste to preserve the terminal actor's FIFO command order.
        if let Some(format) = PastedImageFormat::from_extension(&upload.extension) {
            self.stage_pending_pasted_image(client, upload.pane, format, Arc::from(upload.bytes));
        }
        if let Err(error) = self.input_paste(client, upload.pane, path) {
            self.reject_paste_upload(
                client,
                Some(upload.pane),
                format!("could not paste the image path: {error}"),
            );
        }
    }

    fn record_pasted_image(&self, client: ClientId, upload: PasteUpload) {
        let Some(format) = PastedImageFormat::from_extension(&upload.extension) else {
            self.reject_paste_upload(
                client,
                Some(upload.pane),
                "the recorded pasted image format is unsupported".to_owned(),
            );
            return;
        };
        if !self.stage_pending_pasted_image(client, upload.pane, format, Arc::from(upload.bytes)) {
            self.reject_paste_upload(
                client,
                Some(upload.pane),
                format!(
                    "cannot record an image for {}: the pane is no longer attached",
                    upload.pane
                ),
            );
        }
    }

    /// Stage pending bytes on the pane's terminal and open the bind window. The pending
    /// entry is inserted while `inner` is held so a concurrent pane removal either sees
    /// it (and purges it) or prevents the insert. Returns false when the client is no
    /// longer attached to a terminal pane.
    fn stage_pending_pasted_image(
        &self,
        client: ClientId,
        pane: PaneId,
        format: PastedImageFormat,
        bytes: Arc<[u8]>,
    ) -> bool {
        static NEXT_PASTED_IMAGE_TOKEN: AtomicU64 = AtomicU64::new(1);

        let token = NEXT_PASTED_IMAGE_TOKEN
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        let registered = {
            let inner = self.inner.lock();
            if client_is_attached_to_pane(&inner, client, pane) {
                inner.terminals.get(&pane).cloned().map(|terminal| {
                    let admission = self
                        .pasted_images
                        .lock()
                        .entry(pane)
                        .or_default()
                        .push_pending(PendingPastedImage {
                            token,
                            format,
                            bytes,
                        });
                    (terminal, admission)
                })
            } else {
                None
            }
        };
        let Some((terminal, admission)) = registered else {
            return false;
        };
        for number in admission.evicted_numbers {
            terminal.unbind_pasted_image(number);
        }
        if admission.retained {
            terminal.open_pending_paste(token);
        }
        true
    }

    fn bind_pasted_image(
        &self,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
        token: u64,
        number: u32,
    ) {
        if !self.is_current_terminal(pane, terminal) {
            return;
        }
        let evicted = self
            .pasted_images
            .lock()
            .get_mut(&pane)
            .and_then(|images| images.bind(token, number));
        let Some(evicted) = evicted else {
            terminal.unbind_pasted_image(number);
            return;
        };
        for number in evicted {
            terminal.unbind_pasted_image(number);
        }
    }

    fn expire_pending_pasted_image(
        &self,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
        token: u64,
    ) {
        if !self.is_current_terminal(pane, terminal) {
            return;
        }
        if let Some(images) = self.pasted_images.lock().get_mut(&pane) {
            images.expire(token);
        }
    }

    fn fetch_pasted_image(&self, client: ClientId, pane: PaneId, number: u32) {
        let subscriber = {
            let inner = self.inner.lock();
            client_is_attached_to_pane(&inner, client, pane)
                .then(|| inner.subscribers.get(&client).cloned())
                .flatten()
        };
        let Some(subscriber) = subscriber else {
            return;
        };
        let image = {
            let mut panes = self.pasted_images.lock();
            panes
                .get_mut(&pane)
                .and_then(|images| images.images.get_mut(&number))
                .and_then(|image| {
                    pasted_image_frames(pane, number, image).map(|frames| (image.token, frames))
                })
        };
        let Some((token, frames)) = image else {
            let _ = subscriber
                .enqueue_reliable(&ProtocolMessage::PastedImageUnavailable { pane, number });
            return;
        };
        let _ = subscriber.enqueue_pasted_image(pane, number, token, &frames);
    }

    fn reject_paste_upload(&self, client: ClientId, pane: Option<PaneId>, text: String) {
        log::warn!(
            target: "zz_daemon::diagnostics::input",
            "paste upload rejected client={client} pane={pane:?}: {text}"
        );
        self.publish_to_client(
            client,
            EventPayload::ClientMessage {
                pane,
                kind: ClientMessageKind::Error,
                text,
            },
        );
    }

    fn input_sinks(&self, client: ClientId, source: PaneId) -> Result<Vec<PaneSink>, ServerError> {
        let inner = self.inner.lock();
        if !client_is_attached_to_pane(&inner, client, source) {
            return Err(ServerError::PaneNotAttached(source));
        }
        resolve_input_sinks(&inner, source)
    }

    fn sync_prefix_armed(&self, client: ClientId) {
        let (changed, armed) = {
            let mut inner = self.inner.lock();
            let armed = inner
                .key_engines
                .get(&client)
                .is_some_and(|engine| engine.active_table() == Some("prefix"));
            let changed = if armed {
                inner.prefix_armed.insert(client)
            } else {
                inner.prefix_armed.remove(&client)
            };
            (changed, armed)
        };
        if changed {
            log::info!(
                target: "zz_daemon::diagnostics::input",
                "prefix_armed_published client={client} armed={armed}"
            );
            self.publish_to_client(client, EventPayload::PrefixArmed { armed });
        }
    }

    fn key_decision(&self, client: ClientId, key: &str, release: bool) -> KeyDecision {
        let mut inner = self.inner.lock();
        if release {
            return if inner
                .swallowed_keys
                .get_mut(&client)
                .is_some_and(|keys| keys.remove(bare_key_name(key)))
            {
                KeyDecision::Prefix
            } else {
                KeyDecision::Pass
            };
        }
        let mut key_engine = inner.key_engines.remove(&client).unwrap_or_default();
        let table = key_engine.active_table().map(str::to_owned);
        let (initial_repeat_time_ms, repeat_time_ms) = client_attached_session(&inner, client)
            .map_or((0, 500), |session| {
                (
                    inner.engine.initial_repeat_time_for_session(session),
                    inner.engine.repeat_time_for_session(session),
                )
            });
        let decision = key_engine.handle_with_repeat_times(
            &inner.engine.keys,
            key,
            Instant::now(),
            Duration::from_millis(u64::from(repeat_time_ms)),
            Duration::from_millis(u64::from(initial_repeat_time_ms)),
        );
        inner.key_engines.insert(client, key_engine);
        if decision != KeyDecision::Pass {
            inner
                .swallowed_keys
                .entry(client)
                .or_default()
                .insert(bare_key_name(key).to_owned());
        }
        drop(inner);
        if decision != KeyDecision::Pass {
            log::info!(
                target: "zz_daemon::diagnostics::input",
                "key_decision client={client} key={key} table={} decision={decision:?}",
                table.as_deref().unwrap_or("root")
            );
        }
        decision
    }

    fn filter_suppressed_text<'a>(&self, client: ClientId, text: &'a str) -> Cow<'a, str> {
        let mut inner = self.inner.lock();
        let Some(characters) = inner.suppressed_text.get_mut(&client) else {
            return Cow::Borrowed(text);
        };
        let mut filtered = String::with_capacity(text.len());
        let mut unrepaid = false;
        for character in text.chars() {
            match characters.get_mut(&character) {
                Some(count) if *count > 0 => *count -= 1,
                _ => {
                    unrepaid = true;
                    filtered.push(character);
                }
            }
        }
        characters.retain(|_, count| *count != 0);
        if unrepaid || characters.is_empty() {
            inner.suppressed_text.remove(&client);
        }
        Cow::Owned(filtered)
    }

    fn kitty_image_frames(
        &self,
        pane: PaneId,
        terminal: &TerminalSession,
        image_id: u32,
        generation: u64,
    ) -> Option<Arc<[Vec<u8>]>> {
        let key = KittyImageKey {
            pane,
            image_id,
            generation,
        };
        if let Some(frames) = self.kitty_image_frames.lock().get(&key).cloned() {
            return Some(frames);
        }
        let image = match terminal.kitty_image(image_id) {
            Ok(Some(image)) if image.generation == generation => image,
            Ok(Some(image)) => {
                log::debug!(
                    "Kitty image {image_id} moved from requested generation {generation} to {} before export",
                    image.generation
                );
                return None;
            }
            Ok(None) => return None,
            Err(error) => {
                log::warn!("could not fetch Kitty image {image_id} for {pane}: {error}");
                return None;
            }
        };
        let total_bytes = u32::try_from(image.bgra.len()).ok()?;
        let mut frames =
            Vec::with_capacity(1 + image.bgra.len().div_ceil(MAX_KITTY_IMAGE_CHUNK_BYTES));
        let begin = Self::event(EventPayload::KittyImageBegin {
            pane,
            image_id,
            generation,
            width: image.width,
            height: image.height,
            total_bytes,
        });
        let mut encoded = Vec::new();
        if let Err(error) = encode_protocol_message_into(&begin, &mut encoded) {
            log::warn!("could not encode Kitty image {image_id} header for {pane}: {error}");
            return None;
        }
        frames.push(encoded);
        for chunk in image.bgra.chunks(MAX_KITTY_IMAGE_CHUNK_BYTES) {
            let message = Self::event(EventPayload::KittyImageChunk {
                pane,
                image_id,
                generation,
                bytes: chunk.to_vec(),
            });
            let mut encoded = Vec::new();
            if let Err(error) = encode_protocol_message_into(&message, &mut encoded) {
                log::warn!("could not encode Kitty image {image_id} chunk for {pane}: {error}");
                return None;
            }
            frames.push(encoded);
        }
        let frames: Arc<[Vec<u8>]> = frames.into();
        let mut cache = self.kitty_image_frames.lock();
        Some(
            cache
                .entry(key)
                .or_insert_with(|| Arc::clone(&frames))
                .clone(),
        )
    }

    fn enqueue_kitty_images_for_viewport(
        &self,
        outbound: &OutboundMailbox,
        pane: PaneId,
        terminal: &TerminalSession,
        viewport: &TerminalViewport,
    ) {
        let referenced = viewport
            .kitty_placements
            .iter()
            .map(|placement| (placement.image_id, placement.image_generation))
            .collect::<BTreeSet<_>>();
        for (image_id, generation) in referenced {
            let Some(frames) = self.kitty_image_frames(pane, terminal, image_id, generation) else {
                continue;
            };
            match outbound.enqueue_kitty_image(pane, image_id, generation, &frames) {
                KittyImageEnqueue::AlreadyDelivered => {}
                KittyImageEnqueue::Queued | KittyImageEnqueue::Closed => break,
            }
        }
    }

    fn evict_absent_kitty_images(
        &self,
        pane: PaneId,
        terminal: &TerminalSession,
        referenced: &BTreeSet<(u32, u64)>,
    ) {
        let candidates = self
            .kitty_image_frames
            .lock()
            .keys()
            .filter(|key| key.pane == pane && !referenced.contains(&(key.image_id, key.generation)))
            .copied()
            .collect::<Vec<_>>();
        let mut removed = BTreeSet::new();
        let mut stale = Vec::new();
        for key in candidates {
            match terminal.kitty_image_generation(key.image_id) {
                Ok(generation) if generation == Some(key.generation) => {}
                Ok(_) => {
                    stale.push(key);
                    removed.insert(key.image_id);
                }
                Err(error) => log::warn!(
                    "could not verify Kitty image {} storage for {pane}: {error}",
                    key.image_id
                ),
            }
        }
        if stale.is_empty() {
            return;
        }
        let mut cache = self.kitty_image_frames.lock();
        for key in stale {
            cache.remove(&key);
        }
        drop(cache);
        let image_ids = removed.into_iter().collect::<Vec<_>>();
        let subscribers = self
            .inner
            .lock()
            .subscribers
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            subscriber.enqueue_kitty_images_removed(pane, &image_ids);
        }
    }

    fn send_resync(&self, client: ClientId, outbound: &OutboundMailbox) {
        let (snapshot, viewports, command_prompt, choose_tree, choose_buffer, display_panes) = {
            let inner = self.inner.lock();
            let mut snapshot = inner.engine.state.snapshot();
            let presence = snapshot_presence(&inner);
            stamp_snapshot_for_client(&inner, client, &mut snapshot, &presence);
            let command_prompt = command_prompt_state(&inner, client);
            let choose_tree = inner
                .choose_trees
                .get(&client)
                .map(|chooser| chooser.rendered.clone());
            let choose_buffer = inner
                .choose_buffers
                .get(&client)
                .map(|chooser| chooser.rendered.clone());
            let display_panes = inner
                .display_panes
                .get(&client)
                .map(|overlay| overlay.state.clone());
            let session = client_attached_session(&inner, client);
            let view = TerminalViewId(client.0);
            let viewports = session.map_or_else(Vec::new, |session| {
                visible_terminal_panes(&inner, client, session)
                    .into_iter()
                    .filter_map(|pane| {
                        terminal_viewport_for_pane(&inner, pane, view)
                            .map(|(terminal, viewport)| (pane, terminal, (*viewport).clone()))
                    })
                    .collect()
            });
            (
                snapshot,
                viewports,
                command_prompt,
                choose_tree,
                choose_buffer,
                display_panes,
            )
        };
        Self::send_event(outbound, EventPayload::Snapshot(snapshot));
        Self::send_event(
            outbound,
            EventPayload::CommandPrompt {
                state: command_prompt,
            },
        );
        Self::send_event(outbound, EventPayload::ChooseTree { state: choose_tree });
        Self::send_event(
            outbound,
            EventPayload::ChooseBuffer {
                state: choose_buffer,
            },
        );
        Self::send_event(
            outbound,
            EventPayload::DisplayPanes {
                state: display_panes,
            },
        );
        for (pane, terminal, viewport) in viewports {
            self.enqueue_kitty_images_for_viewport(outbound, pane, &terminal, &viewport);
            let viewport = Self::event(EventPayload::TerminalViewport { pane, viewport });
            if outbound.enqueue_terminal(pane, &viewport) == TerminalEnqueue::NeedsFull {
                let _ = outbound.replace_terminal(pane, &viewport);
            }
        }
        #[cfg(feature = "agent")]
        self.send_agent_resync(client, outbound);
        let inner = self.inner.lock();
        if let Some(output) = inner.command_outputs.get(&client) {
            let message = Self::event(EventPayload::CommandOutput {
                pane: output.pane,
                viewport: output
                    .terminal
                    .latest_viewport_for(TerminalViewId(client.0))
                    .map(|viewport| (*viewport).clone()),
            });
            let _ = outbound.replace_command_output(&message);
        } else {
            Self::send_event(
                outbound,
                EventPayload::CommandOutput {
                    pane: client_context_pane(&inner, client).unwrap_or(PaneId(0)),
                    viewport: None,
                },
            );
        }
    }

    fn send_full(&self, client: ClientId, pane: PaneId, outbound: &OutboundMailbox) {
        let viewport = {
            let inner = self.inner.lock();
            let Some(session) = client_attached_session(&inner, client) else {
                return;
            };
            if !visible_terminal_panes(&inner, client, session).contains(&pane) {
                return;
            }
            let view = TerminalViewId(client.0);
            terminal_viewport_for_pane(&inner, pane, view)
        };
        if let Some((terminal, viewport)) = viewport {
            self.enqueue_kitty_images_for_viewport(outbound, pane, &terminal, &viewport);
            let _ =
                outbound.replace_terminal_viewport(pane, Self::next_sequence(), viewport.as_ref());
        }
    }

    fn send_history(
        &self,
        client: ClientId,
        pane: PaneId,
        start: u32,
        count: u32,
        outbound: &OutboundMailbox,
    ) {
        let terminal = {
            let inner = self.inner.lock();
            let Some(session) = client_attached_session(&inner, client) else {
                return;
            };
            if !visible_terminal_panes(&inner, client, session).contains(&pane) {
                return;
            }
            inner.terminals.get(&pane).cloned()
        };
        let Some(terminal) = terminal else {
            return;
        };
        let Ok((start, rows, dictionary, scrollbar, columns)) =
            terminal.history(start, count.min(MAX_HISTORY_CHUNK_ROWS))
        else {
            return;
        };
        Self::send_event(
            outbound,
            EventPayload::HistoryChunk {
                pane,
                start,
                total: scrollbar.total,
                offset: scrollbar.offset,
                columns,
                rows,
                dictionary,
            },
        );
    }

    fn open_command_output(
        self: &Arc<Self>,
        client: ClientId,
        preferred_pane: Option<PaneId>,
        title: String,
        text: &str,
    ) -> Result<(), DaemonError> {
        if text.is_empty() {
            return Ok(());
        }
        let text = bounded_command_output(text);
        let appearance = Arc::clone(&self.inner.lock().appearance);
        let terminal = Arc::new(TerminalSession::spawn_output_view_with_appearance(
            title, text, appearance,
        ));
        let view = TerminalViewId(client.0);

        let (
            pane,
            replaced,
            choose_tree_closed,
            choose_buffer_closed,
            display_panes_closed,
            command_prompt_closed,
            word_separators,
        ) = {
            let mut inner = self.inner.lock();
            if !inner.subscribers.contains_key(&client) {
                return Err(ServerError::InvalidCommand(
                    "command output requires an interactive client".to_owned(),
                )
                .into());
            }
            let pane = preferred_pane
                .filter(|pane| inner.engine.state.window_for_pane(*pane).is_some())
                .or_else(|| client_context_pane(&inner, client))
                .ok_or_else(|| ServerError::MissingTarget("current pane".to_owned()))?;
            let word_separators = WordSeparators::new(inner.engine.word_separators_for_pane(pane)?);
            let choose_tree_closed = inner.choose_trees.remove(&client).is_some();
            let choose_buffer_closed = inner.choose_buffers.remove(&client).is_some();
            let display_panes_closed = take_display_panes(&mut inner, client).is_some();
            let command_prompt_closed = inner.command_prompts.remove(&client).is_some();
            let replaced = inner.command_outputs.remove(&client);
            let previous_key_table = replaced.as_ref().map_or_else(
                || {
                    inner
                        .key_engines
                        .entry(client)
                        .or_default()
                        .active_table()
                        .map(str::to_owned)
                },
                |output| output.previous_key_table.clone(),
            );
            let table = inner.engine.copy_mode_table_for_pane(pane)?.to_owned();
            inner
                .key_engines
                .entry(client)
                .or_default()
                .switch_table(Some(table));
            inner.command_outputs.insert(
                client,
                CommandOutputSession {
                    pane,
                    terminal: Arc::clone(&terminal),
                    previous_key_table,
                },
            );
            (
                pane,
                replaced,
                choose_tree_closed,
                choose_buffer_closed,
                display_panes_closed,
                command_prompt_closed,
                word_separators,
            )
        };

        terminal.set_word_separators(word_separators);
        terminal.attach_view(view);
        if choose_tree_closed {
            self.publish_to_client(client, EventPayload::ChooseTree { state: None });
        }
        if choose_buffer_closed {
            self.publish_to_client(client, EventPayload::ChooseBuffer { state: None });
        }
        if display_panes_closed {
            self.publish_to_client(client, EventPayload::DisplayPanes { state: None });
        }
        if command_prompt_closed {
            self.publish_to_client(client, EventPayload::CommandPrompt { state: None });
        }
        if let Some(replaced) = replaced {
            replaced.terminal.view_action(
                view,
                zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel),
            );
        }
        if let Err(error) = self.watch_command_output(client, pane, Arc::clone(&terminal)) {
            self.close_command_output(client, &terminal);
            return Err(error);
        }
        Ok(())
    }

    fn watch_command_output(
        self: &Arc<Self>,
        client: ClientId,
        pane: PaneId,
        terminal: Arc<TerminalSession>,
    ) -> Result<(), DaemonError> {
        let events = terminal.events();
        let shared = Arc::clone(self);
        thread::Builder::new()
            .name(format!("zz-output-{}", client.0))
            .spawn(move || {
                while let Ok(event) = events.recv_blocking() {
                    if !shared.is_current_command_output(client, &terminal) {
                        break;
                    }
                    match event {
                        TerminalEvent::ViewportReady => {
                            if let Some(viewport) =
                                terminal.latest_viewport_for(TerminalViewId(client.0))
                            {
                                shared.publish_command_output(client, pane, &terminal, &viewport);
                            }
                        }
                        TerminalEvent::CopyReady { view, copy }
                            if view == TerminalViewId(client.0) =>
                        {
                            let copy = *copy;
                            if let Some(buffer) = copy.buffer {
                                shared.store_copy_buffer(copy.text.clone(), buffer);
                            }
                            if let Some(command) = copy.pipe {
                                shared.spawn_copy_pipe(pane, client, command, copy.text.clone());
                            }
                            if let Some(target) = copy.clipboard {
                                shared.publish_to_client(
                                    client,
                                    EventPayload::Clipboard {
                                        pane,
                                        request_id: copy.request_id,
                                        target,
                                        text: copy.text,
                                    },
                                );
                            }
                        }
                        TerminalEvent::OpenUri(open) if open.view == TerminalViewId(client.0) => {
                            shared.publish_to_client(
                                client,
                                EventPayload::OpenUri {
                                    pane,
                                    uri: open.uri,
                                },
                            );
                        }
                        TerminalEvent::ViewClosed(view) if view == TerminalViewId(client.0) => {
                            shared.close_command_output(client, &terminal);
                            break;
                        }
                        TerminalEvent::CopyReady { .. }
                        | TerminalEvent::OpenUri(_)
                        | TerminalEvent::ViewClosed(_)
                        | TerminalEvent::ClipboardSet { .. }
                        | TerminalEvent::Bell
                        | TerminalEvent::PlaceholderBound { .. }
                        | TerminalEvent::PendingPasteExpired { .. } => {}
                    }
                }
            })
            .map_err(|error| DaemonError::Thread(error.to_string()))?;
        Ok(())
    }

    fn publish_command_output(
        &self,
        client: ClientId,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
        viewport: &TerminalViewport,
    ) {
        self.publish_command_output_with_encoder(
            client,
            pane,
            terminal,
            viewport,
            OutboundMailbox::encode_message,
        );
    }

    fn publish_command_output_with_encoder(
        &self,
        client: ClientId,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
        viewport: &TerminalViewport,
        encode: impl FnOnce(&OutboundMailbox, &ProtocolMessage) -> Result<Vec<u8>, ProtocolError>,
    ) {
        let subscriber = {
            let inner = self.inner.lock();
            current_command_output_subscriber(&inner, client, pane, terminal)
        };
        let Some(subscriber) = subscriber else {
            return;
        };
        let message = Self::event(EventPayload::CommandOutput {
            pane,
            viewport: Some(viewport.clone()),
        });
        let Ok(encoded) = encode(&subscriber, &message) else {
            log::error!("failed to encode command-output viewport");
            return;
        };

        let mut encoded = Some(encoded);
        let installed = {
            let inner = self.inner.lock();
            current_command_output_subscriber(&inner, client, pane, terminal)
                .filter(|current| Arc::ptr_eq(current, &subscriber))
                .is_some_and(|_| {
                    subscriber.replace_encoded_command_output(
                        encoded.take().expect("encoded command output is available"),
                    )
                })
        };
        if !installed && let Some(encoded) = encoded {
            subscriber.recycle_frame(encoded);
        }
    }

    fn close_command_output(&self, client: ClientId, terminal: &Arc<TerminalSession>) {
        let (pane, subscriber) = {
            let mut inner = self.inner.lock();
            let Some(output) = inner.command_outputs.get(&client) else {
                return;
            };
            if !Arc::ptr_eq(&output.terminal, terminal) {
                return;
            }
            let output = inner
                .command_outputs
                .remove(&client)
                .expect("command output was checked above");
            inner
                .key_engines
                .entry(client)
                .or_default()
                .switch_table(output.previous_key_table);
            (output.pane, inner.subscribers.get(&client).cloned())
        };
        if let Some(subscriber) = subscriber {
            Self::send_event(
                &subscriber,
                EventPayload::CommandOutput {
                    pane,
                    viewport: None,
                },
            );
        }
    }

    fn retire_command_output(client: ClientId, (output, subscriber): RetiredCommandOutput) {
        output.terminal.view_action(
            TerminalViewId(client.0),
            zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel),
        );
        if let Some(subscriber) = subscriber {
            Self::send_event(
                &subscriber,
                EventPayload::CommandOutput {
                    pane: output.pane,
                    viewport: None,
                },
            );
        }
    }

    fn is_current_command_output(&self, client: ClientId, terminal: &Arc<TerminalSession>) -> bool {
        self.inner
            .lock()
            .command_outputs
            .get(&client)
            .is_some_and(|output| Arc::ptr_eq(&output.terminal, terminal))
    }

    fn watch_terminal(
        self: &Arc<Self>,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
    ) -> Result<(), DaemonError> {
        let events = terminal.events();
        let terminal = Arc::downgrade(terminal);
        let shared = Arc::clone(self);
        thread::Builder::new()
            .name(format!("zz-pane-{}", pane.0))
            .spawn(move || {
                let mut previous = BTreeMap::<TerminalViewId, Arc<TerminalViewport>>::new();
                let mut previous_title = None::<String>;
                let mut previous_foreground = None::<Option<u32>>;
                let mut current_command = String::new();
                let mut diff_scratch = TerminalDiffScratch::default();
                while let Ok(event) = events.recv_blocking() {
                    let Some(terminal) = terminal.upgrade() else {
                        break;
                    };
                    if !shared.is_current_terminal(pane, &terminal) {
                        break;
                    }
                    match event {
                        TerminalEvent::ViewportReady => {
                            let mut current =
                                terminal.latest_viewports().into_iter().collect::<Vec<_>>();
                            current.sort_by_key(|(view, _)| view.0);
                            let runtime_viewport = current.first().map_or_else(
                                || terminal.latest_viewport(),
                                |(_, viewport)| Arc::clone(viewport),
                            );
                            if !terminal_status_should_close(&runtime_viewport.status) {
                                let foreground = terminal.foreground_process_id();
                                if previous_foreground != Some(foreground) {
                                    current_command = terminal_current_command(&terminal);
                                    previous_foreground = Some(foreground);
                                }
                                shared.synchronize_pane_runtime(
                                    pane,
                                    &terminal,
                                    &runtime_viewport,
                                    &current_command,
                                );
                            }
                            let referenced_images = current
                                .iter()
                                .flat_map(|(_, viewport)| {
                                    viewport.kitty_placements.iter().map(|placement| {
                                        (placement.image_id, placement.image_generation)
                                    })
                                })
                                .collect::<BTreeSet<_>>();
                            shared.evict_absent_kitty_images(pane, &terminal, &referenced_images);
                            for (image_id, generation) in &referenced_images {
                                let _ = shared.kitty_image_frames(
                                    pane,
                                    &terminal,
                                    *image_id,
                                    *generation,
                                );
                            }
                            let active = current
                                .iter()
                                .map(|(view, _)| *view)
                                .collect::<BTreeSet<_>>();
                            let mut finished = false;
                            if active.is_empty() {
                                let viewport = terminal.latest_viewport();
                                if previous_title
                                    .as_deref()
                                    .is_none_or(|previous| previous != viewport.title())
                                {
                                    shared.synchronize_pane_title(
                                        pane,
                                        &terminal,
                                        viewport.title(),
                                    );
                                    previous_title = Some(viewport.title().to_owned());
                                }
                                finished = terminal_status_should_close(&viewport.status);
                            }
                            for (view, viewport) in current {
                                if previous_title
                                    .as_deref()
                                    .is_none_or(|previous| previous != viewport.title())
                                {
                                    shared.synchronize_pane_title(
                                        pane,
                                        &terminal,
                                        viewport.title(),
                                    );
                                    previous_title = Some(viewport.title().to_owned());
                                }
                                finished |= terminal_status_should_close(&viewport.status);
                                let payload = previous
                                    .get(&view)
                                    .and_then(|previous| {
                                        TerminalViewport::diff_with_scratch(
                                            previous,
                                            &viewport,
                                            &mut diff_scratch,
                                        )
                                    })
                                    .map_or_else(|| TerminalFanout::Full, TerminalFanout::Patch);
                                shared.publish_terminal_for_pane(
                                    pane,
                                    ClientId(view.0),
                                    payload,
                                    &viewport,
                                    &terminal,
                                );
                                previous.insert(view, viewport);
                            }
                            previous.retain(|view, _| active.contains(view));
                            if finished {
                                shared.close_exited_terminal(pane, &terminal);
                                return;
                            }
                        }
                        TerminalEvent::CopyReady { view, copy } => {
                            let client = ClientId(view.0);
                            let copy = *copy;
                            if let Some(buffer) = copy.buffer {
                                shared.store_copy_buffer(copy.text.clone(), buffer);
                            }
                            if let Some(command) = copy.pipe {
                                shared.spawn_copy_pipe(pane, client, command, copy.text.clone());
                            }
                            if let Some(target) = copy.clipboard {
                                shared.publish_to_client(
                                    client,
                                    EventPayload::Clipboard {
                                        pane,
                                        request_id: copy.request_id,
                                        target,
                                        text: copy.text,
                                    },
                                );
                            }
                        }
                        TerminalEvent::OpenUri(open) => {
                            shared.publish_to_client(
                                ClientId(open.view.0),
                                EventPayload::OpenUri {
                                    pane,
                                    uri: open.uri,
                                },
                            );
                        }
                        TerminalEvent::PlaceholderBound { token, number } => {
                            shared.bind_pasted_image(pane, &terminal, token, number);
                        }
                        TerminalEvent::PendingPasteExpired { token } => {
                            shared.expire_pending_pasted_image(pane, &terminal, token);
                        }
                        TerminalEvent::ClipboardSet { target, text } => {
                            shared.deliver_clipboard_write(pane, target, text);
                        }
                        TerminalEvent::Bell => shared.raise_pane_bell(pane),
                        TerminalEvent::ViewClosed(_) => {}
                    }
                }
                let Some(terminal) = terminal.upgrade() else {
                    return;
                };
                if terminal_status_should_close(&terminal.latest_viewport().status) {
                    shared.close_exited_terminal(pane, &terminal);
                }
            })
            .map_err(|error| DaemonError::Thread(error.to_string()))?;
        Ok(())
    }

    fn close_exited_terminal(self: &Arc<Self>, pane: PaneId, terminal: &Arc<TerminalSession>) {
        let status = terminal.latest_viewport().status.clone();
        let (failed, dead_status, dead_signal) = match status {
            zz_terminal::SessionStatus::Exited(status) => {
                let failed = status.code != 0 || status.signal.is_some();
                let dead_status = status.signal.is_none().then_some(status.code);
                (failed, dead_status, status.signal.clone())
            }
            zz_terminal::SessionStatus::Failed(_) => (true, None, None),
            zz_terminal::SessionStatus::Starting | zz_terminal::SessionStatus::Running => return,
        };
        let Some((mut context, retained, changed)) = ({
            let mut inner = self.inner.lock();
            if !inner
                .terminals
                .get(&pane)
                .is_some_and(|current| Arc::ptr_eq(current, terminal))
            {
                return;
            }
            let Some(context) = ExecutionContext::for_pane(&inner.engine.state, pane) else {
                return;
            };
            let retained = inner
                .engine
                .retain_exited_pane(pane, failed)
                .unwrap_or(false);
            let changed = if retained {
                let facts = format_hook_facts(&inner);
                let mut hooks = DaemonFormatHooks::command(&facts);
                inner
                    .engine
                    .mark_pane_dead_with_hooks(
                        pane,
                        dead_status,
                        dead_signal.as_deref(),
                        &mut hooks,
                    )
                    .unwrap_or(false)
            } else {
                false
            };
            Some((context, retained, changed))
        }) else {
            return;
        };

        if retained {
            log::debug!(
                target: "zz_daemon::diagnostics::terminal",
                "terminal worker finished pane={pane}; retaining dead pane"
            );
            if changed {
                self.publish_snapshot();
            }
            return;
        }

        log::debug!(
            target: "zz_daemon::diagnostics::terminal",
            "terminal worker finished pane={pane}; closing pane"
        );
        let target = pane.to_string();
        let command = CommandInvocation::new("kill-pane", ["-t", target.as_str()]);
        if let Err(error) = self.execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &command,
        ) && self.is_current_terminal(pane, terminal)
        {
            log::error!(
                target: "zz_daemon::diagnostics::terminal",
                "failed to close exited terminal pane={pane}: {error}"
            );
        }
    }

    fn is_current_terminal(&self, pane: PaneId, terminal: &Arc<TerminalSession>) -> bool {
        self.inner
            .lock()
            .terminals
            .get(&pane)
            .is_some_and(|current| Arc::ptr_eq(current, terminal))
    }

    fn synchronize_pane_title(&self, pane: PaneId, terminal: &Arc<TerminalSession>, title: &str) {
        let changed = {
            let mut inner = self.inner.lock();
            if !inner
                .terminals
                .get(&pane)
                .is_some_and(|current| Arc::ptr_eq(current, terminal))
            {
                return;
            }
            inner
                .engine
                .state
                .update_pane_title(pane, title)
                .unwrap_or(false)
        };
        if changed {
            self.publish_snapshot();
        }
    }

    fn synchronize_pane_runtime(
        &self,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
        viewport: &TerminalViewport,
        current_command: &str,
    ) {
        let current_path = terminal_working_directory(terminal)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let reported_path = viewport.working_directory().unwrap_or_default().to_owned();
        let pid = terminal.process_id();
        let tty = terminal
            .tty()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let changed = {
            let mut inner = self.inner.lock();
            if !inner
                .terminals
                .get(&pane)
                .is_some_and(|current| Arc::ptr_eq(current, terminal))
            {
                return;
            }
            let previous = inner
                .engine
                .pane_runtime_facts(pane)
                .cloned()
                .unwrap_or_default();
            let facts = format_hook_facts(&inner);
            let mut hooks = DaemonFormatHooks::command(&facts);
            inner.engine.set_pane_runtime_facts_with_hooks(
                pane,
                PaneRuntimeFacts {
                    current_command: current_command.to_owned(),
                    current_path,
                    dead_signal: previous.dead_signal,
                    reported_path,
                    start_path: previous.start_path,
                    pid,
                    tty,
                },
                &mut hooks,
            )
        };
        if changed {
            self.publish_snapshot();
        }
    }

    fn detach_removed_sessions(&self) {
        let detached = {
            let inner = self.inner.lock();
            inner
                .attached
                .iter()
                .filter(|(session, _)| !inner.engine.state.sessions.contains_key(session))
                .flat_map(|(session, clients)| clients.iter().map(|client| (*session, *client)))
                .collect::<Vec<_>>()
        };
        for (session, client) in detached {
            self.publish_to_client(client, EventPayload::Detached { session, by: None });
            let _ = self.detach_client_state(client);
        }
    }

    fn publish_snapshot(&self) {
        self.detach_removed_sessions();
        let snapshots = {
            let mut inner = self.inner.lock();
            let snapshot = inner.engine.state.snapshot();
            inner.last_published_mux_generation = snapshot.generation;
            let presence = snapshot_presence(&inner);
            inner
                .subscribers
                .iter()
                .map(|(client, subscriber)| {
                    let mut client_snapshot = snapshot.clone();
                    stamp_snapshot_for_client(&inner, *client, &mut client_snapshot, &presence);
                    (*client, Arc::clone(subscriber), client_snapshot)
                })
                .collect::<Vec<_>>()
        };
        for (_, subscriber, snapshot) in snapshots {
            Self::send_event(&subscriber, EventPayload::Snapshot(snapshot));
        }
        self.refresh_status(false);
        self.refresh_terminal_visibility();
        #[cfg(feature = "agent")]
        self.refresh_agent_visibility();
        self.refresh_choose_trees();
        self.refresh_choose_buffers();
        self.refresh_display_panes();
    }

    fn refresh_choose_trees(&self) {
        let updates = {
            let mut inner = self.inner.lock();
            let clients = inner.choose_trees.keys().copied().collect::<Vec<_>>();
            let mut updates = Vec::with_capacity(clients.len());
            for client in clients {
                let Some(mut chooser) = inner.choose_trees.remove(&client) else {
                    continue;
                };
                let attached_session = client_attached_session(&inner, client);
                let source_session = inner
                    .engine
                    .state
                    .window_for_pane(chooser.source_pane)
                    .map(|window| inner.engine.state.windows[&window].session);
                if attached_session != Some(chooser.source_session)
                    || source_session != Some(chooser.source_session)
                {
                    updates.push((client, None));
                    continue;
                }
                chooser.rebuild(&inner.engine.state, attached_session);
                let state = chooser.rendered.clone();
                inner.choose_trees.insert(client, chooser);
                updates.push((client, Some(state)));
            }
            updates
        };
        for (client, state) in updates {
            self.publish_to_client(client, EventPayload::ChooseTree { state });
        }
    }

    fn refresh_choose_buffers(&self) {
        self.refresh_choose_buffers_except(None);
    }

    fn refresh_choose_buffers_except(&self, excluded: Option<ClientId>) {
        let updates = {
            let mut inner = self.inner.lock();
            let clients = inner
                .choose_buffers
                .keys()
                .copied()
                .filter(|client| Some(*client) != excluded)
                .collect::<Vec<_>>();
            let mut updates = Vec::with_capacity(clients.len());
            for client in clients {
                let Some(mut chooser) = inner.choose_buffers.remove(&client) else {
                    continue;
                };
                let attached_session = client_attached_session(&inner, client);
                let source_session = inner
                    .engine
                    .state
                    .window_for_pane(chooser.source_pane)
                    .map(|window| inner.engine.state.windows[&window].session);
                if attached_session != Some(chooser.source_session)
                    || source_session != Some(chooser.source_session)
                    || inner.paste_buffers.is_empty()
                {
                    updates.push((client, None));
                    continue;
                }
                chooser.rebuild(&inner.paste_buffers);
                let state = chooser.rendered.clone();
                inner.choose_buffers.insert(client, chooser);
                updates.push((client, Some(state)));
            }
            updates
        };
        for (client, state) in updates {
            self.publish_to_client(client, EventPayload::ChooseBuffer { state });
        }
    }

    fn refresh_display_panes(&self) {
        let updates = {
            let mut inner = self.inner.lock();
            let clients = inner.display_panes.keys().copied().collect::<Vec<_>>();
            let mut updates = Vec::with_capacity(clients.len());
            for client in clients {
                let Some(mut overlay) = inner.display_panes.remove(&client) else {
                    continue;
                };
                let attached_session = client_attached_session(&inner, client);
                let active_window = client_focused_window_for_attachment(&inner, client);
                let rebuilt = build_display_panes_state(
                    &inner.engine,
                    overlay.source_pane,
                    overlay.state.duration_ms,
                );
                let Ok((source_session, source_window, state)) = rebuilt else {
                    overlay.cancel_deadline(client);
                    updates.push((client, None));
                    continue;
                };
                if attached_session != Some(overlay.source_session)
                    || source_session != overlay.source_session
                    || source_window != overlay.source_window
                    || active_window != Some(overlay.source_window)
                {
                    overlay.cancel_deadline(client);
                    updates.push((client, None));
                    continue;
                }
                if overlay.state != state {
                    overlay.state = state.clone();
                    updates.push((client, Some(state)));
                }
                inner.display_panes.insert(client, overlay);
            }
            updates
        };
        for (client, state) in updates {
            self.publish_to_client(client, EventPayload::DisplayPanes { state });
        }
    }

    fn refresh_terminal_visibility(&self) {
        let (changes, resizes) = {
            let mut inner = self.inner.lock();
            let attachments = inner
                .attached
                .iter()
                .flat_map(|(session, clients)| clients.iter().map(|client| (*session, *client)))
                .collect::<Vec<_>>();
            let mut changes = Vec::new();
            let mut affected_panes = BTreeSet::new();
            for (session, client) in attachments {
                let next = visible_terminal_panes(&inner, client, session);
                let previous = inner
                    .visible_terminals
                    .get(&client)
                    .cloned()
                    .unwrap_or_default();
                if next == previous {
                    continue;
                }
                affected_panes.extend(previous.iter().copied());
                affected_panes.extend(next.iter().copied());
                let removed = previous.difference(&next).copied().collect::<Vec<_>>();
                let view = TerminalViewId(client.0);
                let newly_visible = next
                    .difference(&previous)
                    .filter_map(|pane| {
                        terminal_viewport_for_pane(&inner, *pane, view)
                            .map(|(terminal, viewport)| (*pane, terminal, (*viewport).clone()))
                    })
                    .collect::<Vec<_>>();
                if let Some(subscriber) = inner.subscribers.get(&client).cloned() {
                    changes.push((subscriber, removed, newly_visible));
                }
                inner.visible_terminals.insert(client, next);
            }
            let resizes = terminal_resizes_for_panes(&inner, &affected_panes);
            (changes, resizes)
        };

        for (subscriber, removed, newly_visible) in changes {
            for pane in removed {
                subscriber.suspend_terminal(pane);
            }
            for (pane, terminal, viewport) in newly_visible {
                self.enqueue_kitty_images_for_viewport(&subscriber, pane, &terminal, &viewport);
                let message = Self::event(EventPayload::TerminalViewport { pane, viewport });
                if subscriber.enqueue_terminal(pane, &message) == TerminalEnqueue::NeedsFull {
                    let _ = subscriber.replace_terminal(pane, &message);
                }
            }
        }
        apply_terminal_resizes(resizes);
    }

    fn publish(&self, payload: EventPayload) {
        if let EventPayload::PaneRemoved(pane) = &payload {
            self.kitty_image_frames
                .lock()
                .retain(|key, _| key.pane != *pane);
            self.pasted_images.lock().remove(pane);
        }
        let message = Self::event(payload);
        let subscribers = self
            .inner
            .lock()
            .subscribers
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            let _ = subscriber.enqueue_reliable(&message);
        }
    }

    fn publish_to_client(&self, client: ClientId, payload: EventPayload) {
        let subscriber = self.inner.lock().subscribers.get(&client).cloned();
        if let Some(subscriber) = subscriber {
            Self::send_event(&subscriber, payload);
        }
    }

    fn next_gui_request_id() -> u64 {
        static REQUESTS: AtomicU64 = AtomicU64::new(1);
        REQUESTS.fetch_add(1, Ordering::Relaxed)
    }

    fn request_from_gui(
        &self,
        pane: PaneId,
        payload: impl FnOnce(u64) -> EventPayload,
    ) -> Result<String, DaemonError> {
        let request_id = Self::next_gui_request_id();
        let (reply, response) = crossbeam_channel::bounded(1);
        let subscriber = {
            let mut inner = self.inner.lock();
            let window = inner
                .engine
                .state
                .window_for_pane(pane)
                .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
            let session = inner.engine.state.windows[&window].session;
            let client = inner
                .attached
                .get(&session)
                .and_then(|clients| clients.iter().next())
                .copied()
                .ok_or(ServerError::PaneNotAttached(pane))?;
            let subscriber = inner
                .subscribers
                .get(&client)
                .cloned()
                .ok_or(ServerError::PaneNotAttached(pane))?;
            inner
                .pending_gui_requests
                .insert(request_id, PendingGuiRequest { client, reply });
            subscriber
        };
        let message = Self::event(payload(request_id));
        if !subscriber.enqueue_reliable(&message) {
            self.inner.lock().pending_gui_requests.remove(&request_id);
            return Err(ServerError::PaneNotAttached(pane).into());
        }
        let outcome = response.recv_timeout(GUI_REQUEST_TIMEOUT);
        self.inner.lock().pending_gui_requests.remove(&request_id);
        match outcome {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(message)) => Err(ServerError::InvalidCommand(message).into()),
            Err(_) => Err(ServerError::Internal(format!(
                "the zz window did not answer within {} seconds",
                GUI_REQUEST_TIMEOUT.as_secs()
            ))
            .into()),
        }
    }

    fn complete_gui_request(&self, client: ClientId, response: GuiResponse) {
        let request_id = response.request_id();
        let Some(pending) = self.inner.lock().pending_gui_requests.remove(&request_id) else {
            return;
        };
        if pending.client != client {
            log::warn!(
                target: "zz_daemon::diagnostics::connection",
                "discarding GUI response for request={request_id} from client={client}; owner is {}",
                pending.client,
            );
            return;
        }
        let _ = pending.reply.try_send(match response {
            GuiResponse::Success { output, .. } => Ok(output),
            GuiResponse::Error { message, .. } => Err(message),
        });
    }

    fn fail_gui_requests_for(&self, client: ClientId) {
        let pending = {
            let mut inner = self.inner.lock();
            let ids = inner
                .pending_gui_requests
                .iter()
                .filter_map(|(id, request)| (request.client == client).then_some(*id))
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| inner.pending_gui_requests.remove(&id))
                .collect::<Vec<_>>()
        };
        for request in pending {
            let _ = request
                .reply
                .try_send(Err("the zz window disconnected".to_owned()));
        }
    }

    fn publish_for_pane(&self, pane: PaneId, payload: &EventPayload) {
        let subscribers = {
            let inner = self.inner.lock();
            let Some(window) = inner.engine.state.window_for_pane(pane) else {
                return;
            };
            let session = inner.engine.state.windows[&window].session;
            inner
                .attached
                .get(&session)
                .into_iter()
                .flatten()
                .filter_map(|client| inner.subscribers.get(client).cloned())
                .collect::<Vec<_>>()
        };
        for subscriber in subscribers {
            Self::send_event(&subscriber, payload.clone());
        }
    }

    fn publish_terminal_for_pane(
        &self,
        pane: PaneId,
        client: ClientId,
        payload: TerminalFanout,
        current: &TerminalViewport,
        terminal: &TerminalSession,
    ) {
        let (subscriber, unclaimed) = {
            let mut inner = self.inner.lock();
            let unclaimed = reconcile_copy_session(&mut inner, pane, client, current.mode);
            let subscriber = inner
                .engine
                .state
                .window_for_pane(pane)
                .map(|window| inner.engine.state.windows[&window].session)
                .filter(|session| {
                    inner
                        .attached
                        .get(session)
                        .is_some_and(|clients| clients.contains(&client))
                        && inner
                            .visible_terminals
                            .get(&client)
                            .is_some_and(|visible| visible.contains(&pane))
                })
                .and_then(|_| inner.subscribers.get(&client).cloned());
            (subscriber, unclaimed)
        };
        if let Some(terminal) = unclaimed {
            terminal.view_action(
                TerminalViewId(client.0),
                zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel),
            );
        }
        let Some(subscriber) = subscriber else {
            return;
        };
        self.enqueue_kitty_images_for_viewport(&subscriber, pane, terminal, current);
        let sequence = Self::next_sequence();
        let result = match payload {
            TerminalFanout::Full => subscriber.enqueue_terminal_viewport(pane, sequence, current),
            TerminalFanout::Patch(patch) => {
                let message = ProtocolMessage::Event(Event {
                    sequence,
                    payload: EventPayload::TerminalPatch { pane, patch },
                });
                subscriber.enqueue_terminal(pane, &message)
            }
        };
        if result == TerminalEnqueue::NeedsFull {
            let _ = subscriber.replace_terminal_viewport(pane, Self::next_sequence(), current);
        }
    }

    fn deliver_clipboard_write(&self, pane: PaneId, target: ClipboardTarget, text: String) {
        let set_clipboard = self
            .inner
            .lock()
            .engine
            .mux_option_value(MuxOptionKey::SetClipboard);
        match set_clipboard.as_str() {
            "off" => return,
            "on" => {
                self.store_copy_buffer(text.clone(), PasteBufferAction::Create { prefix: None });
            }
            _ => {}
        }
        self.publish_for_pane(
            pane,
            &EventPayload::Clipboard {
                pane,
                request_id: 0,
                target,
                text,
            },
        );
    }

    fn raise_pane_bell(&self, pane: PaneId) {
        let raised = self.inner.lock().engine.state.set_pane_bell(pane, true);
        if raised {
            self.publish_snapshot();
            self.publish(EventPayload::Bell { pane });
        }
    }

    fn clear_pane_bell(&self, pane: PaneId) -> bool {
        let (cleared, terminal) = {
            let mut inner = self.inner.lock();
            let cleared = inner.engine.state.set_pane_bell(pane, false);
            let terminal = cleared
                .then(|| inner.terminals.get(&pane).cloned())
                .flatten();
            (cleared, terminal)
        };
        if let Some(terminal) = terminal {
            terminal.clear_bell();
        }
        cleared
    }

    fn store_copy_buffer(&self, data: String, action: PasteBufferAction) {
        if data.is_empty() {
            return;
        }
        let mut data = data.into_bytes();
        let mut inner = self.inner.lock();
        let (requested_name, prefix) = match action {
            PasteBufferAction::Create { prefix } => {
                (None, prefix.unwrap_or_else(|| "buffer".to_owned()))
            }
            PasteBufferAction::Append => {
                let requested_name = inner
                    .paste_buffers
                    .iter()
                    .find(|buffer| buffer.automatic)
                    .map(|buffer| {
                        let combined = buffer.data.len().saturating_add(data.len());
                        validate_paste_buffer_size(combined)?;
                        let mut appended = Vec::with_capacity(combined);
                        appended.extend_from_slice(&buffer.data);
                        appended.append(&mut data);
                        data = appended;
                        Ok::<_, ServerError>(buffer.name.clone())
                    })
                    .transpose();
                let requested_name = match requested_name {
                    Ok(name) => name,
                    Err(error) => {
                        log::warn!("could not append native copy buffer: {error}");
                        return;
                    }
                };
                (requested_name, "buffer".to_owned())
            }
        };
        if let Err(error) =
            insert_paste_buffer(&mut inner, requested_name.as_deref(), &prefix, data)
        {
            log::warn!("could not create native copy buffer: {error}");
            return;
        }
        drop(inner);
        self.refresh_choose_buffers();
    }

    fn spawn_copy_pipe(
        self: &Arc<Self>,
        pane: PaneId,
        client: ClientId,
        command: String,
        data: String,
    ) {
        let subscriber = self.inner.lock().subscribers.get(&client).cloned();
        let rejection = if command.is_empty() {
            Some("copy-pipe command is empty".to_owned())
        } else if command.len() > MAX_COPY_PIPE_COMMAND_BYTES {
            Some(format!(
                "copy-pipe command exceeds {MAX_COPY_PIPE_COMMAND_BYTES} bytes"
            ))
        } else if data.len() > MAX_COPY_PIPE_BYTES {
            Some(format!(
                "copy-pipe input exceeds {MAX_COPY_PIPE_BYTES} bytes"
            ))
        } else {
            let mut inner = self.inner.lock();
            if inner.active_copy_pipes >= MAX_COPY_PIPE_PROCESSES {
                Some(format!(
                    "copy-pipe process limit reached ({MAX_COPY_PIPE_PROCESSES})"
                ))
            } else {
                inner.active_copy_pipes += 1;
                None
            }
        };
        if let Some(message) = rejection {
            self.publish_copy_pipe_message(pane, subscriber.as_deref(), message);
            return;
        }

        let shared = Arc::clone(self);
        let worker_subscriber = subscriber.clone();
        let permit = CopyPipePermit {
            shared: Arc::clone(self),
        };
        if let Err(error) = thread::Builder::new()
            .name("zz-copy-pipe".to_owned())
            .spawn(move || {
                let result = run_copy_pipe(&command, &data);
                drop(permit);
                if let Err(error) = result {
                    shared.publish_copy_pipe_message(
                        pane,
                        worker_subscriber.as_deref(),
                        format!("copy-pipe failed: {error}"),
                    );
                }
            })
        {
            self.publish_copy_pipe_message(
                pane,
                subscriber.as_deref(),
                format!("copy-pipe failed to start worker: {error}"),
            );
        }
    }

    fn publish_copy_pipe_message(
        &self,
        pane: PaneId,
        subscriber: Option<&OutboundMailbox>,
        text: String,
    ) {
        log::warn!("{text}");
        let payload = EventPayload::ClientMessage {
            pane: Some(pane),
            kind: ClientMessageKind::Error,
            text,
        };
        if let Some(subscriber) = subscriber {
            Self::send_event(subscriber, payload);
        } else {
            self.publish_for_pane(pane, &payload);
        }
    }

    fn next_sequence() -> u64 {
        static SEQUENCE: AtomicU64 = AtomicU64::new(1);
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    }

    fn event(payload: EventPayload) -> ProtocolMessage {
        ProtocolMessage::Event(Event {
            sequence: Self::next_sequence(),
            payload,
        })
    }

    fn send_event(outbound: &OutboundMailbox, payload: EventPayload) {
        let _ = outbound.enqueue_reliable(&Self::event(payload));
    }

    fn set_config_overrides(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        entries: &[ConfigOverrideEntry],
    ) {
        let (appearance_entries, mux_entries) = partition_config_overrides(entries);

        let (color_scheme, restore) = {
            let mut inner = self.inner.lock();
            if kind != ClientKind::Interactive || !inner.subscribers.contains_key(&client) {
                log::warn!(
                    target: "zz_daemon::diagnostics::appearance",
                    "ignored configuration overrides from non-interactive client={client}",
                );
                return;
            }
            let previous_mux_keys = inner
                .mux_config_overrides
                .iter()
                .filter_map(|(key, _)| MuxOptionKey::from_config_key(key))
                .collect::<BTreeSet<_>>();
            let restore = previous_mux_keys
                .into_iter()
                .filter_map(|key| {
                    inner
                        .mux_option_underlay
                        .get(key)
                        .cloned()
                        .map(|value| (key, value))
                })
                .collect::<Vec<_>>();
            inner
                .appearance_config_overrides
                .clone_from(&appearance_entries);
            inner.mux_config_overrides.clone_from(&mux_entries);
            (
                inner
                    .client_color_schemes
                    .get(&client)
                    .copied()
                    .unwrap_or(inner.active_color_scheme),
                restore,
            )
        };

        let load = resolve_appearance(color_scheme, &appearance_entries);
        log_appearance_load("config-overrides", &load);
        {
            let appearance = Arc::new(load.appearance);
            let provenance = load.provenance;
            let (terminals, changed) = {
                let mut inner = self.inner.lock();
                if inner.appearance_config_overrides != appearance_entries {
                    return;
                }
                let changed =
                    *inner.appearance != *appearance || inner.appearance_provenance != provenance;
                inner.active_color_scheme = color_scheme;
                inner.appearance = Arc::clone(&appearance);
                inner.appearance_provenance.clone_from(&provenance);
                let terminals = if changed {
                    inner
                        .terminals
                        .values()
                        .chain(
                            inner
                                .command_outputs
                                .values()
                                .map(|output| &output.terminal),
                        )
                        .cloned()
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                (terminals, changed)
            };
            log::info!(
                target: "zz_daemon::diagnostics::appearance",
                "applied configuration override set entries={}",
                appearance_entries.len(),
            );
            if changed {
                for terminal in terminals {
                    terminal.set_appearance(Arc::clone(&appearance));
                }
                self.publish(EventPayload::AppearanceChanged {
                    appearance: Box::new((*appearance).clone()),
                    provenance,
                });
            }
        }

        self.restore_mux_option_underlay(&restore, "config-overrides");
        self.apply_mux_config_overrides(&mux_entries, "config-overrides");
    }

    fn restore_mux_option_underlay(
        self: &Arc<Self>,
        entries: &[(MuxOptionKey, zz_protocol::MuxOptionValue)],
        reason: &str,
    ) {
        let mut context = ExecutionContext::default();
        for (option, entry) in entries {
            let command = mux_set_option_command(*option, &entry.value);
            if let Err(error) = self.execute_with_mux_source(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &command,
                entry.source,
            ) {
                log::warn!(
                    target: "zz_daemon::diagnostics::mux_config",
                    "could not restore mux option underlay reason={reason} key={} error={error}",
                    option.as_str(),
                );
            }
        }
    }

    fn apply_mux_config_overrides(
        self: &Arc<Self>,
        entries: &[ConfigOverrideEntry],
        reason: &str,
    ) -> MuxOverrideApplyReport {
        let mut report = MuxOverrideApplyReport::default();
        let mut context = ExecutionContext::default();
        for (key, value) in entries {
            let Some(option) = MuxOptionKey::from_config_key(key) else {
                let diagnostic = format!("unsupported mux override key: {key}");
                log::warn!(
                    target: "zz_daemon::diagnostics::mux_config",
                    "mux override reason={reason} key={key:?} disposition=invalid message={diagnostic}",
                );
                report.diagnostics.push(diagnostic);
                continue;
            };
            let command = mux_set_option_command(option, value);
            match self.execute_with_mux_source(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &command,
                MuxOptionSource::Override,
            ) {
                Ok(_) => report.applied += 1,
                Err(error) => {
                    let diagnostic = error.to_string();
                    log::warn!(
                        target: "zz_daemon::diagnostics::mux_config",
                        "mux override reason={reason} key={} disposition=invalid message={diagnostic}",
                        option.as_str(),
                    );
                    report.diagnostics.push(diagnostic);
                }
            }
        }
        let (prefix, mode_keys) = {
            let inner = self.inner.lock();
            (
                inner.engine.mux_option_value(MuxOptionKey::Prefix),
                inner.engine.mux_option_value(MuxOptionKey::ModeKeys),
            )
        };
        log::info!(
            target: "zz_daemon::diagnostics::mux_config",
            "applied mux configuration override set reason={reason} entries={} prefix={prefix:?} mode_keys={mode_keys:?}",
            report.applied,
        );
        report
    }

    fn apply_stored_mux_config_overrides(self: &Arc<Self>, reason: &str) {
        let entries = self.inner.lock().mux_config_overrides.clone();
        self.apply_mux_config_overrides(&entries, reason);
    }

    fn set_client_color_scheme(
        self: &Arc<Self>,
        client: ClientId,
        color_scheme: TerminalColorScheme,
    ) {
        let (appearance_changed, appearance_config_overrides) = {
            let mut inner = self.inner.lock();
            if !inner.subscribers.contains_key(&client) {
                return;
            }
            inner.client_color_schemes.insert(client, color_scheme);
            inner.active_color_scheme = color_scheme;
            (
                inner.appearance.color_scheme != color_scheme,
                inner.appearance_config_overrides.clone(),
            )
        };
        log::debug!(
            target: "zz_daemon::diagnostics::appearance",
            "recorded system color scheme client={client} scheme={} appearance_changed={appearance_changed}",
            color_scheme.as_str(),
        );
        if !appearance_changed {
            return;
        }

        let load = resolve_appearance(color_scheme, &appearance_config_overrides);
        log_appearance_load("system-color-scheme", &load);
        let appearance = Arc::new(load.appearance);
        let provenance = load.provenance;
        let terminals = {
            let mut inner = self.inner.lock();
            if inner.active_color_scheme != color_scheme
                || !inner.subscribers.contains_key(&client)
                || inner.appearance_config_overrides != appearance_config_overrides
            {
                return;
            }
            inner.appearance = Arc::clone(&appearance);
            inner.appearance_provenance.clone_from(&provenance);
            inner
                .terminals
                .values()
                .chain(
                    inner
                        .command_outputs
                        .values()
                        .map(|output| &output.terminal),
                )
                .cloned()
                .collect::<Vec<_>>()
        };
        for terminal in terminals {
            terminal.set_appearance(Arc::clone(&appearance));
        }
        self.publish(EventPayload::AppearanceChanged {
            appearance: Box::new((*appearance).clone()),
            provenance,
        });
    }

    fn reload_user_config(
        self: &Arc<Self>,
        client: ClientId,
        context: &mut ExecutionContext,
    ) -> Result<(), DaemonError> {
        let mux_config = self
            .load_user_config
            .then(default_mux_config)
            .flatten()
            .filter(|path| path.is_file());
        self.reload_user_config_with_mux_file(client, context, mux_config.as_deref())
    }

    fn reload_user_config_with_mux_file(
        self: &Arc<Self>,
        client: ClientId,
        context: &mut ExecutionContext,
        mux_config: Option<&Path>,
    ) -> Result<(), DaemonError> {
        let (color_scheme, appearance_config_overrides) = {
            let inner = self.inner.lock();
            (
                inner
                    .client_color_schemes
                    .get(&client)
                    .copied()
                    .unwrap_or(inner.active_color_scheme),
                inner.appearance_config_overrides.clone(),
            )
        };
        let load = resolve_appearance(color_scheme, &appearance_config_overrides);
        log_appearance_load("reload", &load);
        self.inner.lock().engine.keys = KeyTables::default();
        let mut report = ConfigLoadReport::default();
        if let Some(config) = mux_config {
            self.load_config_file_with_report(config, context, 0, &mut report)?;
        }
        self.apply_stored_mux_config_overrides("reload-mux-replay");

        let appearance = Arc::new(load.appearance);
        let provenance = load.provenance;
        let terminals = {
            let mut inner = self.inner.lock();
            if inner.appearance_config_overrides != appearance_config_overrides {
                return Ok(());
            }
            inner.active_color_scheme = color_scheme;
            inner.appearance = Arc::clone(&appearance);
            inner.appearance_provenance.clone_from(&provenance);
            inner
                .terminals
                .values()
                .chain(
                    inner
                        .command_outputs
                        .values()
                        .map(|output| &output.terminal),
                )
                .cloned()
                .collect::<Vec<_>>()
        };
        for terminal in terminals {
            terminal.set_appearance(Arc::clone(&appearance));
        }
        self.publish(EventPayload::AppearanceChanged {
            appearance: Box::new((*appearance).clone()),
            provenance,
        });
        let (kind, text) = match report.summary() {
            Some(summary) => (
                ClientMessageKind::Warning,
                format!("Reloaded zz configuration; {summary}"),
            ),
            None => (
                ClientMessageKind::Success,
                "Reloaded zz configuration".to_owned(),
            ),
        };
        self.publish_to_client(
            client,
            EventPayload::ClientMessage {
                pane: context.pane,
                kind,
                text,
            },
        );
        self.publish_key_tables_if_changed();
        Ok(())
    }

    fn load_config_file(
        self: &Arc<Self>,
        path: &Path,
        context: &mut ExecutionContext,
        depth: usize,
    ) -> Result<(), DaemonError> {
        self.load_config_file_with_report(path, context, depth, &mut ConfigLoadReport::default())
    }

    fn load_config_file_with_report(
        self: &Arc<Self>,
        path: &Path,
        context: &mut ExecutionContext,
        depth: usize,
        report: &mut ConfigLoadReport,
    ) -> Result<(), DaemonError> {
        if depth >= MAX_CONFIG_DEPTH {
            log::warn!(
                "ignoring config source past depth {MAX_CONFIG_DEPTH}: {}",
                path.display()
            );
            return Ok(());
        }
        let input = match fs::read_to_string(path) {
            Ok(input) => input,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                log::warn!("ignoring missing config file: {}", path.display());
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        let parsed = parse_config(path.display().to_string(), &input);
        for diagnostic in parsed.diagnostics {
            log::warn!(
                "{}:{}:{}: {}",
                diagnostic.source,
                diagnostic.line,
                diagnostic.column,
                diagnostic.message
            );
            report.note_invalid(&diagnostic.message);
        }
        for command in parsed.commands {
            if command.name == "reload-config" {
                log::warn!(
                    "{}: ignoring reload-config while loading configuration",
                    path.display()
                );
                continue;
            }
            if matches!(command.name.as_str(), "source" | "source-file") {
                let source_effects = {
                    let mut inner = self.inner.lock();
                    inner.engine.execute(context, &command)
                };
                let source_effects = match source_effects {
                    Ok(execution) => execution
                        .effects
                        .into_iter()
                        .filter_map(|effect| match effect {
                            MuxEffect::SourceFile { path, quiet } => Some((path, quiet)),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                    Err(ServerError::UnsupportedCommand(command)) => {
                        log::warn!(
                            "{}: ignoring unsupported tmux command: {command}",
                            path.display()
                        );
                        report.note_skip(&command);
                        continue;
                    }
                    Err(ServerError::InvalidCommand(message)) => {
                        log::warn!(
                            "{}: ignoring invalid tmux command: {message}",
                            path.display()
                        );
                        report.note_invalid(&message);
                        continue;
                    }
                    Err(error) => {
                        log::warn!("{}: ignoring tmux command error: {error}", path.display());
                        continue;
                    }
                };
                if source_effects.is_empty() {
                    log::warn!("{}: ignoring source-file without a path", path.display());
                }
                let mut source_error = None;
                for (source, quiet) in source_effects {
                    if source == "-" {
                        log::warn!("source-file from standard input is not supported");
                        report.note_invalid("source-file from standard input is not supported");
                        continue;
                    }
                    let source = expand_relative(path, &source);
                    let matches = source_glob_matches(&source);
                    for error in &matches.errors {
                        log::warn!("source-file glob error for {}: {error}", source.display());
                    }
                    if matches.paths.is_empty() && matches.errors.is_empty() {
                        if !quiet {
                            log::warn!("no such file: {}", source.display());
                        }
                        continue;
                    }
                    for source in matches.paths {
                        if let Err(error) =
                            self.load_config_file_with_report(&source, context, depth + 1, report)
                            && source_error.is_none()
                        {
                            source_error = Some(error);
                        }
                    }
                }
                if let Some(error) = source_error {
                    return Err(error);
                }
                continue;
            }
            match self.execute_with_mux_source(
                ClientId(u64::MAX),
                ClientKind::Command,
                context,
                &command,
                MuxOptionSource::TmuxConfig,
            ) {
                Ok(_) => {}
                Err(DaemonError::Server(ServerError::UnsupportedCommand(command))) => {
                    log::warn!(
                        "{}: ignoring unsupported tmux command: {command}",
                        path.display()
                    );
                    report.note_skip(&command);
                }
                Err(DaemonError::Server(ServerError::InvalidCommand(message))) => {
                    log::warn!(
                        "{}: ignoring invalid tmux command: {message}",
                        path.display()
                    );
                    report.note_invalid(&message);
                }
                Err(DaemonError::Server(error)) => {
                    log::warn!("{}: ignoring tmux command error: {error}", path.display());
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

/// Everything the daemon owns on behalf of agent panes. Nothing in here may be
/// reached while the daemon's own state lock is held: the runtime calls back
/// into it from its pane threads.
#[cfg(feature = "agent")]
impl Shared {
    fn agent_runtime(self: &Arc<Self>) -> Option<Arc<AgentRuntime>> {
        if self.agent_stopped.load(Ordering::Acquire) {
            return None;
        }
        if let Some(runtime) = self.agent.lock().as_ref() {
            return Some(Arc::clone(runtime));
        }
        // Tests never journal and never spawn an adapter: the pane opens
        // against a runner that reports there is none, and whoever wants a
        // real conversation installs a fixture first.
        let journal = if cfg!(test) {
            None
        } else {
            load_persistent_journal()
        };
        self.build_agent_runtime(journal)
    }

    fn build_agent_runtime(
        self: &Arc<Self>,
        journal: Option<Arc<crate::agent::journal::AgentJournal>>,
    ) -> Option<Arc<AgentRuntime>> {
        let config = self.agent_spawn_config();
        let mut slot = self.agent.lock();
        if self.agent_stopped.load(Ordering::Acquire) {
            return None;
        }
        if let Some(runtime) = slot.as_ref() {
            return Some(Arc::clone(runtime));
        }
        let publisher: Arc<dyn AgentPublisher> = Arc::<Self>::clone(self);
        let runtime = Arc::new(AgentRuntime::new(&publisher, config, journal));
        runtime.prewarm();
        #[cfg(test)]
        runtime.set_runner_factory(Box::new(|_| {
            Box::new(|_| Box::pin(async { Err("no agent adapter in tests".to_owned()) }))
        }));
        *slot = Some(Arc::clone(&runtime));
        Some(runtime)
    }

    /// The runtime only if a pane has already asked for one.
    fn open_agent_runtime(&self) -> Option<Arc<AgentRuntime>> {
        self.agent.lock().clone()
    }

    fn agent_spawn_config(&self) -> AgentSpawnConfig {
        let inner = self.inner.lock();
        let options = inner.engine.agent_options();
        AgentSpawnConfig {
            command: options.command.clone(),
            claude_code_command: options.claude_code_command.clone(),
            auto_approve: options.auto_approve,
            workspace: AgentWorkspaceEnvironment {
                pane: None,
                session: None,
                socket: Some(self.socket_path.display().to_string()),
            },
        }
    }

    fn reconfigure_agents(&self) {
        let _effects = self.agent_effects.lock();
        let config = self.agent_spawn_config();
        let Some(runtime) = self.open_agent_runtime() else {
            return;
        };
        runtime.reconfigure(config);
    }

    /// Start the adapter for a pane the mux just materialized.
    fn open_agent_pane(self: &Arc<Self>, pane: PaneId) {
        let _effects = self.agent_effects.lock();
        let Some(spec) = self.agent_pane_spec(pane) else {
            return;
        };
        let Some(runtime) = self.agent_runtime() else {
            return;
        };
        runtime.reconfigure(self.agent_spawn_config());
        if !runtime.open(pane, spec) {
            log::warn!(target: "zz::agent", "{pane} already has an agent runtime");
        }
    }

    fn restart_agent_pane(self: &Arc<Self>, pane: PaneId) {
        let _effects = self.agent_effects.lock();
        let Some(spec) = self.agent_pane_spec(pane) else {
            return;
        };
        let Some(runtime) = self.agent_runtime() else {
            return;
        };
        runtime.reconfigure(self.agent_spawn_config());
        if !runtime.restart(pane, spec) {
            log::warn!(target: "zz::agent", "could not restart the agent runtime for {pane}");
        }
    }

    fn agent_pane_spec(&self, pane: PaneId) -> Option<AgentPaneSpec> {
        let inner = self.inner.lock();
        let PaneKind::Agent(descriptor) = &inner.engine.state.pane(pane)?.kind else {
            return None;
        };
        let session = inner
            .engine
            .state
            .window_for_pane(pane)
            .map(|window| inner.engine.state.windows[&window].session.to_string());
        Some(AgentPaneSpec {
            provider: descriptor.provider,
            cwd: descriptor
                .cwd
                .clone()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("/")),
            resume_session: descriptor.session_id.clone(),
            workspace: AgentWorkspaceEnvironment {
                pane: Some(pane.to_string()),
                session,
                socket: None,
            },
        })
    }

    fn close_agent_panes(&self, panes: &[PaneId]) {
        let _effects = self.agent_effects.lock();
        let Some(runtime) = self.open_agent_runtime() else {
            return;
        };
        for pane in panes {
            runtime.close(*pane);
        }
    }

    fn shutdown_agents(&self) {
        let runtime = {
            let _effects = self.agent_effects.lock();
            self.agent_stopped.store(true, Ordering::Release);
            self.agent.lock().take()
        };
        let Some(runtime) = runtime else {
            return;
        };
        runtime.shutdown();
    }

    /// One client's answer to an agent message, or the reason it was refused.
    fn agent_message(
        self: &Arc<Self>,
        client: ClientId,
        message: ProtocolMessage,
    ) -> Result<(), ServerError> {
        let pane = agent_message_pane(&message)?;
        {
            let inner = self.inner.lock();
            match inner.engine.state.pane(pane).map(|pane| &pane.kind) {
                Some(PaneKind::Agent(_)) => {}
                Some(_) => {
                    return Err(ServerError::InvalidTarget(format!(
                        "{pane} is not an agent pane"
                    )));
                }
                None => return Err(ServerError::MissingTarget(pane.to_string())),
            }
        };
        if let ProtocolMessage::AgentSessionOp { op, .. } = &message {
            let valid_directory = |path: &Path| {
                path.is_absolute()
                    && path.as_os_str().as_encoded_bytes().len() <= MAX_GUI_TEXT_BYTES
            };
            let valid = match op {
                AgentSessionOpKind::List { cwd, .. } => cwd.as_deref().is_none_or(valid_directory),
                AgentSessionOpKind::New { cwd } => valid_directory(cwd),
                AgentSessionOpKind::Switch {
                    cwd,
                    additional_directories,
                    ..
                } => {
                    valid_directory(cwd)
                        && additional_directories.len() <= MAX_AGENT_SESSION_DIRECTORIES
                        && additional_directories
                            .iter()
                            .all(|path| valid_directory(path))
                }
                AgentSessionOpKind::Delete { .. } => true,
            };
            if !valid {
                return Err(ServerError::InvalidCommand(
                    "agent session directories must be absolute on the daemon host".to_owned(),
                ));
            }
        }
        let Some(runtime) = self.agent_runtime() else {
            return Err(ServerError::PaneExited(pane));
        };
        let command = match message {
            ProtocolMessage::AgentReplay { from_seq, .. } => {
                runtime.replay(client, pane, from_seq);
                return Ok(());
            }
            ProtocolMessage::AgentAcknowledgePromptRestore { reclaim_id, .. } => {
                let owner = self
                    .client_instance_id(client)
                    .ok_or(ServerError::PaneExited(pane))?;
                runtime.acknowledge_prompt_restore(owner, pane, reclaim_id);
                return Ok(());
            }
            ProtocolMessage::AgentPrompt { text, images, .. } => {
                let owner = self
                    .client_instance_id(client)
                    .ok_or(ServerError::PaneExited(pane))?;
                return runtime
                    .prompt(
                        pane,
                        AgentPrompt {
                            owner,
                            text,
                            images: images
                                .into_iter()
                                .map(|image| AgentImage {
                                    format: image.format,
                                    data: image.data,
                                })
                                .collect(),
                        },
                    )
                    .then_some(())
                    .ok_or(ServerError::PaneExited(pane));
            }
            ProtocolMessage::AgentCancel { .. } => HostCommand::Cancel,
            ProtocolMessage::AgentUnqueue { .. } => HostCommand::Unqueue,
            ProtocolMessage::AgentRespondPermission {
                request_id,
                option_id,
                ..
            } => HostCommand::RespondPermission {
                request_id,
                option_id,
            },
            ProtocolMessage::AgentSetConfigOption {
                option_id, value, ..
            } => HostCommand::SetConfigOption { option_id, value },
            ProtocolMessage::AgentSetMode { mode_id, .. } => HostCommand::SetMode { mode_id },
            ProtocolMessage::AgentAuthenticate { method_id, .. } => {
                HostCommand::Authenticate { method_id }
            }
            ProtocolMessage::AgentSessionOp { op, .. } => match op {
                AgentSessionOpKind::List {
                    cwd,
                    cursor,
                    replace,
                } => HostCommand::ListSessions {
                    client,
                    cwd,
                    cursor,
                    replace,
                },
                AgentSessionOpKind::New { cwd } => HostCommand::NewSession { cwd },
                AgentSessionOpKind::Switch {
                    session_id,
                    cwd,
                    additional_directories,
                } => HostCommand::SwitchSession {
                    session: AgentSessionSummary {
                        session_id,
                        cwd,
                        additional_directories,
                        title: None,
                        updated_at: None,
                    },
                },
                AgentSessionOpKind::Delete { session_id } => {
                    HostCommand::DeleteSession { client, session_id }
                }
            },
            _ => return Ok(()),
        };
        runtime
            .command(pane, command)
            .then_some(())
            .ok_or(ServerError::PaneExited(pane))
    }

    /// Dispatch `agent-send --submit` straight into the pane's runtime; the
    /// composer half still round-trips through the GUI that owns the draft.
    fn submit_agent_prompt(self: &Arc<Self>, pane: PaneId, text: String) -> bool {
        self.agent_runtime().is_some_and(|runtime| {
            runtime.prompt(
                pane,
                AgentPrompt {
                    owner: ClientInstanceId(u64::MAX),
                    text,
                    images: Vec::new(),
                },
            )
        })
    }

    /// Push the pane state of every agent pane a reattaching client can see.
    /// The stream itself is not pushed: the client asks for the replay it
    /// wants from the sequence it kept.
    fn send_agent_resync(&self, client: ClientId, outbound: &OutboundMailbox) {
        let Some(runtime) = self.open_agent_runtime() else {
            return;
        };
        let panes = {
            let inner = self.inner.lock();
            let Some(session) = client_attached_session(&inner, client) else {
                return;
            };
            session_agent_panes(&inner, session)
        };
        for pane in panes {
            let Some(state) = runtime.wire_state(pane) else {
                continue;
            };
            Self::send_event(outbound, EventPayload::AgentState { pane, state });
        }
    }

    /// A pane entering a client's visible set gets its state pushed; the
    /// client replays the stream from the sequence it cached.
    fn refresh_agent_visibility(&self) {
        let changes = {
            let mut inner = self.inner.lock();
            let attachments = inner
                .attached
                .iter()
                .flat_map(|(session, clients)| clients.iter().map(|client| (*session, *client)))
                .collect::<Vec<_>>();
            let mut changes = Vec::new();
            for (session, client) in attachments {
                let next = visible_agent_panes(&inner, client, session);
                let previous = inner
                    .visible_agents
                    .get(&client)
                    .cloned()
                    .unwrap_or_default();
                if next == previous {
                    continue;
                }
                let removed = previous.difference(&next).copied().collect::<Vec<_>>();
                let entered = next.difference(&previous).copied().collect::<Vec<_>>();
                if let Some(subscriber) = inner.subscribers.get(&client).cloned() {
                    changes.push((subscriber, removed, entered));
                }
                inner.visible_agents.insert(client, next);
            }
            changes
        };
        if changes.is_empty() {
            return;
        }
        let runtime = self.open_agent_runtime();
        for (subscriber, removed, entered) in changes {
            for pane in removed {
                subscriber.cancel_agent(pane);
            }
            let Some(runtime) = runtime.as_ref() else {
                continue;
            };
            for pane in entered {
                let Some(state) = runtime.wire_state(pane) else {
                    continue;
                };
                Self::send_event(&subscriber, EventPayload::AgentState { pane, state });
            }
        }
    }
}

#[cfg(feature = "agent")]
impl AgentPublisher for Shared {
    fn publish_agent_updates(
        &self,
        pane: PaneId,
        first_seq: u64,
        items: Vec<Vec<u8>>,
        also: Option<ClientId>,
    ) {
        let message = Self::event(EventPayload::AgentUpdates {
            pane,
            first_seq,
            items,
        });
        let subscribers = {
            let inner = self.inner.lock();
            let mut clients = attached_clients_for_pane(&inner, pane)
                .into_iter()
                .flatten()
                .filter(|client| {
                    inner
                        .visible_agents
                        .get(client)
                        .is_some_and(|visible| visible.contains(&pane))
                })
                .copied()
                .collect::<BTreeSet<_>>();
            clients.extend(also.filter(|client| client_is_attached_to_pane(&inner, *client, pane)));
            clients
                .into_iter()
                .filter_map(|client| inner.subscribers.get(&client).cloned())
                .collect::<Vec<_>>()
        };
        for subscriber in subscribers {
            let Ok(encoded) = subscriber.encode_message(&message) else {
                log::error!("failed to encode an agent update batch for {pane}");
                continue;
            };
            subscriber.enqueue_agent(pane, first_seq, encoded);
        }
    }

    fn send_agent_replay(&self, client: ClientId, pane: PaneId, frames: Vec<(u64, Vec<Vec<u8>>)>) {
        let Some(subscriber) = self.inner.lock().subscribers.get(&client).cloned() else {
            return;
        };
        let mut encoded = Vec::with_capacity(frames.len());
        for (first_seq, items) in frames {
            let message = Self::event(EventPayload::AgentUpdates {
                pane,
                first_seq,
                items,
            });
            let Ok(frame) = subscriber.encode_message(&message) else {
                log::error!("failed to encode an agent replay batch for {pane}");
                return;
            };
            encoded.push((first_seq, frame));
        }
        subscriber.enqueue_agent_replay(pane, encoded);
    }

    fn publish_agent_replay(
        &self,
        pane: PaneId,
        frames: Vec<(u64, Vec<Vec<u8>>)>,
        also: Option<ClientId>,
    ) {
        let subscribers = {
            let inner = self.inner.lock();
            let mut clients = attached_clients_for_pane(&inner, pane)
                .into_iter()
                .flatten()
                .filter(|client| {
                    inner
                        .visible_agents
                        .get(client)
                        .is_some_and(|visible| visible.contains(&pane))
                })
                .copied()
                .collect::<BTreeSet<_>>();
            clients.extend(also.filter(|client| client_is_attached_to_pane(&inner, *client, pane)));
            clients
                .into_iter()
                .filter_map(|client| inner.subscribers.get(&client).cloned())
                .collect::<Vec<_>>()
        };
        for subscriber in subscribers {
            let mut encoded = Vec::with_capacity(frames.len());
            for (first_seq, items) in &frames {
                let message = Self::event(EventPayload::AgentUpdates {
                    pane,
                    first_seq: *first_seq,
                    items: items.clone(),
                });
                let Ok(frame) = subscriber.encode_message(&message) else {
                    log::error!("failed to encode an agent replay batch for {pane}");
                    encoded.clear();
                    break;
                };
                encoded.push((*first_seq, frame));
            }
            if !encoded.is_empty() {
                subscriber.enqueue_agent_replay(pane, encoded);
            }
        }
    }

    fn publish_agent_state(&self, pane: PaneId, state: AgentPaneWire) {
        self.publish_for_pane(pane, &EventPayload::AgentState { pane, state });
    }

    fn send_agent_reply(&self, pane: PaneId, reply: AgentRequestReply) {
        let AgentRequestReply::Sessions { client, result } = reply;
        let payload = EventPayload::AgentSessions {
            pane,
            request_id: 0,
            result,
        };
        let subscriber = self.inner.lock().subscribers.get(&client).cloned();
        if let Some(subscriber) = subscriber {
            Self::send_event(&subscriber, payload);
        }
    }

    fn adopt_agent_session(
        &self,
        pane: PaneId,
        provider: AgentProvider,
        session_id: String,
        cwd: Option<PathBuf>,
    ) {
        let changed = {
            let mut inner = self.inner.lock();
            let matches_provider = inner.engine.state.pane(pane).is_some_and(
                |pane| matches!(&pane.kind, PaneKind::Agent(agent) if agent.provider == provider),
            );
            if !matches_provider {
                return;
            }
            match inner
                .engine
                .state
                .update_agent_session(pane, session_id, cwd)
            {
                Ok(()) => true,
                Err(error) => {
                    log::debug!(target: "zz::agent", "could not adopt the session for {pane}: {error}");
                    false
                }
            }
        };
        if changed {
            self.publish_snapshot();
        }
    }

    fn title_agent_pane(&self, pane: PaneId, title: String) {
        let changed = {
            let mut inner = self.inner.lock();
            let keeps_default_title = inner
                .engine
                .state
                .pane(pane)
                .is_some_and(|pane| is_default_agent_title(&pane.title));
            keeps_default_title
                && inner
                    .engine
                    .state
                    .update_pane_title(pane, title)
                    .unwrap_or(false)
        };
        if changed {
            self.publish_snapshot();
        }
    }
}

/// Which pane an agent message addresses.
#[cfg(feature = "agent")]
fn agent_message_pane(message: &ProtocolMessage) -> Result<PaneId, ServerError> {
    match message {
        ProtocolMessage::AgentPrompt { pane, .. }
        | ProtocolMessage::AgentCancel { pane }
        | ProtocolMessage::AgentUnqueue { pane }
        | ProtocolMessage::AgentRespondPermission { pane, .. }
        | ProtocolMessage::AgentSetConfigOption { pane, .. }
        | ProtocolMessage::AgentSetMode { pane, .. }
        | ProtocolMessage::AgentAuthenticate { pane, .. }
        | ProtocolMessage::AgentSessionOp { pane, .. }
        | ProtocolMessage::AgentReplay { pane, .. }
        | ProtocolMessage::AgentAcknowledgePromptRestore { pane, .. } => Ok(*pane),
        _ => Err(ServerError::InvalidCommand(
            "not an agent message".to_owned(),
        )),
    }
}

#[cfg(feature = "agent")]
fn session_agent_panes(inner: &ServerState, session: SessionId) -> Vec<PaneId> {
    inner
        .engine
        .state
        .sessions
        .get(&session)
        .into_iter()
        .flat_map(|session| session.windows.iter())
        .filter_map(|window| inner.engine.state.windows.get(window))
        .flat_map(|window| window.panes.keys())
        .filter(|pane| {
            inner
                .engine
                .state
                .pane(**pane)
                .is_some_and(|pane| matches!(pane.kind, PaneKind::Agent(_)))
        })
        .copied()
        .collect()
}

#[derive(Default)]
struct ConfigLoadReport {
    skipped_count: usize,
    skipped: Vec<String>,
    invalid_count: usize,
    invalid: Vec<String>,
}

impl ConfigLoadReport {
    const SUMMARY_NAMES: usize = 6;

    fn note_skip(&mut self, name: &str) {
        self.skipped_count += 1;
        if !self.skipped.iter().any(|existing| existing == name) {
            self.skipped.push(name.to_owned());
        }
    }

    fn note_invalid(&mut self, message: &str) {
        self.invalid_count += 1;
        if !self.invalid.iter().any(|existing| existing == message) {
            self.invalid.push(message.to_owned());
        }
    }

    fn summary(&self) -> Option<String> {
        if self.skipped_count == 0 && self.invalid_count == 0 {
            return None;
        }
        let mut parts = Vec::new();
        if self.skipped_count != 0 {
            parts.push(format!(
                "skipped {} unsupported tmux command{}: {}",
                self.skipped_count,
                if self.skipped_count == 1 { "" } else { "s" },
                Self::summary_names(&self.skipped),
            ));
        }
        if self.invalid_count != 0 {
            parts.push(format!(
                "{} invalid line{}: {}",
                self.invalid_count,
                if self.invalid_count == 1 { "" } else { "s" },
                Self::summary_names(&self.invalid),
            ));
        }
        Some(parts.join("; "))
    }

    fn summary_names(entries: &[String]) -> String {
        let mut names =
            entries
                .iter()
                .take(Self::SUMMARY_NAMES)
                .fold(String::new(), |mut names, name| {
                    if !names.is_empty() {
                        names.push_str(", ");
                    }
                    names.push_str(name);
                    names
                });
        if entries.len() > Self::SUMMARY_NAMES {
            names.push_str(", …");
        }
        names
    }
}

#[derive(Default)]
struct ServerState {
    engine: MuxEngine,
    last_published_mux_generation: u64,
    appearance: Arc<TerminalAppearance>,
    appearance_provenance: AppearanceProvenance,
    appearance_config_overrides: Vec<ConfigOverrideEntry>,
    mux_config_overrides: Vec<ConfigOverrideEntry>,
    mux_options: MuxOptions,
    mux_option_underlay: MuxOptions,
    key_tables: Vec<zz_protocol::KeyTableSnapshot>,
    active_color_scheme: TerminalColorScheme,
    client_color_schemes: BTreeMap<ClientId, TerminalColorScheme>,
    client_names: BTreeMap<ClientId, String>,
    client_instances: BTreeMap<ClientId, ClientInstanceId>,
    terminals: BTreeMap<PaneId, Arc<TerminalSession>>,
    terminal_spawns: BTreeMap<PaneId, TerminalSpawn>,
    command_outputs: BTreeMap<ClientId, CommandOutputSession>,
    subscribers: BTreeMap<ClientId, Arc<OutboundMailbox>>,
    attached: BTreeMap<SessionId, BTreeSet<ClientId>>,
    visible_terminals: BTreeMap<ClientId, BTreeSet<PaneId>>,
    visible_agents: BTreeMap<ClientId, BTreeSet<PaneId>>,
    focused_windows: BTreeMap<ClientId, WindowId>,
    terminal_geometries: BTreeMap<PaneId, BTreeMap<ClientId, TerminalGeometry>>,
    terminal_input_sequence: u64,
    client_terminal_input_sequences: BTreeMap<ClientId, u64>,
    key_engines: BTreeMap<ClientId, KeyEngine>,
    copy_sessions: BTreeMap<ClientId, CopySession>,
    prefix_armed: BTreeSet<ClientId>,
    swallowed_keys: BTreeMap<ClientId, BTreeSet<String>>,
    suppressed_text: BTreeMap<ClientId, BTreeMap<char, u32>>,
    command_prompts: BTreeMap<ClientId, CommandPrompt>,
    choose_trees: BTreeMap<ClientId, ChooseTreeSession>,
    choose_buffers: BTreeMap<ClientId, ChooseBufferSession>,
    display_panes: BTreeMap<ClientId, DisplayPanesSession>,
    command_history: Vec<String>,
    message_log: VecDeque<ServerMessage>,
    next_message_number: u64,
    paste_buffers: Vec<PasteBuffer>,
    automatic_paste_buffer_limit: AutomaticPasteBufferLimit,
    active_copy_pipes: usize,
    next_buffer_id: u64,
    next_client_id: u64,
    next_display_panes_token: u64,
    pending_gui_requests: BTreeMap<u64, PendingGuiRequest>,
    paste_uploads: BTreeMap<(ClientId, u64), PasteUpload>,
}

#[derive(Debug)]
struct PasteUpload {
    pane: PaneId,
    purpose: PasteUploadPurpose,
    extension: String,
    total_bytes: usize,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CopySession {
    pane: PaneId,
    observed: bool,
    scroll_exit: bool,
}

struct DiagnosticSample {
    mux: MuxSnapshot,
    terminals: Vec<(PaneId, Arc<TerminalSession>)>,
    command_outputs: Vec<(ClientId, PaneId, Arc<TerminalSession>)>,
    subscribers: Vec<(ClientId, Arc<OutboundMailbox>)>,
    attached: BTreeMap<SessionId, BTreeSet<ClientId>>,
    visible_terminals: BTreeMap<ClientId, BTreeSet<PaneId>>,
    key_clients: Vec<ClientId>,
    copy_sessions: BTreeMap<ClientId, CopySession>,
    swallowed_keys: BTreeMap<ClientId, BTreeSet<String>>,
    suppressed_text: BTreeMap<ClientId, BTreeMap<char, u32>>,
    command_prompts: Vec<ClientId>,
    choose_trees: Vec<ClientId>,
    choose_buffers: Vec<ClientId>,
    display_panes: Vec<ClientId>,
    command_history: Vec<String>,
    paste_buffers: Vec<PasteBuffer>,
    active_copy_pipes: usize,
}

impl DiagnosticSample {
    fn capture(inner: &ServerState) -> Self {
        Self {
            mux: inner.engine.state.snapshot(),
            terminals: inner
                .terminals
                .iter()
                .map(|(pane, terminal)| (*pane, Arc::clone(terminal)))
                .collect(),
            command_outputs: inner
                .command_outputs
                .iter()
                .map(|(client, output)| (*client, output.pane, Arc::clone(&output.terminal)))
                .collect(),
            subscribers: inner
                .subscribers
                .iter()
                .map(|(client, subscriber)| (*client, Arc::clone(subscriber)))
                .collect(),
            attached: inner.attached.clone(),
            visible_terminals: inner.visible_terminals.clone(),
            key_clients: inner.key_engines.keys().copied().collect(),
            copy_sessions: inner.copy_sessions.clone(),
            swallowed_keys: inner.swallowed_keys.clone(),
            suppressed_text: inner.suppressed_text.clone(),
            command_prompts: inner.command_prompts.keys().copied().collect(),
            choose_trees: inner.choose_trees.keys().copied().collect(),
            choose_buffers: inner.choose_buffers.keys().copied().collect(),
            display_panes: inner.display_panes.keys().copied().collect(),
            command_history: inner.command_history.clone(),
            paste_buffers: inner.paste_buffers.clone(),
            active_copy_pipes: inner.active_copy_pipes,
        }
    }
}

#[derive(Debug)]
struct PendingGuiRequest {
    client: ClientId,
    reply: crossbeam_channel::Sender<Result<String, String>>,
}

#[derive(Clone, Copy, Debug)]
struct AutomaticPasteBufferLimit(usize);

impl Default for AutomaticPasteBufferLimit {
    fn default() -> Self {
        Self(DEFAULT_BUFFER_LIMIT)
    }
}

#[derive(Debug)]
struct DisplayPanesSession {
    token: u64,
    source_pane: PaneId,
    source_session: SessionId,
    source_window: WindowId,
    state: DisplayPanesState,
    deadline: Option<Instant>,
    cancel: Option<crossbeam_channel::Sender<DisplayPanesDeadlineCommand>>,
}

impl DisplayPanesSession {
    fn cancel_deadline(&self, client: ClientId) {
        if let Some(cancel) = &self.cancel {
            let _ = cancel.try_send(DisplayPanesDeadlineCommand::Cancel {
                client,
                token: self.token,
            });
        }
    }
}

fn take_display_panes(inner: &mut ServerState, client: ClientId) -> Option<DisplayPanesSession> {
    let overlay = inner.display_panes.remove(&client)?;
    overlay.cancel_deadline(client);
    Some(overlay)
}

fn build_display_panes_state(
    engine: &MuxEngine,
    source_pane: PaneId,
    duration_ms: u32,
) -> Result<(SessionId, WindowId, DisplayPanesState), ServerError> {
    let state = &engine.state;
    let window_id = state
        .window_for_pane(source_pane)
        .ok_or_else(|| ServerError::MissingTarget(source_pane.to_string()))?;
    let window = state
        .windows
        .get(&window_id)
        .ok_or_else(|| ServerError::MissingTarget(window_id.to_string()))?;
    let panes = window.pane_order().to_vec();
    if panes.len() > MAX_DISPLAY_PANE_INDICATORS {
        return Err(ServerError::Internal(format!(
            "window {window_id} exceeds the pane indicator limit"
        )));
    }
    let indicators = panes
        .into_iter()
        .filter(|pane| window.zoomed_pane.is_none_or(|zoomed| *pane == zoomed))
        .map(|pane| {
            let index = engine
                .pane_index(window_id, pane)
                .expect("window pane has an effective index");
            let select_key = match index {
                0..=9 => b'0' + u8::try_from(index).expect("digit pane index"),
                10..=35 => b'a' + u8::try_from(index - 10).expect("letter pane index"),
                _ => 0,
            };
            PaneIndicator {
                pane,
                index,
                select_key,
                flags: if pane == window.active_pane {
                    PaneIndicator::ACTIVE
                } else {
                    0
                },
            }
        })
        .collect();
    Ok((
        window.session,
        window_id,
        DisplayPanesState {
            window: window_id,
            duration_ms,
            indicators,
        },
    ))
}

fn bare_key_name(key: &str) -> &str {
    let mut bare = key;
    while let Some(rest) = bare.strip_prefix("C-").or_else(|| bare.strip_prefix("M-")) {
        bare = rest;
    }
    bare
}

fn display_panes_selection_key(input: &zz_terminal::KeyInput) -> Option<u8> {
    if input.action != zz_terminal::KeyAction::Press
        || input.modifiers != zz_terminal::Modifiers::default()
    {
        return None;
    }
    let character = input
        .text
        .as_deref()
        .and_then(|text| {
            let mut characters = text.chars();
            let character = characters.next()?;
            characters.next().is_none().then_some(character)
        })
        .or(match input.key {
            zz_terminal::KeyCode::Character(character) => Some(character),
            _ => None,
        })?;
    character
        .is_ascii()
        .then(|| u8::try_from(u32::from(character)).expect("ASCII character fits in u8"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ChooseBufferResult {
    Updated,
    Close,
    Paste(String),
    Delete(String),
}

enum ChooseBufferInputOutcome {
    Delta {
        search: Option<ChooseBufferSearchState>,
        selected: u32,
    },
    Full(Option<ChooseBufferState>),
    Paste {
        pane: PaneId,
        name: String,
    },
    Close,
}

#[derive(Debug)]
struct ChooseBufferSession {
    source_pane: PaneId,
    source_session: SessionId,
    names: Vec<String>,
    selected: Option<String>,
    search: Option<ChooseBufferSearchState>,
    last_search: Option<ChooseBufferSearchState>,
    rendered: ChooseBufferState,
}

impl ChooseBufferSession {
    fn new(
        source_pane: PaneId,
        state: &MuxState,
        buffers: &[PasteBuffer],
    ) -> Result<Option<Self>, ServerError> {
        let source_window = state
            .window_for_pane(source_pane)
            .ok_or_else(|| ServerError::MissingTarget(source_pane.to_string()))?;
        let source_session = state.windows[&source_window].session;
        if buffers.is_empty() {
            return Ok(None);
        }
        let mut chooser = Self {
            source_pane,
            source_session,
            names: Vec::new(),
            selected: buffers.first().map(|buffer| buffer.name.clone()),
            search: None,
            last_search: None,
            rendered: ChooseBufferState {
                items: Vec::new(),
                search: None,
                selected: 0,
            },
        };
        chooser.rebuild(buffers);
        Ok(Some(chooser))
    }

    fn rebuild(&mut self, buffers: &[PasteBuffer]) {
        self.names.clear();
        let mut items = Vec::with_capacity(buffers.len().min(MAX_CHOOSE_BUFFER_ITEMS));
        for buffer in buffers.iter().take(MAX_CHOOSE_BUFFER_ITEMS) {
            self.names.push(buffer.name.clone());
            items.push(ChooseBufferItem {
                name: bounded_choose_buffer_name(&buffer.name),
                preview: bounded_choose_buffer_preview(&buffer.data),
                size_bytes: u64::try_from(buffer.data.len()).unwrap_or(u64::MAX),
                created_unix_seconds: buffer
                    .created
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
        }
        let previous_index = usize::try_from(self.rendered.selected).unwrap_or(usize::MAX);
        let selected = self
            .selected
            .as_ref()
            .and_then(|name| self.names.iter().position(|candidate| candidate == name))
            .unwrap_or(previous_index)
            .min(items.len().saturating_sub(1));
        self.selected = self.names.get(selected).cloned();
        self.rendered = ChooseBufferState {
            items,
            search: self.search.clone(),
            selected: u32::try_from(selected).unwrap_or(u32::MAX),
        };
    }

    fn apply(
        &mut self,
        action: ChooseBufferAction,
        buffers: &[PasteBuffer],
    ) -> Result<ChooseBufferResult, ServerError> {
        let len = self.rendered.items.len();
        if len == 0 {
            return Ok(ChooseBufferResult::Close);
        }
        let current = usize::try_from(self.rendered.selected)
            .unwrap_or(usize::MAX)
            .min(len - 1);
        match action {
            ChooseBufferAction::Previous => self.select_index(current.saturating_sub(1)),
            ChooseBufferAction::Next => self.select_index((current + 1).min(len - 1)),
            ChooseBufferAction::PagePrevious => {
                self.select_index(current.saturating_sub(CHOOSE_BUFFER_PAGE_ROWS));
            }
            ChooseBufferAction::PageNext => {
                self.select_index(current.saturating_add(CHOOSE_BUFFER_PAGE_ROWS).min(len - 1));
            }
            ChooseBufferAction::First => self.select_index(0),
            ChooseBufferAction::Last => self.select_index(len - 1),
            ChooseBufferAction::Select(index) => {
                self.select_index(usize::try_from(index).unwrap_or(usize::MAX));
            }
            ChooseBufferAction::PasteIndex(index) => {
                self.select_index(usize::try_from(index).unwrap_or(usize::MAX));
                return Ok(self
                    .selected
                    .clone()
                    .map_or(ChooseBufferResult::Updated, ChooseBufferResult::Paste));
            }
            ChooseBufferAction::Paste => {
                return Ok(self
                    .selected
                    .clone()
                    .map_or(ChooseBufferResult::Updated, ChooseBufferResult::Paste));
            }
            ChooseBufferAction::Delete => {
                return Ok(self
                    .selected
                    .clone()
                    .map_or(ChooseBufferResult::Updated, ChooseBufferResult::Delete));
            }
            ChooseBufferAction::SearchStart { reverse } => {
                self.search = Some(ChooseBufferSearchState {
                    query: String::new(),
                    reverse,
                });
                self.rendered.search.clone_from(&self.search);
            }
            ChooseBufferAction::SearchAppend(text) => {
                let Some(search) = self.search.as_mut() else {
                    return Ok(ChooseBufferResult::Updated);
                };
                if search.query.len().saturating_add(text.len()) > MAX_CHOOSE_BUFFER_QUERY_BYTES {
                    return Err(ServerError::InvalidCommand(format!(
                        "choose-buffer search exceeds {MAX_CHOOSE_BUFFER_QUERY_BYTES} bytes"
                    )));
                }
                search.query.push_str(&text);
                let search = search.clone();
                self.rendered.search = Some(search.clone());
                self.select_search_match(buffers, &search.query, search.reverse, true);
            }
            ChooseBufferAction::SearchBackspace => {
                let Some(search) = self.search.as_mut() else {
                    return Ok(ChooseBufferResult::Updated);
                };
                search.query.pop();
                let search = search.clone();
                self.rendered.search = Some(search.clone());
                self.select_search_match(buffers, &search.query, search.reverse, true);
            }
            ChooseBufferAction::SearchAccept => {
                if let Some(search) = self.search.take()
                    && !search.query.is_empty()
                {
                    self.last_search = Some(search);
                }
                self.rendered.search = None;
            }
            ChooseBufferAction::SearchCancel => {
                self.search = None;
                self.rendered.search = None;
            }
            ChooseBufferAction::SearchNext { reverse } => {
                if let Some(search) = self.last_search.clone() {
                    self.select_search_match(
                        buffers,
                        &search.query,
                        search.reverse ^ reverse,
                        false,
                    );
                }
            }
            ChooseBufferAction::Close => return Ok(ChooseBufferResult::Close),
            ChooseBufferAction::Key(_) => {}
        }
        Ok(ChooseBufferResult::Updated)
    }

    fn select_index(&mut self, index: usize) {
        let index = index.min(self.names.len().saturating_sub(1));
        self.rendered.selected = u32::try_from(index).unwrap_or(u32::MAX);
        self.selected = self.names.get(index).cloned();
    }

    fn select_search_match(
        &mut self,
        buffers: &[PasteBuffer],
        query: &str,
        reverse: bool,
        include_current: bool,
    ) {
        if query.is_empty() || self.names.is_empty() {
            return;
        }
        let len = self.names.len();
        let current = usize::try_from(self.rendered.selected)
            .unwrap_or(usize::MAX)
            .min(len - 1);
        let start = usize::from(!include_current);
        for offset in start..start.saturating_add(len) {
            let index = if reverse {
                (current + len - (offset % len)) % len
            } else {
                (current + offset) % len
            };
            let name = &self.names[index];
            let Some(buffer) = buffers.iter().find(|buffer| &buffer.name == name) else {
                continue;
            };
            if contains_buffer_query(buffer.name.as_bytes(), query)
                || contains_buffer_query(&buffer.data, query)
            {
                self.select_index(index);
                return;
            }
        }
    }
}

fn contains_buffer_query(haystack: &[u8], query: &str) -> bool {
    let needle = query.as_bytes();
    if !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|candidate| candidate == needle)
    {
        return true;
    }
    !needle.is_empty()
        && needle.is_ascii()
        && haystack
            .windows(needle.len())
            .any(|candidate| candidate.eq_ignore_ascii_case(needle))
}

fn bounded_choose_buffer_name(name: &str) -> String {
    if name.len() <= MAX_CHOOSE_BUFFER_NAME_BYTES {
        return name.to_owned();
    }
    let mut end = MAX_CHOOSE_BUFFER_NAME_BYTES.saturating_sub('…'.len_utf8());
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &name[..end])
}

fn bounded_choose_buffer_preview(data: &[u8]) -> String {
    let mut preview = String::with_capacity(MAX_CHOOSE_BUFFER_PREVIEW_BYTES);
    let prefix_len = data.len().min(MAX_CHOOSE_BUFFER_PREVIEW_BYTES);
    let sample = String::from_utf8_lossy(&data[..prefix_len]);
    let mut truncated = prefix_len < data.len();
    for character in sample.chars() {
        let mut encoded = [0; 4];
        let fragment = match character {
            '\n' => " ⏎ ",
            '\r' => "",
            '\t' => " ⇥ ",
            character if character.is_control() => "�",
            character => character.encode_utf8(&mut encoded),
        };
        if preview.len().saturating_add(fragment.len())
            > MAX_CHOOSE_BUFFER_PREVIEW_BYTES.saturating_sub('…'.len_utf8())
        {
            truncated = true;
            break;
        }
        preview.push_str(fragment);
    }
    if truncated {
        preview.push('…');
    }
    preview
}

fn bounded_buffer_sample(data: &[u8], max_bytes: usize) -> String {
    let prefix_len = data.len().min(max_bytes);
    let sample = String::from_utf8_lossy(&data[..prefix_len]);
    let mut output = String::with_capacity(max_bytes);
    let mut truncated = prefix_len < data.len();
    for character in sample.chars() {
        let mut encoded = [0; 4];
        let fragment = match character {
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            character if character.is_control() => "�",
            character => character.encode_utf8(&mut encoded),
        };
        if output.len().saturating_add(fragment.len()) > max_bytes.saturating_sub(3) {
            truncated = true;
            break;
        }
        output.push_str(fragment);
    }
    if truncated {
        output.push_str("...");
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChooseTreeResult {
    Updated(ChooseTreeUpdateKind),
    Close,
    Activate(ChooseTreeTarget),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChooseTreeUpdateKind {
    Delta,
    Full,
}

#[derive(Debug)]
struct ChooseTreeSession {
    source_pane: PaneId,
    source_session: SessionId,
    kind: ChooseTreeKind,
    expanded_sessions: BTreeSet<SessionId>,
    expanded_windows: BTreeSet<zz_protocol::WindowId>,
    selected: Option<ChooseTreeTarget>,
    search: Option<ChooseTreeSearchState>,
    last_search: Option<ChooseTreeSearchState>,
    rendered: ChooseTreeState,
}

impl ChooseTreeSession {
    fn new(
        kind: ChooseTreeKind,
        source_pane: PaneId,
        state: &MuxState,
        attached_session: Option<SessionId>,
    ) -> Result<Self, ServerError> {
        let source_window = state
            .window_for_pane(source_pane)
            .ok_or_else(|| ServerError::MissingTarget(source_pane.to_string()))?;
        let source_session = state.windows[&source_window].session;
        let expanded_sessions = state.sessions.keys().copied().collect();
        let expanded_windows = if kind == ChooseTreeKind::Panes {
            state.windows.keys().copied().collect()
        } else {
            BTreeSet::new()
        };
        let selected = Some(match kind {
            ChooseTreeKind::Windows => ChooseTreeTarget::Window(source_window),
            ChooseTreeKind::Panes => ChooseTreeTarget::Pane(source_pane),
        });
        let mut chooser = Self {
            source_pane,
            source_session,
            kind,
            expanded_sessions,
            expanded_windows,
            selected,
            search: None,
            last_search: None,
            rendered: ChooseTreeState {
                items: Vec::new(),
                search: None,
                selected: 0,
                kind,
            },
        };
        chooser.rebuild(state, attached_session);
        Ok(chooser)
    }

    fn rebuild(&mut self, state: &MuxState, attached_session: Option<SessionId>) {
        let mut items = Vec::new();
        'sessions: for session in state.sessions.values() {
            let session_has_children = session
                .windows
                .iter()
                .any(|window| state.windows.contains_key(window));
            let session_expanded =
                session_has_children && self.expanded_sessions.contains(&session.id);
            if !push_choose_tree_item(
                &mut items,
                ChooseTreeItem {
                    label: bounded_choose_tree_text(&session.name),
                    detail: format_count(session.windows.len(), "window"),
                    target: ChooseTreeTarget::Session(session.id),
                    depth: 0,
                    flags: choose_tree_flags(
                        session_expanded,
                        session_has_children,
                        attached_session == Some(session.id),
                    ),
                    pane_kind: None,
                },
            ) {
                break;
            }
            if !session_expanded {
                continue;
            }

            for window_id in &session.windows {
                let Some(window) = state.windows.get(window_id) else {
                    continue;
                };
                let window_has_children =
                    self.kind == ChooseTreeKind::Panes && !window.panes.is_empty();
                let window_expanded =
                    window_has_children && self.expanded_windows.contains(window_id);
                if !push_choose_tree_item(
                    &mut items,
                    ChooseTreeItem {
                        label: bounded_choose_tree_text(&format!(
                            "{}:{}",
                            window.index, window.name
                        )),
                        detail: format_count(window.panes.len(), "pane"),
                        target: ChooseTreeTarget::Window(*window_id),
                        depth: 1,
                        flags: choose_tree_flags(
                            window_expanded,
                            window_has_children,
                            session.active_window == *window_id,
                        ),
                        pane_kind: None,
                    },
                ) {
                    break 'sessions;
                }
                if !window_expanded {
                    continue;
                }

                for pane_id in window.pane_order().iter().copied() {
                    let Some(pane) = window.panes.get(&pane_id) else {
                        continue;
                    };
                    let (pane_kind, detail) = match &pane.kind {
                        PaneKind::Picker { .. } => (None, "Choose pane type".to_owned()),
                        PaneKind::Terminal => {
                            (Some(ChooseTreePaneKind::Terminal), "Terminal".to_owned())
                        }
                        PaneKind::Browser(browser) => (
                            Some(ChooseTreePaneKind::Browser),
                            format!("Browser · {} · {}", browser.profile, browser.url()),
                        ),
                        PaneKind::Agent(_) => (Some(ChooseTreePaneKind::Agent), "Agent".to_owned()),
                        PaneKind::Editor(editor) => (
                            Some(ChooseTreePaneKind::Editor),
                            editor.path.as_ref().map_or_else(
                                || "Editor · scratch".to_owned(),
                                |path| format!("Editor · {path}"),
                            ),
                        ),
                    };
                    if !push_choose_tree_item(
                        &mut items,
                        ChooseTreeItem {
                            label: bounded_choose_tree_text(&pane.title),
                            detail: bounded_choose_tree_text(&detail),
                            target: ChooseTreeTarget::Pane(pane_id),
                            depth: 2,
                            flags: choose_tree_flags(false, false, window.active_pane == pane_id),
                            pane_kind,
                        },
                    ) {
                        break 'sessions;
                    }
                }
            }
        }

        let fallback = state
            .window_for_pane(self.source_pane)
            .map(|window| match self.kind {
                ChooseTreeKind::Windows => ChooseTreeTarget::Window(window),
                ChooseTreeKind::Panes => ChooseTreeTarget::Pane(self.source_pane),
            });
        let selected = self
            .selected
            .and_then(|target| items.iter().position(|item| item.target == target))
            .or_else(|| {
                fallback.and_then(|target| items.iter().position(|item| item.target == target))
            })
            .unwrap_or(0)
            .min(items.len().saturating_sub(1));
        self.selected = items.get(selected).map(|item| item.target);
        self.rendered = ChooseTreeState {
            items,
            search: self.search.clone(),
            selected: u32::try_from(selected).unwrap_or(u32::MAX),
            kind: self.kind,
        };
    }

    fn apply(
        &mut self,
        action: ChooseTreeAction,
        state: &MuxState,
        attached_session: Option<SessionId>,
    ) -> Result<ChooseTreeResult, ServerError> {
        let len = self.rendered.items.len();
        let current = usize::try_from(self.rendered.selected)
            .unwrap_or(usize::MAX)
            .min(len.saturating_sub(1));
        let mut update = ChooseTreeUpdateKind::Delta;
        match action {
            ChooseTreeAction::Previous => self.select_index(current.saturating_sub(1)),
            ChooseTreeAction::Next => self.select_index((current + 1).min(len.saturating_sub(1))),
            ChooseTreeAction::PagePrevious => {
                self.select_index(current.saturating_sub(CHOOSE_TREE_PAGE_ROWS));
            }
            ChooseTreeAction::PageNext => self.select_index(
                current
                    .saturating_add(CHOOSE_TREE_PAGE_ROWS)
                    .min(len.saturating_sub(1)),
            ),
            ChooseTreeAction::First => self.select_index(0),
            ChooseTreeAction::Last => self.select_index(len.saturating_sub(1)),
            ChooseTreeAction::Select(index) => {
                self.select_index(usize::try_from(index).unwrap_or(usize::MAX));
            }
            ChooseTreeAction::ActivateIndex(index) => {
                self.select_index(usize::try_from(index).unwrap_or(usize::MAX));
                return Ok(self.selected.map_or(
                    ChooseTreeResult::Updated(ChooseTreeUpdateKind::Delta),
                    ChooseTreeResult::Activate,
                ));
            }
            ChooseTreeAction::Collapse => {
                if let Some(item) = self.rendered.items.get(current).cloned() {
                    if item.expanded() {
                        self.set_expanded(item.target, false);
                        self.rebuild(state, attached_session);
                        update = ChooseTreeUpdateKind::Full;
                    } else if item.depth > 0
                        && let Some(parent) = self.rendered.items[..current]
                            .iter()
                            .rev()
                            .position(|candidate| candidate.depth < item.depth)
                    {
                        self.select_index(current - parent - 1);
                    }
                }
            }
            ChooseTreeAction::Expand => {
                if let Some(item) = self.rendered.items.get(current).cloned() {
                    if item.has_children() && !item.expanded() {
                        self.set_expanded(item.target, true);
                        self.rebuild(state, attached_session);
                        update = ChooseTreeUpdateKind::Full;
                    } else {
                        self.select_index((current + 1).min(len.saturating_sub(1)));
                    }
                }
            }
            ChooseTreeAction::Activate => {
                return Ok(self.selected.map_or(
                    ChooseTreeResult::Updated(ChooseTreeUpdateKind::Delta),
                    ChooseTreeResult::Activate,
                ));
            }
            ChooseTreeAction::SearchStart { reverse } => {
                self.search = Some(ChooseTreeSearchState {
                    query: String::new(),
                    reverse,
                });
                self.rendered.search.clone_from(&self.search);
            }
            ChooseTreeAction::SearchAppend(text) => {
                let Some(search) = self.search.as_mut() else {
                    return Ok(ChooseTreeResult::Updated(ChooseTreeUpdateKind::Delta));
                };
                if search.query.len().saturating_add(text.len()) > MAX_CHOOSE_TREE_QUERY_BYTES {
                    return Err(ServerError::InvalidCommand(format!(
                        "choose-tree search exceeds {MAX_CHOOSE_TREE_QUERY_BYTES} bytes"
                    )));
                }
                search.query.push_str(&text);
                let search = search.clone();
                self.rendered.search = Some(search.clone());
                self.select_search_match(&search.query, search.reverse, true);
            }
            ChooseTreeAction::SearchBackspace => {
                let Some(search) = self.search.as_mut() else {
                    return Ok(ChooseTreeResult::Updated(ChooseTreeUpdateKind::Delta));
                };
                search.query.pop();
                let search = search.clone();
                self.rendered.search = Some(search.clone());
                self.select_search_match(&search.query, search.reverse, true);
            }
            ChooseTreeAction::SearchAccept => {
                if let Some(search) = self.search.take()
                    && !search.query.is_empty()
                {
                    self.last_search = Some(search);
                }
                self.rendered.search = None;
            }
            ChooseTreeAction::SearchCancel => {
                self.search = None;
                self.rendered.search = None;
            }
            ChooseTreeAction::SearchNext { reverse } => {
                if let Some(search) = self.last_search.clone() {
                    self.select_search_match(&search.query, search.reverse ^ reverse, false);
                }
            }
            ChooseTreeAction::Close => return Ok(ChooseTreeResult::Close),
            ChooseTreeAction::Key(_) => {}
        }
        Ok(ChooseTreeResult::Updated(update))
    }

    fn select_index(&mut self, index: usize) {
        let index = index.min(self.rendered.items.len().saturating_sub(1));
        self.rendered.selected = u32::try_from(index).unwrap_or(u32::MAX);
        self.selected = self.rendered.items.get(index).map(|item| item.target);
    }

    fn set_expanded(&mut self, target: ChooseTreeTarget, expanded: bool) {
        match target {
            ChooseTreeTarget::Session(session) => {
                if expanded {
                    self.expanded_sessions.insert(session);
                } else {
                    self.expanded_sessions.remove(&session);
                }
            }
            ChooseTreeTarget::Window(window) => {
                if expanded {
                    self.expanded_windows.insert(window);
                } else {
                    self.expanded_windows.remove(&window);
                }
            }
            ChooseTreeTarget::Pane(_) => {}
        }
    }

    fn select_search_match(&mut self, query: &str, reverse: bool, include_current: bool) {
        if query.is_empty() || self.rendered.items.is_empty() {
            return;
        }
        let query = query.to_lowercase();
        let len = self.rendered.items.len();
        let current = usize::try_from(self.rendered.selected)
            .unwrap_or(usize::MAX)
            .min(len - 1);
        let start = usize::from(!include_current);
        for offset in start..start.saturating_add(len) {
            let index = if reverse {
                (current + len - (offset % len)) % len
            } else {
                (current + offset) % len
            };
            let item = &self.rendered.items[index];
            if item.label.to_lowercase().contains(&query)
                || item.detail.to_lowercase().contains(&query)
                || item.target.to_string().to_lowercase().contains(&query)
            {
                self.select_index(index);
                return;
            }
        }
    }
}

fn push_choose_tree_item(items: &mut Vec<ChooseTreeItem>, item: ChooseTreeItem) -> bool {
    if items.len() >= MAX_CHOOSE_TREE_ITEMS {
        return false;
    }
    items.push(item);
    true
}

fn bounded_choose_tree_text(text: &str) -> String {
    if text.len() <= MAX_CHOOSE_TREE_ITEM_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_CHOOSE_TREE_ITEM_BYTES.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn format_count(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

const fn choose_tree_flags(expanded: bool, has_children: bool, active: bool) -> u8 {
    let mut flags = 0;
    if expanded {
        flags |= ChooseTreeItem::EXPANDED;
    }
    if has_children {
        flags |= ChooseTreeItem::HAS_CHILDREN;
    }
    if active {
        flags |= ChooseTreeItem::ACTIVE;
    }
    flags
}

#[derive(Debug)]
struct CommandOutputSession {
    pane: PaneId,
    terminal: Arc<TerminalSession>,
    previous_key_table: Option<String>,
}

fn current_command_output_subscriber(
    inner: &ServerState,
    client: ClientId,
    pane: PaneId,
    terminal: &Arc<TerminalSession>,
) -> Option<Arc<OutboundMailbox>> {
    let output = inner.command_outputs.get(&client)?;
    if output.pane != pane || !Arc::ptr_eq(&output.terminal, terminal) {
        return None;
    }
    inner.subscribers.get(&client).cloned()
}

type RetiredCommandOutput = (CommandOutputSession, Option<Arc<OutboundMailbox>>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Overlay {
    CommandPrompt,
    ChooseTree,
    ChooseBuffer,
    DisplayPanes,
}

fn dismiss_overlays(
    inner: &mut ServerState,
    client: ClientId,
    raising: Option<Overlay>,
    events: &mut Vec<EventPayload>,
    retired: &mut Vec<(ClientId, RetiredCommandOutput)>,
) {
    if raising != Some(Overlay::CommandPrompt) && inner.command_prompts.remove(&client).is_some() {
        events.push(EventPayload::CommandPrompt { state: None });
    }
    if raising != Some(Overlay::ChooseTree) && inner.choose_trees.remove(&client).is_some() {
        events.push(EventPayload::ChooseTree { state: None });
    }
    if raising != Some(Overlay::ChooseBuffer) && inner.choose_buffers.remove(&client).is_some() {
        events.push(EventPayload::ChooseBuffer { state: None });
    }
    if raising != Some(Overlay::DisplayPanes) && take_display_panes(inner, client).is_some() {
        events.push(EventPayload::DisplayPanes { state: None });
    }
    if let Some(output) = take_command_output(inner, client) {
        retired.push((client, output));
    }
}

fn take_command_output(inner: &mut ServerState, client: ClientId) -> Option<RetiredCommandOutput> {
    let output = inner.command_outputs.remove(&client)?;
    inner
        .key_engines
        .entry(client)
        .or_default()
        .switch_table(output.previous_key_table.clone());
    let subscriber = inner.subscribers.get(&client).cloned();
    Some((output, subscriber))
}

#[derive(Clone, Debug)]
struct PasteBuffer {
    name: String,
    data: Arc<[u8]>,
    created: SystemTime,
    automatic: bool,
    utf8: bool,
}

#[derive(Clone, Debug)]
struct ServerMessage {
    number: u64,
    time: SystemTime,
    text: String,
}

#[derive(Clone, Copy, Debug)]
struct PasteBufferPaste<'a> {
    requested_name: Option<&'a str>,
    separator: &'a [u8],
    expected_client: Option<ClientId>,
    delete: bool,
    bracketed: bool,
    literal: bool,
}

#[derive(Clone, Debug)]
struct CommandPrompt {
    prompt: String,
    input: String,
    cursor: usize,
    template: Option<String>,
    history_index: Option<usize>,
    history_draft: String,
}

impl CommandPrompt {
    fn new(prompt: String, input: String, template: Option<String>) -> Self {
        let cursor = input.len();
        Self {
            prompt,
            history_draft: input.clone(),
            input,
            cursor,
            template,
            history_index: None,
        }
    }

    fn kind(&self) -> CommandPromptKind {
        if self.template.is_some() {
            CommandPromptKind::Value
        } else {
            CommandPromptKind::Command
        }
    }

    fn state(&self, history: &[String]) -> CommandPromptState {
        CommandPromptState {
            prompt: self.prompt.clone(),
            input: self.input.clone(),
            cursor: u32::try_from(self.input[..self.cursor].chars().count()).unwrap_or(u32::MAX),
            kind: self.kind(),
            history: if self.kind() == CommandPromptKind::Command {
                command_prompt_history_snapshot(history)
            } else {
                Vec::new()
            },
        }
    }

    fn replace_input(&mut self, input: String, cursor: u32) -> Result<(), ServerError> {
        if input.len() > zz_protocol::MAX_COMMAND_PROMPT_BYTES {
            return Err(ServerError::InvalidCommand(format!(
                "command prompt input exceeds {} bytes",
                zz_protocol::MAX_COMMAND_PROMPT_BYTES
            )));
        }
        let cursor = usize::try_from(cursor).unwrap_or(usize::MAX);
        let cursor = char_index_to_byte(&input, cursor).ok_or_else(|| {
            ServerError::InvalidCommand("command prompt cursor is out of bounds".to_owned())
        })?;
        self.input = input;
        self.cursor = cursor;
        self.finish_edit();
        Ok(())
    }

    fn insert(&mut self, text: &str) -> bool {
        if text.is_empty()
            || self.input.len().saturating_add(text.len()) > zz_protocol::MAX_COMMAND_PROMPT_BYTES
        {
            return false;
        }
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.finish_edit();
        true
    }

    fn move_left(&mut self) -> bool {
        let next = previous_char_boundary(&self.input, self.cursor);
        let changed = next != self.cursor;
        self.cursor = next;
        changed
    }

    fn move_right(&mut self) -> bool {
        let next = next_char_boundary(&self.input, self.cursor);
        let changed = next != self.cursor;
        self.cursor = next;
        changed
    }

    fn move_word_left(&mut self) -> bool {
        let original = self.cursor;
        while self.cursor > 0 {
            let previous = previous_char_boundary(&self.input, self.cursor);
            if !self.input[previous..self.cursor]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                break;
            }
            self.cursor = previous;
        }
        while self.cursor > 0 {
            let previous = previous_char_boundary(&self.input, self.cursor);
            if self.input[previous..self.cursor]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                break;
            }
            self.cursor = previous;
        }
        self.cursor != original
    }

    fn move_word_right(&mut self) -> bool {
        let original = self.cursor;
        while self.cursor < self.input.len() {
            let next = next_char_boundary(&self.input, self.cursor);
            if !self.input[self.cursor..next]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                break;
            }
            self.cursor = next;
        }
        while self.cursor < self.input.len() {
            let next = next_char_boundary(&self.input, self.cursor);
            if self.input[self.cursor..next]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                break;
            }
            self.cursor = next;
        }
        self.cursor != original
    }

    fn delete_backward(&mut self) -> bool {
        let previous = previous_char_boundary(&self.input, self.cursor);
        if previous == self.cursor {
            return false;
        }
        self.input.replace_range(previous..self.cursor, "");
        self.cursor = previous;
        self.finish_edit();
        true
    }

    fn delete_forward(&mut self) -> bool {
        let next = next_char_boundary(&self.input, self.cursor);
        if next == self.cursor {
            return false;
        }
        self.input.replace_range(self.cursor..next, "");
        self.finish_edit();
        true
    }

    fn delete_previous_word(&mut self) -> bool {
        let end = self.cursor;
        self.move_word_left();
        if self.cursor == end {
            return false;
        }
        self.input.replace_range(self.cursor..end, "");
        self.finish_edit();
        true
    }

    fn clear(&mut self) -> bool {
        if self.input.is_empty() {
            return false;
        }
        self.input.clear();
        self.cursor = 0;
        self.finish_edit();
        true
    }

    fn delete_to_end(&mut self) -> bool {
        if self.cursor == self.input.len() {
            return false;
        }
        self.input.truncate(self.cursor);
        self.finish_edit();
        true
    }

    fn history_up(&mut self, history: &[String]) -> bool {
        if history.is_empty() {
            return false;
        }
        let index = self.history_index.map_or_else(
            || {
                self.history_draft.clone_from(&self.input);
                history.len() - 1
            },
            |index| index.saturating_sub(1),
        );
        if self.history_index == Some(index) {
            return false;
        }
        self.history_index = Some(index);
        self.input.clone_from(&history[index]);
        self.cursor = self.input.len();
        true
    }

    fn history_down(&mut self, history: &[String]) -> bool {
        let Some(index) = self.history_index else {
            return false;
        };
        if index + 1 < history.len() {
            self.history_index = Some(index + 1);
            self.input.clone_from(&history[index + 1]);
        } else {
            self.history_index = None;
            self.input.clone_from(&self.history_draft);
        }
        self.cursor = self.input.len();
        true
    }

    fn finish_edit(&mut self) {
        self.history_index = None;
        self.history_draft.clone_from(&self.input);
    }
}

fn command_prompt_history_snapshot(history: &[String]) -> Vec<String> {
    let mut bytes: usize = 0;
    let mut snapshot = history
        .iter()
        .rev()
        .take(MAX_COMMAND_PROMPT_HISTORY_SNAPSHOT_ITEMS)
        .take_while(|entry| {
            let next = bytes.saturating_add(entry.len());
            if next > MAX_COMMAND_PROMPT_HISTORY_SNAPSHOT_BYTES {
                false
            } else {
                bytes = next;
                true
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    snapshot.reverse();
    snapshot
}

fn command_prompt_state(inner: &ServerState, client: ClientId) -> Option<CommandPromptState> {
    inner
        .command_prompts
        .get(&client)
        .map(|prompt| prompt.state(&inner.command_history))
}

fn char_index_to_byte(input: &str, index: usize) -> Option<usize> {
    input
        .char_indices()
        .nth(index)
        .map(|(offset, _)| offset)
        .or_else(|| (index == input.chars().count()).then_some(input.len()))
}

fn previous_char_boundary(text: &str, index: usize) -> usize {
    text[..index]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    text[index..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(offset, _)| index + offset)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromptKeyAction {
    Handled,
    Updated,
    Close,
    Submit,
    LimitExceeded,
}

struct CommandPromptSubmission {
    input: String,
    template: Option<String>,
}

fn append_command_prompt_output(output: &mut String, piece: &str) -> bool {
    let content_limit =
        MAX_COMMAND_PROMPT_OUTPUT_BYTES.saturating_sub(COMMAND_PROMPT_OUTPUT_TRUNCATED.len() + 1);
    let separator_bytes = usize::from(!output.is_empty());
    let available = content_limit.saturating_sub(output.len());
    if separator_bytes.saturating_add(piece.len()) <= available {
        if separator_bytes != 0 {
            output.push('\n');
        }
        output.push_str(piece);
        return false;
    }

    let piece_available = available.saturating_sub(separator_bytes);
    let mut prefix_bytes = piece.len().min(piece_available);
    while !piece.is_char_boundary(prefix_bytes) {
        prefix_bytes -= 1;
    }
    if prefix_bytes != 0 {
        if separator_bytes != 0 {
            output.push('\n');
        }
        output.push_str(&piece[..prefix_bytes]);
    }
    true
}

fn bounded_command_output(text: &str) -> String {
    let mut output = String::new();
    if append_command_prompt_output(&mut output, text) {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(COMMAND_PROMPT_OUTPUT_TRUNCATED);
    }
    output
}

fn client_context_pane(inner: &ServerState, client: ClientId) -> Option<PaneId> {
    client_attached_session(inner, client)
        .and_then(|session| inner.engine.state.sessions.get(&session))
        .and_then(|session| {
            inner
                .engine
                .state
                .windows
                .get(&client_focused_window(inner, client, session))
        })
        .map(|window| window.active_pane)
        .or_else(|| {
            inner
                .engine
                .state
                .default_context()
                .map(|(_, _, pane)| pane)
        })
}

fn enter_copy_session(
    inner: &mut ServerState,
    client: ClientId,
    pane: PaneId,
) -> Result<(), ServerError> {
    let table = inner.engine.copy_mode_table_for_pane(pane)?.to_owned();
    inner
        .key_engines
        .entry(client)
        .or_default()
        .switch_table(Some(table));
    inner.copy_sessions.insert(
        client,
        CopySession {
            pane,
            observed: false,
            scroll_exit: false,
        },
    );
    Ok(())
}

fn terminal_view_action_arms_scroll_exit(action: &zz_terminal::TerminalViewAction) -> bool {
    matches!(
        action,
        zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::PageDownScrollExit)
    )
}

fn exit_copy_session(inner: &mut ServerState, client: ClientId) {
    inner.copy_sessions.remove(&client);
    inner
        .key_engines
        .entry(client)
        .or_default()
        .switch_table(None);
}

fn sync_copy_session_for_view_action(
    inner: &mut ServerState,
    client: ClientId,
    pane: PaneId,
    action: &zz_terminal::TerminalViewAction,
) -> Result<(), ServerError> {
    if terminal_view_action_enters_copy_mode(action) {
        enter_copy_session(inner, client, pane)?;
    } else if terminal_view_action_exits_copy_mode(action) {
        exit_copy_session(inner, client);
    } else if terminal_view_action_arms_scroll_exit(action)
        && let Some(session) = inner.copy_sessions.get_mut(&client)
        && session.pane == pane
    {
        session.scroll_exit = true;
    }
    Ok(())
}

fn terminal_view_action_enters_copy_mode(action: &zz_terminal::TerminalViewAction) -> bool {
    matches!(
        action,
        zz_terminal::TerminalViewAction::EnterCopyMode
            | zz_terminal::TerminalViewAction::EnterCopyModeScrollExit
    )
}

fn terminal_view_action_exits_copy_mode(action: &zz_terminal::TerminalViewAction) -> bool {
    match action {
        zz_terminal::TerminalViewAction::ClearHistory
        | zz_terminal::TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel) => true,
        zz_terminal::TerminalViewAction::CopyMode(
            zz_terminal::CopyModeAction::CopySelection(copy)
            | zz_terminal::CopyModeAction::CopyEndOfLine(copy),
        ) => copy.cancel,
        _ => false,
    }
}

fn retarget_copy_mode_tables(inner: &mut ServerState, changed_window: Option<WindowId>) {
    let retargeted = inner
        .copy_sessions
        .iter()
        .filter_map(|(client, session)| {
            let window = inner.engine.state.window_for_pane(session.pane)?;
            changed_window
                .is_none_or(|changed| changed == window)
                .then_some((*client, session.pane))
        })
        .collect::<Vec<_>>();
    for (client, pane) in retargeted {
        let Ok(table) = inner.engine.copy_mode_table_for_pane(pane) else {
            continue;
        };
        let table = table.to_owned();
        inner
            .key_engines
            .entry(client)
            .or_default()
            .switch_table(Some(table));
    }
}

fn unfocused_copy_sessions(inner: &mut ServerState) -> Vec<(ClientId, Arc<TerminalSession>)> {
    let stale = inner
        .copy_sessions
        .iter()
        .filter(|(client, session)| client_context_pane(inner, **client) != Some(session.pane))
        .map(|(client, session)| (*client, session.pane))
        .collect::<Vec<_>>();
    stale
        .into_iter()
        .filter_map(|(client, pane)| {
            exit_copy_session(inner, client);
            Some((client, inner.terminals.get(&pane).cloned()?))
        })
        .collect()
}

fn reconcile_copy_session(
    inner: &mut ServerState,
    pane: PaneId,
    client: ClientId,
    mode: TerminalMode,
) -> Option<Arc<TerminalSession>> {
    match (mode, inner.copy_sessions.get_mut(&client)) {
        (TerminalMode::Copy { .. }, Some(session)) if session.pane == pane => {
            session.observed = true;
            None
        }
        (TerminalMode::Copy { .. }, _) => inner.terminals.get(&pane).cloned(),
        (TerminalMode::Live, Some(session))
            if session.pane == pane && (session.observed || session.scroll_exit) =>
        {
            exit_copy_session(inner, client);
            None
        }
        _ => None,
    }
}

fn client_attached_session(inner: &ServerState, client: ClientId) -> Option<SessionId> {
    inner
        .attached
        .iter()
        .find_map(|(session, clients)| clients.contains(&client).then_some(*session))
}

fn client_focused_window(
    inner: &ServerState,
    client: ClientId,
    session: &zz_mux::Session,
) -> WindowId {
    inner
        .focused_windows
        .get(&client)
        .copied()
        .filter(|focused| session.windows.contains(focused))
        .unwrap_or(session.active_window)
}

fn client_focused_window_for_attachment(inner: &ServerState, client: ClientId) -> Option<WindowId> {
    client_attached_session(inner, client)
        .and_then(|session| inner.engine.state.sessions.get(&session))
        .map(|session| client_focused_window(inner, client, session))
}

type SnapshotPresence = BTreeMap<SessionId, Vec<(ClientId, SessionViewer)>>;

fn snapshot_presence(inner: &ServerState) -> SnapshotPresence {
    inner
        .attached
        .iter()
        .filter_map(|(session, clients)| {
            let session_state = inner.engine.state.sessions.get(session)?;
            let viewers = clients
                .iter()
                .map(|client| {
                    let name = inner
                        .client_names
                        .get(client)
                        .cloned()
                        .unwrap_or_else(|| format!("device-{}", client.0));
                    (
                        *client,
                        SessionViewer {
                            name,
                            window: client_focused_window(inner, *client, session_state),
                            is_self: false,
                        },
                    )
                })
                .collect();
            Some((*session, viewers))
        })
        .collect()
}

fn stamp_snapshot_for_client(
    inner: &ServerState,
    client: ClientId,
    snapshot: &mut MuxSnapshot,
    presence: &SnapshotPresence,
) {
    snapshot.focused_window = client_focused_window_for_attachment(inner, client);
    for session in &mut snapshot.sessions {
        session.viewers = presence
            .get(&session.id)
            .into_iter()
            .flatten()
            .map(|(viewer_client, viewer)| SessionViewer {
                is_self: *viewer_client == client,
                ..viewer.clone()
            })
            .collect();
    }
}

fn command_prompt_key(
    prompt: &mut CommandPrompt,
    input: &zz_terminal::KeyInput,
    text_follows: bool,
    history: &[String],
) -> PromptKeyAction {
    if input.action == zz_terminal::KeyAction::Release {
        return PromptKeyAction::Handled;
    }
    let changed = |changed| {
        if changed {
            PromptKeyAction::Updated
        } else {
            PromptKeyAction::Handled
        }
    };
    let control = input.modifiers.control();
    let alt = input.modifiers.alt();
    let platform = input.modifiers.platform();

    match input.key {
        zz_terminal::KeyCode::Enter => PromptKeyAction::Submit,
        zz_terminal::KeyCode::Escape => PromptKeyAction::Close,
        zz_terminal::KeyCode::Backspace => changed(prompt.delete_backward()),
        zz_terminal::KeyCode::Delete => changed(prompt.delete_forward()),
        zz_terminal::KeyCode::Home => changed({
            let changed = prompt.cursor != 0;
            prompt.cursor = 0;
            changed
        }),
        zz_terminal::KeyCode::End => changed({
            let changed = prompt.cursor != prompt.input.len();
            prompt.cursor = prompt.input.len();
            changed
        }),
        zz_terminal::KeyCode::ArrowLeft if control || alt => changed(prompt.move_word_left()),
        zz_terminal::KeyCode::ArrowRight if control || alt => changed(prompt.move_word_right()),
        zz_terminal::KeyCode::ArrowLeft if platform => changed({
            let changed = prompt.cursor != 0;
            prompt.cursor = 0;
            changed
        }),
        zz_terminal::KeyCode::ArrowRight if platform => changed({
            let changed = prompt.cursor != prompt.input.len();
            prompt.cursor = prompt.input.len();
            changed
        }),
        zz_terminal::KeyCode::ArrowLeft => changed(prompt.move_left()),
        zz_terminal::KeyCode::ArrowRight => changed(prompt.move_right()),
        zz_terminal::KeyCode::ArrowUp => changed(prompt.history_up(history)),
        zz_terminal::KeyCode::ArrowDown => changed(prompt.history_down(history)),
        zz_terminal::KeyCode::Character(character) if control => {
            match character.to_ascii_lowercase() {
                'a' => changed({
                    let changed = prompt.cursor != 0;
                    prompt.cursor = 0;
                    changed
                }),
                'e' => changed({
                    let changed = prompt.cursor != prompt.input.len();
                    prompt.cursor = prompt.input.len();
                    changed
                }),
                'b' => changed(prompt.move_left()),
                'f' => changed(prompt.move_right()),
                'p' => changed(prompt.history_up(history)),
                'n' => changed(prompt.history_down(history)),
                'h' => changed(prompt.delete_backward()),
                'd' => changed(prompt.delete_forward()),
                'u' => changed(prompt.clear()),
                'k' => changed(prompt.delete_to_end()),
                'w' => changed(prompt.delete_previous_word()),
                'c' | 'g' | '[' => PromptKeyAction::Close,
                _ => PromptKeyAction::Handled,
            }
        }
        zz_terminal::KeyCode::Character(character) if alt => match character.to_ascii_lowercase() {
            'b' => changed(prompt.move_word_left()),
            'f' => changed(prompt.move_word_right()),
            _ => PromptKeyAction::Handled,
        },
        zz_terminal::KeyCode::Character(_) if platform => PromptKeyAction::Handled,
        zz_terminal::KeyCode::Character(character) => {
            if text_follows {
                PromptKeyAction::Handled
            } else {
                let text = input
                    .text
                    .as_deref()
                    .map_or_else(|| character.to_string(), str::to_owned);
                if prompt.insert(&text) {
                    PromptKeyAction::Updated
                } else {
                    PromptKeyAction::LimitExceeded
                }
            }
        }
        zz_terminal::KeyCode::Tab
        | zz_terminal::KeyCode::Insert
        | zz_terminal::KeyCode::PageUp
        | zz_terminal::KeyCode::PageDown
        | zz_terminal::KeyCode::Function(_)
        | zz_terminal::KeyCode::Unidentified => PromptKeyAction::Handled,
    }
}

struct CopyPipePermit {
    shared: Arc<Shared>,
}

impl Drop for CopyPipePermit {
    fn drop(&mut self) {
        let mut inner = self.shared.inner.lock();
        inner.active_copy_pipes = inner.active_copy_pipes.saturating_sub(1);
    }
}

fn run_copy_pipe(command: &str, data: &str) -> Result<(), String> {
    let mut input = tempfile::tempfile()
        .map_err(|error| format!("could not stage input in a temporary file: {error}"))?;
    input
        .write_all(data.as_bytes())
        .map_err(|error| format!("could not stage input: {error}"))?;
    input
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("could not rewind staged input: {error}"))?;

    let mut process = shell_process(command);
    process
        .stdin(Stdio::from(input))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = process
        .spawn()
        .map_err(|error| format!("could not start process: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("process exited unsuccessfully ({status})")),
            Ok(None) if started.elapsed() >= COPY_PIPE_TIMEOUT => {
                let kill_error = child.kill().err();
                let wait_error = child.wait().err();
                if let Some(error) = kill_error {
                    return Err(format!(
                        "process timed out after {} seconds and could not be terminated: {error}",
                        COPY_PIPE_TIMEOUT.as_secs()
                    ));
                }
                if let Some(error) = wait_error {
                    return Err(format!(
                        "process timed out after {} seconds and could not be reaped: {error}",
                        COPY_PIPE_TIMEOUT.as_secs()
                    ));
                }
                return Err(format!(
                    "process timed out after {} seconds",
                    COPY_PIPE_TIMEOUT.as_secs()
                ));
            }
            Ok(None) => thread::sleep(COPY_PIPE_POLL_INTERVAL),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("could not query process status: {error}"));
            }
        }
    }
}

enum PaneSink {
    Terminal(Arc<TerminalSession>),
    Browser(PaneId),
}

enum DeferredTerminalCommand {
    SetWordSeparators {
        terminal: Arc<TerminalSession>,
        separators: WordSeparators,
    },
    AttachView {
        terminal: Arc<TerminalSession>,
        view: TerminalViewId,
    },
    Resize {
        terminal: Arc<TerminalSession>,
        geometry: TerminalGeometry,
    },
    ViewAction {
        terminal: Arc<TerminalSession>,
        view: TerminalViewId,
        action: zz_terminal::TerminalViewAction,
    },
    SendTokens {
        terminals: Vec<Arc<TerminalSession>>,
        keys: Vec<zz_protocol::KeyToken>,
    },
}

impl DeferredTerminalCommand {
    fn run(self) {
        match self {
            Self::SetWordSeparators {
                terminal,
                separators,
            } => terminal.set_word_separators(separators),
            Self::AttachView { terminal, view } => terminal.attach_view(view),
            Self::Resize { terminal, geometry } => terminal.resize(
                geometry.columns,
                geometry.rows,
                geometry.cell_width_px,
                geometry.cell_height_px,
            ),
            Self::ViewAction {
                terminal,
                view,
                action,
            } => terminal.view_action(view, action),
            Self::SendTokens { terminals, keys } => send_tokens(&terminals, &keys),
        }
    }
}

fn resolve_input_sinks(inner: &ServerState, source: PaneId) -> Result<Vec<PaneSink>, ServerError> {
    let targets = inner.engine.synchronized_input_targets(source)?;
    let mut sinks = Vec::with_capacity(targets.len());
    for pane in targets {
        if let Some(terminal) = inner.terminals.get(&pane) {
            sinks.push(PaneSink::Terminal(Arc::clone(terminal)));
        } else if matches!(
            inner.engine.state.pane(pane).map(|pane| &pane.kind),
            Some(
                zz_mux::PaneKind::Picker { .. }
                    | zz_mux::PaneKind::Agent(_)
                    | zz_mux::PaneKind::Editor(_)
            )
        ) {
        } else if matches!(
            inner.engine.state.pane(pane).map(|pane| &pane.kind),
            Some(zz_mux::PaneKind::Browser(_))
        ) {
            ensure_browser_attached(inner, pane)?;
            sinks.push(PaneSink::Browser(pane));
        } else {
            return Err(ServerError::PaneExited(pane));
        }
    }
    Ok(sinks)
}

fn session_terminals(inner: &ServerState, session: SessionId) -> Vec<Arc<TerminalSession>> {
    inner
        .engine
        .state
        .sessions
        .get(&session)
        .into_iter()
        .flat_map(|session| session.windows.iter())
        .filter_map(|window| inner.engine.state.windows.get(window))
        .flat_map(|window| window.panes.keys())
        .filter_map(|pane| inner.terminals.get(pane).cloned())
        .collect()
}

fn terminal_viewport_for_pane(
    inner: &ServerState,
    pane: PaneId,
    view: TerminalViewId,
) -> Option<(Arc<TerminalSession>, Arc<TerminalViewport>)> {
    let terminal = inner.terminals.get(&pane)?;
    let viewport = terminal.latest_viewport_for(view).or_else(|| {
        inner
            .engine
            .state
            .pane(pane)
            .is_some_and(|pane| pane.dead)
            .then(|| terminal.latest_viewport())
    })?;
    Some((Arc::clone(terminal), viewport))
}

fn visible_terminal_panes(
    inner: &ServerState,
    client: ClientId,
    session: SessionId,
) -> BTreeSet<PaneId> {
    let Some(session) = inner.engine.state.sessions.get(&session) else {
        return BTreeSet::new();
    };
    let focused_window = client_focused_window(inner, client, session);
    let Some(window) = inner.engine.state.windows.get(&focused_window) else {
        return BTreeSet::new();
    };
    window
        .panes
        .keys()
        .filter(|pane| {
            window.zoomed_pane.is_none_or(|zoomed| **pane == zoomed)
                && inner.terminals.contains_key(pane)
        })
        .copied()
        .collect()
}

/// The agent panes a client can actually see, derived exactly like the
/// terminal ones: attached session, focused window, zoom.
#[cfg(feature = "agent")]
fn visible_agent_panes(
    inner: &ServerState,
    client: ClientId,
    session: SessionId,
) -> BTreeSet<PaneId> {
    let Some(session) = inner.engine.state.sessions.get(&session) else {
        return BTreeSet::new();
    };
    let focused_window = client_focused_window(inner, client, session);
    let Some(window) = inner.engine.state.windows.get(&focused_window) else {
        return BTreeSet::new();
    };
    window
        .panes
        .keys()
        .filter(|pane| {
            window.zoomed_pane.is_none_or(|zoomed| **pane == zoomed)
                && inner
                    .engine
                    .state
                    .pane(**pane)
                    .is_some_and(|pane| matches!(pane.kind, PaneKind::Agent(_)))
        })
        .copied()
        .collect()
}

fn attached_clients_for_pane(inner: &ServerState, pane: PaneId) -> Option<&BTreeSet<ClientId>> {
    let window = inner.engine.state.window_for_pane(pane)?;
    let session = inner.engine.state.windows.get(&window)?.session;
    inner.attached.get(&session)
}

fn client_is_attached_to_pane(inner: &ServerState, client: ClientId, pane: PaneId) -> bool {
    attached_clients_for_pane(inner, pane).is_some_and(|clients| clients.contains(&client))
}

fn remove_client_terminal_geometries(
    inner: &mut ServerState,
    client: ClientId,
) -> BTreeSet<PaneId> {
    let mut affected = BTreeSet::new();
    inner.terminal_geometries.retain(|pane, geometries| {
        if geometries.remove(&client).is_some() {
            affected.insert(*pane);
        }
        !geometries.is_empty()
    });
    affected
}

fn terminal_geometry_owner(inner: &ServerState, pane: PaneId) -> Option<ClientId> {
    inner
        .terminal_geometries
        .get(&pane)?
        .iter()
        .filter(|(client, _)| {
            inner
                .visible_terminals
                .get(client)
                .is_some_and(|visible| visible.contains(&pane))
        })
        .min_by_key(|(client, _)| {
            (
                Reverse(
                    inner
                        .client_terminal_input_sequences
                        .get(client)
                        .copied()
                        .unwrap_or_default(),
                ),
                client.0,
            )
        })
        .map(|(client, _)| *client)
}

fn terminal_resize_for_pane(
    inner: &ServerState,
    pane: PaneId,
) -> Option<(Arc<TerminalSession>, TerminalGeometry)> {
    let geometries = inner.terminal_geometries.get(&pane)?;
    let owner = terminal_geometry_owner(inner, pane)?;
    let geometry = *geometries.get(&owner)?;
    let terminal = inner.terminals.get(&pane).cloned()?;
    Some((terminal, geometry))
}

fn terminal_resizes_after_client_input(
    inner: &mut ServerState,
    client: ClientId,
    source_pane: PaneId,
) -> Vec<(Arc<TerminalSession>, TerminalGeometry)> {
    let Some(visible_panes) = inner.visible_terminals.get(&client) else {
        return Vec::new();
    };
    if !visible_panes.contains(&source_pane) || !inner.terminals.contains_key(&source_pane) {
        return Vec::new();
    }
    let previous_owners = visible_panes
        .iter()
        .map(|pane| (*pane, terminal_geometry_owner(inner, *pane)))
        .collect::<Vec<_>>();

    let sequence = inner
        .terminal_input_sequence
        .checked_add(1)
        .expect("terminal input sequence exhausted");
    inner.terminal_input_sequence = sequence;
    inner
        .client_terminal_input_sequences
        .insert(client, sequence);

    previous_owners
        .into_iter()
        .filter(|(pane, previous)| terminal_geometry_owner(inner, *pane) != *previous)
        .filter_map(|(pane, _)| terminal_resize_for_pane(inner, pane))
        .collect()
}

fn terminal_view_action_is_input(action: &zz_terminal::TerminalViewAction) -> bool {
    use zz_terminal::TerminalViewAction as Action;

    match action {
        Action::Focus(_) | Action::ClearLinkHover => false,
        Action::Mouse(input) => input.phase() != zz_terminal::TerminalMousePhase::Motion,
        Action::ScrollLines(_)
        | Action::ScrollPages(_)
        | Action::ScrollTop
        | Action::ScrollBottom
        | Action::ScrollToFraction(_)
        | Action::ScrollToOffset(_)
        | Action::ScrollWheel { .. }
        | Action::SelectionPress(_)
        | Action::SelectionDrag(_)
        | Action::SelectionAutoscroll { .. }
        | Action::SelectionRelease(_)
        | Action::SelectAll
        | Action::ClearSelection
        | Action::ClearHistory
        | Action::EnterCopyMode
        | Action::EnterCopyModeScrollExit
        | Action::CopyMode(_)
        | Action::CopySelection { .. }
        | Action::SearchBegin(_)
        | Action::SearchUpdate(_)
        | Action::SearchNext
        | Action::SearchPrevious
        | Action::SearchClose
        | Action::Paste(_) => true,
    }
}

fn terminal_resizes_for_panes(
    inner: &ServerState,
    panes: &BTreeSet<PaneId>,
) -> Vec<(Arc<TerminalSession>, TerminalGeometry)> {
    panes
        .iter()
        .filter_map(|pane| terminal_resize_for_pane(inner, *pane))
        .collect()
}

fn apply_terminal_resizes(resizes: Vec<(Arc<TerminalSession>, TerminalGeometry)>) {
    for (terminal, geometry) in resizes {
        terminal.resize(
            geometry.columns,
            geometry.rows,
            geometry.cell_width_px,
            geometry.cell_height_px,
        );
    }
}

fn find_buffer<'a>(inner: &'a ServerState, name: Option<&str>) -> Option<&'a PasteBuffer> {
    inner
        .paste_buffers
        .iter()
        .find(|buffer| name.map_or(buffer.automatic, |name| buffer.name == name))
}

fn buffer_format_facts(buffer: &PasteBuffer) -> BufferFormatFacts {
    BufferFormatFacts {
        name: buffer.name.clone(),
        data: Arc::clone(&buffer.data),
        created: buffer.created,
    }
}

fn format_hook_facts(inner: &ServerState) -> FormatHookFacts {
    FormatHookFacts {
        terminals: Arc::new(inner.terminals.clone()),
        buffer: inner
            .paste_buffers
            .iter()
            .find(|buffer| buffer.automatic)
            .map(buffer_format_facts),
        ..FormatHookFacts::default()
    }
}

fn resolve_buffer<'a>(
    inner: &'a ServerState,
    name: Option<&str>,
) -> Result<&'a PasteBuffer, ServerError> {
    find_buffer(inner, name).ok_or_else(|| {
        name.map_or_else(
            || ServerError::MissingTarget("paste buffer".to_owned()),
            |name| ServerError::MissingTarget(name.to_owned()),
        )
    })
}

#[derive(Debug, Default)]
struct ParsedBufferCommandArgs {
    flags: BTreeSet<char>,
    values: BTreeMap<char, String>,
    positional: Vec<String>,
}

impl ParsedBufferCommandArgs {
    fn has(&self, flag: char) -> bool {
        self.flags.contains(&flag)
    }

    fn value(&self, option: char) -> Option<&str> {
        self.values.get(&option).map(String::as_str)
    }
}

fn parse_buffer_command_args(
    command: &str,
    args: &[String],
    value_options: &[char],
    flags: &[char],
) -> Result<ParsedBufferCommandArgs, ServerError> {
    let mut parsed = ParsedBufferCommandArgs::default();
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument == "--" {
            parsed.positional.extend(args[index + 1..].iter().cloned());
            break;
        }
        if !argument.starts_with('-') || argument == "-" {
            parsed.positional.push(argument.clone());
            index += 1;
            continue;
        }

        let mut consumed_next = false;
        for (offset, option) in argument[1..].char_indices() {
            if value_options.contains(&option) {
                let value_start = 1 + offset + option.len_utf8();
                let value = if value_start < argument.len() {
                    argument[value_start..].to_owned()
                } else {
                    consumed_next = true;
                    args.get(index + 1).cloned().ok_or_else(|| {
                        ServerError::InvalidCommand(format!("{command} -{option} requires a value"))
                    })?
                };
                parsed.values.insert(option, value);
                break;
            }
            if flags.contains(&option) {
                parsed.flags.insert(option);
            } else {
                return Err(ServerError::UnsupportedCommand(format!(
                    "{command} -{option}"
                )));
            }
        }
        index += usize::from(consumed_next) + 1;
    }
    Ok(parsed)
}

fn require_no_positionals(
    command: &str,
    parsed: &ParsedBufferCommandArgs,
) -> Result<(), ServerError> {
    if parsed.positional.is_empty() {
        Ok(())
    } else {
        Err(ServerError::InvalidCommand(format!(
            "{command} does not accept positional arguments"
        )))
    }
}

fn require_one_positional<'a>(
    command: &str,
    parsed: &'a ParsedBufferCommandArgs,
) -> Result<&'a str, ServerError> {
    let [value] = parsed.positional.as_slice() else {
        return Err(ServerError::InvalidCommand(format!(
            "{command} requires exactly one path"
        )));
    };
    Ok(value)
}

fn validate_paste_buffer_size(size: usize) -> Result<(), ServerError> {
    if size > MAX_PASTE_BUFFER_BYTES {
        Err(ServerError::InvalidCommand(format!(
            "paste buffer exceeds {MAX_PASTE_BUFFER_BYTES} bytes"
        )))
    } else {
        Ok(())
    }
}

fn validate_paste_buffer_name(name: &str) -> Result<(), ServerError> {
    if name.is_empty() {
        return Err(ServerError::InvalidCommand(
            "paste buffer name cannot be empty".to_owned(),
        ));
    }
    if name.len() > MAX_PASTE_BUFFER_NAME_BYTES {
        return Err(ServerError::InvalidCommand(format!(
            "paste buffer name exceeds {MAX_PASTE_BUFFER_NAME_BYTES} bytes"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ServerError::InvalidCommand(
            "paste buffer name contains control characters".to_owned(),
        ));
    }
    Ok(())
}

fn insert_paste_buffer(
    inner: &mut ServerState,
    requested_name: Option<&str>,
    automatic_prefix: &str,
    data: Vec<u8>,
) -> Result<(), ServerError> {
    if data.is_empty() {
        return Ok(());
    }
    validate_paste_buffer_size(data.len())?;
    let (name, automatic) = if let Some(name) = requested_name {
        validate_paste_buffer_name(name)?;
        (name.to_owned(), false)
    } else {
        let mut name = None;
        for _ in 0..inner.paste_buffers.len().saturating_add(1) {
            let candidate = format!("{automatic_prefix}{}", inner.next_buffer_id);
            inner.next_buffer_id = inner.next_buffer_id.wrapping_add(1);
            validate_paste_buffer_name(&candidate)?;
            if !inner
                .paste_buffers
                .iter()
                .any(|buffer| buffer.name == candidate)
            {
                name = Some(candidate);
                break;
            }
        }
        let name = name.ok_or_else(|| {
            ServerError::InvalidCommand("no automatic paste buffer names available".to_owned())
        })?;
        while inner
            .paste_buffers
            .iter()
            .filter(|buffer| buffer.automatic)
            .count()
            >= inner.automatic_paste_buffer_limit.0
        {
            let Some(index) = inner
                .paste_buffers
                .iter()
                .rposition(|buffer| buffer.automatic)
            else {
                break;
            };
            inner.paste_buffers.remove(index);
        }
        (name, true)
    };

    let utf8 = std::str::from_utf8(&data).is_ok();
    inner.paste_buffers.retain(|buffer| buffer.name != name);
    inner.paste_buffers.insert(
        0,
        PasteBuffer {
            name,
            data: Arc::from(data),
            created: SystemTime::now(),
            automatic,
            utf8,
        },
    );
    Ok(())
}

fn read_paste_buffer_file(path: &Path) -> Result<Vec<u8>, ServerError> {
    let file =
        fs::File::open(path).map_err(|error| buffer_file_error("load-buffer", path, &error))?;
    if file
        .metadata()
        .map_err(|error| buffer_file_error("load-buffer", path, &error))?
        .len()
        > u64::try_from(MAX_PASTE_BUFFER_BYTES).expect("paste buffer limit fits in u64")
    {
        return Err(ServerError::InvalidCommand(format!(
            "{}: paste buffer exceeds {MAX_PASTE_BUFFER_BYTES} bytes",
            path.display()
        )));
    }
    let mut data = Vec::new();
    file.take(
        u64::try_from(MAX_PASTE_BUFFER_BYTES)
            .expect("paste buffer limit fits in u64")
            .saturating_add(1),
    )
    .read_to_end(&mut data)
    .map_err(|error| buffer_file_error("load-buffer", path, &error))?;
    validate_paste_buffer_size(data.len())?;
    Ok(data)
}

fn write_paste_buffer_file(path: &Path, data: &[u8], append: bool) -> Result<(), ServerError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let mut file = options
        .open(path)
        .map_err(|error| buffer_file_error("save-buffer", path, &error))?;
    file.write_all(data)
        .and_then(|()| file.flush())
        .map_err(|error| buffer_file_error("save-buffer", path, &error))
}

fn buffer_file_error(command: &str, path: &Path, error: &std::io::Error) -> ServerError {
    ServerError::InvalidCommand(format!("{command}: {}: {error}", path.display()))
}

#[derive(Debug)]
struct ParsedCapturePane {
    target: Option<String>,
    buffer_name: Option<String>,
    options: CaptureOptions,
    quiet: bool,
}

fn parse_capture_pane_args(args: &[String]) -> Result<ParsedCapturePane, ServerError> {
    let mut parsed = ParsedCapturePane {
        target: None,
        buffer_name: None,
        options: CaptureOptions::default(),
        quiet: false,
    };
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if matches!(argument.as_str(), "-t" | "-S" | "-E" | "-b") {
            let value = args.get(index + 1).ok_or_else(|| {
                ServerError::InvalidCommand(format!("{argument} requires a value"))
            })?;
            apply_capture_value(&mut parsed, argument, value)?;
            index += 2;
            continue;
        }
        if let Some((option, value)) = ["-t", "-S", "-E", "-b"].iter().find_map(|option| {
            argument
                .strip_prefix(option)
                .filter(|value| !value.is_empty())
                .map(|value| (*option, value))
        }) {
            apply_capture_value(&mut parsed, option, value)?;
            index += 1;
            continue;
        }
        if argument == "--" {
            if index + 1 != args.len() {
                return Err(ServerError::InvalidCommand(
                    "capture-pane does not accept positional arguments".to_owned(),
                ));
            }
            break;
        }
        if !argument.starts_with('-') || argument == "-" {
            return Err(ServerError::InvalidCommand(format!(
                "unexpected capture-pane argument: {argument}"
            )));
        }
        for flag in argument[1..].chars() {
            match flag {
                'a' => parsed.options.alternate = true,
                'e' => parsed.options.escape_sequences = true,
                'J' => {
                    parsed.options.join_wrapped = true;
                    parsed.options.preserve_trailing = true;
                }
                'M' => parsed.options.mode = true,
                'N' => parsed.options.preserve_trailing = true,
                'p' | 'T' => {}
                'q' => parsed.quiet = true,
                unsupported => {
                    return Err(ServerError::UnsupportedCommand(format!(
                        "capture-pane -{unsupported}"
                    )));
                }
            }
        }
        index += 1;
    }
    Ok(parsed)
}

fn apply_capture_value(
    parsed: &mut ParsedCapturePane,
    option: &str,
    value: &str,
) -> Result<(), ServerError> {
    match option {
        "-t" => parsed.target = Some(value.to_owned()),
        "-S" => parsed.options.start = parse_capture_boundary(value, true)?,
        "-E" => parsed.options.end = parse_capture_boundary(value, false)?,
        "-b" => parsed.buffer_name = Some(value.to_owned()),
        _ => unreachable!("capture value option is validated by the caller"),
    }
    Ok(())
}

fn parse_capture_boundary(value: &str, start: bool) -> Result<CaptureBoundary, ServerError> {
    if value == "-" {
        return Ok(if start {
            CaptureBoundary::HistoryStart
        } else {
            CaptureBoundary::VisibleEnd
        });
    }
    value
        .parse::<i64>()
        .map(CaptureBoundary::Relative)
        .map_err(|_| ServerError::InvalidCommand(format!("invalid capture-pane line: {value}")))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ContextReference {
    path: String,
    start: Option<u32>,
    end: Option<u32>,
}

impl ContextReference {
    fn parse(value: &str) -> Result<Self, ServerError> {
        let invalid = |message: &str| ServerError::InvalidCommand(message.to_owned());
        let (path, range) = match value.rsplit_once(':') {
            Some((path, range)) if !path.is_empty() && parse_line_range(range).is_some() => {
                (path, parse_line_range(range))
            }
            _ => (value, None),
        };
        if path.is_empty() || path.len() > MAX_CONTEXT_PATH_BYTES {
            return Err(invalid("--context path must be 1..=4096 bytes"));
        }
        if path.chars().any(char::is_control) {
            return Err(invalid(
                "--context path must not contain control characters",
            ));
        }
        let (start, end) = range.unwrap_or((None, None));
        if let (Some(start), Some(end)) = (start, end)
            && start > end
        {
            return Err(invalid("--context line range must not run backwards"));
        }
        Ok(Self {
            path: path.to_owned(),
            start,
            end,
        })
    }

    fn header(&self) -> String {
        match (self.start, self.end) {
            (Some(start), Some(end)) => format!("{}:{start}-{end}", self.path),
            (Some(start), None) => format!("{}:{start}", self.path),
            _ => self.path.clone(),
        }
    }
}

fn parse_line_range(value: &str) -> Option<(Option<u32>, Option<u32>)> {
    if let Some((start, end)) = value.split_once('-') {
        return Some((Some(start.parse().ok()?), Some(end.parse().ok()?)));
    }
    Some((Some(value.parse().ok()?), None))
}

#[derive(Debug, Default)]
struct ParsedAgentSend {
    target: Option<String>,
    submit: bool,
    context: Option<ContextReference>,
    text: Vec<String>,
}

impl ParsedAgentSend {
    fn payload(&self) -> Result<String, ServerError> {
        let text = self.text.join(" ");
        let text = text.trim_end_matches(['\n', '\r']);
        if text.trim().is_empty() {
            return Err(ServerError::InvalidCommand(
                "agent-send needs text on the command line or on standard input".to_owned(),
            ));
        }
        if text.len() > MAX_AGENT_SEND_BYTES {
            return Err(ServerError::InvalidCommand(format!(
                "agent-send payload is {} bytes; the limit is {MAX_AGENT_SEND_BYTES}",
                text.len()
            )));
        }
        if text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        {
            return Err(ServerError::InvalidCommand(
                "agent-send payload contains control characters; pipe plain text".to_owned(),
            ));
        }
        let payload = match &self.context {
            Some(reference) => fenced_block(&reference.header(), text),
            None => text.to_owned(),
        };
        if payload.len() > MAX_AGENT_SEND_BYTES {
            return Err(ServerError::InvalidCommand(format!(
                "agent-send payload is {} bytes with its context header; the limit is \
                 {MAX_AGENT_SEND_BYTES}",
                payload.len()
            )));
        }
        Ok(payload)
    }
}

fn fenced_block(header: &str, text: &str) -> String {
    let widest = text
        .split('\n')
        .flat_map(|line| line.split(|character| character != '`'))
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(widest.saturating_add(1).max(3));
    format!("{header}\n{fence}\n{text}\n{fence}")
}

fn last_command_block(pane: PaneId, capture: &LastCommandCapture) -> String {
    let mut body = String::new();
    if capture.truncated_rows > 0 {
        let _ = writeln!(
            body,
            "truncated: dropped the first {} lines",
            capture.truncated_rows
        );
    }
    body.push_str(&capture.output);
    fenced_block(&format!("{pane} $ {}", capture.command), &body)
}

/// Whether `zz agent-send` must read its payload from standard input.
///
/// Malformed arguments answer `false` and let the daemon report the real error.
#[must_use]
pub fn agent_send_reads_stdin(args: &[String]) -> bool {
    parse_agent_send_args(args).is_ok_and(|parsed| parsed.text.is_empty())
}

fn parse_agent_send_args(args: &[String]) -> Result<ParsedAgentSend, ServerError> {
    let mut parsed = ParsedAgentSend::default();
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument == "--" {
            parsed
                .text
                .extend(args[index.saturating_add(1)..].iter().cloned());
            break;
        }
        if argument == "--submit" {
            parsed.submit = true;
            index += 1;
            continue;
        }
        if let Some(value) = option_value("agent-send", args, index, &["-t", "--target"])? {
            parsed.target = Some(value.value);
            index += value.consumed;
            continue;
        }
        if let Some(value) = option_value("agent-send", args, index, &["--context"])? {
            parsed.context = Some(ContextReference::parse(&value.value)?);
            index += value.consumed;
            continue;
        }
        if argument.starts_with('-') && argument != "-" {
            return Err(ServerError::UnsupportedCommand(format!(
                "agent-send {argument}"
            )));
        }
        parsed.text.push(argument.clone());
        index += 1;
    }
    Ok(parsed)
}

#[derive(Debug, Default)]
struct ParsedCaptureBrowser {
    target: Option<String>,
    output: Option<String>,
}

fn parse_capture_browser_args(args: &[String]) -> Result<ParsedCaptureBrowser, ServerError> {
    let mut parsed = ParsedCaptureBrowser::default();
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument == "--" {
            if index.saturating_add(1) != args.len() {
                return Err(ServerError::InvalidCommand(
                    "capture-browser does not accept positional arguments".to_owned(),
                ));
            }
            break;
        }
        if let Some(value) = option_value("capture-browser", args, index, &["-t"])? {
            parsed.target = Some(value.value);
            index += value.consumed;
            continue;
        }
        if let Some(value) = option_value("capture-browser", args, index, &["-o"])? {
            parsed.output = Some(value.value);
            index += value.consumed;
            continue;
        }
        return Err(ServerError::InvalidCommand(format!(
            "unexpected capture-browser argument: {argument}"
        )));
    }
    Ok(parsed)
}

fn parse_target_only_args(command: &str, args: &[String]) -> Result<Option<String>, ServerError> {
    let mut target = None;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument == "--" {
            if index.saturating_add(1) != args.len() {
                return Err(ServerError::InvalidCommand(format!(
                    "{command} does not accept positional arguments"
                )));
            }
            break;
        }
        if let Some(value) = option_value(command, args, index, &["-t"])? {
            target = Some(value.value);
            index += value.consumed;
            continue;
        }
        return Err(ServerError::InvalidCommand(format!(
            "unexpected {command} argument: {argument}"
        )));
    }
    Ok(target)
}

struct OptionValue {
    value: String,
    consumed: usize,
}

fn option_value(
    command: &str,
    args: &[String],
    index: usize,
    names: &[&str],
) -> Result<Option<OptionValue>, ServerError> {
    let argument = &args[index];
    for name in names {
        if argument == name {
            let value = args.get(index.saturating_add(1)).ok_or_else(|| {
                ServerError::InvalidCommand(format!("{command} {name} requires a value"))
            })?;
            return Ok(Some(OptionValue {
                value: value.clone(),
                consumed: 2,
            }));
        }
        let attached = name
            .strip_prefix("--")
            .and_then(|_| argument.strip_prefix(&format!("{name}=")))
            .or_else(|| {
                (!name.starts_with("--"))
                    .then(|| argument.strip_prefix(name))
                    .flatten()
            })
            .filter(|value| !value.is_empty());
        if let Some(value) = attached {
            return Ok(Some(OptionValue {
                value: value.to_owned(),
                consumed: 1,
            }));
        }
    }
    Ok(None)
}

fn debug_marker(client: ClientId, context: &ExecutionContext, args: &[String]) -> Execution {
    log::info!(
        target: "zz_daemon::diagnostics::marker",
        "user_marker client={client} session={:?} window={:?} pane={:?} note={:?}",
        context.session,
        context.window,
        context.pane,
        args.join(" "),
    );
    log::logger().flush();
    Execution::default()
}

fn workspace_tools_catalog() -> Execution {
    Execution {
        output: WORKSPACE_TOOLS.to_owned(),
        effects: Vec::new(),
    }
}

const WORKSPACE_TOOLS: &str = "\
zz workspace verbs: drive the surrounding zz session from inside an agent pane.

Targets are stable IDs: %N a pane, @N a window, $N a session. Your own pane is
$ZZ_PANE, your session is $ZZ_SESSION, and the daemon socket is $ZZ_SOCKET.
Run `zz list-panes -t @N` or `zz list-windows` to discover the rest.

  zz tools
      Print this catalog.

  zz agent-send [-t %N] [--submit] [--context PATH[:START[-END]]] [TEXT]
      Put TEXT in an agent pane's composer for its user to review, or submit
      it outright with --submit (only when that pane is idle). A -t naming a
      non-agent pane, or no -t at all, routes to that window's most recently
      focused agent pane, so a pipe from a terminal needs no addressing at
      all: `git diff | zz agent-send`. Reads standard input when TEXT is
      omitted. --context adds a file/line header and fences the payload.

  zz capture-pane -p -t %N [-S -] [-E -] [-J]
      Print a terminal pane's text. -S -/-E - widen the range to the whole
      scrollback; -J rejoins soft-wrapped lines.

  zz send-last-output -t %N
      Send a terminal pane's last completed command and its output to the most
      recently focused agent pane in the same window. Needs a shell that emits
      OSC 133 prompt marks.

  zz capture-browser -t %N -o /absolute/out.png
      Write a browser pane's latest rendered frame to a PNG.

  zz set-browser-url -t %N URL
      Point a browser pane at URL.

  zz send-keys -t %N 'text' Enter
      Type into a terminal pane. -l sends text literally.

  zz split-window | zz split-browser | zz split-picker [-h|-v] [-t %N]
      Add a pane beside another one.

  zz debug-marker [NOTE]
      Stamp a user_marker line into the daemon log, so the moment something
      looked wrong is easy to find when reading diagnostics later.
";

fn ensure_browser_attached(inner: &ServerState, pane: PaneId) -> Result<(), ServerError> {
    let window = inner
        .engine
        .state
        .window_for_pane(pane)
        .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
    let session = inner.engine.state.windows[&window].session;
    if inner
        .attached
        .get(&session)
        .is_some_and(|clients| !clients.is_empty())
    {
        Ok(())
    } else {
        Err(ServerError::PaneNotAttached(pane))
    }
}

fn handle_connection<S: TransportStream>(
    mut stream: S,
    shared: &Arc<Shared>,
) -> Result<(), DaemonError> {
    let connection_started = diagnostic_timer();
    let mut inbound_frame = Vec::new();
    let first_message = match read_protocol_message_into(&mut stream, &mut inbound_frame) {
        Ok(message) => message,
        Err(error @ ProtocolError::VersionMismatch { received, .. }) => {
            best_effort_protocol_mismatch_reply(&mut stream, received);
            log::info!(
                target: "zz_daemon::diagnostics::connection",
                "rejected client protocol={received} server_protocol={PROTOCOL_VERSION}",
            );
            return Err(error.into());
        }
        Err(error) => return Err(error.into()),
    };
    let ProtocolMessage::ClientHello(hello) = first_message else {
        return Err(ServerError::InvalidCommand(
            "first protocol message must be ClientHello".to_owned(),
        )
        .into());
    };
    if let Err(error) = validate_hello(&hello) {
        best_effort_protocol_mismatch_reply(&mut stream, hello.protocol_version);
        log::info!(
            target: "zz_daemon::diagnostics::connection",
            "rejected client hello protocol={} server_protocol={PROTOCOL_VERSION}",
            hello.protocol_version,
        );
        return Err(error);
    }

    let outbound = OutboundMailbox::new();
    let (client, server_hello) = shared.register(
        hello.kind,
        hello.client_instance_id,
        hello.device_name.clone(),
        hello.color_scheme,
    );
    let mut registration = ClientRegistrationGuard::new(shared, client);
    log::debug!(
        target: "zz_daemon::diagnostics::connection",
        "registered client={client} kind={:?} hello={hello:#?}",
        hello.kind,
    );
    let mut writer = stream.try_clone()?;
    let writer_mailbox = Arc::clone(&outbound);
    let writer_thread = thread::Builder::new()
        .name(format!("zz-client-writer-{}", client.0))
        .spawn(move || write_outbound(&mut writer, &writer_mailbox))
        .map_err(|error| DaemonError::Thread(error.to_string()))?;
    let _ = outbound.enqueue_reliable(&ProtocolMessage::ServerHello(server_hello));
    if hello.kind == ClientKind::Interactive {
        shared.subscribe(client, Arc::clone(&outbound));
    }

    let mut context = {
        let inner = shared.inner.lock();
        hello
            .origin
            .and_then(|pane| ExecutionContext::for_pane(&inner.engine.state, pane))
            .or_else(|| {
                let fallback = if hello.kind == ClientKind::Command {
                    inner.engine.state.most_recent_context()
                } else {
                    inner.engine.state.default_context()
                };
                fallback.map(|(session, window, pane)| ExecutionContext {
                    session: Some(session),
                    window: Some(window),
                    pane: Some(pane),
                })
            })
            .unwrap_or_default()
    };

    let result = loop {
        let message = match read_protocol_message_into(&mut stream, &mut inbound_frame) {
            Ok(message) => message,
            Err(ProtocolError::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset | ErrorKind::BrokenPipe
                ) =>
            {
                break Ok(());
            }
            Err(error) => break Err(error.into()),
        };
        let message_started = diagnostic_timer();
        log::trace!(
            target: "zz_daemon::diagnostics::connection",
            "message begin client={client} bytes={} frame_capacity={} message={message:#?}",
            inbound_frame.len(),
            inbound_frame.capacity(),
        );
        match message {
            ProtocolMessage::CommandRequest(CommandRequest {
                request_id,
                command,
            }) => {
                let response = shared.execute_command_request(
                    client,
                    hello.kind,
                    &mut context,
                    request_id,
                    &command,
                );
                let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(response));
            }
            ProtocolMessage::Attach { session } => match shared.attach_target(client, &session) {
                Ok((session, snapshot)) => {
                    outbound.reset_kitty_images();
                    outbound.reset_pasted_images();
                    let _ =
                        outbound.enqueue_reliable(&ProtocolMessage::Attached { session, snapshot });
                    shared.send_resync(client, &outbound);
                    shared.publish_snapshot();
                }
                Err(error) => {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error,
                        },
                    ));
                }
            },
            ProtocolMessage::Detach => {
                shared.detach(client);
            }
            ProtocolMessage::SetColorScheme(color_scheme) => {
                shared.set_client_color_scheme(client, color_scheme);
            }
            ProtocolMessage::SetConfigOverrides { entries } => {
                shared.set_config_overrides(client, hello.kind, &entries);
            }
            ProtocolMessage::Input(input) => {
                if let Err(error) = shared.input(client, hello.kind, &mut context, input) {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error: daemon_server_error(error),
                        },
                    ));
                }
            }
            ProtocolMessage::GuiResponse(response) => {
                shared.complete_gui_request(client, response);
            }
            ProtocolMessage::Resync => shared.send_resync(client, &outbound),
            ProtocolMessage::RequestFull { pane } => {
                shared.send_full(client, pane, &outbound);
            }
            ProtocolMessage::HistoryRequest { pane, start, count } => {
                shared.send_history(client, pane, start, count, &outbound);
            }
            ProtocolMessage::PasteUploadBegin {
                upload_id,
                pane,
                purpose,
                extension,
                total_bytes,
            } => {
                shared.begin_paste_upload(
                    client,
                    hello.kind,
                    upload_id,
                    pane,
                    purpose,
                    extension,
                    total_bytes,
                );
            }
            ProtocolMessage::PasteUploadChunk { upload_id, bytes } => {
                shared.extend_paste_upload(client, upload_id, &bytes);
            }
            ProtocolMessage::FetchPastedImage { pane, number } => {
                shared.fetch_pasted_image(client, pane, number);
            }
            message @ (ProtocolMessage::AgentPrompt { .. }
            | ProtocolMessage::AgentCancel { .. }
            | ProtocolMessage::AgentUnqueue { .. }
            | ProtocolMessage::AgentRespondPermission { .. }
            | ProtocolMessage::AgentSetConfigOption { .. }
            | ProtocolMessage::AgentSetMode { .. }
            | ProtocolMessage::AgentAuthenticate { .. }
            | ProtocolMessage::AgentSessionOp { .. }
            | ProtocolMessage::AgentReplay { .. }
            | ProtocolMessage::AgentAcknowledgePromptRestore { .. }) => {
                if let Err(error) = handle_agent_message(shared, client, message) {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error,
                        },
                    ));
                }
            }
            _ => {}
        }
        log::trace!(
            target: "zz_daemon::diagnostics::connection",
            "message end client={client} elapsed_us={} context={context:#?}",
            diagnostic_elapsed_us(message_started),
        );
    };

    registration.unregister();
    outbound.close();
    if writer_thread.join().is_err() {
        log::error!(
            target: "zz_daemon::diagnostics::connection",
            "writer thread panicked for client={client}",
        );
    }
    log::debug!(
        target: "zz_daemon::diagnostics::connection",
        "unregistered client={client} success={} connection_elapsed_us={}",
        result.is_ok(),
        diagnostic_elapsed_us(connection_started),
    );
    result
}

#[cfg(feature = "agent")]
fn handle_agent_message(
    shared: &Arc<Shared>,
    client: ClientId,
    message: ProtocolMessage,
) -> Result<(), ServerError> {
    shared.agent_message(client, message)
}

/// A daemon built without the agent feature still speaks the protocol; it just
/// has nothing to run the adapter with.
#[cfg(not(feature = "agent"))]
fn handle_agent_message(
    _shared: &Arc<Shared>,
    _client: ClientId,
    _message: ProtocolMessage,
) -> Result<(), ServerError> {
    Err(ServerError::UnsupportedCommand(
        "this daemon was built without agent support".to_owned(),
    ))
}

fn best_effort_protocol_mismatch_reply(stream: &mut impl Write, client: u16) {
    let message = ProtocolMessage::CommandResponse(CommandResponse::Error {
        request_id: 0,
        error: ServerError::ProtocolMismatch {
            client,
            server: PROTOCOL_VERSION,
        },
    });
    let mut frame = Vec::new();
    if encode_protocol_message_into(&message, &mut frame).is_ok() {
        let _ = stream.write_all(&frame).and_then(|()| stream.flush());
    }
}

fn write_outbound(stream: &mut impl TransportStream, outbound: &OutboundMailbox) {
    while let Some(frame) = outbound.recv() {
        let started = diagnostic_timer();
        let bytes = frame.len();
        let capacity = frame.capacity();
        let write_started = diagnostic_timer();
        let write_result = stream.write_all(&frame);
        let write_us = diagnostic_elapsed_us(write_started);
        let flush_started = diagnostic_timer();
        let result = write_result.and_then(|()| stream.flush());
        let flush_us = diagnostic_elapsed_us(flush_started);
        log::trace!(
            target: "zz_daemon::diagnostics::outbound",
            "write bytes={bytes} frame_capacity={capacity} success={} write_us={} flush_us={} elapsed_us={}",
            result.is_ok(),
            write_us,
            flush_us,
            diagnostic_elapsed_us(started),
        );
        if result.is_err() {
            outbound.close();
            break;
        }
        outbound.recycle_frame(frame);
    }
}

fn validate_hello(hello: &ClientHello) -> Result<(), DaemonError> {
    if hello.protocol_version != PROTOCOL_VERSION {
        return Err(ServerError::ProtocolMismatch {
            client: hello.protocol_version,
            server: PROTOCOL_VERSION,
        }
        .into());
    }
    Ok(())
}

fn daemon_server_error(error: DaemonError) -> ServerError {
    match error {
        DaemonError::Server(error) => error,
        other => ServerError::Internal(other.to_string()),
    }
}

#[cfg(unix)]
fn prepare_socket(path: &Path) -> Result<(), DaemonError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        match LocalTransport::connect(path) {
            Ok(_) => return Err(DaemonError::AlreadyRunning(path.to_owned())),
            Err(_) => fs::remove_file(path)?,
        }
    }
    Ok(())
}

#[cfg(windows)]
fn prepare_socket(_: &Path) -> Result<(), DaemonError> {
    Ok(())
}

#[cfg(unix)]
fn restrict_socket_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn restrict_socket_permissions(_: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

fn server_id() -> u64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let folded = u64::try_from(timestamp & u128::from(u64::MAX)).unwrap_or_default();
    folded ^ u64::from(std::process::id())
}

fn mux_set_option_command(option: MuxOptionKey, value: &str) -> CommandInvocation {
    CommandInvocation::new("set-option", ["-g", "--", option.as_str(), value])
}

fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home_directory().map_or_else(|| PathBuf::from(path), |home| home.join(rest));
    }
    PathBuf::from(path)
}

#[derive(Default)]
struct SourceGlobMatches {
    paths: Vec<PathBuf>,
    errors: Vec<String>,
}

fn source_glob_matches(path: &Path) -> SourceGlobMatches {
    match glob::glob(path.to_string_lossy().as_ref()) {
        Ok(paths) => {
            let mut matches = SourceGlobMatches::default();
            for path in paths {
                match path {
                    Ok(path) => matches.paths.push(path),
                    Err(error) => matches.errors.push(error.to_string()),
                }
            }
            matches.paths.sort();
            matches
        }
        Err(error) => SourceGlobMatches {
            paths: Vec::new(),
            errors: vec![error.to_string()],
        },
    }
}

fn expand_relative(source: &Path, nested: &str) -> PathBuf {
    let nested = expand_path(nested);
    if nested.is_absolute() {
        nested
    } else {
        source
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(nested)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Instant;

    use crossbeam_channel::Receiver;
    use zz_protocol::{
        EventPayload, LayoutNode, ProtocolMessage, TerminalUiCommand, decode_protocol_frame,
        encode_protocol_message,
    };
    use zz_terminal::{
        GRAPHEME_TABLE_BIT, KeyAction, KeyCode, KeyInput, Modifiers, PointerCellEvent,
        SearchDirection, SessionStatus, TerminalDictionary, TerminalMode, TerminalMouseButton,
        TerminalMouseInput, TerminalMousePhase, TerminalViewAction,
    };

    use super::*;
    use crate::{CommandClient, InteractiveClient};

    #[cfg(unix)]
    impl TransportStream for std::os::unix::net::UnixStream {
        fn try_clone(&self) -> std::io::Result<Self> {
            std::os::unix::net::UnixStream::try_clone(self)
        }
    }

    #[test]
    fn editor_sniff_matches_the_pin_basename_rule() {
        for editor in ["vi", "vim", "nvim", "gvim", "/usr/bin/evil"] {
            assert_eq!(
                mode_keys_from_environment(None, Some(OsStr::new(editor))),
                "vi",
                "{editor}"
            );
        }
        for editor in ["emacsclient", "VI", "/usr/bin/emacs"] {
            assert_eq!(
                mode_keys_from_environment(None, Some(OsStr::new(editor))),
                "emacs",
                "{editor}"
            );
        }
        assert_eq!(mode_keys_from_environment(None, None), "emacs");
        assert_eq!(
            mode_keys_from_environment(Some(OsStr::new("")), Some(OsStr::new("vim"))),
            "emacs"
        );
    }

    #[test]
    fn configured_engine_receives_boot_defaults_before_initialization() {
        let shared = Shared::configured_with_boot_environment(
            1,
            Arc::new(TerminalAppearance::default()),
            AppearanceProvenance::default(),
            false,
            std::env::temp_dir().join("zz-test-paste"),
            std::env::temp_dir().join("zz-test.sock"),
            "vi",
            [("PHASE4D_BOOT", "seeded")],
        );
        let mut inner = shared.inner.lock();
        assert_eq!(inner.engine.mux_option_value(MuxOptionKey::ModeKeys), "vi");
        assert_eq!(
            inner
                .engine
                .execute(
                    &mut ExecutionContext::default(),
                    &CommandInvocation::new("show-environment", ["-g", "PHASE4D_BOOT"],),
                )
                .unwrap()
                .output,
            "PHASE4D_BOOT=seeded"
        );
    }

    #[test]
    fn attach_and_attached_key_input_update_session_activity() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) = shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "A"]),
            )
            .unwrap();
        let first = context.session.expect("first session");
        let first_pane = context.pane.expect("first pane");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "B"]),
            )
            .unwrap();
        let second = context.session.expect("second session");
        assert_eq!(
            shared
                .inner
                .lock()
                .engine
                .state
                .most_recent_context()
                .map(|context| context.0),
            Some(second)
        );

        shared.attach(client, first).unwrap();
        assert_eq!(
            shared
                .inner
                .lock()
                .engine
                .state
                .most_recent_context()
                .map(|context| context.0),
            Some(first)
        );
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "C"]),
            )
            .unwrap();
        let third = context.session.expect("third session");
        assert_eq!(
            shared
                .inner
                .lock()
                .engine
                .state
                .most_recent_context()
                .map(|context| context.0),
            Some(third)
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut ExecutionContext::default(),
                InputMessage::Key {
                    pane: first_pane,
                    input: test_key(KeyCode::Character('x'), Modifiers::default(), Some("x")),
                    text_follows: false,
                },
            )
            .unwrap();
        assert_eq!(
            shared
                .inner
                .lock()
                .engine
                .state
                .most_recent_context()
                .map(|context| context.0),
            Some(first)
        );
    }

    #[cfg(unix)]
    #[test]
    fn envelope_mismatch_gets_a_legible_reply_before_disconnect() {
        let shared = Arc::new(Shared::new(1));
        let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let server_shared = Arc::clone(&shared);
        let connection = thread::spawn(move || handle_connection(server, &server_shared));
        let stale = PROTOCOL_VERSION.saturating_sub(1);
        let mut hello = encode_protocol_message(&ProtocolMessage::ClientHello(ClientHello {
            protocol_version: stale,
            client_instance_id: ClientInstanceId(1),
            kind: ClientKind::Command,
            device_name: None,
            capabilities: Vec::new(),
            color_scheme: None,
            origin: None,
        }))
        .unwrap();
        hello[6..8].copy_from_slice(&stale.to_le_bytes());
        client.write_all(&hello).unwrap();
        assert_eq!(
            zz_protocol::read_protocol_message(&mut client).unwrap(),
            ProtocolMessage::CommandResponse(CommandResponse::Error {
                request_id: 0,
                error: ServerError::ProtocolMismatch {
                    client: stale,
                    server: PROTOCOL_VERSION,
                },
            })
        );
        assert!(matches!(
            connection.join().unwrap(),
            Err(DaemonError::Protocol(ProtocolError::VersionMismatch {
                received,
                ..
            })) if received == stale
        ));
    }

    #[cfg(unix)]
    #[test]
    fn client_hello_mismatch_gets_a_legible_reply_before_disconnect() {
        let shared = Arc::new(Shared::new(1));
        let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let server_shared = Arc::clone(&shared);
        let connection = thread::spawn(move || handle_connection(server, &server_shared));
        let stale = PROTOCOL_VERSION.saturating_sub(1);
        zz_protocol::write_protocol_message(
            &mut client,
            &ProtocolMessage::ClientHello(ClientHello {
                protocol_version: stale,
                client_instance_id: ClientInstanceId(1),
                kind: ClientKind::Command,
                device_name: None,
                capabilities: Vec::new(),
                color_scheme: None,
                origin: None,
            }),
        )
        .unwrap();
        assert_eq!(
            zz_protocol::read_protocol_message(&mut client).unwrap(),
            ProtocolMessage::CommandResponse(CommandResponse::Error {
                request_id: 0,
                error: ServerError::ProtocolMismatch {
                    client: stale,
                    server: PROTOCOL_VERSION,
                },
            })
        );
        assert!(matches!(
            connection.join().unwrap(),
            Err(DaemonError::Server(ServerError::ProtocolMismatch {
                client,
                server: PROTOCOL_VERSION,
            })) if client == stale
        ));
    }

    #[test]
    fn client_registration_guard_cleans_up_automatic_and_explicit_exits() {
        let shared = Shared::new(1);

        let automatic = {
            let (client, _) = shared.register_subscribed(
                ClientKind::Interactive,
                Some("automatic".to_owned()),
                None,
                OutboundMailbox::new(),
            );
            let guard = ClientRegistrationGuard::new(&shared, client);
            assert!(shared.inner.lock().subscribers.contains_key(&client));
            drop(guard);
            client
        };
        {
            let inner = shared.inner.lock();
            assert!(!inner.subscribers.contains_key(&automatic));
            assert!(!inner.client_color_schemes.contains_key(&automatic));
            assert!(!inner.client_names.contains_key(&automatic));
        }

        let (explicit, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("explicit".to_owned()),
            None,
            OutboundMailbox::new(),
        );
        let mut guard = ClientRegistrationGuard::new(&shared, explicit);
        guard.unregister();
        drop(guard);
        let inner = shared.inner.lock();
        assert!(!inner.subscribers.contains_key(&explicit));
        assert!(!inner.client_color_schemes.contains_key(&explicit));
        assert!(!inner.client_names.contains_key(&explicit));
    }

    #[test]
    fn terminal_watcher_does_not_retain_idle_session_after_pane_removal() {
        let shared = Arc::new(Shared::new(1));
        let pane = PaneId(u64::MAX - 1);
        let terminal = Arc::new(TerminalSession::spawn_output_view(
            "idle watcher fixture".to_owned(),
            String::new(),
        ));
        let terminal_weak = Arc::downgrade(&terminal);
        shared
            .inner
            .lock()
            .terminals
            .insert(pane, Arc::clone(&terminal));
        shared
            .watch_terminal(pane, &terminal)
            .expect("start terminal watcher");

        let deadline = Instant::now() + Duration::from_secs(2);
        while terminal.diagnostics().event_queue_len != 0 {
            assert!(Instant::now() < deadline, "watcher did not become idle");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            Arc::strong_count(&terminal),
            2,
            "only the test and terminal map should own the session"
        );

        shared.inner.lock().terminals.remove(&pane);
        drop(terminal);
        let deadline = Instant::now() + Duration::from_secs(2);
        while terminal_weak.upgrade().is_some() {
            assert!(
                Instant::now() < deadline,
                "idle terminal remained owned after pane removal"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn terminal_watcher_persists_dynamic_viewport_titles_in_mux_snapshots() {
        let shared = Arc::new(Shared::new(1));
        let pane = shared
            .inner
            .lock()
            .engine
            .state
            .create_session("title-fixture")
            .expect("session")
            .2;
        let terminal = Arc::new(TerminalSession::spawn_output_view(
            "dynamic terminal title".to_owned(),
            String::new(),
        ));
        shared
            .inner
            .lock()
            .terminals
            .insert(pane, Arc::clone(&terminal));
        shared
            .watch_terminal(pane, &terminal)
            .expect("start terminal watcher");
        terminal.attach_view(TerminalViewId(u64::MAX - 1));

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let title = shared
                .inner
                .lock()
                .engine
                .state
                .pane(pane)
                .map(|pane| pane.title.clone());
            if title.as_deref() == Some("dynamic terminal title") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "terminal title did not reach mux state; last title was {title:?}"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let snapshot = shared.inner.lock().engine.state.snapshot();
        assert_eq!(
            snapshot.sessions[0].windows[0].panes[&pane].title,
            "dynamic terminal title"
        );

        let replacement = Arc::new(TerminalSession::spawn_output_view(
            "replacement terminal".to_owned(),
            String::new(),
        ));
        shared
            .inner
            .lock()
            .terminals
            .insert(pane, Arc::clone(&replacement));
        shared.synchronize_pane_title(pane, &terminal, "stale watcher title");
        assert_eq!(
            shared.inner.lock().engine.state.pane(pane).unwrap().title,
            "dynamic terminal title",
            "a retired watcher must not rename its replacement"
        );

        shared.inner.lock().terminals.remove(&pane);
    }

    #[test]
    fn terminal_watcher_classifies_failed_workers_as_reapable() {
        assert!(terminal_status_should_close(&SessionStatus::exited(
            0, None
        )));
        assert!(terminal_status_should_close(&SessionStatus::failed(
            "emulation failure"
        )));
        assert!(!terminal_status_should_close(&SessionStatus::Running));
    }

    #[test]
    fn terminal_process_exit_removes_its_owning_pane_and_requests_shutdown_with_no_client() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "exit-fixture", "exit"]),
            )
            .expect("create terminal pane");
        shared.exit_empty_armed.store(true, Ordering::Release);
        let pane = context.pane.expect("terminal pane");

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let inner = shared.inner.lock();
            let pane_exists = inner.engine.state.window_for_pane(pane).is_some();
            let terminal_exists = inner.terminals.contains_key(&pane);
            drop(inner);
            if !pane_exists && !terminal_exists {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "exited terminal pane remained in mux state"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(shared.stopping.load(Ordering::Acquire));
    }

    #[test]
    fn remain_on_exit_retains_and_respawns_the_same_terminal_pane() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-window-option", ["-g", "remain-on-exit", "on"]),
            )
            .expect("enable retained exits");
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "new-session",
                    ["-s", "dead-fixture", "printf 'ZZ_DEAD_FRAME\\n'; exit 7"],
                ),
            )
            .expect("create exiting terminal pane");
        let session = context.session.expect("terminal session");
        let pane = context.pane.expect("terminal pane");
        let window = context.window.expect("terminal window");
        let layout = shared.inner.lock().engine.state.windows[&window]
            .layout
            .project();

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let inner = shared.inner.lock();
            if inner
                .engine
                .state
                .pane(pane)
                .is_some_and(|pane| pane.dead && pane.dead_status == Some(7))
            {
                assert!(inner.terminals.contains_key(&pane));
                break;
            }
            drop(inner);
            assert!(
                Instant::now() < deadline,
                "exited terminal did not become retained-dead"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let output = shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "display-message",
                    [
                        "-p",
                        "-t",
                        &pane.to_string(),
                        "#{pane_id}:#{pane_dead}:#{pane_dead_status}",
                    ],
                ),
            )
            .expect("read dead facts")
            .output;
        assert_eq!(output, format!("{pane}:1:7"));

        let mailbox = OutboundMailbox::new();
        let (late_client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        shared
            .attach(late_client, session)
            .expect("attach after terminal exit");
        shared.send_resync(late_client, &mailbox);
        let frozen = {
            let state = mailbox.state.lock();
            state
                .terminals
                .get(&pane)
                .map(|pending| pending.encoded.clone())
                .expect("retained frame queued for late client")
        };
        let ProtocolMessage::Event(Event {
            payload:
                EventPayload::TerminalViewport {
                    pane: delivered,
                    viewport,
                },
            ..
        }) = decode_protocol_frame(&frozen).expect("decode retained frame")
        else {
            panic!("late client did not receive a full terminal viewport");
        };
        assert_eq!(delivered, pane);
        assert_eq!(viewport.status, SessionStatus::exited(7, None));
        assert!(viewport_text(&viewport).contains("ZZ_DEAD_FRAME"));

        let previous = Arc::clone(&shared.inner.lock().terminals[&pane]);
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "respawn-pane",
                    ["-E", "-t", &pane.to_string(), "sleep 30"],
                ),
            )
            .expect("respawn dead pane empty");
        let empty = {
            let inner = shared.inner.lock();
            let replacement = Arc::clone(&inner.terminals[&pane]);
            assert!(!Arc::ptr_eq(&previous, &replacement));
            assert!(!inner.engine.state.pane(pane).unwrap().dead);
            assert_eq!(inner.engine.state.windows[&window].layout.project(), layout);
            assert_eq!(replacement.process_id(), None);
            assert_eq!(replacement.foreground_process_id(), None);
            assert_eq!(
                inner.terminal_spawns[&pane].command.as_deref(),
                Some("sleep 30")
            );
            replacement
        };

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let viewport = empty.latest_viewport_for(TerminalViewId(late_client.0));
            if viewport.as_ref().is_some_and(|viewport| {
                matches!(viewport.status, SessionStatus::Running)
                    && matches!(viewport.mode, TerminalMode::Live)
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "empty pane did not publish a live viewport"
            );
            thread::sleep(Duration::from_millis(10));
        }

        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("respawn-pane", ["-t", &pane.to_string()]),
            )
            .expect("respawn empty pane without kill");
        {
            let inner = shared.inner.lock();
            let replacement = &inner.terminals[&pane];
            assert!(!Arc::ptr_eq(&empty, replacement));
            assert!(!inner.engine.state.pane(pane).unwrap().dead);
            assert_eq!(inner.engine.state.windows[&window].layout.project(), layout);
            assert_eq!(
                inner.terminal_spawns[&pane].command.as_deref(),
                Some("sleep 30")
            );
        }

        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("kill-pane", ["-t", &pane.to_string()]),
            )
            .expect("clean up respawned pane");
    }

    #[cfg(unix)]
    #[test]
    fn retained_signal_exit_exposes_the_tmux_signal_name() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-window-option", ["-g", "remain-on-exit", "on"]),
            )
            .expect("enable retained exits");
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "signal-fixture", "kill -TERM $$"]),
            )
            .expect("create signalled terminal pane");
        let pane = context.pane.expect("terminal pane");

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if shared
                .inner
                .lock()
                .engine
                .state
                .pane(pane)
                .is_some_and(|pane| pane.dead)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "signalled terminal did not become retained-dead"
            );
            thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(
            shared
                .execute(
                    ClientId(u64::MAX),
                    ClientKind::Command,
                    &mut context,
                    &CommandInvocation::new(
                        "display-message",
                        [
                            "-p",
                            "-t",
                            &pane.to_string(),
                            "#{pane_dead}:#{pane_dead_status}:#{pane_dead_signal}",
                        ],
                    ),
                )
                .expect("read signal facts")
                .output,
            "1::term"
        );
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("kill-pane", ["-t", &pane.to_string()]),
            )
            .expect("clean up retained signal pane");
    }

    #[test]
    fn custom_appearance_reaches_hello_and_both_terminal_actor_kinds() {
        let appearance = TerminalAppearance {
            font_families: vec!["Review Fixture Mono".to_owned()],
            font_size_points: 17.0,
            foreground: zz_terminal::Color::rgb(0x12, 0x34, 0x56),
            background: zz_terminal::Color::rgb(0x65, 0x43, 0x21),
            ..TerminalAppearance::default()
        };
        let shared = Arc::new(Shared::with_appearance(41, Arc::new(appearance.clone())));
        let mailbox = OutboundMailbox::new();
        let (client, hello) =
            shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
        assert_eq!(hello.appearance, appearance);

        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "appearance"]),
            )
            .expect("create terminal pane");
        let session = context.session.expect("session id");
        let pane = context.pane.expect("terminal pane");
        shared.attach(client, session).expect("attach client");

        let live = Arc::clone(&shared.inner.lock().terminals[&pane]);
        assert_eq!(live.latest_viewport().background, appearance.background);

        shared
            .open_command_output(client, Some(pane), "fixture".to_owned(), "hello")
            .expect("open command-output actor");
        let output = Arc::clone(&shared.inner.lock().command_outputs[&client].terminal);
        assert_eq!(output.latest_viewport().background, appearance.background);
    }

    #[test]
    fn interactive_new_session_attaches_the_requesting_client() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, hello) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        assert!(
            hello
                .capabilities
                .iter()
                .any(|capability| capability == NEW_SESSION_ATTACH_CAPABILITY)
        );
        let mut context = ExecutionContext::default();

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", [] as [&str; 0]),
            )
            .expect("create and attach session");
        let session = context.session.expect("session id");

        assert_eq!(
            shared.inner.lock().attached.get(&session),
            Some(&BTreeSet::from([client]))
        );
        let messages = take_reliable_messages(&mailbox);
        let attached_index = messages.iter().position(|message| {
            matches!(
                message,
                ProtocolMessage::Attached {
                    session: attached,
                    snapshot,
                } if *attached == session
                    && snapshot.sessions.iter().any(|candidate| candidate.id == session)
            )
        });
        let snapshot_index = messages.iter().position(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(_),
                    ..
                })
            )
        });
        assert!(attached_index.is_some());
        assert!(snapshot_index.is_some());
        assert!(attached_index < snapshot_index);
    }

    #[test]
    fn configuration_override_updates_state_provenance_and_broadcast() {
        let shared = Arc::new(Shared::new(44));
        let mailbox = OutboundMailbox::new();
        let (client, hello) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            Some(TerminalColorScheme::Dark),
            Arc::clone(&mailbox),
        );
        assert_eq!(
            hello
                .appearance_provenance
                .source(zz_terminal::AppearanceConfigKey::Background),
            zz_terminal::AppearanceSource::Default
        );

        shared.set_config_overrides(
            client,
            ClientKind::Interactive,
            &[("background".to_owned(), "#123456".to_owned())],
        );

        let inner = shared.inner.lock();
        assert_eq!(
            inner.appearance.background,
            zz_terminal::Color::rgb(0x12, 0x34, 0x56)
        );
        assert_eq!(
            inner
                .appearance_provenance
                .source(zz_terminal::AppearanceConfigKey::Background),
            zz_terminal::AppearanceSource::Override
        );
        drop(inner);
        let state = mailbox.state.lock();
        assert!(state.reliable.iter().any(|frame| {
            matches!(
                decode_protocol_frame(frame),
                Ok(ProtocolMessage::Event(Event {
                    payload: EventPayload::AppearanceChanged {
                        appearance,
                        provenance,
                    },
                    ..
                })) if appearance.background == zz_terminal::Color::rgb(0x12, 0x34, 0x56)
                    && provenance.source(zz_terminal::AppearanceConfigKey::Background)
                        == zz_terminal::AppearanceSource::Override
            )
        }));
    }

    #[test]
    fn mux_override_beats_mux_config_file_and_survives_reload_user_config() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mux_config = directory.path().join("mux.conf");
        fs::write(&mux_config, "set -g prefix C-b\nset -g mode-keys emacs\n")
            .expect("mux config fixture");

        let shared = Arc::new(Shared::new(45));
        let mailbox = OutboundMailbox::new();
        let (client, hello) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            Some(TerminalColorScheme::Dark),
            Arc::clone(&mailbox),
        );
        assert_eq!(
            hello
                .mux_options
                .get(MuxOptionKey::Prefix)
                .expect("prefix in hello")
                .source,
            MuxOptionSource::Default
        );

        let mut context = ExecutionContext::default();
        shared
            .load_config_file(&mux_config, &mut context, 0)
            .expect("initial mux config replay");
        shared.set_config_overrides(
            client,
            ClientKind::Interactive,
            &[
                ("prefix".to_owned(), "C-a".to_owned()),
                ("mode-keys".to_owned(), "vi".to_owned()),
            ],
        );
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.engine.keys.prefix(), "C-a");
            assert_eq!(
                inner
                    .mux_options
                    .get(MuxOptionKey::Prefix)
                    .expect("effective prefix"),
                &zz_protocol::MuxOptionValue {
                    value: "C-a".to_owned(),
                    source: MuxOptionSource::Override,
                }
            );
            assert_eq!(inner.engine.mux_option_value(MuxOptionKey::ModeKeys), "vi");
        }

        shared
            .reload_user_config_with_mux_file(client, &mut context, Some(&mux_config))
            .expect("reload user configuration");
        let inner = shared.inner.lock();
        assert_eq!(inner.engine.keys.prefix(), "C-a");
        assert_eq!(inner.engine.mux_option_value(MuxOptionKey::ModeKeys), "vi");
        assert_eq!(
            inner
                .mux_options
                .get(MuxOptionKey::Prefix)
                .expect("replayed prefix")
                .source,
            MuxOptionSource::Override
        );
        drop(inner);

        let state = mailbox.state.lock();
        assert!(state.reliable.iter().any(|frame| {
            matches!(
                decode_protocol_frame(frame),
                Ok(ProtocolMessage::Event(Event {
                    payload: EventPayload::MuxOptionsChanged { options },
                    ..
                })) if options.get(MuxOptionKey::Prefix).is_some_and(|entry| {
                    entry.value == "C-a" && entry.source == MuxOptionSource::Override
                })
            )
        }));
    }

    #[test]
    fn reload_user_config_resets_key_tables_to_the_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mux_config = directory.path().join("mux.conf");
        fs::write(&mux_config, "bind-key F2 new-window\n").expect("mux config fixture");

        let shared = Arc::new(Shared::new(49));
        let mut context = ExecutionContext::default();
        shared
            .load_config_file(&mux_config, &mut context, 0)
            .expect("source mux config");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("bind-key", ["F3", "kill-pane"]),
            )
            .expect("interactive bind");
        {
            let inner = shared.inner.lock();
            assert!(inner.engine.keys.get("prefix", "F2").is_some());
            assert!(inner.engine.keys.get("prefix", "F3").is_some());
        }

        fs::write(&mux_config, "bind-key F4 new-window\n").expect("rewrite mux config");
        shared
            .reload_user_config_with_mux_file(ClientId(7), &mut context, Some(&mux_config))
            .expect("reload user configuration");
        let inner = shared.inner.lock();
        assert!(inner.engine.keys.get("prefix", "F2").is_none());
        assert!(inner.engine.keys.get("prefix", "F3").is_none());
        assert!(inner.engine.keys.get("prefix", "F4").is_some());
    }

    #[test]
    fn sourced_mux_config_file_binds_keys_and_missing_file_is_tolerated() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mux_config = directory.path().join("mux.conf");
        fs::write(&mux_config, "set -g prefix C-a\nbind-key F2 new-window\n")
            .expect("mux config fixture");

        let shared = Arc::new(Shared::new(48));
        let mut context = ExecutionContext::default();
        shared
            .load_config_file(&mux_config, &mut context, 0)
            .expect("source mux config");
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.engine.keys.prefix(), "C-a");
            assert!(inner.engine.keys.get("prefix", "F2").is_some());
        }

        shared
            .load_config_file(&directory.path().join("missing.conf"), &mut context, 0)
            .expect("a missing mux config file loads defaults");
    }

    #[test]
    fn source_file_globs_nested_sources_and_refuses_standard_input() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let top = directory.path().join("top");
        let nested = directory.path().join("nested");
        fs::create_dir_all(&top).expect("top-level source directory");
        fs::create_dir_all(&nested).expect("nested source directory");
        fs::write(
            top.join("10-first.conf"),
            "set -g prefix C-a\nbind-key F2 new-window\n",
        )
        .expect("first top-level source");
        fs::write(
            top.join("20-second.conf"),
            "set -g prefix C-z\nbind-key F3 kill-pane\n",
        )
        .expect("second top-level source");
        fs::write(nested.join("10-first.conf"), "bind-key F4 new-window\n")
            .expect("first nested source");
        fs::write(nested.join("20-second.conf"), "bind-key F5 kill-pane\n")
            .expect("second nested source");
        let entry = directory.path().join("entry.conf");
        fs::write(&entry, "source-file 'nested/*.conf'\n").expect("nested source entry");

        let shared = Arc::new(Shared::new(50));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "source-glob"]),
            )
            .expect("source-file session");
        take_reliable_messages(&mailbox);

        let top_pattern = top.join("*.conf").display().to_string();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("source-file", [&top_pattern]),
            )
            .expect("top-level source glob");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("source-file", [entry.display().to_string()]),
            )
            .expect("nested source glob");
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.engine.keys.prefix(), "C-z");
            for key in ["F2", "F3", "F4", "F5"] {
                assert!(inner.engine.keys.get("prefix", key).is_some(), "{key}");
            }
        }

        let unsupported_directory = directory.path().join("unsupported");
        fs::create_dir_all(&unsupported_directory).expect("unsupported source directory");
        fs::write(
            unsupported_directory.join("ignored.conf"),
            "bind-key F8 new-window\n",
        )
        .expect("unsupported nested source");
        let unsupported_entry = directory.path().join("unsupported-entry.conf");
        fs::write(
            &unsupported_entry,
            "source-file -v 'unsupported/*.conf'\nbind-key F7 new-window\n",
        )
        .expect("unsupported source entry");
        let mut report = ConfigLoadReport::default();
        shared
            .load_config_file_with_report(&unsupported_entry, &mut context, 0, &mut report)
            .expect("unsupported nested source is skipped");
        {
            let inner = shared.inner.lock();
            assert!(inner.engine.keys.get("prefix", "F7").is_some());
            assert!(inner.engine.keys.get("prefix", "F8").is_none());
        }
        assert!(
            report
                .summary()
                .is_some_and(|summary| summary.contains("source-file -v"))
        );

        take_reliable_messages(&mailbox);
        let missing = directory
            .path()
            .join("missing-*.conf")
            .display()
            .to_string();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("source-file", ["-q", &missing]),
            )
            .expect("quiet no-match source glob");
        assert!(!take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientMessage { .. },
                    ..
                })
            )
        }));

        let malformed = directory.path().join("[").display().to_string();
        let after_error = directory.path().join("after-error.conf");
        fs::write(&after_error, "bind-key F6 new-window\n").expect("post-error source");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new(
                    "source-file",
                    ["-q", &malformed, &after_error.display().to_string()],
                ),
            )
            .expect("glob error continues to later sources");
        assert!(
            shared
                .inner
                .lock()
                .engine
                .keys
                .get("prefix", "F6")
                .is_some()
        );
        assert!(take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientMessage {
                        kind: ClientMessageKind::Warning,
                        text,
                        ..
                    },
                    ..
                }) if text.contains("source-file glob error")
            )
        }));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("source-file", [&missing]),
            )
            .expect("loud no-match source glob");
        assert!(take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientMessage {
                        kind: ClientMessageKind::Warning,
                        text,
                        ..
                    },
                    ..
                }) if text.contains("no such file") && text.contains("missing-*.conf")
            )
        }));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("source-file", ["-q", "-"]),
            )
            .expect("standard-input refusal");
        assert!(take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientMessage {
                        kind: ClientMessageKind::Warning,
                        text,
                        ..
                    },
                    ..
                }) if text.contains("source-file from standard input is not supported")
            )
        }));
    }

    #[test]
    fn config_report_counts_invalid_and_unsupported_bound_commands() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mux_config = directory.path().join("mux.conf");
        fs::write(
            &mux_config,
            "bind F2 new-window\nbind x not-a-command\nbind r run-shell x\nbind F3 kill-pane\n",
        )
        .expect("mux config fixture");
        let defaults = KeyTables::default();
        let default_x = defaults.get("prefix", "x").cloned();
        let default_r = defaults.get("prefix", "r").cloned();

        let shared = Arc::new(Shared::new(52));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .reload_user_config_with_mux_file(client, &mut context, Some(&mux_config))
            .expect("reload mixed config");
        {
            let inner = shared.inner.lock();
            assert!(inner.engine.keys.get("prefix", "F2").is_some());
            assert!(inner.engine.keys.get("prefix", "F3").is_some());
            assert_eq!(inner.engine.keys.get("prefix", "x"), default_x.as_ref());
            assert_eq!(inner.engine.keys.get("prefix", "r"), default_r.as_ref());
        }
        assert!(take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientMessage {
                        kind: ClientMessageKind::Warning,
                        text,
                        ..
                    },
                    ..
                }) if text.contains("skipped 1 unsupported tmux command")
                    && text.contains("bind-key run-shell")
                    && text.contains("1 invalid line")
                    && text.contains("unknown command: not-a-command")
            )
        }));
    }

    #[test]
    fn invalid_mux_override_is_diagnostic_and_later_entries_still_apply() {
        let shared = Arc::new(Shared::new(46));
        let report = shared.apply_mux_config_overrides(
            &[
                ("mode-keys".to_owned(), "not-a-mode".to_owned()),
                ("prefix".to_owned(), "C-a".to_owned()),
            ],
            "test-invalid",
        );

        assert_eq!(report.applied, 1);
        assert_eq!(report.diagnostics.len(), 1);
        assert!(report.diagnostics[0].contains("invalid mode-keys value"));
        let inner = shared.inner.lock();
        assert_eq!(inner.engine.keys.prefix(), "C-a");
        assert_eq!(
            inner.engine.mux_option_value(MuxOptionKey::ModeKeys),
            "emacs"
        );
    }

    #[test]
    fn pushed_experimental_pane_override_unlocks_agent_materialization() {
        let shared = Arc::new(Shared::new(51));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(3),
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "gated"]),
            )
            .expect("session");
        shared
            .execute(
                ClientId(3),
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-picker", ["-v"]),
            )
            .expect("picker");
        let picker = context.pane.expect("picker");

        let gated = shared.execute(
            ClientId(3),
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("select-pane-kind", ["-t", &picker.to_string(), "agent"]),
        );
        assert!(matches!(
            gated,
            Err(DaemonError::Server(ServerError::InvalidCommand(message)))
                if message.contains("experimental-agent-pane")
        ));

        let report = shared.apply_mux_config_overrides(
            &[("experimental-agent-pane".to_owned(), "true".to_owned())],
            "test-experimental-unlock",
        );
        assert_eq!(report.applied, 1);
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);

        shared
            .execute(
                ClientId(3),
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane-kind", ["-t", &picker.to_string(), "agent"]),
            )
            .expect("agent materializes once the override lands");
        let inner = shared.inner.lock();
        assert!(matches!(
            inner.engine.state.pane(picker).map(|pane| &pane.kind),
            Some(PaneKind::Agent(_))
        ));
        assert_eq!(
            inner
                .mux_options
                .get(MuxOptionKey::ExperimentalAgentPane)
                .expect("option present")
                .value,
            "on"
        );
    }

    #[test]
    fn mux_option_source_tracks_default_tmux_override_and_runtime_writers() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tmux_config = directory.path().join("source-tiers.conf");
        fs::write(&tmux_config, "set -g prefix C-c\n").expect("tmux fixture");
        let shared = Arc::new(Shared::new(47));
        let mut context = ExecutionContext::default();

        assert_eq!(
            shared
                .inner
                .lock()
                .mux_options
                .get(MuxOptionKey::Prefix)
                .expect("default prefix")
                .source,
            MuxOptionSource::Default
        );
        shared
            .load_config_file(&tmux_config, &mut context, 0)
            .expect("tmux replay");
        assert_eq!(
            shared
                .inner
                .lock()
                .mux_options
                .get(MuxOptionKey::Prefix)
                .expect("tmux prefix")
                .source,
            MuxOptionSource::TmuxConfig
        );

        shared.apply_mux_config_overrides(
            &[("prefix".to_owned(), "C-a".to_owned())],
            "test-source-tier",
        );
        assert_eq!(
            shared
                .inner
                .lock()
                .mux_options
                .get(MuxOptionKey::Prefix)
                .expect("override prefix")
                .source,
            MuxOptionSource::Override
        );

        shared
            .execute(
                ClientId(9),
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "prefix", "C-z"]),
            )
            .expect("runtime prefix command");
        let inner = shared.inner.lock();
        let runtime = inner
            .mux_options
            .get(MuxOptionKey::Prefix)
            .expect("runtime prefix");
        assert_eq!(runtime.value, "C-z");
        assert_eq!(runtime.source, MuxOptionSource::RuntimeCommand);
    }

    #[test]
    fn reload_updates_live_actor_kinds_and_broadcasts_the_appearance() {
        let initial = TerminalAppearance {
            background: zz_terminal::Color::rgb(0x12, 0x34, 0x56),
            ..TerminalAppearance::default()
        };
        let shared = Arc::new(Shared::with_appearance(42, Arc::new(initial.clone())));
        let mailbox = OutboundMailbox::new();
        let (client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            Some(TerminalColorScheme::Light),
            Arc::clone(&mailbox),
        );
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "reload"]),
            )
            .expect("create terminal pane");
        let session = context.session.expect("session");
        let pane = context.pane.expect("pane");
        shared.attach(client, session).expect("attach");
        shared
            .open_command_output(client, Some(pane), "fixture".to_owned(), "preserved")
            .expect("command output");
        let (live, output) = {
            let inner = shared.inner.lock();
            (
                Arc::clone(&inner.terminals[&pane]),
                Arc::clone(&inner.command_outputs[&client].terminal),
            )
        };

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("reload-config", [] as [&str; 0]),
            )
            .expect("reload configuration");

        let expected = TerminalAppearance {
            color_scheme: TerminalColorScheme::Light,
            ..TerminalAppearance::default()
        };
        assert_eq!(*shared.inner.lock().appearance, expected);
        let deadline = Instant::now() + Duration::from_secs(30);
        while live.latest_viewport().background != expected.background
            || output.latest_viewport().background != expected.background
        {
            assert!(Instant::now() < deadline, "appearance did not reach actors");
            thread::sleep(Duration::from_millis(10));
        }

        let state = mailbox.state.lock();
        assert!(state.reliable.iter().any(|frame| {
            matches!(
                decode_protocol_frame(frame),
                Ok(ProtocolMessage::Event(Event {
                    payload: EventPayload::AppearanceChanged { appearance, .. },
                    ..
                })) if *appearance == expected
            )
        }));
    }

    #[test]
    fn system_color_scheme_change_applies_and_broadcasts_the_appearance() {
        let initial = TerminalAppearance {
            color_scheme: TerminalColorScheme::Dark,
            background: zz_terminal::Color::rgb(0x12, 0x34, 0x56),
            ..TerminalAppearance::default()
        };
        let shared = Arc::new(Shared::with_appearance(43, Arc::new(initial.clone())));
        let mailbox = OutboundMailbox::new();
        let (client, hello) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            Some(TerminalColorScheme::Dark),
            Arc::clone(&mailbox),
        );
        assert_eq!(hello.appearance, initial);

        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "scheme-change"]),
            )
            .expect("create terminal pane");
        let pane = context.pane.expect("pane");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        assert_eq!(terminal.latest_viewport().background, initial.background);

        shared.set_client_color_scheme(client, TerminalColorScheme::Light);

        let expected = TerminalAppearance {
            color_scheme: TerminalColorScheme::Light,
            ..TerminalAppearance::default()
        };
        assert_eq!(*shared.inner.lock().appearance, expected);
        let deadline = Instant::now() + Duration::from_secs(30);
        while terminal.latest_viewport().background != expected.background {
            assert!(Instant::now() < deadline, "appearance did not reach actor");
            thread::sleep(Duration::from_millis(10));
        }
        let state = mailbox.state.lock();
        assert!(state.reliable.iter().any(|frame| {
            matches!(
                decode_protocol_frame(frame),
                Ok(ProtocolMessage::Event(Event {
                    payload: EventPayload::AppearanceChanged { appearance, .. },
                    ..
                })) if *appearance == expected
            )
        }));
    }

    #[test]
    fn text_suppression_filter_borrows_the_normal_path_and_owns_changes() {
        let shared = Shared::new(1);
        let client = ClientId(99);
        let text = String::from("xλz");
        assert!(matches!(
            shared.filter_suppressed_text(client, &text),
            Cow::Borrowed("xλz")
        ));

        shared
            .inner
            .lock()
            .suppressed_text
            .entry(client)
            .or_default()
            .insert('x', 1);
        assert!(matches!(
            shared.filter_suppressed_text(client, &text),
            Cow::Owned(filtered) if filtered == "λz"
        ));
        assert!(!shared.inner.lock().suppressed_text.contains_key(&client));
    }

    #[test]
    fn text_that_fails_to_repay_a_binding_disarms_the_suppression() {
        let shared = Shared::new(1);
        let client = ClientId(98);
        shared
            .inner
            .lock()
            .suppressed_text
            .insert(client, BTreeMap::from([('c', 1)]));
        assert_eq!(shared.filter_suppressed_text(client, "ç").as_ref(), "ç");
        assert!(!shared.inner.lock().suppressed_text.contains_key(&client));
    }

    #[test]
    fn a_swallowed_chord_press_pairs_with_its_bare_release() {
        let shared = Shared::new(1);
        let client = ClientId(97);
        assert_eq!(
            shared.key_decision(client, "C-b", false),
            KeyDecision::Prefix
        );
        assert_eq!(shared.key_decision(client, "b", true), KeyDecision::Prefix);
        assert!(shared.inner.lock().swallowed_keys[&client].is_empty());
        assert_eq!(shared.key_decision(client, "b", true), KeyDecision::Pass);
    }

    #[test]
    fn capture_pane_parser_supports_history_mode_and_wrapped_output() {
        let args = ["-pJ", "-M", "-S", "-", "-E4", "-t%7", "-bcaptured"].map(str::to_owned);
        let parsed = parse_capture_pane_args(&args).expect("capture args");
        assert_eq!(parsed.target.as_deref(), Some("%7"));
        assert_eq!(parsed.buffer_name.as_deref(), Some("captured"));
        assert_eq!(parsed.options.start, CaptureBoundary::HistoryStart);
        assert_eq!(parsed.options.end, CaptureBoundary::Relative(4));
        assert!(parsed.options.mode);
        assert!(parsed.options.join_wrapped);
        assert!(parsed.options.preserve_trailing);
    }

    #[test]
    fn display_message_is_published_only_to_the_requesting_client() {
        let shared = Arc::new(Shared::new(1));
        let requesting_mailbox = OutboundMailbox::new();
        let observing_mailbox = OutboundMailbox::new();
        let (requesting, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&requesting_mailbox),
        );
        let (_observing, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&observing_mailbox),
        );
        take_reliable_messages(&requesting_mailbox);
        take_reliable_messages(&observing_mailbox);

        shared
            .execute(
                requesting,
                ClientKind::Interactive,
                &mut ExecutionContext::default(),
                &CommandInvocation::new("display-message", ["hello", "client"]),
            )
            .expect("display message");

        assert!(
            take_reliable_messages(&requesting_mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::TimedClientMessage {
                            pane: None,
                            kind: ClientMessageKind::Info,
                            text,
                            duration_ms: 750,
                        },
                        ..
                    }) if text == "hello client"
                ))
        );
        assert!(
            take_reliable_messages(&observing_mailbox)
                .iter()
                .all(|message| !matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::TimedClientMessage { .. },
                        ..
                    })
                ))
        );
    }

    #[test]
    fn agent_send_parser_reads_target_context_and_trailing_text() {
        let args = [
            "-t%4",
            "--submit",
            "--context",
            "src/lib.rs:10-42",
            "--",
            "--- a/src/lib.rs",
        ]
        .map(str::to_owned);
        let parsed = parse_agent_send_args(&args).expect("agent-send args");
        assert_eq!(parsed.target.as_deref(), Some("%4"));
        assert!(parsed.submit);
        assert_eq!(
            parsed.context,
            Some(ContextReference {
                path: "src/lib.rs".to_owned(),
                start: Some(10),
                end: Some(42),
            })
        );
        assert_eq!(parsed.text, ["--- a/src/lib.rs"]);

        let attached = ["--context=notes.md", "-t", "%9", "hello", "world"].map(str::to_owned);
        let parsed = parse_agent_send_args(&attached).expect("attached options");
        assert_eq!(parsed.target.as_deref(), Some("%9"));
        assert!(!parsed.submit);
        assert_eq!(
            parsed
                .context
                .as_ref()
                .map(ContextReference::header)
                .as_deref(),
            Some("notes.md")
        );
        assert_eq!(
            parsed.payload().expect("payload"),
            "notes.md\n```\nhello world\n```"
        );
    }

    #[test]
    fn agent_send_reads_stdin_only_when_argv_carries_no_text() {
        assert!(agent_send_reads_stdin(&["-t".to_owned(), "%1".to_owned()]));
        assert!(agent_send_reads_stdin(&[
            "-t%1".to_owned(),
            "--submit".to_owned()
        ]));
        assert!(!agent_send_reads_stdin(&[
            "-t%1".to_owned(),
            "look at this".to_owned()
        ]));
        assert!(!agent_send_reads_stdin(&[
            "-t%1".to_owned(),
            "--".to_owned(),
            "-piped-".to_owned()
        ]));
        assert!(!agent_send_reads_stdin(&["-Z".to_owned()]));
    }

    #[test]
    fn agent_send_payload_enforces_bounds_and_wraps_context() {
        let empty = ParsedAgentSend::default();
        assert!(matches!(
            empty.payload(),
            Err(ServerError::InvalidCommand(_))
        ));

        let control = ParsedAgentSend {
            text: vec!["before\u{7}after".to_owned()],
            ..ParsedAgentSend::default()
        };
        assert!(matches!(
            control.payload(),
            Err(ServerError::InvalidCommand(_))
        ));

        let oversized = ParsedAgentSend {
            text: vec!["x".repeat(MAX_AGENT_SEND_BYTES + 1)],
            ..ParsedAgentSend::default()
        };
        assert!(matches!(
            oversized.payload(),
            Err(ServerError::InvalidCommand(_))
        ));

        let fenced = ParsedAgentSend {
            context: Some(ContextReference {
                path: "src/lib.rs".to_owned(),
                start: Some(7),
                end: None,
            }),
            text: vec!["```rust\nfn main() {}\n```".to_owned()],
            ..ParsedAgentSend::default()
        };
        assert_eq!(
            fenced.payload().expect("fenced payload"),
            "src/lib.rs:7\n````\n```rust\nfn main() {}\n```\n````"
        );
    }

    #[test]
    fn context_references_keep_colons_that_are_not_line_ranges() {
        assert_eq!(
            ContextReference::parse("C:\\src\\lib.rs").expect("windows path"),
            ContextReference {
                path: "C:\\src\\lib.rs".to_owned(),
                start: None,
                end: None,
            }
        );
        assert_eq!(
            ContextReference::parse("notes.md:12").expect("single line"),
            ContextReference {
                path: "notes.md".to_owned(),
                start: Some(12),
                end: None,
            }
        );
        assert!(ContextReference::parse("notes.md:40-12").is_err());
        assert!(ContextReference::parse("").is_err());
        assert!(ContextReference::parse(&"x".repeat(MAX_CONTEXT_PATH_BYTES + 1)).is_err());
    }

    #[test]
    fn capture_browser_and_send_last_output_parsers_reject_stray_positionals() {
        let args = ["-t", "%2", "-o", "/tmp/frame.png"].map(str::to_owned);
        let parsed = parse_capture_browser_args(&args).expect("capture-browser args");
        assert_eq!(parsed.target.as_deref(), Some("%2"));
        assert_eq!(parsed.output.as_deref(), Some("/tmp/frame.png"));
        assert!(parse_capture_browser_args(&["frame.png".to_owned()]).is_err());
        assert!(
            parse_capture_browser_args(&["-o".to_owned()]).is_err(),
            "-o without a value is rejected"
        );

        assert_eq!(
            parse_target_only_args("send-last-output", &["-t%5".to_owned()]).expect("target"),
            Some("%5".to_owned())
        );
        assert!(parse_target_only_args("send-last-output", &["%5".to_owned()]).is_err());
    }

    #[test]
    fn tools_catalog_matches_dispatchable_verbs() {
        const TOOL_VERBS: [&str; 6] = [
            "capture-pane",
            "agent-send",
            "send-last-output",
            "capture-browser",
            "debug-marker",
            "tools",
        ];
        let catalog = workspace_tools_catalog().output;
        let mut documented = BTreeSet::new();
        for line in catalog.lines().filter(|line| line.starts_with("  zz ")) {
            let tokens = line.split_whitespace().collect::<Vec<_>>();
            for pair in tokens.windows(2) {
                if pair[0] == "zz" {
                    documented.insert(pair[1]);
                }
            }
        }
        for verb in TOOL_VERBS {
            assert!(documented.contains(verb), "catalog is missing `zz {verb}`");
        }
        for verb in &documented {
            assert!(
                TOOL_VERBS.contains(verb) || zz_mux::command_spec(verb).is_some(),
                "catalog documents `zz {verb}`, which no command implements"
            );
        }
        assert!(catalog.contains(crate::transport::SOCKET_ENVIRONMENT_VARIABLE));
    }

    #[test]
    fn daemon_command_catalog_matches_preemption_dispatch() {
        let names = zz_mux::CommandSpec::DAEMON_COMMAND_NAMES
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let dispatches = DAEMON_COMMAND_DISPATCHES
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>();
        assert_eq!(zz_mux::CommandSpec::DAEMON_COMMAND_NAMES.len(), names.len());
        assert_eq!(DAEMON_COMMAND_DISPATCHES.len(), dispatches.len());
        assert_eq!(names, dispatches);
        for name in ["new-session", "capture-pane-extra"] {
            assert!(daemon_command_dispatch(name).is_none());
        }
    }

    #[test]
    fn agent_send_round_trips_through_the_attached_gui() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "agent-send"]),
            )
            .expect("session");
        let session = context.session.expect("session");
        let terminal = context.pane.expect("terminal");

        let error = shared
            .execute(
                client,
                ClientKind::Command,
                &mut context.clone(),
                &CommandInvocation::new("agent-send", ["-t", &terminal.to_string(), "hi"]),
            )
            .expect_err("no recipient");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::MissingTarget(message))
                if message.contains("no agent pane")
        ));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-picker", ["-v"]),
            )
            .expect("picker");
        let agent = context.pane.expect("picker");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "experimental-agent-pane", "on"]),
            )
            .expect("enable agent panes");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane-kind", ["-t", &agent.to_string(), "agent"]),
            )
            .expect("agent pane");
        shared.attach(client, session).expect("attach");
        take_reliable_messages(&mailbox);

        let sender = Arc::clone(&shared);
        let target = terminal.to_string();
        let caller = thread::spawn(move || {
            let mut context = ExecutionContext::default();
            sender.execute(
                ClientId(99),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("agent-send", ["-t", &target, "review", "this"]),
            )
        });

        let deadline = Instant::now() + Duration::from_secs(30);
        let request_id = loop {
            let found = take_reliable_messages(&mailbox)
                .into_iter()
                .find_map(|message| match message {
                    ProtocolMessage::Event(Event {
                        payload:
                            EventPayload::AgentCommand {
                                pane,
                                request_id,
                                command: AgentCommand::ComposerAppend { text },
                            },
                        ..
                    }) if pane == agent && text == "review this" => Some(request_id),
                    _ => None,
                });
            if let Some(request_id) = found {
                break request_id;
            }
            assert!(Instant::now() < deadline, "no agent command was published");
            thread::sleep(Duration::from_millis(10));
        };
        shared.complete_gui_request(
            client,
            GuiResponse::Success {
                request_id,
                output: "appended".to_owned(),
            },
        );
        let execution = caller.join().expect("caller thread").expect("agent-send");
        assert_eq!(execution.output, "appended");
        assert!(shared.inner.lock().pending_gui_requests.is_empty());
    }

    #[test]
    fn gui_requests_fail_when_the_window_disconnects() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "disconnect"]),
            )
            .expect("session");
        let session = context.session.expect("session");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-picker", ["-v"]),
            )
            .expect("picker");
        let agent = context.pane.expect("picker");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "experimental-agent-pane", "on"]),
            )
            .expect("enable agent panes");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane-kind", ["-t", &agent.to_string(), "agent"]),
            )
            .expect("agent pane");
        shared.attach(client, session).expect("attach");

        let sender = Arc::clone(&shared);
        let target = agent.to_string();
        let caller = thread::spawn(move || {
            let mut context = ExecutionContext::default();
            sender.execute(
                ClientId(98),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("agent-send", ["-t", &target, "anything"]),
            )
        });

        let deadline = Instant::now() + Duration::from_secs(30);
        while shared.inner.lock().pending_gui_requests.is_empty() {
            assert!(Instant::now() < deadline, "request was never registered");
            thread::sleep(Duration::from_millis(10));
        }
        shared.fail_gui_requests_for(client);
        let error = caller
            .join()
            .expect("caller thread")
            .expect_err("disconnected window");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message)) if message.contains("disconnected")
        ));
    }

    #[test]
    fn send_last_output_requires_a_terminal_and_an_agent_pane() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "explain"]),
            )
            .expect("session");
        let session = context.session.expect("session");
        let terminal = context.pane.expect("terminal");
        shared.attach(client, session).expect("attach");

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-picker", ["-v"]),
            )
            .expect("picker");
        let picker = context.pane.expect("picker");
        let error = shared
            .execute(
                client,
                ClientKind::Command,
                &mut context.clone(),
                &CommandInvocation::new("send-last-output", ["-t", &picker.to_string()]),
            )
            .expect_err("picker is not a terminal");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidTarget(_))
        ));

        let error = shared
            .execute(
                client,
                ClientKind::Command,
                &mut context.clone(),
                &CommandInvocation::new("send-last-output", ["-t", &terminal.to_string()]),
            )
            .expect_err("no marks");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message.contains("OSC 133")
        ));
    }

    #[test]
    fn capture_browser_rejects_non_browser_panes_and_relative_paths() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "shots"]),
            )
            .expect("session");
        let session = context.session.expect("session");
        let terminal = context.pane.expect("terminal");
        shared.attach(client, session).expect("attach");
        let output = std::env::temp_dir()
            .join("zz-shot.png")
            .to_string_lossy()
            .into_owned();

        let error = shared
            .execute(
                client,
                ClientKind::Command,
                &mut context.clone(),
                &CommandInvocation::new(
                    "capture-browser",
                    ["-t", &terminal.to_string(), "-o", &output],
                ),
            )
            .expect_err("terminal target");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidTarget(_))
        ));

        let error = shared
            .execute(
                client,
                ClientKind::Command,
                &mut context.clone(),
                &CommandInvocation::new(
                    "capture-browser",
                    ["-t", &terminal.to_string(), "-o", "relative.png"],
                ),
            )
            .expect_err("relative path");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message.contains("absolute")
        ));
    }

    #[test]
    fn capture_pane_parser_rejects_unimplemented_flags() {
        let error = parse_capture_pane_args(&["-C".to_owned()]).expect_err("unsupported flag");
        assert!(matches!(error, ServerError::UnsupportedCommand(_)));
    }

    #[test]
    fn buffer_argument_parser_handles_compact_options_and_explicit_boundaries() {
        let args = ["-dprS", "-bbinary", "-s::", "--", "-literal"].map(str::to_owned);
        let parsed = parse_buffer_command_args(
            "paste-buffer",
            &args,
            &['b', 's', 't'],
            &['d', 'p', 'r', 'S'],
        )
        .expect("buffer arguments");
        assert!(parsed.has('d'));
        assert!(parsed.has('p'));
        assert!(parsed.has('r'));
        assert!(parsed.has('S'));
        assert_eq!(parsed.value('b'), Some("binary"));
        assert_eq!(parsed.value('s'), Some("::"));
        assert_eq!(parsed.positional, ["-literal"]);

        let error =
            parse_buffer_command_args("paste-buffer", &["-x".to_owned()], &['b', 't'], &['d', 'p'])
                .expect_err("unsupported option");
        assert!(matches!(
            error,
            ServerError::UnsupportedCommand(command) if command == "paste-buffer -x"
        ));
    }

    #[test]
    fn load_and_save_buffers_preserve_binary_bytes_and_append() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let input = directory.path().join("input.bin");
        let output = directory.path().join("output.bin");
        let bytes = b"head\0middle\n\xfftail";
        fs::write(&input, bytes).expect("binary fixture");

        let shared = Shared::new(1);
        let context = ExecutionContext::default();
        shared
            .buffer_command(
                &context,
                "load-buffer",
                &["-bbinary".to_owned(), input.to_string_lossy().into_owned()],
            )
            .expect("load binary buffer");
        {
            let inner = shared.inner.lock();
            let buffer = resolve_buffer(&inner, Some("binary")).expect("named buffer");
            assert_eq!(buffer.data.as_ref(), bytes);
            assert!(!buffer.automatic);
        }

        let error = shared
            .buffer_command(
                &context,
                "show-buffer",
                &["-b", "binary"].map(str::to_owned),
            )
            .expect_err("binary output cannot cross the text command response");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message.contains("non-UTF-8") && message.contains("save-buffer")
        ));

        shared
            .buffer_command(
                &context,
                "save-buffer",
                &["-b", "binary", output.to_string_lossy().as_ref()].map(str::to_owned),
            )
            .expect("save binary buffer");
        assert_eq!(fs::read(&output).expect("saved bytes"), bytes);

        fs::write(&output, b"prefix:").expect("append prefix");
        shared
            .buffer_command(
                &context,
                "save-buffer",
                &["-a", "-bbinary", output.to_string_lossy().as_ref()].map(str::to_owned),
            )
            .expect("append binary buffer");
        let mut expected = b"prefix:".to_vec();
        expected.extend_from_slice(bytes);
        assert_eq!(fs::read(&output).expect("appended bytes"), expected);
    }

    #[test]
    fn set_buffer_append_updates_named_buffers_but_creates_automatic_buffers_without_a_name() {
        let shared = Shared::new(1);
        let context = ExecutionContext::default();
        for arguments in [
            vec!["-b", "named", "alpha"],
            vec!["automatic"],
            vec!["-abnamed", "--", "-omega"],
            vec!["-a", "new automatic"],
        ] {
            let arguments = arguments.into_iter().map(str::to_owned).collect::<Vec<_>>();
            shared
                .buffer_command(&context, "set-buffer", &arguments)
                .expect("set buffer");
        }

        let inner = shared.inner.lock();
        assert_eq!(inner.paste_buffers.len(), 3);
        assert_eq!(inner.paste_buffers[0].data.as_ref(), b"new automatic");
        assert!(inner.paste_buffers[0].automatic);
        let named = resolve_buffer(&inner, Some("named")).expect("named buffer");
        assert_eq!(named.data.as_ref(), b"alpha-omega");
        assert!(!named.automatic);
    }

    #[test]
    fn buffer_formats_read_named_rows_and_the_top_automatic_buffer() {
        let shared = Arc::new(Shared::new(1));
        let mut context = {
            let mut inner = shared.inner.lock();
            let (_, _, pane) = inner
                .engine
                .state
                .create_session("w")
                .expect("format target session");
            ExecutionContext::for_pane(&inner.engine.state, pane).expect("format target pane")
        };
        shared
            .buffer_command(
                &context,
                "set-buffer",
                &["-b", "named", "alpha"].map(str::to_owned),
            )
            .expect("set named buffer");
        let named = shared
            .buffer_command(
                &context,
                "list-buffers",
                &[
                    "-F".to_owned(),
                    "#{buffer_name}|#{buffer_size}|#{buffer_sample}|#{!!:#{buffer_created}}"
                        .to_owned(),
                ],
            )
            .expect("format named buffer");
        assert_eq!(named.output, "named|5|alpha|1");

        shared
            .buffer_command(&context, "set-buffer", &["bravo".to_owned()])
            .expect("set automatic buffer");
        let displayed = shared
            .execute(
                ClientId(1),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "display-message",
                    [
                        "-p",
                        "#{buffer_name}|#{buffer_size}|#{buffer_sample}|#{!!:#{buffer_created}}",
                    ],
                ),
            )
            .expect("format automatic buffer");
        assert_eq!(displayed.output, "buffer0|5|bravo|1");

        let pane = context.pane.expect("format target pane").to_string();
        let targeted = shared
            .execute(
                ClientId(1),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "display-message",
                    [
                        "-p",
                        "-t",
                        pane.as_str(),
                        "#{buffer_name}|#{buffer_size}|#{buffer_sample}|#{!!:#{buffer_created}}",
                    ],
                ),
            )
            .expect("format automatic buffer with a pane target");
        assert_eq!(targeted.output, "buffer0|5|bravo|1");
    }

    #[test]
    fn unnamed_buffer_commands_target_the_newest_automatic_buffer() {
        let shared = Shared::new(1);
        let context = ExecutionContext::default();
        for arguments in [
            vec!["-b", "named-old", "named old"],
            vec!["automatic"],
            vec!["-b", "named-new", "named new"],
        ] {
            shared
                .buffer_command(
                    &context,
                    "set-buffer",
                    &arguments.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                )
                .expect("set buffer");
        }

        let shown = shared
            .buffer_command(&context, "show-buffer", &[])
            .expect("show automatic buffer");
        assert_eq!(shown.output, "automatic");
        shared
            .buffer_command(&context, "delete-buffer", &[])
            .expect("delete automatic buffer");
        let inner = shared.inner.lock();
        assert_eq!(inner.paste_buffers.len(), 2);
        assert!(inner.paste_buffers.iter().all(|buffer| !buffer.automatic));
        assert_eq!(inner.paste_buffers[0].name, "named-new");
        drop(inner);
        assert!(matches!(
            shared.buffer_command(&context, "show-buffer", &[]),
            Err(DaemonError::Server(ServerError::MissingTarget(target)))
                if target == "paste buffer"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn paste_buffer_reaches_a_detached_terminal_actor() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "detached"]),
            )
            .expect("detached session");
        let pane = context.pane.expect("terminal pane");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-buffer", ["printf 'ZZ_DETACHED_BUFFER_OK\\n'\n"]),
            )
            .expect("shell buffer");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("paste-buffer", ["-t", &pane.to_string()]),
            )
            .expect("paste without an interactive attachment");

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let captured = shared
                .execute(
                    ClientId(7),
                    ClientKind::Command,
                    &mut context,
                    &CommandInvocation::new("capture-pane", ["-p", "-t", &pane.to_string()]),
                )
                .expect("capture detached terminal")
                .output;
            if captured.contains("ZZ_DETACHED_BUFFER_OK") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "detached terminal did not receive paste-buffer"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let buffered = shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "capture-pane",
                    ["-p", "-b", "detached-capture", "-t", &pane.to_string()],
                ),
            )
            .expect("capture detached terminal into a named buffer");
        assert!(buffered.output.is_empty(), "-b wins over -p");
        let shown = shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("show-buffer", ["-b", "detached-capture"]),
            )
            .expect("show captured named buffer")
            .output;
        assert!(shown.contains("ZZ_DETACHED_BUFFER_OK"));
    }

    #[cfg(unix)]
    #[test]
    fn seeded_global_environment_and_session_markers_reach_terminal_spawn() {
        let shared = Arc::new(Shared::new(1));
        shared.inner.lock().engine.seed_global_environment([
            ("PHASE4D_SEEDED", "daemon"),
            ("HIDDENPROBE", "daemonval"),
            ("TERM", "inherited-term"),
        ]);
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-environment",
                    ["-g", "-h", "HIDDENPROBE", "newhidden"],
                ),
            )
            .expect("hidden global environment");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "seeded-environment"]),
            )
            .expect("session");
        let pane = context.pane.expect("terminal pane");
        assert_eq!(
            shared.inner.lock().terminal_spawns[&pane].terminal_type,
            Some("tmux-256color".to_owned())
        );
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-buffer",
                    [
                        "printf 'PHASE4D_SEEDED=[%s] DISPLAY=[%s] HP=[%s] TERM=[%s]\\n' \"$PHASE4D_SEEDED\" \"${DISPLAY-unset}\" \"${HIDDENPROBE-}\" \"$TERM\"\n",
                    ],
                ),
            )
            .expect("shell probe");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("paste-buffer", ["-t", &pane.to_string()]),
            )
            .expect("paste shell probe");

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let captured = shared
                .execute(
                    ClientId(7),
                    ClientKind::Command,
                    &mut context,
                    &CommandInvocation::new("capture-pane", ["-p", "-t", &pane.to_string()]),
                )
                .expect("capture terminal")
                .output;
            if captured
                .contains("PHASE4D_SEEDED=[daemon] DISPLAY=[unset] HP=[] TERM=[tmux-256color]")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "seeded environment did not reach terminal spawn"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn paste_upload_fixture(
        directory: &Path,
        name: &str,
    ) -> (Arc<Shared>, Arc<OutboundMailbox>, ClientId, PaneId) {
        let mut shared = Shared::new(1);
        shared.paste_directory = directory.to_path_buf();
        let shared = Arc::new(shared);
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", name]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let pane = context.pane.expect("pane");
        shared.attach(client, session).expect("attach session");
        (shared, mailbox, client, pane)
    }

    fn client_message_texts(messages: Vec<ProtocolMessage>) -> Vec<String> {
        messages
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::ClientMessage {
                            kind: ClientMessageKind::Error,
                            text,
                            ..
                        },
                    ..
                }) => Some(text),
                _ => None,
            })
            .collect()
    }

    fn wait_for_pasted_path(shared: &Arc<Shared>, client: ClientId, pane: PaneId, file_name: &str) {
        let mut context = ExecutionContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let captured = shared
                .execute(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    &CommandInvocation::new("capture-pane", ["-p", "-t", &pane.to_string()]),
                )
                .expect("capture pane")
                .output;
            if captured.contains(file_name) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the uploaded image path never reached the pane; captured {captured:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn paste_path_image_upload_writes_a_private_file_stages_preview_and_pastes_the_path() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().expect("temporary directory");
        let (shared, mailbox, client, pane) =
            paste_upload_fixture(&directory.path().join("paste"), "paste-upload");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        terminal.resize(200, 10, 8, 18);
        wait_for_terminal_dimensions(&terminal, TerminalViewId(client.0), 200, 10);
        take_reliable_messages(&mailbox);

        let image = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a];
        shared.begin_paste_upload(
            client,
            ClientKind::Interactive,
            1,
            pane,
            PasteUploadPurpose::PastePath,
            "png".to_owned(),
            6,
        );
        shared.extend_paste_upload(client, 1, &image[..3]);
        assert!(
            shared.inner.lock().paste_uploads.contains_key(&(client, 1)),
            "a partial upload should stay open"
        );
        shared.extend_paste_upload(client, 1, &image[3..]);
        assert!(
            shared.inner.lock().paste_uploads.is_empty(),
            "reaching the declared total should finish the upload"
        );

        let file_name = format!("paste-{client}-1.png");
        let written = shared.paste_directory.join(&file_name);
        assert_eq!(fs::read(&written).expect("uploaded image"), image);
        assert_eq!(
            fs::metadata(&written)
                .expect("upload metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600,
        );
        assert_eq!(
            fs::metadata(&shared.paste_directory)
                .expect("upload directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700,
        );
        {
            let pasted_images = shared.pasted_images.lock();
            let stored = pasted_images.get(&pane).expect("pane image state");
            assert_eq!(stored.pending.len(), 1);
            assert_eq!(stored.pending_bytes, image.len());
            assert_eq!(stored.pending[0].format, PastedImageFormat::Png);
            assert_eq!(stored.pending[0].bytes.as_ref(), image);
            assert!(stored.images.is_empty());
        }
        assert!(
            client_message_texts(take_reliable_messages(&mailbox)).is_empty(),
            "a completed upload should not report an error"
        );

        wait_for_pasted_path(&shared, client, pane, &file_name);
    }

    #[test]
    fn paste_path_non_image_upload_pastes_without_staging_or_rejection() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (shared, mailbox, client, pane) =
            paste_upload_fixture(&directory.path().join("paste"), "paste-upload-non-image");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        terminal.resize(200, 10, 8, 18);
        wait_for_terminal_dimensions(&terminal, TerminalViewId(client.0), 200, 10);
        take_reliable_messages(&mailbox);

        let bytes = b"remote path payload";
        shared.begin_paste_upload(
            client,
            ClientKind::Interactive,
            2,
            pane,
            PasteUploadPurpose::PastePath,
            "txt".to_owned(),
            u32::try_from(bytes.len()).expect("payload length"),
        );
        shared.extend_paste_upload(client, 2, bytes);

        let file_name = format!("paste-{client}-2.txt");
        assert_eq!(
            fs::read(shared.paste_directory.join(&file_name)).expect("uploaded file"),
            bytes
        );
        assert!(!shared.pasted_images.lock().contains_key(&pane));
        assert!(client_message_texts(take_reliable_messages(&mailbox)).is_empty());

        wait_for_pasted_path(&shared, client, pane, &file_name);
    }

    #[test]
    fn record_only_paste_upload_stays_encoded_and_opens_a_pending_binding_window() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (shared, mailbox, client, pane) =
            paste_upload_fixture(&directory.path().join("paste"), "paste-record-only");
        take_reliable_messages(&mailbox);

        let image = [0x52, 0x49, 0x46, 0x46, 0x57, 0x45, 0x42, 0x50];
        shared.begin_paste_upload(
            client,
            ClientKind::Interactive,
            71,
            pane,
            PasteUploadPurpose::RecordPastedImage,
            "webp".to_owned(),
            u32::try_from(image.len()).expect("image length"),
        );
        shared.extend_paste_upload(client, 71, &image[..3]);
        shared.extend_paste_upload(client, 71, &image[3..]);

        assert!(shared.inner.lock().paste_uploads.is_empty());
        let pasted_images = shared.pasted_images.lock();
        let stored = pasted_images.get(&pane).expect("pane image state");
        assert_eq!(stored.pending.len(), 1);
        assert_eq!(stored.pending_bytes, image.len());
        assert_eq!(stored.pending[0].format, PastedImageFormat::Webp);
        assert_eq!(stored.pending[0].bytes.as_ref(), image);
        assert!(stored.images.is_empty());
        assert!(!shared.paste_directory.exists());
        assert!(client_message_texts(take_reliable_messages(&mailbox)).is_empty());
    }

    #[test]
    fn paste_uploads_are_bounded_per_client_and_dropped_on_overflow() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (shared, mailbox, client, pane) =
            paste_upload_fixture(&directory.path().join("paste"), "paste-upload-bounds");
        take_reliable_messages(&mailbox);

        shared.extend_paste_upload(client, 404, &[0; 8]);
        assert!(shared.inner.lock().paste_uploads.is_empty());
        assert!(client_message_texts(take_reliable_messages(&mailbox)).is_empty());

        shared.begin_paste_upload(
            client,
            ClientKind::Interactive,
            1,
            pane,
            PasteUploadPurpose::PastePath,
            "png".to_owned(),
            4,
        );
        shared.extend_paste_upload(client, 1, &[0; 5]);
        assert!(
            shared.inner.lock().paste_uploads.is_empty(),
            "an overflowing upload should be dropped"
        );
        assert_eq!(
            client_message_texts(take_reliable_messages(&mailbox)).len(),
            1
        );
        assert!(
            !shared
                .paste_directory
                .join(format!("paste-{client}-1.png"))
                .exists()
        );

        for upload_id in 1..=MAX_CONCURRENT_PASTE_UPLOADS as u64 {
            shared.begin_paste_upload(
                client,
                ClientKind::Interactive,
                upload_id,
                pane,
                PasteUploadPurpose::PastePath,
                "png".to_owned(),
                64,
            );
        }
        shared.begin_paste_upload(
            client,
            ClientKind::Interactive,
            1,
            pane,
            PasteUploadPurpose::PastePath,
            "png".to_owned(),
            32,
        );
        assert!(client_message_texts(take_reliable_messages(&mailbox)).is_empty());
        shared.begin_paste_upload(
            client,
            ClientKind::Interactive,
            9,
            pane,
            PasteUploadPurpose::PastePath,
            "png".to_owned(),
            64,
        );
        assert_eq!(
            shared.inner.lock().paste_uploads.len(),
            MAX_CONCURRENT_PASTE_UPLOADS
        );
        assert_eq!(
            client_message_texts(take_reliable_messages(&mailbox)).len(),
            1
        );

        shared.begin_paste_upload(
            client,
            ClientKind::Interactive,
            10,
            PaneId(u64::MAX),
            PasteUploadPurpose::PastePath,
            "png".to_owned(),
            64,
        );
        assert_eq!(
            client_message_texts(take_reliable_messages(&mailbox)).len(),
            1
        );

        shared.unregister(client);
        assert!(
            shared.inner.lock().paste_uploads.is_empty(),
            "a disconnect should take its client's uploads with it"
        );
    }

    #[test]
    fn paste_upload_retention_keeps_only_the_newest_files() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paste = directory.path().join("paste");
        for index in 0..PASTE_UPLOAD_RETENTION + 4 {
            thread::sleep(Duration::from_millis(10));
            write_paste_upload(&paste, &format!("paste-c1-{index}.png"), b"upload")
                .expect("write upload");
            prune_paste_uploads(&paste, PASTE_UPLOAD_RETENTION);
        }

        let remaining = fs::read_dir(&paste)
            .expect("upload directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(remaining.len(), PASTE_UPLOAD_RETENTION);
        assert!(remaining.contains(&format!("paste-c1-{}.png", PASTE_UPLOAD_RETENTION + 3)));
        assert!(!remaining.contains("paste-c1-0.png"));
    }

    #[test]
    fn buffer_file_commands_bound_input_and_reject_unsupported_stream_modes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let empty = directory.path().join("empty");
        let oversized = directory.path().join("oversized");
        fs::write(&empty, []).expect("empty fixture");
        fs::File::create(&oversized)
            .expect("oversized fixture")
            .set_len(u64::try_from(MAX_PASTE_BUFFER_BYTES).unwrap() + 1)
            .expect("sparse oversized fixture");

        let shared = Shared::new(1);
        let context = ExecutionContext::default();
        shared
            .buffer_command(
                &context,
                "load-buffer",
                &[empty.to_string_lossy().into_owned()],
            )
            .expect("empty load is a no-op");
        assert!(shared.inner.lock().paste_buffers.is_empty());

        let error = shared
            .buffer_command(
                &context,
                "load-buffer",
                &[oversized.to_string_lossy().into_owned()],
            )
            .expect_err("oversized buffer");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message.contains("exceeds")
        ));
        assert!(shared.inner.lock().paste_buffers.is_empty());

        for (command, arguments, unsupported) in [
            ("load-buffer", vec!["-w", "fixture"], "load-buffer -w"),
            ("save-buffer", vec!["-"], "save-buffer to standard output"),
            ("paste-buffer", vec!["-x"], "paste-buffer -x"),
        ] {
            let error = shared
                .buffer_command(
                    &context,
                    command,
                    &arguments.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                )
                .expect_err("unsupported buffer mode");
            assert!(matches!(
                error,
                DaemonError::Server(ServerError::UnsupportedCommand(message))
                    if message == unsupported
            ));
        }
    }

    #[test]
    fn named_buffers_replace_in_place_and_automatic_eviction_preserves_them() {
        let mut inner = ServerState::default();
        insert_paste_buffer(&mut inner, Some("named"), "buffer", b"alpha".to_vec())
            .expect("named buffer");
        insert_paste_buffer(&mut inner, None, "buffer", b"automatic".to_vec())
            .expect("automatic buffer");
        insert_paste_buffer(&mut inner, Some("named"), "buffer", b"replacement".to_vec())
            .expect("replace named buffer");
        assert_eq!(inner.paste_buffers[0].name, "named");
        assert_eq!(inner.paste_buffers[0].data.as_ref(), b"replacement");
        assert!(!inner.paste_buffers[0].automatic);
        assert_eq!(
            inner
                .paste_buffers
                .iter()
                .filter(|buffer| buffer.name == "named")
                .count(),
            1
        );

        for index in 0..=DEFAULT_BUFFER_LIMIT {
            insert_paste_buffer(
                &mut inner,
                None,
                "buffer",
                vec![u8::try_from(index).unwrap_or(u8::MAX)],
            )
            .expect("bounded automatic buffer");
        }
        assert_eq!(
            inner
                .paste_buffers
                .iter()
                .filter(|buffer| buffer.automatic)
                .count(),
            DEFAULT_BUFFER_LIMIT
        );
        assert!(inner.paste_buffers.iter().any(|buffer| {
            buffer.name == "named" && buffer.data.as_ref() == b"replacement" && !buffer.automatic
        }));
    }

    #[test]
    fn configured_buffer_limit_controls_daemon_automatic_eviction() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "buffer-limit", "2"]),
            )
            .expect("configure buffer limit");
        assert_eq!(shared.inner.lock().automatic_paste_buffer_limit.0, 2);

        for data in ["zero", "one", "two"] {
            shared
                .buffer_command(&context, "set-buffer", &[data.to_owned()])
                .expect("automatic buffer");
        }
        let inner = shared.inner.lock();
        assert_eq!(
            inner
                .paste_buffers
                .iter()
                .filter(|buffer| buffer.automatic)
                .map(|buffer| buffer.name.as_str())
                .collect::<Vec<_>>(),
            ["buffer2", "buffer1"]
        );
    }

    #[test]
    fn history_limit_is_captured_when_each_terminal_actor_is_created() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "history-limit", "7"]),
            )
            .expect("global history limit");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "history"]),
            )
            .expect("session");
        let session = context.session.expect("session id");
        let first = context.pane.expect("first pane");
        assert_eq!(shared.inner.lock().terminals[&first].max_scrollback(), 7);

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-option",
                    ["-t", &session.to_string(), "history-limit", "2"],
                ),
            )
            .expect("session history limit");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("second terminal");
        let second = context.pane.expect("second pane");
        let inner = shared.inner.lock();
        assert_eq!(inner.terminals[&first].max_scrollback(), 7);
        assert_eq!(inner.terminals[&second].max_scrollback(), 2);
    }

    #[test]
    fn configured_split_window_binding_creates_a_terminal_without_rewriting_the_command() {
        let shared = Arc::new(Shared::new(1));
        let client = ClientId(7);
        let mut context = ExecutionContext::default();
        let config = parse_config(
            "picker.conf",
            r##"set -g prefix C-a
bind | split-window -h -c "#{pane_current_path}"
bind - split-window -v -c "#{pane_current_path}"
"##,
        );
        assert!(config.diagnostics.is_empty());
        for command in &config.commands {
            shared
                .execute(client, ClientKind::Interactive, &mut context, command)
                .expect("tmux config command");
        }
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "picker"]),
            )
            .expect("session");
        let source = context.pane.expect("source terminal");
        let window = context.window.expect("window");
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.engine.keys.prefix(), "C-a");
            assert_eq!(
                inner.engine.keys.get("prefix", "|").unwrap().commands[0].name,
                "split-window"
            );
            assert_eq!(
                inner.engine.keys.get("prefix", "-").unwrap().commands[0].name,
                "split-window"
            );
        }

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: source,
                    input: test_key(
                        KeyCode::Character('a'),
                        Modifiers::new(false, true, false, false),
                        None,
                    ),
                    text_follows: false,
                },
            )
            .expect("configured prefix");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: source,
                    input: test_key(KeyCode::Character('|'), Modifiers::default(), Some("|")),
                    text_follows: true,
                },
            )
            .expect("configured horizontal split binding");
        let terminal = context.pane.expect("terminal id");
        {
            let inner = shared.inner.lock();
            assert!(inner.terminals.contains_key(&source));
            assert!(inner.terminals.contains_key(&terminal));
            assert!(matches!(
                inner.engine.state.pane(terminal).map(|pane| &pane.kind),
                Some(PaneKind::Terminal)
            ));
            assert!(matches!(
                inner.engine.state.windows[&window].layout.project(),
                LayoutNode::Split {
                    axis: zz_protocol::Axis::Horizontal,
                    ..
                }
            ));
        }

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: terminal,
                    input: test_key(
                        KeyCode::Character('a'),
                        Modifiers::new(false, true, false, false),
                        None,
                    ),
                    text_follows: false,
                },
            )
            .expect("configured prefix again");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: terminal,
                    input: test_key(KeyCode::Character('-'), Modifiers::default(), Some("-")),
                    text_follows: true,
                },
            )
            .expect("configured vertical split binding");
        let vertical = context.pane.expect("vertical terminal id");
        {
            let inner = shared.inner.lock();
            assert!(inner.terminals.contains_key(&vertical));
            assert!(matches!(
                inner.engine.state.pane(vertical).map(|pane| &pane.kind),
                Some(PaneKind::Terminal)
            ));
        }
    }

    #[test]
    fn default_percent_binding_creates_a_picker_via_split_picker() {
        let shared = Arc::new(Shared::new(1));
        let client = ClientId(7);
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "picker"]),
            )
            .expect("session");
        let source = context.pane.expect("source terminal");
        let window = context.window.expect("window");
        {
            let inner = shared.inner.lock();
            let binding = inner.engine.keys.get("prefix", "%").unwrap();
            assert_eq!(binding.commands[0].name, "split-picker");
            assert_eq!(binding.commands[0].args, ["-h"]);
        }

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: source,
                    input: test_key(
                        KeyCode::Character('b'),
                        Modifiers::new(false, true, false, false),
                        None,
                    ),
                    text_follows: false,
                },
            )
            .expect("default prefix");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: source,
                    input: test_key(KeyCode::Character('%'), Modifiers::default(), Some("%")),
                    text_follows: true,
                },
            )
            .expect("default horizontal split binding");
        let picker = context.pane.expect("picker id");
        let inner = shared.inner.lock();
        assert!(inner.terminals.contains_key(&source));
        assert!(!inner.terminals.contains_key(&picker));
        assert!(matches!(
            inner.engine.state.pane(picker).map(|pane| &pane.kind),
            Some(PaneKind::Picker { .. })
        ));
        assert!(matches!(
            inner.engine.state.windows[&window].layout.project(),
            LayoutNode::Split {
                axis: zz_protocol::Axis::Horizontal,
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn terminal_cwd_flag_prefers_a_valid_literal_and_bogus_paths_land_in_home() {
        let donor_directory = tempfile::tempdir().expect("donor working directory");
        let donor_physical = donor_directory.path().join("physical");
        fs::create_dir(&donor_physical).expect("physical donor working directory");
        let donor_literal = donor_directory.path().join("literal");
        std::os::unix::fs::symlink(&donor_physical, &donor_literal)
            .expect("literal donor working directory");
        let donor_path = donor_physical
            .canonicalize()
            .expect("canonical donor working directory");
        let literal_directory = tempfile::tempdir().expect("literal working directory");
        let literal_path = literal_directory
            .path()
            .canonicalize()
            .expect("canonical literal working directory");
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "new-session",
                    ["-s", "cwd", "-c", donor_literal.to_string_lossy().as_ref()],
                ),
            )
            .expect("session with literal cwd");
        let donor = context.pane.expect("donor pane");
        let donor_terminal = Arc::clone(&shared.inner.lock().terminals[&donor]);
        let wait_for_cwd = |terminal: &TerminalSession, expected: &Path| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if terminal_working_directory(terminal).as_deref() == Some(expected) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for terminal cwd; expected={expected:?}, pid={:?}, cwd={:?}",
                    terminal.foreground_process_id(),
                    terminal_working_directory(terminal),
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        wait_for_cwd(&donor_terminal, &donor_path);
        assert_eq!(
            shared.inner.lock().terminal_spawns[&donor].working_directory,
            Some(donor_literal.clone())
        );
        assert_eq!(
            shared
                .execute(
                    ClientId(7),
                    ClientKind::Command,
                    &mut context,
                    &CommandInvocation::new("display-message", ["-p", "#{pane_start_path}"],),
                )
                .expect("literal pane start path")
                .output,
            donor_literal.to_string_lossy()
        );
        donor_terminal.send_text("printf 'ZZ_LITERAL_PWD=[%s]\\n' \"$PWD\"\n");
        let expected_pwd = format!("ZZ_LITERAL_PWD=[{}]", donor_literal.display());
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let captured = shared
                .execute(
                    ClientId(7),
                    ClientKind::Command,
                    &mut context,
                    &CommandInvocation::new("capture-pane", ["-pJ", "-t", &donor.to_string()]),
                )
                .expect("capture literal PWD")
                .output;
            if captured.contains(&expected_pwd) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "literal PWD did not reach the child; expected={expected_pwd:?}, captured={captured:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "split-window",
                    ["-h", "-c", literal_path.to_string_lossy().as_ref()],
                ),
            )
            .expect("split with valid literal cwd");
        let literal = context.pane.expect("literal cwd pane");
        let literal_terminal = Arc::clone(&shared.inner.lock().terminals[&literal]);
        wait_for_cwd(&literal_terminal, &literal_path);

        let bogus_path = literal_directory.path().join("missing");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "split-window",
                    ["-v", "-c", bogus_path.to_string_lossy().as_ref()],
                ),
            )
            .expect("split with bogus literal cwd");
        let fallback = context.pane.expect("fallback cwd pane");
        let fallback_terminal = Arc::clone(&shared.inner.lock().terminals[&fallback]);
        let home_path = std::env::var_os("HOME")
            .map(PathBuf::from)
            .expect("HOME is set")
            .canonicalize()
            .expect("canonical home directory");
        wait_for_cwd(&fallback_terminal, &home_path);
    }

    #[cfg(unix)]
    #[test]
    fn terminal_agent_and_editor_panes_inherit_live_working_directory() {
        let directory = tempfile::tempdir().expect("temporary working directory");
        let physical = directory.path().join("physical");
        fs::create_dir(&physical).expect("physical working directory");
        let reported = directory.path().join("reported");
        std::os::unix::fs::symlink(&physical, &reported).expect("working directory symlink");
        let expected = physical
            .canonicalize()
            .expect("canonical working directory");
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "cwd"]),
            )
            .expect("session");
        let first = context.pane.expect("first pane");
        let source = Arc::clone(&shared.inner.lock().terminals[&first]);
        let reported_path = reported.to_string_lossy().into_owned();
        source.send_text(format!(
            "cd '{}'\nprintf '\\033]7;file://workstation{}\\a'\n",
            reported.display(),
            reported_path
        ));

        let wait_for_cwd = |terminal: &TerminalSession| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if terminal_working_directory(terminal).as_deref() == Some(expected.as_path()) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for terminal cwd; pid={:?}, cwd={:?}",
                    terminal.foreground_process_id(),
                    terminal_working_directory(terminal),
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        wait_for_cwd(&source);

        let expected_path = expected.to_string_lossy().into_owned();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let facts = shared
                .inner
                .lock()
                .engine
                .pane_runtime_facts(first)
                .cloned()
                .unwrap_or_default();
            if facts.current_path == expected_path && facts.reported_path == reported_path {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for pane path facts; facts={facts:?}",
            );
            thread::sleep(Duration::from_millis(10));
        }
        let first_target = first.to_string();
        let paths = shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "display-message",
                    [
                        "-p",
                        "-t",
                        first_target.as_str(),
                        "#{pane_current_path}|#{pane_path}",
                    ],
                ),
            )
            .expect("live pane paths");
        assert_eq!(paths.output, format!("{expected_path}|{reported_path}"));

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("split terminal");
        let second = context.pane.expect("second pane");
        let inherited = Arc::clone(&shared.inner.lock().terminals[&second]);
        wait_for_cwd(&inherited);

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("split-picker", ["-v"]),
            )
            .expect("agent picker");
        let agent = context.pane.expect("agent picker pane");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "experimental-agent-pane", "on"]),
            )
            .expect("enable agent panes");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("select-pane-kind", ["-t", &agent.to_string(), "agent"]),
            )
            .expect("agent materialization");
        {
            let inner = shared.inner.lock();
            assert!(matches!(
                inner.engine.state.pane(agent).map(|pane| &pane.kind),
                Some(PaneKind::Agent(descriptor))
                    if descriptor.cwd.as_deref() == Some(expected.as_path())
            ));
        }

        let configured_directory = tempfile::tempdir().expect("configured agent directory");
        let configured = configured_directory
            .path()
            .canonicalize()
            .expect("configured working directory");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("split-picker", ["-v"]),
            )
            .expect("configured agent picker");
        let configured_agent = context.pane.expect("configured agent picker pane");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "select-pane-kind",
                    [
                        "-t",
                        &configured_agent.to_string(),
                        "-c",
                        configured.to_string_lossy().as_ref(),
                        "agent",
                    ],
                ),
            )
            .expect("configured agent materialization");
        assert!(matches!(
            shared
                .inner
                .lock()
                .engine
                .state
                .pane(configured_agent)
                .map(|pane| &pane.kind),
            Some(PaneKind::Agent(descriptor))
                if descriptor.cwd.as_deref() == Some(configured.as_path())
        ));

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("split-picker", ["-v"]),
            )
            .expect("editor picker");
        let editor = context.pane.expect("editor picker pane");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "experimental-editor-pane", "on"]),
            )
            .expect("enable editor panes");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("select-pane-kind", ["-t", &editor.to_string(), "editor"]),
            )
            .expect("editor materialization");
        let restore_path = expected.join("restored.rs");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-editor-path",
                    [
                        "-t",
                        &editor.to_string(),
                        restore_path.to_string_lossy().as_ref(),
                    ],
                ),
            )
            .expect("editor restore metadata");
        let inner = shared.inner.lock();
        assert!(matches!(
            inner.engine.state.pane(editor).map(|pane| &pane.kind),
            Some(PaneKind::Editor(descriptor))
                if descriptor.cwd == expected.to_string_lossy()
                    && descriptor.path.as_deref()
                        == Some(restore_path.to_string_lossy().as_ref())
        ));
    }

    #[test]
    fn word_separator_changes_reconfigure_existing_and_future_terminal_actors() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "words"]),
            )
            .expect("session");
        let session = context.session.expect("session id");
        let first = context.pane.expect("first pane");
        assert!(
            shared.inner.lock().terminals[&first]
                .word_separators()
                .contains_separator('.')
        );

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-option",
                    ["-t", &session.to_string(), "word-separators", ""],
                ),
            )
            .expect("empty session separators");
        assert!(
            !shared.inner.lock().terminals[&first]
                .word_separators()
                .contains_separator('.')
        );

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("second terminal");
        let second = context.pane.expect("second pane");
        assert!(
            !shared.inner.lock().terminals[&second]
                .word_separators()
                .contains_separator('.')
        );

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "word-separators", "|"]),
            )
            .expect("global separators");
        assert!(
            !shared.inner.lock().terminals[&first]
                .word_separators()
                .contains_separator('|')
        );
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-option",
                    ["-u", "-t", &session.to_string(), "word-separators"],
                ),
            )
            .expect("restore inheritance");
        let inner = shared.inner.lock();
        for pane in [first, second] {
            assert!(
                inner.terminals[&pane]
                    .word_separators()
                    .contains_separator('|')
            );
        }
    }

    #[test]
    fn mode_keys_retarget_active_and_future_native_copy_modes() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) = shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "mode-keys"]),
            )
            .expect("session");
        let session = context.session.expect("session id");
        let window = context.window.expect("window id");
        let pane = context.pane.expect("pane id");
        shared.attach(client, session).expect("attach client");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        let view = TerminalViewId(client.0);

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("copy-mode", ["-t", &pane.to_string()]),
            )
            .expect("enter default copy mode");
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("copy-mode")
        );
        wait_for_view_mode(
            &terminal,
            view,
            "default copy mode never reached the viewport",
            |mode| matches!(mode, TerminalMode::Copy { .. }),
        );
        wait_for_observed_copy_session(&shared, client);

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-window-option",
                    ["-t", &window.to_string(), "mode-keys", "vi"],
                ),
            )
            .expect("switch active mode table");
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("copy-mode-vi")
        );

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-gw", "mode-keys", "emacs"]),
            )
            .expect("change global default under override");
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("copy-mode-vi")
        );

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-option",
                    ["-u", "-t", &window.to_string(), "mode-keys"],
                ),
            )
            .expect("restore live inheritance");
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("copy-mode")
        );

        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("send-keys", ["-t", &pane.to_string(), "-X", "cancel"]),
            )
            .expect("leave copy mode");
        wait_for_view_mode(&terminal, view, "cancel never thawed the pane", |mode| {
            mode == TerminalMode::Live
        });
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "set-window-option",
                    ["-t", &window.to_string(), "mode-keys", "vi"],
                ),
            )
            .expect("configure future copy mode");
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("copy-mode", ["-t", &pane.to_string()]),
            )
            .expect("enter configured copy mode");
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("copy-mode-vi")
        );
    }

    #[test]
    fn native_copy_buffers_create_named_entries_and_append_to_the_top() {
        let shared = Shared::new(1);
        shared.store_copy_buffer(
            "alpha".to_owned(),
            PasteBufferAction::Create {
                prefix: Some("selection".to_owned()),
            },
        );
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.paste_buffers.len(), 1);
            assert_eq!(inner.paste_buffers[0].name, "selection0");
            assert_eq!(inner.paste_buffers[0].data.as_ref(), b"alpha");
            assert!(inner.paste_buffers[0].automatic);
        }

        shared.store_copy_buffer("-beta".to_owned(), PasteBufferAction::Append);
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.paste_buffers.len(), 1);
            assert_eq!(inner.paste_buffers[0].data.as_ref(), b"alpha-beta");
            assert!(!inner.paste_buffers[0].automatic);
        }

        shared.store_copy_buffer(
            "gamma".to_owned(),
            PasteBufferAction::Create { prefix: None },
        );
        let inner = shared.inner.lock();
        assert_eq!(inner.paste_buffers.len(), 2);
        assert_eq!(inner.paste_buffers[0].name, "buffer1");
        assert_eq!(inner.paste_buffers[0].data.as_ref(), b"gamma");
        assert!(inner.paste_buffers[0].automatic);
    }

    #[test]
    fn app_clipboard_writes_reach_every_viewer_and_obey_set_clipboard() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "clip"]),
            )
            .expect("session");
        let session = context.session.expect("session id");
        let pane = context.pane.expect("pane id");
        shared.attach(client, session).expect("attach session");

        let observer_mailbox = OutboundMailbox::new();
        let (observer, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&observer_mailbox),
        );
        shared.attach(observer, session).expect("attach observer");
        take_reliable_messages(&mailbox);
        take_reliable_messages(&observer_mailbox);

        shared.deliver_clipboard_write(pane, ClipboardTarget::Clipboard, "external".to_owned());
        assert_eq!(
            take_clipboard_writes(&mailbox, pane),
            vec![(ClipboardTarget::Clipboard, "external".to_owned())]
        );
        assert_eq!(
            take_clipboard_writes(&observer_mailbox, pane),
            vec![(ClipboardTarget::Clipboard, "external".to_owned())]
        );
        assert!(shared.inner.lock().paste_buffers.is_empty());

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "set-clipboard", "on"]),
            )
            .expect("set-clipboard on");
        take_reliable_messages(&mailbox);
        take_reliable_messages(&observer_mailbox);
        shared.deliver_clipboard_write(pane, ClipboardTarget::Primary, "buffered".to_owned());
        assert_eq!(
            take_clipboard_writes(&mailbox, pane),
            vec![(ClipboardTarget::Primary, "buffered".to_owned())]
        );
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.paste_buffers.len(), 1);
            assert_eq!(inner.paste_buffers[0].data.as_ref(), b"buffered");
            assert!(inner.paste_buffers[0].automatic);
        }

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "set-clipboard", "off"]),
            )
            .expect("set-clipboard off");
        take_reliable_messages(&mailbox);
        take_reliable_messages(&observer_mailbox);
        shared.deliver_clipboard_write(pane, ClipboardTarget::Clipboard, "dropped".to_owned());
        assert!(take_clipboard_writes(&mailbox, pane).is_empty());
        assert!(take_clipboard_writes(&observer_mailbox, pane).is_empty());
        assert_eq!(shared.inner.lock().paste_buffers.len(), 1);
    }

    #[cfg(not(windows))]
    #[test]
    fn copy_pipe_receives_the_selection_on_standard_input() {
        let output = tempfile::NamedTempFile::new().expect("output file");
        let command = format!("cat > {}", shell_quote(output.path()));

        run_copy_pipe(&command, "native selection\nwith two lines").expect("copy pipe");

        assert_eq!(
            fs::read_to_string(output.path()).expect("piped output"),
            "native selection\nwith two lines"
        );
    }

    #[test]
    fn copy_pipe_failure_is_a_reliable_client_message_and_releases_its_permit() {
        let shared = Arc::new(Shared::new(1));
        let subscriber = OutboundMailbox::new();
        let (pane, client) = {
            let mut inner = shared.inner.lock();
            let mut context = ExecutionContext::default();
            inner
                .engine
                .execute(
                    &mut context,
                    &CommandInvocation::new("new-session", [] as [&str; 0]),
                )
                .expect("session");
            let session = context.session.expect("session id");
            let pane = context.pane.expect("pane id");
            let client = ClientId(41);
            inner.attached.entry(session).or_default().insert(client);
            inner.subscribers.insert(client, Arc::clone(&subscriber));
            (pane, client)
        };
        let command = if cfg!(windows) { "exit /B 7" } else { "exit 7" };

        shared.spawn_copy_pipe(pane, client, command.to_owned(), "selection".to_owned());

        let deadline = Instant::now() + Duration::from_secs(30);
        while subscriber.state.lock().reliable.is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !subscriber.state.lock().reliable.is_empty(),
            "copy-pipe diagnostic was not delivered"
        );
        let frame = subscriber.recv().expect("copy-pipe diagnostic");
        let message = decode_protocol_frame(&frame).expect("diagnostic frame");
        assert!(matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::ClientMessage {
                    pane: Some(actual),
                    kind: ClientMessageKind::Error,
                    text,
                },
                ..
            }) if actual == pane && text.contains("exited unsuccessfully")
        ));
        assert_eq!(shared.inner.lock().active_copy_pipes, 0);
    }

    #[test]
    fn copy_pipe_rejects_work_past_the_process_limit() {
        let shared = Arc::new(Shared::new(1));
        shared.inner.lock().active_copy_pipes = MAX_COPY_PIPE_PROCESSES;

        shared.spawn_copy_pipe(
            PaneId(9),
            ClientId(9),
            "ignored".to_owned(),
            "selection".to_owned(),
        );

        assert_eq!(
            shared.inner.lock().active_copy_pipes,
            MAX_COPY_PIPE_PROCESSES
        );
    }

    #[cfg(not(windows))]
    fn shell_quote(path: &Path) -> String {
        let path = path.to_string_lossy();
        format!("'{}'", path.replace('\'', "'\"'\"'"))
    }

    #[test]
    fn outbound_mailbox_promotes_overlapping_terminal_updates() {
        let mailbox = OutboundMailbox::new();
        let pane = PaneId(9);
        let first = terminal_test_message(pane, 1, 1);
        let second = terminal_test_message(pane, 2, 2);

        assert_eq!(
            mailbox.enqueue_terminal(pane, &first),
            TerminalEnqueue::Queued
        );
        assert_eq!(
            mailbox.enqueue_terminal(pane, &second),
            TerminalEnqueue::NeedsFull
        );
        assert!(mailbox.replace_terminal(pane, &second));

        let encoded = mailbox.recv().expect("coalesced terminal update");
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), second);
        mailbox.close();
        assert!(mailbox.recv().is_none());
    }

    #[test]
    fn outbound_mailbox_delivers_kitty_pixels_before_viewports_and_resets_per_pane() {
        let mailbox = OutboundMailbox::new();
        let pane = PaneId(9);
        let image_id = 4;
        let generation = 2;
        let controls = [
            ProtocolMessage::Event(Event {
                sequence: 1,
                payload: EventPayload::KittyImageBegin {
                    pane,
                    image_id,
                    generation,
                    width: 1,
                    height: 1,
                    total_bytes: 4,
                },
            }),
            ProtocolMessage::Event(Event {
                sequence: 2,
                payload: EventPayload::KittyImageChunk {
                    pane,
                    image_id,
                    generation,
                    bytes: vec![1, 2, 3, 4],
                },
            }),
        ];
        let frames = controls
            .iter()
            .map(|message| encode_protocol_message(message).expect("encode Kitty control"))
            .collect::<Vec<_>>();
        assert_eq!(
            mailbox.enqueue_kitty_image(pane, image_id, generation, &frames),
            KittyImageEnqueue::Queued
        );
        let viewport = terminal_test_message(pane, 3, 1);
        assert_eq!(
            mailbox.enqueue_terminal(pane, &viewport),
            TerminalEnqueue::Queued
        );

        for expected in &controls {
            let frame = mailbox.recv().expect("reliable Kitty frame");
            assert_eq!(
                decode_protocol_frame(&frame).expect("decode Kitty"),
                *expected
            );
        }
        let frame = mailbox.recv().expect("viewport after Kitty pixels");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode viewport"),
            viewport
        );
        assert_eq!(
            mailbox.state.lock().delivered_images[&pane][&image_id],
            generation
        );

        assert_eq!(
            mailbox.enqueue_kitty_image(pane, image_id, generation, &frames),
            KittyImageEnqueue::AlreadyDelivered
        );
        assert!(mailbox.state.lock().reliable.is_empty());
        mailbox.reset_kitty_images();
        assert!(!mailbox.state.lock().delivered_images.contains_key(&pane));
        assert_eq!(
            mailbox.enqueue_kitty_image(pane, image_id, generation, &frames),
            KittyImageEnqueue::Queued
        );
        assert_eq!(take_reliable_messages(&mailbox).len(), 2);

        mailbox.suspend_terminal(pane);
        assert_eq!(
            mailbox.state.lock().delivered_images[&pane][&image_id],
            generation
        );
        assert_eq!(
            mailbox.enqueue_kitty_image(pane, image_id, generation, &frames),
            KittyImageEnqueue::AlreadyDelivered
        );
        assert!(mailbox.state.lock().reliable.is_empty());

        mailbox.cancel_terminal(pane);
        assert!(!mailbox.state.lock().delivered_images.contains_key(&pane));
        assert_eq!(
            mailbox.enqueue_kitty_image(pane, image_id, generation, &frames),
            KittyImageEnqueue::Queued
        );
        assert_eq!(take_reliable_messages(&mailbox).len(), 2);

        mailbox.enqueue_kitty_images_removed(pane, &[image_id]);
        assert!(matches!(
            take_reliable_messages(&mailbox).as_slice(),
            [ProtocolMessage::Event(Event {
                payload: EventPayload::KittyImagesRemoved { pane: target, image_ids },
                ..
            })] if *target == pane && image_ids == &[image_id]
        ));
        assert!(!mailbox.state.lock().delivered_images.contains_key(&pane));
    }

    #[test]
    fn pasted_image_fetch_uses_encoded_frames_and_a_per_mailbox_delivery_ledger() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let pane = {
            let mut inner = shared.inner.lock();
            let (session, _, pane) = inner
                .engine
                .state
                .create_session("pasted-image-fetch")
                .expect("session");
            inner.attached.entry(session).or_default().insert(client);
            pane
        };
        let number = 4;
        let token = 17;
        let bytes = Arc::<[u8]>::from(vec![0x89, 0x50, 0x4e, 0x47]);
        {
            let mut panes = shared.pasted_images.lock();
            let images = panes.entry(pane).or_default();
            images.stored_bytes = bytes.len();
            images.order.push_back((number, token));
            images.images.insert(
                number,
                StoredPastedImage {
                    token,
                    format: PastedImageFormat::Png,
                    bytes: Arc::clone(&bytes),
                    frames: None,
                },
            );
        }

        shared.fetch_pasted_image(client, pane, number);
        assert_eq!(
            take_reliable_messages(&mailbox),
            vec![
                ProtocolMessage::PastedImageBegin {
                    pane,
                    number,
                    format: PastedImageFormat::Png,
                    total_bytes: 4,
                },
                ProtocolMessage::PastedImageChunk {
                    pane,
                    number,
                    bytes: bytes.to_vec(),
                },
            ]
        );
        assert!(
            shared.pasted_images.lock()[&pane].images[&number]
                .frames
                .is_some()
        );
        shared.fetch_pasted_image(client, pane, number);
        assert!(take_reliable_messages(&mailbox).is_empty());

        mailbox.cancel_terminal(pane);
        shared.fetch_pasted_image(client, pane, number);
        assert_eq!(take_reliable_messages(&mailbox).len(), 2);
        shared.fetch_pasted_image(client, pane, number + 1);
        assert_eq!(
            take_reliable_messages(&mailbox),
            vec![ProtocolMessage::PastedImageUnavailable {
                pane,
                number: number + 1,
            }]
        );
    }

    #[test]
    fn pasted_image_store_caps_pending_and_bound_bytes_together() {
        let mut images = PanePastedImages::default();
        let bytes = Arc::<[u8]>::from(vec![0; 4 * 1024 * 1024]);
        for token in 1..=6 {
            let admission = images.push_pending(PendingPastedImage {
                token,
                format: PastedImageFormat::Png,
                bytes: Arc::clone(&bytes),
            });
            assert!(admission.evicted_numbers.is_empty());
            assert!(admission.retained);
            assert_eq!(
                images.bind(token, u32::try_from(token).unwrap()),
                Some(Vec::new())
            );
        }
        assert_eq!(images.stored_bytes, MAX_PASTED_IMAGE_BYTES_PER_PANE);

        let admission = images.push_pending(PendingPastedImage {
            token: 7,
            format: PastedImageFormat::Png,
            bytes,
        });
        assert_eq!(admission.evicted_numbers, [1]);
        assert!(admission.retained);
        assert!(!images.images.contains_key(&1));
        assert_eq!(images.pending.front().map(|pending| pending.token), Some(7));
        assert_eq!(
            images.stored_bytes + images.pending_bytes,
            MAX_PASTED_IMAGE_BYTES_PER_PANE
        );
        assert_eq!(images.images.len() + images.pending.len(), 6);
    }

    #[test]
    fn pasted_image_store_rejects_the_newest_pending_window_when_only_windows_remain() {
        let mut images = PanePastedImages::default();
        let bytes = Arc::<[u8]>::from(vec![0; 3 * 1024 * 1024]);
        for token in 1..=MAX_PASTED_IMAGES_PER_PANE as u64 {
            let admission = images.push_pending(PendingPastedImage {
                token,
                format: PastedImageFormat::Png,
                bytes: Arc::clone(&bytes),
            });
            assert!(admission.retained);
            assert!(admission.evicted_numbers.is_empty());
        }
        let admission = images.push_pending(PendingPastedImage {
            token: 99,
            format: PastedImageFormat::Png,
            bytes,
        });
        assert!(!admission.retained);
        assert!(admission.evicted_numbers.is_empty());
        assert_eq!(images.pending.len(), MAX_PASTED_IMAGES_PER_PANE);
        assert!(images.pending.iter().all(|pending| pending.token != 99));
    }

    #[test]
    fn outbound_mailbox_reuses_completed_frame_buffers() {
        let mailbox = OutboundMailbox::new();
        let pane = PaneId(9);
        let first = terminal_test_message(pane, 1, 1);
        assert_eq!(
            mailbox.enqueue_terminal(pane, &first),
            TerminalEnqueue::Queued
        );
        let first_frame = mailbox.recv().expect("first terminal update");
        let allocation = first_frame.as_ptr();
        let capacity = first_frame.capacity();
        assert_eq!(decode_protocol_frame(&first_frame).expect("decode"), first);
        mailbox.recycle_frame(first_frame);

        {
            let state = mailbox.state.lock();
            assert_eq!(state.recycled_frames.len(), 1);
            assert_eq!(state.recycled_capacity, capacity);
        }

        let second = terminal_test_message(pane, 2, 2);
        assert_eq!(
            mailbox.enqueue_terminal(pane, &second),
            TerminalEnqueue::Queued
        );
        let second_frame = mailbox.recv().expect("second terminal update");
        assert_eq!(second_frame.as_ptr(), allocation);
        assert_eq!(second_frame.capacity(), capacity);
        assert_eq!(
            decode_protocol_frame(&second_frame).expect("decode"),
            second
        );
    }

    #[test]
    fn outbound_mailbox_bounds_recycled_frame_storage() {
        let mailbox = OutboundMailbox::new();
        mailbox.recycle_frame(Vec::with_capacity(MAX_RECYCLED_FRAME_CAPACITY + 1));
        assert!(mailbox.state.lock().recycled_frames.is_empty());

        for _ in 0..=MAX_RECYCLED_FRAME_BUFFERS {
            mailbox.recycle_frame(Vec::with_capacity(16));
        }
        let state = mailbox.state.lock();
        assert_eq!(state.recycled_frames.len(), MAX_RECYCLED_FRAME_BUFFERS);
        assert_eq!(state.recycled_capacity, MAX_RECYCLED_FRAME_BUFFERS * 16);
    }

    #[test]
    fn outbound_mailbox_promotes_a_patch_after_visibility_reset() {
        let mailbox = OutboundMailbox::new();
        let pane = PaneId(9);
        let first = terminal_test_message(pane, 1, 1);
        assert_eq!(
            mailbox.enqueue_terminal(pane, &first),
            TerminalEnqueue::Queued
        );
        let _ = mailbox.recv().expect("initial full viewport");
        mailbox.cancel_terminal(pane);

        let patch = terminal_patch_test_message(pane, 2, 1, 2);
        assert_eq!(
            mailbox.enqueue_terminal(pane, &patch),
            TerminalEnqueue::NeedsFull
        );
        let full = terminal_test_message(pane, 3, 2);
        assert_eq!(
            mailbox.enqueue_terminal(pane, &full),
            TerminalEnqueue::Queued
        );
        assert_eq!(
            decode_protocol_frame(&mailbox.recv().expect("replacement full viewport"))
                .expect("decode replacement"),
            full
        );
        mailbox.close();
    }

    #[test]
    fn request_full_enqueues_only_the_requested_visible_pane() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "request-full"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let first = context.pane.expect("first pane");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("split pane");
        let second = context.pane.expect("second pane");
        shared.attach(client, session).expect("attach session");

        let deadline = Instant::now() + Duration::from_secs(30);
        for pane in [first, second] {
            loop {
                let ready = shared.inner.lock().terminals[&pane]
                    .latest_viewport_for(TerminalViewId(client.0))
                    .is_some();
                if ready {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "terminal {pane} did not publish its attached viewport"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }
        thread::sleep(Duration::from_millis(50));
        loop {
            let available = {
                let state = mailbox.state.lock();
                !state.reliable.is_empty()
                    || state.command_output.is_some()
                    || !state.terminals.is_empty()
            };
            if !available {
                break;
            }
            let _ = mailbox.recv().expect("available pre-request message");
        }

        shared.send_full(client, first, &mailbox);
        let frame = mailbox.recv().expect("requested full viewport");
        let message = decode_protocol_frame(&frame).expect("decode requested viewport");
        assert!(matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalViewport { pane, .. },
                ..
            }) if pane == first
        ));
        assert!(mailbox.state.lock().terminals.is_empty());

        shared.send_full(client, PaneId(u64::MAX), &mailbox);
        assert!(mailbox.state.lock().terminals.is_empty());
    }

    fn history_chunk_text(
        rows: &[Vec<zz_terminal::PackedCell>],
        dictionary: &TerminalDictionary,
    ) -> String {
        let mut output = String::new();
        for row in rows {
            for cell in row {
                let glyph = cell.glyph();
                if glyph == 0 {
                    continue;
                }
                if glyph & GRAPHEME_TABLE_BIT == 0 {
                    if let Some(character) = char::from_u32(glyph) {
                        output.push(character);
                    }
                    continue;
                }
                let index = usize::try_from(glyph & !GRAPHEME_TABLE_BIT).unwrap_or(usize::MAX);
                let Some((&start, &end)) = dictionary
                    .grapheme_offsets
                    .get(index)
                    .zip(dictionary.grapheme_offsets.get(index.saturating_add(1)))
                else {
                    continue;
                };
                let Some(bytes) = dictionary.grapheme_bytes.get(
                    usize::try_from(start).unwrap_or(usize::MAX)
                        ..usize::try_from(end).unwrap_or(usize::MAX),
                ) else {
                    continue;
                };
                if let Ok(grapheme) = std::str::from_utf8(bytes) {
                    output.push_str(grapheme);
                }
            }
            output.push('\n');
        }
        output
    }

    #[cfg(unix)]
    #[test]
    fn upward_wheel_scrolls_a_live_viewport_and_typing_snaps_it_back_to_the_bottom() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "wheel-copy-mode"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let pane = context.pane.expect("pane");
        shared.attach(client, session).expect("attach session");

        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        let view = TerminalViewId(client.0);
        terminal.resize(16, 4, 8, 18);
        wait_for_terminal_dimensions(&terminal, view, 16, 4);
        terminal.send_text(
            "i=0; while [ $i -lt 40 ]; do printf 'ZZW%02d\\r\\n' \"$i\"; i=$((i+1)); done; printf 'ZZ_WHEEL_DONE\\r\\n'\n",
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let ready = terminal.latest_viewport_for(view).is_some_and(|viewport| {
                viewport
                    .scrollbar
                    .total
                    .saturating_sub(viewport.scrollbar.len)
                    >= 10
            });
            if ready {
                break;
            }
            assert!(Instant::now() < deadline, "terminal history did not fill");
            thread::sleep(Duration::from_millis(10));
        }

        terminal.view_action(view, TerminalViewAction::ScrollToOffset(5));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let pinned = terminal.latest_viewport_for(view).is_some_and(|viewport| {
                matches!(viewport.mode, TerminalMode::Live) && viewport.scrollbar.offset == 5
            });
            if pinned {
                break;
            }
            assert!(Instant::now() < deadline, "live viewport did not pin");
            thread::sleep(Duration::from_millis(10));
        }

        let wheel = TerminalMouseInput::new(
            TerminalMousePhase::Press,
            Some(TerminalMouseButton::ScrollUp),
            PointerCellEvent {
                column: 3,
                row: 1,
                click_count: 1,
                rectangle: false,
            },
            24,
            18,
            128,
            72,
            8,
            18,
            Modifiers::default(),
            false,
        );
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::TerminalView {
                    pane,
                    action: TerminalViewAction::ScrollWheel {
                        lines: -1,
                        input: wheel,
                    },
                },
            )
            .expect("wheel scrolls the live viewport");

        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner
                    .key_engines
                    .get(&client)
                    .and_then(KeyEngine::active_table),
                None,
                "a wheel gesture must not move the client onto a copy-mode table"
            );
            assert!(
                !inner.copy_sessions.contains_key(&client),
                "a wheel gesture must not open a copy session"
            );
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let scrolled = terminal.latest_viewport_for(view).is_some_and(|viewport| {
                viewport.mode == TerminalMode::Live && viewport.scrollbar.offset == 4
            });
            if scrolled {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "wheel did not scroll the live viewport"
            );
            thread::sleep(Duration::from_millis(10));
        }

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane,
                    input: test_key(KeyCode::Character('z'), Modifiers::default(), Some("z")),
                    text_follows: false,
                },
            )
            .expect("type while scrolled back");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let at_bottom = terminal.latest_viewport_for(view).is_some_and(|viewport| {
                viewport.mode == TerminalMode::Live
                    && viewport
                        .scrollbar
                        .offset
                        .saturating_add(viewport.scrollbar.len)
                        >= viewport.scrollbar.total
            });
            if at_bottom {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "typing did not snap the live viewport back to the bottom"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn copy_mode_fixture(
        name: &str,
        producer: &str,
    ) -> (
        Arc<Shared>,
        ClientId,
        ExecutionContext,
        PaneId,
        Arc<TerminalSession>,
        Arc<OutboundMailbox>,
    ) {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let command =
            format!("read _; {producer}; printf '\\033]0;zz-copy-ready\\007'; exec /bin/cat");
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", name, &command]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let pane = context.pane.expect("pane");
        shared.attach(client, session).expect("attach session");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        terminal.resize(16, 4, 8, 18);
        wait_for_terminal_dimensions(&terminal, TerminalViewId(client.0), 16, 4);
        terminal.send_text("ready\r");
        wait_for_viewport(
            &terminal,
            TerminalViewId(client.0),
            "copy fixture producer never became ready",
            |viewport| viewport.title() == "zz-copy-ready",
        );
        (shared, client, context, pane, terminal, mailbox)
    }

    fn wait_for_viewport(
        terminal: &Arc<TerminalSession>,
        view: TerminalViewId,
        expected: &str,
        predicate: impl Fn(&TerminalViewport) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let latest = terminal.latest_viewport_for(view);
            if latest.as_ref().is_some_and(|viewport| predicate(viewport)) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{expected}; latest={:?} text={:?}",
                latest.as_ref().map(|viewport| (
                    viewport.status.clone(),
                    viewport.columns,
                    viewport.rows,
                    viewport.generation,
                    viewport.view_generation,
                )),
                latest.as_ref().map(|viewport| viewport_text(viewport)),
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_view_mode(
        terminal: &Arc<TerminalSession>,
        view: TerminalViewId,
        expected: &str,
        predicate: impl Fn(TerminalMode) -> bool,
    ) {
        wait_for_viewport(terminal, view, expected, |viewport| {
            predicate(viewport.mode)
        });
    }

    fn wait_for_observed_copy_session(shared: &Arc<Shared>, client: ClientId) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if shared
                .inner
                .lock()
                .copy_sessions
                .get(&client)
                .is_some_and(|session| session.observed)
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "daemon never observed the pane frozen"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn wait_for_root_key_table(shared: &Arc<Shared>, client: ClientId, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            {
                let inner = shared.inner.lock();
                if inner
                    .key_engines
                    .get(&client)
                    .and_then(KeyEngine::active_table)
                    .is_none()
                    && !inner.copy_sessions.contains_key(&client)
                {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "{expected}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn test_drag(column: u16, phase: TerminalMousePhase) -> TerminalMouseInput {
        TerminalMouseInput::new(
            phase,
            Some(TerminalMouseButton::Left),
            PointerCellEvent {
                column,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            u32::from(column) * 8,
            0,
            128,
            72,
            8,
            18,
            Modifiers::default(),
            false,
        )
    }

    #[cfg(unix)]
    #[test]
    fn copy_mode_ed_instant_exit_frees_the_key_table_without_observation() {
        let (shared, client, mut context, pane, terminal, _mailbox) =
            copy_mode_fixture("copy-instant-exit", "printf 'one\\ntwo\\n'");
        let view = TerminalViewId(client.0);
        let target = pane.to_string();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("copy-mode", ["-e", "-d", "-t", target.as_str()]),
            )
            .expect("instant scroll-exit entry");
        wait_for_view_mode(
            &terminal,
            view,
            "copy mode did not exit instantly",
            |mode| mode == TerminalMode::Live,
        );
        wait_for_root_key_table(
            &shared,
            client,
            "instant scroll-exit stranded the key table",
        );
    }

    #[test]
    fn copy_mode_scroll_exit_reconciles_the_client_copy_session() {
        for (name, copy_args, exits) in [
            ("copy-scroll-exit", &["-e"][..], true),
            ("copy-scroll-stays", &[][..], false),
        ] {
            let (shared, client, mut context, pane, terminal, _mailbox) = copy_mode_fixture(
                name,
                "i=0; while [ $i -lt 12 ]; do printf 'line-%02d\\n' \"$i\"; i=$((i+1)); done",
            );
            let view = TerminalViewId(client.0);
            let target = pane.to_string();
            let args = copy_args
                .iter()
                .copied()
                .chain(["-t", target.as_str()])
                .collect::<Vec<_>>();
            shared
                .execute(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    &CommandInvocation::new("copy-mode", args),
                )
                .expect("enter copy mode");
            wait_for_view_mode(&terminal, view, "copy mode did not freeze", |mode| {
                matches!(mode, TerminalMode::Copy { .. })
            });
            wait_for_observed_copy_session(&shared, client);
            let generation = terminal
                .latest_viewport_for(view)
                .expect("copy viewport")
                .view_generation;
            shared
                .execute(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    &CommandInvocation::new("send-keys", ["-t", &target, "-X", "page-down"]),
                )
                .expect("page down");
            if exits {
                wait_for_view_mode(
                    &terminal,
                    view,
                    "scroll-exit did not leave copy mode",
                    |mode| mode == TerminalMode::Live,
                );
                wait_for_root_key_table(
                    &shared,
                    client,
                    "scroll-exit left the client on a copy table",
                );
            } else {
                wait_for_viewport(
                    &terminal,
                    view,
                    "plain copy mode did not publish page-down",
                    |viewport| {
                        viewport.view_generation > generation
                            && matches!(viewport.mode, TerminalMode::Copy { .. })
                    },
                );
                let inner = shared.inner.lock();
                assert!(inner.copy_sessions.contains_key(&client));
                assert_eq!(
                    inner
                        .key_engines
                        .get(&client)
                        .and_then(KeyEngine::active_table),
                    Some("copy-mode")
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn send_keys_count_prefix_moves_once_with_the_armed_count() {
        let (shared, client, mut context, pane, terminal, _mailbox) = copy_mode_fixture(
            "copy-repeat-count",
            "i=0; while [ $i -lt 12 ]; do printf 'line-%02d\\n' \"$i\"; i=$((i+1)); done",
        );
        let view = TerminalViewId(client.0);
        let target = pane.to_string();
        for command in [
            CommandInvocation::new("set-window-option", ["mode-keys", "vi"]),
            CommandInvocation::new("copy-mode", ["-t", &target]),
        ] {
            shared
                .execute(client, ClientKind::Interactive, &mut context, &command)
                .expect("configure copy mode");
        }
        wait_for_view_mode(&terminal, view, "copy mode did not freeze", |mode| {
            matches!(mode, TerminalMode::Copy { .. })
        });
        wait_for_observed_copy_session(&shared, client);
        let start = match terminal
            .latest_viewport_for(view)
            .expect("copy viewport")
            .mode
        {
            TerminalMode::Copy { position, .. } => position,
            other => panic!("expected copy mode, got {other:?}"),
        };
        assert!(start > 3);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("send-keys", ["-t", &target, "-N", "3"]),
            )
            .expect("arm copy-mode count");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane,
                    input: test_key(KeyCode::Character('k'), Modifiers::default(), Some("k")),
                    text_follows: false,
                },
            )
            .expect("counted movement key");
        wait_for_view_mode(&terminal, view, "counted movement did not land", |mode| {
            matches!(
                mode,
                TerminalMode::Copy { position, .. } if position == start - 3
            )
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_left_drag_selects_live_and_never_enters_copy_mode() {
        let (shared, client, mut context, pane, terminal, _mailbox) = copy_mode_fixture(
            "drag-select",
            "printf '\\033[2J\\033[HZZDRAG one two\\r\\n'",
        );
        let view = TerminalViewId(client.0);
        wait_for_viewport(
            &terminal,
            view,
            "drag fixture never reached the viewport",
            |viewport| {
                viewport.mode == TerminalMode::Live
                    && viewport_text(viewport).contains("ZZDRAG one two")
            },
        );

        for (column, phase) in [
            (1_u16, TerminalMousePhase::Press),
            (5, TerminalMousePhase::Motion),
            (9, TerminalMousePhase::Motion),
            (9, TerminalMousePhase::Release),
        ] {
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    InputMessage::TerminalView {
                        pane,
                        action: TerminalViewAction::Mouse(test_drag(column, phase)),
                    },
                )
                .expect("drag gesture");
        }

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let selected = terminal
                .latest_viewport_for(view)
                .is_some_and(|viewport| !viewport.overlays.is_empty());
            if selected {
                break;
            }
            assert!(Instant::now() < deadline, "drag painted no selection");
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            terminal
                .latest_viewport_for(view)
                .is_some_and(|viewport| viewport.mode == TerminalMode::Live),
            "a drag must leave the pane live"
        );
        let inner = shared.inner.lock();
        assert_eq!(
            inner
                .key_engines
                .get(&client)
                .and_then(KeyEngine::active_table),
            None,
            "a drag must not move the client onto a copy-mode table"
        );
        assert!(!inner.copy_sessions.contains_key(&client));
    }

    #[cfg(unix)]
    #[test]
    fn clear_selection_or_cancel_clears_before_leaving_copy_mode() {
        let (shared, client, mut context, pane, terminal, _mailbox) =
            copy_mode_fixture("vi-escape", ":");
        let view = TerminalViewId(client.0);
        let target = pane.to_string();
        let run = |context: &mut ExecutionContext, command: CommandInvocation| {
            shared
                .execute(client, ClientKind::Interactive, context, &command)
                .expect("copy-mode command");
        };

        run(
            &mut context,
            CommandInvocation::new("set-window-option", ["mode-keys", "vi"]),
        );
        run(
            &mut context,
            CommandInvocation::new("copy-mode", ["-t", &target]),
        );
        assert_eq!(
            shared
                .inner
                .lock()
                .key_engines
                .get(&client)
                .and_then(KeyEngine::active_table),
            Some("copy-mode-vi")
        );
        wait_for_view_mode(
            &terminal,
            view,
            "copy-mode command did not freeze",
            |mode| matches!(mode, TerminalMode::Copy { .. }),
        );
        wait_for_observed_copy_session(&shared, client);

        run(
            &mut context,
            CommandInvocation::new("send-keys", ["-t", &target, "-X", "begin-selection"]),
        );
        run(
            &mut context,
            CommandInvocation::new(
                "send-keys",
                ["-t", &target, "-X", "clear-selection-or-cancel"],
            ),
        );
        thread::sleep(Duration::from_millis(50));
        assert!(
            terminal
                .latest_viewport_for(view)
                .is_some_and(|viewport| matches!(viewport.mode, TerminalMode::Copy { .. })),
            "the first clear-or-cancel action must not leave copy mode"
        );
        assert_eq!(
            shared
                .inner
                .lock()
                .key_engines
                .get(&client)
                .and_then(KeyEngine::active_table),
            Some("copy-mode-vi")
        );

        run(
            &mut context,
            CommandInvocation::new(
                "send-keys",
                ["-t", &target, "-X", "clear-selection-or-cancel"],
            ),
        );
        wait_for_view_mode(
            &terminal,
            view,
            "the second clear-or-cancel action did not exit",
            |mode| mode == TerminalMode::Live,
        );
        wait_for_root_key_table(
            &shared,
            client,
            "clear-or-cancel left the client on a copy table",
        );
    }

    #[cfg(unix)]
    #[test]
    fn configured_v_e_y_preserves_indentation_in_the_clipboard() {
        let (shared, client, mut context, pane, terminal, mailbox) =
            copy_mode_fixture("vi-v-e-y", "printf '\\033[2J\\033[H    alpha beta\\n'");
        let view = TerminalViewId(client.0);
        let target = pane.to_string();
        for command in [
            CommandInvocation::new("set-window-option", ["mode-keys", "vi"]),
            CommandInvocation::new("set-option", ["-g", "set-clipboard", "on"]),
            CommandInvocation::new(
                "bind-key",
                [
                    "-T",
                    "copy-mode-vi",
                    "v",
                    "send-keys",
                    "-X",
                    "begin-selection",
                ],
            ),
            CommandInvocation::new(
                "bind-key",
                [
                    "-T",
                    "copy-mode-vi",
                    "y",
                    "send-keys",
                    "-X",
                    "copy-selection-and-cancel",
                ],
            ),
        ] {
            shared
                .execute(client, ClientKind::Interactive, &mut context, &command)
                .expect("configure vi copy mode");
        }

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if terminal
                .capture(CaptureOptions::default())
                .is_ok_and(|text| text.lines().any(|line| line == "    alpha beta"))
            {
                break;
            }
            assert!(Instant::now() < deadline, "terminal never printed fixture");
            thread::sleep(Duration::from_millis(10));
        }
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("copy-mode", ["-t", &target]),
            )
            .expect("enter copy mode");
        wait_for_view_mode(&terminal, view, "copy mode did not freeze", |mode| {
            matches!(mode, TerminalMode::Copy { .. })
        });
        wait_for_observed_copy_session(&shared, client);
        take_reliable_messages(&mailbox);

        for input in [
            test_key(KeyCode::Character('k'), Modifiers::default(), Some("k")),
            test_key(KeyCode::Character('0'), Modifiers::default(), Some("0")),
            test_key(KeyCode::Character('v'), Modifiers::default(), Some("v")),
            test_key(
                KeyCode::Character('e'),
                Modifiers::new(true, false, false, false),
                Some("E"),
            ),
        ] {
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    InputMessage::Key {
                        pane,
                        input,
                        text_follows: false,
                    },
                )
                .expect("vi copy key");
        }
        // The copy cursor is also an overlay, so wait for the exact selection
        // produced by 0 v E before allowing the yank to cancel copy mode.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if terminal.latest_viewport_for(view).is_some_and(|viewport| {
                viewport.overlays.iter().any(|overlay| {
                    overlay.kind() == zz_terminal::OverlayKind::Selection
                        && overlay.row == 0
                        && overlay.start == 0
                        && overlay.end == 9
                })
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "v E painted no selection");
            thread::sleep(Duration::from_millis(10));
        }
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane,
                    input: test_key(KeyCode::Character('y'), Modifiers::default(), Some("y")),
                    text_follows: false,
                },
            )
            .expect("vi copy yank key");

        let deadline = Instant::now() + Duration::from_secs(30);
        let mut observed = Vec::new();
        loop {
            let writes = take_clipboard_writes(&mailbox, pane);
            observed.extend(writes.iter().cloned());
            if writes
                .iter()
                .any(|(target, text)| *target == ClipboardTarget::Clipboard && text == "    alpha")
            {
                break;
            }
            if Instant::now() >= deadline {
                let mode = terminal
                    .latest_viewport_for(view)
                    .map(|viewport| viewport.mode);
                let (table, buffers) = {
                    let inner = shared.inner.lock();
                    (
                        inner
                            .key_engines
                            .get(&client)
                            .and_then(KeyEngine::active_table)
                            .map(str::to_owned),
                        inner
                            .paste_buffers
                            .iter()
                            .map(|buffer| String::from_utf8_lossy(&buffer.data).into_owned())
                            .collect::<Vec<_>>(),
                    )
                };
                panic!(
                    "v E y never copied the indented word: {observed:?}; mode={mode:?}; table={table:?}; buffers={buffers:?}"
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn focusing_another_pane_ends_the_copy_session() {
        let (shared, client, mut context, first, terminal, _mailbox) =
            copy_mode_fixture("copy-focus", ":");
        let view = TerminalViewId(client.0);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("split pane");
        let second = context.pane.expect("second pane");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane", ["-t", &first.to_string()]),
            )
            .expect("focus the first pane");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("copy-mode", ["-t", &first.to_string()]),
            )
            .expect("enter copy mode");
        wait_for_view_mode(&terminal, view, "copy-mode did not freeze", |mode| {
            matches!(mode, TerminalMode::Copy { .. })
        });

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane", ["-t", &second.to_string()]),
            )
            .expect("focus the second pane");

        {
            let inner = shared.inner.lock();
            assert!(
                !inner.copy_sessions.contains_key(&client),
                "focusing away must end the copy session"
            );
            assert_eq!(
                inner
                    .key_engines
                    .get(&client)
                    .and_then(KeyEngine::active_table),
                None
            );
        }
        wait_for_view_mode(
            &terminal,
            view,
            "the unfocused pane stayed frozen",
            |mode| mode == TerminalMode::Live,
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_session_that_diverges_from_its_pane_heals_on_the_next_publish() {
        let (shared, client, mut context, pane, terminal, _mailbox) =
            copy_mode_fixture("copy-reconcile", ":");
        let view = TerminalViewId(client.0);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("copy-mode", ["-t", &pane.to_string()]),
            )
            .expect("enter copy mode");
        wait_for_view_mode(&terminal, view, "copy-mode did not freeze", |mode| {
            matches!(mode, TerminalMode::Copy { .. })
        });
        wait_for_observed_copy_session(&shared, client);
        terminal.view_action(view, TerminalViewAction::ClearHistory);
        wait_for_root_key_table(
            &shared,
            client,
            "a live pane left the client stranded on a copy table",
        );

        terminal.view_action(view, TerminalViewAction::EnterCopyMode);
        thread::sleep(Duration::from_millis(200));
        wait_for_view_mode(
            &terminal,
            view,
            "an unclaimed frozen pane was left frozen",
            |mode| mode == TerminalMode::Live,
        );
        assert!(!shared.inner.lock().copy_sessions.contains_key(&client));
    }

    #[cfg(unix)]
    #[test]
    fn history_request_is_guarded_clamped_and_returns_self_contained_rows() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let unattached_mailbox = OutboundMailbox::new();
        let (unattached, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&unattached_mailbox),
        );
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "history-request"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let first = context.pane.expect("first pane");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("split pane");
        let second = context.pane.expect("second pane");
        shared.attach(client, session).expect("attach session");

        let terminal = Arc::clone(&shared.inner.lock().terminals[&first]);
        terminal.resize(16, 4, 8, 18);
        wait_for_terminal_dimensions(&terminal, TerminalViewId(client.0), 16, 4);
        terminal.send_text(
            "i=0; while [ $i -lt 530 ]; do printf 'ZZH%03d\\r\\n' \"$i\"; i=$((i+1)); done; printf 'ZZ_HISTORY_DONE\\r\\n'\n",
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let ready = terminal
                .latest_viewport_for(TerminalViewId(client.0))
                .is_some_and(|viewport| {
                    viewport
                        .scrollbar
                        .total
                        .saturating_sub(viewport.scrollbar.len)
                        >= 512
                });
            if ready {
                break;
            }
            assert!(Instant::now() < deadline, "terminal history did not fill");
            thread::sleep(Duration::from_millis(10));
        }

        take_reliable_messages(&mailbox);
        take_reliable_messages(&unattached_mailbox);
        shared.send_history(unattached, first, 0, 10, &unattached_mailbox);
        assert!(take_reliable_messages(&unattached_mailbox).is_empty());

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("resize-pane", ["-Z", "-t", &first.to_string()]),
            )
            .expect("zoom first pane");
        take_reliable_messages(&mailbox);
        shared.send_history(client, second, 0, 10, &mailbox);
        assert!(take_reliable_messages(&mailbox).is_empty());

        shared.send_history(client, first, 0, u32::MAX, &mailbox);
        let messages = take_reliable_messages(&mailbox);
        let chunk = messages.into_iter().find_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload:
                    EventPayload::HistoryChunk {
                        pane,
                        start,
                        total,
                        offset,
                        columns,
                        rows,
                        dictionary,
                    },
                ..
            }) => Some((pane, start, total, offset, columns, rows, dictionary)),
            _ => None,
        });
        let (pane, start, total, offset, columns, rows, dictionary) = chunk.expect("history chunk");
        assert_eq!(pane, first);
        assert_eq!(start, 0);
        assert_eq!(columns, 16);
        assert_eq!(rows.len(), usize::try_from(MAX_HISTORY_CHUNK_ROWS).unwrap());
        assert!(offset <= total);
        assert!(rows.iter().all(|row| row.len() == usize::from(columns)));
        assert!(history_chunk_text(&rows, &dictionary).contains("ZZH"));
    }

    #[test]
    fn outbound_mailbox_coalesces_command_output_and_close_cancels_it() {
        let mailbox = OutboundMailbox::new();
        let pane = PaneId(9);
        let first = command_output_test_message(pane, 1, 1);
        let second = command_output_test_message(pane, 2, 2);

        assert!(mailbox.replace_command_output(&first));
        assert!(mailbox.replace_command_output(&second));
        let close = ProtocolMessage::Event(Event {
            sequence: 3,
            payload: EventPayload::CommandOutput {
                pane,
                viewport: None,
            },
        });
        assert!(mailbox.enqueue_reliable(&close));

        let encoded = mailbox.recv().expect("reliable command-output close");
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), close);
        assert!(mailbox.state.lock().command_output.is_none());
        mailbox.close();
    }

    #[test]
    fn command_output_encoding_releases_state_lock_and_revalidates_before_enqueue() {
        let shared = Shared::new(1);
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let pane = PaneId(9);
        let terminal = Arc::new(TerminalSession::spawn_output_view(
            "command output".to_owned(),
            "fixture".to_owned(),
        ));
        shared.inner.lock().command_outputs.insert(
            client,
            CommandOutputSession {
                pane,
                terminal: Arc::clone(&terminal),
                previous_key_table: None,
            },
        );
        let viewport = terminal.latest_viewport();
        let mut encoded_without_state_lock = false;

        shared.publish_command_output_with_encoder(
            client,
            pane,
            &terminal,
            &viewport,
            |subscriber, message| {
                let removed = {
                    let mut inner = shared
                        .inner
                        .try_lock()
                        .expect("command-output encoding must not hold ServerState");
                    encoded_without_state_lock = true;
                    inner.command_outputs.remove(&client)
                }
                .expect("current command output");
                assert!(Arc::ptr_eq(&removed.terminal, &terminal));
                subscriber.encode_message(message)
            },
        );

        assert!(encoded_without_state_lock);
        assert!(!shared.inner.lock().command_outputs.contains_key(&client));
        assert!(mailbox.state.lock().command_output.is_none());
    }

    #[test]
    fn command_prompt_editor_preserves_unicode_boundaries_and_history_drafts() {
        let mut prompt = CommandPrompt::new(":".to_owned(), "α beta".to_owned(), None);
        assert_eq!(prompt.state(&[]).cursor, 6);
        assert!(prompt.delete_previous_word());
        assert_eq!(prompt.input, "α ");
        assert!(prompt.insert("界"));
        assert_eq!(prompt.state(&[]).cursor, 3);
        assert!(prompt.move_left());
        assert_eq!(prompt.state(&[]).cursor, 2);
        assert!(prompt.delete_forward());
        assert_eq!(prompt.input, "α ");

        let history = ["first".to_owned(), "second".to_owned()];
        assert!(prompt.history_up(&history));
        assert_eq!(prompt.input, "second");
        assert!(prompt.history_up(&history));
        assert_eq!(prompt.input, "first");
        assert!(prompt.history_down(&history));
        assert_eq!(prompt.input, "second");
        assert!(prompt.history_down(&history));
        assert_eq!(prompt.input, "α ");

        let printable = test_key(KeyCode::Character('x'), Modifiers::default(), Some("x"));
        assert_eq!(
            command_prompt_key(&mut prompt, &printable, true, &history),
            PromptKeyAction::Handled
        );
        assert_eq!(prompt.input, "α ");
        assert_eq!(
            command_prompt_key(&mut prompt, &printable, false, &history),
            PromptKeyAction::Updated
        );
        assert_eq!(prompt.input, "α x");
    }

    #[test]
    fn native_command_prompt_actions_persist_without_echo_and_submit_final_input() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "palette"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("command-prompt", ["-I", "list-"]),
            )
            .expect("open prompt");
        let opened = take_reliable_messages(&mailbox);
        assert!(opened.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::CommandPrompt { state: Some(state) },
                ..
            }) if state.kind == CommandPromptKind::Command && state.history.is_empty()
        )));

        let update = "rename-window café".to_owned();
        let update_cursor = u32::try_from(update.chars().count()).expect("short cursor");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::CommandPrompt {
                    action: CommandPromptAction::Update {
                        input: update.clone(),
                        cursor: update_cursor,
                    },
                },
            )
            .expect("persist prompt input");
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .all(|message| !matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::CommandPrompt { .. },
                        ..
                    })
                )),
            "local prompt updates must not echo back into InputState"
        );
        {
            let inner = shared.inner.lock();
            let prompt = &inner.command_prompts[&client];
            assert_eq!(prompt.input, update);
            assert_eq!(prompt.cursor, prompt.input.len());
        }

        shared.send_resync(client, &mailbox);
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::CommandPrompt { state: Some(state) },
                        ..
                    }) if state.input == update && state.cursor == update_cursor
                ))
        );

        let final_input = "new-window -n completed".to_owned();
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::CommandPrompt {
                    action: CommandPromptAction::Submit {
                        input: final_input.clone(),
                    },
                },
            )
            .expect("submit final prompt input");
        {
            let inner = shared.inner.lock();
            assert!(!inner.command_prompts.contains_key(&client));
            assert_eq!(inner.command_history.last(), Some(&final_input));
            assert!(
                inner
                    .engine
                    .state
                    .windows
                    .values()
                    .any(|window| window.name == "completed")
            );
        }
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::CommandPrompt { state: None },
                        ..
                    })
                ))
        );

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("command-prompt", ["-I", "draft", "rename-window -- '%%'"]),
            )
            .expect("open value prompt");
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::CommandPrompt { state: Some(state) },
                        ..
                    }) if state.kind == CommandPromptKind::Value && state.history.is_empty()
                ))
        );
        assert!(
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    InputMessage::CommandPrompt {
                        action: CommandPromptAction::Update {
                            input: "α".to_owned(),
                            cursor: 2,
                        },
                    },
                )
                .is_err()
        );
        assert_eq!(shared.inner.lock().command_prompts[&client].input, "draft");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::CommandPrompt {
                    action: CommandPromptAction::Close,
                },
            )
            .expect("close prompt");
        assert!(!shared.inner.lock().command_prompts.contains_key(&client));
    }

    #[test]
    fn command_prompt_output_is_joined_and_bounded_on_utf8_boundaries() {
        let mut output = String::new();
        assert!(!append_command_prompt_output(&mut output, "first"));
        assert!(!append_command_prompt_output(&mut output, "second"));
        assert_eq!(output, "first\nsecond");

        let content_limit = MAX_COMMAND_PROMPT_OUTPUT_BYTES
            .saturating_sub(COMMAND_PROMPT_OUTPUT_TRUNCATED.len() + 1);
        let mut output = "x".repeat(content_limit - 1);
        assert!(append_command_prompt_output(&mut output, "界"));
        assert_eq!(output.len(), content_limit - 1);
        output.push('\n');
        output.push_str(COMMAND_PROMPT_OUTPUT_TRUNCATED);
        assert!(output.len() <= MAX_COMMAND_PROMPT_OUTPUT_BYTES);
        assert!(output.is_char_boundary(output.len()));
    }

    #[test]
    fn command_prompt_rejects_command_only_clients() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        assert!(matches!(
            shared.execute(
                ClientId(7),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("command-prompt", [] as [&str; 0]),
            ),
            Err(DaemonError::Server(ServerError::InvalidCommand(message)))
                if message.contains("interactive client")
        ));
    }

    #[test]
    fn choose_tree_model_flattens_mixed_panes_and_supports_collapse_and_search() {
        let mut state = MuxState::default();
        let (session, window, terminal) = state.create_session("work").expect("session");
        let browser = state
            .split_pane(
                terminal,
                zz_protocol::Axis::Horizontal,
                PaneKind::Browser(zz_protocol::BrowserDescriptor::single(
                    "https://example.com/docs".to_owned(),
                    "default".to_owned(),
                )),
            )
            .expect("browser pane");
        let mut chooser =
            ChooseTreeSession::new(ChooseTreeKind::Panes, terminal, &state, Some(session))
                .expect("chooser");

        assert_eq!(chooser.rendered.items.len(), 4);
        assert!(chooser.rendered.items.iter().any(|item| {
            item.target == ChooseTreeTarget::Pane(browser)
                && item.pane_kind == Some(ChooseTreePaneKind::Browser)
        }));
        assert_eq!(
            chooser.rendered.items[usize::try_from(chooser.rendered.selected).unwrap()].target,
            ChooseTreeTarget::Pane(terminal)
        );

        chooser
            .apply(ChooseTreeAction::Select(0), &state, Some(session))
            .expect("select session");
        chooser
            .apply(ChooseTreeAction::Collapse, &state, Some(session))
            .expect("collapse session");
        assert_eq!(chooser.rendered.items.len(), 1);
        assert_eq!(
            chooser.rendered.items[0].target,
            ChooseTreeTarget::Session(session)
        );

        chooser
            .apply(ChooseTreeAction::Expand, &state, Some(session))
            .expect("expand session");
        chooser
            .apply(ChooseTreeAction::Select(1), &state, Some(session))
            .expect("select window");
        chooser
            .apply(ChooseTreeAction::Expand, &state, Some(session))
            .expect("expand window");
        chooser
            .apply(
                ChooseTreeAction::SearchStart { reverse: false },
                &state,
                Some(session),
            )
            .expect("start search");
        chooser
            .apply(
                ChooseTreeAction::SearchAppend("example.com".to_owned()),
                &state,
                Some(session),
            )
            .expect("search browser");
        assert_eq!(chooser.selected, Some(ChooseTreeTarget::Pane(browser)));
        assert_eq!(state.windows[&window].panes.len(), 2);
    }

    #[test]
    fn choose_buffer_model_bounds_metadata_and_searches_server_side_contents() {
        let mut state = MuxState::default();
        let (_, _, pane) = state.create_session("work").expect("session");
        let buffers = vec![
            PasteBuffer {
                name: "newest".to_owned(),
                data: Arc::from(b"first line\nsecond line".as_slice()),
                created: UNIX_EPOCH + Duration::from_secs(20),
                automatic: false,
                utf8: true,
            },
            PasteBuffer {
                name: "older".to_owned(),
                data: Arc::from(
                    format!(
                        "{}hidden needle",
                        "x".repeat(MAX_CHOOSE_BUFFER_PREVIEW_BYTES)
                    )
                    .into_bytes(),
                ),
                created: UNIX_EPOCH + Duration::from_secs(10),
                automatic: false,
                utf8: true,
            },
        ];
        let mut chooser = ChooseBufferSession::new(pane, &state, &buffers)
            .expect("valid pane")
            .expect("nonempty chooser");

        assert_eq!(chooser.rendered.items.len(), 2);
        assert_eq!(chooser.rendered.items[0].name, "newest");
        assert!(chooser.rendered.items[0].preview.contains('⏎'));
        assert!(
            chooser.rendered.items[1].preview.len() <= MAX_CHOOSE_BUFFER_PREVIEW_BYTES,
            "wire previews must remain bounded"
        );
        chooser
            .apply(ChooseBufferAction::SearchStart { reverse: false }, &buffers)
            .expect("start search");
        chooser
            .apply(
                ChooseBufferAction::SearchAppend("NEEDLE".to_owned()),
                &buffers,
            )
            .expect("search full buffer");
        assert_eq!(chooser.selected.as_deref(), Some("older"));
        assert!(matches!(
            chooser
                .apply(ChooseBufferAction::Paste, &buffers)
                .expect("paste selected"),
            ChooseBufferResult::Paste(name) if name == "older"
        ));
    }

    #[test]
    fn daemon_choose_buffer_searches_pastes_and_deletes_across_native_surfaces() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-browser", ["https://example.com"]),
            )
            .expect("browser pane");
        let browser = context.pane.expect("browser pane");
        shared.attach(client, session).expect("attach session");
        {
            let mut inner = shared.inner.lock();
            insert_paste_buffer(&mut inner, Some("binary"), "buffer", b"a\nb\0\xff".to_vec())
                .expect("binary buffer");
        }
        take_reliable_messages(&mailbox);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new(
                    "paste-buffer",
                    ["-b", "binary", "-t", &browser.to_string()],
                ),
            )
            .expect("safe binary browser paste");
        assert_eq!(take_browser_literals(&mailbox, browser), ["a\rb^@M^?"]);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new(
                    "paste-buffer",
                    ["-r", "-b", "binary", "-t", &browser.to_string()],
                ),
            )
            .expect("raw-newline browser paste");
        assert_eq!(take_browser_literals(&mailbox, browser), ["a\nb^@M^?"]);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new(
                    "paste-buffer",
                    ["-s", "::", "-b", "binary", "-t", &browser.to_string()],
                ),
            )
            .expect("custom-separator browser paste");
        assert_eq!(take_browser_literals(&mailbox, browser), ["a::b^@M^?"]);
        let error = shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new(
                    "paste-buffer",
                    ["-d", "-S", "-b", "binary", "-t", &browser.to_string()],
                ),
            )
            .expect_err("binary browser paste");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message.contains("not valid UTF-8 for a browser pane")
        ));
        assert!(
            shared
                .inner
                .lock()
                .paste_buffers
                .iter()
                .any(|buffer| buffer.name == "binary"),
            "a rejected delete-and-paste must leave the buffer intact"
        );
        assert!(
            !take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::BrowserCommand { .. },
                        ..
                    })
                )),
            "a rejected binary paste must not partially reach Chromium"
        );
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("delete-buffer", ["-b", "binary"]),
            )
            .expect("delete binary fixture");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-buffer", ["-b", "older", "alpha payload"]),
            )
            .expect("older buffer");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-buffer", ["-b", "newest", "beta payload"]),
            )
            .expect("newer buffer");
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("choose-buffer", ["-Z"]),
            )
            .expect("open buffer chooser");
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::ChooseBuffer { state: Some(state) },
                        ..
                    }) if state.items.len() == 2 && state.items[0].name == "newest"
                ))
        );
        shared.send_resync(client, &mailbox);
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::ChooseBuffer { state: Some(state) },
                        ..
                    }) if state.items.len() == 2
                ))
        );
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Text {
                    pane: browser,
                    text: "must not leak".to_owned(),
                },
            )
            .expect("block covered browser input");
        assert!(
            !take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::BrowserCommand { .. },
                        ..
                    })
                ))
        );
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ChooseBuffer {
                    action: ChooseBufferAction::SearchStart { reverse: false },
                },
            )
            .expect("start search");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ChooseBuffer {
                    action: ChooseBufferAction::SearchAppend("alpha".to_owned()),
                },
            )
            .expect("search older buffer");
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::ChooseBufferUpdate { selected: 1, .. },
                        ..
                    })
                ))
        );
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ChooseBuffer {
                    action: ChooseBufferAction::Paste,
                },
            )
            .expect("paste into browser");
        let pasted = take_reliable_messages(&mailbox);
        assert!(pasted.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::ChooseBuffer { state: None },
                ..
            })
        )));
        assert!(pasted.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::BrowserCommand {
                    pane,
                    command: BrowserCommand::SendKeys(keys),
                },
                ..
            }) if *pane == browser
                && keys == &[zz_protocol::KeyToken::Literal("alpha payload".to_owned())]
        )));

        for remaining in [1, 0] {
            shared
                .execute(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    &CommandInvocation::new("choose-buffer", ["-Z"]),
                )
                .expect("reopen buffer chooser");
            take_reliable_messages(&mailbox);
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    InputMessage::ChooseBuffer {
                        action: ChooseBufferAction::Delete,
                    },
                )
                .expect("delete selected buffer");
            let messages = take_reliable_messages(&mailbox);
            assert!(messages.iter().any(|message| matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ChooseBuffer { state },
                    ..
                }) if state.as_ref().map_or(0, |state| state.items.len()) == remaining
            )));
        }
        assert!(shared.inner.lock().paste_buffers.is_empty());
        assert!(!shared.inner.lock().choose_buffers.contains_key(&client));
    }

    #[test]
    fn display_panes_model_uses_pane_order_and_tmux_selection_keys() {
        let mut engine = MuxEngine::default();
        let (_, window, source) = engine.state.create_session("work").expect("session");
        for _ in 1..=36 {
            engine
                .state
                .split_pane_with(
                    source,
                    zz_protocol::Axis::Horizontal,
                    PaneKind::Terminal,
                    zz_mux::SplitPlacement {
                        size: zz_mux::SplitSize::Cells(1),
                        ..zz_mux::SplitPlacement::default()
                    },
                )
                .expect("split pane");
        }
        let active = engine.state.windows[&window].pane_order()[36];
        engine.state.select_pane(active).expect("select last pane");

        let (_, actual_window, overlay) =
            build_display_panes_state(&engine, active, 1_000).expect("pane indicators");
        assert_eq!(actual_window, window);
        assert_eq!(overlay.indicators.len(), 37);
        assert_eq!(overlay.indicators[0].select_key, b'0');
        assert_eq!(overlay.indicators[9].select_key, b'9');
        assert_eq!(overlay.indicators[10].select_key, b'a');
        assert_eq!(overlay.indicators[35].select_key, b'z');
        assert_eq!(overlay.indicators[36].select_key, 0);
        assert!(overlay.indicators[36].active());

        engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new("set-window-option", ["-g", "pane-base-index", "1"]),
            )
            .expect("one-based pane indicators");
        let (_, _, overlay) =
            build_display_panes_state(&engine, active, 1_000).expect("one-based indicators");
        assert_eq!(overlay.indicators[0].index, 1);
        assert_eq!(overlay.indicators[0].select_key, b'1');
        assert_eq!(overlay.indicators[8].index, 9);
        assert_eq!(overlay.indicators[8].select_key, b'9');
        assert_eq!(overlay.indicators[9].index, 10);
        assert_eq!(overlay.indicators[9].select_key, b'a');
        assert_eq!(overlay.indicators[35].index, 36);
        assert_eq!(overlay.indicators[35].select_key, 0);
    }

    #[test]
    fn daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let window = context.window.expect("window");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("split pane");
        let panes = shared.inner.lock().engine.state.windows[&window]
            .pane_order()
            .to_vec();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let settled = {
                let inner = shared.inner.lock();
                panes.iter().all(|pane| {
                    inner.engine.pane_runtime_facts(*pane).is_some_and(|facts| {
                        !facts.current_command.is_empty()
                            && facts.pid.is_some()
                            && !facts.tty.is_empty()
                    })
                })
            };
            if settled {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "pane runtime facts did not settle before split resize"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let split = {
            let inner = shared.inner.lock();
            let mut splits = Vec::new();
            inner.engine.state.windows[&window]
                .layout
                .project()
                .splits(&mut splits);
            splits[0]
        };
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ResizeSplit {
                    window,
                    split,
                    ratio_basis_points: 6_750,
                },
            )
            .expect("resize split");
        {
            let inner = shared.inner.lock();
            let LayoutNode::Split { ratio, .. } =
                inner.engine.state.windows[&window].layout.project()
            else {
                panic!("window should remain split");
            };
            assert!((ratio - 53.0 / 79.0).abs() < f32::EPSILON);
        }
        assert!(take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) if matches!(
                    snapshot.sessions[0].windows[0].layout,
                    LayoutNode::Split { ratio, .. }
                        if (ratio - 53.0 / 79.0).abs() < f32::EPSILON
                )
            )
        }));

        let generation = shared.inner.lock().engine.state.generation();
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ResizeSplit {
                    window,
                    split,
                    ratio_basis_points: 6_751,
                },
            )
            .expect("cell-snapped resize");
        assert_eq!(shared.inner.lock().engine.state.generation(), generation);
        assert!(take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) if snapshot.generation == generation
            )
        }));

        let command_error = shared
            .input(
                client,
                ClientKind::Command,
                &mut context,
                InputMessage::ResizeSplit {
                    window,
                    split,
                    ratio_basis_points: 5_000,
                },
            )
            .expect_err("command clients cannot drag");
        assert!(matches!(
            command_error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message.contains("interactive client")
        ));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("resize-pane", ["-Z"]),
            )
            .expect("zoom pane");
        let zoom_error = shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ResizeSplit {
                    window,
                    split,
                    ratio_basis_points: 5_000,
                },
            )
            .expect_err("zoomed windows cannot be dragged");
        assert!(matches!(
            zoom_error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message.contains("zoomed")
        ));
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("resize-pane", ["-Z"]),
            )
            .expect("unzoom pane");

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-window", [] as [&str; 0]),
            )
            .expect("new active window");
        let stale_error = shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ResizeSplit {
                    window,
                    split,
                    ratio_basis_points: 5_000,
                },
            )
            .expect_err("inactive windows cannot be dragged");
        assert!(matches!(
            stale_error,
            DaemonError::Server(ServerError::InvalidTarget(message))
                if message.contains("not active")
        ));
    }

    #[test]
    fn multi_client_attach_uses_latest_active_geometry_and_keeps_independent_views() {
        let shared = Arc::new(Shared::new(1));
        let first_mailbox = OutboundMailbox::new();
        let second_mailbox = OutboundMailbox::new();
        let third_mailbox = OutboundMailbox::new();
        let (first_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&first_mailbox),
        );
        let (second_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&second_mailbox),
        );
        let (third_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&third_mailbox),
        );
        let fixture = std::iter::once("zz-terminal-ready".to_owned())
            .chain((0..100).map(|line| format!("slice1-line-{line}")))
            .collect::<Vec<_>>()
            .join("\n");
        let (session, pane, terminal) = output_view_session_fixture(&shared, "shared", fixture);

        shared
            .attach(first_client, session)
            .expect("attach first client");
        shared
            .attach(second_client, session)
            .expect("attach second client");
        assert_eq!(
            shared.inner.lock().attached.get(&session),
            Some(&BTreeSet::from([first_client, second_client]))
        );
        wait_for_viewport(
            &terminal,
            TerminalViewId(first_client.0),
            "first attached terminal view never became available",
            |viewport| viewport_text(viewport).contains("zz-terminal-ready"),
        );
        wait_for_viewport(
            &terminal,
            TerminalViewId(second_client.0),
            "second attached terminal view never became available",
            |viewport| viewport_text(viewport).contains("zz-terminal-ready"),
        );
        shared.send_resync(first_client, &first_mailbox);
        shared.send_resync(second_client, &second_mailbox);

        let mut first_state = TerminalTestState::default();
        let mut second_state = TerminalTestState::default();
        wait_for_mailbox_terminal(&first_mailbox, &mut first_state, |message, _| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::TerminalViewport { pane: target, .. },
                    ..
                }) if *target == pane
            )
        });
        wait_for_mailbox_terminal(&second_mailbox, &mut second_state, |message, _| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::TerminalViewport { pane: target, .. },
                    ..
                }) if *target == pane
            )
        });

        let mut first_context = ExecutionContext::default();
        let mut second_context = ExecutionContext::default();
        shared
            .input(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                InputMessage::ResizeTerminal {
                    pane,
                    columns: 200,
                    rows: 30,
                    cell_width_px: 8,
                    cell_height_px: 18,
                },
            )
            .expect("first geometry");
        shared
            .input(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                InputMessage::ResizeTerminal {
                    pane,
                    columns: 100,
                    rows: 60,
                    cell_width_px: 7,
                    cell_height_px: 16,
                },
            )
            .expect("second geometry");
        let effective = terminal_resize_for_pane(&shared.inner.lock(), pane)
            .expect("effective shared geometry")
            .1;
        assert_eq!(
            effective,
            TerminalGeometry {
                columns: 200,
                rows: 30,
                cell_width_px: 8,
                cell_height_px: 18,
            },
            "the lowest client id owns one whole geometry before either client is active"
        );
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(first_client)
        );
        wait_for_terminal_dimensions(&terminal, TerminalViewId(first_client.0), 200, 30);

        shared
            .input(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                InputMessage::Text {
                    pane,
                    text: "zz-input\r".to_owned(),
                },
            )
            .expect("second client types");
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(second_client)
        );
        assert_eq!(
            terminal_resize_for_pane(&shared.inner.lock(), pane)
                .expect("second client geometry")
                .1,
            TerminalGeometry {
                columns: 100,
                rows: 60,
                cell_width_px: 7,
                cell_height_px: 16,
            }
        );
        wait_for_terminal_dimensions(&terminal, TerminalViewId(first_client.0), 100, 60);

        shared
            .input(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                InputMessage::Text {
                    pane,
                    text: "zz-input\r".to_owned(),
                },
            )
            .expect("first client types again");
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(first_client)
        );
        wait_for_terminal_dimensions(&terminal, TerminalViewId(first_client.0), 200, 30);

        let mouse_input = |phase, button| {
            TerminalMouseInput::new(
                phase,
                button,
                PointerCellEvent {
                    column: 0,
                    row: 0,
                    click_count: 1,
                    rectangle: false,
                },
                4,
                8,
                800,
                600,
                8,
                16,
                Modifiers::default(),
                false,
            )
        };
        shared
            .input(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                InputMessage::TerminalView {
                    pane,
                    action: TerminalViewAction::Mouse(mouse_input(
                        TerminalMousePhase::Motion,
                        None,
                    )),
                },
            )
            .expect("second client moves the mouse");
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(first_client),
            "mouse motion must not transfer geometry ownership"
        );

        for focused in [true, false] {
            shared
                .input(
                    second_client,
                    ClientKind::Interactive,
                    &mut second_context,
                    InputMessage::TerminalView {
                        pane,
                        action: TerminalViewAction::Focus(focused),
                    },
                )
                .expect("second client changes focus");
            assert_eq!(
                terminal_geometry_owner(&shared.inner.lock(), pane),
                Some(first_client),
                "focus({focused}) must not transfer geometry ownership: a window-manager \
                 event is not someone typing, and letting it claim the pty makes every \
                 focus change resize the terminal out from under the other client"
            );
        }

        shared
            .input(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                InputMessage::TerminalView {
                    pane,
                    action: TerminalViewAction::Mouse(mouse_input(
                        TerminalMousePhase::Press,
                        Some(TerminalMouseButton::Left),
                    )),
                },
            )
            .expect("second client presses the mouse");
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(second_client),
            "mouse press must transfer geometry ownership"
        );
        wait_for_terminal_dimensions(&terminal, TerminalViewId(first_client.0), 100, 60);

        wait_for_mailbox_terminal(&first_mailbox, &mut first_state, |_, state| {
            state
                .viewports
                .get(&pane)
                .is_some_and(|viewport| viewport.columns == 100 && viewport.rows == 60)
        });
        wait_for_mailbox_terminal(&second_mailbox, &mut second_state, |_, state| {
            state
                .viewports
                .get(&pane)
                .is_some_and(|viewport| viewport.columns == 100 && viewport.rows == 60)
        });

        wait_for_viewport(
            &terminal,
            TerminalViewId(first_client.0),
            "output fixture did not retain enough rows to test independent views",
            |viewport| {
                viewport
                    .scrollbar
                    .total
                    .saturating_sub(viewport.scrollbar.len)
                    >= 10
            },
        );
        for view in [
            TerminalViewId(first_client.0),
            TerminalViewId(second_client.0),
        ] {
            terminal.view_action(view, zz_terminal::TerminalViewAction::ScrollBottom);
            wait_for_viewport(
                &terminal,
                view,
                "terminal view did not reach the bottom before the isolation check",
                |viewport| {
                    viewport
                        .scrollbar
                        .offset
                        .saturating_add(viewport.scrollbar.len)
                        == viewport.scrollbar.total
                },
            );
        }
        wait_for_mailbox_terminal(&first_mailbox, &mut first_state, |_, state| {
            state.viewports.get(&pane).is_some_and(|viewport| {
                viewport
                    .scrollbar
                    .offset
                    .saturating_add(viewport.scrollbar.len)
                    == viewport.scrollbar.total
            })
        });
        wait_for_mailbox_terminal(&second_mailbox, &mut second_state, |_, state| {
            state.viewports.get(&pane).is_some_and(|viewport| {
                viewport
                    .scrollbar
                    .offset
                    .saturating_add(viewport.scrollbar.len)
                    == viewport.scrollbar.total
            })
        });
        let second_before = second_state.viewports[&pane].view_generation;
        terminal.view_action(
            TerminalViewId(first_client.0),
            zz_terminal::TerminalViewAction::ScrollLines(-10),
        );
        wait_for_mailbox_terminal(&first_mailbox, &mut first_state, |_, state| {
            state.viewports.get(&pane).is_some_and(|viewport| {
                viewport
                    .scrollbar
                    .offset
                    .saturating_add(viewport.scrollbar.len)
                    < viewport.scrollbar.total
            })
        });
        wait_for_mailbox_terminal(&second_mailbox, &mut second_state, |_, state| {
            state.viewports.get(&pane).is_some_and(|viewport| {
                viewport.view_generation > second_before
                    && viewport
                        .scrollbar
                        .offset
                        .saturating_add(viewport.scrollbar.len)
                        == viewport.scrollbar.total
            })
        });
        assert!(
            first_state.viewports[&pane]
                .scrollbar
                .offset
                .saturating_add(first_state.viewports[&pane].scrollbar.len)
                < first_state.viewports[&pane].scrollbar.total,
            "the scrolled client must have an independent viewport"
        );
        assert_eq!(
            second_state.viewports[&pane]
                .scrollbar
                .offset
                .saturating_add(second_state.viewports[&pane].scrollbar.len),
            second_state.viewports[&pane].scrollbar.total,
        );

        shared.detach(second_client);
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(first_client),
            "the surviving viewer owns the geometry after the active owner detaches"
        );
        wait_for_mailbox_terminal(&first_mailbox, &mut first_state, |_, state| {
            state
                .viewports
                .get(&pane)
                .is_some_and(|viewport| viewport.columns == 200 && viewport.rows == 30)
        });

        shared
            .attach(third_client, session)
            .expect("attach inactive third client");
        let mut third_context = ExecutionContext::default();
        shared
            .input(
                third_client,
                ClientKind::Interactive,
                &mut third_context,
                InputMessage::ResizeTerminal {
                    pane,
                    columns: 60,
                    rows: 20,
                    cell_width_px: 6,
                    cell_height_px: 14,
                },
            )
            .expect("third client geometry");
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(first_client),
            "attach and geometry reports do not displace the last active viewer"
        );
        assert_eq!(
            terminal_resize_for_pane(&shared.inner.lock(), pane)
                .expect("active first client geometry")
                .1,
            TerminalGeometry {
                columns: 200,
                rows: 30,
                cell_width_px: 8,
                cell_height_px: 18,
            }
        );
        wait_for_terminal_dimensions(&terminal, TerminalViewId(first_client.0), 200, 30);

        let inner = shared.inner.lock();
        assert_eq!(
            inner.attached.get(&session),
            Some(&BTreeSet::from([first_client, third_client]))
        );
        assert!(
            inner
                .terminal_geometries
                .values()
                .all(|geometries| !geometries.contains_key(&second_client))
        );
    }

    #[test]
    fn detach_yields_geometry_ownership_until_the_client_types_again() {
        let shared = Arc::new(Shared::new(1));
        let first_mailbox = OutboundMailbox::new();
        let second_mailbox = OutboundMailbox::new();
        let (first_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&first_mailbox),
        );
        let (second_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&second_mailbox),
        );
        let mut setup = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut setup,
                &CommandInvocation::new("new-session", ["-s", "yield"]),
            )
            .expect("create shared session");
        let session = setup.session.expect("shared session");
        let pane = setup.pane.expect("shared pane");

        let geometry = |columns, rows| InputMessage::ResizeTerminal {
            pane,
            columns,
            rows,
            cell_width_px: 8,
            cell_height_px: 18,
        };
        let typing = || InputMessage::Text {
            pane,
            text: "true\n".to_owned(),
        };

        shared
            .attach(first_client, session)
            .expect("attach first client");
        shared
            .attach(second_client, session)
            .expect("attach second client");
        let mut first_context = ExecutionContext::default();
        let mut second_context = ExecutionContext::default();
        shared
            .input(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                geometry(200, 30),
            )
            .expect("first geometry");
        shared
            .input(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                geometry(100, 60),
            )
            .expect("second geometry");
        shared
            .input(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                typing(),
            )
            .expect("second client types");
        shared
            .input(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                typing(),
            )
            .expect("first client types");
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(first_client)
        );

        shared.detach(first_client);
        {
            let inner = shared.inner.lock();
            assert!(
                !inner
                    .client_terminal_input_sequences
                    .contains_key(&first_client)
            );
            assert_eq!(terminal_geometry_owner(&inner, pane), Some(second_client));
        }

        for stray in [geometry(300, 40), typing()] {
            let error = shared
                .input(
                    first_client,
                    ClientKind::Interactive,
                    &mut first_context,
                    stray,
                )
                .expect_err("detached clients cannot report input");
            assert!(matches!(
                error,
                DaemonError::Server(ServerError::PaneNotAttached(target)) if target == pane
            ));
        }
        {
            let inner = shared.inner.lock();
            assert!(!inner.terminal_geometries[&pane].contains_key(&first_client));
            assert!(
                !inner
                    .client_terminal_input_sequences
                    .contains_key(&first_client)
            );
            assert_eq!(terminal_geometry_owner(&inner, pane), Some(second_client));
        }

        shared
            .attach(first_client, session)
            .expect("re-attach first client");
        shared
            .input(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                geometry(200, 30),
            )
            .expect("first geometry after re-attach");
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(second_client),
            "re-attaching does not reclaim ownership with a pre-detach sequence"
        );

        shared
            .input(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                typing(),
            )
            .expect("first client types after re-attach");
        assert_eq!(
            terminal_geometry_owner(&shared.inner.lock(), pane),
            Some(first_client),
            "typing again reclaims ownership"
        );
    }

    #[test]
    fn unattached_client_key_input_builds_no_key_state_and_runs_no_binding() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut setup = ExecutionContext::default();
        for command in [
            CommandInvocation::new("set-option", ["-g", "prefix", "C-a"]),
            CommandInvocation::new("bind-key", ["c", "new-window"]),
            CommandInvocation::new("new-session", ["-s", "stray"]),
        ] {
            shared
                .execute(
                    ClientId(u64::MAX),
                    ClientKind::Command,
                    &mut setup,
                    &command,
                )
                .expect("prepare stray session");
        }
        let pane = setup.pane.expect("stray pane");
        let windows = shared.inner.lock().engine.state.windows.len();
        take_reliable_messages(&mailbox);

        let mut context = ExecutionContext::default();
        let inputs = [
            InputMessage::Key {
                pane,
                input: test_key(
                    KeyCode::Character('a'),
                    Modifiers::new(false, true, false, false),
                    None,
                ),
                text_follows: false,
            },
            InputMessage::Key {
                pane,
                input: test_key(KeyCode::Character('c'), Modifiers::default(), Some("c")),
                text_follows: true,
            },
            InputMessage::Text {
                pane,
                text: "c".to_owned(),
            },
        ];
        for input in inputs {
            let error = shared
                .input(client, ClientKind::Interactive, &mut context, input)
                .expect_err("clients without an attached session cannot send input");
            assert!(matches!(
                error,
                DaemonError::Server(ServerError::PaneNotAttached(target)) if target == pane
            ));
        }

        {
            let inner = shared.inner.lock();
            assert!(!inner.key_engines.contains_key(&client));
            assert!(!inner.swallowed_keys.contains_key(&client));
            assert!(!inner.prefix_armed.contains(&client));
            assert_eq!(
                inner.engine.state.windows.len(),
                windows,
                "a prefix binding must not run for an unattached client"
            );
        }
        assert!(!take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::PrefixArmed { .. },
                    ..
                })
            )
        }));
    }

    #[test]
    fn multi_client_presence_stamps_names_focus_self_and_nameless_fallback() {
        let shared = Arc::new(Shared::new(1));
        let desktop_mailbox = OutboundMailbox::new();
        let laptop_mailbox = OutboundMailbox::new();
        let nameless_mailbox = OutboundMailbox::new();
        let (desktop, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("desktop".to_owned()),
            None,
            Arc::clone(&desktop_mailbox),
        );
        let (laptop, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("laptop".to_owned()),
            None,
            Arc::clone(&laptop_mailbox),
        );
        let (nameless, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&nameless_mailbox),
        );

        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "presence", "-n", "agent"]),
            )
            .expect("create presence session");
        let session = context.session.expect("presence session");
        let agent_window = context.window.expect("agent window");
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-window", ["-n", "logs"]),
            )
            .expect("create logs window");
        let logs_window = context.window.expect("logs window");
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("select-window", ["-t", &agent_window.to_string()]),
            )
            .expect("restore agent default");

        shared.attach(desktop, session).expect("attach desktop");
        shared.attach(laptop, session).expect("attach laptop");
        take_reliable_messages(&desktop_mailbox);
        take_reliable_messages(&laptop_mailbox);

        let mut laptop_context = context.clone();
        shared
            .execute(
                laptop,
                ClientKind::Interactive,
                &mut laptop_context,
                &CommandInvocation::new("select-window", ["-t", &logs_window.to_string()]),
            )
            .expect("laptop focuses logs");

        let desktop_messages = take_reliable_messages(&desktop_mailbox);
        let laptop_messages = take_reliable_messages(&laptop_mailbox);
        let desktop_snapshot = latest_reliable_snapshot(&desktop_messages);
        let laptop_snapshot = latest_reliable_snapshot(&laptop_messages);
        let desktop_session = desktop_snapshot
            .sessions
            .iter()
            .find(|candidate| candidate.id == session)
            .expect("desktop session snapshot");
        let laptop_session = laptop_snapshot
            .sessions
            .iter()
            .find(|candidate| candidate.id == session)
            .expect("laptop session snapshot");

        assert_eq!(desktop_snapshot.focused_window, Some(agent_window));
        assert_eq!(laptop_snapshot.focused_window, Some(logs_window));
        assert!(desktop_session.viewers.iter().any(|viewer| {
            viewer.name == "desktop" && viewer.window == agent_window && viewer.is_self
        }));
        assert!(desktop_session.viewers.iter().any(|viewer| {
            viewer.name == "laptop" && viewer.window == logs_window && !viewer.is_self
        }));
        assert!(laptop_session.viewers.iter().any(|viewer| {
            viewer.name == "desktop" && viewer.window == agent_window && !viewer.is_self
        }));
        assert!(laptop_session.viewers.iter().any(|viewer| {
            viewer.name == "laptop" && viewer.window == logs_window && viewer.is_self
        }));

        let nameless_snapshot = shared
            .attach(nameless, session)
            .expect("attach nameless client");
        shared.publish_snapshot();
        let fallback = format!("device-{}", nameless.0);
        assert!(nameless_snapshot.sessions[0].viewers.iter().any(|viewer| {
            viewer.name == fallback && viewer.window == logs_window && viewer.is_self
        }));
        let desktop_messages = take_reliable_messages(&desktop_mailbox);
        assert!(
            latest_reliable_snapshot(&desktop_messages).sessions[0]
                .viewers
                .iter()
                .any(|viewer| viewer.name == fallback && !viewer.is_self)
        );
    }

    #[test]
    fn detach_client_command_notifies_its_issuer() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("terminal".to_owned()),
            None,
            Arc::clone(&mailbox),
        );
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "work"]),
            )
            .expect("create session");
        let session = context.session.expect("created session");
        shared
            .attach(client, session)
            .expect("attach terminal client");
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("detach-client", [] as [&str; 0]),
            )
            .expect("detach through mux command");

        assert!(take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Detached { session: detached, by: None },
                    ..
                }) if *detached == session
            )
        }));
        assert!(
            shared
                .inner
                .lock()
                .attached
                .values()
                .all(|clients| !clients.contains(&client))
        );
    }

    #[test]
    fn detach_client_dash_a_kicks_every_peer_and_keeps_the_caller() {
        let shared = Arc::new(Shared::new(1));
        let mine = OutboundMailbox::new();
        let theirs = OutboundMailbox::new();
        let (caller, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("desktop".to_owned()),
            None,
            Arc::clone(&mine),
        );
        let (peer, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("laptop".to_owned()),
            None,
            Arc::clone(&theirs),
        );
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "work"]),
            )
            .expect("create session");
        let session = context.session.expect("created session");
        shared.attach(caller, session).expect("attach caller");
        shared.attach(peer, session).expect("attach peer");
        take_reliable_messages(&mine);
        take_reliable_messages(&theirs);

        shared
            .execute(
                caller,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("detach-client", ["-a"]),
            )
            .expect("detach every other client");

        assert!(take_reliable_messages(&theirs).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Detached { session: detached, by: Some(by) },
                    ..
                }) if *detached == session && by == "desktop"
            )
        }));
        let inner = shared.inner.lock();
        assert!(
            inner.attached[&session].contains(&caller),
            "-a must never detach its own client"
        );
        assert!(!inner.attached[&session].contains(&peer));
    }

    #[test]
    fn detach_client_dash_s_clears_the_session_including_the_caller() {
        let shared = Arc::new(Shared::new(1));
        let mine = OutboundMailbox::new();
        let theirs = OutboundMailbox::new();
        let (caller, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("desktop".to_owned()),
            None,
            Arc::clone(&mine),
        );
        let (peer, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("laptop".to_owned()),
            None,
            Arc::clone(&theirs),
        );
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "work"]),
            )
            .expect("create session");
        let session = context.session.expect("created session");
        shared.attach(caller, session).expect("attach caller");
        shared.attach(peer, session).expect("attach peer");
        take_reliable_messages(&mine);
        take_reliable_messages(&theirs);

        shared
            .execute(
                caller,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("detach-client", ["-s", "work"]),
            )
            .expect("detach the session");

        assert!(take_reliable_messages(&mine).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Detached { session: detached, by: None },
                    ..
                }) if *detached == session
            )
        }));
        let inner = shared.inner.lock();
        assert!(
            inner.attached.get(&session).is_none_or(BTreeSet::is_empty),
            "-s empties the session"
        );
    }

    #[test]
    fn steal_attach_notifies_peer_and_releases_focus_views_and_geometry() {
        let shared = Arc::new(Shared::new(1));
        let desktop_mailbox = OutboundMailbox::new();
        let laptop_mailbox = OutboundMailbox::new();
        let (desktop, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("desktop".to_owned()),
            None,
            Arc::clone(&desktop_mailbox),
        );
        let (laptop, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("laptop".to_owned()),
            None,
            Arc::clone(&laptop_mailbox),
        );
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "shared"]),
            )
            .expect("create shared session");
        let session = context.session.expect("shared session");
        let window = context.window.expect("shared window");
        let pane = context.pane.expect("shared pane");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);

        shared.attach(desktop, session).expect("attach desktop");
        shared.attach(laptop, session).expect("plain attach laptop");
        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner.attached.get(&session),
                Some(&BTreeSet::from([desktop, laptop])),
                "plain attach must keep both clients attached"
            );
        }
        assert!(
            take_reliable_messages(&desktop_mailbox)
                .iter()
                .all(|message| !matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::Detached { .. },
                        ..
                    })
                )),
            "plain attach must not detach its peer"
        );

        let mut desktop_context = context.clone();
        let mut laptop_context = context.clone();
        shared
            .input(
                desktop,
                ClientKind::Interactive,
                &mut desktop_context,
                InputMessage::ResizeTerminal {
                    pane,
                    columns: 80,
                    rows: 24,
                    cell_width_px: 8,
                    cell_height_px: 16,
                },
            )
            .expect("desktop geometry");
        shared
            .input(
                laptop,
                ClientKind::Interactive,
                &mut laptop_context,
                InputMessage::ResizeTerminal {
                    pane,
                    columns: 140,
                    rows: 40,
                    cell_width_px: 9,
                    cell_height_px: 18,
                },
            )
            .expect("laptop geometry");
        {
            let mut inner = shared.inner.lock();
            inner.focused_windows.insert(desktop, window);
            let effective = terminal_resize_for_pane(&inner, pane)
                .expect("lowest-id owner geometry")
                .1;
            assert_eq!((effective.columns, effective.rows), (80, 24));
        }
        take_reliable_messages(&desktop_mailbox);
        take_reliable_messages(&laptop_mailbox);

        shared
            .execute(
                laptop,
                ClientKind::Interactive,
                &mut laptop_context,
                &CommandInvocation::new("attach-session", ["-d", "-t", "shared"]),
            )
            .expect("laptop steal-attaches");

        assert!(
            take_reliable_messages(&desktop_mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload:
                            EventPayload::Detached {
                                session: detached,
                                by: Some(device),
                            },
                        ..
                    }) if *detached == session && device == "laptop"
                )),
            "victim must receive the named detach event"
        );
        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner.attached.get(&session),
                Some(&BTreeSet::from([laptop]))
            );
            assert!(!inner.focused_windows.contains_key(&desktop));
            assert!(!inner.visible_terminals.contains_key(&desktop));
            assert!(inner.subscribers.contains_key(&desktop));
            assert!(
                inner
                    .terminal_geometries
                    .values()
                    .all(|geometries| !geometries.contains_key(&desktop))
            );
            let effective = terminal_resize_for_pane(&inner, pane)
                .expect("laptop-only geometry")
                .1;
            assert_eq!(
                effective,
                TerminalGeometry {
                    columns: 140,
                    rows: 40,
                    cell_width_px: 9,
                    cell_height_px: 18,
                }
            );
        }
        wait_for_terminal_dimensions(&terminal, TerminalViewId(laptop.0), 140, 40);
        let deadline = Instant::now() + Duration::from_secs(30);
        while terminal
            .latest_viewport_for(TerminalViewId(desktop.0))
            .is_some()
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            terminal
                .latest_viewport_for(TerminalViewId(desktop.0))
                .is_none(),
            "steal-attach must release the victim's terminal view"
        );
    }

    #[test]
    fn killing_attached_session_notifies_and_detaches_every_client() {
        let shared = Arc::new(Shared::new(1));
        let first_mailbox = OutboundMailbox::new();
        let second_mailbox = OutboundMailbox::new();
        let (first, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("desktop".to_owned()),
            None,
            Arc::clone(&first_mailbox),
        );
        let (second, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("laptop".to_owned()),
            None,
            Arc::clone(&second_mailbox),
        );
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "doomed"]),
            )
            .expect("create doomed session");
        let session = context.session.expect("doomed session");
        let window = context.window.expect("doomed window");
        shared.attach(first, session).expect("attach first");
        shared.attach(second, session).expect("attach second");
        {
            let mut inner = shared.inner.lock();
            inner.focused_windows.insert(first, window);
            inner.focused_windows.insert(second, window);
        }
        take_reliable_messages(&first_mailbox);
        take_reliable_messages(&second_mailbox);

        shared
            .execute(
                first,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("kill-session", ["-t", "doomed"]),
            )
            .expect("kill attached session");

        for mailbox in [&first_mailbox, &second_mailbox] {
            assert!(
                take_reliable_messages(mailbox)
                    .iter()
                    .any(|message| matches!(
                        message,
                        ProtocolMessage::Event(Event {
                            payload:
                                EventPayload::Detached {
                                    session: detached,
                                    by: None,
                                },
                            ..
                        }) if *detached == session
                    )),
                "every attached client must learn that the session ended"
            );
        }
        let inner = shared.inner.lock();
        assert!(!inner.attached.contains_key(&session));
        assert!(!inner.visible_terminals.contains_key(&first));
        assert!(!inner.visible_terminals.contains_key(&second));
        assert!(!inner.focused_windows.contains_key(&first));
        assert!(!inner.focused_windows.contains_key(&second));
        assert!(inner.subscribers.contains_key(&first));
        assert!(inner.subscribers.contains_key(&second));
    }

    #[test]
    fn multi_client_window_focus_isolated_stamped_and_falls_back_after_kill() {
        let shared = Arc::new(Shared::new(1));
        let first_mailbox = OutboundMailbox::new();
        let second_mailbox = OutboundMailbox::new();
        let observer_mailbox = OutboundMailbox::new();
        let (first_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&first_mailbox),
        );
        let (second_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&second_mailbox),
        );
        let (observer, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&observer_mailbox),
        );

        let mut create_context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut create_context,
                &CommandInvocation::new("new-session", ["-s", "shared-focus", "-n", "agent"]),
            )
            .expect("create shared session");
        let session = create_context.session.expect("shared session");
        let first_window = create_context.window.expect("agent window");
        let first_pane = create_context.pane.expect("agent terminal");
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut create_context,
                &CommandInvocation::new("new-window", ["-n", "logs"]),
            )
            .expect("create logs window");
        let second_window = create_context.window.expect("logs window");
        let second_pane = create_context.pane.expect("logs terminal");
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut create_context,
                &CommandInvocation::new("select-window", ["-t", &first_window.to_string()]),
            )
            .expect("restore agent as the session default");

        let first_attached = shared
            .attach(first_client, session)
            .expect("attach first client");
        let second_attached = shared
            .attach(second_client, session)
            .expect("attach second client");
        assert_eq!(first_attached.focused_window, Some(first_window));
        assert_eq!(second_attached.focused_window, Some(first_window));
        assert_eq!(
            first_attached.focused_window_for(&first_attached.sessions[0]),
            first_window
        );

        let mut first_context = create_context.clone();
        let mut second_context = create_context.clone();
        let resize = |client, context: &mut ExecutionContext, pane, columns, rows| {
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    context,
                    InputMessage::ResizeTerminal {
                        pane,
                        columns,
                        rows,
                        cell_width_px: 8,
                        cell_height_px: 16,
                    },
                )
                .expect("record client geometry");
        };
        resize(first_client, &mut first_context, first_pane, 80, 24);
        resize(second_client, &mut second_context, first_pane, 120, 40);
        resize(first_client, &mut first_context, second_pane, 160, 50);
        resize(second_client, &mut second_context, second_pane, 90, 30);
        take_reliable_messages(&first_mailbox);
        take_reliable_messages(&second_mailbox);
        take_reliable_messages(&observer_mailbox);

        shared
            .execute(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                &CommandInvocation::new("select-window", ["-t", &second_window.to_string()]),
            )
            .expect("first client focuses logs");

        {
            let mut inner = shared.inner.lock();
            let state = &inner.engine.state.sessions[&session];
            assert_eq!(state.active_window, second_window);
            assert_eq!(
                client_focused_window(&inner, first_client, state),
                second_window
            );
            assert_eq!(
                client_focused_window(&inner, second_client, state),
                first_window
            );
            assert_eq!(
                inner.visible_terminals[&first_client],
                BTreeSet::from([second_pane])
            );
            assert_eq!(
                inner.visible_terminals[&second_client],
                BTreeSet::from([first_pane])
            );

            let first_resize = terminal_resize_for_pane(&inner, first_pane)
                .expect("second client still views the agent terminal")
                .1;
            assert_eq!((first_resize.columns, first_resize.rows), (120, 40));
            let second_resize = terminal_resize_for_pane(&inner, second_pane)
                .expect("first client now views the logs terminal")
                .1;
            assert_eq!((second_resize.columns, second_resize.rows), (160, 50));
            inner.engine.set_pane_geometry(first_pane, 120, 40);
            inner.engine.set_pane_geometry(second_pane, 160, 50);

            let snapshot = inner.engine.state.snapshot();
            let first_status = status_context(
                &snapshot,
                &inner.engine,
                Some(session),
                client_focused_window_for_attachment(&inner, first_client),
            );
            let second_status = status_context(
                &snapshot,
                &inner.engine,
                Some(session),
                client_focused_window_for_attachment(&inner, second_client),
            );
            assert_eq!(first_status.window_name, "logs");
            assert_eq!(second_status.window_name, "agent");
            assert_eq!(first_status.pane_width, Some(160));
            assert_eq!(first_status.pane_height, Some(50));
            assert_eq!(first_status.window_width, Some(160));
            assert_eq!(first_status.window_height, Some(50));
            assert_eq!(first_status.pane_active, Some(true));
            assert_eq!(first_status.window_active, Some(true));
            assert_eq!(second_status.pane_width, Some(120));
            assert_eq!(second_status.pane_height, Some(40));
            assert_eq!(second_status.window_width, Some(120));
            assert_eq!(second_status.window_height, Some(40));
            assert_eq!(second_status.pane_active, Some(true));
            assert_eq!(second_status.window_active, Some(true));
        }

        let first_messages = take_reliable_messages(&first_mailbox);
        let second_messages = take_reliable_messages(&second_mailbox);
        let observer_messages = take_reliable_messages(&observer_mailbox);
        assert!(first_messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::Snapshot(snapshot),
                ..
            }) if snapshot.focused_window == Some(second_window)
        )));
        assert!(second_messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::Snapshot(snapshot),
                ..
            }) if snapshot.focused_window == Some(first_window)
        )));
        assert!(observer_messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::Snapshot(snapshot),
                ..
            }) if snapshot.focused_window.is_none()
        )));

        let mut command_context = ExecutionContext {
            session: Some(session),
            window: Some(second_window),
            pane: Some(second_pane),
        };
        shared
            .execute(
                ClientId(7),
                ClientKind::Command,
                &mut command_context,
                &CommandInvocation::new("select-window", ["-t", &first_window.to_string()]),
            )
            .expect("unattached command client moves every viewer");
        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner.focused_windows.get(&first_client),
                Some(&first_window)
            );
            assert_eq!(
                inner.focused_windows.get(&second_client),
                Some(&first_window)
            );
            assert_eq!(
                inner.visible_terminals[&first_client],
                BTreeSet::from([first_pane])
            );
            assert_eq!(
                inner.visible_terminals[&second_client],
                BTreeSet::from([first_pane])
            );
        }

        shared
            .execute(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                &CommandInvocation::new("select-window", ["-t", &second_window.to_string()]),
            )
            .expect("second client focuses logs");
        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner.focused_windows.get(&first_client),
                Some(&first_window)
            );
            assert_eq!(
                inner.focused_windows.get(&second_client),
                Some(&second_window)
            );
        }
        take_reliable_messages(&second_mailbox);

        shared
            .execute(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                &CommandInvocation::new("kill-window", ["-t", &second_window.to_string()]),
            )
            .expect("kill the second client's focused window");
        {
            let inner = shared.inner.lock();
            let state = &inner.engine.state.sessions[&session];
            assert_eq!(state.active_window, first_window);
            assert_eq!(
                inner.focused_windows.get(&second_client),
                Some(&second_window),
                "stale entries are retained and healed on read"
            );
            assert_eq!(
                client_focused_window(&inner, second_client, state),
                first_window
            );
            assert_eq!(
                inner.visible_terminals[&second_client],
                BTreeSet::from([first_pane])
            );
        }
        assert!(
            take_reliable_messages(&second_mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::Snapshot(snapshot),
                        ..
                    }) if snapshot.focused_window == Some(first_window)
                ))
        );

        shared.detach(first_client);
        assert!(
            !shared
                .inner
                .lock()
                .focused_windows
                .contains_key(&first_client)
        );
        shared.unregister(second_client);
        assert!(
            !shared
                .inner
                .lock()
                .focused_windows
                .contains_key(&second_client)
        );
        shared.unregister(observer);
    }

    #[test]
    fn multi_client_session_switch_and_unregister_clean_views_and_geometry() {
        let shared = Arc::new(Shared::new(1));
        let first_mailbox = OutboundMailbox::new();
        let second_mailbox = OutboundMailbox::new();
        let (first_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&first_mailbox),
        );
        let (second_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&second_mailbox),
        );
        let (first_session, first_pane, first_terminal) =
            output_view_session_fixture(&shared, "one", "slice1-peer-still-live");
        let (second_session, second_pane, second_terminal) =
            output_view_session_fixture(&shared, "two", "second session");

        shared
            .attach(first_client, first_session)
            .expect("attach first client");
        shared
            .attach(second_client, first_session)
            .expect("join second client");
        let mut first_context = ExecutionContext::default();
        let mut second_context = ExecutionContext::default();
        shared
            .input(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                InputMessage::ResizeTerminal {
                    pane: first_pane,
                    columns: 200,
                    rows: 60,
                    cell_width_px: 8,
                    cell_height_px: 18,
                },
            )
            .expect("first geometry");
        shared
            .input(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                InputMessage::ResizeTerminal {
                    pane: first_pane,
                    columns: 100,
                    rows: 30,
                    cell_width_px: 7,
                    cell_height_px: 16,
                },
            )
            .expect("second geometry");
        wait_for_terminal_dimensions(&first_terminal, TerminalViewId(first_client.0), 200, 60);

        {
            let mut inner = shared.inner.lock();
            let first_window = inner.engine.state.sessions[&first_session].active_window;
            inner.focused_windows.insert(second_client, first_window);
        }
        shared
            .attach(second_client, second_session)
            .expect("switch second client to session two");
        wait_for_terminal_dimensions(&first_terminal, TerminalViewId(first_client.0), 200, 60);
        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner.attached.get(&first_session),
                Some(&BTreeSet::from([first_client]))
            );
            assert_eq!(
                inner.attached.get(&second_session),
                Some(&BTreeSet::from([second_client]))
            );
            assert_eq!(
                inner.visible_terminals.get(&second_client),
                Some(&BTreeSet::from([second_pane]))
            );
            assert!(
                !inner.focused_windows.contains_key(&second_client),
                "switching sessions must evict the previous per-client focus"
            );
        }
        shared
            .input(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                InputMessage::ResizeTerminal {
                    pane: second_pane,
                    columns: 90,
                    rows: 20,
                    cell_width_px: 7,
                    cell_height_px: 16,
                },
            )
            .expect("second-session geometry");
        wait_for_terminal_dimensions(&second_terminal, TerminalViewId(second_client.0), 90, 20);

        shared.unregister(second_client);
        let deadline = Instant::now() + Duration::from_secs(30);
        while (first_terminal
            .latest_viewport_for(TerminalViewId(second_client.0))
            .is_some()
            || second_terminal
                .latest_viewport_for(TerminalViewId(second_client.0))
                .is_some())
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        shared.send_resync(first_client, &first_mailbox);
        let mut first_state = TerminalTestState::default();
        wait_for_mailbox_terminal(&first_mailbox, &mut first_state, |message, state| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::TerminalViewport { pane, .. },
                    ..
                }) if *pane == first_pane
            ) && state
                .viewports
                .get(&first_pane)
                .is_some_and(|viewport| viewport_text(viewport).contains("slice1-peer-still-live"))
        });

        let inner = shared.inner.lock();
        assert_eq!(
            inner.attached.get(&first_session),
            Some(&BTreeSet::from([first_client]))
        );
        assert!(!inner.attached.contains_key(&second_session));
        assert!(inner.subscribers.contains_key(&first_client));
        assert!(
            inner
                .terminal_geometries
                .values()
                .all(|geometries| !geometries.contains_key(&second_client))
        );
        assert!(
            first_terminal
                .latest_viewport_for(TerminalViewId(first_client.0))
                .is_some(),
            "unregistering one client must leave the peer view active"
        );
        assert!(
            first_terminal
                .latest_viewport_for(TerminalViewId(second_client.0))
                .is_none()
                && second_terminal
                    .latest_viewport_for(TerminalViewId(second_client.0))
                    .is_none(),
            "unregister must release every view owned by the client"
        );
    }

    #[test]
    fn daemon_cross_session_swap_transfers_visibility_without_restarting_surfaces() {
        let shared = Arc::new(Shared::new(1));
        let first_mailbox = OutboundMailbox::new();
        let second_mailbox = OutboundMailbox::new();
        let (first_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&first_mailbox),
        );
        let (second_client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&second_mailbox),
        );
        let mut first_context = ExecutionContext::default();
        let mut second_context = ExecutionContext::default();

        shared
            .execute(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                &CommandInvocation::new("new-session", ["-s", "first"]),
            )
            .expect("first session");
        let first_session = first_context.session.expect("first session id");
        let first_pane = first_context.pane.expect("first pane");
        shared
            .attach(first_client, first_session)
            .expect("attach first session");

        shared
            .execute(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                &CommandInvocation::new("new-session", ["-s", "second"]),
            )
            .expect("second session");
        let second_session = second_context.session.expect("second session id");
        let second_pane = second_context.pane.expect("second pane");
        shared
            .attach(second_client, second_session)
            .expect("attach second session");
        let (first_terminal, second_terminal) = {
            let inner = shared.inner.lock();
            (
                Arc::clone(&inner.terminals[&first_pane]),
                Arc::clone(&inner.terminals[&second_pane]),
            )
        };
        take_reliable_messages(&first_mailbox);
        take_reliable_messages(&second_mailbox);
        shared
            .execute(
                first_client,
                ClientKind::Interactive,
                &mut first_context,
                &CommandInvocation::new("choose-tree", [] as [&str; 0]),
            )
            .expect("open chooser on first pane");
        assert!(shared.inner.lock().choose_trees.contains_key(&first_client));

        shared
            .execute(
                second_client,
                ClientKind::Interactive,
                &mut second_context,
                &CommandInvocation::new(
                    "swap-pane",
                    [
                        "-s",
                        &first_pane.to_string(),
                        "-t",
                        &second_pane.to_string(),
                    ],
                ),
            )
            .expect("cross-session swap");

        let inner = shared.inner.lock();
        assert_eq!(
            inner.visible_terminals[&first_client],
            BTreeSet::from([second_pane])
        );
        assert_eq!(
            inner.visible_terminals[&second_client],
            BTreeSet::from([first_pane])
        );
        assert!(!inner.choose_trees.contains_key(&first_client));
        assert!(Arc::ptr_eq(&first_terminal, &inner.terminals[&first_pane]));
        assert!(Arc::ptr_eq(
            &second_terminal,
            &inner.terminals[&second_pane]
        ));
        assert_eq!(
            inner
                .engine
                .state
                .window_for_pane(first_pane)
                .map(|window| inner.engine.state.windows[&window].session),
            Some(second_session)
        );
        assert_eq!(
            inner
                .engine
                .state
                .window_for_pane(second_pane)
                .map(|window| inner.engine.state.windows[&window].session),
            Some(first_session)
        );
    }

    #[test]
    fn daemon_break_and_join_keep_the_terminal_actor_and_refresh_visibility() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let first = context.pane.expect("first pane");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("split terminal");
        let moving = context.pane.expect("moving pane");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&moving]);
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("break-pane", ["-s", &moving.to_string()]),
            )
            .expect("break pane");
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.engine.state.sessions[&session].windows.len(), 2);
            assert_eq!(inner.visible_terminals[&client], BTreeSet::from([moving]));
            assert!(Arc::ptr_eq(&terminal, &inner.terminals[&moving]));
        }

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new(
                    "join-pane",
                    ["-h", "-s", &moving.to_string(), "-t", &first.to_string()],
                ),
            )
            .expect("join pane");
        let inner = shared.inner.lock();
        assert_eq!(inner.engine.state.sessions[&session].windows.len(), 1);
        assert_eq!(
            inner.visible_terminals[&client],
            BTreeSet::from([first, moving])
        );
        assert!(Arc::ptr_eq(&terminal, &inner.terminals[&moving]));
        assert!(inner.engine.state.validate().is_ok());
    }

    #[test]
    fn daemon_rotation_preserves_terminal_actors_and_moves_zoom_visibility() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let first = context.pane.expect("first terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("second terminal");
        let second = context.pane.expect("second terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-browser", ["-h", "https://rotate.example"]),
            )
            .expect("browser pane");
        let browser = context.pane.expect("browser pane");
        let first_actor = Arc::clone(&shared.inner.lock().terminals[&first]);
        let second_actor = Arc::clone(&shared.inner.lock().terminals[&second]);
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("resize-pane", ["-Z"]),
            )
            .expect("zoom browser");
        assert!(shared.inner.lock().visible_terminals[&client].is_empty());
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("rotate-window", ["-Z"]),
            )
            .expect("rotate zoomed browser forward");
        assert_eq!(context.pane, Some(first));
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.visible_terminals[&client], BTreeSet::from([first]));
            assert!(Arc::ptr_eq(&first_actor, &inner.terminals[&first]));
            assert!(Arc::ptr_eq(&second_actor, &inner.terminals[&second]));
            assert_eq!(
                inner.engine.state.windows[&context.window.unwrap()].zoomed_pane,
                Some(first)
            );
        }
        assert!(take_reliable_messages(&mailbox).iter().any(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) if snapshot.sessions[0].windows[0].active_pane == first
                    && snapshot.sessions[0].windows[0].zoomed_pane == Some(first)
            )
        }));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("rotate-window", ["-D", "-Z"]),
            )
            .expect("rotate back to browser");
        assert_eq!(context.pane, Some(browser));
        assert!(shared.inner.lock().visible_terminals[&client].is_empty());

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("rotate-window", [] as [&str; 0]),
            )
            .expect("rotate and leave zoom");
        let inner = shared.inner.lock();
        assert_eq!(context.pane, Some(first));
        assert_eq!(
            inner.visible_terminals[&client],
            BTreeSet::from([first, second])
        );
        assert_eq!(
            inner.engine.state.windows[&context.window.unwrap()].zoomed_pane,
            None
        );
        assert!(Arc::ptr_eq(&first_actor, &inner.terminals[&first]));
        assert!(Arc::ptr_eq(&second_actor, &inner.terminals[&second]));
        assert!(inner.engine.state.validate().is_ok());
    }

    #[test]
    fn daemon_display_panes_routes_keys_and_expires_reliably() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let terminal = context.pane.expect("terminal pane");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-browser", ["-h", "https://example.com"]),
            )
            .expect("browser pane");
        let browser = context.pane.expect("browser pane");
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("display-panes", ["-d", "1000"]),
            )
            .expect("open pane indicators");
        let opened = take_reliable_messages(&mailbox);
        assert!(opened.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::DisplayPanes { state: Some(state) },
                ..
            }) if state.indicators.len() == 2
                && state.indicators.iter().any(|indicator| {
                    indicator.pane == browser && indicator.active() && indicator.select_key == b'1'
                })
        )));

        shared.send_resync(client, &mailbox);
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::DisplayPanes { state: Some(_) },
                        ..
                    })
                ))
        );

        let invalid = test_key(KeyCode::Character('x'), Modifiers::default(), Some("x"));
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::DisplayPanes {
                    action: DisplayPanesAction::Key(invalid.clone()),
                },
            )
            .expect("fall through invalid key");
        let invalid_messages = take_reliable_messages(&mailbox);
        assert!(invalid_messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::DisplayPanes { state: None },
                ..
            })
        )));
        assert!(invalid_messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::BrowserCommand {
                    pane,
                    command: BrowserCommand::Key(input),
                },
                ..
            }) if *pane == browser && input == &invalid
        )));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("displayp", ["-d1000"]),
            )
            .expect("reopen pane indicators");
        take_reliable_messages(&mailbox);
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::DisplayPanes {
                    action: DisplayPanesAction::Key(test_key(
                        KeyCode::Character('0'),
                        Modifiers::default(),
                        Some("0"),
                    )),
                },
            )
            .expect("select pane zero");
        assert_eq!(context.pane, Some(terminal));
        assert!(!shared.inner.lock().display_panes.contains_key(&client));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("display-panes", ["-d", "20"]),
            )
            .expect("open timed pane indicators");
        let scheduled = {
            let inner = shared.inner.lock();
            let overlay = &inner.display_panes[&client];
            DisplayPanesDeadline {
                client,
                token: overlay.token,
                deadline: overlay.deadline.expect("timed pane deadline"),
            }
        };
        assert!(shared.expire_display_panes(scheduled, scheduled.deadline));
        assert!(!shared.inner.lock().display_panes.contains_key(&client));
        let messages = take_reliable_messages(&mailbox);
        let opened = messages.iter().position(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::DisplayPanes { state: Some(_) },
                    ..
                })
            )
        });
        let closed = opened.and_then(|opened| {
            messages.iter().skip(opened + 1).position(|message| {
                matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::DisplayPanes { state: None },
                        ..
                    })
                )
            })
        });
        assert!(closed.is_some());
    }

    #[test]
    fn list_clients_uses_attached_registry_name_order_and_session_filtering() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(90),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "z"]),
            )
            .unwrap();
        let z = context.session.unwrap();
        shared
            .execute(
                ClientId(90),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "A"]),
            )
            .unwrap();
        let a = context.session.unwrap();

        let (zeta, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("zeta".to_owned()),
            Some(TerminalColorScheme::Dark),
            OutboundMailbox::new(),
        );
        let (alpha, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("alpha".to_owned()),
            Some(TerminalColorScheme::Light),
            OutboundMailbox::new(),
        );
        let (_detached, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("ghost".to_owned()),
            None,
            OutboundMailbox::new(),
        );
        shared.attach(zeta, z).unwrap();
        shared.attach(alpha, a).unwrap();

        let listed = shared
            .execute(
                ClientId(91),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("list-clients", [] as [&str; 0]),
            )
            .unwrap();
        assert_eq!(listed.output, "alpha: A [0x0 ] \nzeta: z [0x0 ] ");

        let filtered = shared
            .execute(
                ClientId(91),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "lsc",
                    [
                        "-t",
                        "z",
                        "-F",
                        "#{line}:#{client_name}:#{session_name}:#{client_width}x#{client_height}:#{client_termname}",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(filtered.output, "1:zeta:z:0x0:");
    }

    #[test]
    fn show_messages_logs_commands_newest_first_and_bounds_the_ring() {
        let shared = Arc::new(Shared::new(1));
        let client = ClientId(7);
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "work"]),
            )
            .unwrap();

        let displayed = shared.execute_command_request(
            client,
            ClientKind::Command,
            &mut context,
            1,
            &CommandInvocation::new("display-message", ["hello"]),
        );
        assert!(matches!(
            displayed,
            CommandResponse::Success { output, .. } if output.is_empty()
        ));
        let shown = shared.execute_command_request(
            client,
            ClientKind::Command,
            &mut context,
            2,
            &CommandInvocation::new("showmsgs", [] as [&str; 0]),
        );
        let CommandResponse::Success { output, .. } = shown else {
            panic!("show-messages should succeed");
        };
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with(": client-7 command: show-messages"));
        assert!(lines[1].ends_with(": hello"));
        assert!(lines[2].ends_with(": client-7 command: display-message hello"));
        assert!(lines.iter().all(|line| {
            let bytes = line.as_bytes();
            bytes.get(2) == Some(&b':') && bytes.get(5) == Some(&b':')
        }));

        let mut inner = shared.inner.lock();
        inner.message_log.clear();
        inner.next_message_number = 0;
        drop(inner);
        shared
            .execute(
                client,
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-s", "message-limit", "3"]),
            )
            .unwrap();
        let mut inner = shared.inner.lock();
        for number in 0..5 {
            push_server_message(&mut inner, format!("message-{number}"));
        }
        assert_eq!(
            inner
                .message_log
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            ["message-2", "message-3", "message-4"]
        );
        assert_eq!(
            inner
                .message_log
                .iter()
                .rev()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            ["message-4", "message-3", "message-2"]
        );
        inner.message_log.clear();
        inner.next_message_number = 0;
        drop(inner);

        shared
            .execute(
                client,
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-su", "message-limit"]),
            )
            .unwrap();
        let mut inner = shared.inner.lock();
        for number in 0..5 {
            push_server_message(&mut inner, format!("message-{number}"));
        }
        drop(inner);
        shared
            .execute(
                client,
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-option", ["-s", "message-limit", "3"]),
            )
            .unwrap();
        let mut inner = shared.inner.lock();
        assert_eq!(inner.message_log.len(), 5);
        push_server_message(&mut inner, "message-5".to_owned());
        assert_eq!(
            inner
                .message_log
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            ["message-3", "message-4", "message-5"]
        );
    }

    #[test]
    fn failing_command_logs_its_line_and_then_the_error_message() {
        let shared = Arc::new(Shared::new(1));
        let client = ClientId(7);
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "work"]),
            )
            .unwrap();

        let response = shared.execute_command_request(
            client,
            ClientKind::Command,
            &mut context,
            1,
            &CommandInvocation::new("select-window", ["-t", "99"]),
        );
        assert!(matches!(
            response,
            CommandResponse::Error {
                request_id: 1,
                error: ServerError::WindowNotFound(window),
            } if window == "99"
        ));

        let inner = shared.inner.lock();
        assert_eq!(inner.message_log.len(), 2);
        assert_eq!(inner.next_message_number, 2);
        assert_eq!(
            inner
                .message_log
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            [
                "client-7 command: select-window -t 99",
                "client-7 message: can't find window: 99",
            ]
        );
    }

    #[test]
    fn detached_refresh_client_accepts_the_pin_grammar_then_errors_exactly() {
        let shared = Arc::new(Shared::new(1));
        let mut context = ExecutionContext::default();
        let error = shared
            .execute(
                ClientId(5),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "refresh",
                    [
                        "-cDlLRSU",
                        "-A",
                        "%0:on",
                        "-B",
                        "name:what:format",
                        "-C",
                        "80x24",
                        "-F",
                        "focused",
                        "-f",
                        "focused",
                        "-r",
                        "%0:report",
                        "-t",
                        "client",
                        "+1",
                    ],
                ),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message == "no current client"
        ));
    }

    #[test]
    fn move_and_swap_window_publish_topology_snapshots() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some("desktop".to_owned()),
            None,
            Arc::clone(&mailbox),
        );
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work", "-n", "main"]),
            )
            .unwrap();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-window", ["-d", "-n", "other"]),
            )
            .unwrap();
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("move-window", ["-d", "-s", "work:other", "-t", "work:5"]),
            )
            .unwrap();
        let moved = take_reliable_messages(&mailbox)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) => Some(snapshot),
                _ => None,
            })
            .collect::<Vec<_>>();
        let moved = moved.last().expect("move publishes a snapshot");
        assert_eq!(
            moved.sessions[0]
                .windows
                .iter()
                .map(|window| (window.index, window.name.as_str()))
                .collect::<Vec<_>>(),
            [(0, "main"), (5, "other")]
        );

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("swap-window", ["-s", "work:0", "-t", "work:5"]),
            )
            .unwrap();
        let swapped = take_reliable_messages(&mailbox)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) => Some(snapshot),
                _ => None,
            })
            .collect::<Vec<_>>();
        let swapped = swapped.last().expect("swap publishes a snapshot");
        assert_eq!(
            swapped.sessions[0]
                .windows
                .iter()
                .map(|window| (window.index, window.name.as_str()))
                .collect::<Vec<_>>(),
            [(0, "other"), (5, "main")]
        );
    }

    #[test]
    fn command_error_that_mutates_mux_state_publishes_snapshot() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let (window, first) = {
            let mut inner = shared.inner.lock();
            let (_, window, first) = inner
                .engine
                .state
                .create_session("work")
                .expect("create session");
            inner
                .engine
                .state
                .split_pane(first, zz_protocol::Axis::Horizontal, PaneKind::Terminal)
                .expect("split window");
            inner.engine.state.toggle_zoom(first).expect("zoom pane");
            (window, first)
        };
        let mut context = ExecutionContext::for_pane(&shared.inner.lock().engine.state, first)
            .expect("command context");
        shared.publish_snapshot();
        take_reliable_messages(&mailbox);
        let generation = shared.inner.lock().engine.state.generation();

        let error = shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-layout", ["bogus"]),
            )
            .expect_err("invalid layout");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message == "invalid layout: bogus"
        ));
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.engine.state.windows[&window].zoomed_pane, None);
            assert_eq!(inner.engine.state.generation(), generation + 1);
            assert_eq!(inner.last_published_mux_generation, generation + 1);
        }
        let snapshots = take_reliable_messages(&mailbox)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) => Some(snapshot),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].generation, generation + 1);
        assert_eq!(snapshots[0].sessions[0].windows[0].zoomed_pane, None);
    }

    #[test]
    fn daemon_zoom_subscribes_only_the_visible_terminal_surface() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let first = context.pane.expect("first terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("second terminal");
        let second = context.pane.expect("second terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-browser", ["https://example.com"]),
            )
            .expect("browser pane");
        let browser = context.pane.expect("browser pane");
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);
        assert_eq!(
            shared.inner.lock().visible_terminals[&client],
            BTreeSet::from([first, second])
        );

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("resize-pane", ["-Z", "-t", &first.to_string()]),
            )
            .expect("zoom terminal");
        assert_eq!(
            shared.inner.lock().visible_terminals[&client],
            BTreeSet::from([first])
        );
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::Snapshot(snapshot),
                        ..
                    }) if snapshot.sessions[0].windows[0].zoomed_pane == Some(first)
                ))
        );

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("resize-pane", ["-Z", "-t", &browser.to_string()]),
            )
            .expect("unzoom terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("resize-pane", ["-Z", "-t", &browser.to_string()]),
            )
            .expect("zoom browser");
        assert!(shared.inner.lock().visible_terminals[&client].is_empty());

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("next-layout", [] as [&str; 0]),
            )
            .expect("apply a preset layout");
        assert_eq!(
            shared.inner.lock().engine.state.windows[&context.window.unwrap()].zoomed_pane,
            None
        );
        assert_eq!(
            shared.inner.lock().visible_terminals[&client],
            BTreeSet::from([first, second])
        );
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::Snapshot(snapshot),
                        ..
                    }) if matches!(
                        snapshot.sessions[0].windows[0].layout,
                        LayoutNode::Split {
                            axis: zz_protocol::Axis::Horizontal,
                            ..
                        }
                    ) && snapshot.sessions[0].windows[0].zoomed_pane.is_none()
                ))
        );
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("resize-pane", ["-Z", "-t", &browser.to_string()]),
            )
            .expect("rezoom browser after layout selection");
        assert!(shared.inner.lock().visible_terminals[&client].is_empty());

        let (_, _, display) =
            build_display_panes_state(&shared.inner.lock().engine, browser, 1_000)
                .expect("zoomed display panes");
        assert_eq!(display.indicators.len(), 1);
        assert_eq!(display.indicators[0].pane, browser);
        assert_eq!(display.indicators[0].select_key, b'2');

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("display-panes", ["-d", "0"]),
            )
            .expect("display zoomed browser pane");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::DisplayPanes {
                    action: DisplayPanesAction::Key(test_key(
                        KeyCode::Character('2'),
                        Modifiers::default(),
                        Some("2"),
                    )),
                },
            )
            .expect("select displayed zoomed pane");
        assert_eq!(
            shared.inner.lock().engine.state.windows[&context.window.unwrap()].zoomed_pane,
            None
        );
        assert_eq!(
            shared.inner.lock().visible_terminals[&client],
            BTreeSet::from([first, second])
        );
    }

    #[test]
    fn choose_tree_closes_when_its_source_pane_is_removed() {
        let shared = Shared::new(1);
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let session = {
            let mut inner = shared.inner.lock();
            let (session, _, source) = inner
                .engine
                .state
                .create_session("ephemeral")
                .expect("session");
            inner.attached.entry(session).or_default().insert(client);
            let chooser = ChooseTreeSession::new(
                ChooseTreeKind::Panes,
                source,
                &inner.engine.state,
                Some(session),
            )
            .expect("chooser");
            inner.choose_trees.insert(client, chooser);
            session
        };
        shared
            .inner
            .lock()
            .engine
            .state
            .kill_session(session)
            .expect("remove source session");

        shared.refresh_choose_trees();

        assert!(!shared.inner.lock().choose_trees.contains_key(&client));
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::ChooseTree { state: None },
                        ..
                    })
                ))
        );
    }

    #[test]
    fn daemon_session_and_window_choosers_focus_sidebar_without_opening_a_tree() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work"]),
            )
            .expect("session");
        let session = context.session.expect("session id");
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        for command in [
            CommandInvocation::new("focus-sidebar", [] as [&str; 0]),
            CommandInvocation::new("choose-tree", ["-Zs"]),
            CommandInvocation::new("choose-tree", ["-Zw"]),
        ] {
            shared
                .execute(client, ClientKind::Interactive, &mut context, &command)
                .expect("focus sidebar");
            let messages = take_reliable_messages(&mailbox);
            assert!(messages.iter().any(|message| matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::FocusSidebar,
                    ..
                })
            )));
            assert!(!shared.inner.lock().choose_trees.contains_key(&client));
        }

        let error = shared
            .execute(
                client,
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("focus-sidebar", [] as [&str; 0]),
            )
            .expect_err("command clients cannot focus native UI");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidCommand(message))
                if message.contains("interactive client")
        ));
    }

    #[test]
    fn default_rename_bindings_prefill_and_submit_native_prompts() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "work"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let window = context.window.expect("window");
        let pane = context.pane.expect("pane");
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        let open_binding = |context: &mut ExecutionContext, binding: &str| {
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    context,
                    InputMessage::Key {
                        pane,
                        input: test_key(
                            KeyCode::Character('b'),
                            Modifiers::new(false, true, false, false),
                            None,
                        ),
                        text_follows: false,
                    },
                )
                .expect("prefix key");
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    context,
                    InputMessage::Text {
                        pane,
                        text: binding.to_owned(),
                    },
                )
                .expect("rename binding");
        };
        let replace_and_submit = |context: &mut ExecutionContext, name: &str| {
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    context,
                    InputMessage::Key {
                        pane,
                        input: test_key(
                            KeyCode::Character('u'),
                            Modifiers::new(false, true, false, false),
                            None,
                        ),
                        text_follows: false,
                    },
                )
                .expect("clear prompt");
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    context,
                    InputMessage::Text {
                        pane,
                        text: name.to_owned(),
                    },
                )
                .expect("replacement name");
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    context,
                    InputMessage::Key {
                        pane,
                        input: test_key(KeyCode::Enter, Modifiers::default(), None),
                        text_follows: false,
                    },
                )
                .expect("submit rename");
        };

        open_binding(&mut context, "$");
        {
            let inner = shared.inner.lock();
            let prompt = &inner.command_prompts[&client];
            assert_eq!(prompt.input, "work");
            assert_eq!(prompt.template.as_deref(), Some("rename-session -- '%%'"));
        }
        replace_and_submit(&mut context, "primary");
        assert_eq!(
            shared.inner.lock().engine.state.sessions[&session].name,
            "primary"
        );

        let expected_window_name = shared.inner.lock().engine.state.windows[&window]
            .name
            .clone();
        open_binding(&mut context, ",");
        {
            let inner = shared.inner.lock();
            let prompt = &inner.command_prompts[&client];
            assert_eq!(prompt.input, expected_window_name);
            assert_eq!(prompt.template.as_deref(), Some("rename-window -- '%%'"));
        }
        replace_and_submit(&mut context, "editor");
        {
            let inner = shared.inner.lock();
            assert_eq!(inner.engine.state.windows[&window].name, "editor");
            assert!(!inner.command_prompts.contains_key(&client));
        }
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::Snapshot(snapshot),
                ..
            }) if snapshot.sessions[0].name == "primary"
                && snapshot.sessions[0].windows[0].name == "editor"
        )));
    }

    #[test]
    fn native_command_prompt_precedes_pane_input_and_runs_command_sequences() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", [] as [&str; 0]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let source = context.pane.expect("source pane");
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: source,
                    input: test_key(
                        KeyCode::Character('b'),
                        Modifiers::new(false, true, false, false),
                        None,
                    ),
                    text_follows: false,
                },
            )
            .expect("prefix key");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: source,
                    input: test_key(KeyCode::Character(':'), Modifiers::default(), Some(":")),
                    text_follows: true,
                },
            )
            .expect("command-prompt binding");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Text {
                    pane: source,
                    text: ":".to_owned(),
                },
            )
            .expect("suppress committed binding text");
        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner.command_prompts[&client].input, "",
                "the binding character must not leak into the prompt"
            );
            assert!(
                !inner.suppressed_text.contains_key(&client),
                "consumed committed text must not leave suppression bookkeeping behind"
            );
        }
        let opened = take_reliable_messages(&mailbox);
        assert!(opened.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::CommandPrompt { state: Some(state) },
                ..
            }) if state.input.is_empty()
        )));
        shared.send_resync(client, &mailbox);
        let resync = take_reliable_messages(&mailbox);
        assert!(resync.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::Snapshot(_),
                ..
            })
        )));
        assert!(resync.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::CommandPrompt { state: Some(state) },
                ..
            }) if state.input.is_empty()
        )));

        let command = "new-window -n prompted; split-window -h";
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Text {
                    pane: source,
                    text: command.to_owned(),
                },
            )
            .expect("prompt text");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: source,
                    input: test_key(KeyCode::Enter, Modifiers::default(), None),
                    text_follows: false,
                },
            )
            .expect("submit prompt");

        {
            let inner = shared.inner.lock();
            let session_state = &inner.engine.state.sessions[&session];
            assert_eq!(session_state.windows.len(), 2);
            let active = &inner.engine.state.windows[&session_state.active_window];
            assert_eq!(active.name, "prompted");
            assert_eq!(active.panes.len(), 2);
            assert!(!inner.command_prompts.contains_key(&client));
            assert_eq!(
                inner.command_history.last().map(String::as_str),
                Some(command)
            );
        }
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::CommandPrompt { state: None },
                ..
            })
        )));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("command-prompt", [] as [&str; 0]),
            )
            .expect("reopen prompt");
        let pane = context.pane.expect("active pane");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane,
                    input: test_key(KeyCode::ArrowUp, Modifiers::default(), None),
                    text_follows: false,
                },
            )
            .expect("prompt history");
        assert_eq!(shared.inner.lock().command_prompts[&client].input, command);
    }

    #[test]
    fn command_prompt_templates_and_output_use_native_view_mode() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", [] as [&str; 0]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        shared.attach(client, session).expect("attach session");
        take_reliable_messages(&mailbox);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("command-prompt", ["-I", "scratch", "new-window -n %%"]),
            )
            .expect("template prompt");
        let pane = context.pane.expect("active pane");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane,
                    input: test_key(KeyCode::Enter, Modifiers::default(), None),
                    text_follows: false,
                },
            )
            .expect("submit template");
        assert_eq!(
            shared.inner.lock().engine.state.windows[&context.window.unwrap()].name,
            "scratch"
        );

        take_reliable_messages(&mailbox);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("command-prompt", ["-I", "list-panes"]),
            )
            .expect("output prompt");
        let pane = context.pane.expect("active pane");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane,
                    input: test_key(KeyCode::Enter, Modifiers::default(), None),
                    text_follows: false,
                },
            )
            .expect("submit output command");
        let message = take_command_output_message(&mailbox);
        let ProtocolMessage::Event(Event {
            payload:
                EventPayload::CommandOutput {
                    pane: output_pane,
                    viewport: Some(viewport),
                },
            ..
        }) = message
        else {
            panic!("expected native command-output viewport");
        };
        assert_eq!(output_pane, pane);
        assert!(matches!(
            viewport.mode,
            zz_terminal::TerminalMode::View { .. }
        ));
        assert!(viewport_text(&viewport).contains("terminal"));
        assert!(
            shared.inner.lock().key_engines[&client]
                .active_table()
                .is_some()
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ResizeCommandOutput {
                    columns: 20,
                    rows: 4,
                    cell_width_px: 8,
                    cell_height_px: 18,
                },
            )
            .expect("resize output view");
        let resized = take_command_output_message(&mailbox);
        assert!(matches!(
            resized,
            ProtocolMessage::Event(Event {
                payload: EventPayload::CommandOutput {
                    viewport: Some(TerminalViewport {
                        columns: 20,
                        rows: 4,
                        ..
                    }),
                    ..
                },
                ..
            })
        ));

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Text {
                    pane,
                    text: "q".to_owned(),
                },
            )
            .expect("close output view");
        wait_for_command_output_close(&mailbox);
        let inner = shared.inner.lock();
        assert!(!inner.command_outputs.contains_key(&client));
        assert_eq!(inner.key_engines[&client].active_table(), None);
    }

    #[test]
    fn command_output_retires_with_its_pane_or_attached_session() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "first"]),
            )
            .expect("first session");
        let first_session = context.session.expect("first session id");
        let source = context.pane.expect("source pane");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("surviving pane");
        shared.attach(client, first_session).expect("attach first");
        take_reliable_messages(&mailbox);

        shared
            .open_command_output(
                client,
                Some(source),
                "pane output".to_owned(),
                "pane-bound output",
            )
            .expect("open pane output");
        take_command_output_message(&mailbox);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("kill-pane", ["-t", &source.to_string()]),
            )
            .expect("remove output source");
        wait_for_command_output_close(&mailbox);
        assert!(!shared.inner.lock().command_outputs.contains_key(&client));

        let remaining = context.pane.expect("remaining pane");
        shared
            .open_command_output(
                client,
                Some(remaining),
                "session output".to_owned(),
                "session-bound output",
            )
            .expect("open session output");
        take_command_output_message(&mailbox);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "second"]),
            )
            .expect("second session");
        let second_session = context.session.expect("second session id");
        shared
            .attach(client, second_session)
            .expect("switch attached session");
        wait_for_command_output_close(&mailbox);
        let second_pane = context.pane.expect("second-session pane");
        shared
            .open_command_output(
                client,
                Some(second_pane),
                "window output".to_owned(),
                "window-bound output",
            )
            .expect("open window output");
        take_command_output_message(&mailbox);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-window", [] as [&str; 0]),
            )
            .expect("change active window");
        wait_for_command_output_close(&mailbox);
        let inner = shared.inner.lock();
        assert!(!inner.command_outputs.contains_key(&client));
        assert_eq!(inner.key_engines[&client].active_table(), None);
    }

    #[test]
    fn pane_input_resolves_key_tables_from_every_pane_kind() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        for command in [
            CommandInvocation::new("set-option", ["-g", "prefix", "C-a"]),
            CommandInvocation::new("bind-key", ["-n", "C-h", "focus-sidebar"]),
            CommandInvocation::new("bind-key", ["-r", "s", "focus-sidebar"]),
        ] {
            shared
                .execute(client, ClientKind::Interactive, &mut context, &command)
                .expect("configure key tables");
        }
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "routes"]),
            )
            .expect("session");
        let session = context.session.expect("session");
        let terminal = context.pane.expect("terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-browser", ["-h", "https://example.com"]),
            )
            .expect("browser");
        let browser = context.pane.expect("browser");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-picker", ["-v"]),
            )
            .expect("agent picker");
        let agent = context.pane.expect("agent picker");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "experimental-agent-pane", "on"]),
            )
            .expect("enable agent panes");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane-kind", ["-t", &agent.to_string(), "agent"]),
            )
            .expect("agent");
        shared.attach(client, session).expect("attach");
        take_reliable_messages(&mailbox);

        let browser_key = |message: &ProtocolMessage| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::BrowserCommand {
                        command: BrowserCommand::Key(_),
                        ..
                    },
                    ..
                })
            )
        };
        let prefix_armed = |message: &ProtocolMessage, wanted: bool| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::PrefixArmed { armed },
                    ..
                }) if *armed == wanted
            )
        };
        let focus_sidebar = |message: &ProtocolMessage| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::FocusSidebar,
                    ..
                })
            )
        };

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: browser,
                    input: test_key(
                        KeyCode::Character('a'),
                        Modifiers::new(false, true, false, false),
                        None,
                    ),
                    text_follows: false,
                },
            )
            .expect("browser prefix");
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(|message| prefix_armed(message, true)));
        assert!(!messages.iter().any(browser_key));

        let mut unrelated_surface_release = test_key(
            KeyCode::Character('a'),
            Modifiers::new(false, true, false, false),
            None,
        );
        unrelated_surface_release.action = KeyAction::Release;
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::BrowserSurfaceKey {
                    pane: browser,
                    input: unrelated_surface_release,
                    text_follows: false,
                },
            )
            .expect("unrelated browser surface release");
        assert!(take_reliable_messages(&mailbox).iter().any(browser_key));
        assert!(shared.inner.lock().swallowed_keys[&client].contains("a"));

        let mut prefix_release = test_key(
            KeyCode::Character('a'),
            Modifiers::new(false, true, false, false),
            None,
        );
        prefix_release.action = KeyAction::Release;
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: browser,
                    input: prefix_release,
                    text_follows: false,
                },
            )
            .expect("browser prefix release");
        assert!(!take_reliable_messages(&mailbox).iter().any(browser_key));
        assert!(shared.inner.lock().swallowed_keys[&client].is_empty());
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("prefix")
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: browser,
                    input: test_key(KeyCode::Character('s'), Modifiers::default(), Some("s")),
                    text_follows: true,
                },
            )
            .expect("browser prefix-table binding");
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(focus_sidebar));
        assert!(!messages.iter().any(browser_key));
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("prefix")
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: browser,
                    input: test_key(KeyCode::Character('Z'), Modifiers::default(), Some("Z")),
                    text_follows: false,
                },
            )
            .expect("unbound key after the repeat window retries root");
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(browser_key));
        assert!(messages.iter().any(|message| prefix_armed(message, false)));

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::BrowserSurfaceKey {
                    pane: browser,
                    input: test_key(KeyCode::Character('y'), Modifiers::default(), Some("y")),
                    text_follows: false,
                },
            )
            .expect("plain browser key passes to the page");
        assert!(take_reliable_messages(&mailbox).iter().any(browser_key));

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::BrowserSurfaceKey {
                    pane: browser,
                    input: test_key(
                        KeyCode::Character('h'),
                        Modifiers::new(false, true, false, false),
                        None,
                    ),
                    text_follows: false,
                },
            )
            .expect("browser surface key");
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(browser_key));
        assert!(!messages.iter().any(focus_sidebar));

        let mut surface_release = test_key(
            KeyCode::Character('h'),
            Modifiers::new(false, true, false, false),
            None,
        );
        surface_release.action = KeyAction::Release;
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::BrowserSurfaceKey {
                    pane: browser,
                    input: surface_release,
                    text_follows: false,
                },
            )
            .expect("browser surface key release");
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(browser_key));
        assert!(!messages.iter().any(focus_sidebar));

        for (input, text_follows) in [
            (
                test_key(
                    KeyCode::Character('a'),
                    Modifiers::new(false, true, false, false),
                    None,
                ),
                false,
            ),
            (
                test_key(KeyCode::Character('s'), Modifiers::default(), Some("s")),
                true,
            ),
            (
                test_key(KeyCode::Character('Z'), Modifiers::default(), Some("Z")),
                false,
            ),
        ] {
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    InputMessage::Key {
                        pane: agent,
                        input,
                        text_follows,
                    },
                )
                .expect("agent-source key");
        }
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(focus_sidebar));
        assert!(!messages.iter().any(browser_key));
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            None
        );

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("bind-key", ["h", "select-pane", "-L"]),
            )
            .expect("bind h to select-pane -L");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-picker", ["-h", "-t", &terminal.to_string()]),
            )
            .expect("editor picker beside the terminal");
        let editor = context.pane.expect("editor picker");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "experimental-editor-pane", "on"]),
            )
            .expect("enable editor panes");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane-kind", ["-t", &editor.to_string(), "editor"]),
            )
            .expect("editor");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane", ["-t", &editor.to_string()]),
            )
            .expect("focus the editor");
        let window = context.window.expect("window");
        assert_eq!(
            shared.inner.lock().engine.state.windows[&window].active_pane,
            editor
        );
        take_reliable_messages(&mailbox);

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: editor,
                    input: test_key(
                        KeyCode::Character('a'),
                        Modifiers::new(false, true, false, false),
                        None,
                    ),
                    text_follows: false,
                },
            )
            .expect("editor prefix");
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(|message| prefix_armed(message, true)));
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("prefix")
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: editor,
                    input: test_key(KeyCode::Character('h'), Modifiers::default(), Some("h")),
                    text_follows: true,
                },
            )
            .expect("editor prefix-table select-pane -L");
        let messages = take_reliable_messages(&mailbox);
        assert!(messages.iter().any(|message| prefix_armed(message, false)));
        assert_eq!(
            shared.inner.lock().engine.state.windows[&window].active_pane,
            terminal,
            "prefix+h from the editor must focus the terminal to its left"
        );
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            None
        );

        let prefix = test_key(
            KeyCode::Character('a'),
            Modifiers::new(false, true, false, false),
            None,
        );
        let binding = test_key(KeyCode::Character('s'), Modifiers::default(), Some("s"));

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: terminal,
                    input: prefix,
                    text_follows: false,
                },
            )
            .expect("normal prefix");
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("prefix")
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: terminal,
                    input: binding,
                    text_follows: true,
                },
            )
            .expect("prefix-table binding");
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::FocusSidebar,
                        ..
                    })
                ))
        );
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("prefix")
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: terminal,
                    input: test_key(KeyCode::Character('Z'), Modifiers::default(), Some("Z")),
                    text_follows: false,
                },
            )
            .expect("unbound prefix-table key is discarded");
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            None
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Key {
                    pane: terminal,
                    input: test_key(
                        KeyCode::Character('h'),
                        Modifiers::new(false, true, false, false),
                        None,
                    ),
                    text_follows: false,
                },
            )
            .expect("root binding");
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::FocusSidebar,
                        ..
                    })
                ))
        );

        let error = shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::BrowserSurfaceKey {
                    pane: terminal,
                    input: test_key(KeyCode::Character('x'), Modifiers::default(), Some("x")),
                    text_follows: true,
                },
            )
            .expect_err("terminal panes cannot claim the browser surface route");
        assert!(matches!(
            error,
            DaemonError::Server(ServerError::InvalidTarget(message))
                if message == format!("{terminal} is not a browser pane")
        ));
    }

    #[test]
    fn synchronized_input_fans_out_to_terminal_and_browser_panes() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new(
                    "new-session",
                    ["-s", "sync-input", terminal_line_echo_command()],
                ),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let first = context.pane.expect("first terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h", terminal_line_echo_command()]),
            )
            .expect("second terminal");
        let second = context.pane.expect("second terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-browser", ["-h", "https://example.com"]),
            )
            .expect("browser pane");
        let browser = context.pane.expect("browser pane");
        shared.attach(client, session).expect("attach session");
        let (first_terminal, second_terminal) = {
            let inner = shared.inner.lock();
            (
                Arc::clone(&inner.terminals[&first]),
                Arc::clone(&inner.terminals[&second]),
            )
        };
        for terminal in [&first_terminal, &second_terminal] {
            wait_for_viewport(
                terminal,
                TerminalViewId(client.0),
                "synchronized-input terminal never became ready",
                |viewport| viewport_text(viewport).contains("zz-terminal-ready"),
            );
        }
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-option", ["-w", "synchronize-panes", "on"]),
            )
            .expect("enable synchronized input");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("bind-key", ["-n", "Any", "focus-sidebar"]),
            )
            .expect("install a root catch-all that browser surface input must bypass");
        take_reliable_messages(&mailbox);

        let marker = "ZZ_SYNC_SEND_KEYS";
        let send_keys = vec![
            zz_protocol::KeyToken::Literal(format!("echo {marker}")),
            zz_protocol::KeyToken::Named("Enter".to_owned()),
        ];
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new(
                    "send-keys",
                    ["-t", &first.to_string(), &format!("echo {marker}"), "Enter"],
                ),
            )
            .expect("synchronized send-keys");
        let browser_messages = take_reliable_messages(&mailbox);
        assert!(browser_messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::BrowserCommand {
                    pane,
                    command: BrowserCommand::SendKeys(keys),
                },
                ..
            }) if *pane == browser && keys == &send_keys
        )));

        for pane in [first, second] {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let captured = shared
                    .execute(
                        client,
                        ClientKind::Command,
                        &mut context,
                        &CommandInvocation::new("capture-pane", ["-p", "-t", &pane.to_string()]),
                    )
                    .expect("capture synchronized terminal")
                    .output;
                if captured.contains(marker) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "{pane} did not receive synchronized send-keys"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        take_reliable_messages(&mailbox);
        let key = KeyInput {
            action: KeyAction::Press,
            key: KeyCode::Enter,
            modifiers: Modifiers::default(),
            text: None,
            unshifted_codepoint: None,
        };
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::BrowserSurfaceText {
                    pane: browser,
                    text: "xλz".to_owned(),
                },
            )
            .expect("synchronized text");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::BrowserSurfaceKey {
                    pane: browser,
                    input: key.clone(),
                    text_follows: false,
                },
            )
            .expect("synchronized key");
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::TerminalView {
                    pane: first,
                    action: zz_terminal::TerminalViewAction::Paste("pasted".to_owned()),
                },
            )
            .expect("synchronized paste");

        let messages = take_reliable_messages(&mailbox);
        assert!(!messages.iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::FocusSidebar,
                ..
            })
        )));
        let commands = messages
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::BrowserCommand { pane, command },
                    ..
                }) if pane == browser => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands,
            vec![
                BrowserCommand::SendKeys(vec![zz_protocol::KeyToken::Literal("xλz".to_owned())]),
                BrowserCommand::Key(key),
                BrowserCommand::SendKeys(vec![zz_protocol::KeyToken::Literal("pasted".to_owned())]),
            ]
        );

        take_reliable_messages(&mailbox);
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("set-buffer", ["buffer paste"]),
            )
            .expect("set paste buffer");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("paste-buffer", ["-t", &first.to_string()]),
            )
            .expect("synchronized paste-buffer");
        assert!(
            take_reliable_messages(&mailbox)
                .iter()
                .any(|message| matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::BrowserCommand {
                            pane,
                            command: BrowserCommand::SendKeys(keys),
                        },
                        ..
                    }) if *pane == browser
                        && keys == &[zz_protocol::KeyToken::Literal("buffer paste".to_owned())]
                ))
        );
    }

    fn belled_session(
        shared: &Arc<Shared>,
        mailbox: &Arc<OutboundMailbox>,
    ) -> (ClientId, ExecutionContext, PaneId, PaneId) {
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", [] as [&str; 0]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let first = context.pane.expect("first terminal");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["-h"]),
            )
            .expect("second terminal");
        let second = context.pane.expect("second terminal");
        shared.attach(client, session).expect("attach session");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane", ["-t", &first.to_string()]),
            )
            .expect("select the first terminal");
        take_reliable_messages(mailbox);
        (client, context, first, second)
    }

    fn pane_bell(messages: &[ProtocolMessage], pane: PaneId) -> bool {
        latest_reliable_snapshot(messages).sessions[0].windows[0].panes[&pane].bell
    }

    fn bell_events(messages: &[ProtocolMessage]) -> Vec<PaneId> {
        messages
            .iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Bell { pane },
                    ..
                }) => Some(*pane),
                _ => None,
            })
            .collect()
    }

    #[cfg(unix)]
    fn ring_terminal_and_wait_for_bell(
        shared: &Arc<Shared>,
        mailbox: &OutboundMailbox,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
    ) {
        terminal.send_text("printf '\\007'\n");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut saw_edge = false;
        loop {
            saw_edge |= bell_events(&take_reliable_messages(mailbox)).contains(&pane);
            let state_belled = shared
                .inner
                .lock()
                .engine
                .state
                .pane(pane)
                .is_some_and(|pane| pane.bell);
            if saw_edge && state_belled {
                return;
            }
            assert!(Instant::now() < deadline, "bell did not publish");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_bell_publishes_one_edge_and_a_flag_that_input_clears() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, mut context, first, second) = belled_session(&shared, &mailbox);
        let background_mailbox = OutboundMailbox::new();
        shared.register_subscribed(
            ClientKind::Interactive,
            None,
            None,
            Arc::clone(&background_mailbox),
        );
        take_reliable_messages(&background_mailbox);

        shared.raise_pane_bell(second);
        let messages = take_reliable_messages(&mailbox);
        let background_messages = take_reliable_messages(&background_mailbox);
        assert_eq!(bell_events(&messages), vec![second]);
        assert_eq!(bell_events(&background_messages), vec![second]);
        assert!(pane_bell(&messages, second));
        assert!(pane_bell(&background_messages, second));
        assert!(!pane_bell(&messages, first));

        shared.raise_pane_bell(second);
        assert!(bell_events(&take_reliable_messages(&mailbox)).is_empty());

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Text {
                    pane: first,
                    text: "true\n".to_owned(),
                },
            )
            .expect("type into the quiet pane");
        assert!(
            shared.inner.lock().engine.state.windows[&context.window.expect("window")].panes
                [&second]
                .bell
        );

        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::Text {
                    pane: second,
                    text: "true\n".to_owned(),
                },
            )
            .expect("type into the belled pane");
        assert!(!pane_bell(&take_reliable_messages(&mailbox), second));

        shared.raise_pane_bell(second);
        assert_eq!(bell_events(&take_reliable_messages(&mailbox)), vec![second]);
    }

    #[test]
    fn selecting_a_belled_pane_clears_it() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, mut context, first, second) = belled_session(&shared, &mailbox);

        shared.raise_pane_bell(second);
        assert!(pane_bell(&take_reliable_messages(&mailbox), second));

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane", ["-t", &first.to_string()]),
            )
            .expect("reselect the active pane");
        assert!(
            shared.inner.lock().engine.state.windows[&context.window.expect("window")].panes
                [&second]
                .bell
        );

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane", ["-t", &second.to_string()]),
            )
            .expect("select the belled pane");
        assert!(!pane_bell(&take_reliable_messages(&mailbox), second));
    }

    #[cfg(unix)]
    #[test]
    fn activating_a_belled_window_releases_the_terminal_bell_latch() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "bell-window"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let first_window = context.window.expect("first window");
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-window", ["-d", "-n", "alert"]),
            )
            .expect("background window");
        let (alert_window, alert_pane, terminal) = {
            let inner = shared.inner.lock();
            let alert_window = inner.engine.state.sessions[&session]
                .windows
                .iter()
                .copied()
                .find(|window| *window != first_window)
                .expect("alert window");
            let alert_pane = inner.engine.state.windows[&alert_window].active_pane;
            (
                alert_window,
                alert_pane,
                Arc::clone(&inner.terminals[&alert_pane]),
            )
        };
        assert_eq!(
            shared.inner.lock().engine.state.sessions[&session].active_window,
            first_window
        );
        take_reliable_messages(&mailbox);

        ring_terminal_and_wait_for_bell(&shared, &mailbox, alert_pane, &terminal);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("next-window", ["-a"]),
            )
            .expect("activate alerted window");
        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner.engine.state.sessions[&session].active_window,
                alert_window
            );
            assert!(
                !inner
                    .engine
                    .state
                    .pane(alert_pane)
                    .expect("alert pane")
                    .bell
            );
        }
        take_reliable_messages(&mailbox);

        ring_terminal_and_wait_for_bell(&shared, &mailbox, alert_pane, &terminal);
    }

    #[cfg(unix)]
    #[test]
    fn kill_session_dash_c_releases_the_terminal_bell_latch() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "bell-clear"]),
            )
            .expect("new session");
        let session = context.session.expect("session");
        let pane = context.pane.expect("pane");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        take_reliable_messages(&mailbox);
        ring_terminal_and_wait_for_bell(&shared, &mailbox, pane, &terminal);

        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("kill-session", ["-C", "-t", &session.to_string()]),
            )
            .expect("clear session alerts");
        assert!(
            !shared
                .inner
                .lock()
                .engine
                .state
                .pane(pane)
                .expect("pane")
                .bell
        );
        take_reliable_messages(&mailbox);
        ring_terminal_and_wait_for_bell(&shared, &mailbox, pane, &terminal);
    }

    #[test]
    fn killing_the_last_session_requests_shutdown_with_no_interactive_client() {
        let shared = Arc::new(Shared::new(1));
        shared.initialize(false).expect("initialize daemon state");
        assert!(!shared.stopping.load(Ordering::Acquire));

        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut ExecutionContext::default(),
                &CommandInvocation::new("kill-session", ["-t", "0"]),
            )
            .expect("kill the last session");

        assert!(shared.inner.lock().engine.state.sessions.is_empty());
        assert!(shared.stopping.load(Ordering::Acquire));
    }

    #[test]
    fn an_attached_client_keeps_the_daemon_alive_with_zero_sessions() {
        let shared = Arc::new(Shared::new(1));
        shared.initialize(false).expect("initialize daemon state");
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());

        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut ExecutionContext::default(),
                &CommandInvocation::new("kill-session", ["-t", "0"]),
            )
            .expect("kill the last session");

        assert!(shared.inner.lock().engine.state.sessions.is_empty());
        assert!(
            !shared.stopping.load(Ordering::Acquire),
            "daemon must outlive its last session while a client is attached"
        );

        shared.unregister(client);
        assert!(shared.stopping.load(Ordering::Acquire));
    }

    #[test]
    fn disconnecting_a_client_that_still_has_sessions_keeps_the_daemon_alive() {
        let shared = Arc::new(Shared::new(1));
        shared.initialize(false).expect("initialize daemon state");
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());

        shared.unregister(client);

        assert!(!shared.inner.lock().engine.state.sessions.is_empty());
        assert!(!shared.stopping.load(Ordering::Acquire));
    }

    #[test]
    fn daemon_keeps_pty_and_mux_state_across_interactive_detach() {
        let socket = daemon_test_endpoint("daemon-test");
        let daemon = Daemon::new(&socket).without_user_config();
        let daemon_thread = thread::spawn(move || daemon.run_foreground());

        let mut commands = connect_command_retry(&socket);
        let session_name = "detach-fixture";
        commands
            .execute(CommandInvocation::new(
                "new-session",
                ["-d", "-s", session_name, terminal_line_echo_command()],
            ))
            .unwrap();
        let pane = commands
            .execute(CommandInvocation::new(
                "list-panes",
                ["-t", "detach-fixture:0", "-F", "#{pane_id}"],
            ))
            .unwrap()
            .trim()
            .parse::<PaneId>()
            .expect("fixture pane id");
        let pane_target = pane.to_string();
        let interactive = Arc::new(connect_interactive_retry(&socket));
        let (messages, reader) = spawn_reader(Arc::clone(&interactive));
        let mut terminal_state = TerminalTestState::default();
        interactive.attach(session_name).unwrap();
        wait_for(&messages, &mut terminal_state, |message, _| {
            matches!(message, ProtocolMessage::Attached { .. })
        });
        wait_for(&messages, &mut terminal_state, |_, terminal_state| {
            terminal_state
                .viewports
                .get(&pane)
                .is_some_and(|viewport| viewport_text(viewport).contains("zz-terminal-ready"))
        });

        interactive
            .execute(CommandInvocation::new(
                "copy-mode-search-prompt",
                ["-b", "-t", &pane_target],
            ))
            .unwrap();
        wait_for(&messages, &mut terminal_state, |message, _| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::TerminalUiCommand {
                        pane: target,
                        command: TerminalUiCommand::BeginSearch {
                            direction: SearchDirection::Backward,
                        },
                    },
                    ..
                })
                if *target == pane
            )
        });

        commands
            .execute(CommandInvocation::new("copy-mode", ["-t", &pane_target]))
            .unwrap();
        wait_for(&messages, &mut terminal_state, |_, terminal_state| {
            terminal_state
                .viewports
                .get(&pane)
                .is_some_and(|viewport| matches!(viewport.mode, TerminalMode::Copy { .. }))
        });
        let frozen_mode = commands
            .execute(CommandInvocation::new(
                "capture-pane",
                ["-M", "-p", "-t", &pane_target],
            ))
            .unwrap();
        commands
            .execute(CommandInvocation::new(
                "send-keys",
                ["-t", &pane_target, "MODE_LIVE_ONLY", "Enter"],
            ))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let live = commands
                .execute(CommandInvocation::new(
                    "capture-pane",
                    ["-p", "-t", &pane_target],
                ))
                .unwrap();
            if live.contains("MODE_LIVE_ONLY") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "live PTY did not advance under copy mode"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let still_frozen = commands
            .execute(CommandInvocation::new(
                "capture-pane",
                ["-M", "-p", "-t", &pane_target],
            ))
            .unwrap();
        assert_eq!(still_frozen, frozen_mode);
        assert!(!still_frozen.contains("MODE_LIVE_ONLY"));
        wait_for(&messages, &mut terminal_state, |_, terminal_state| {
            terminal_state.viewports.get(&pane).is_some_and(|viewport| {
                matches!(viewport.mode, TerminalMode::Copy { .. })
                    && viewport.unseen_output > 0
                    && !viewport_text(viewport).contains("MODE_LIVE_ONLY")
            })
        });
        commands
            .execute(CommandInvocation::new(
                "send-keys",
                ["-t", &pane_target, "-X", "cancel"],
            ))
            .unwrap();
        wait_for(&messages, &mut terminal_state, |_, terminal_state| {
            terminal_state.viewports.get(&pane).is_some_and(|viewport| {
                viewport.mode == TerminalMode::Live
                    && viewport_text(viewport).contains("MODE_LIVE_ONLY")
            })
        });

        commands
            .execute(CommandInvocation::new("new-window", ["-t", session_name]))
            .unwrap();
        wait_for(&messages, &mut terminal_state, |message, _| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) if snapshot
                    .sessions
                    .iter()
                    .find(|session| session.name == session_name)
                    .is_some_and(|session| session.windows.len() == 2)
            )
        });

        commands
            .execute(CommandInvocation::new(
                "send-keys",
                ["-t", &pane_target, "E2E_DAEMON_OK", "Enter"],
            ))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let captured = commands
                .execute(CommandInvocation::new("capture-pane", ["-t", &pane_target]))
                .unwrap();
            if captured.contains("E2E_DAEMON_OK") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "hidden terminal did not produce output"
            );
            thread::sleep(Duration::from_millis(10));
        }
        while let Ok(message) = messages.try_recv() {
            terminal_state.observe(&message);
        }
        assert!(
            terminal_state
                .viewports
                .get(&pane)
                .is_none_or(|viewport| !viewport_text(viewport).contains("E2E_DAEMON_OK")),
            "hidden windows must not receive terminal frames"
        );

        commands
            .execute(CommandInvocation::new(
                "select-window",
                ["-t", "detach-fixture:0"],
            ))
            .unwrap();
        wait_for(&messages, &mut terminal_state, |_, terminal_state| {
            terminal_state
                .viewports
                .get(&pane)
                .is_some_and(|viewport| viewport_text(viewport).contains("E2E_DAEMON_OK"))
        });
        commands
            .execute(CommandInvocation::new(
                "send-keys",
                ["-t", &pane_target, "PATCH_OK", "Enter"],
            ))
            .unwrap();
        wait_for(&messages, &mut terminal_state, |_, terminal_state| {
            terminal_state
                .viewports
                .get(&pane)
                .is_some_and(|viewport| viewport_text(viewport).contains("PATCH_OK"))
        });

        interactive.detach().unwrap();
        interactive.attach(session_name).unwrap();
        wait_for(&messages, &mut terminal_state, |message, _| {
            matches!(message, ProtocolMessage::Attached { .. })
        });
        let reattached = commands
            .execute(CommandInvocation::new(
                "capture-pane",
                ["-p", "-t", &pane_target],
            ))
            .unwrap();
        assert!(reattached.contains("E2E_DAEMON_OK"));
        assert!(reattached.contains("PATCH_OK"));

        terminal_state = TerminalTestState::default();
        interactive.request_resync().unwrap();
        wait_for(&messages, &mut terminal_state, |_, terminal_state| {
            terminal_state.viewports.get(&pane).is_some_and(|viewport| {
                let text = viewport_text(viewport);
                text.contains("E2E_DAEMON_OK") && text.contains("PATCH_OK")
            })
        });
        drop(messages);
        interactive.detach().unwrap();
        drop(interactive);
        reader.join().unwrap();

        commands
            .execute(CommandInvocation::new("kill-server", [] as [&str; 0]))
            .unwrap();
        daemon_thread.join().unwrap().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn kitty_images_and_placements_reach_an_interactive_client() {
        let socket = std::env::temp_dir().join(format!(
            "zz-kitty-e2e-{}-{}.sock",
            std::process::id(),
            server_id()
        ));
        let daemon = Daemon::new(&socket).without_user_config();
        let daemon_thread = thread::spawn(move || daemon.run_foreground());

        let mut commands = connect_command_retry(&socket);
        let interactive = Arc::new(connect_interactive_retry(&socket));
        let (messages, reader) = spawn_reader(Arc::clone(&interactive));
        let mut terminal_state = TerminalTestState::default();
        interactive.attach("").unwrap();
        wait_for(&messages, &mut terminal_state, |message, _| {
            matches!(message, ProtocolMessage::Attached { .. })
        });
        interactive
            .send_input(InputMessage::ResizeTerminal {
                pane: PaneId(0),
                columns: 100,
                rows: 30,
                cell_width_px: 8,
                cell_height_px: 18,
            })
            .unwrap();

        commands
            .execute(CommandInvocation::new(
                "send-keys",
                [
                    "-t",
                    "%0",
                    r"printf '\033_Ga=T,f=24,s=1,v=1,i=77;/wAA\033\\'",
                    "Enter",
                ],
            ))
            .unwrap();

        let mut saw_begin = false;
        let mut pixel_bytes = 0_usize;
        wait_for(&messages, &mut terminal_state, |message, terminal_state| {
            if let ProtocolMessage::Event(Event { payload, .. }) = message {
                match payload {
                    EventPayload::KittyImageBegin {
                        pane,
                        image_id,
                        width,
                        height,
                        total_bytes,
                        ..
                    } => {
                        assert_eq!((*pane, *image_id), (PaneId(0), 77));
                        assert_eq!((*width, *height, *total_bytes), (1, 1, 4));
                        saw_begin = true;
                    }
                    EventPayload::KittyImageChunk {
                        image_id, bytes, ..
                    } => {
                        assert!(saw_begin, "pixel chunk arrived before its header");
                        assert_eq!(*image_id, 77);
                        pixel_bytes += bytes.len();
                    }
                    _ => {}
                }
            }
            saw_begin
                && pixel_bytes == 4
                && terminal_state
                    .viewports
                    .get(&PaneId(0))
                    .is_some_and(|viewport| {
                        viewport
                            .kitty_placements
                            .iter()
                            .any(|placement| placement.image_id == 77)
                    })
        });

        drop(messages);
        interactive.detach().unwrap();
        drop(interactive);
        reader.join().unwrap();
        commands
            .execute(CommandInvocation::new("kill-server", [] as [&str; 0]))
            .unwrap();
        daemon_thread.join().unwrap().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn detach_keeps_one_connection_restores_views_and_clears_client_state() {
        let shared = Arc::new(Shared::new(1));
        shared.initialize(false).expect("initialize daemon state");

        let (mut client_stream, server_stream) =
            std::os::unix::net::UnixStream::pair().expect("create in-memory client connection");
        client_stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("set client read timeout");
        let connection_shared = Arc::clone(&shared);
        let connection =
            thread::spawn(move || handle_connection(server_stream, &connection_shared));

        zz_protocol::write_protocol_message(
            &mut client_stream,
            &ProtocolMessage::ClientHello(ClientHello {
                protocol_version: PROTOCOL_VERSION,
                client_instance_id: ClientInstanceId(1),
                kind: ClientKind::Interactive,
                device_name: Some("same-connection".to_owned()),
                capabilities: Vec::new(),
                color_scheme: None,
                origin: None,
            }),
        )
        .expect("send client hello");
        let client = match zz_protocol::read_protocol_message(&mut client_stream)
            .expect("receive server hello")
        {
            ProtocolMessage::ServerHello(hello) => hello.client_id,
            other => panic!("expected ServerHello, got {other:?}"),
        };

        zz_protocol::write_protocol_message(
            &mut client_stream,
            &ProtocolMessage::Attach {
                session: String::new(),
            },
        )
        .expect("send first attach");
        let (session, pane) = loop {
            if let ProtocolMessage::Attached { session, snapshot } =
                zz_protocol::read_protocol_message(&mut client_stream)
                    .expect("receive first Attached")
            {
                let pane = snapshot.sessions[0].windows[0].active_pane;
                break (session, pane);
            }
        };
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        let view = TerminalViewId(client.0);

        zz_protocol::write_protocol_message(
            &mut client_stream,
            &ProtocolMessage::Input(InputMessage::TerminalView {
                pane,
                action: TerminalViewAction::EnterCopyMode,
            }),
        )
        .expect("enter copy mode");
        let mut terminal_state = TerminalTestState::default();
        while !terminal_state
            .viewports
            .get(&pane)
            .is_some_and(|viewport| matches!(viewport.mode, TerminalMode::Copy { .. }))
        {
            let message = zz_protocol::read_protocol_message(&mut client_stream)
                .expect("receive copy-mode viewport");
            terminal_state.observe(&message);
        }
        assert_eq!(
            shared.inner.lock().key_engines[&client].active_table(),
            Some("copy-mode"),
            "entering copy mode owns the client's key table"
        );

        zz_protocol::write_protocol_message(
            &mut client_stream,
            &ProtocolMessage::Input(InputMessage::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(zz_terminal::CopyModeAction::Cancel),
            }),
        )
        .expect("leave copy mode");
        while !terminal_state
            .viewports
            .get(&pane)
            .is_some_and(|viewport| viewport.mode == TerminalMode::Live)
        {
            let message = zz_protocol::read_protocol_message(&mut client_stream)
                .expect("receive live viewport");
            terminal_state.observe(&message);
        }

        zz_protocol::write_protocol_message(
            &mut client_stream,
            &ProtocolMessage::Input(InputMessage::Key {
                pane,
                input: KeyInput {
                    action: KeyAction::Press,
                    key: KeyCode::Character('b'),
                    modifiers: Modifiers::new(false, true, false, false),
                    text: None,
                    unshifted_codepoint: Some('b'),
                },
                text_follows: false,
            }),
        )
        .expect("arm prefix");
        loop {
            let message = zz_protocol::read_protocol_message(&mut client_stream)
                .expect("receive armed-prefix event");
            terminal_state.observe(&message);
            if matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::PrefixArmed { armed: true },
                    ..
                })
            ) {
                break;
            }
        }

        let (reply, response) = crossbeam_channel::bounded(1);
        {
            let mut inner = shared.inner.lock();
            assert_eq!(inner.key_engines[&client].active_table(), Some("prefix"));
            assert!(inner.prefix_armed.contains(&client));
            assert!(inner.swallowed_keys.contains_key(&client));
            inner
                .suppressed_text
                .insert(client, BTreeMap::from([('b', 1)]));
            inner
                .pending_gui_requests
                .insert(99, PendingGuiRequest { client, reply });
        }

        zz_protocol::write_protocol_message(&mut client_stream, &ProtocolMessage::Detach)
            .expect("detach without closing the connection");
        loop {
            let message = zz_protocol::read_protocol_message(&mut client_stream)
                .expect("receive disarmed-prefix event");
            terminal_state.observe(&message);
            if matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::PrefixArmed { armed: false },
                    ..
                })
            ) {
                break;
            }
        }
        assert!(
            response
                .recv_timeout(Duration::from_secs(1))
                .expect("detaching must fail the in-flight GUI request")
                .is_err()
        );
        {
            let inner = shared.inner.lock();
            assert!(inner.subscribers.contains_key(&client));
            assert!(
                inner
                    .attached
                    .values()
                    .all(|clients| !clients.contains(&client))
            );
            assert!(!inner.visible_terminals.contains_key(&client));
            assert!(!inner.key_engines.contains_key(&client));
            assert!(!inner.prefix_armed.contains(&client));
            assert!(!inner.swallowed_keys.contains_key(&client));
            assert!(!inner.suppressed_text.contains_key(&client));
            assert!(!inner.pending_gui_requests.contains_key(&99));
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while terminal.latest_viewport_for(view).is_some() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            terminal.latest_viewport_for(view).is_none(),
            "detach must park the terminal view"
        );

        zz_protocol::write_protocol_message(
            &mut client_stream,
            &ProtocolMessage::Attach {
                session: session.to_string(),
            },
        )
        .expect("re-attach on the same connection");
        loop {
            if let ProtocolMessage::Attached {
                session: attached, ..
            } = zz_protocol::read_protocol_message(&mut client_stream)
                .expect("receive second Attached")
            {
                assert_eq!(attached, session);
                break;
            }
        }
        wait_for_viewport(
            &terminal,
            view,
            "reattached terminal view never became available",
            |_| true,
        );
        terminal_state = TerminalTestState::default();
        zz_protocol::write_protocol_message(&mut client_stream, &ProtocolMessage::Resync)
            .expect("request a full restored terminal view");
        while !terminal_state
            .viewports
            .get(&pane)
            .is_some_and(|viewport| viewport.mode == TerminalMode::Live)
        {
            let message = zz_protocol::read_protocol_message(&mut client_stream)
                .expect("receive restored terminal view");
            terminal_state.observe(&message);
        }
        {
            let inner = shared.inner.lock();
            assert_eq!(
                inner.attached.get(&session),
                Some(&BTreeSet::from([client]))
            );
            assert!(!inner.key_engines.contains_key(&client));
            assert!(!inner.copy_sessions.contains_key(&client));
            assert!(!inner.prefix_armed.contains(&client));
        }

        drop(client_stream);
        connection
            .join()
            .expect("connection handler panicked")
            .expect("connection handler failed");
    }

    fn connect_command_retry(path: &Path) -> CommandClient {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match CommandClient::connect(path) {
                Ok(client) => return client,
                Err(error) if Instant::now() >= deadline => panic!("daemon did not start: {error}"),
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    fn daemon_test_endpoint(name: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!(
                r"\\.\pipe\zz-{name}-{}-{}",
                std::process::id(),
                server_id()
            ))
        }
        #[cfg(not(windows))]
        {
            std::env::temp_dir().join(format!(
                "zz-{name}-{}-{}.sock",
                std::process::id(),
                server_id()
            ))
        }
    }

    #[cfg(not(windows))]
    fn terminal_line_echo_command() -> &'static str {
        "printf 'zz-terminal-ready\\r\\n'; exec /bin/cat"
    }

    #[cfg(windows)]
    fn terminal_line_echo_command() -> &'static str {
        "echo zz-terminal-ready & findstr .*"
    }

    fn output_view_session_fixture(
        shared: &Arc<Shared>,
        name: &str,
        text: impl Into<String>,
    ) -> (SessionId, PaneId, Arc<TerminalSession>) {
        let (session, _, pane) = shared
            .inner
            .lock()
            .engine
            .state
            .create_session(name)
            .expect("create output-view session");
        let terminal = Arc::new(TerminalSession::spawn_output_view(
            format!("{name} fixture"),
            text.into(),
        ));
        shared
            .inner
            .lock()
            .terminals
            .insert(pane, Arc::clone(&terminal));
        shared
            .watch_terminal(pane, &terminal)
            .expect("watch output-view terminal");
        (session, pane, terminal)
    }

    fn terminal_test_message(pane: PaneId, sequence: u64, generation: u64) -> ProtocolMessage {
        let mut viewport = zz_terminal::TerminalViewport::blank(2, 2, SessionStatus::Running);
        viewport.generation = generation;
        viewport.view_generation = generation;
        ProtocolMessage::Event(Event {
            sequence,
            payload: EventPayload::TerminalViewport { pane, viewport },
        })
    }

    fn terminal_patch_test_message(
        pane: PaneId,
        sequence: u64,
        base_generation: u64,
        generation: u64,
    ) -> ProtocolMessage {
        let mut previous = zz_terminal::TerminalViewport::blank(2, 2, SessionStatus::Running);
        previous.generation = base_generation;
        previous.view_generation = base_generation;
        let mut current = previous.clone();
        current.generation = generation;
        current.view_generation = generation;
        let patch = zz_terminal::TerminalViewport::diff(&previous, &current).expect("test patch");
        ProtocolMessage::Event(Event {
            sequence,
            payload: EventPayload::TerminalPatch { pane, patch },
        })
    }

    fn command_output_test_message(
        pane: PaneId,
        sequence: u64,
        generation: u64,
    ) -> ProtocolMessage {
        let mut viewport = zz_terminal::TerminalViewport::blank(2, 2, SessionStatus::Running);
        viewport.generation = generation;
        viewport.view_generation = generation;
        viewport.mode = zz_terminal::TerminalMode::View {
            position: 1,
            total: 1,
        };
        ProtocolMessage::Event(Event {
            sequence,
            payload: EventPayload::CommandOutput {
                pane,
                viewport: Some(viewport),
            },
        })
    }

    fn take_command_output_message(mailbox: &OutboundMailbox) -> ProtocolMessage {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let frame = {
                let mut state = mailbox.state.lock();
                let frame = state.command_output.take();
                if let Some(frame) = &frame {
                    state.queued_bytes = state.queued_bytes.saturating_sub(frame.len());
                }
                frame
            };
            if let Some(frame) = frame {
                return decode_protocol_frame(&frame).expect("decode command-output viewport");
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for command-output viewport"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_command_output_close(mailbox: &OutboundMailbox) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let closed = take_reliable_messages(mailbox).into_iter().any(|message| {
                matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::CommandOutput { viewport: None, .. },
                        ..
                    })
                )
            });
            if closed {
                return;
            }
            assert!(Instant::now() < deadline, "output view did not close");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[track_caller]
    fn wait_for_mailbox_terminal(
        mailbox: &OutboundMailbox,
        terminal_state: &mut TerminalTestState,
        mut predicate: impl FnMut(&ProtocolMessage, &TerminalTestState) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let available = {
                let state = mailbox.state.lock();
                !state.reliable.is_empty()
                    || state.command_output.is_some()
                    || !state.terminals.is_empty()
            };
            if available {
                let frame = mailbox.recv().expect("available mailbox frame");
                let message = decode_protocol_frame(&frame).expect("decode mailbox frame");
                terminal_state.observe(&message);
                if predicate(&message, terminal_state) {
                    return;
                }
            } else {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for terminal mailbox update"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn wait_for_terminal_dimensions(
        terminal: &TerminalSession,
        view: TerminalViewId,
        columns: u16,
        rows: u16,
    ) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if terminal
                .latest_viewport_for(view)
                .is_some_and(|viewport| viewport.columns == columns && viewport.rows == rows)
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "terminal did not reach {columns}x{rows}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn take_reliable_messages(mailbox: &OutboundMailbox) -> Vec<ProtocolMessage> {
        let frames = {
            let mut state = mailbox.state.lock();
            let frames = state.reliable.drain(..).collect::<Vec<_>>();
            let bytes = frames.iter().map(Vec::len).sum::<usize>();
            state.queued_bytes = state.queued_bytes.saturating_sub(bytes);
            frames
        };
        frames
            .into_iter()
            .map(|frame| decode_protocol_frame(&frame).expect("decode reliable message"))
            .collect()
    }

    fn latest_reliable_snapshot(messages: &[ProtocolMessage]) -> &MuxSnapshot {
        messages
            .iter()
            .rev()
            .find_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) => Some(snapshot),
                _ => None,
            })
            .expect("reliable snapshot event")
    }

    fn take_clipboard_writes(
        mailbox: &OutboundMailbox,
        pane: PaneId,
    ) -> Vec<(ClipboardTarget, String)> {
        take_reliable_messages(mailbox)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::Clipboard {
                            pane: written,
                            target,
                            text,
                            ..
                        },
                    ..
                }) if written == pane => Some((target, text)),
                _ => None,
            })
            .collect()
    }

    fn take_browser_literals(mailbox: &OutboundMailbox, pane: PaneId) -> Vec<String> {
        take_reliable_messages(mailbox)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::BrowserCommand {
                            pane: target,
                            command: BrowserCommand::SendKeys(keys),
                        },
                    ..
                }) if target == pane => Some(keys),
                _ => None,
            })
            .flatten()
            .filter_map(|key| match key {
                zz_protocol::KeyToken::Literal(text) => Some(text),
                zz_protocol::KeyToken::Named(_) => None,
            })
            .collect()
    }

    fn test_key(key: KeyCode, modifiers: Modifiers, text: Option<&str>) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key,
            modifiers,
            text: text.map(Box::<str>::from),
            unshifted_codepoint: match key {
                KeyCode::Character(character) => Some(character),
                _ => None,
            },
        }
    }

    fn connect_interactive_retry(path: &Path) -> InteractiveClient {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match InteractiveClient::connect(path) {
                Ok(client) => return client,
                Err(error) if Instant::now() >= deadline => panic!("daemon did not start: {error}"),
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    fn spawn_reader(
        client: Arc<InteractiveClient>,
    ) -> (Receiver<ProtocolMessage>, thread::JoinHandle<()>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let reader = thread::spawn(move || {
            while let Ok(message) = client.recv() {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        (receiver, reader)
    }

    #[derive(Default)]
    struct TerminalTestState {
        viewports: BTreeMap<PaneId, zz_terminal::TerminalViewport>,
    }

    impl TerminalTestState {
        fn observe(&mut self, message: &ProtocolMessage) {
            let ProtocolMessage::Event(Event { payload, .. }) = message else {
                return;
            };
            match payload {
                EventPayload::TerminalViewport { pane, viewport } => {
                    self.viewports.insert(*pane, viewport.clone());
                }
                EventPayload::TerminalPatch { pane, patch } => {
                    if let Some(viewport) = self.viewports.get_mut(pane) {
                        viewport
                            .apply_patch(patch.clone())
                            .expect("daemon emitted an applicable terminal patch");
                    }
                }
                _ => {}
            }
        }
    }

    #[track_caller]
    fn wait_for(
        messages: &Receiver<ProtocolMessage>,
        terminal_state: &mut TerminalTestState,
        mut predicate: impl FnMut(&ProtocolMessage, &TerminalTestState) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = messages
                .recv_timeout(remaining)
                .expect("timed out waiting for daemon event");
            terminal_state.observe(&message);
            if predicate(&message, terminal_state) {
                return;
            }
        }
    }

    fn viewport_text(viewport: &zz_terminal::TerminalViewport) -> String {
        let mut output = String::new();
        for cell in viewport.cells.iter() {
            viewport.push_glyph(*cell, &mut output);
        }
        output
    }

    #[cfg(feature = "agent")]
    mod agent {
        use zz_protocol::{AgentImage as WireAgentImage, AgentProvider};

        use super::*;
        use crate::agent::{
            fixture::{Behavior, fixture_runner},
            journal::AgentJournal,
            stream::{AgentStreamItem, AgentStreamPayload},
        };

        const DEADLINE: Duration = Duration::from_secs(10);

        fn take_agent_frames(mailbox: &OutboundMailbox) -> Vec<ProtocolMessage> {
            let frames = {
                let mut state = mailbox.state.lock();
                let panes = state.agent_order.drain(..).collect::<Vec<_>>();
                let mut frames = Vec::new();
                for pane in panes {
                    let Some(queued) = state.agent.remove(&pane) else {
                        continue;
                    };
                    state.queued_bytes = state.queued_bytes.saturating_sub(queued.bytes);
                    frames.extend(queued.frames.into_iter().map(|(_, frame)| frame));
                }
                frames
            };
            frames
                .into_iter()
                .map(|frame| decode_protocol_frame(&frame).expect("decode agent frame"))
                .collect()
        }

        /// Every stream item a mailbox has been handed, in delivery order.
        fn drain_items(mailbox: &OutboundMailbox) -> Vec<AgentStreamItem> {
            take_agent_frames(mailbox)
                .into_iter()
                .filter_map(|message| match message {
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::AgentUpdates { items, .. },
                        ..
                    }) => Some(items),
                    _ => None,
                })
                .flatten()
                .map(|item| serde_json::from_slice(&item).expect("decode stream item"))
                .collect()
        }

        fn chunk_text(item: &AgentStreamItem) -> Option<&str> {
            let AgentStreamPayload::Update { update } = &item.payload else {
                return None;
            };
            update.get("content")?.get("text")?.as_str()
        }

        fn encoded_updates(pane: PaneId, first_seq: u64, items: usize, bytes: usize) -> Vec<u8> {
            let message = Shared::event(EventPayload::AgentUpdates {
                pane,
                first_seq,
                items: (0..items).map(|_| vec![b'x'; bytes]).collect(),
            });
            encode_protocol_message(&message).expect("encode agent updates")
        }

        struct Workspace {
            shared: Arc<Shared>,
            mailbox: Arc<OutboundMailbox>,
            client: ClientId,
            session: SessionId,
            agent: PaneId,
            runtime: Arc<crate::agent::fanout::AgentRuntime>,
            _journal: tempfile::TempDir,
        }

        /// A daemon with one attached client and one materialized agent pane,
        /// opened against the in-process fixture adapter.
        fn workspace(behavior: Behavior) -> Workspace {
            let shared = Arc::new(Shared::new(1));
            let mailbox = OutboundMailbox::new();
            let (client, _) = shared.register_subscribed(
                ClientKind::Interactive,
                None,
                None,
                Arc::clone(&mailbox),
            );
            let directory = tempfile::tempdir().expect("journal directory");
            let journal = Arc::new(AgentJournal::open(directory.path()).expect("open journal"));
            let runtime = shared
                .build_agent_runtime(Some(journal))
                .expect("agent runtime");
            runtime.set_runner_factory(Box::new(move |_| {
                fixture_runner(AgentProvider::Codex, behavior, false, true)
            }));

            let mut context = ExecutionContext::default();
            for command in [
                CommandInvocation::new("new-session", ["-s", "agents"]),
                CommandInvocation::new("set-option", ["-g", "experimental-agent-pane", "on"]),
                CommandInvocation::new("split-picker", ["-v"]),
            ] {
                shared
                    .execute(client, ClientKind::Interactive, &mut context, &command)
                    .expect("workspace command");
            }
            let session = context.session.expect("session");
            let agent = context.pane.expect("picker");
            shared
                .execute(
                    client,
                    ClientKind::Interactive,
                    &mut context,
                    &CommandInvocation::new(
                        "select-pane-kind",
                        ["-t", &agent.to_string(), "agent"],
                    ),
                )
                .expect("agent pane");
            shared.attach(client, session).expect("attach");
            shared.send_resync(client, &mailbox);
            take_reliable_messages(&mailbox);
            Workspace {
                shared,
                mailbox,
                client,
                session,
                agent,
                runtime,
                _journal: directory,
            }
        }

        impl Workspace {
            #[track_caller]
            fn wait_for_items<F>(&self, what: &str, accept: F) -> Vec<AgentStreamItem>
            where
                F: Fn(&[AgentStreamItem]) -> bool,
            {
                let deadline = Instant::now() + DEADLINE;
                let mut seen = Vec::new();
                while Instant::now() < deadline {
                    seen.extend(drain_items(&self.mailbox));
                    if accept(&seen) {
                        return seen;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                panic!("timed out waiting for {what}: {seen:#?}");
            }

            fn prompt(&self, text: &str) -> Result<(), ServerError> {
                self.shared.agent_message(
                    self.client,
                    ProtocolMessage::AgentPrompt {
                        pane: self.agent,
                        text: text.to_owned(),
                        images: Vec::new(),
                    },
                )
            }
        }

        impl Drop for Workspace {
            fn drop(&mut self) {
                self.shared.shutdown_agents();
            }
        }

        #[test]
        fn the_agent_lane_batches_per_pane_and_drains_behind_the_reliable_one() {
            let mailbox = OutboundMailbox::new();
            let first = PaneId(1);
            let second = PaneId(2);

            assert!(mailbox.enqueue_agent(first, 1, encoded_updates(first, 1, 2, 16)));
            assert!(mailbox.enqueue_agent(second, 1, encoded_updates(second, 1, 1, 16)));
            assert!(mailbox.enqueue_agent(first, 3, encoded_updates(first, 3, 1, 16)));
            assert!(mailbox.enqueue_reliable(&Shared::event(EventPayload::Bell { pane: first })));

            let panes = std::iter::from_fn(|| mailbox.recv())
                .take(4)
                .map(
                    |frame| match decode_protocol_frame(&frame).expect("decode") {
                        ProtocolMessage::Event(Event {
                            payload:
                                EventPayload::AgentUpdates {
                                    pane, first_seq, ..
                                },
                            ..
                        }) => (Some(pane), first_seq),
                        _ => (None, 0),
                    },
                )
                .collect::<Vec<_>>();

            assert_eq!(
                panes,
                [
                    (None, 0),
                    (Some(first), 1),
                    (Some(second), 1),
                    (Some(first), 3)
                ],
                "the reliable lane goes first, then one agent frame per pane per turn"
            );
        }

        #[test]
        fn agent_request_replies_reach_only_the_requesting_client() {
            let shared = Arc::new(Shared::new(1));
            let first = OutboundMailbox::new();
            let second = OutboundMailbox::new();
            let (first_client, _) =
                shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&first));
            let (_second_client, _) = shared.register_subscribed(
                ClientKind::Interactive,
                None,
                None,
                Arc::clone(&second),
            );

            shared.send_agent_reply(
                PaneId(4),
                AgentRequestReply::Sessions {
                    client: first_client,
                    result: "history".to_owned(),
                },
            );

            assert!(
                take_reliable_messages(&first)
                    .iter()
                    .any(|message| matches!(
                        message,
                        ProtocolMessage::Event(Event {
                            payload: EventPayload::AgentSessions { result, .. },
                            ..
                        }) if result == "history"
                    ))
            );
            assert!(take_reliable_messages(&second).is_empty());
        }

        #[test]
        fn an_overflowing_agent_lane_asks_for_a_replay_instead_of_closing() {
            let mailbox = OutboundMailbox::new();
            let pane = PaneId(1);
            let chunk = MAX_PENDING_AGENT_BYTES / 4;

            for seq in 0..3 {
                assert!(mailbox.enqueue_agent(
                    pane,
                    seq * 10 + 1,
                    encoded_updates(pane, seq * 10 + 1, 1, chunk)
                ));
            }
            // Three frames of a quarter of the cap each fit; the fourth,
            // with its framing overhead, does not.
            assert!(mailbox.enqueue_agent(pane, 31, encoded_updates(pane, 31, 1, chunk)));

            {
                let state = mailbox.state.lock();
                assert!(!state.closed, "a slow client is not disconnected");
                assert!(state.agent.is_empty(), "the pane's lane was cleared");
            }
            let frame = mailbox.recv().expect("the lag marker");
            assert!(matches!(
                decode_protocol_frame(&frame).expect("decode"),
                ProtocolMessage::Event(Event {
                    payload: EventPayload::AgentLagged { pane: lagged, next_seq: 1 },
                    ..
                }) if lagged == pane
            ));
        }

        #[test]
        fn a_multiframe_agent_replay_is_admitted_atomically() {
            let mailbox = OutboundMailbox::new();
            let pane = PaneId(1);
            let chunk = MAX_PENDING_AGENT_BYTES / 3;
            let frames = (0..4)
                .map(|index| {
                    let first_seq = index * 10 + 1;
                    (first_seq, encoded_updates(pane, first_seq, 1, chunk))
                })
                .collect();

            assert!(mailbox.enqueue_agent_replay(pane, frames));
            {
                let state = mailbox.state.lock();
                let queued = state.agent.get(&pane).expect("replay lane");
                assert!(queued.bytes > MAX_PENDING_AGENT_BYTES);
                assert_eq!(queued.frames.len(), 4);
                assert!(state.reliable.is_empty());
            }

            for expected in [1, 11, 21, 31] {
                let frame = mailbox.recv().expect("replay frame");
                assert!(matches!(
                    decode_protocol_frame(&frame).expect("decode"),
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::AgentUpdates {
                            pane: target,
                            first_seq,
                            ..
                        },
                        ..
                    }) if target == pane && first_seq == expected
                ));
            }
        }

        #[test]
        fn agent_updates_reach_only_the_clients_the_pane_is_visible_to() {
            let workspace = workspace(Behavior::Chunk);
            let elsewhere = OutboundMailbox::new();
            let (watcher, _) = workspace.shared.register_subscribed(
                ClientKind::Interactive,
                None,
                None,
                Arc::clone(&elsewhere),
            );
            workspace
                .shared
                .attach(watcher, workspace.session)
                .expect("attach the second client");
            let mut context = ExecutionContext::default();
            workspace
                .shared
                .execute(
                    watcher,
                    ClientKind::Interactive,
                    &mut context,
                    &CommandInvocation::new("new-window", ["-t", "agents"]),
                )
                .expect("a window of its own");
            workspace.shared.publish_snapshot();
            take_reliable_messages(&elsewhere);
            let _ = drain_items(&elsewhere);

            workspace.prompt("go").expect("prompt");
            workspace.wait_for_items("the turn", |items| {
                items.iter().any(|item| chunk_text(item) == Some("turn 0"))
            });

            assert!(
                drain_items(&elsewhere).is_empty(),
                "a client looking at another window is not sent the transcript"
            );
            assert!(
                take_reliable_messages(&elsewhere)
                    .iter()
                    .any(|message| matches!(
                        message,
                        ProtocolMessage::Event(Event {
                            payload: EventPayload::AgentState { pane, .. },
                            ..
                        }) if *pane == workspace.agent
                    )),
                "but it still sees the pane state its badges need"
            );
        }

        #[test]
        fn a_submitted_agent_send_is_dispatched_by_the_daemon_itself() {
            let workspace = workspace(Behavior::Chunk);
            let mut context = ExecutionContext::default();

            let execution = workspace
                .shared
                .execute(
                    ClientId(99),
                    ClientKind::Command,
                    &mut context,
                    &CommandInvocation::new(
                        "agent-send",
                        [
                            "-t",
                            &workspace.agent.to_string(),
                            "--submit",
                            "review this",
                        ],
                    ),
                )
                .expect("agent-send reaches the runtime without a GUI");
            assert!(execution.output.is_empty());

            let items = workspace.wait_for_items("the dispatched turn", |items| {
                items.iter().any(|item| chunk_text(item) == Some("turn 0"))
            });
            assert!(items.iter().any(|item| matches!(
                &item.payload,
                AgentStreamPayload::SessionReady { session_id, .. } if session_id == "fixture-session"
            )));
            let title = workspace
                .shared
                .inner
                .lock()
                .engine
                .state
                .pane(workspace.agent)
                .expect("the agent pane")
                .title
                .clone();
            assert_eq!(title, "review this", "the first prompt names the pane");
        }

        #[test]
        fn the_agent_options_reconfigure_what_the_next_pane_spawns() {
            let workspace = workspace(Behavior::Chunk);
            let mut context = ExecutionContext::default();

            assert_eq!(
                workspace.runtime.spawn_config().command,
                zz_protocol::DEFAULT_AGENT_COMMAND
            );
            for (option, value) in [
                ("agent-command", "my-codex --acp"),
                ("agent-claude-code-command", "my-claude --acp"),
                ("agent-auto-approve", "off"),
            ] {
                workspace
                    .shared
                    .execute(
                        workspace.client,
                        ClientKind::Interactive,
                        &mut context,
                        &CommandInvocation::new("set-option", ["-g", "--", option, value]),
                    )
                    .expect("set an agent option");
            }

            let config = workspace.runtime.spawn_config();
            assert_eq!(config.command, "my-codex --acp");
            assert_eq!(config.claude_code_command, "my-claude --acp");
            assert!(!config.auto_approve);
            assert_eq!(
                workspace
                    .shared
                    .inner
                    .lock()
                    .mux_options
                    .get(MuxOptionKey::AgentCommand)
                    .expect("the option is published")
                    .value,
                "my-codex --acp"
            );
        }

        #[test]
        fn provider_switch_and_retry_replace_the_daemon_runtime() {
            let workspace = workspace(Behavior::Chunk);
            let initial = workspace.wait_for_items("the initial session", |items| {
                items
                    .iter()
                    .any(|item| matches!(item.payload, AgentStreamPayload::SessionReady { .. }))
            });
            let last_initial = initial.last().expect("initial stream").seq;
            let providers = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&providers);
            workspace.runtime.set_runner_factory(Box::new(move |spec| {
                seen.lock().push(spec.provider);
                fixture_runner(spec.provider, Behavior::Chunk, false, true)
            }));

            let mut context = ExecutionContext::default();
            workspace
                .shared
                .execute(
                    workspace.client,
                    ClientKind::Interactive,
                    &mut context,
                    &CommandInvocation::new(
                        "set-agent-provider",
                        ["-t", &workspace.agent.to_string(), "claude-code"],
                    ),
                )
                .expect("switch provider");
            let switched = workspace.wait_for_items("the replacement session", |items| {
                items.iter().any(|item| {
                    matches!(item.payload, AgentStreamPayload::SessionReady { .. })
                        && item.seq > last_initial
                })
            });
            assert!(switched.iter().all(|item| item.seq > last_initial));
            assert_eq!(&*providers.lock(), &[AgentProvider::ClaudeCode]);
            workspace.shared.adopt_agent_session(
                workspace.agent,
                AgentProvider::Codex,
                "stale-codex-session".to_owned(),
                None,
            );
            {
                let inner = workspace.shared.inner.lock();
                let PaneKind::Agent(descriptor) = &inner
                    .engine
                    .state
                    .pane(workspace.agent)
                    .expect("agent pane")
                    .kind
                else {
                    unreachable!()
                };
                assert_eq!(descriptor.provider, AgentProvider::ClaudeCode);
                assert_ne!(
                    descriptor.session_id.as_deref(),
                    Some("stale-codex-session")
                );
            }

            workspace
                .shared
                .execute(
                    workspace.client,
                    ClientKind::Interactive,
                    &mut context,
                    &CommandInvocation::new(
                        "restart-agent-pane",
                        ["-t", &workspace.agent.to_string()],
                    ),
                )
                .expect("retry pane");
            assert_eq!(
                &*providers.lock(),
                &[AgentProvider::ClaudeCode, AgentProvider::ClaudeCode]
            );
        }

        #[test]
        fn a_reattaching_client_gets_the_pane_state_and_replays_the_transcript() {
            let workspace = workspace(Behavior::Chunk);
            workspace
                .shared
                .agent_message(
                    workspace.client,
                    ProtocolMessage::AgentPrompt {
                        pane: workspace.agent,
                        text: "go".to_owned(),
                        images: vec![WireAgentImage {
                            format: "image/png".to_owned(),
                            data: b"zz".to_vec(),
                        }],
                    },
                )
                .expect("prompt");
            let live = workspace.wait_for_items("the first turn", |items| {
                items.iter().any(|item| chunk_text(item) == Some("turn 0"))
            });
            assert_eq!(
                live.iter().map(|item| item.seq).collect::<Vec<_>>(),
                (1..=live.len() as u64).collect::<Vec<_>>(),
                "a live client sees every sequence, in order"
            );

            workspace.shared.detach(workspace.client);
            workspace.prompt("again").expect("prompt while detached");
            let deadline = Instant::now() + DEADLINE;
            while Instant::now() < deadline
                && workspace
                    .runtime
                    .wire_state(workspace.agent)
                    .is_none_or(|state| state.session_id.is_none())
            {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(
                drain_items(&workspace.mailbox).is_empty(),
                "a detached client is sent nothing"
            );

            workspace
                .shared
                .attach(workspace.client, workspace.session)
                .expect("reattach");
            workspace
                .shared
                .send_resync(workspace.client, &workspace.mailbox);
            assert!(
                take_reliable_messages(&workspace.mailbox)
                    .iter()
                    .any(|message| matches!(
                        message,
                        ProtocolMessage::Event(Event {
                            payload: EventPayload::AgentState { pane, state },
                            ..
                        }) if *pane == workspace.agent && state.session_id.as_deref() == Some("fixture-session")
                    )),
                "a resync carries the pane state"
            );

            workspace
                .shared
                .agent_message(
                    workspace.client,
                    ProtocolMessage::AgentReplay {
                        pane: workspace.agent,
                        from_seq: 0,
                    },
                )
                .expect("replay");
            let replayed = workspace.wait_for_items("the replayed transcript", |items| {
                items.iter().any(|item| chunk_text(item) == Some("turn 1"))
            });
            assert_eq!(
                replayed.first().map(|item| item.seq),
                Some(1),
                "the replay starts at the beginning for a client that kept nothing"
            );
            assert_eq!(
                replayed.iter().map(|item| item.seq).collect::<Vec<_>>(),
                (1..=replayed.len() as u64).collect::<Vec<_>>(),
                "and converges on the same contiguous transcript"
            );
            assert_eq!(
                replayed.iter().filter_map(chunk_text).collect::<Vec<_>>(),
                ["turn 0", "turn 1"]
            );
        }
    }
}
