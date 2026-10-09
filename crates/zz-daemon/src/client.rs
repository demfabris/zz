use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsString,
    fmt,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
};

use parking_lot::Mutex;
use zz_protocol::{
    AgentImage, AgentSessionOpKind, ClientFileOperation, ClientFileRequest, ClientFileResponse,
    ClientHello, ClientInstanceId, ClientKind, ClientPath, CommandInvocation, CommandRequest,
    CommandResponse, ConfigOverrideEntry, GuiResponse, InputMessage, MAX_CLIENT_ENVIRONMENT_BYTES,
    MAX_CLIENT_ENVIRONMENT_ENTRIES, MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES, MAX_CLIENT_FILE_BYTES,
    MAX_CLIENT_WORKING_DIRECTORY_BYTES, MAX_PASTE_UPLOAD_CHUNK_BYTES, PANE_FRAME_CAPABILITY,
    PROTOCOL_VERSION, PaneId, PasteUploadPurpose, PreparedCommand, PreparedCommandResult,
    ProtocolError, ProtocolMessage, RawText, ServerError, ServerHello, StdoutClaim,
    encode_key_input_into, encode_protocol_message_into, read_protocol_message_into,
};
use zz_protocol::{
    ClientEnvironmentBlob, EXEC_CAPABILITY, ExecFlags, ExecOutcome, ExecRequest, ExecResume,
    ExecResumeKind,
};

/// `EIO`, the error the pin's client reports for anything that fails after the
/// file opened.
const CLIENT_FILE_READ_ERRNO: i32 = 5;

#[cfg(any(target_os = "linux", target_os = "macos"))]
static STDIN_WAS_CLOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[used]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".init_array"))]
#[cfg_attr(target_os = "macos", unsafe(link_section = "__DATA,__mod_init_func"))]
#[allow(
    unsafe_code,
    reason = "capture descriptor validity before Rust sanitizes standard input"
)]
static CAPTURE_STDIN: extern "C" fn() = capture_stdin;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(
    unsafe_code,
    reason = "fcntl and errno inspect fd 0 without allocating or changing it"
)]
extern "C" fn capture_stdin() {
    unsafe {
        if libc::fcntl(0, libc::F_GETFD) == -1 {
            #[cfg(target_os = "linux")]
            let error = *libc::__errno_location();
            #[cfg(target_os = "macos")]
            let error = *libc::__error();
            STDIN_WAS_CLOSED.store(error == libc::EBADF, Ordering::Relaxed);
        }
    }
}

static CLIENT_INSTANCE_ID: OnceLock<ClientInstanceId> = OnceLock::new();

fn client_instance_id() -> ClientInstanceId {
    *CLIENT_INSTANCE_ID.get_or_init(|| ClientInstanceId(getrandom::u64().unwrap_or(1).max(1)))
}
use zz_terminal::TerminalColorScheme;

// iOS cannot run ssh, so it tunnels in-process instead of forwarding a socket.
#[cfg(all(any(unix, windows), not(target_os = "ios")))]
use crate::endpoint::SshForward;
#[cfg(target_os = "ios")]
use crate::russh_client::{RusshForward, RusshStream};
use crate::{
    DaemonError, diagnostic_elapsed_us, diagnostic_timer,
    endpoint::Endpoint,
    transport::{LocalStream, LocalTransport, Transport, TransportStream},
};

pub(crate) enum ClientStream {
    Local(LocalStream),
    #[cfg(target_os = "ios")]
    Ssh(RusshStream),
}

impl Read for ClientStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Local(stream) => stream.read(buffer),
            #[cfg(target_os = "ios")]
            Self::Ssh(stream) => stream.read(buffer),
        }
    }
}

impl Write for ClientStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Local(stream) => stream.write(buffer),
            #[cfg(target_os = "ios")]
            Self::Ssh(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Local(stream) => stream.flush(),
            #[cfg(target_os = "ios")]
            Self::Ssh(stream) => stream.flush(),
        }
    }
}

#[cfg(unix)]
impl ClientStream {
    fn shutdown(&self) -> io::Result<()> {
        match self {
            Self::Local(stream) => stream.shutdown(),
            #[cfg(target_os = "ios")]
            Self::Ssh(_) => Ok(()),
        }
    }
}

impl TransportStream for ClientStream {
    #[cfg(unix)]
    fn receive_fd(&self) -> io::Result<std::os::fd::OwnedFd> {
        match self {
            Self::Local(stream) => stream.receive_fd(),
            #[cfg(target_os = "ios")]
            Self::Ssh(_) => Err(io::Error::from(io::ErrorKind::Unsupported)),
        }
    }

    #[cfg(unix)]
    fn read_ready(&self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Local(stream) => stream.read_ready(buffer),
            #[cfg(target_os = "ios")]
            Self::Ssh(_) => Err(io::Error::from(io::ErrorKind::Unsupported)),
        }
    }

    fn try_clone(&self) -> io::Result<Self> {
        match self {
            Self::Local(stream) => stream.try_clone().map(Self::Local),
            #[cfg(target_os = "ios")]
            Self::Ssh(stream) => stream.try_clone().map(Self::Ssh),
        }
    }
}

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Both output streams and the exit status of one completed command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutcome {
    pub stdout: RawText,
    pub stderr: String,
    pub exit_code: u8,
    /// Which of the pin's two stdout writers claimed the command client's
    /// stream while `stdout` was produced. The CLI writer reads this instead of
    /// guessing from the bytes.
    pub stdout_claim: StdoutClaim,
    /// The pin's `CLIENT_EXIT`: the daemon asked this client to stop before the
    /// rest of its command chain runs.
    pub client_exit: bool,
}

pub type ExecClassifier<'a> =
    &'a dyn Fn(&[CommandInvocation], &[PreparedCommand]) -> Option<ExecResumeKind>;

pub struct ExecChain<'a> {
    pub commands: Vec<CommandInvocation>,
    pub spawned_server_id: Option<u64>,
    pub expect_server_id: Option<u64>,
    pub resume: Option<ExecClassifier<'a>>,
    pub prepared: bool,
    pub last: bool,
}

impl ExecChain<'_> {
    #[must_use]
    pub fn new(commands: Vec<CommandInvocation>) -> Self {
        Self {
            commands,
            spawned_server_id: None,
            expect_server_id: None,
            resume: None,
            prepared: false,
            last: false,
        }
    }
}

#[derive(Debug)]
pub enum ExecChainEnd {
    Ran { exit_code: u8 },
    Failed(DaemonError),
    Resume(ExecResume),
    Rejected(ServerError),
    ServerMismatch,
}

pub struct CommandClient {
    stdin_enabled: bool,
    stdin_spent: bool,
    stderr_handler: Option<fn(&str)>,
    stdout_handler: Option<fn(&RawText)>,
    link: CommandLink,
    route: CommandRoute,
    server_id: Option<u64>,
    #[cfg(all(any(unix, windows), not(target_os = "ios")))]
    _ssh_forward: Option<SshForward>,
}

enum CommandLink {
    Exec {
        reader: ProtocolReceiver<LocalStream>,
        writer: ProtocolSender<LocalStream>,
        answered: bool,
        spent: bool,
    },
    Legacy {
        reader: ProtocolReceiver<LocalStream>,
        writer: ProtocolSender<LocalStream>,
        server_id: u64,
    },
}

struct CommandRoute {
    socket: PathBuf,
    display: String,
    facts: EndpointFactsScope,
    send_origin: bool,
}

fn attach_session_command(
    session: String,
    detach_others: bool,
    read_only: bool,
    client_flags: Option<&str>,
) -> CommandInvocation {
    let mut args = Vec::new();
    if detach_others {
        args.push("-d".to_owned());
    }
    if read_only {
        args.push("-r".to_owned());
    }
    if let Some(client_flags) = client_flags {
        args.extend(["-f".to_owned(), client_flags.to_owned()]);
    }
    if !session.is_empty() {
        args.extend(["-t".to_owned(), session]);
    }
    CommandInvocation::new("attach-session", args)
}

fn stdout_or_exit(outcome: CommandOutcome) -> Result<String, DaemonError> {
    if outcome.exit_code == 0 {
        Ok(outcome.stdout.to_string())
    } else {
        Err(DaemonError::CommandExit {
            output: outcome.stdout,
            exit_code: outcome.exit_code,
        })
    }
}

fn exec_maybe_unsupported(error: &DaemonError) -> bool {
    match error {
        DaemonError::Protocol(ProtocolError::Decode(_)) => true,
        DaemonError::Protocol(ProtocolError::Io(error)) | DaemonError::Io(error) => {
            error.kind() == io::ErrorKind::UnexpectedEof
        }
        _ => false,
    }
}

fn command_failure(error: ServerError, output: RawText) -> DaemonError {
    let error = DaemonError::Server(error);
    if output.is_empty() {
        error
    } else {
        DaemonError::CommandFailed {
            output,
            error: Box::new(error),
        }
    }
}

fn exec_environment() -> ClientEnvironmentBlob {
    exec_environment_with(std::env::vars_os())
}

fn exec_environment_with<I, K, V>(environment: I) -> ClientEnvironmentBlob
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
{
    let mut blob = Vec::new();
    let mut count = 0usize;
    let mut total = 0usize;
    for (name, value) in environment {
        if count == MAX_CLIENT_ENVIRONMENT_ENTRIES {
            break;
        }
        let name = RawText::from_os_str(&name.into()).into_bytes();
        let value = RawText::from_os_str(&value.into()).into_bytes();
        if name.is_empty() || name.contains(&b'=') || name.contains(&0) || value.contains(&0) {
            continue;
        }
        let entry_bytes = name.len() + 1 + value.len();
        if entry_bytes > MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES
            || total + entry_bytes > MAX_CLIENT_ENVIRONMENT_BYTES
        {
            continue;
        }
        blob.extend_from_slice(&name);
        blob.push(b'=');
        blob.extend_from_slice(&value);
        blob.push(0);
        count += 1;
        total += entry_bytes;
    }
    ClientEnvironmentBlob::from_bytes(blob)
}

fn exec_feature_mask() -> u32 {
    crate::terminal_features::terminal_feature_mask(
        client_terminal_flags()
            .features
            .iter()
            .filter(|spec| spec.len() <= MAX_CLIENT_FEATURE_SPEC_BYTES)
            .take(MAX_CLIENT_FEATURE_SPECS)
            .map(String::as_str),
    ) | LEARNED_TERMINAL_FEATURES.load(Ordering::Relaxed)
}

impl CommandRoute {
    fn exec_request(
        &self,
        commands: Vec<CommandInvocation>,
        flags: ExecFlags,
        spawned_server_id: Option<u64>,
        expect_server_id: Option<u64>,
    ) -> ExecRequest {
        let mut flags = flags;
        flags.set(ExecFlags::UTF8, client_takes_utf8_terminal());
        flags.set(
            ExecFlags::NESTED,
            self.facts.includes_tty()
                && std::env::var_os("TMUX").is_some_and(|value| !value.is_empty()),
        );
        ExecRequest {
            protocol_version: PROTOCOL_VERSION,
            flags,
            client_instance_id: client_instance_id(),
            origin: self
                .send_origin
                .then(|| std::env::var("ZZ_PANE").ok())
                .flatten()
                .and_then(|pane| pane.parse().ok()),
            working_directory: client_working_directory(self.facts, logical_current_dir),
            tty: self
                .facts
                .tty_scope()
                .and_then(caller_tty)
                .filter(|tty| tty.len() <= zz_protocol::MAX_EXEC_TTY_BYTES),
            size: self
                .facts
                .includes_terminal_size()
                .then(caller_terminal_size)
                .flatten()
                .map(|(cols, rows, _, _)| (cols, rows)),
            features: exec_feature_mask(),
            startup_reentry: std::env::var(crate::STARTUP_REENTRY_ENVIRONMENT_VARIABLE)
                .ok()
                .and_then(|token| token.parse().ok()),
            spawned_server_id,
            expect_server_id,
            process_id: std::process::id(),
            environment: exec_environment(),
            commands,
            raw_control_line: None,
        }
    }

    fn connect_legacy(&self) -> Result<(CommandLink, bool), DaemonError> {
        let stream = LocalTransport::connect(&self.socket)?;
        let (reader, writer, hello) = connect_stream(
            stream,
            &self.display,
            ClientKind::Command,
            None,
            None,
            false,
            self.send_origin,
            self.facts,
        )?;
        let speaks_exec = hello
            .capabilities
            .iter()
            .any(|capability| capability == EXEC_CAPABILITY);
        Ok((
            CommandLink::Legacy {
                reader,
                writer,
                server_id: hello.server_id,
            },
            speaks_exec,
        ))
    }

    fn connect(&self) -> Result<CommandLink, DaemonError> {
        let stream = LocalTransport::connect(&self.socket)?;
        Ok(CommandLink::Exec {
            reader: ProtocolReceiver::new(stream.try_clone()?),
            writer: ProtocolSender::new(stream),
            answered: false,
            spent: false,
        })
    }
}

impl CommandClient {
    pub fn into_interactive(
        self,
        attach: zz_protocol::AttachOperation,
    ) -> Result<InteractiveClient, DaemonError> {
        let stream = match self.link {
            CommandLink::Exec { reader, writer, .. } => {
                drop(reader);
                writer.stream
            }
            CommandLink::Legacy { reader, writer, .. } => {
                drop(reader);
                drop(writer);
                LocalTransport::connect(&self.route.socket)?
            }
        };
        let connected = connect_stream_hello(
            ClientStream::Local(stream),
            &self.route.display,
            ClientKind::Interactive,
            short_device_name(),
            None,
            std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            false,
            false,
            &[
                crate::CLIENT_EXITS_ON_DETACH_CAPABILITY,
                zz_protocol::TTY_INPUT_CAPABILITY,
            ],
            self.route.facts,
            Some(attach),
        )?;
        let client = InteractiveClient::from_connected(connected);
        #[cfg(all(any(unix, windows), not(target_os = "ios")))]
        let client = {
            let mut client = client;
            client.ssh_forward = self._ssh_forward;
            client
        };
        Ok(client)
    }

    pub fn connect(path: &Path) -> Result<Self, DaemonError> {
        Self::connect_route(CommandRoute {
            socket: path.to_path_buf(),
            display: path.display().to_string(),
            facts: EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
            send_origin: true,
        })
    }

    fn connect_route(route: CommandRoute) -> Result<Self, DaemonError> {
        let link = route.connect()?;
        let server_id = match &link {
            CommandLink::Legacy { server_id, .. } => Some(*server_id),
            CommandLink::Exec { .. } => None,
        };
        Ok(Self {
            stdin_enabled: false,
            stdin_spent: false,
            stderr_handler: None,
            stdout_handler: None,
            link,
            route,
            server_id,
            #[cfg(all(any(unix, windows), not(target_os = "ios")))]
            _ssh_forward: None,
        })
    }

