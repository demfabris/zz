use std::{
    fmt,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use parking_lot::Mutex;
use zz_protocol::{
    AgentImage, AgentSessionOpKind, ClientHello, ClientInstanceId, ClientKind, CommandInvocation,
    CommandRequest, CommandResponse, ConfigOverrideEntry, GuiResponse, InputMessage,
    MAX_CLIENT_WORKING_DIRECTORY_BYTES, MAX_PASTE_UPLOAD_CHUNK_BYTES, PROTOCOL_VERSION, PaneId,
    PasteUploadPurpose, PreparedCommand, ProtocolMessage, ServerError, ServerHello,
    encode_protocol_message_into, read_protocol_message_into,
};

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
    pub stdout: String,
    pub stderr: String,
    pub exit_code: u8,
}

pub struct CommandClient {
    reader: ProtocolReceiver<LocalStream>,
    writer: ProtocolSender<LocalStream>,
    hello: ServerHello,
    #[cfg(all(any(unix, windows), not(target_os = "ios")))]
    _ssh_forward: Option<SshForward>,
}

impl CommandClient {
    pub fn connect(path: &Path) -> Result<Self, DaemonError> {
        let connected = connect::<LocalTransport>(
            path,
            path.display(),
            ClientKind::Command,
            None,
            None,
            false,
            true,
            EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
        )?;
        Ok(Self::from_connected(connected))
    }

    /// Connect a short-lived command client to a configured fleet endpoint.
    pub fn connect_endpoint(endpoint: &Endpoint) -> Result<Self, DaemonError> {
        match endpoint {
            Endpoint::Local(path) => {
                let stream = LocalTransport::connect(path)?;
                let connected = connect_stream(
                    stream,
                    path.display(),
                    ClientKind::Command,
                    None,
                    None,
                    false,
                    false,
                    EndpointFactsScope::LocalHostWorkingDirectoryAndTerminal,
                )?;
                Ok(Self::from_connected(connected))
            }
            Endpoint::Ssh(endpoint) => {
                #[cfg(target_os = "ios")]
                {
                    let _ = endpoint;
                    Err(crate::EndpointError::UnsupportedPlatform.into())
                }
                #[cfg(all(any(unix, windows), not(target_os = "ios")))]
                {
                    let ssh_forward = SshForward::start(endpoint, None)?;
                    let stream = LocalTransport::connect(ssh_forward.local_socket())?;
                    let connected = connect_stream(
                        stream,
                        endpoint,
                        ClientKind::Command,
                        None,
                        None,
                        false,
                        false,
                        EndpointFactsScope::PortableTerminalSize,
                    )?;
                    Ok(Self::from_connected_with_ssh(connected, ssh_forward))
                }
                #[cfg(not(any(unix, windows)))]
                {
                    let _ = endpoint;
                    Err(crate::EndpointError::UnsupportedPlatform.into())
                }
            }
        }
    }

    fn from_connected((reader, writer, hello): Connected<LocalStream>) -> Self {
        Self {
            reader,
            writer,
            hello,
            #[cfg(all(any(unix, windows), not(target_os = "ios")))]
            _ssh_forward: None,
        }
    }

    #[cfg(all(any(unix, windows), not(target_os = "ios")))]
    fn from_connected_with_ssh(connected: Connected<LocalStream>, ssh_forward: SshForward) -> Self {
        let mut client = Self::from_connected(connected);
        client._ssh_forward = Some(ssh_forward);
        client
    }

    #[must_use]
    pub fn server_hello(&self) -> &ServerHello {
        &self.hello
    }

    /// Run one command and keep only its stdout, folding a nonzero exit into
    /// `DaemonError::CommandExit`. Callers that need the command's stderr or
    /// that must treat a nonzero exit as a completed command use
    /// [`CommandClient::execute_streams`].
    pub fn execute(&mut self, command: CommandInvocation) -> Result<String, DaemonError> {
        let outcome = self.execute_streams(command)?;
        if outcome.exit_code == 0 {
            Ok(outcome.stdout)
        } else {
            Err(DaemonError::CommandExit {
                output: outcome.stdout,
                exit_code: outcome.exit_code,
            })
        }
    }

