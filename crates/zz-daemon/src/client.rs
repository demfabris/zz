use std::{
    fmt,
    io::{self, Read, Write},
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use parking_lot::Mutex;
use zz_protocol::{
    AgentImage, AgentSessionOpKind, ClientHello, ClientInstanceId, ClientKind, CommandInvocation,
    CommandRequest, CommandResponse, ConfigOverrideEntry, GuiResponse, InputMessage,
    MAX_PASTE_UPLOAD_CHUNK_BYTES, PROTOCOL_VERSION, PaneId, PasteUploadPurpose, ProtocolMessage,
    ServerError, ServerHello, encode_protocol_message_into, read_protocol_message_into,
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
            TerminalFactsScope::Full,
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
                    TerminalFactsScope::Full,
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
                        TerminalFactsScope::SizeOnly,
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

    pub fn execute(&mut self, command: CommandInvocation) -> Result<String, DaemonError> {
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.writer
            .send(&ProtocolMessage::CommandRequest(CommandRequest {
                request_id,
                command,
            }))?;
        loop {
            match self.reader.recv()? {
                ProtocolMessage::CommandResponse(CommandResponse::Success {
                    request_id: response_id,
                    output,
                    exit_code,
                    ..
                }) if response_id == request_id => {
                    return if exit_code == 0 {
                        Ok(output)
                    } else {
                        Err(DaemonError::CommandExit { output, exit_code })
                    };
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
        let stream = LocalTransport::connect(path)?;
        let connected = connect_stream(
            ClientStream::Local(stream),
            path.display(),
            ClientKind::Control,
            None,
            None,
            false,
            false,
            TerminalFactsScope::None,
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

    /// Connect a raw-terminal attach surface: the hello carries the caller's
    /// terminal size, and the caller's tty when `$TMUX` marks a nested run.
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
                        TerminalFactsScope::Full
                    } else {
                        TerminalFactsScope::None
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
                            TerminalFactsScope::SizeOnly
                        } else {
                            TerminalFactsScope::None
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
                            TerminalFactsScope::SizeOnly
                        } else {
                            TerminalFactsScope::None
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
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.send(&ProtocolMessage::CommandRequest(CommandRequest {
            request_id,
            command,
        }))?;
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
enum TerminalFactsScope {
    None,
    SizeOnly,
    Full,
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
fn caller_tty() -> Option<String> {
    use std::os::fd::AsFd as _;

    let stdin = std::io::stdin();
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
fn caller_tty() -> Option<String> {
    None
}

fn terminal_facts_capabilities(scope: TerminalFactsScope, capabilities: &mut Vec<String>) {
    if scope == TerminalFactsScope::None {
        return;
    }
    if let Some((columns, rows)) = caller_terminal_size() {
        capabilities.push(format!(
            "{}{columns}x{rows}",
            ClientHello::CLIENT_SIZE_CAPABILITY_PREFIX
        ));
    }
    if scope == TerminalFactsScope::Full
        && std::env::var("TMUX").is_ok_and(|value| !value.is_empty())
        && let Some(tty) = caller_tty()
    {
        capabilities.push(format!(
            "{}{tty}",
            ClientHello::CLIENT_TTY_CAPABILITY_PREFIX
        ));
    }
}

fn connect<T: Transport>(
    endpoint: &T::Endpoint,
    endpoint_display: impl fmt::Display,
    kind: ClientKind,
    device_name: Option<String>,
    color_scheme: Option<TerminalColorScheme>,
    client_has_terminal: bool,
    send_origin: bool,
    terminal_facts: TerminalFactsScope,
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
        terminal_facts,
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
    terminal_facts: TerminalFactsScope,
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
    if kind == ClientKind::Interactive && client_has_terminal {
        capabilities.push(ClientHello::CLIENT_TERMINAL_CAPABILITY.to_owned());
    }
    terminal_facts_capabilities(terminal_facts, &mut capabilities);
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