    /// Connect a short-lived command client to a configured fleet endpoint.
    pub fn connect_endpoint(endpoint: &Endpoint) -> Result<Self, DaemonError> {
        match endpoint {
            Endpoint::Local(path) => Self::connect_route(CommandRoute {
                socket: path.clone(),
                display: path.display().to_string(),
                facts: EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
                send_origin: false,
            }),
            Endpoint::Ssh(endpoint) => {
                #[cfg(target_os = "ios")]
                {
                    let _ = endpoint;
                    Err(crate::EndpointError::UnsupportedPlatform.into())
                }
                #[cfg(all(any(unix, windows), not(target_os = "ios")))]
                {
                    let ssh_forward = SshForward::start(endpoint, None)?;
                    let mut client = Self::connect_route(CommandRoute {
                        socket: ssh_forward.local_socket().to_path_buf(),
                        display: endpoint.to_string(),
                        facts: EndpointFactsScope::PortableTerminalSize,
                        send_origin: false,
                    })?;
                    client._ssh_forward = Some(ssh_forward);
                    Ok(client)
                }
                #[cfg(not(any(unix, windows)))]
                {
                    let _ = endpoint;
                    Err(crate::EndpointError::UnsupportedPlatform.into())
                }
            }
        }
    }

    pub fn server_id(&mut self) -> Result<u64, DaemonError> {
        if let Some(server_id) = self.server_id {
            return Ok(server_id);
        }
        self.exec_chain(ExecChain::new(Vec::new()), |_| 0)?;
        self.server_id.ok_or_else(|| {
            DaemonError::Server(ServerError::Internal(
                "daemon did not report its identity".to_owned(),
            ))
        })
    }

    pub fn set_stderr_handler(&mut self, handler: fn(&str)) {
        self.stderr_handler = Some(handler);
    }

    pub fn set_stdout_handler(&mut self, handler: fn(&RawText)) {
        self.stdout_handler = Some(handler);
    }

    pub fn enable_stdin(&mut self) {
        self.stdin_enabled = true;
    }

    /// Run one command and keep only its stdout, folding a nonzero exit into
    /// `DaemonError::CommandExit`. Callers that need the command's stderr or
    /// that must treat a nonzero exit as a completed command use
    /// [`CommandClient::execute_streams`].
    pub fn execute(&mut self, command: CommandInvocation) -> Result<String, DaemonError> {
        stdout_or_exit(self.execute_streams(command)?)
    }

    pub fn execute_on_server(
        &mut self,
        server_id: u64,
        command: CommandInvocation,
    ) -> Result<Option<String>, DaemonError> {
        let mut chain = ExecChain::new(vec![command]);
        chain.expect_server_id = Some(server_id);
        self.run_single(chain)?.map(stdout_or_exit).transpose()
    }

    /// Run one command and keep all three of its streams. A command that ran to
    /// completion is `Ok` whatever its exit status; `Err` stays reserved for
    /// dispatch, transport, and server failures.
    pub fn execute_streams(
        &mut self,
        command: CommandInvocation,
    ) -> Result<CommandOutcome, DaemonError> {
        if matches!(self.link, CommandLink::Legacy { .. }) {
            return self.execute_streams_with_prepared(command, false);
        }
        self.run_single(ExecChain::new(vec![command]))?
            .ok_or_else(|| {
                DaemonError::Server(ServerError::Internal("daemon ran no command".to_owned()))
            })
    }

    pub fn execute_prepared_streams(
        &mut self,
        command: CommandInvocation,
    ) -> Result<CommandOutcome, DaemonError> {
        if matches!(self.link, CommandLink::Legacy { .. }) {
            return self.execute_streams_with_prepared(command, true);
        }
        let mut chain = ExecChain::new(vec![command]);
        chain.prepared = true;
        self.run_single(chain)?.ok_or_else(|| {
            DaemonError::Server(ServerError::Internal("daemon ran no command".to_owned()))
        })
    }

    fn run_single(&mut self, chain: ExecChain<'_>) -> Result<Option<CommandOutcome>, DaemonError> {
        let mut outcome = None;
        let end = self.exec_chain(chain, |ran| {
            outcome = Some(ran.clone());
            0
        })?;
        match end {
            ExecChainEnd::Rejected(error) => Err(DaemonError::Server(error)),
            ExecChainEnd::Failed(error) => Err(error),
            ExecChainEnd::ServerMismatch => Ok(None),
            ExecChainEnd::Ran { .. } | ExecChainEnd::Resume(_) => {
                outcome.map(Some).ok_or_else(|| {
                    DaemonError::Server(ServerError::Internal("daemon ran no command".to_owned()))
                })
            }
        }
    }

    pub fn exec_chain(
        &mut self,
        chain: ExecChain<'_>,
        mut emit: impl FnMut(&CommandOutcome) -> u8,
    ) -> Result<ExecChainEnd, DaemonError> {
        if matches!(self.link, CommandLink::Legacy { .. }) {
            return self.legacy_exec_chain(chain, emit);
        }
        if matches!(self.link, CommandLink::Exec { spent: true, .. }) {
            self.link = self.route.connect()?;
        }
        #[cfg(all(unix, feature = "daemon"))]
        let _signal = self
            .stdin_enabled
            .then(StdinReadSignal::install)
            .transpose()?;
        let ExecChain {
            commands,
            spawned_server_id,
            expect_server_id,
            resume,
            prepared,
            last,
        } = chain;
        let mut flags = ExecFlags::default();
        flags.set(ExecFlags::STDIN_AVAILABLE, self.stdin_enabled);
        flags.set(ExecFlags::RESUME, resume.is_some());
        flags.set(ExecFlags::PREPARED, prepared);
        flags.set(ExecFlags::LAST, last);
        let request = ProtocolMessage::Exec(self.route.exec_request(
            commands,
            flags,
            spawned_server_id,
            expect_server_id,
        ));
        let CommandLink::Exec {
            reader,
            writer,
            answered,
            spent,
        } = &mut self.link
        else {
            unreachable!("legacy links returned above");
        };
        *spent = last;
        let sent = writer.send(&request);
        let first = match sent {
            Ok(()) => reader.recv(),
            Err(error) => Err(error),
        };
        let first = match first {
            Err(error) if !*answered && exec_maybe_unsupported(&error) => {
                let Ok((link, speaks_exec)) = self.route.connect_legacy() else {
                    return Ok(ExecChainEnd::Failed(error));
                };
                if speaks_exec {
                    return Ok(ExecChainEnd::Failed(error));
                }
                log::debug!(
                    target: "zz_daemon::diagnostics::client",
                    "daemon did not take Exec ({error}); retrying over the hello path",
                );
                let ProtocolMessage::Exec(request) = request else {
                    unreachable!("request is an Exec");
                };
                self.link = link;
                if let CommandLink::Legacy { server_id, .. } = &self.link {
                    self.server_id = Some(*server_id);
                }
                return self.legacy_exec_chain(
                    ExecChain {
                        commands: request.commands,
                        spawned_server_id,
                        expect_server_id,
                        resume,
                        prepared,
                        last,
                    },
                    emit,
                );
            }
            result => result?,
        };
        if let ProtocolMessage::CommandResponse(CommandResponse::Error {
            request_id: 0,
            error,
            ..
        }) = first
        {
            return Err(DaemonError::Server(error));
        }
        *answered = true;
        let mut message = Some(first);
        let mut exit_code = 0u8;
        let mut pending_error = None;
        let mut streamed_stderr = String::new();
        let mut client_exit = false;
        loop {
            let current = match message.take() {
                Some(current) => current,
                None => match reader.recv() {
                    Ok(current) => current,
                    Err(error) => return Ok(ExecChainEnd::Failed(pending_error.unwrap_or(error))),
                },
            };
            match current {
                ProtocolMessage::ExecExit(finished) => {
                    self.server_id = Some(finished.server_id);
                    if let Some(error) = pending_error {
                        return Ok(ExecChainEnd::Failed(error));
                    }
                    return Ok(match finished.outcome {
                        ExecOutcome::Ran => ExecChainEnd::Ran { exit_code },
                        ExecOutcome::Resume(resume) => ExecChainEnd::Resume(resume),
                        ExecOutcome::Rejected(error) => ExecChainEnd::Rejected(error),
                        ExecOutcome::ServerMismatch => ExecChainEnd::ServerMismatch,
                    });
                }
                ProtocolMessage::CommandResponse(CommandResponse::Error {
                    error, output, ..
                }) => {
                    pending_error = Some(command_failure(error, output));
                }
                ProtocolMessage::CommandResponse(CommandResponse::Success {
                    output,
                    exit_code: status,
                    stderr,
                    stdout_claim,
                    ..
                }) => {
                    let outcome = CommandOutcome {
                        stdout: output,
                        stderr: stderr
                            .strip_prefix(&streamed_stderr)
                            .unwrap_or(&stderr)
                            .to_owned(),
                        exit_code: status,
                        stdout_claim,
                        client_exit: std::mem::take(&mut client_exit),
                    };
                    streamed_stderr.clear();
                    let output_status = emit(&outcome);
                    if outcome.exit_code != 0 {
                        exit_code = outcome.exit_code;
                    }
                    if output_status != 0 {
                        exit_code = output_status;
                    }
                }
                ProtocolMessage::Event(zz_protocol::Event {
                    payload:
                        zz_protocol::EventPayload::ClientMessage {
                            kind: zz_protocol::ClientMessageKind::Error,
                            text,
                            ..
                        },
                    ..
                }) if self.stderr_handler.is_some() => {
                    let line = format!("{text}\n");
                    self.stderr_handler.expect("stderr handler checked")(&line);
                    streamed_stderr.push_str(&line);
                }
                ProtocolMessage::Event(zz_protocol::Event {
                    payload: zz_protocol::EventPayload::CommandStdout { output },
                    ..
                }) => {
                    if let Some(handler) = self.stdout_handler {
                        handler(&output);
                    }
                }
                ProtocolMessage::Event(zz_protocol::Event {
                    payload: zz_protocol::EventPayload::CommandClientExit,
                    ..
                }) => {
                    client_exit = true;
                }
                ProtocolMessage::ClientFileRequest(request) => {
                    let response =
                        answer_command_file(&request, self.stdin_enabled, &mut self.stdin_spent);
                    if let Err(error) = writer.send(&ProtocolMessage::ClientFileResponse(response))
                    {
                        return Ok(ExecChainEnd::Failed(error));
                    }
                }
                _ => {}
            }
        }
    }

    fn legacy_exec_chain(
        &mut self,
        chain: ExecChain<'_>,
        mut emit: impl FnMut(&CommandOutcome) -> u8,
    ) -> Result<ExecChainEnd, DaemonError> {
        let CommandLink::Legacy { server_id, .. } = self.link else {
            unreachable!("legacy chains run on a hello link");
        };
        if chain
            .expect_server_id
            .is_some_and(|expected| expected != server_id)
        {
            return Ok(ExecChainEnd::ServerMismatch);
        }
        if chain.commands.is_empty() {
            return Ok(ExecChainEnd::Ran { exit_code: 0 });
        }
        if chain.prepared {
            let mut exit_code = 0;
            for command in chain.commands {
                let outcome = match self.execute_streams_with_prepared(command, true) {
                    Ok(outcome) => outcome,
                    Err(error) => return Ok(ExecChainEnd::Failed(error)),
                };
                let output_status = emit(&outcome);
                if outcome.exit_code != 0 {
                    exit_code = outcome.exit_code;
                }
                if output_status != 0 {
                    exit_code = output_status;
                }
                if outcome.client_exit {
                    break;
                }
            }
            return Ok(ExecChainEnd::Ran { exit_code });
        }
        let typed = chain.resume.is_some().then(|| chain.commands.clone());
        let prepared = match chain.spawned_server_id {
            Some(spawned) => self.prepare_commands_or_stop_empty(chain.commands, spawned)?,
            None => self.prepare_commands(chain.commands)?,
        };
        if let Some(error) = prepared.iter().find_map(|command| match &command.result {
            PreparedCommandResult::Ready => None,
            PreparedCommandResult::Error(error) => Some(error.clone()),
        }) {
            return Ok(ExecChainEnd::Rejected(error));
        }
        if let (Some(classify), Some(typed)) = (chain.resume, typed)
            && let Some(kind) = classify(&typed, &prepared)
        {
            return Ok(ExecChainEnd::Resume(ExecResume {
                kind,
                commands: prepared,
            }));
        }
        let mut exit_code = 0;
        for command in prepared {
            let outcome = match self.execute_streams_with_prepared(command.invocation, true) {
                Ok(outcome) => outcome,
                Err(error) => return Ok(ExecChainEnd::Failed(error)),
            };
            let output_status = emit(&outcome);
            if outcome.exit_code != 0 {
                exit_code = outcome.exit_code;
            }
            if output_status != 0 {
                exit_code = output_status;
            }
            if outcome.client_exit {
                break;
            }
        }
        Ok(ExecChainEnd::Ran { exit_code })
    }

    fn legacy_link(
        &mut self,
    ) -> (
        &mut ProtocolReceiver<LocalStream>,
        &mut ProtocolSender<LocalStream>,
    ) {
        match &mut self.link {
            CommandLink::Legacy { reader, writer, .. }
            | CommandLink::Exec { reader, writer, .. } => (reader, writer),
        }
    }

    fn execute_streams_with_prepared(
        &mut self,
        command: CommandInvocation,
        prepared: bool,
    ) -> Result<CommandOutcome, DaemonError> {
        #[cfg(all(unix, feature = "daemon"))]
        let _signal = self
            .stdin_enabled
            .then(StdinReadSignal::install)
            .transpose()?;
        let mut command = command;
        command.set_stdin_available(self.stdin_enabled);
        let mut streamed_stderr = String::new();
        let mut client_exit = false;
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let stdin_enabled = self.stdin_enabled;
        let stderr_handler = self.stderr_handler;
        let stdout_handler = self.stdout_handler;
        let mut stdin_spent = self.stdin_spent;
        let (reader, writer) = self.legacy_link();
        let result = (|| {
            writer.send(&ProtocolMessage::CommandRequest(CommandRequest {
                request_id,
                command,
                prepared,
            }))?;
            loop {
                match reader.recv()? {
                    ProtocolMessage::CommandResponse(CommandResponse::Success {
                        request_id: response_id,
                        output,
                        exit_code,
                        stderr,
                        stdout_claim,
                    }) if response_id == request_id => {
                        return Ok(CommandOutcome {
                            stdout: output,
                            stderr: stderr
                                .strip_prefix(&streamed_stderr)
                                .unwrap_or(&stderr)
                                .to_owned(),
                            exit_code,
                            stdout_claim,
                            client_exit,
                        });
                    }
                    ProtocolMessage::CommandResponse(CommandResponse::Error {
                        request_id: response_id,
                        error,
                        output,
                    }) if response_id == request_id => {
                        return Err(command_failure(error, output));
                    }
                    ProtocolMessage::Event(zz_protocol::Event {
                        payload:
                            zz_protocol::EventPayload::ClientMessage {
                                kind: zz_protocol::ClientMessageKind::Error,
                                text,
                                ..
                            },
                        ..
                    }) if stderr_handler.is_some() => {
                        let line = format!("{text}\n");
                        stderr_handler.expect("stderr handler checked")(&line);
                        streamed_stderr.push_str(&line);
                    }
                    ProtocolMessage::Event(zz_protocol::Event {
                        payload: zz_protocol::EventPayload::CommandStdout { output },
                        ..
                    }) => {
                        if let Some(handler) = stdout_handler {
                            handler(&output);
                        }
                    }
                    ProtocolMessage::Event(zz_protocol::Event {
                        payload: zz_protocol::EventPayload::CommandClientExit,
                        ..
                    }) => {
                        client_exit = true;
                    }
                    ProtocolMessage::ClientFileRequest(request) => {
                        let response =
                            answer_command_file(&request, stdin_enabled, &mut stdin_spent);
                        writer.send(&ProtocolMessage::ClientFileResponse(response))?;
                    }
                    _ => {}
                }
            }
        })();
        self.stdin_spent = stdin_spent;
        result
    }