    /// Run one command and keep all three of its streams. A command that ran to
    /// completion is `Ok` whatever its exit status; `Err` stays reserved for
    /// dispatch, transport, and server failures.
    pub fn execute_streams(
        &mut self,
        command: CommandInvocation,
    ) -> Result<CommandOutcome, DaemonError> {
        self.execute_streams_with_prepared(command, false)
    }

    pub fn execute_prepared_streams(
        &mut self,
        command: CommandInvocation,
    ) -> Result<CommandOutcome, DaemonError> {
        self.execute_streams_with_prepared(command, true)
    }

    fn execute_streams_with_prepared(
        &mut self,
        command: CommandInvocation,
        prepared: bool,
    ) -> Result<CommandOutcome, DaemonError> {
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.writer
            .send(&ProtocolMessage::CommandRequest(CommandRequest {
                request_id,
                command,
                prepared,
            }))?;
        loop {
            match self.reader.recv()? {
                ProtocolMessage::CommandResponse(CommandResponse::Success {
                    request_id: response_id,
                    output,
                    exit_code,
                    stderr,
                }) if response_id == request_id => {
                    return Ok(CommandOutcome {
                        stdout: output,
                        stderr,
                        exit_code,
                    });
                }
                ProtocolMessage::CommandResponse(CommandResponse::Error {
                    request_id: response_id,
                    error,
                    output,
                }) if response_id == request_id => {
                    let error = DaemonError::Server(error);
                    return if output.is_empty() {
                        Err(error)
                    } else {
                        Err(DaemonError::CommandFailed {
                            output,
                            error: Box::new(error),
                        })
                    };
                }
                _ => {}
            }
        }
    }

    pub fn prepare_commands(
        &mut self,
        commands: Vec<CommandInvocation>,
    ) -> Result<Vec<PreparedCommand>, DaemonError> {
        let command_count = commands.len();
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.writer.send(&ProtocolMessage::PrepareCommandList {
            request_id,
            commands,
        })?;
        loop {
            if let ProtocolMessage::PreparedCommandList {
                request_id: response_id,
                commands,
            } = self.reader.recv()?
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

pub struct InteractiveClient {
    reader: Mutex<ProtocolReceiver<ClientStream>>,
    writer: Mutex<ProtocolSender<ClientStream>>,
    hello: ServerHello,
    #[cfg(all(any(unix, windows), not(target_os = "ios")))]
    ssh_forward: Option<SshForward>,
    #[cfg(target_os = "ios")]
    russh_forward: Option<RusshForward>,
}

impl InteractiveClient {
    pub fn connect(path: &Path) -> Result<Self, DaemonError> {
        Self::connect_endpoint(&Endpoint::Local(path.to_owned()), TerminalColorScheme::Dark)
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
            color_scheme,
            None,
            true,
            false,
        )
    }

    pub fn connect_with_color_scheme_and_terminal(
        path: &Path,
        color_scheme: TerminalColorScheme,
        client_has_terminal: bool,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            &Endpoint::Local(path.to_owned()),
            color_scheme,
            None,
            client_has_terminal,
            false,
        )
    }

    pub fn connect_terminal_surface(
        path: &Path,
        color_scheme: TerminalColorScheme,
        client_has_terminal: bool,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            &Endpoint::Local(path.to_owned()),
            color_scheme,
            None,
            client_has_terminal,
            true,
        )
    }

    pub fn connect_endpoint(
        endpoint: &Endpoint,
        color_scheme: TerminalColorScheme,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(endpoint, color_scheme, None, true, true)
    }