    fn prepare_commands(
        &mut self,
        commands: Vec<CommandInvocation>,
    ) -> Result<Vec<PreparedCommand>, DaemonError> {
        self.prepare_commands_with_count(commands, None)
    }

    fn prepare_commands_or_stop_empty(
        &mut self,
        mut commands: Vec<CommandInvocation>,
        server_id: u64,
    ) -> Result<Vec<PreparedCommand>, DaemonError> {
        let command_count = commands.len();
        commands.push(CommandInvocation::new(
            crate::COLD_START_PREPARE_ABORT_COMMAND,
            [server_id.to_string()],
        ));
        self.prepare_commands_with_count(commands, Some(command_count))
    }

    fn prepare_commands_with_count(
        &mut self,
        commands: Vec<CommandInvocation>,
        command_count: Option<usize>,
    ) -> Result<Vec<PreparedCommand>, DaemonError> {
        let command_count = command_count.unwrap_or(commands.len());
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let (reader, writer) = self.legacy_link();
        writer.send(&ProtocolMessage::PrepareCommandList {
            request_id,
            commands,
        })?;
        loop {
            if let ProtocolMessage::PreparedCommandList {
                request_id: response_id,
                commands,
            } = reader.recv()?
                && response_id == request_id
            {
                if commands.len() != command_count {
                    return Err(DaemonError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "daemon returned the wrong prepared-command count",
                    )));
                }
                return Ok(commands);
            }
        }
    }
}

fn answer_command_file(
    request: &ClientFileRequest,
    stdin_enabled: bool,
    stdin_spent: &mut bool,
) -> ClientFileResponse {
    let readable = stdin_enabled && !*stdin_spent;
    let result = match request.operation {
        ClientFileOperation::ReadStdin { binary } if readable => {
            *stdin_spent = true;
            read_command_stdin(binary)
        }
        ClientFileOperation::ReadStdinChunk if readable => {
            let chunk = read_command_stdin_chunk();
            *stdin_spent = !matches!(&chunk, Ok(chunk) if !chunk.is_empty());
            chunk
        }
        _ => return answer_client_file(request),
    };
    ClientFileResponse {
        request_id: request.request_id,
        data: result.as_ref().cloned().unwrap_or_default(),
        error: result.err(),
    }
}

pub struct InteractiveClient {
    reader: Mutex<ProtocolReceiver<ClientStream>>,
    writer: Arc<Mutex<ProtocolSender<ClientStream>>>,
    hello: ServerHello,
    #[cfg(all(any(unix, windows), not(target_os = "ios")))]
    ssh_forward: Option<SshForward>,
    #[cfg(target_os = "ios")]
    russh_forward: Option<RusshForward>,
}

impl InteractiveClient {
    pub fn connect_endpoint_with_attach(
        endpoint: &Endpoint,
        attach: zz_protocol::AttachOperation,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_attach(
            endpoint,
            None,
            None,
            &[
                crate::CLIENT_EXITS_ON_DETACH_CAPABILITY,
                zz_protocol::TTY_INPUT_CAPABILITY,
            ],
            Some(attach),
            true,
        )
    }

    pub fn connect_endpoint_with_prompts_and_attach(
        endpoint: &Endpoint,
        color_scheme: Option<TerminalColorScheme>,
        prompts: Option<crate::askpass::SshPrompts>,
        capabilities: &[&str],
        attach: Option<zz_protocol::AttachOperation>,
        terminal_surface: bool,
    ) -> Result<Self, DaemonError> {
        let client_has_terminal = !terminal_surface
            || (std::io::stdin().is_terminal() && std::io::stdout().is_terminal());
        match endpoint {
            Endpoint::Local(path) => {
                let stream = LocalTransport::connect(path)?;
                let connected = connect_stream_hello(
                    ClientStream::Local(stream),
                    path.display(),
                    ClientKind::Interactive,
                    short_device_name(),
                    color_scheme,
                    client_has_terminal,
                    false,
                    false,
                    capabilities,
                    if terminal_surface {
                        EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal
                    } else {
                        EndpointFactsScope::LocalHostWorkingDirectory
                    },
                    attach,
                )?;
                Ok(Self::from_connected(connected))
            }
            Endpoint::Ssh(endpoint) => {
                #[cfg(target_os = "ios")]
                {
                    let (forward, stream) = RusshForward::start(endpoint, prompts)?;
                    let connected = connect_stream_hello(
                        ClientStream::Ssh(stream),
                        endpoint,
                        ClientKind::Interactive,
                        short_device_name(),
                        color_scheme,
                        client_has_terminal,
                        false,
                        false,
                        capabilities,
                        if terminal_surface {
                            EndpointFactsScope::PortableTerminalSize
                        } else {
                            EndpointFactsScope::None
                        },
                        attach,
                    )?;
                    let mut client = Self::from_connected(connected);
                    client.russh_forward = Some(forward);
                    Ok(client)
                }
                #[cfg(all(any(unix, windows), not(target_os = "ios")))]
                {
                    let forward = SshForward::start(endpoint, prompts)?;
                    let stream = LocalTransport::connect(forward.local_socket())?;
                    let connected = connect_stream_hello(
                        ClientStream::Local(stream),
                        endpoint,
                        ClientKind::Interactive,
                        short_device_name(),
                        color_scheme,
                        client_has_terminal,
                        false,
                        false,
                        capabilities,
                        if terminal_surface {
                            EndpointFactsScope::PortableTerminalSize
                        } else {
                            EndpointFactsScope::None
                        },
                        attach,
                    )?;
                    Ok(Self::from_connected_with_ssh(connected, forward))
                }
                #[cfg(not(any(unix, windows)))]
                {
                    let _ = (endpoint, prompts, capabilities, attach, terminal_surface);
                    Err(crate::EndpointError::UnsupportedPlatform.into())
                }
            }
        }
    }

    pub fn take_pending_message(&self) -> Option<ProtocolMessage> {
        self.reader.lock().pending.pop_front()
    }

    pub fn is_initially_attached(&self) -> bool {
        self.reader
            .lock()
            .pending
            .iter()
            .any(|message| match message {
                ProtocolMessage::Batch(batch) => batch.messages().is_ok_and(|messages| {
                    messages.into_iter().any(|message| {
                        matches!(
                            message,
                            ProtocolMessage::Event(zz_protocol::Event {
                                payload: zz_protocol::EventPayload::ClientView(
                                    zz_protocol::ClientView {
                                        session: Some(_),
                                        ..
                                    }
                                ),
                                ..
                            }) | ProtocolMessage::Attached { .. }
                        )
                    })
                }),
                ProtocolMessage::Attached { .. } => true,
                _ => false,
            })
    }

    pub fn take_initial_responses(&self) -> Vec<CommandResponse> {
        let attached = self.is_initially_attached();
        let mut reader = self.reader.lock();
        let mut responses = Vec::new();
        for message in &mut reader.pending {
            if let ProtocolMessage::Batch(batch) = message {
                batch.frames.retain(|frame| {
                    if let Ok(ProtocolMessage::CommandResponse(response)) =
                        zz_protocol::decode_protocol_frame(frame)
                    {
                        if attached && matches!(response, CommandResponse::Error { .. }) {
                            return true;
                        }
                        responses.push(response);
                        false
                    } else {
                        true
                    }
                });
            }
        }
        responses
    }

    pub fn execute_chain(&self, commands: Vec<CommandInvocation>) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::Exec(ExecRequest {
            protocol_version: PROTOCOL_VERSION,
            client_instance_id: self.hello.client_instance_id,
            process_id: std::process::id(),
            origin: None,
            working_directory: None,
            tty: None,
            size: None,
            features: 0,
            environment: ClientEnvironmentBlob::default(),
            startup_reentry: None,
            spawned_server_id: None,
            expect_server_id: Some(self.hello.server_id),
            flags: ExecFlags::default(),
            commands,
            raw_control_line: None,
        }))
    }

    pub fn execute_control_line(&self, line: String) -> Result<(), DaemonError> {
        let request = ExecRequest {
            protocol_version: PROTOCOL_VERSION,
            client_instance_id: self.hello.client_instance_id,
            process_id: std::process::id(),
            origin: None,
            working_directory: None,
            tty: None,
            size: None,
            features: 0,
            environment: ClientEnvironmentBlob::default(),
            startup_reentry: None,
            spawned_server_id: None,
            expect_server_id: Some(self.hello.server_id),
            flags: ExecFlags::default(),
            commands: Vec::new(),
            raw_control_line: Some(line),
        };
        self.send(&ProtocolMessage::Exec(request))
    }

    pub fn request_tree_sync(&self) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::TreeSync)
    }

    pub fn connect(path: &Path) -> Result<Self, DaemonError> {
        Self::connect_with_color_scheme(path, TerminalColorScheme::Dark)
    }

    pub fn connect_control(path: &Path) -> Result<Self, DaemonError> {
        Self::connect_control_with_startup_owner(path, false)
    }

    pub fn connect_control_with_startup_owner(
        path: &Path,
        startup_config_owner: bool,
    ) -> Result<Self, DaemonError> {
        let stream = LocalTransport::connect(path)?;
        let connected = connect_stream_with_startup_owner(
            ClientStream::Local(stream),
            path.display(),
            ClientKind::Control,
            None,
            None,
            false,
            false,
            startup_config_owner,
            &[],
            EndpointFactsScope::LocalControlTerminalIdentity,
        )?;
        Ok(Self::from_connected(connected))
    }

    pub fn connect_with_color_scheme(
        path: &Path,
        color_scheme: TerminalColorScheme,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            &Endpoint::Local(path.to_owned()),
            Some(color_scheme),
            None,
            true,
            false,
            &[],
        )
    }

    pub fn connect_with_color_scheme_and_terminal(
        path: &Path,
        color_scheme: TerminalColorScheme,
        client_has_terminal: bool,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            &Endpoint::Local(path.to_owned()),
            Some(color_scheme),
            None,
            client_has_terminal,
            false,
            &[],
        )
    }

    pub fn connect_with_capabilities(
        path: &Path,
        color_scheme: TerminalColorScheme,
        client_has_terminal: bool,
        capabilities: &[&str],
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            &Endpoint::Local(path.to_owned()),
            Some(color_scheme),
            None,
            client_has_terminal,
            false,
            capabilities,
        )
    }

    #[cfg(unix)]
    pub fn connect_terminal_surface_with_timeout(
        path: &Path,
        color_scheme: TerminalColorScheme,
        timeout: std::time::Duration,
    ) -> Result<Self, DaemonError> {
        let stream = LocalTransport::connect(path)?;
        stream.set_timeout(Some(timeout))?;
        let control = stream.try_clone()?;
        let connected = connect_stream(
            ClientStream::Local(stream),
            path.display(),
            ClientKind::Interactive,
            short_device_name(),
            Some(color_scheme),
            true,
            false,
            EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
        )?;
        control.set_timeout(None)?;
        Ok(Self::from_connected(connected))
    }

    pub fn connect_terminal_surface(
        path: &Path,
        color_scheme: TerminalColorScheme,
        client_has_terminal: bool,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            &Endpoint::Local(path.to_owned()),
            Some(color_scheme),
            None,
            client_has_terminal,
            true,
            &[],
        )
    }

    pub fn connect_endpoint(
        endpoint: &Endpoint,
        color_scheme: TerminalColorScheme,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            endpoint,
            Some(color_scheme),
            None,
            true,
            true,
            &[],
        )
    }

    pub fn connect_endpoint_with_terminal(
        endpoint: &Endpoint,
        color_scheme: TerminalColorScheme,
        client_has_terminal: bool,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            endpoint,
            Some(color_scheme),
            None,
            client_has_terminal,
            true,
            &[],
        )
    }

    /// Connect, letting `prompts` answer whatever ssh asks along the way.
    ///
    /// Only a windowed caller passes them; a CLI invocation has ssh's own terminal.
    pub fn connect_endpoint_with_prompts(
        endpoint: &Endpoint,
        color_scheme: TerminalColorScheme,
        prompts: Option<crate::askpass::SshPrompts>,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            endpoint,
            Some(color_scheme),
            prompts,
            true,
            false,
            &[],
        )
    }

    pub fn connect_endpoint_with_prompts_and_capabilities(
        endpoint: &Endpoint,
        color_scheme: TerminalColorScheme,
        prompts: Option<crate::askpass::SshPrompts>,
        capabilities: &[&str],
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            endpoint,
            Some(color_scheme),
            prompts,
            true,
            false,
            capabilities,
        )
    }

    pub fn connect_terminal_surface_endpoint_with_prompts(
        endpoint: &Endpoint,
        color_scheme: TerminalColorScheme,
        prompts: Option<crate::askpass::SshPrompts>,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            endpoint,
            Some(color_scheme),
            prompts,
            true,
            true,
            &[],
        )
    }

    /// A client that runs inside somebody else's terminal has no theme of its
    /// own to report until that terminal answers an OSC 11 query. `c->theme`
    /// stays `THEME_UNKNOWN` for the pin until then, and
    /// `format_cb_client_theme` gives nothing for it.
    pub fn connect_endpoint_without_theme(
        endpoint: &Endpoint,
        client_has_terminal: bool,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            endpoint,
            None,
            None,
            client_has_terminal,
            true,
            &[],
        )
    }

    pub fn connect_terminal_surface_without_theme(
        path: &Path,
        client_has_terminal: bool,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_without_theme(&Endpoint::Local(path.to_owned()), client_has_terminal)
    }

    fn connect_endpoint_with_prompts_and_terminal(
        endpoint: &Endpoint,
        color_scheme: Option<TerminalColorScheme>,
        prompts: Option<crate::askpass::SshPrompts>,
        client_has_terminal: bool,
        terminal_surface: bool,
        extra_capabilities: &[&str],
    ) -> Result<Self, DaemonError> {
        let device_name = short_device_name();
        match endpoint {
            Endpoint::Local(path) => {
                let stream = LocalTransport::connect(path)?;
                let connected = connect_stream_with_startup_owner(
                    ClientStream::Local(stream),
                    path.display(),
                    ClientKind::Interactive,
                    device_name.clone(),
                    color_scheme,
                    client_has_terminal,
                    false,
                    false,
                    extra_capabilities,
                    if terminal_surface {
                        EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal
                    } else {
                        EndpointFactsScope::LocalHostWorkingDirectory
                    },
                )?;
                Ok(Self::from_connected(connected))
            }
            Endpoint::Ssh(endpoint) => {
                #[cfg(target_os = "ios")]
                {
                    let (russh_forward, stream) = RusshForward::start(endpoint, prompts)?;
                    let connected = connect_stream_with_startup_owner(
                        ClientStream::Ssh(stream),
                        endpoint,
                        ClientKind::Interactive,
                        device_name,
                        color_scheme,
                        client_has_terminal,
                        false,
                        false,
                        extra_capabilities,
                        if terminal_surface {
                            EndpointFactsScope::PortableTerminalSize
                        } else {
                            EndpointFactsScope::None
                        },
                    )?;
                    let mut client = Self::from_connected(connected);
                    client.russh_forward = Some(russh_forward);
                    Ok(client)
                }
                #[cfg(all(any(unix, windows), not(target_os = "ios")))]
                {
                    let ssh_forward = SshForward::start(endpoint, prompts)?;
                    let stream = LocalTransport::connect(ssh_forward.local_socket())?;
                    let connected = connect_stream_with_startup_owner(
                        ClientStream::Local(stream),
                        endpoint,
                        ClientKind::Interactive,
                        device_name,
                        color_scheme,
                        client_has_terminal,
                        false,
                        false,
                        extra_capabilities,
                        if terminal_surface {
                            EndpointFactsScope::PortableTerminalSize
                        } else {
                            EndpointFactsScope::None
                        },
                    )?;
                    Ok(Self::from_connected_with_ssh(connected, ssh_forward))
                }
                #[cfg(not(any(unix, windows)))]
                {
                    let _ = (endpoint, prompts);
                    Err(crate::EndpointError::UnsupportedPlatform.into())
                }
            }
        }
    }

    fn from_connected((reader, writer, hello): Connected<ClientStream>) -> Self {
        let writer = Arc::new(Mutex::new(writer));
        register_interactive_writer(&writer);
        Self {
            reader: Mutex::new(reader),
            writer,
            hello,
            #[cfg(all(any(unix, windows), not(target_os = "ios")))]
            ssh_forward: None,
            #[cfg(target_os = "ios")]
            russh_forward: None,
        }
    }

    #[cfg(all(any(unix, windows), not(target_os = "ios")))]
    fn from_connected_with_ssh(
        connected: Connected<ClientStream>,
        ssh_forward: SshForward,
    ) -> Self {
        let mut client = Self::from_connected(connected);
        client.ssh_forward = Some(ssh_forward);
        client
    }

    #[must_use]
    pub fn server_hello(&self) -> &ServerHello {
        &self.hello
    }

    /// The loopback SOCKS5 port this client's ssh forward opened, if it has one.
    #[must_use]
    pub fn socks_port(&self) -> Option<u16> {
        #[cfg(target_os = "ios")]
        {
            self.russh_forward
                .as_ref()
                .and_then(RusshForward::socks_port)
        }
        #[cfg(all(any(unix, windows), not(target_os = "ios")))]
        {
            self.ssh_forward.as_ref().and_then(SshForward::socks_port)
        }
        #[cfg(not(any(unix, windows)))]
        {
            None
        }
    }

    pub fn forward_loopback(&self, port: u16) -> io::Result<()> {
        if port == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "A nonzero localhost port is required.",
            ));
        }
        #[cfg(target_os = "ios")]
        if let Some(forward) = &self.russh_forward {
            return forward.forward_loopback(port);
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Localhost forwarding requires an embedded SSH connection.",
        ))
    }

    pub fn attach(&self, session: impl Into<String>) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::Attach {
            session: session.into(),
        })
    }

    pub fn attach_default_in(&self, working_directory: &Path) -> Result<(), DaemonError> {
        if working_directory.is_absolute() && working_directory.is_dir() {
            self.send(&ProtocolMessage::CommandRequest(CommandRequest {
                request_id: 0,
                command: CommandInvocation::new(
                    "new-session",
                    [
                        "-A".to_owned(),
                        "-d".to_owned(),
                        "-c".to_owned(),
                        working_directory.to_string_lossy().into_owned(),
                    ],
                ),
                prepared: false,
            }))?;
        }
        self.attach("")
    }

    pub fn attach_session(
        &self,
        session: impl Into<String>,
        detach_others: bool,
        read_only: bool,
        client_flags: Option<&str>,
    ) -> Result<(), DaemonError> {
        let session = session.into();
        if !detach_others && !read_only && client_flags.is_none() {
            return self.attach(session);
        }
        self.send(&ProtocolMessage::CommandRequest(CommandRequest {
            request_id: 0,
            command: attach_session_command(session, detach_others, read_only, client_flags),
            prepared: false,
        }))
    }

    pub fn request_attach_session(
        &self,
        session: impl Into<String>,
        detach_others: bool,
        read_only: bool,
        client_flags: Option<&str>,
    ) -> Result<u64, DaemonError> {
        self.execute(attach_session_command(
            session.into(),
            detach_others,
            read_only,
            client_flags,
        ))
    }

    pub fn detach(&self) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::Detach)
    }

    pub fn send_input(&self, input: InputMessage) -> Result<(), DaemonError> {
        if let InputMessage::Key {
            pane,
            input: key,
            text_follows,
        } = &input
        {
            return self.send_locked(&input, |writer| {
                writer.send_encoded(&input, |frame| {
                    encode_key_input_into(*pane, key, *text_follows, frame)
                })
            });
        }
        self.send(&ProtocolMessage::Input(input))
    }

    /// Stream one pasted image to the daemon, which writes it on its own host
    /// and pastes the resulting path into `pane`.
    pub fn send_paste_upload(
        &self,
        upload_id: u64,
        pane: PaneId,
        extension: String,
        bytes: &[u8],
    ) -> Result<(), DaemonError> {
        self.send_paste_upload_with_purpose(
            upload_id,
            pane,
            PasteUploadPurpose::PastePath,
            extension,
            bytes,
        )
    }

    /// Stream one pasted image into the daemon-owned placeholder binding store.
    pub fn record_pasted_image(
        &self,
        upload_id: u64,
        pane: PaneId,
        extension: String,
        bytes: &[u8],
    ) -> Result<(), DaemonError> {
        self.send_paste_upload_with_purpose(
            upload_id,
            pane,
            PasteUploadPurpose::RecordPastedImage,
            extension,
            bytes,
        )
    }

    fn send_paste_upload_with_purpose(
        &self,
        upload_id: u64,
        pane: PaneId,
        purpose: PasteUploadPurpose,
        extension: String,
        bytes: &[u8],
    ) -> Result<(), DaemonError> {
        let total_bytes = u32::try_from(bytes.len()).map_err(|_| {
            DaemonError::Server(ServerError::InvalidCommand(
                "pasted image exceeds the upload limit".to_owned(),
            ))
        })?;
        let mut writer = self.writer.lock();
        writer.send(&ProtocolMessage::PasteUploadBegin {
            upload_id,
            pane,
            purpose,
            extension,
            total_bytes,
        })?;
        for chunk in bytes.chunks(MAX_PASTE_UPLOAD_CHUNK_BYTES) {
            writer.send(&ProtocolMessage::PasteUploadChunk {
                upload_id,
                bytes: chunk.to_vec(),
            })?;
        }
        Ok(())
    }

    pub fn fetch_pasted_image(&self, pane: PaneId, number: u32) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::FetchPastedImage { pane, number })
    }

    pub fn execute(&self, command: CommandInvocation) -> Result<u64, DaemonError> {
        self.execute_with_prepared(command, false)
    }

    pub fn execute_prepared(&self, command: CommandInvocation) -> Result<u64, DaemonError> {
        self.execute_with_prepared(command, true)
    }

    fn execute_with_prepared(
        &self,
        command: CommandInvocation,
        prepared: bool,
    ) -> Result<u64, DaemonError> {
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.send(&ProtocolMessage::CommandRequest(CommandRequest {
            request_id,
            command,
            prepared,
        }))?;
        Ok(request_id)
    }

    pub fn prepare_commands(&self, commands: Vec<CommandInvocation>) -> Result<u64, DaemonError> {
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.send(&ProtocolMessage::PrepareCommandList {
            request_id,
            commands,
        })?;
        Ok(request_id)
    }

    pub fn request_home_directories(&self, users: Vec<String>) -> Result<u64, DaemonError> {
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.send(&ProtocolMessage::HomeDirectoryRequest { request_id, users })?;
        Ok(request_id)
    }

    pub fn request_path_list(&self, pane: PaneId, dir: Option<String>) -> Result<u64, DaemonError> {
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.send(&ProtocolMessage::PathListRequest {
            request_id,
            pane,
            dir,
        })?;
        Ok(request_id)
    }

    pub fn cancel_path_list(&self, request_id: u64) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::PathListCancel { request_id })
    }

    pub fn request_environment(&self, names: Vec<String>) -> Result<u64, DaemonError> {
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.send(&ProtocolMessage::EnvironmentRequest { request_id, names })?;
        Ok(request_id)
    }

    pub fn request_resync(&self) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::Resync)
    }

    pub fn request_full(&self, pane: PaneId) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::RequestFull { pane })
    }

    pub fn set_terminal_preview(&self, enabled: bool) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::SetTerminalPreview { enabled })
    }

    pub fn request_history(&self, pane: PaneId, start: u32, count: u32) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::HistoryRequest { pane, start, count })
    }

    pub fn client_instance_id(&self) -> ClientInstanceId {
        client_instance_id()
    }

    /// Submit a prompt to the pane's daemon-owned agent. Images cross as
    /// bytes plus MIME format; the daemon turns them into ACP content blocks.
    pub fn agent_prompt(
        &self,
        pane: PaneId,
        text: String,
        images: Vec<AgentImage>,
    ) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentPrompt { pane, text, images })
    }

    pub fn agent_cancel(&self, pane: PaneId) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentCancel { pane })
    }

    /// Reclaim the pane's queued prompts; they come back inside the stream.
    pub fn agent_unqueue(&self, pane: PaneId) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentUnqueue { pane })
    }

    /// Answer a parked permission request; `None` cancels it.
    pub fn agent_respond_permission(
        &self,
        pane: PaneId,
        request_id: u64,
        option_id: Option<String>,
    ) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentRespondPermission {
            pane,
            request_id,
            option_id,
        })
    }

    pub fn agent_set_config_option(
        &self,
        pane: PaneId,
        option_id: String,
        value: String,
    ) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentSetConfigOption {
            pane,
            option_id,
            value,
        })
    }

    pub fn agent_set_mode(&self, pane: PaneId, mode_id: String) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentSetMode { pane, mode_id })
    }

    pub fn agent_authenticate(&self, pane: PaneId, method_id: String) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentAuthenticate { pane, method_id })
    }

    pub fn agent_session_op(
        &self,
        pane: PaneId,
        op: AgentSessionOpKind,
    ) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentSessionOp { pane, op })
    }

    /// Replay the pane's agent stream from `from_seq`, then tail it.
    pub fn agent_replay(&self, pane: PaneId, from_seq: u64) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentReplay { pane, from_seq })
    }

    pub fn agent_acknowledge_prompt_restore(
        &self,
        pane: PaneId,
        reclaim_id: u64,
    ) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::AgentAcknowledgePromptRestore { pane, reclaim_id })
    }

    /// Answer one daemon-issued request for GUI-owned work, success or failure.
    pub fn send_gui_response(&self, response: GuiResponse) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::GuiResponse(response))
    }

    pub fn set_color_scheme(&self, color_scheme: TerminalColorScheme) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::SetColorScheme(color_scheme))
    }

    pub fn set_config_overrides(
        &self,
        entries: Vec<ConfigOverrideEntry>,
    ) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::SetConfigOverrides { entries })
    }

    #[cfg(unix)]
    pub fn shutdown(&self) -> Result<(), DaemonError> {
        #[cfg(target_os = "ios")]
        if let Some(forward) = &self.russh_forward {
            forward.shutdown();
            return Ok(());
        }
        self.writer.lock().stream.shutdown()?;
        Ok(())
    }

    #[cfg(unix)]
    pub fn receive_fd(&self) -> io::Result<std::os::fd::OwnedFd> {
        self.reader.lock().stream.get_ref().receive_fd()
    }

    #[cfg(unix)]
    pub fn send_with_fd(
        &self,
        message: &ProtocolMessage,
        fd: std::os::fd::BorrowedFd<'_>,
    ) -> io::Result<()> {
        let frame = zz_protocol::encode_protocol_message(message).map_err(io::Error::other)?;
        let writer = self.writer.lock();
        let socket = writer.stream.receive_fd()?;
        let fds = [fd];
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut space);
        ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&fds));
        let mut sent = loop {
            match rustix::net::sendmsg(
                &socket,
                &[io::IoSlice::new(&frame)],
                &mut ancillary,
                rustix::net::SendFlags::empty(),
            ) {
                Ok(sent) => break sent,
                Err(rustix::io::Errno::INTR) => {}
                Err(error) => return Err(error.into()),
            }
        };
        while sent < frame.len() {
            match rustix::io::write(&socket, &frame[sent..]) {
                Ok(written) => sent += written,
                Err(rustix::io::Errno::INTR) => {}
                Err(error) => return Err(error.into()),
            }
        }
        drop(writer);
        Ok(())
    }

    #[cfg(unix)]
    pub fn try_recv(&self) -> Result<Option<Box<ProtocolMessage>>, DaemonError> {
        self.try_recv_with_read(true)
    }

    #[cfg(unix)]
    pub fn try_recv_buffered(&self) -> Result<Option<Box<ProtocolMessage>>, DaemonError> {
        self.try_recv_with_read(false)
    }

    #[cfg(unix)]
    fn try_recv_with_read(
        &self,
        read_socket: bool,
    ) -> Result<Option<Box<ProtocolMessage>>, DaemonError> {
        let result = self.reader.lock().try_recv_decodable(read_socket)?;
        if let Some((message, skipped)) = result {
            if skipped {
                self.request_resync()?;
            }
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    pub fn recv(&self) -> Result<ProtocolMessage, DaemonError> {
        let started = diagnostic_timer();
        let lock_started = diagnostic_timer();
        let mut reader = self.reader.lock();
        let lock_wait_us = diagnostic_elapsed_us(lock_started);
        let result = reader.recv_decodable();
        drop(reader);
        log::trace!(
            target: "zz_daemon::diagnostics::client",
            "interactive_recv success={} lock_wait_us={} total_elapsed_us={}",
            result.is_ok(),
            lock_wait_us,
            diagnostic_elapsed_us(started),
        );
        match result {
            Ok((message, false)) => Ok(message),
            Ok((message, true)) => {
                self.request_resync()?;
                Ok(message)
            }
            Err(error) => Err(error),
        }
    }

    pub fn send(&self, message: &ProtocolMessage) -> Result<(), DaemonError> {
        self.send_locked(&TracedMessage(message), |writer| writer.send(message))
    }

    fn send_locked(
        &self,
        message: &impl std::fmt::Debug,
        send: impl FnOnce(&mut ProtocolSender<ClientStream>) -> Result<(), DaemonError>,
    ) -> Result<(), DaemonError> {
        let started = diagnostic_timer();
        let lock_started = diagnostic_timer();
        let mut writer = self.writer.lock();
        let lock_wait_us = diagnostic_elapsed_us(lock_started);
        let result = send(&mut writer);
        log::trace!(
            target: "zz_daemon::diagnostics::client",
            "interactive_send success={} lock_wait_us={} total_elapsed_us={} message={message:#?}",
            result.is_ok(),
            lock_wait_us,
            diagnostic_elapsed_us(started),
        );
        result
    }
}

pub(crate) struct TracedMessage<'a>(pub(crate) &'a ProtocolMessage);