    pub fn connect_endpoint_with_terminal(
        endpoint: &Endpoint,
        color_scheme: TerminalColorScheme,
        client_has_terminal: bool,
    ) -> Result<Self, DaemonError> {
        Self::connect_endpoint_with_prompts_and_terminal(
            endpoint,
            color_scheme,
            None,
            client_has_terminal,
            true,
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
            color_scheme,
            prompts,
            true,
            false,
        )
    }

    fn connect_endpoint_with_prompts_and_terminal(
        endpoint: &Endpoint,
        color_scheme: TerminalColorScheme,
        prompts: Option<crate::askpass::SshPrompts>,
        client_has_terminal: bool,
        terminal_surface: bool,
    ) -> Result<Self, DaemonError> {
        let device_name = short_device_name();
        match endpoint {
            Endpoint::Local(path) => {
                let stream = LocalTransport::connect(path)?;
                let connected = connect_stream(
                    ClientStream::Local(stream),
                    path.display(),
                    ClientKind::Interactive,
                    device_name.clone(),
                    Some(color_scheme),
                    client_has_terminal,
                    false,
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
                    let connected = connect_stream(
                        ClientStream::Ssh(stream),
                        endpoint,
                        ClientKind::Interactive,
                        device_name,
                        Some(color_scheme),
                        client_has_terminal,
                        false,
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
                    let connected = connect_stream(
                        ClientStream::Local(stream),
                        endpoint,
                        ClientKind::Interactive,
                        device_name,
                        Some(color_scheme),
                        client_has_terminal,
                        false,
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
        Self {
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
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
    ) -> Result<(), DaemonError> {
        let session = session.into();
        if !detach_others && !read_only {
            return self.attach(session);
        }
        let mut args = Vec::new();
        if detach_others {
            args.push("-d".to_owned());
        }
        if read_only {
            args.push("-r".to_owned());
        }
        if !session.is_empty() {
            args.extend(["-t".to_owned(), session]);
        }
        self.send(&ProtocolMessage::CommandRequest(CommandRequest {
            request_id: 0,
            command: CommandInvocation::new("attach-session", args),
            prepared: false,
        }))
    }

    pub fn detach(&self) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::Detach)
    }

    pub fn send_input(&self, input: InputMessage) -> Result<(), DaemonError> {
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

    pub fn request_resync(&self) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::Resync)
    }

    pub fn request_full(&self, pane: PaneId) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::RequestFull { pane })
    }

    pub fn request_history(&self, pane: PaneId, start: u32, count: u32) -> Result<(), DaemonError> {
        self.send(&ProtocolMessage::HistoryRequest { pane, start, count })
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

    pub fn recv(&self) -> Result<ProtocolMessage, DaemonError> {
        let started = diagnostic_timer();
        let lock_started = diagnostic_timer();
        let mut reader = self.reader.lock();
        let lock_wait_us = diagnostic_elapsed_us(lock_started);
        let result = reader.recv();
        drop(reader);
        log::trace!(
            target: "zz_daemon::diagnostics::client",
            "interactive_recv success={} lock_wait_us={} total_elapsed_us={}",
            result.is_ok(),
            lock_wait_us,
            diagnostic_elapsed_us(started),
        );
        result
    }

    fn send(&self, message: &ProtocolMessage) -> Result<(), DaemonError> {
        let started = diagnostic_timer();
        let lock_started = diagnostic_timer();
        let mut writer = self.writer.lock();
        let lock_wait_us = diagnostic_elapsed_us(lock_started);
        let result = writer.send(message);
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

struct ProtocolSender<S> {
    stream: S,
    frame: Vec<u8>,
}

struct ProtocolReceiver<S> {
    stream: S,
    frame: Vec<u8>,
}

type Connected<S> = (ProtocolReceiver<S>, ProtocolSender<S>, ServerHello);

impl<S: TransportStream> ProtocolReceiver<S> {
    fn new(stream: S) -> Self {
        Self {
            stream,
            frame: Vec::new(),
        }
    }

    fn recv(&mut self) -> Result<ProtocolMessage, DaemonError> {
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
        let started = diagnostic_timer();
        let encode_started = diagnostic_timer();
        encode_protocol_message_into(message, &mut self.frame)?;
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

#[cfg(unix)]
fn caller_terminal_size() -> Option<(u16, u16)> {
    let size = rustix::termios::tcgetwinsize(std::io::stdout()).ok()?;
    (size.ws_col > 0 && size.ws_row > 0).then_some((size.ws_col, size.ws_row))
}

#[cfg(not(unix))]
fn caller_terminal_size() -> Option<(u16, u16)> {
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

fn terminal_facts_capabilities_with(
    scope: EndpointFactsScope,
    nested: bool,
    terminal_size: impl FnOnce() -> Option<(u16, u16)>,
    tty: impl FnOnce(CallerTtyScope) -> Option<String>,
    capabilities: &mut Vec<String>,
) {
    if scope.includes_terminal_size()
        && let Some((columns, rows)) = terminal_size()
    {
        capabilities.push(format!(
            "{}{columns}x{rows}",
            ClientHello::CLIENT_SIZE_CAPABILITY_PREFIX
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
) -> Option<PathBuf> {
    scope
        .includes_working_directory()
        .then(current_dir)
        .flatten()
        .filter(|working_directory| working_directory.to_str().is_some())
        .filter(|working_directory| {
            working_directory.as_os_str().as_encoded_bytes().len()
                <= MAX_CLIENT_WORKING_DIRECTORY_BYTES
        })
}

fn connect<T: Transport>(
    endpoint: &T::Endpoint,
    endpoint_display: impl fmt::Display,
    kind: ClientKind,
    device_name: Option<String>,
    color_scheme: Option<TerminalColorScheme>,
    client_has_terminal: bool,
    send_origin: bool,
    client_facts: EndpointFactsScope,
) -> Result<Connected<T::Stream>, DaemonError> {
    let stream = T::connect(endpoint)?;
    connect_stream(
        stream,
        endpoint_display,
        kind,
        device_name,
        color_scheme,
        client_has_terminal,
        send_origin,
        client_facts,
    )
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
    client_facts: EndpointFactsScope,
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
    if kind == ClientKind::Interactive && client_has_terminal {
        capabilities.push(ClientHello::CLIENT_TERMINAL_CAPABILITY.to_owned());
    }
    terminal_facts_capabilities(
        client_facts,
        std::env::var_os("TMUX").is_some_and(|value| !value.is_empty()),
        &mut capabilities,
    );
    writer.send(&ProtocolMessage::ClientHello(ClientHello {
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
        working_directory: client_working_directory(client_facts, || std::env::current_dir().ok()),
    }))?;
    let hello = match reader.recv()? {
        ProtocolMessage::ServerHello(hello) => hello,
        ProtocolMessage::CommandResponse(CommandResponse::Error { error, .. }) => {
            return Err(DaemonError::Server(error));
        }
        _ => {
            return Err(DaemonError::Server(ServerError::Internal(
                "daemon did not send ServerHello".to_owned(),
            )));
        }
    };
    log::debug!(
        target: "zz_daemon::diagnostics::client",
        "connected path={endpoint_display} kind={kind:?} server_hello={hello:#?} elapsed_us={}",
        diagnostic_elapsed_us(started),
    );
    Ok((reader, writer, hello))
}

pub fn short_device_name() -> Option<String> {
    let host = sysinfo::System::host_name()?;
    host.trim()
        .split('.')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zz_protocol::{ClientHello, ClientKind, MAX_CLIENT_WORKING_DIRECTORY_BYTES};

    use super::{
        CallerTtyScope, EndpointFactsScope, client_working_directory, terminal_facts_capabilities,
        startup_config_owner_capability, terminal_facts_capabilities_with,
    };

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
                Some(fixture.clone())
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
    fn local_client_fact_scopes_omit_non_utf8_working_directories() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let fixture = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));
        assert_eq!(
            client_working_directory(EndpointFactsScope::LocalHostWorkingDirectory, || Some(
                fixture
            )),
            None
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