impl fmt::Debug for TracedMessage<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ProtocolMessage::AgentAnswerQuestion {
                pane,
                request_id,
                answers,
            } => formatter
                .debug_struct("AgentAnswerQuestion")
                .field("pane", pane)
                .field("request_id", request_id)
                .field(
                    "answers",
                    &answers
                        .iter()
                        .map(|answer| (&answer.id, answer.answers.len()))
                        .collect::<Vec<_>>(),
                )
                .finish(),
            message => message.fmt(formatter),
        }
    }
}

struct ProtocolSender<S> {
    stream: S,
    frame: Vec<u8>,
}

struct ProtocolReceiver<S> {
    stream: io::BufReader<S>,
    frame: Vec<u8>,
    pending: VecDeque<ProtocolMessage>,
    #[cfg(unix)]
    ready: Option<ReadyFrames>,
    #[cfg(unix)]
    skipped_decode: bool,
}

#[cfg(all(test, unix, feature = "daemon"))]
#[path = "daemon/ctrl_client_tests.rs"]
mod ctrl_client_tests;

#[cfg(unix)]
#[derive(Default)]
struct ReadyFrames {
    bytes: Vec<u8>,
    consumed: usize,
}

#[cfg(unix)]
impl ReadyFrames {
    fn message(&mut self) -> Result<Option<Box<ProtocolMessage>>, ProtocolError> {
        let remaining = &self.bytes[self.consumed..];
        let Some(prefix) = remaining.get(..4) else {
            return Ok(None);
        };
        let length = u32::from_le_bytes(prefix.try_into().expect("four-byte prefix")) as usize;
        if length > zz_protocol::MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(length));
        }
        if length < 4 {
            return Err(ProtocolError::Truncated);
        }
        let Some(frame) = remaining.get(..length + 4) else {
            return Ok(None);
        };
        let message = zz_protocol::decode_protocol_frame(frame);
        self.consumed += length + 4;
        message.map(|message| Some(Box::new(message)))
    }

    fn read(
        &mut self,
        scratch: &mut Vec<u8>,
        read: impl FnOnce(&mut [u8]) -> io::Result<usize>,
    ) -> io::Result<usize> {
        if scratch.len() < RECEIVE_BUFFER_BYTES {
            scratch.resize(RECEIVE_BUFFER_BYTES, 0);
        }
        let read = read(&mut scratch[..RECEIVE_BUFFER_BYTES])?;
        if read != 0 {
            if self.consumed != 0 {
                self.bytes.drain(..self.consumed);
                self.consumed = 0;
            }
            self.bytes.extend_from_slice(&scratch[..read]);
        }
        Ok(read)
    }
}

const RECEIVE_BUFFER_BYTES: usize = 64 * 1024;

type Connected<S> = (ProtocolReceiver<S>, ProtocolSender<S>, ServerHello);

impl<S: TransportStream> ProtocolReceiver<S> {
    fn new(stream: S) -> Self {
        Self {
            stream: io::BufReader::with_capacity(RECEIVE_BUFFER_BYTES, stream),
            frame: Vec::new(),
            pending: VecDeque::new(),
            #[cfg(unix)]
            ready: None,
            #[cfg(unix)]
            skipped_decode: false,
        }
    }

    #[cfg(unix)]
    fn try_recv_decodable(
        &mut self,
        read_socket: bool,
    ) -> Result<Option<(Box<ProtocolMessage>, bool)>, DaemonError> {
        loop {
            match self.try_recv(read_socket) {
                Err(DaemonError::Protocol(ProtocolError::Decode(error))) => {
                    self.skipped_decode = true;
                    log::warn!(
                        target: "zz_daemon::diagnostics::client",
                        "skipping an undecodable daemon message: {error}"
                    );
                }
                Ok(Some(message)) => {
                    return Ok(Some((message, std::mem::take(&mut self.skipped_decode))));
                }
                Ok(None) => return Ok(None),
                Err(error) => return Err(error),
            }
        }
    }

    #[cfg(unix)]
    fn try_recv(&mut self, read_socket: bool) -> Result<Option<Box<ProtocolMessage>>, DaemonError> {
        use std::io::BufRead as _;
        if let Some(message) = self.pending.pop_front() {
            return Ok(Some(Box::new(message)));
        }
        let ready = self.ready.get_or_insert_with(ReadyFrames::default);
        let buffered = self.stream.buffer();
        ready.bytes.extend_from_slice(buffered);
        let count = buffered.len();
        self.stream.consume(count);
        loop {
            if let Some(message) = ready.message()? {
                return Ok(Some(message));
            }
            if !read_socket {
                return Ok(None);
            }
            match ready.read(&mut self.frame, |buffer| {
                self.stream.get_ref().read_ready(buffer)
            }) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn recv_decodable(&mut self) -> Result<(ProtocolMessage, bool), DaemonError> {
        #[cfg(unix)]
        let mut skipped = std::mem::take(&mut self.skipped_decode);
        #[cfg(not(unix))]
        let mut skipped = false;
        loop {
            match self.recv() {
                Err(DaemonError::Protocol(ProtocolError::Decode(error))) => {
                    skipped = true;
                    log::warn!(
                        target: "zz_daemon::diagnostics::client",
                        "skipping an undecodable daemon message: {error}"
                    );
                }
                result => return result.map(|message| (message, skipped)),
            }
        }
    }

    fn recv(&mut self) -> Result<ProtocolMessage, DaemonError> {
        if let Some(message) = self.pending.pop_front() {
            return Ok(message);
        }
        #[cfg(unix)]
        if let Some(ready) = self.ready.as_mut() {
            loop {
                if let Some(message) = ready.message()? {
                    return Ok(*message);
                }
                if ready.read(&mut self.frame, |buffer| self.stream.read(buffer))? == 0 {
                    return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
                }
            }
        }
        let started = diagnostic_timer();
        let message = read_protocol_message_into(&mut self.stream, &mut self.frame)?;
        log::trace!(
            target: "zz_daemon::diagnostics::protocol",
            "recv bytes={} frame_capacity={} elapsed_us={} message={message:#?}",
            self.frame.len(),
            self.frame.capacity(),
            diagnostic_elapsed_us(started),
        );
        Ok(message)
    }
}

impl<S: TransportStream> ProtocolSender<S> {
    fn new(stream: S) -> Self {
        Self {
            stream,
            frame: Vec::new(),
        }
    }

    fn send(&mut self, message: &ProtocolMessage) -> Result<(), DaemonError> {
        self.send_encoded(message, |frame| {
            encode_protocol_message_into(message, frame)
        })
    }

    fn send_encoded(
        &mut self,
        message: &impl std::fmt::Debug,
        encode: impl FnOnce(&mut Vec<u8>) -> Result<(), ProtocolError>,
    ) -> Result<(), DaemonError> {
        let started = diagnostic_timer();
        let encode_started = diagnostic_timer();
        encode(&mut self.frame)?;
        let encode_us = diagnostic_elapsed_us(encode_started);
        let write_started = diagnostic_timer();
        self.stream.write_all(&self.frame)?;
        let write_us = diagnostic_elapsed_us(write_started);
        let flush_started = diagnostic_timer();
        self.stream.flush()?;
        let flush_us = diagnostic_elapsed_us(flush_started);
        log::trace!(
            target: "zz_daemon::diagnostics::protocol",
            "send bytes={} frame_capacity={} encode_us={} write_us={} flush_us={} elapsed_us={} message={message:#?}",
            self.frame.len(),
            self.frame.capacity(),
            encode_us,
            write_us,
            flush_us,
            diagnostic_elapsed_us(started),
        );
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EndpointFactsScope {
    None,
    LocalHostWorkingDirectory,
    LocalControlTerminalIdentity,
    PortableTerminalSize,
    LocalHostWorkingDirectoryAndTerminal,
}

impl EndpointFactsScope {
    fn includes_working_directory(self) -> bool {
        matches!(
            self,
            Self::LocalHostWorkingDirectory
                | Self::LocalControlTerminalIdentity
                | Self::LocalHostWorkingDirectoryAndTerminal
        )
    }

    fn includes_terminal_size(self) -> bool {
        matches!(
            self,
            Self::PortableTerminalSize | Self::LocalHostWorkingDirectoryAndTerminal
        )
    }

    fn includes_tty(self) -> bool {
        matches!(
            self,
            Self::LocalControlTerminalIdentity | Self::LocalHostWorkingDirectoryAndTerminal
        )
    }

    fn tty_scope(self) -> Option<CallerTtyScope> {
        match self {
            Self::LocalControlTerminalIdentity => Some(CallerTtyScope::StandardInput),
            Self::LocalHostWorkingDirectoryAndTerminal => Some(CallerTtyScope::StandardStreams),
            Self::None | Self::LocalHostWorkingDirectory | Self::PortableTerminalSize => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallerTtyScope {
    StandardInput,
    StandardStreams,
}

pub const DEFAULT_CELL_WIDTH_PX: u32 = 8;
pub const DEFAULT_CELL_HEIGHT_PX: u32 = 16;

#[must_use]
pub fn cell_pixel_extent(pixels: u16, cells: u16, fallback: u32) -> u32 {
    if pixels == 0 || cells == 0 {
        fallback
    } else {
        (u32::from(pixels) / u32::from(cells)).max(1)
    }
}

#[cfg(unix)]
fn caller_terminal_size() -> Option<(u16, u16, u32, u32)> {
    let size = rustix::termios::tcgetwinsize(std::io::stdout()).ok()?;
    (size.ws_col > 0 && size.ws_row > 0).then(|| {
        (
            size.ws_col,
            size.ws_row,
            cell_pixel_extent(size.ws_xpixel, size.ws_col, DEFAULT_CELL_WIDTH_PX),
            cell_pixel_extent(size.ws_ypixel, size.ws_row, DEFAULT_CELL_HEIGHT_PX),
        )
    })
}

#[cfg(not(unix))]
fn caller_terminal_size() -> Option<(u16, u16, u32, u32)> {
    None
}

#[cfg(unix)]
fn caller_tty(scope: CallerTtyScope) -> Option<String> {
    use std::os::fd::AsFd as _;

    let stdin = std::io::stdin();
    if scope == CallerTtyScope::StandardInput {
        return rustix::termios::ttyname(stdin.as_fd(), Vec::new())
            .ok()?
            .into_string()
            .ok();
    }
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    for fd in [stdin.as_fd(), stdout.as_fd(), stderr.as_fd()] {
        if let Ok(name) = rustix::termios::ttyname(fd, Vec::new()) {
            return name.into_string().ok();
        }
    }
    None
}

#[cfg(not(unix))]
fn caller_tty(_scope: CallerTtyScope) -> Option<String> {
    None
}

fn terminal_facts_capabilities(
    scope: EndpointFactsScope,
    nested: bool,
    capabilities: &mut Vec<String>,
) {
    terminal_facts_capabilities_with(
        scope,
        nested,
        caller_terminal_size,
        caller_tty,
        capabilities,
    );
}

fn startup_config_owner_capability(
    kind: ClientKind,
    startup_config_owner: bool,
    capabilities: &mut Vec<String>,
) {
    if kind == ClientKind::Control && startup_config_owner {
        capabilities.push(ClientHello::STARTUP_CONFIG_OWNER_CAPABILITY.to_owned());
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientTerminalFlags {
    pub utf8: bool,
    pub features: Vec<String>,
}

static CLIENT_TERMINAL_FLAGS: std::sync::OnceLock<ClientTerminalFlags> = std::sync::OnceLock::new();

pub fn set_client_terminal_flags(flags: ClientTerminalFlags) {
    let _ = CLIENT_TERMINAL_FLAGS.set(flags);
}

fn client_terminal_flags() -> &'static ClientTerminalFlags {
    CLIENT_TERMINAL_FLAGS.get_or_init(ClientTerminalFlags::default)
}

/// What `tty_check_fg` and `tty_check_bg` ask before they write a cell: how
/// many colours the terminal this client runs in takes, from its own `TERM`
/// and `COLORTERM` and the features `-2` and `-T` requested, folded with what
/// its terminal has answered since (`tty_update_features`, which raises the
/// pin's count the same way and never lowers it).
pub fn client_terminal_colour_count() -> u32 {
    let flags = client_terminal_flags();
    crate::terminal_features::terminal_colour_count(
        &std::env::var("TERM").unwrap_or_default(),
        &std::env::var("COLORTERM").unwrap_or_default(),
        crate::terminal_features::terminal_feature_mask(flags.features.iter().map(String::as_str))
            | LEARNED_TERMINAL_FEATURES.load(Ordering::Relaxed),
    )
}

/// Whether this client's terminal takes UTF-8, the `CLIENT_UTF8` flag
/// `tty_check_codeset` reads before it writes a cell: `-u` raises it and
/// otherwise the locale decides. A writer with no terminal behind it never
/// asks.
#[must_use]
pub fn client_takes_utf8_terminal() -> bool {
    client_terminal_flags().utf8 || client_takes_utf8(|name| std::env::var_os(name))
}

fn client_utf8_capability(capabilities: &mut Vec<String>) {
    if client_takes_utf8_terminal() {
        capabilities.push(ClientHello::CLIENT_UTF8_CAPABILITY.to_owned());
    }
}

/// What this client's terminal answered after the hello. `tty_update_features`
/// folds a reply straight into the pin's own `c->term_features` because the
/// client and the server share one process there; here the client learns it,
/// keeps it for the next hello and reports it over the connection it already
/// holds.
static LEARNED_TERMINAL_FEATURES: AtomicU32 = AtomicU32::new(0);

static INTERACTIVE_WRITER: Mutex<Option<Weak<Mutex<ProtocolSender<ClientStream>>>>> =
    Mutex::new(None);

fn register_interactive_writer(writer: &Arc<Mutex<ProtocolSender<ClientStream>>>) {
    *INTERACTIVE_WRITER.lock() = Some(Arc::downgrade(writer));
    report_learned_terminal_features(LEARNED_TERMINAL_FEATURES.load(Ordering::Relaxed));
}

/// Record one `tty_default_features` list, or one feature name a device
/// attributes reply named, and report the union to the daemon.
pub fn learn_client_terminal_features(features: &str) {
    let mask = crate::terminal_features::terminal_feature_mask([features]);
    if mask == 0 {
        return;
    }
    let learned = LEARNED_TERMINAL_FEATURES.fetch_or(mask, Ordering::Relaxed) | mask;
    report_learned_terminal_features(learned);
}

/// `c->term_features` as the client process itself knows it: what `-2` and
/// `-T` asked for, plus everything its terminal has answered with since.
#[must_use]
pub fn client_terminal_feature_mask() -> u32 {
    crate::terminal_features::terminal_feature_mask(
        client_terminal_flags().features.iter().map(String::as_str),
    ) | LEARNED_TERMINAL_FEATURES.load(Ordering::Relaxed)
}

fn report_learned_terminal_features(learned: u32) {
    if learned == 0 {
        return;
    }
    let Some(writer) = INTERACTIVE_WRITER.lock().as_ref().and_then(Weak::upgrade) else {
        return;
    };
    let features = crate::terminal_features::terminal_features_list(learned)
        .split(',')
        .map(str::to_owned)
        .collect();
    let _ = writer
        .lock()
        .send(&ProtocolMessage::ClientTerminalFeatures { features });
}

/// `tty_keys_extended_device_attributes` keeps the XTVERSION reply on the
/// client as `c->term_type` beside the features it implies, so the raw name
/// travels too and the daemon can answer `#{client_termtype}` with it.
pub fn report_terminal_type(term_type: &str) {
    if term_type.is_empty() || term_type.len() > zz_protocol::MAX_CLIENT_TERMINAL_TYPE_BYTES {
        return;
    }
    let Some(writer) = INTERACTIVE_WRITER.lock().as_ref().and_then(Weak::upgrade) else {
        return;
    };
    let _ = writer.lock().send(&ProtocolMessage::ClientTerminalType {
        term_type: term_type.to_owned(),
    });
}

const MAX_CLIENT_FEATURE_SPECS: usize = 16;
const MAX_CLIENT_FEATURE_SPEC_BYTES: usize = 200;

/// A reconnecting client already knows what its terminal answered, so the
/// learned set rides the hello as one more spec rather than waiting for the
/// terminal to answer the same questions again.
fn client_learned_features_capability(capabilities: &mut Vec<String>) {
    let learned = LEARNED_TERMINAL_FEATURES.load(Ordering::Relaxed);
    if learned == 0 {
        return;
    }
    capabilities.push(format!(
        "{}{}",
        ClientHello::CLIENT_FEATURES_CAPABILITY_PREFIX,
        crate::terminal_features::terminal_features_list(learned)
    ));
}

fn client_features_capabilities(features: &[String], capabilities: &mut Vec<String>) {
    capabilities.extend(
        features
            .iter()
            .filter(|spec| spec.len() <= MAX_CLIENT_FEATURE_SPEC_BYTES)
            .take(MAX_CLIENT_FEATURE_SPECS)
            .map(|spec| format!("{}{spec}", ClientHello::CLIENT_FEATURES_CAPABILITY_PREFIX)),
    );
}

/// tmux.c decides this in the client process, before it ever dials the server:
/// `-u` raises it outright, `$TMUX` being set at all means the terminal is
/// tmux's own and takes UTF-8, and otherwise the first of `LC_ALL`, `LC_CTYPE`
/// and `LANG` that is set and non-empty decides it, by holding `UTF-8` or
/// `UTF8` in any case. This reads the last two; `-u` arrives through
/// `set_client_terminal_flags`.
fn client_takes_utf8(lookup: impl Fn(&str) -> Option<OsString>) -> bool {
    if lookup("TMUX").is_some() {
        return true;
    }
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .filter_map(&lookup)
        .find(|value| !value.is_empty())
        .is_some_and(|value| {
            let value = value.to_string_lossy().to_ascii_uppercase();
            value.contains("UTF-8") || value.contains("UTF8")
        })
}

fn terminal_facts_capabilities_with(
    scope: EndpointFactsScope,
    nested: bool,
    terminal_size: impl FnOnce() -> Option<(u16, u16, u32, u32)>,
    tty: impl FnOnce(CallerTtyScope) -> Option<String>,
    capabilities: &mut Vec<String>,
) {
    if scope.includes_terminal_size()
        && let Some((columns, rows, cell_width_px, cell_height_px)) = terminal_size()
    {
        capabilities.push(format!(
            "{}{columns}x{rows}",
            ClientHello::CLIENT_SIZE_CAPABILITY_PREFIX
        ));
        capabilities.push(format!(
            "{}{cell_width_px}x{cell_height_px}",
            ClientHello::CLIENT_CELL_CAPABILITY_PREFIX
        ));
    }
    if let Some(tty_scope) = scope.tty_scope()
        && let Some(tty) = tty(tty_scope)
    {
        capabilities.push(format!(
            "{}{tty}",
            ClientHello::CLIENT_TTY_CAPABILITY_PREFIX
        ));
    }
    if scope.includes_tty() && nested {
        capabilities.push(ClientHello::CLIENT_NESTED_CAPABILITY.to_owned());
    }
}

fn client_working_directory(
    scope: EndpointFactsScope,
    current_dir: impl FnOnce() -> Option<PathBuf>,
) -> Option<ClientPath> {
    scope
        .includes_working_directory()
        .then(current_dir)
        .flatten()
        .as_deref()
        .and_then(ClientPath::from_path)
        .filter(|working_directory| working_directory.len() <= MAX_CLIENT_WORKING_DIRECTORY_BYTES)
}

/// The directory this client reports as its own. The pin's `find_cwd` prefers
/// `PWD` over `getcwd` whenever both resolve to the same place, so a client
/// started under a symlinked path reports the path the user typed.
fn logical_current_dir() -> Option<PathBuf> {
    let current = std::env::current_dir().ok()?;
    let Some(declared) = std::env::var_os("PWD").filter(|value| !value.is_empty()) else {
        return Some(current);
    };
    let declared = PathBuf::from(declared);
    match (declared.canonicalize(), current.canonicalize()) {
        (Ok(left), Ok(right)) if left == right => Some(declared),
        _ => Some(current),
    }
}

fn client_environment() -> Vec<RawText> {
    client_environment_with(std::env::vars_os())
}

/// The caller's environment as the bytes the OS gave it. A Unix name and value
/// are byte strings, so an entry holding a byte past ASCII rides the hello
/// verbatim instead of being dropped for failing `into_string`.
fn client_environment_with<I, K, V>(environment: I) -> Vec<RawText>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
{
    let environment = environment
        .into_iter()
        .map(|(name, value)| {
            (
                RawText::from_os_str(&name.into()).into_bytes(),
                RawText::from_os_str(&value.into()).into_bytes(),
            )
        })
        .collect::<BTreeMap<Vec<u8>, Vec<u8>>>();
    let mut snapshot = Vec::new();
    let mut total_bytes = 0usize;
    for (name, value) in environment {
        if snapshot.len() == MAX_CLIENT_ENVIRONMENT_ENTRIES {
            break;
        }
        if name.is_empty() || name.contains(&b'=') || name.contains(&0) || value.contains(&0) {
            continue;
        }
        let Some(entry_bytes) = name
            .len()
            .checked_add(1)
            .and_then(|length| length.checked_add(value.len()))
        else {
            continue;
        };
        if entry_bytes > MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES {
            continue;
        }
        let Some(next_total_bytes) = total_bytes.checked_add(entry_bytes) else {
            continue;
        };
        if next_total_bytes > MAX_CLIENT_ENVIRONMENT_BYTES {
            continue;
        }
        let mut entry = name;
        entry.push(b'=');
        entry.extend_from_slice(&value);
        snapshot.push(RawText::from_bytes(entry));
        total_bytes = next_total_bytes;
    }
    snapshot
}

fn materialize_welcome<S: TransportStream>(
    reader: &mut ProtocolReceiver<S>,
    welcome: zz_protocol::Welcome,
) -> Result<ServerHello, DaemonError> {
    let mut hello = ServerHello {
        protocol_version: welcome.protocol_version,
        server_id: welcome.server_id,
        client_id: welcome.client_id,
        client_instance_id: welcome.client_instance_id,
        capabilities: welcome.capability_strings(),
        appearance: zz_terminal::TerminalAppearance::default(),
        appearance_provenance: zz_terminal::AppearanceProvenance::default(),
        mux_options: zz_protocol::MuxOptions::default(),
        status: zz_protocol::StatusLine::default(),
        key_tables: Vec::new(),
    };
    let batch = loop {
        let message = reader.recv()?;
        if let ProtocolMessage::Batch(batch) = message {
            break batch;
        }
        if let ProtocolMessage::CommandResponse(CommandResponse::Error { error, .. }) = message {
            return Err(DaemonError::Server(error));
        }
    };
    for message in batch.messages()? {
        if let ProtocolMessage::Event(zz_protocol::Event { payload, .. }) = message {
            match payload {
                zz_protocol::EventPayload::AppearanceChanged {
                    appearance,
                    provenance,
                } => {
                    hello.appearance = *appearance;
                    hello.appearance_provenance = provenance;
                }
                zz_protocol::EventPayload::MuxOptionsChanged { options } => {
                    hello.mux_options = options;
                }
                zz_protocol::EventPayload::MuxOptionsPatched { options } => {
                    for (key, value) in options.iter() {
                        hello
                            .mux_options
                            .set(key, value.value.clone(), value.source);
                    }
                }
                zz_protocol::EventPayload::StatusChanged { status } => hello.status = status,
                zz_protocol::EventPayload::KeyTablesChanged { tables } => hello.key_tables = tables,
                _ => {}
            }
        }
    }
    reader.pending.push_back(ProtocolMessage::Batch(batch));
    Ok(hello)
}

#[expect(clippy::too_many_arguments)]
fn connect_stream<S: TransportStream>(
    stream: S,
    endpoint_display: impl fmt::Display,
    kind: ClientKind,
    device_name: Option<String>,
    color_scheme: Option<TerminalColorScheme>,
    client_has_terminal: bool,
    send_origin: bool,
    client_facts: EndpointFactsScope,
) -> Result<Connected<S>, DaemonError> {
    connect_stream_with_startup_owner(
        stream,
        endpoint_display,
        kind,
        device_name,
        color_scheme,
        client_has_terminal,
        send_origin,
        false,
        &[],
        client_facts,
    )
}

#[expect(clippy::too_many_arguments)]
fn connect_stream_with_startup_owner<S: TransportStream>(
    stream: S,
    endpoint_display: impl fmt::Display,
    kind: ClientKind,
    device_name: Option<String>,
    color_scheme: Option<TerminalColorScheme>,
    client_has_terminal: bool,
    send_origin: bool,
    startup_config_owner: bool,
    extra_capabilities: &[&str],
    client_facts: EndpointFactsScope,
) -> Result<Connected<S>, DaemonError> {
    connect_stream_hello(
        stream,
        endpoint_display,
        kind,
        device_name,
        color_scheme,
        client_has_terminal,
        send_origin,
        startup_config_owner,
        extra_capabilities,
        client_facts,
        None,
    )
}

#[expect(clippy::too_many_arguments)]
fn connect_stream_hello<S: TransportStream>(
    stream: S,
    endpoint_display: impl fmt::Display,
    kind: ClientKind,
    device_name: Option<String>,
    color_scheme: Option<TerminalColorScheme>,
    client_has_terminal: bool,
    send_origin: bool,
    startup_config_owner: bool,
    extra_capabilities: &[&str],
    client_facts: EndpointFactsScope,
    attach: Option<zz_protocol::AttachOperation>,
) -> Result<Connected<S>, DaemonError> {
    let started = diagnostic_timer();
    let mut reader = ProtocolReceiver::new(stream.try_clone()?);
    let mut writer = ProtocolSender::new(stream);
    let mut capabilities = if kind == ClientKind::Command {
        std::env::var(crate::STARTUP_REENTRY_ENVIRONMENT_VARIABLE)
            .ok()
            .filter(|token| !token.is_empty())
            .map(|token| {
                vec![format!(
                    "{}{}",
                    crate::STARTUP_REENTRY_CAPABILITY_PREFIX,
                    token
                )]
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    startup_config_owner_capability(kind, startup_config_owner, &mut capabilities);
    client_utf8_capability(&mut capabilities);
    client_features_capabilities(&client_terminal_flags().features, &mut capabilities);
    client_learned_features_capability(&mut capabilities);
    if kind == ClientKind::Interactive && client_has_terminal {
        capabilities.push(ClientHello::CLIENT_TERMINAL_CAPABILITY.to_owned());
        if matches!(
            client_facts,
            EndpointFactsScope::None | EndpointFactsScope::LocalHostWorkingDirectory
        ) {
            capabilities.push(ClientHello::CLIENT_NATIVE_TERMINAL_SEARCH_CAPABILITY.to_owned());
            capabilities.push(ClientHello::CLIENT_NATIVE_CHOOSER_CAPABILITY.to_owned());
        }
    }
    capabilities.extend(
        extra_capabilities
            .iter()
            .map(|capability| (*capability).to_owned()),
    );
    terminal_facts_capabilities(
        client_facts,
        std::env::var_os("TMUX").is_some_and(|value| !value.is_empty()),
        &mut capabilities,
    );
    let client_hello = ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: client_instance_id(),
        kind,
        device_name,
        capabilities,
        color_scheme,
        origin: (send_origin && kind == ClientKind::Command)
            .then(|| std::env::var("ZZ_PANE").ok())
            .flatten()
            .and_then(|pane| pane.parse().ok()),
        working_directory: client_working_directory(client_facts, logical_current_dir),
        environment: client_environment(),
        process_id: std::process::id(),
    };
    if kind == ClientKind::Command {
        writer.send(&ProtocolMessage::ClientHello(client_hello))?;
    } else {
        let mut hello = zz_protocol::Hello::from_client(client_hello);
        hello.attach = attach;
        if kind == ClientKind::Control {
            hello.subscriptions.tree = zz_protocol::TreeSubscription::All;
        } else if client_facts.includes_terminal_size() {
            hello.subscriptions = zz_protocol::Subscriptions::terminal();
        }
        hello.viewport = client_facts
            .includes_terminal_size()
            .then(caller_terminal_size)
            .flatten()
            .map(
                |(columns, rows, cell_width_px, cell_height_px)| zz_protocol::ClientViewport {
                    columns,
                    rows,
                    cell_width_px,
                    cell_height_px,
                },
            );
        writer.send(&ProtocolMessage::Hello(hello))?;
    }
    let hello = match reader.recv()? {
        ProtocolMessage::Welcome(welcome) => materialize_welcome(&mut reader, welcome)?,
        ProtocolMessage::ServerHello(hello) => *hello,
        ProtocolMessage::CommandResponse(CommandResponse::Error { error, .. }) => {
            return Err(DaemonError::Server(error));
        }
        _ => {
            return Err(DaemonError::Server(ServerError::Internal(
                "daemon did not send ServerHello".to_owned(),
            )));
        }
    };
    if kind == ClientKind::Interactive
        && [PANE_FRAME_CAPABILITY, zz_protocol::CONTROL_CAPABILITY]
            .iter()
            .any(|required| {
                !hello
                    .capabilities
                    .iter()
                    .any(|capability| capability == required)
            })
    {
        return Err(DaemonError::Protocol(ProtocolError::VersionMismatch {
            expected: PROTOCOL_VERSION,
            received: hello.protocol_version,
        }));
    }
    log::debug!(
        target: "zz_daemon::diagnostics::client",
        "connected path={endpoint_display} kind={kind:?} server_hello={hello:#?} elapsed_us={}",
        diagnostic_elapsed_us(started),
    );
    Ok((reader, writer, hello))
}

pub fn short_device_name() -> Option<String> {
    let host = crate::process_info::host_name()?;
    host.trim()
        .split('.')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

const STDIN_CHUNK_BYTES: usize = 16 * 1024;

fn caller_stdin() -> Result<std::fs::File, String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if STDIN_WAS_CLOSED.load(Ordering::Relaxed) {
        eprintln!(
            "[err] evsig_cb: recv: {}",
            crate::strerror_text(&io::Error::from_raw_os_error(libc::EBADF))
        );
        std::process::exit(1);
    }
    let stdin = io::stdin();
    #[cfg(unix)]
    let duplicate = std::os::fd::AsFd::as_fd(&stdin).try_clone_to_owned();
    #[cfg(windows)]
    let duplicate = std::os::windows::io::AsHandle::as_handle(&stdin).try_clone_to_owned();
    duplicate
        .map(std::fs::File::from)
        .map_err(|error| crate::strerror_text(&error))
}

fn stdin_read_error() -> String {
    errno_text(CLIENT_FILE_READ_ERRNO)
}

#[cfg(unix)]
fn errno_text(errno: i32) -> String {
    crate::strerror_text(&io::Error::from_raw_os_error(errno))
}

#[cfg(not(unix))]
fn errno_text(errno: i32) -> String {
    match errno {
        5 => "Input/output error",
        9 => "Bad file descriptor",
        _ => "Unknown error",
    }
    .to_owned()
}

fn read_command_stdin(binary: bool) -> Result<Vec<u8>, String> {
    read_command_stdin_from(caller_stdin()?, binary)
}

fn read_command_stdin_chunk() -> Result<Vec<u8>, String> {
    read_command_stdin_chunk_from(caller_stdin()?)
}

fn read_command_stdin_chunk_from(mut reader: impl Read) -> Result<Vec<u8>, String> {
    #[cfg(all(unix, feature = "daemon"))]
    let _signal = StdinReadSignal::install().map_err(|error| error.to_string())?;
    let mut chunk = vec![0; STDIN_CHUNK_BYTES];
    loop {
        match reader.read(&mut chunk) {
            Ok(read) => {
                chunk.truncate(read);
                return Ok(chunk);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(stdin_read_error()),
        }
    }
}

fn read_command_stdin_from(reader: impl Read, binary: bool) -> Result<Vec<u8>, String> {
    #[cfg(all(unix, feature = "daemon"))]
    let _signal = StdinReadSignal::install().map_err(|error| error.to_string())?;
    let limit = zz_protocol::MAX_AGENT_SEND_BYTES;
    let mut payload = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut payload)
        .map_err(|_| stdin_read_error())?;
    if payload.len() > limit {
        return Err(format!("standard input exceeds {limit} bytes"));
    }
    if !binary && std::str::from_utf8(&payload).is_err() {
        return Err("could not read standard input: stream did not contain valid UTF-8".to_owned());
    }
    Ok(payload)
}

#[cfg(all(unix, feature = "daemon"))]
struct StdinReadSignal(libc::sigaction);

#[cfg(all(unix, feature = "daemon"))]
#[allow(
    unsafe_code,
    reason = "sigaction saves and restores the exact previous disposition"
)]
impl StdinReadSignal {
    fn install() -> io::Result<Self> {
        unsafe extern "C" fn terminate(_: libc::c_int) {
            unsafe { libc::_exit(0) };
        }
        unsafe {
            let mut previous = std::mem::zeroed();
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = terminate as *const () as usize;
            libc::sigemptyset(&raw mut action.sa_mask);
            if libc::sigaction(libc::SIGTERM, &raw const action, &raw mut previous) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(previous))
        }
    }
}

#[cfg(all(unix, feature = "daemon"))]
#[allow(
    unsafe_code,
    reason = "restore the saved signal disposition when the scope ends"
)]
impl Drop for StdinReadSignal {
    fn drop(&mut self) {
        unsafe { libc::sigaction(libc::SIGTERM, &raw const self.0, std::ptr::null_mut()) };
    }
}

fn answer_client_file(request: &ClientFileRequest) -> ClientFileResponse {
    let path = PathBuf::from(&request.path);
    let (data, error) = match &request.operation {
        ClientFileOperation::ReadStdin { .. } | ClientFileOperation::ReadStdinChunk => {
            (Vec::new(), Some(errno_text(9)))
        }
        ClientFileOperation::Read => match read_client_file(&path) {
            Ok(data) => (data, None),
            Err(error) => (Vec::new(), Some(error)),
        },
        ClientFileOperation::Write { append, data } => {
            (Vec::new(), write_client_file(&path, data, *append).err())
        }
    };
    ClientFileResponse {
        request_id: request.request_id,
        data,
        error,
    }
}

/// The pin's client opens the file itself and reads it through a bufferevent,
/// so an open failure reports its own `errno` while anything that goes wrong
/// afterwards reports `EIO` (`file_read_error_callback`).
fn read_client_file(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| crate::strerror_text(&error))?;
    let limit = u64::try_from(MAX_CLIENT_FILE_BYTES).unwrap_or(u64::MAX);
    let mut data = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|_| errno_text(CLIENT_FILE_READ_ERRNO))?;
    Ok(data)
}

fn write_client_file(path: &Path, data: &[u8], append: bool) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let mut file = options
        .open(path)
        .map_err(|error| crate::strerror_text(&error))?;
    file.write_all(data)
        .and_then(|()| file.flush())
        .map_err(|error| crate::strerror_text(&error))
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    use zz_protocol::{
        ClientHello, ClientKind, ClientPath, MAX_CLIENT_ENVIRONMENT_BYTES,
        MAX_CLIENT_ENVIRONMENT_ENTRIES, MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES,
        MAX_CLIENT_WORKING_DIRECTORY_BYTES, RawText,
    };

    use super::{
        CallerTtyScope, EndpointFactsScope, MAX_CLIENT_FEATURE_SPECS, attach_session_command,
        client_environment_with, client_features_capabilities, client_takes_utf8,
        client_working_directory, startup_config_owner_capability, terminal_facts_capabilities,
        terminal_facts_capabilities_with,
    };

    #[test]
    fn traced_answers_keep_the_question_and_drop_what_was_typed() {
        let message = zz_protocol::ProtocolMessage::AgentAnswerQuestion {
            pane: zz_protocol::PaneId(3),
            request_id: 9,
            answers: vec![zz_protocol::AgentQuestionAnswer {
                id: "Which token?".to_owned(),
                answers: vec!["hunter2".to_owned()],
            }],
        };
        let traced = format!("{:#?}", super::TracedMessage(&message));
        assert!(traced.contains("Which token?"));
        assert!(!traced.contains("hunter2"));
        let prompt = zz_protocol::ProtocolMessage::AgentCancel {
            pane: zz_protocol::PaneId(3),
        };
        assert_eq!(
            format!("{:#?}", super::TracedMessage(&prompt)),
            format!("{prompt:#?}")
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_undecodable_frame_is_skipped_without_losing_the_next_one() {
        use std::io::Write as _;

        let (mut daemon, client) = std::os::unix::net::UnixStream::pair().unwrap();
        let oversized = zz_protocol::ProtocolMessage::Event(zz_protocol::Event {
            sequence: 1,
            payload: zz_protocol::EventPayload::ChooseTree {
                state: Some(zz_protocol::ChooseTreeState {
                    items: Vec::new(),
                    search: None,
                    selected: 0,
                    kind: zz_protocol::ChooseTreeKind::Panes,
                    filter_no_matches: false,
                    prompt: "x".repeat(zz_protocol::MAX_CHOOSE_ITEM_TEXT_BYTES + 1),
                    help: false,
                }),
            },
        });
        let next = zz_protocol::ProtocolMessage::Attach {
            session: "next".to_owned(),
        };
        for message in [&oversized, &next] {
            daemon
                .write_all(&zz_protocol::encode_protocol_message(message).unwrap())
                .unwrap();
        }
        let mut receiver = super::ProtocolReceiver::new(client);
        assert_eq!(receiver.recv_decodable().unwrap(), (next, true));
    }

    #[cfg(all(unix, feature = "daemon"))]
    #[test]
    #[allow(
        unsafe_code,
        reason = "query signal dispositions without changing them"
    )]
    fn stdin_read_restores_signal_disposition_on_success_and_errors() {
        fn disposition() -> libc::sighandler_t {
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                assert_eq!(
                    libc::sigaction(libc::SIGTERM, std::ptr::null(), &raw mut action),
                    0
                );
                action.sa_sigaction
            }
        }
        struct FailedRead;
        impl std::io::Read for FailedRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("read failed"))
            }
        }
        let previous = disposition();
        let command = super::StdinReadSignal::install().expect("command signal");
        let pending = disposition();
        assert_eq!(
            super::read_command_stdin_from(&b"ok"[..], false),
            Ok(b"ok".to_vec())
        );
        assert_eq!(disposition(), pending);
        assert!(super::read_command_stdin_from(FailedRead, false).is_err());
        assert_eq!(disposition(), pending);
        assert!(super::read_command_stdin_from(&b"\xff"[..], false).is_err());
        assert_eq!(disposition(), pending);
        assert!(super::read_command_stdin_from(std::io::repeat(b'x'), true).is_err());
        assert_eq!(disposition(), pending);
        drop(command);
        assert_eq!(disposition(), previous);
    }

    #[test]
    fn stdin_chunk_returns_available_bytes_and_maps_read_errors_to_eio() {
        struct OneRead(Option<&'static [u8]>);
        impl std::io::Read for OneRead {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let bytes = self
                    .0
                    .take()
                    .expect("a chunk read never waits for more input");
                buffer[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            }
        }
        struct FailedRead;
        impl std::io::Read for FailedRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("is a directory"))
            }
        }
        assert_eq!(
            super::read_command_stdin_chunk_from(OneRead(Some(b"\x1b[31"))),
            Ok(b"\x1b[31".to_vec())
        );
        assert_eq!(
            super::read_command_stdin_chunk_from(OneRead(Some(b""))),
            Ok(Vec::new())
        );
        assert_eq!(
            super::read_command_stdin_chunk_from(FailedRead),
            Err("Input/output error".to_owned())
        );
        assert_eq!(
            super::read_command_stdin_from(FailedRead, true),
            Err("Input/output error".to_owned())
        );
        let chunk = super::read_command_stdin_chunk_from(std::io::repeat(b'x')).expect("chunk");
        assert_eq!(chunk.len(), super::STDIN_CHUNK_BYTES);
    }

    #[test]
    fn attach_session_command_preserves_the_exact_requested_flag_mutation() {
        assert_eq!(
            attach_session_command(
                "work".to_owned(),
                true,
                true,
                Some("ignore-size,!active-pane"),
            ),
            zz_protocol::CommandInvocation::new(
                "attach-session",
                ["-d", "-r", "-f", "ignore-size,!active-pane", "-t", "work",],
            )
        );
    }

    #[test]
    fn local_client_fact_scopes_publish_the_supplied_working_directory() {
        let fixture = PathBuf::from("/tmp/zz-client-cwd-fixture");
        for scope in [
            EndpointFactsScope::LocalHostWorkingDirectory,
            EndpointFactsScope::LocalControlTerminalIdentity,
            EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
        ] {
            assert_eq!(
                client_working_directory(scope, || Some(fixture.clone())),
                ClientPath::from_path(&fixture)
            );
        }
    }

    #[test]
    fn remote_client_fact_scopes_never_read_or_publish_a_local_working_directory() {
        for scope in [
            EndpointFactsScope::None,
            EndpointFactsScope::PortableTerminalSize,
        ] {
            assert_eq!(client_working_directory(scope, || panic!()), None);
        }
    }

    #[test]
    fn local_client_fact_scopes_omit_oversized_working_directories() {
        let fixture = PathBuf::from("x".repeat(MAX_CLIENT_WORKING_DIRECTORY_BYTES + 1));
        assert_eq!(
            client_working_directory(EndpointFactsScope::LocalHostWorkingDirectory, || Some(
                fixture
            )),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_client_fact_scopes_carry_non_utf8_working_directory_bytes() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let fixture = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));
        let published =
            client_working_directory(EndpointFactsScope::LocalHostWorkingDirectory, || {
                Some(fixture.clone())
            })
            .expect("non-UTF-8 working directory is published");
        assert_eq!(published.to_path_buf(), Some(fixture));
    }

    /// tmux.c: `$TMUX` set at all wins outright, otherwise the FIRST of
    /// `LC_ALL`, `LC_CTYPE` and `LANG` that is set and non-empty decides on its own,
    /// so an `LC_CTYPE=C` masks a UTF-8 `LANG` behind it; the test is `strcasestr`
    /// for `UTF-8` or `UTF8`, so either spelling in any case answers. Measured
    /// on tmux d77c9dc6 against show-environment of a byte value.
    #[test]
    fn the_client_utf8_flag_reads_the_inputs_tmux_c_reads_in_the_order_it_reads_them() {
        let takes = |pairs: &[(&str, &str)]| {
            let pairs = pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), OsString::from(*value)))
                .collect::<Vec<_>>();
            client_takes_utf8(|name| {
                pairs
                    .iter()
                    .find(|(candidate, _)| candidate == name)
                    .map(|(_, value)| value.clone())
            })
        };

        assert!(!takes(&[]));
        assert!(!takes(&[("LC_ALL", "C"), ("LC_CTYPE", "C"), ("LANG", "C")]));
        assert!(takes(&[("TMUX", "/tmp/zz,1,0")]));
        assert!(takes(&[("TMUX", ""), ("LC_ALL", "C")]));
        assert!(takes(&[("LC_ALL", "en_US.UTF-8")]));
        assert!(takes(&[("LC_ALL", "en_US.utf8")]));
        assert!(takes(&[("LANG", "C.UTF-8")]));
        assert!(!takes(&[("LC_CTYPE", "C"), ("LANG", "en_US.UTF-8")]));
        assert!(takes(&[("LC_CTYPE", ""), ("LANG", "en_US.UTF-8")]));
        assert!(!takes(&[("LC_ALL", "C"), ("LANG", "en_US.UTF-8")]));
    }

    #[test]
    fn global_feature_flags_travel_as_one_bounded_token_per_flag() {
        let mut capabilities = Vec::new();
        let mut features = vec!["256".to_owned(), "sixel:RGB".to_owned(), "x".repeat(201)];
        features.extend((0..20).map(|index| format!("title{index}")));
        client_features_capabilities(&features, &mut capabilities);
        assert_eq!(capabilities.len(), MAX_CLIENT_FEATURE_SPECS);
        assert_eq!(capabilities[0], "client-features-v1:256");
        assert_eq!(capabilities[1], "client-features-v1:sixel:RGB");
        assert_eq!(capabilities[2], "client-features-v1:title0");
    }

    #[test]
    fn client_environment_is_sorted_deduplicated_and_bounded() {
        let mut environment = (0..MAX_CLIENT_ENVIRONMENT_ENTRIES + 4)
            .rev()
            .map(|index| (format!("ZZ_{index:04}"), index.to_string()))
            .collect::<Vec<_>>();
        environment.extend([
            (String::new(), "empty-name".to_owned()),
            ("BAD=NAME".to_owned(), "value".to_owned()),
            ("NUL_NAME\0".to_owned(), "value".to_owned()),
            ("NUL_VALUE".to_owned(), "value\0".to_owned()),
            ("ZZ_0000".to_owned(), "replacement".to_owned()),
        ]);
        let snapshot = client_environment_with(environment);
        assert_eq!(snapshot.len(), MAX_CLIENT_ENVIRONMENT_ENTRIES);
        assert_eq!(
            snapshot.first().map(crate::RawText::as_str),
            Some("ZZ_0000=replacement")
        );
        assert_eq!(
            snapshot.last().map(crate::RawText::as_str),
            Some("ZZ_4095=4095")
        );
        assert!(snapshot.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn client_environment_skips_oversized_entries_and_caps_aggregate_deterministically() {
        let environment = (0..300)
            .map(|index| {
                (
                    format!("ZZ_{index:03}"),
                    "x".repeat(MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES - 7),
                )
            })
            .chain([(
                "ZZ_OVERSIZED".to_owned(),
                "x".repeat(MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES),
            )])
            .collect::<Vec<_>>();
        let forward = client_environment_with(environment.clone());
        let reverse = client_environment_with(environment.into_iter().rev());
        assert_eq!(forward, reverse);
        assert_eq!(
            forward.len(),
            MAX_CLIENT_ENVIRONMENT_BYTES / MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES
        );
        assert!(
            forward
                .iter()
                .map(zz_protocol::RawText::byte_len)
                .sum::<usize>()
                <= MAX_CLIENT_ENVIRONMENT_BYTES
        );
        assert!(
            forward
                .iter()
                .all(|entry| entry.len() <= MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES)
        );
        assert!(
            !forward
                .iter()
                .any(|entry| entry.starts_with("ZZ_OVERSIZED="))
        );
    }

    /// The pin keeps an environment name and value as a C string, so a byte past
    /// ASCII reaches the session it attaches to. Measured on 2026-09-04 against
    /// tmux d77c9dc6 with `update-environment ZZBYTES` and a pty client launched
    /// with `ZZBYTES=a<0xff>b`: `show-environment -t <session> ZZBYTES` answers
    /// the bytes `5a 5a 42 59 54 45 53 3d 61 ff 62 0a`. The hello therefore has
    /// to carry the entry rather than drop it for failing `into_string`.
    #[cfg(unix)]
    #[test]
    fn client_environment_carries_non_utf8_os_strings_as_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let snapshot = client_environment_with([
            (
                OsString::from_vec(vec![b'Z', b'Z', b'_', 0xff]),
                OsString::from("value"),
            ),
            (
                OsString::from("ZZ_VALUE"),
                OsString::from_vec(vec![b'a', 0xff, b'b']),
            ),
            (OsString::from("ZZ_VALID"), OsString::from("kept")),
        ]);
        assert_eq!(
            snapshot
                .iter()
                .map(crate::RawText::as_bytes)
                .collect::<Vec<_>>(),
            [
                b"ZZ_VALID=kept".as_slice(),
                b"ZZ_VALUE=a\xffb".as_slice(),
                b"ZZ_\xff=value".as_slice(),
            ]
        );
    }

    /// A NUL anywhere and an empty or `=`-bearing name are still refused, on
    /// bytes now rather than on text.
    #[cfg(unix)]
    #[test]
    fn client_environment_still_rejects_nul_and_malformed_names() {
        use std::os::unix::ffi::OsStringExt;

        let snapshot = client_environment_with([
            (
                OsString::from("ZZ_NUL"),
                OsString::from_vec(vec![b'v', 0, b'x']),
            ),
            (
                OsString::from_vec(vec![b'Z', 0, b'Z']),
                OsString::from("value"),
            ),
            (OsString::from(""), OsString::from("value")),
            (OsString::from("ZZ=EQ"), OsString::from("value")),
            (OsString::from("ZZ_VALID"), OsString::from("kept")),
        ]);
        assert_eq!(
            snapshot
                .iter()
                .map(crate::RawText::as_bytes)
                .collect::<Vec<_>>(),
            [b"ZZ_VALID=kept".as_slice()]
        );
    }

    #[test]
    fn portable_terminal_facts_never_include_a_local_tty() {
        assert!(EndpointFactsScope::PortableTerminalSize.includes_terminal_size());
        assert!(!EndpointFactsScope::PortableTerminalSize.includes_tty());
        assert!(!EndpointFactsScope::LocalControlTerminalIdentity.includes_terminal_size());
        assert!(EndpointFactsScope::LocalControlTerminalIdentity.includes_tty());
        assert!(EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal.includes_terminal_size());
        assert!(EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal.includes_tty());
    }

    #[cfg(all(unix, feature = "daemon"))]
    #[test]
    fn handshake_advertises_native_ui_only_for_graphical_terminal_clients() {
        use super::{ProtocolReceiver, ProtocolSender, connect_stream_with_startup_owner};
        use crate::transport::{LocalTransport, Transport, TransportListener};
        use zz_protocol::{CommandResponse, ProtocolMessage, ServerError};

        for (kind, client_has_terminal, scope, expected, desktop) in [
            (
                ClientKind::Interactive,
                true,
                EndpointFactsScope::LocalHostWorkingDirectory,
                true,
            ),
            (
                ClientKind::Interactive,
                true,
                EndpointFactsScope::None,
                true,
            ),
            (
                ClientKind::Interactive,
                true,
                EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
                false,
            ),
            (
                ClientKind::Interactive,
                true,
                EndpointFactsScope::PortableTerminalSize,
                false,
            ),
            (
                ClientKind::Interactive,
                false,
                EndpointFactsScope::LocalHostWorkingDirectory,
                false,
            ),
            (
                ClientKind::Command,
                true,
                EndpointFactsScope::LocalHostWorkingDirectory,
                false,
            ),
            (ClientKind::Control, true, EndpointFactsScope::None, false),
        ]
        .into_iter()
        .map(|row| (row, false))
        .chain([(
            (
                ClientKind::Interactive,
                true,
                EndpointFactsScope::LocalHostWorkingDirectory,
                true,
            ),
            true,
        )])
        .map(|((kind, client_has_terminal, scope, expected), desktop)| {
            (kind, client_has_terminal, scope, expected, desktop)
        }) {
            let directory = tempfile::Builder::new()
                .prefix("zz-search-")
                .tempdir_in("/tmp")
                .expect("create socket directory");
            let socket = directory.path().join("daemon.sock");
            let listener = LocalTransport::bind(&socket).expect("bind handshake listener");
            let server = std::thread::spawn(move || {
                let stream = listener.accept().expect("accept client");
                stream
                    .set_timeout(Some(std::time::Duration::from_secs(5)))
                    .expect("set timeout");
                let mut reader = ProtocolReceiver::new(
                    super::TransportStream::try_clone(&stream).expect("clone stream"),
                );
                let mut writer = ProtocolSender::new(stream);
                let hello = match reader.recv().expect("receive handshake") {
                    ProtocolMessage::ClientHello(hello) => hello,
                    ProtocolMessage::Hello(hello) => hello.into_client(),
                    _ => panic!("expected client handshake"),
                };
                writer
                    .send(&ProtocolMessage::CommandResponse(CommandResponse::Error {
                        request_id: 0,
                        error: ServerError::Internal("handshake captured".to_owned()),
                        output: RawText::default(),
                    }))
                    .expect("finish handshake");
                hello
            });
            let extra: &[&str] = if desktop {
                &[ClientHello::CLIENT_PATH_PICKER_CAPABILITY]
            } else {
                &[]
            };
            let result = connect_stream_with_startup_owner(
                LocalTransport::connect(&socket).expect("connect handshake client"),
                socket.display(),
                kind,
                None,
                None,
                client_has_terminal,
                false,
                false,
                extra,
                scope,
            );
            assert!(
                matches!(result, Err(crate::DaemonError::Server(ServerError::Internal(message))) if message == "handshake captured")
            );
            let hello = server.join().expect("join handshake server");
            assert_eq!(
                hello.capabilities.iter().any(|capability| capability
                    == ClientHello::CLIENT_NATIVE_TERMINAL_SEARCH_CAPABILITY),
                expected,
            );
            assert_eq!(
                hello
                    .capabilities
                    .iter()
                    .any(|capability| capability == ClientHello::CLIENT_NATIVE_CHOOSER_CAPABILITY),
                expected,
            );
            assert_eq!(
                hello
                    .capabilities
                    .iter()
                    .any(|capability| capability == ClientHello::CLIENT_TERMINAL_CAPABILITY),
                kind == ClientKind::Interactive && client_has_terminal,
            );
            assert_eq!(
                hello
                    .capabilities
                    .iter()
                    .any(|capability| capability == ClientHello::CLIENT_PATH_PICKER_CAPABILITY),
                desktop,
            );
        }
    }

    #[test]
    fn an_interactive_client_refuses_a_daemon_without_pane_frames() {
        use super::{ProtocolReceiver, ProtocolSender, connect_stream_with_startup_owner};
        use crate::transport::{LocalTransport, Transport, TransportListener};
        use zz_protocol::{
            ClientId, ClientInstanceId, MuxOptions, PROTOCOL_VERSION, ProtocolError,
            ProtocolMessage, ServerHello, StatusLine,
        };

        for kind in [ClientKind::Interactive, ClientKind::Command] {
            #[cfg(unix)]
            let directory = tempfile::Builder::new()
                .prefix("zz-paneframe-")
                .tempdir_in("/tmp")
                .expect("create socket directory");
            #[cfg(unix)]
            let socket = directory.path().join("daemon.sock");
            #[cfg(windows)]
            let socket = std::path::PathBuf::from(format!(
                r"\\.\pipe\zz-paneframe-{}-{kind:?}",
                std::process::id()
            ));
            let listener = LocalTransport::bind(&socket).expect("bind handshake listener");
            let server = std::thread::spawn(move || {
                let stream = listener.accept().expect("accept client");
                let mut reader = ProtocolReceiver::new(
                    super::TransportStream::try_clone(&stream).expect("clone stream"),
                );
                let mut writer = ProtocolSender::new(stream);
                assert!(matches!(
                    reader.recv().expect("receive handshake"),
                    ProtocolMessage::ClientHello(_) | ProtocolMessage::Hello(_)
                ));

                writer
                    .send(&ProtocolMessage::ServerHello(Box::new(ServerHello {
                        protocol_version: PROTOCOL_VERSION,
                        server_id: 1,
                        client_id: ClientId(1),
                        client_instance_id: ClientInstanceId(1),
                        capabilities: Vec::new(),
                        appearance: zz_terminal::TerminalAppearance::default(),
                        appearance_provenance: zz_terminal::AppearanceProvenance::default(),
                        mux_options: MuxOptions::default(),
                        status: StatusLine::default(),
                        key_tables: Vec::new(),
                    })))
                    .expect("send hello");
            });
            let result = connect_stream_with_startup_owner(
                LocalTransport::connect(&socket).expect("connect handshake client"),
                socket.display(),
                kind,
                None,
                None,
                kind == ClientKind::Interactive,
                false,
                false,
                &[],
                EndpointFactsScope::None,
            );
            server.join().expect("join handshake server");
            match kind {
                ClientKind::Interactive => {
                    let Err(error) = result else {
                        panic!("an interactive client accepted a daemon without pane frames");
                    };
                    assert!(matches!(
                        error,
                        crate::DaemonError::Protocol(ProtocolError::VersionMismatch {
                            expected: PROTOCOL_VERSION,
                            received: PROTOCOL_VERSION,
                        })
                    ));
                    assert!(matches!(
                        crate::classify_local_connect_error(&socket, error),
                        crate::DaemonError::IncompatibleDaemon {
                            daemon: Some(PROTOCOL_VERSION),
                            client: PROTOCOL_VERSION,
                        }
                    ));
                }
                _ => assert!(result.is_ok(), "a command client needs no terminal frames"),
            }
        }
    }

    #[test]
    fn local_control_facts_use_only_stdin_identity() {
        let mut capabilities = Vec::new();
        terminal_facts_capabilities_with(
            EndpointFactsScope::LocalControlTerminalIdentity,
            true,
            || panic!("control facts must not inspect terminal size"),
            |scope| {
                assert_eq!(scope, CallerTtyScope::StandardInput);
                Some("/dev/ttys007".to_owned())
            },
            &mut capabilities,
        );
        assert_eq!(
            capabilities,
            ["client-tty-v1:/dev/ttys007", "client-nested-v1"]
        );
    }

    #[test]
    fn startup_config_owner_capability_is_control_only_and_opt_in() {
        assert_eq!(
            ClientHello::STARTUP_CONFIG_OWNER_CAPABILITY,
            "startup-config-owner-v1"
        );
        for (kind, startup_config_owner, expected) in [
            (ClientKind::Interactive, false, false),
            (ClientKind::Interactive, true, false),
            (ClientKind::Command, false, false),
            (ClientKind::Command, true, false),
            (ClientKind::Control, false, false),
            (ClientKind::Control, true, true),
        ] {
            let mut capabilities = Vec::new();
            startup_config_owner_capability(kind, startup_config_owner, &mut capabilities);
            assert_eq!(
                capabilities,
                if expected {
                    vec![ClientHello::STARTUP_CONFIG_OWNER_CAPABILITY.to_owned()]
                } else {
                    Vec::new()
                }
            );
        }
    }

    #[test]
    fn nested_capability_requires_local_terminal_facts_and_a_nonempty_environment() {
        for (scope, nested, expected) in [
            (EndpointFactsScope::None, true, false),
            (EndpointFactsScope::PortableTerminalSize, true, false),
            (
                EndpointFactsScope::LocalControlTerminalIdentity,
                false,
                false,
            ),
            (EndpointFactsScope::LocalControlTerminalIdentity, true, true),
            (
                EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
                false,
                false,
            ),
            (
                EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
                true,
                true,
            ),
        ] {
            let mut capabilities = Vec::new();
            terminal_facts_capabilities(scope, nested, &mut capabilities);
            assert_eq!(
                capabilities.iter().any(|capability| {
                    capability == zz_protocol::ClientHello::CLIENT_NESTED_CAPABILITY
                }),
                expected
            );
        }
    }
}
