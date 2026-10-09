use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, IsTerminal as _, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(not(unix))]
use std::io::BufRead as _;
#[cfg(unix)]
use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd};
#[cfg(any(not(unix), test))]
use std::sync::mpsc;
#[cfg(any(not(unix), test))]
use std::thread;

use zz_daemon::InteractiveClient;
use zz_protocol::{
    CommandInvocation, CommandResponse, ControlSourceFileEvent, EventPayload, ExecOutcome,
    MuxSnapshot, ProtocolMessage, SessionId, WindowId,
};
#[cfg(test)]
use zz_protocol::{PreparedCommand, PreparedCommandResult, RawText, ServerError, StdoutClaim};

use super::{
    SocketSelectionSource, connect_or_spawn_daemon, format_local_command_error,
    tmux_command_starts_server, tmux_label_creation_error,
};

#[cfg(test)]
const CONTROL_PARSE_SOURCE: &str = "<control>";
const DCS: &[u8] = b"\x1bP1000p";
const ST: &[u8] = b"\x1b\\";

static TERMINATION_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
struct ControlSignal(libc::sigaction);

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "install and restore the control client's SIGTERM disposition"
)]
impl ControlSignal {
    fn install() -> io::Result<Self> {
        extern "C" fn terminate(_: libc::c_int) {
            TERMINATION_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        TERMINATION_REQUESTED.store(false, std::sync::atomic::Ordering::Relaxed);
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

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "restore the disposition saved by the control signal scope"
)]
impl Drop for ControlSignal {
    fn drop(&mut self) {
        unsafe { libc::sigaction(libc::SIGTERM, &raw const self.0, std::ptr::null_mut()) };
    }
}

struct ControlReceiver {
    source: ControlSource,
    pending: VecDeque<ProtocolMessage>,
}

enum ControlSource {
    #[cfg(any(not(unix), test))]
    Channel(mpsc::Receiver<MainEvent>),
    #[cfg(unix)]
    Direct(DirectControl),
}

impl ControlReceiver {
    #[cfg(any(not(unix), test))]
    fn new(events: mpsc::Receiver<MainEvent>) -> Self {
        Self {
            source: ControlSource::Channel(events),
            pending: VecDeque::new(),
        }
    }

    #[cfg(unix)]
    fn direct(
        client: Arc<InteractiveClient>,
        stdio: bool,
        pending: VecDeque<ProtocolMessage>,
    ) -> io::Result<Self> {
        Ok(Self {
            source: ControlSource::Direct(DirectControl::new(client, stdio)?),
            pending,
        })
    }

    #[cfg(unix)]
    fn take_sync(&mut self) -> Option<u64> {
        match &mut self.source {
            ControlSource::Direct(direct) => direct.sync.take(),
            #[cfg(test)]
            ControlSource::Channel(_) => None,
        }
    }

    #[cfg(unix)]
    fn take_submitted(&mut self) -> usize {
        match &mut self.source {
            ControlSource::Direct(direct) => std::mem::take(&mut direct.input.submitted_taken),
            #[cfg(test)]
            ControlSource::Channel(_) => 0,
        }
    }

    #[cfg(unix)]
    fn idle_received(&self) -> Option<u64> {
        match &self.source {
            ControlSource::Direct(direct) => (direct.stdio
                && self.pending.is_empty()
                && direct.pending_protocol.is_none()
                && direct.sync.is_none()
                && direct.input.submitted_taken == 0
                && direct.input.bytes.is_empty()
                && !direct.input.has_event())
            .then_some(direct.received),
            #[cfg(test)]
            ControlSource::Channel(_) => None,
        }
    }

    fn receive(&mut self, timeout: Option<std::time::Duration>) -> io::Result<Option<MainEvent>> {
        self.receive_with_probe(timeout, true, true)
    }

    fn receive_with_probe(
        &mut self,
        timeout: Option<std::time::Duration>,
        mut probe_protocol: bool,
        mut poll_empty: bool,
    ) -> io::Result<Option<MainEvent>> {
        loop {
            if let Some(message) = self.pending.pop_front() {
                return Ok(Some(MainEvent::Protocol(Box::new(message))));
            }
            let event = match &mut self.source {
                #[cfg(any(not(unix), test))]
                ControlSource::Channel(events) => {
                    let _ = (probe_protocol, poll_empty);
                    if let Some(timeout) = timeout {
                        match events.recv_timeout(timeout) {
                            Ok(event) => Some(event),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                Some(MainEvent::Disconnected)
                            }
                        }
                    } else {
                        match events.try_recv() {
                            Ok(event) => Some(event),
                            Err(mpsc::TryRecvError::Empty) => None,
                            Err(mpsc::TryRecvError::Disconnected) => Some(MainEvent::Disconnected),
                        }
                    }
                }
                #[cfg(unix)]
                ControlSource::Direct(direct) => {
                    direct.receive(timeout, probe_protocol, poll_empty)?
                }
            };
            probe_protocol = true;
            poll_empty = true;
            match event {
                Some(MainEvent::Protocol(message))
                    if matches!(message.as_ref(), ProtocolMessage::Batch(_)) =>
                {
                    let ProtocolMessage::Batch(batch) = *message else {
                        unreachable!();
                    };
                    self.pending = batch
                        .messages()
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                        .into();
                }
                event => return Ok(event),
            }
        }
    }
}

#[cfg(unix)]
struct DirectControl {
    client: Arc<InteractiveClient>,
    socket: OwnedFd,
    input: ControlInput,
    readable: Vec<rustix::event::FdSetElement>,
    pending_protocol: Option<MainEvent>,
    disconnected: bool,
    prefer_stdin: bool,
    stdio: bool,
    received: u64,
    sync: Option<u64>,
}

#[cfg(unix)]
impl DirectControl {
    fn new(client: Arc<InteractiveClient>, stdio: bool) -> io::Result<Self> {
        let mut direct = Self::with_input(client, io::stdin().as_fd().try_clone_to_owned()?)?;
        direct.input.enabled = false;
        direct.input.remote = stdio;
        direct.stdio = stdio;
        Ok(direct)
    }

    fn with_input(client: Arc<InteractiveClient>, input: OwnedFd) -> io::Result<Self> {
        let socket = client.receive_fd()?;
        let input = ControlInput::new(input);
        let bound = socket.as_raw_fd().max(input.fd.as_raw_fd()) + 1;
        Ok(Self {
            client,
            socket,
            input,
            readable: vec![
                rustix::event::FdSetElement::default();
                rustix::event::fd_set_num_elements(2, bound)
            ],
            pending_protocol: None,
            disconnected: false,
            prefer_stdin: false,
            stdio: false,
            received: 0,
            sync: None,
        })
    }

    fn read_protocol(&mut self, read_socket: bool) {
        while self.pending_protocol.is_none() && !self.disconnected {
            let result = if read_socket {
                self.client.try_recv()
            } else {
                self.client.try_recv_buffered()
            };
            match result {
                Ok(Some(message)) => {
                    if self.stdio {
                        self.received += 1;
                        match *message {
                            ProtocolMessage::ControlStdin {
                                bytes,
                                submitted,
                                closed,
                                error,
                            } => {
                                self.input.bytes.extend_from_slice(&bytes);
                                self.input.presubmitted += usize::from(submitted);
                                self.input.closed |= closed || error.is_some();
                                if error.is_some() {
                                    self.input.error = error;
                                }
                                return;
                            }
                            ProtocolMessage::ControlStdioSync { next_number } => {
                                self.sync = Some(next_number);
                            }
                            message => {
                                self.pending_protocol =
                                    Some(MainEvent::Protocol(Box::new(message)));
                            }
                        }
                    } else {
                        self.pending_protocol = Some(MainEvent::Protocol(message));
                    }
                }
                Ok(None) => return,
                Err(_) => {
                    self.disconnected = true;
                    self.pending_protocol = Some(MainEvent::Disconnected);
                }
            }
        }
    }

    #[allow(
        unsafe_code,
        reason = "select only borrows the two owned open descriptors"
    )]
    fn receive(
        &mut self,
        timeout: Option<std::time::Duration>,
        probe_protocol: bool,
        poll_empty: bool,
    ) -> io::Result<Option<MainEvent>> {
        if self.prefer_stdin && self.input.has_event() {
            return Ok(self.take_ready(false));
        }
        if probe_protocol {
            self.read_protocol(true);
        } else if !poll_empty {
            self.read_protocol(self.input.has_event());
        }
        if self.input.has_event() || (self.pending_protocol.is_some() && !self.prefer_stdin) {
            return Ok(self.take_ready(false));
        }
        if !poll_empty && self.pending_protocol.is_none() && !self.disconnected {
            return Ok(None);
        }
        self.readable.fill(rustix::event::FdSetElement::default());
        if self.input.can_read() && !self.input.has_event() {
            rustix::event::fd_set_insert(&mut self.readable, self.input.fd.as_raw_fd());
        }
        if self.pending_protocol.is_none() && !self.disconnected {
            rustix::event::fd_set_insert(&mut self.readable, self.socket.as_raw_fd());
        }
        let timeout = if self.pending_protocol.is_some() || self.input.has_event() {
            std::time::Duration::ZERO
        } else {
            timeout.unwrap_or_default()
        };
        let timeout = rustix::event::Timespec {
            tv_sec: timeout.as_secs().try_into().unwrap_or(i64::MAX),
            tv_nsec: i64::from(timeout.subsec_nanos()),
        };
        let bound = rustix::event::fd_set_bound(&self.readable);
        match unsafe {
            rustix::event::select(bound, Some(&mut self.readable), None, None, Some(&timeout))
        } {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let mut input_ready = false;
        let mut socket_ready = false;
        for fd in rustix::event::FdSetIter::new(&self.readable) {
            input_ready |= fd == self.input.fd.as_raw_fd();
            socket_ready |= fd == self.socket.as_raw_fd();
        }
        if input_ready {
            self.input.read();
        }
        if socket_ready {
            self.read_protocol(true);
        }
        Ok(self.take_ready(true))
    }

    fn take_ready(&mut self, polled: bool) -> Option<MainEvent> {
        if self.input.has_event() && (self.prefer_stdin || self.pending_protocol.is_none()) {
            self.prefer_stdin = false;
            return self.input.event().map(MainEvent::Stdin);
        }
        if let Some(event) = self.pending_protocol.take() {
            self.prefer_stdin = !polled;
            return Some(event);
        }
        self.disconnected.then_some(MainEvent::Disconnected)
    }
}

#[cfg(unix)]
struct ControlInput {
    enabled: bool,
    remote: bool,
    presubmitted: usize,
    submitted_taken: usize,
    fd: OwnedFd,
    bytes: Vec<u8>,
    closed: bool,
    eof_sent: bool,
    error: Option<String>,
}

#[cfg(unix)]
impl ControlInput {
    fn new(fd: OwnedFd) -> Self {
        Self {
            enabled: true,
            remote: false,
            presubmitted: 0,
            submitted_taken: 0,
            fd,
            bytes: Vec::new(),
            closed: false,
            eof_sent: false,
            error: None,
        }
    }

    fn can_read(&self) -> bool {
        self.enabled && !self.closed && !self.remote
    }

    fn has_event(&self) -> bool {
        if !self.enabled {
            return false;
        }
        self.error.is_some() || (self.closed && !self.eof_sent) || self.bytes.contains(&b'\n')
    }

    fn read(&mut self) {
        let mut bytes = [0; 4096];
        match rustix::io::read(&self.fd, &mut bytes) {
            Ok(0) => self.closed = true,
            Ok(count) => self.bytes.extend_from_slice(&bytes[..count]),
            Err(rustix::io::Errno::INTR | rustix::io::Errno::AGAIN) => {}
            Err(error) => {
                self.closed = true;
                self.error = Some(error.to_string());
            }
        }
    }

    fn event(&mut self) -> Option<StdinEvent> {
        if let Some(error) = self.error.take() {
            self.bytes.clear();
            self.eof_sent = true;
            return Some(StdinEvent::Error(error));
        }
        let bytes = if let Some(end) = self.bytes.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<_> = self.bytes.drain(..=end).collect();
            line.pop();
            line
        } else if self.closed && !self.bytes.is_empty() {
            std::mem::take(&mut self.bytes)
        } else if self.closed && !self.eof_sent {
            self.eof_sent = true;
            return Some(StdinEvent::Eof);
        } else {
            return None;
        };
        Some(match String::from_utf8(bytes) {
            Ok(line) => {
                if self.presubmitted != 0 {
                    self.presubmitted -= 1;
                    self.submitted_taken += 1;
                }
                StdinEvent::Line(line)
            }
            Err(error) => {
                self.bytes.clear();
                self.closed = true;
                self.eof_sent = true;
                StdinEvent::Error(error.to_string())
            }
        })
    }
}

fn receive_control_event(receiver: &mut ControlReceiver) -> io::Result<MainEvent> {
    receive_control_event_with_probe(receiver, true)
}

fn receive_control_event_with_probe(
    receiver: &mut ControlReceiver,
    mut probe_protocol: bool,
) -> io::Result<MainEvent> {
    loop {
        if TERMINATION_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if let Some(event) = receiver.receive_with_probe(
            Some(std::time::Duration::from_millis(20)),
            probe_protocol,
            true,
        )? {
            return Ok(event);
        }
        probe_protocol = true;
    }
}

fn receive_buffered_control_event<W: Write>(
    receiver: &mut ControlReceiver,
    output: &mut ControlWriter<W>,
) -> io::Result<MainEvent> {
    if TERMINATION_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(io::Error::from(io::ErrorKind::Interrupted));
    }
    let event = if let Some(event) = receiver.receive_with_probe(None, output.block_open, false)? {
        if !matches!(
            &event,
            MainEvent::Protocol(message)
                if matches!(message.as_ref(), ProtocolMessage::CommandResponse(_))
        ) {
            output.output.flush()?;
        }
        event
    } else {
        output.output.flush()?;
        receive_control_event_with_probe(receiver, false)?
    };
    #[cfg(unix)]
    if let Some(next_number) = receiver.take_sync() {
        output.next_number = next_number;
    }
    Ok(event)
}

pub(crate) fn run(
    socket_path: &Path,
    socket_source: SocketSelectionSource,
    mux_config_files: &[PathBuf],
    no_start_server: bool,
    level: u8,
    arguments: &[zz_protocol::RawText],
) -> ExitCode {
    #[cfg(unix)]
    let _signal = match ControlSignal::install() {
        Ok(signal) => signal,
        Err(error) => {
            eprintln!("zz: {error}");
            return ExitCode::FAILURE;
        }
    };
    let commands = if arguments.is_empty() {
        vec![CommandInvocation::new("new-session", [] as [&str; 0])]
    } else {
        super::split_command_chain(arguments)
    };
    let start_server = !no_start_server
        && commands
            .first()
            .is_some_and(|command| tmux_command_starts_server(&command.name));
    if let Some(error) = tmux_label_creation_error(socket_path, socket_source, start_server) {
        eprintln!("{}", error.message);
        return ExitCode::FAILURE;
    }
    let client = if start_server {
        connect_or_spawn_daemon(
            socket_path,
            None,
            mux_config_files,
            |startup_config_owner| {
                InteractiveClient::connect_control_with_startup_owner(
                    socket_path,
                    startup_config_owner,
                )
            },
            InteractiveClient::server_hello,
        )
    } else {
        InteractiveClient::connect_control(socket_path)
    };
    let client = match client {
        Ok(client) => Arc::new(client),
        Err(error) => {
            eprintln!("{}", format_local_command_error(socket_path, error));
            return ExitCode::FAILURE;
        }
    };
    #[cfg(unix)]
    let (flags, stash) = match start_stdio(&client) {
        Ok(started) => started,
        Err(error) => {
            eprintln!("{}", format_local_command_error(socket_path, error.into()));
            return ExitCode::FAILURE;
        }
    };
    #[cfg(not(unix))]
    let (flags, stash) = (None::<()>, VecDeque::new());
    let stdio = flags.is_some();
    let terminal = match ControlTerminal::enter(level >= 2) {
        Ok(terminal) => terminal,
        Err(error) => {
            eprintln!("zz: {error}");
            return ExitCode::FAILURE;
        }
    };
    let sink = if let Some(flags) = flags {
        ControlOutput::daemon(Arc::clone(&client), flags)
    } else {
        ControlOutput::Stdout(io::BufWriter::new(io::stdout().lock()))
    };
    let mut output = ControlWriter::new(sink, level >= 2);
    let result = output
        .start()
        .and_then(|()| drive(&client, commands, &mut output, stdio, stash));
    let result = if TERMINATION_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
        #[cfg(unix)]
        if !stdio {
            let _ = client.shutdown();
        }
        let terminated = output.terminate().map(|()| 0);
        #[cfg(unix)]
        if stdio {
            output.output.close();
            let _ = client.shutdown();
        }
        terminated
    } else {
        result
    };
    drop(terminal);
    output.close();
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("zz: {error}");
            ExitCode::FAILURE
        }
    }
}

fn drive(
    client: &Arc<InteractiveClient>,
    initial: Vec<CommandInvocation>,
    output: &mut ControlWriter<ControlOutput>,
    stdio: bool,
    stash: VecDeque<ProtocolMessage>,
) -> io::Result<u8> {
    #[cfg(unix)]
    let mut receiver = ControlReceiver::direct(Arc::clone(client), stdio, stash)?;
    #[cfg(not(unix))]
    let _ = (stdio, stash);
    #[cfg(not(unix))]
    let (events, channel) = mpsc::sync_channel(32);
    #[cfg(not(unix))]
    spawn_protocol_reader(Arc::clone(client), events.clone());
    #[cfg(not(unix))]
    let mut receiver = ControlReceiver::new(channel);
    let mut state = ControlState::default();
    let mut pending_stdin = VecDeque::new();
    let initial_result = execute_command_unit(
        client.as_ref(),
        &mut receiver,
        output,
        initial,
        None,
        0,
        &mut state,
        &mut pending_stdin,
        None,
    )?;
    #[cfg(unix)]
    {
        match &mut receiver.source {
            ControlSource::Direct(direct) => direct.input.enabled = true,
            #[cfg(test)]
            ControlSource::Channel(_) => unreachable!(),
        }
    }
    #[cfg(not(unix))]
    spawn_stdin_reader(events);

    if initial_result.exit.is_some() {
        finish_exit(
            output,
            initial_result.exit.reason(),
            state.wait_exit,
            false,
            &mut receiver,
            &mut pending_stdin,
        )?;
        return Ok(match initial_result.exit {
            ExitSignal::Detached => 0,
            ExitSignal::Clean => state.return_code,
            _ => initial_result.exit_code,
        });
    }
    if initial_result.exit_code != 0 || state.attached_session.is_none() {
        finish_exit(
            output,
            None,
            state.wait_exit,
            false,
            &mut receiver,
            &mut pending_stdin,
        )?;
        return Ok(completed_exit_code(initial_result.exit_code, &state));
    }

    if let Some(pending_return) = take_ready_pending_return(&mut state.pending_return) {
        return finish_control_return(
            client.as_ref(),
            pending_return,
            output,
            &mut state,
            &mut receiver,
            &mut pending_stdin,
        );
    }
    loop {
        if state.tree_sync_required {
            client.request_tree_sync().map_err(io::Error::other)?;
            state.tree_sync_required = false;
        }
        #[cfg(unix)]
        if pending_stdin.is_empty()
            && state.pending_return.is_none()
            && !state.tree_sync_required
            && state.submitted_lines == 0
            && !output.block_open
            && output.deferred.is_empty()
            && let Some(received) = receiver.idle_received()
        {
            output.output.report_idle(received, output.next_number);
        }
        let event = pending_stdin.pop_front().map_or_else(
            || receive_control_event(&mut receiver),
            |stdin| {
                if let Some(pending_return) = state.pending_return.as_mut() {
                    pending_return.consume_preceding_input();
                }
                Ok(MainEvent::Stdin(stdin))
            },
        )?;
        #[cfg(unix)]
        {
            if let Some(next_number) = receiver.take_sync() {
                output.next_number = next_number;
            }
            if matches!(event, MainEvent::Stdin(StdinEvent::Line(_))) {
                state.submitted_lines += receiver.take_submitted();
            }
        }
        match event {
            MainEvent::Stdin(StdinEvent::Line(line)) => {
                if line.is_empty() {
                    return finish_control_return(
                        client.as_ref(),
                        PendingReturn::Blank {
                            code: state.return_code,
                            preceding_input: 0,
                            observed_preceding_input: false,
                        },
                        output,
                        &mut state,
                        &mut receiver,
                        &mut pending_stdin,
                    );
                }
                if line.trim().is_empty() || line.trim_start().starts_with('#') {
                    continue;
                }
                let result = execute_command_unit(
                    client.as_ref(),
                    &mut receiver,
                    output,
                    Vec::new(),
                    Some(line),
                    1,
                    &mut state,
                    &mut pending_stdin,
                    None,
                )?;
                if result.exit.is_some() {
                    finish_exit(
                        output,
                        result.exit.reason(),
                        state.wait_exit,
                        false,
                        &mut receiver,
                        &mut pending_stdin,
                    )?;
                    return Ok(match result.exit {
                        ExitSignal::Detached => 0,
                        ExitSignal::Clean => state.return_code,
                        _ => result.exit_code,
                    });
                }
                if let Some(pending_return) = take_ready_pending_return(&mut state.pending_return) {
                    return finish_control_return(
                        client.as_ref(),
                        pending_return,
                        output,
                        &mut state,
                        &mut receiver,
                        &mut pending_stdin,
                    );
                }
            }
            MainEvent::Stdin(StdinEvent::Eof) => {
                return finish_control_return(
                    client.as_ref(),
                    PendingReturn::Eof {
                        code: state.return_code,
                        preceding_input: 0,
                        observed_preceding_input: false,
                    },
                    output,
                    &mut state,
                    &mut receiver,
                    &mut pending_stdin,
                );
            }
            MainEvent::Stdin(StdinEvent::Error(error)) => {
                return finish_control_return(
                    client.as_ref(),
                    PendingReturn::InputError {
                        message: error,
                        preceding_input: 0,
                    },
                    output,
                    &mut state,
                    &mut receiver,
                    &mut pending_stdin,
                );
            }
            MainEvent::Protocol(message) => {
                let exit = handle_protocol(*message, &mut state, output)?;
                if exit.is_some() {
                    finish_exit(
                        output,
                        exit.reason(),
                        state.wait_exit,
                        false,
                        &mut receiver,
                        &mut pending_stdin,
                    )?;
                    return Ok(match exit {
                        ExitSignal::Detached => 0,
                        ExitSignal::Clean => state.return_code,
                        _ => 1,
                    });
                }
            }
            MainEvent::Disconnected => {
                finish_exit(
                    output,
                    Some("server exited unexpectedly"),
                    state.wait_exit,
                    false,
                    &mut receiver,
                    &mut pending_stdin,
                )?;
                return Ok(1);
            }
        }
    }
}

#[cfg(test)]
fn prepared_error(commands: &[PreparedCommand]) -> Option<&ServerError> {
    commands.iter().find_map(|command| match &command.result {
        PreparedCommandResult::Ready => None,
        PreparedCommandResult::Error(error) => Some(error),
    })
}

#[cfg(test)]
struct PendingExpansion {
    variables: bool,
    request_id: u64,
}

#[cfg(test)]
impl PendingExpansion {
    fn answers(&self, message: &ProtocolMessage) -> bool {
        match message {
            ProtocolMessage::HomeDirectoryResponse { request_id, .. } => {
                !self.variables && *request_id == self.request_id
            }
            ProtocolMessage::EnvironmentResponse { request_id, .. } => {
                self.variables && *request_id == self.request_id
            }
            _ => false,
        }
    }
}

#[cfg(test)]
fn expansion_answer(message: ProtocolMessage) -> Vec<Option<String>> {
    match message {
        ProtocolMessage::HomeDirectoryResponse { homes, .. } => homes,
        ProtocolMessage::EnvironmentResponse { values, .. } => values,
        _ => Vec::new(),
    }
}

#[cfg(test)]
fn match_prepared_response(
    message: ProtocolMessage,
    request_id: u64,
) -> Result<Vec<PreparedCommand>, ProtocolMessage> {
    match message {
        ProtocolMessage::PreparedCommandList {
            request_id: response_id,
            commands,
        } if response_id == request_id => Ok(commands),
        message => Err(message),
    }
}

fn execute_command_unit<W: Write>(
    client: &InteractiveClient,
    receiver: &mut ControlReceiver,
    output: &mut ControlWriter<W>,
    commands: Vec<CommandInvocation>,
    raw_line: Option<String>,
    flags: u8,
    state: &mut ControlState,
    pending_stdin: &mut VecDeque<StdinEvent>,
    mut deferred_return: Option<PendingReturn>,
) -> io::Result<CommandResult> {
    let mut first_is_detach = commands
        .first()
        .is_some_and(|command| zz_protocol::canonical_command(&command.name) == "detach-client");
    if !first_is_detach && state.pending_return.is_none() {
        state.pending_return = deferred_return.take();
    }
    output.hold_exit();
    let names: Vec<_> = commands
        .iter()
        .map(|command| command.name.clone())
        .collect();
    if let Some(line) = raw_line {
        if state.submitted_lines == 0 {
            client
                .execute_control_line(line)
                .map_err(io::Error::other)?;
        } else {
            state.submitted_lines -= 1;
        }
        if state.attached_session.is_some() {
            submit_pending_control_lines(
                pending_stdin,
                &mut state.submitted_lines,
                state
                    .pending_return
                    .as_ref()
                    .map(PendingReturn::preceding_input),
                |line| {
                    client
                        .execute_control_line(line.to_owned())
                        .map_err(io::Error::other)
                },
            )?;
        }
    } else {
        client.execute_chain(commands).map_err(io::Error::other)?;
    }
    let mut result = CommandResult {
        exit_code: 0,
        exit: ExitSignal::None,
    };
    let mut parked = false;
    let mut completed_guards = output.command_guard_frames;
    let mut started_command: Option<StartedCommand> = None;
    loop {
        if state.tree_sync_required {
            client.request_tree_sync().map_err(io::Error::other)?;
            state.tree_sync_required = false;
        }
        match receive_buffered_control_event(receiver, output)? {
            MainEvent::Protocol(message) => match *message {
                ProtocolMessage::Event(zz_protocol::Event {
                    payload:
                        EventPayload::ControlCommandStarted {
                            request_id,
                            flags,
                            canonical_name,
                            guard,
                        },
                    ..
                }) => {
                    let flags = u8::try_from(flags).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid control command flags")
                    })?;
                    if request_id == 1 && canonical_name.as_deref() == Some("detach-client") {
                        first_is_detach = true;
                        if deferred_return.is_none() {
                            deferred_return = state.pending_return.take();
                        }
                    }
                    parked = false;
                    started_command = Some(StartedCommand {
                        request_id,
                        flags,
                        canonical_name,
                        command_guard_frames: output.command_guard_frames,
                        frame: if guard {
                            Some(output.begin_buffered_at(unix_timestamp(), flags)?)
                        } else {
                            None
                        },
                    });
                }
                ProtocolMessage::CommandQueueParked { .. } => {
                    parked = true;
                    if release_parked_queue_at_client_exit(state, pending_stdin) {
                        if let Some(frame) =
                            started_command.as_ref().and_then(|started| started.frame)
                        {
                            output.end(&frame, false)?;
                        } else if output.command_guard_frames == completed_guards {
                            output.control_command_guard("", false, flags)?;
                        }
                        output.release_exit()?;
                        return Ok(result);
                    }
                }
                ProtocolMessage::CommandResponse(response) => {
                    let request_id = response_request_id(&response);
                    let index = request_id.saturating_sub(1) as usize;
                    let started = if started_command
                        .as_ref()
                        .is_some_and(|started| started.request_id == request_id)
                    {
                        started_command.take()
                    } else {
                        None
                    };
                    let name = started
                        .as_ref()
                        .and_then(|started| started.canonical_name.as_deref())
                        .or_else(|| {
                            names
                                .get(index)
                                .map(|name| zz_protocol::canonical_command(name))
                        });
                    let updates_return_code = response_sets_return_code(name, &response);
                    let new_failure = updates_return_code && state.return_code == 0;
                    if updates_return_code {
                        state.return_code = 1;
                    }
                    settle_deferred_return(
                        result.exit == ExitSignal::Detached,
                        &mut deferred_return,
                        state,
                    );
                    if let Some(pending_return) = state.pending_return.as_mut() {
                        if (new_failure && !response_is_post_admission_callback_failure(&response))
                            || (updates_return_code && name == Some("source-file"))
                        {
                            pending_return.observe_preceding_input();
                        }
                        pending_return.refresh_code_after_preceding_input(state.return_code);
                    }
                    result.exit_code = if let Some(started) = started {
                        render_command_response(
                            output,
                            started.frame.as_ref(),
                            started.flags,
                            started.command_guard_frames,
                            response,
                        )?
                    } else {
                        response_exit_code(&response)
                    };
                    completed_guards = output.command_guard_frames;
                }
                ProtocolMessage::ExecExit(finished) => {
                    match finished.outcome {
                        ExecOutcome::Ran => {}
                        ExecOutcome::Rejected(error) => {
                            if flags == 0 {
                                output.write_line(&error.tmux_message())?;
                            } else {
                                let message = error.tmux_message();
                                output.parse_error(&if message.starts_with("parse error: ") {
                                    message
                                } else {
                                    format!("parse error: {message}")
                                })?;
                            }
                            result.exit_code = 1;
                            settle_deferred_return(false, &mut deferred_return, state);
                        }
                        ExecOutcome::Resume(_) | ExecOutcome::ServerMismatch => {
                            result.exit_code = 1;
                            result.exit = ExitSignal::Unexpected;
                        }
                    }
                    output.release_exit()?;
                    return Ok(result);
                }
                message => {
                    let signal = handle_protocol(message, state, output)?;
                    if signal.is_some() {
                        result.exit = signal;
                    }
                }
            },
            MainEvent::Stdin(stdin) => {
                if let StdinEvent::Line(line) = &stdin
                    && state.attached_session.is_some()
                    && state.pending_return.is_none()
                    && deferred_return.is_none()
                    && pending_stdin.len() == state.submitted_lines
                    && state.submitted_lines < 32
                    && !line.trim().is_empty()
                    && !line.trim_start().starts_with('#')
                {
                    client
                        .execute_control_line(line.clone())
                        .map_err(io::Error::other)?;
                    state.submitted_lines = state.submitted_lines.saturating_add(1);
                }
                capture_pending_return(
                    stdin,
                    state.return_code,
                    if first_is_detach {
                        &mut deferred_return
                    } else {
                        &mut state.pending_return
                    },
                    pending_stdin,
                    output,
                );
                if parked && release_parked_queue_at_client_exit(state, pending_stdin) {
                    if let Some(frame) = started_command.as_ref().and_then(|started| started.frame)
                    {
                        output.end(&frame, false)?;
                    } else if output.command_guard_frames == completed_guards {
                        output.control_command_guard("", false, flags)?;
                    }
                    output.release_exit()?;
                    return Ok(result);
                }
            }
            MainEvent::Disconnected => {
                if let Some(started) = started_command {
                    if result.exit == ExitSignal::Clean {
                        if let Some(frame) = started.frame {
                            output.end(&frame, false)?;
                        }
                    } else {
                        render_command_failure(
                            output,
                            started.frame.as_ref(),
                            started.flags,
                            started.command_guard_frames,
                            "server exited unexpectedly",
                        )?;
                    }
                }
                output.release_exit()?;
                if !result.exit.is_some() {
                    result.exit = ExitSignal::Unexpected;
                    result.exit_code = 1;
                }
                return Ok(result);
            }
        }
    }
}

fn submit_pending_control_lines(
    pending: &VecDeque<StdinEvent>,
    submitted: &mut usize,
    preceding_input: Option<usize>,
    mut submit: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    for stdin in pending
        .iter()
        .take(preceding_input.unwrap_or(usize::MAX))
        .skip(*submitted)
        .take(32usize.saturating_sub(*submitted))
    {
        let StdinEvent::Line(line) = stdin else {
            break;
        };
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            break;
        }
        submit(line)?;
        *submitted += 1;
    }
    Ok(())
}

#[derive(Default)]
struct ControlState {
    attached_session: Option<SessionId>,
    snapshot: MuxSnapshot,
    tree: MuxSnapshot,
    view: Option<zz_protocol::ClientView>,
    tree_sync_required: bool,
    last_windows: BTreeMap<SessionId, WindowId>,
    self_name: Option<String>,
    wait_exit: bool,
    new_layouts: bool,
    return_code: u8,
    pending_return: Option<PendingReturn>,
    parked_queue_released: bool,
    submitted_lines: usize,
}

impl ControlState {
    fn attach(&mut self, session: SessionId, snapshot: MuxSnapshot) {
        self.tree = snapshot.clone();
        self.attached_session = Some(session);
        self.adopt_snapshot(snapshot);
    }

    fn adopt_snapshot(&mut self, snapshot: MuxSnapshot) {
        for session in &snapshot.sessions {
            if let Some(previous) = self
                .snapshot
                .sessions
                .iter()
                .find(|previous| previous.id == session.id)
                && previous.active_window != session.active_window
            {
                self.last_windows.insert(session.id, previous.active_window);
            }
        }
        self.snapshot = snapshot;
        self.self_name = self
            .attached_session
            .and_then(|attached| {
                self.snapshot
                    .sessions
                    .iter()
                    .find(|session| session.id == attached)
            })
            .and_then(|session| session.viewers.iter().find(|viewer| viewer.is_self))
            .map(|viewer| viewer.name.clone());
    }

    fn window(
        &self,
        id: &str,
    ) -> Option<(&zz_protocol::SessionSnapshot, &zz_protocol::WindowSnapshot)> {
        let attached = self.attached_session.and_then(|attached| {
            self.snapshot
                .sessions
                .iter()
                .find(|session| session.id == attached)
        });
        attached
            .and_then(|session| {
                session
                    .windows
                    .iter()
                    .find(|window| window.id.to_string() == id)
                    .map(|window| (session, window))
            })
            .or_else(|| {
                self.snapshot.sessions.iter().find_map(|session| {
                    session
                        .windows
                        .iter()
                        .find(|window| window.id.to_string() == id)
                        .map(|window| (session, window))
                })
            })
    }

    fn mine(&self, variables: &BTreeMap<String, String>) -> bool {
        self.attached_session.is_some_and(|attached| {
            self.snapshot.sessions.iter().any(|session| {
                session.id == attached
                    && session
                        .windows
                        .iter()
                        .any(|window| variables.get("hook_window") == Some(&window.id.to_string()))
            })
        })
    }

    fn raw_window_flags(
        &self,
        session: &zz_protocol::SessionSnapshot,
        window: &zz_protocol::WindowSnapshot,
    ) -> String {
        let mut flags = String::new();
        if window.activity {
            flags.push('#');
        }
        if window.panes.values().any(|pane| pane.bell) {
            flags.push('!');
        }
        if session.active_window == window.id {
            flags.push('*');
        }
        if self.last_windows.get(&session.id) == Some(&window.id) {
            flags.push('-');
        }
        if window.zoomed_pane.is_some() {
            flags.push('Z');
        }
        if window.silence {
            flags.push('~');
        }
        flags
    }
}

fn handle_protocol<W: Write>(
    message: ProtocolMessage,
    state: &mut ControlState,
    output: &mut ControlWriter<W>,
) -> io::Result<ExitSignal> {
    match message {
        ProtocolMessage::Attached {
            session, snapshot, ..
        } => state.attach(session, snapshot),
        ProtocolMessage::Event(event) => match event.payload {
            EventPayload::Snapshot(snapshot) => {
                state.tree = snapshot.clone();
                state.adopt_snapshot(snapshot);
                state.tree_sync_required = false;
            }
            EventPayload::TreeDelta(delta) => {
                if delta.apply(&mut state.tree).is_err() {
                    state.tree_sync_required = true;
                } else {
                    let mut snapshot = state.tree.clone();
                    if let Some(view) = &state.view {
                        let _ = view.apply(&mut snapshot);
                    }
                    state.adopt_snapshot(snapshot);
                }
            }
            EventPayload::ClientView(view) => {
                let mut snapshot = state.tree.clone();
                if view.apply(&mut snapshot).is_err() {
                    state.tree_sync_required = true;
                } else {
                    state.attached_session = view.session;
                    state.adopt_snapshot(snapshot);
                    state.view = Some(view);
                }
            }
            EventPayload::HookEvent { name, variables } => {
                if state.attached_session.is_some()
                    && (name != "window-layout-changed" || !output.exit_draining)
                    && let Some(line) = render_hook(state, &name, &variables)
                {
                    output.notify(line.as_bytes())?;
                }
            }
            EventPayload::PaneOutput { pane, bytes } => {
                output.pane_output(&render_pane_output(pane, &bytes))?;
            }
            EventPayload::PaneOutputState { pane, paused } => {
                output.notify(
                    format!("%{} {pane}", if paused { "pause" } else { "continue" }).as_bytes(),
                )?;
            }
            EventPayload::PaneOutputAged {
                pane,
                age_ms,
                bytes,
            } => {
                output.pane_output(&render_pane_output_aged(pane, age_ms, &bytes))?;
            }
            EventPayload::ControlFlags {
                wait_exit,
                new_layouts,
                ..
            } => {
                state.wait_exit = wait_exit;
                state.new_layouts = new_layouts;
            }
            EventPayload::SubscriptionChanged {
                name,
                session,
                window,
                window_index,
                pane,
                value,
            } => {
                output.notify(
                    render_subscription_changed(&name, session, window, window_index, pane, &value)
                        .as_bytes(),
                )?;
            }
            EventPayload::TimedClientMessage { text, .. } => {
                let mut line = b"%message ".to_vec();
                line.extend(render_message(&text));
                output.notify(&line)?;
            }
            EventPayload::ControlCommandGuard {
                output: text,
                error,
                sticky_failure,
                flags,
            } => {
                if sticky_failure || (error && is_source_error_message(&text)) {
                    state.return_code = 1;
                }
                output.control_command_guard_bytes(text.as_bytes(), error, flags)?;
            }
            EventPayload::ControlCommandGuardRaw {
                output: text,
                error,
                sticky_failure,
                flags,
            } => {
                if sticky_failure || (error && is_source_error_message(&text)) {
                    state.return_code = 1;
                }
                output.control_command_guard_bytes(text.as_bytes(), error, flags)?;
            }
            EventPayload::ControlSourceFile { event } => {
                if matches!(event, ControlSourceFileEvent::ReadError(_)) {
                    state.return_code = 1;
                }
                output.control_source_file(event)?;
            }
            EventPayload::ControlCommandOutput { output: text } => {
                output.control_command_output(&text)?;
            }
            EventPayload::StartupConfigCauses { causes } => {
                output.startup_config_causes(&causes)?;
            }
            EventPayload::ClientMessage {
                kind: zz_protocol::ClientMessageKind::Error,
                text,
                ..
            } => {
                if is_source_error_message(&text) {
                    state.return_code = 1;
                }
                output.diagnostic_error(&text)?;
            }
            EventPayload::ClientMessage { kind, text, .. }
                if kind == zz_protocol::ClientMessageKind::Warning
                    && is_source_error_message(&text) =>
            {
                state.return_code = 1;
                output.diagnostic_error(&text)?;
            }
            EventPayload::ControlConfigError { text } => {
                output.notify(format!("%config-error {text}").as_bytes())?;
            }
            EventPayload::Detached { .. } => {
                state.attached_session = None;
                return Ok(ExitSignal::Detached);
            }
            EventPayload::ServerStopping => {
                output.emit_exit(None)?;
                return Ok(ExitSignal::Clean);
            }
            EventPayload::ControlExit { reason } if reason.is_empty() => {
                output.release_exit()?;
                output.emit_exit(None)?;
                return Ok(ExitSignal::Clean);
            }
            EventPayload::ControlExit { reason } if reason == "too far behind" => {
                return Ok(ExitSignal::TooFarBehind);
            }
            _ => {}
        },
        _ => {}
    }
    Ok(ExitSignal::None)
}

fn render_hook(
    state: &ControlState,
    name: &str,
    variables: &BTreeMap<String, String>,
) -> Option<String> {
    let value = |key| variables.get(key).map(String::as_str);
    match name {
        "window-linked" => Some(format!(
            "{} {}",
            if state.mine(variables) {
                "%window-add"
            } else {
                "%unlinked-window-add"
            },
            value("hook_window")?
        )),
        "window-unlinked" => Some(format!(
            "{} {}",
            if state.mine(variables) {
                "%window-close"
            } else {
                "%unlinked-window-close"
            },
            value("hook_window")?
        )),
        "window-renamed" => Some(format!(
            "{} {} {}",
            if state.mine(variables) {
                "%window-renamed"
            } else {
                "%unlinked-window-renamed"
            },
            value("hook_window")?,
            value("hook_window_name")?
        )),
        "window-pane-changed" => Some(format!(
            "%window-pane-changed {} {}",
            value("hook_window")?,
            value("hook_pane")?
        )),
        "window-layout-changed" => {
            if !state.mine(variables) {
                return None;
            }
            let window_id = value("hook_window")?;
            let (session, window) = state.window(window_id)?;
            let layout = |dump: &str| {
                if state.new_layouts {
                    dump.to_owned()
                } else {
                    zz_mux::legacy_layout(dump)
                }
            };
            Some(format!(
                "%layout-change {window_id} {} {} {}",
                layout(&window.layout_dump),
                layout(&window.visible_layout_dump),
                state.raw_window_flags(session, window)
            ))
        }
        "session-created" | "session-closed" => Some("%sessions-changed".to_owned()),
        "session-renamed" => Some(format!(
            "%session-renamed {} {}",
            value("hook_session")?,
            value("hook_session_name")?
        )),
        "session-window-changed" => Some(format!(
            "%session-window-changed {} {}",
            value("hook_session")?,
            value("hook_window")?
        )),
        "client-session-changed" => {
            let client = value("hook_client")?;
            if state.self_name.as_deref() == Some(client) {
                Some(format!(
                    "%session-changed {} {}",
                    value("hook_session")?,
                    value("hook_session_name")?
                ))
            } else {
                Some(format!(
                    "%client-session-changed {client} {} {}",
                    value("hook_session")?,
                    value("hook_session_name")?
                ))
            }
        }
        "client-detached" => Some(format!("%client-detached {}", value("hook_client")?)),
        "pane-mode-changed" => Some(format!("%pane-mode-changed {}", value("hook_pane")?)),
        "paste-buffer-changed" => Some(format!(
            "%paste-buffer-changed {}",
            value("hook_paste_buffer")?
        )),
        "paste-buffer-deleted" => Some(format!(
            "%paste-buffer-deleted {}",
            value("hook_paste_buffer")?
        )),
        _ => None,
    }
}

fn render_subscription_changed(
    name: &str,
    session: SessionId,
    window: Option<WindowId>,
    window_index: Option<u32>,
    pane: Option<zz_protocol::PaneId>,
    value: &str,
) -> String {
    match (window, window_index, pane) {
        (Some(window), Some(index), Some(pane)) => {
            format!("%subscription-changed {name} {session} {window} {index} {pane} : {value}")
        }
        (Some(window), Some(index), None) => {
            format!("%subscription-changed {name} {session} {window} {index} - : {value}")
        }
        _ => format!("%subscription-changed {name} {session} - - - : {value}"),
    }
}

fn render_pane_output(pane: zz_protocol::PaneId, bytes: &[u8]) -> Vec<u8> {
    let mut line = format!("%output {pane} ").into_bytes();
    append_output_bytes(&mut line, bytes);
    line
}

fn render_pane_output_aged(pane: zz_protocol::PaneId, age_ms: u64, bytes: &[u8]) -> Vec<u8> {
    let mut line = format!("%extended-output {pane} {age_ms} : ").into_bytes();
    append_output_bytes(&mut line, bytes);
    line
}

fn append_output_bytes(line: &mut Vec<u8>, bytes: &[u8]) {
    for byte in bytes {
        if *byte < 0x20 || *byte == b'\\' {
            line.extend([
                b'\\',
                b'0' + (byte >> 6),
                b'0' + ((byte >> 3) & 7),
                b'0' + (byte & 7),
            ]);
        } else {
            line.push(*byte);
        }
    }
}

fn render_message(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut rendered = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            0 => {
                rendered.extend_from_slice(b"\\0");
                if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) && bytes[index + 1] < b'8' {
                    rendered.extend_from_slice(b"00");
                }
            }
            b'\r' => rendered.extend_from_slice(b"\\r"),
            8 => rendered.extend_from_slice(b"\\b"),
            7 => rendered.extend_from_slice(b"\\a"),
            11 => rendered.extend_from_slice(b"\\v"),
            12 => rendered.extend_from_slice(b"\\f"),
            b'\t' | b'\n' | b' ' | b'!'..=b'~' | 0x80..=0xff => rendered.push(byte),
            _ => rendered.extend([
                b'\\',
                b'0' + (byte >> 6),
                b'0' + ((byte >> 3) & 7),
                b'0' + (byte & 7),
            ]),
        }
    }
    rendered
}

fn is_source_error_message(text: &str) -> bool {
    let mut lines = text.lines();
    lines.next().is_some_and(is_source_error_line) && lines.all(is_source_error_line)
}

fn is_source_error_line(text: &str) -> bool {
    text.starts_with("No such file or directory: ")
        || text.starts_with("Invalid argument: ")
        || text.starts_with("Cannot allocate memory: ")
        || text.starts_with("Pattern syntax error")
        || text == "too many nested files"
        || text
            .strip_prefix("stream did not contain valid UTF-8: ")
            .is_some_and(|path| Path::new(path).is_absolute())
        || text.split_once("): ").is_some_and(|(error, path)| {
            error
                .rsplit_once(" (os error ")
                .is_some_and(|(_, code)| code.parse::<i32>().is_ok())
                && Path::new(path).is_absolute()
        })
}

#[cfg(test)]
fn response_aborts_line(response: &CommandResponse) -> bool {
    matches!(response, CommandResponse::Error { .. })
}

fn response_sets_return_code(canonical_name: Option<&str>, response: &CommandResponse) -> bool {
    match response {
        CommandResponse::Error { error, .. } => !error.is_command_parse(),
        CommandResponse::Success { exit_code, .. } => {
            canonical_name == Some("source-file") && *exit_code != 0
        }
    }
}

fn response_is_post_admission_callback_failure(response: &CommandResponse) -> bool {
    matches!(
        response,
        CommandResponse::Error { error, .. } if error.is_post_admission_callback()
    )
}

fn response_request_id(response: &CommandResponse) -> u64 {
    match response {
        CommandResponse::Success { request_id, .. } | CommandResponse::Error { request_id, .. } => {
            *request_id
        }
    }
}

fn response_exit_code(response: &CommandResponse) -> u8 {
    match response {
        CommandResponse::Success { exit_code, .. } => *exit_code,
        CommandResponse::Error { .. } => 1,
    }
}

fn render_command_response<W: Write>(
    output: &mut ControlWriter<W>,
    frame: Option<&Frame>,
    flags: u8,
    command_guard_frames: u64,
    response: CommandResponse,
) -> io::Result<u8> {
    if let Some(frame) = frame {
        return output.response(frame, response);
    }
    let exit_code = response_exit_code(&response);
    if matches!(response, CommandResponse::Error { .. })
        && output.command_guard_frames == command_guard_frames
    {
        let frame = output.begin(flags)?;
        output.response(&frame, response)
    } else {
        Ok(exit_code)
    }
}

fn render_command_failure<W: Write>(
    output: &mut ControlWriter<W>,
    frame: Option<&Frame>,
    flags: u8,
    command_guard_frames: u64,
    error: &str,
) -> io::Result<()> {
    if let Some(frame) = frame {
        return output.error(frame, error);
    }
    if output.command_guard_frames == command_guard_frames {
        let frame = output.begin(flags)?;
        output.error(&frame, error)?;
    }
    Ok(())
}

fn completed_exit_code(command_exit_code: u8, state: &ControlState) -> u8 {
    if command_exit_code == 0 {
        state.return_code
    } else {
        command_exit_code
    }
}

fn capture_pending_return<W: Write>(
    stdin: StdinEvent,
    return_code: u8,
    pending_return: &mut Option<PendingReturn>,
    pending_stdin: &mut VecDeque<StdinEvent>,
    output: &mut ControlWriter<W>,
) {
    if pending_return.is_some() {
        pending_stdin.push_back(stdin);
        return;
    }
    match PendingReturn::from_stdin(stdin, return_code, pending_stdin.len()) {
        Ok(return_event) => {
            if return_event.discards_pane_output() {
                output.begin_exit_drain();
            }
            *pending_return = Some(return_event);
        }
        Err(stdin) => pending_stdin.push_back(stdin),
    }
}

fn release_parked_queue_at_client_exit(
    state: &mut ControlState,
    pending_stdin: &mut VecDeque<StdinEvent>,
) -> bool {
    let Some(
        PendingReturn::Eof {
            preceding_input, ..
        }
        | PendingReturn::Blank {
            preceding_input, ..
        },
    ) = state.pending_return.as_mut()
    else {
        return false;
    };
    *preceding_input = 0;
    pending_stdin.clear();
    state.submitted_lines = 0;
    state.parked_queue_released = true;
    true
}

fn take_ready_pending_return(pending_return: &mut Option<PendingReturn>) -> Option<PendingReturn> {
    if pending_return
        .as_ref()
        .is_some_and(|pending_return| !pending_return.has_preceding_input())
    {
        pending_return.take()
    } else {
        None
    }
}

#[cfg(test)]
fn settle_preparation_error_return(
    prepared_return: &mut Option<PendingReturn>,
    state_return: &mut Option<PendingReturn>,
) -> Option<PendingReturn> {
    if state_return.is_none() {
        *state_return = prepared_return.take();
    }
    take_ready_pending_return(state_return)
}

fn settle_deferred_return(
    caller_detached: bool,
    deferred_return: &mut Option<PendingReturn>,
    state: &mut ControlState,
) {
    if caller_detached {
        deferred_return.take();
    } else if state.pending_return.is_none() {
        state.pending_return = deferred_return.take();
    }
}

fn finish_control_return<W: Write + ControlClose>(
    client: &InteractiveClient,
    pending_return: PendingReturn,
    output: &mut ControlWriter<W>,
    state: &mut ControlState,
    receiver: &mut ControlReceiver,
    pending_stdin: &mut VecDeque<StdinEvent>,
) -> io::Result<u8> {
    if pending_return.discards_pane_output() {
        output.begin_exit_drain();
    }
    let code = pending_return.code();
    let (input_closed, input_error) = match pending_return {
        PendingReturn::Blank { .. } => (false, None),
        PendingReturn::Eof { .. } => (true, None),
        PendingReturn::InputError { message, .. } => (true, Some(message)),
    };
    if let Some(error) = input_error.as_deref() {
        output.output.close_sink();
        eprintln!("zz: {error}");
    }
    let _ = client.detach();
    if input_error.is_none() && !state.parked_queue_released {
        drain_before_exit(receiver, state, output, pending_stdin)?;
    }
    finish_exit(
        output,
        None,
        state.wait_exit,
        input_closed,
        receiver,
        pending_stdin,
    )?;
    Ok(code)
}

fn drain_before_exit<W: Write>(
    receiver: &mut ControlReceiver,
    state: &mut ControlState,
    output: &mut ControlWriter<W>,
    pending_stdin: &mut VecDeque<StdinEvent>,
) -> io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return Ok(());
        };
        if TERMINATION_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        match receiver.receive(Some(remaining.min(std::time::Duration::from_millis(20))))? {
            Some(MainEvent::Protocol(message)) => {
                if handle_protocol(*message, state, output)?.is_some() {
                    return Ok(());
                }
            }
            Some(MainEvent::Stdin(input)) => pending_stdin.push_back(input),
            Some(MainEvent::Disconnected) => {
                return Ok(());
            }
            None => {}
        }
    }
}

#[cfg(not(unix))]
fn spawn_protocol_reader(client: Arc<InteractiveClient>, events: mpsc::SyncSender<MainEvent>) {
    let _ = thread::Builder::new()
        .name("zz-control-protocol".to_owned())
        .spawn(move || {
            loop {
                if let Ok(message) = client.recv() {
                    if forward_protocol_message(message, &events).is_err() {
                        break;
                    }
                } else {
                    let _ = events.send(MainEvent::Disconnected);
                    break;
                }
            }
        });
}

#[cfg(any(not(unix), test))]
fn forward_protocol_message(
    message: ProtocolMessage,
    events: &mpsc::SyncSender<MainEvent>,
) -> Result<(), ()> {
    events
        .send(MainEvent::Protocol(Box::new(message)))
        .map_err(|_| ())
}

#[cfg(not(unix))]
fn spawn_stdin_reader(events: mpsc::SyncSender<MainEvent>) {
    let _ = thread::Builder::new()
        .name("zz-control-stdin".to_owned())
        .spawn(move || {
            let mut stdin = io::stdin().lock();
            loop {
                let mut bytes = Vec::new();
                match stdin.read_until(b'\n', &mut bytes) {
                    Ok(0) => {
                        let _ = events.send(MainEvent::Stdin(StdinEvent::Eof));
                        break;
                    }
                    Ok(_) => {
                        if bytes.last() == Some(&b'\n') {
                            bytes.pop();
                        }
                        let line = match String::from_utf8(bytes) {
                            Ok(line) => line,
                            Err(error) => {
                                let _ = events
                                    .send(MainEvent::Stdin(StdinEvent::Error(error.to_string())));
                                break;
                            }
                        };
                        if events
                            .send(MainEvent::Stdin(StdinEvent::Line(line)))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        let _ = events.send(MainEvent::Stdin(StdinEvent::Error(error.to_string())));
                        break;
                    }
                }
            }
        });
}

fn finish_exit<W: Write>(
    output: &mut ControlWriter<W>,
    reason: Option<&str>,
    wait_exit: bool,
    input_closed: bool,
    receiver: &mut ControlReceiver,
    pending_stdin: &mut VecDeque<StdinEvent>,
) -> io::Result<()> {
    output.emit_exit(reason)?;
    if wait_exit && !input_closed {
        wait_for_exit_input(receiver, pending_stdin);
    }
    output.finish()
}

fn wait_for_exit_input(receiver: &mut ControlReceiver, pending_stdin: &mut VecDeque<StdinEvent>) {
    loop {
        let event = pending_stdin
            .pop_front()
            .map(MainEvent::Stdin)
            .or_else(|| receive_control_event(receiver).ok());
        match event {
            Some(MainEvent::Stdin(StdinEvent::Line(line))) if line.is_empty() => return,
            Some(MainEvent::Stdin(StdinEvent::Eof | StdinEvent::Error(_))) | None => return,
            Some(
                MainEvent::Stdin(StdinEvent::Line(_))
                | MainEvent::Protocol(_)
                | MainEvent::Disconnected,
            ) => {}
        }
    }
}

#[cfg(test)]
fn parse_line(
    line: &str,
    homes: &BTreeMap<String, String>,
    variables: &BTreeMap<String, String>,
) -> ParsedLine {
    if line.is_empty() {
        return ParsedLine::Return;
    }
    let parsed = zz_mux::parse_config_with_expansions(CONTROL_PARSE_SOURCE, line, homes, variables);
    if let Some(diagnostic) = parsed.diagnostics.first() {
        return ParsedLine::Error(format!("parse error: {}", diagnostic.message));
    }
    if parsed.commands.is_empty() {
        ParsedLine::Ignore
    } else {
        ParsedLine::Commands(parsed.commands)
    }
}

#[cfg(unix)]
fn stdio_passable() -> bool {
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
    let Ok(stdin) = io::stdin().as_fd().try_clone_to_owned() else {
        return false;
    };
    std::fs::File::from(stdin).metadata().is_ok_and(|metadata| {
        let kind = metadata.file_type();
        kind.is_socket() || (kind.is_fifo() && (cfg!(target_os = "linux") || metadata.nlink() == 0))
    })
}

#[cfg(unix)]
struct StdioFlags([Option<rustix::fs::OFlags>; 3]);

#[cfg(unix)]
impl StdioFlags {
    fn capture() -> Self {
        Self([
            rustix::fs::fcntl_getfl(io::stdin()).ok(),
            rustix::fs::fcntl_getfl(io::stdout()).ok(),
            rustix::fs::fcntl_getfl(io::stderr()).ok(),
        ])
    }

    fn restore(&self) {
        let [stdin, stdout, stderr] = self.0;
        if let Some(flags) = stdin {
            let _ = rustix::fs::fcntl_setfl(io::stdin(), flags);
        }
        if let Some(flags) = stdout {
            let _ = rustix::fs::fcntl_setfl(io::stdout(), flags);
        }
        if let Some(flags) = stderr {
            let _ = rustix::fs::fcntl_setfl(io::stderr(), flags);
        }
    }
}

#[cfg(unix)]
fn start_stdio(
    client: &InteractiveClient,
) -> io::Result<(Option<StdioFlags>, VecDeque<ProtocolMessage>)> {
    let mut stash = VecDeque::new();
    if std::env::var_os("ZZ_CONTROL_RELAY").is_some_and(|value| value == "1")
        || !client
            .server_hello()
            .capabilities
            .iter()
            .any(|capability| capability == zz_protocol::CONTROL_STDIO_CAPABILITY)
        || !stdio_passable()
    {
        return Ok((None, stash));
    }
    let flags = StdioFlags::capture();
    let frame = zz_protocol::encode_protocol_message(&ProtocolMessage::ControlStdio)
        .map_err(io::Error::other)?;
    let socket = client.receive_fd()?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let fds = [stdin.as_fd(), stdout.as_fd()];
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut space);
    ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&fds));
    let mut sent = rustix::net::sendmsg(
        &socket,
        &[io::IoSlice::new(&frame)],
        &mut ancillary,
        rustix::net::SendFlags::empty(),
    )?;
    while sent < frame.len() {
        sent += rustix::io::write(&socket, &frame[sent..])?;
    }
    loop {
        match client.recv().map_err(io::Error::other)? {
            ProtocolMessage::ControlStdioSync { next_number: 0 } => {
                return Ok((Some(flags), stash));
            }
            ProtocolMessage::ControlStdioClosed => return Ok((None, stash)),
            ProtocolMessage::Batch(batch) => stash.extend(
                batch
                    .messages()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            ),
            message => stash.push_back(message),
        }
    }
}

enum ControlOutput {
    Stdout(io::BufWriter<io::StdoutLock<'static>>),
    #[cfg(unix)]
    Daemon {
        client: Arc<InteractiveClient>,
        buffer: Vec<u8>,
        reported: Option<(u64, u64)>,
        direct: Option<io::BufWriter<io::StdoutLock<'static>>>,
        flags: StdioFlags,
    },
}

impl ControlOutput {
    #[cfg(unix)]
    fn daemon(client: Arc<InteractiveClient>, flags: StdioFlags) -> Self {
        Self::Daemon {
            client,
            buffer: Vec::new(),
            reported: None,
            direct: None,
            flags,
        }
    }

    #[cfg(not(unix))]
    fn daemon(_client: Arc<InteractiveClient>, _flags: ()) -> Self {
        Self::Stdout(io::BufWriter::new(io::stdout().lock()))
    }

    #[cfg(unix)]
    fn send(&mut self, idle: Option<(u64, u64)>, close: bool) -> io::Result<()> {
        let Self::Daemon {
            client,
            buffer,
            direct,
            flags,
            ..
        } = self
        else {
            return Ok(());
        };
        if let Some(direct) = direct {
            direct.write_all(buffer)?;
            buffer.clear();
            return direct.flush();
        }
        let message = ProtocolMessage::ControlWrite {
            bytes: std::mem::take(buffer),
            idle,
            close,
        };
        if client.send(&message).is_err() {
            let ProtocolMessage::ControlWrite { bytes, .. } = message else {
                unreachable!();
            };
            flags.restore();
            let mut stdout = io::BufWriter::new(io::stdout().lock());
            stdout.write_all(&bytes)?;
            stdout.flush()?;
            *direct = Some(stdout);
        }
        Ok(())
    }

    #[cfg(unix)]
    fn report_idle(&mut self, received: u64, next_number: u64) {
        let Self::Daemon {
            buffer, reported, ..
        } = self
        else {
            return;
        };
        if *reported == Some((received, next_number)) && buffer.is_empty() {
            return;
        }
        *reported = Some((received, next_number));
        let _ = self.send(Some((received, next_number)), false);
    }

    #[cfg(unix)]
    fn close(&mut self) {
        let Self::Daemon { client, direct, .. } = self else {
            return;
        };
        if direct.is_some() {
            let _ = self.flush();
            return;
        }
        let client = Arc::clone(client);
        if self.send(None, true).is_ok()
            && let Self::Daemon { direct: None, .. } = self
            && let Ok(socket) = client.receive_fd()
        {
            loop {
                match client.try_recv() {
                    Ok(Some(message))
                        if matches!(*message, ProtocolMessage::ControlStdioClosed) =>
                    {
                        break;
                    }
                    Ok(Some(_)) => {}
                    Err(_) => break,
                    Ok(None) => {
                        let mut poll = [rustix::event::PollFd::new(
                            &socket,
                            rustix::event::PollFlags::IN,
                        )];
                        let _ = rustix::event::poll(&mut poll, None);
                    }
                }
            }
        }
        if let Self::Daemon { direct, flags, .. } = self
            && direct.is_none()
        {
            flags.restore();
            *direct = Some(io::BufWriter::new(io::stdout().lock()));
        }
    }

    #[cfg(not(unix))]
    fn close(&mut self) {}
}

impl Write for ControlOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(stdout) => stdout.write(bytes),
            #[cfg(unix)]
            Self::Daemon {
                direct: Some(direct),
                ..
            } => direct.write(bytes),
            #[cfg(unix)]
            Self::Daemon { buffer, .. } => {
                buffer.extend_from_slice(bytes);
                Ok(bytes.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(stdout) => stdout.flush(),
            #[cfg(unix)]
            Self::Daemon { buffer, direct, .. } => {
                if direct.is_none() && buffer.is_empty() {
                    return Ok(());
                }
                self.send(None, false)
            }
        }
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

enum DeferredOutput {
    Notification(Vec<u8>),
    PaneOutput(Vec<u8>),
    Exit(Option<String>),
    DiagnosticError {
        time: u64,
        text: String,
    },
    ControlCommandGuard {
        time: u64,
        output: Vec<u8>,
        error: bool,
        flags: u8,
    },
    ControlSourceFile(ControlSourceFileEvent),
    ControlCommandOutput(String),
}

trait ControlClose {
    fn close_sink(&mut self);
}

impl ControlClose for ControlOutput {
    fn close_sink(&mut self) {
        self.close();
    }
}

struct ControlWriter<W: Write> {
    output: W,
    double: bool,
    next_number: u64,
    command_guard_frames: u64,
    block_open: bool,
    open_frame: Option<Frame>,
    deferred: VecDeque<DeferredOutput>,
    exit_draining: bool,
    exit_requested: bool,
    exit_held: bool,
    st_sent: bool,
}

impl<W: Write> ControlWriter<W> {
    fn new(output: W, double: bool) -> Self {
        Self {
            output,
            double,
            next_number: 1,
            command_guard_frames: 0,
            block_open: false,
            open_frame: None,
            deferred: VecDeque::new(),
            exit_draining: false,
            exit_requested: false,
            exit_held: false,
            st_sent: false,
        }
    }

    fn start(&mut self) -> io::Result<()> {
        if self.double {
            self.output.write_all(DCS)?;
            self.output.flush()?;
        }
        Ok(())
    }

    fn notify(&mut self, line: &[u8]) -> io::Result<()> {
        if self.block_open {
            self.deferred
                .push_back(DeferredOutput::Notification(line.to_vec()));
            return Ok(());
        }
        self.output.write_all(line)?;
        self.output.write_all(b"\n")?;
        self.output.flush()
    }

    fn pane_output(&mut self, line: &[u8]) -> io::Result<()> {
        if self.exit_draining {
            return Ok(());
        }
        if self.block_open {
            self.deferred
                .push_back(DeferredOutput::PaneOutput(line.to_vec()));
            return Ok(());
        }
        self.output.write_all(line)?;
        self.output.write_all(b"\n")?;
        self.output.flush()
    }

    fn begin_exit_drain(&mut self) {
        self.exit_draining = true;
        self.deferred
            .retain(|deferred| !matches!(deferred, DeferredOutput::PaneOutput(_)));
    }

    fn startup_config_causes(&mut self, causes: &[String]) -> io::Result<()> {
        for cause in causes {
            self.output.write_all(b"%config-error ")?;
            self.output.write_all(cause.as_bytes())?;
            self.output.write_all(b"\n")?;
        }
        self.output.flush()
    }

    fn flush_deferred(&mut self) -> io::Result<()> {
        let mut exit = None;
        while let Some(deferred) = self.deferred.pop_front() {
            match deferred {
                DeferredOutput::Notification(line) | DeferredOutput::PaneOutput(line) => {
                    self.output.write_all(&line)?;
                    self.output.write_all(b"\n")?;
                }
                DeferredOutput::Exit(reason) => exit = Some(reason),
                DeferredOutput::DiagnosticError { time, text } => {
                    let frame = self.allocate_frame(time, 1)?;
                    self.write_frame_begin(&frame)?;
                    self.write_line(&text)?;
                    self.write_frame_end(&frame, true)?;
                }
                DeferredOutput::ControlCommandGuard {
                    time,
                    output,
                    error,
                    flags,
                } => self.write_control_command_guard(time, &output, error, flags)?,
                DeferredOutput::ControlSourceFile(event) => {
                    self.write_control_source_file(event)?;
                }
                DeferredOutput::ControlCommandOutput(output) => self.payload(output.as_bytes())?,
            }
        }
        if let Some(reason) = exit {
            if self.exit_held {
                self.deferred.push_back(DeferredOutput::Exit(reason));
            } else {
                self.write_exit(reason.as_deref())?;
            }
        }
        Ok(())
    }

    fn hold_exit(&mut self) {
        self.exit_held = true;
    }

    fn release_exit(&mut self) -> io::Result<()> {
        self.exit_held = false;
        if !self.block_open {
            self.flush_deferred()?;
            self.output.flush()?;
        }
        Ok(())
    }

    fn diagnostic_error(&mut self, text: &str) -> io::Result<()> {
        self.diagnostic_error_at(unix_timestamp(), text)
    }

    fn diagnostic_error_at(&mut self, time: u64, text: &str) -> io::Result<()> {
        if self.block_open {
            self.deferred.push_back(DeferredOutput::DiagnosticError {
                time,
                text: text.to_owned(),
            });
            return Ok(());
        }
        let frame = self.allocate_frame(time, 1)?;
        self.write_frame_begin(&frame)?;
        self.write_line(text)?;
        self.write_frame_end(&frame, true)?;
        self.output.flush()
    }

    fn control_command_guard(&mut self, output: &str, error: bool, flags: u8) -> io::Result<()> {
        self.control_command_guard_at(unix_timestamp(), output, error, flags)
    }

    fn control_command_guard_at(
        &mut self,
        time: u64,
        output: &str,
        error: bool,
        flags: u8,
    ) -> io::Result<()> {
        self.control_command_guard_bytes_at(time, output.as_bytes(), error, flags)
    }

    fn control_command_guard_bytes(
        &mut self,
        output: &[u8],
        error: bool,
        flags: u8,
    ) -> io::Result<()> {
        self.control_command_guard_bytes_at(unix_timestamp(), output, error, flags)
    }

    fn control_command_guard_bytes_at(
        &mut self,
        time: u64,
        output: &[u8],
        error: bool,
        flags: u8,
    ) -> io::Result<()> {
        if self.block_open {
            self.deferred
                .push_back(DeferredOutput::ControlCommandGuard {
                    time,
                    output: output.to_vec(),
                    error,
                    flags,
                });
            return Ok(());
        }
        self.write_control_command_guard(time, output, error, flags)?;
        self.output.flush()
    }

    fn write_control_command_guard(
        &mut self,
        time: u64,
        output: &[u8],
        error: bool,
        flags: u8,
    ) -> io::Result<()> {
        let frame = self.allocate_frame(time, flags)?;
        self.write_frame_begin(&frame)?;
        self.payload(output)?;
        self.write_frame_end(&frame, error)?;
        self.command_guard_frames = self.command_guard_frames.saturating_add(1);
        Ok(())
    }

    fn control_source_file(&mut self, event: ControlSourceFileEvent) -> io::Result<()> {
        if self.block_open {
            self.deferred
                .push_back(DeferredOutput::ControlSourceFile(event));
            return Ok(());
        }
        self.write_control_source_file(event)?;
        self.output.flush()
    }

    fn write_control_source_file(&mut self, event: ControlSourceFileEvent) -> io::Result<()> {
        match event {
            ControlSourceFileEvent::ReadError(text) => self.write_line(&text),
            ControlSourceFileEvent::Complete => {
                self.next_number = self.next_number.saturating_add(1);
                Ok(())
            }
        }
    }

    fn control_command_output(&mut self, output: &str) -> io::Result<()> {
        if self.block_open {
            self.deferred
                .push_back(DeferredOutput::ControlCommandOutput(output.to_owned()));
            return Ok(());
        }
        self.payload(output.as_bytes())?;
        self.output.flush()
    }

    fn begin(&mut self, flags: u8) -> io::Result<Frame> {
        self.begin_at(unix_timestamp(), flags)
    }

    fn begin_at(&mut self, time: u64, flags: u8) -> io::Result<Frame> {
        let frame = self.begin_buffered_at(time, flags)?;
        self.output.flush()?;
        Ok(frame)
    }

    fn begin_buffered_at(&mut self, time: u64, flags: u8) -> io::Result<Frame> {
        let frame = self.allocate_frame(time, flags)?;
        self.block_open = true;
        self.open_frame = Some(frame);
        self.write_frame_begin(&frame)?;
        Ok(frame)
    }

    fn allocate_frame(&mut self, time: u64, flags: u8) -> io::Result<Frame> {
        let number = self.next_number;
        self.next_number = self.next_number.saturating_add(1);
        let mut body = [0; 46];
        let mut remaining = body.as_mut_slice();
        writeln!(remaining, "{time} {number} {flags}")?;
        let length = (46 - remaining.len()) as u8;
        Ok(Frame { body, length })
    }

    fn write_frame_begin(&mut self, frame: &Frame) -> io::Result<()> {
        self.output.write_all(b"%begin ")?;
        self.output
            .write_all(&frame.body[..usize::from(frame.length)])
    }

    fn write_frame_end(&mut self, frame: &Frame, error: bool) -> io::Result<()> {
        let marker: &[u8] = if error { b"%error " } else { b"%end " };
        self.output.write_all(marker)?;
        self.output
            .write_all(&frame.body[..usize::from(frame.length)])
    }

    fn response(&mut self, frame: &Frame, response: CommandResponse) -> io::Result<u8> {
        match response {
            CommandResponse::Success {
                output, exit_code, ..
            } => {
                self.payload(output.as_bytes())?;
                self.end(frame, false)?;
                Ok(exit_code)
            }
            CommandResponse::Error { error, output, .. } => {
                self.payload(output.as_bytes())?;
                self.write_line(&error.tmux_message())?;
                self.end(frame, true)?;
                Ok(1)
            }
        }
    }

    fn parse_error(&mut self, error: &str) -> io::Result<()> {
        let frame = self.begin(1)?;
        self.write_line(error)?;
        self.end(&frame, true)
    }

    fn error(&mut self, frame: &Frame, error: &str) -> io::Result<()> {
        self.write_line(error)?;
        self.end(frame, true)
    }

    fn payload(&mut self, output: &[u8]) -> io::Result<()> {
        if !output.is_empty() {
            self.output.write_all(output)?;
            if output.last() != Some(&b'\n') {
                self.output.write_all(b"\n")?;
            }
        }
        Ok(())
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.output.write_all(line.as_bytes())?;
        self.output.write_all(b"\n")
    }

    fn end(&mut self, frame: &Frame, error: bool) -> io::Result<()> {
        self.write_frame_end(frame, error)?;
        self.block_open = false;
        self.open_frame = None;
        self.flush_deferred()?;
        self.output.flush()
    }

    fn emit_exit(&mut self, reason: Option<&str>) -> io::Result<()> {
        if self.exit_requested {
            return Ok(());
        }
        self.exit_requested = true;
        if self.block_open || self.exit_held {
            self.deferred
                .push_back(DeferredOutput::Exit(reason.map(str::to_owned)));
            return Ok(());
        }
        self.flush_deferred()?;
        self.write_exit(reason)?;
        self.output.flush()
    }

    fn write_exit(&mut self, reason: Option<&str>) -> io::Result<()> {
        match reason {
            Some(reason) => writeln!(self.output, "%exit {reason}")?,
            None => self.output.write_all(b"%exit\n")?,
        }
        Ok(())
    }

    fn close(&mut self)
    where
        W: ControlClose,
    {
        if self.double && !self.st_sent {
            let _ = self.output.write_all(ST);
            self.st_sent = true;
        }
        let _ = self.output.flush();
        self.output.close_sink();
    }

    fn terminate(&mut self) -> io::Result<()> {
        self.exit_held = false;
        if let Some(frame) = self.open_frame {
            self.end(&frame, false)?;
        }
        self.emit_exit(None)?;
        if self.st_sent {
            self.output.flush()
        } else {
            self.finish()
        }
    }

    fn finish(&mut self) -> io::Result<()> {
        if self.double {
            self.output.write_all(ST)?;
            self.st_sent = true;
        }
        self.output.flush()
    }
}

impl<W: Write> Drop for ControlWriter<W> {
    fn drop(&mut self) {
        if self.double && !self.st_sent {
            let _ = self.output.write_all(ST);
            let _ = self.output.flush();
        }
    }
}

#[derive(Clone, Copy)]
struct Frame {
    body: [u8; 46],
    length: u8,
}

struct StartedCommand {
    request_id: u64,
    flags: u8,
    canonical_name: Option<String>,
    command_guard_frames: u64,
    frame: Option<Frame>,
}

struct CommandResult {
    exit_code: u8,
    exit: ExitSignal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ExitSignal {
    #[default]
    None,
    Clean,
    Detached,
    TooFarBehind,
    Unexpected,
}

impl ExitSignal {
    fn is_some(self) -> bool {
        self != Self::None
    }

    fn reason(self) -> Option<&'static str> {
        match self {
            Self::TooFarBehind => Some("too far behind"),
            Self::Unexpected => Some("server exited unexpectedly"),
            Self::None | Self::Clean | Self::Detached => None,
        }
    }
}

enum MainEvent {
    Protocol(Box<ProtocolMessage>),
    Stdin(StdinEvent),
    Disconnected,
}

enum StdinEvent {
    Line(String),
    Eof,
    Error(String),
}

enum PendingReturn {
    Blank {
        code: u8,
        preceding_input: usize,
        observed_preceding_input: bool,
    },
    Eof {
        code: u8,
        preceding_input: usize,
        observed_preceding_input: bool,
    },
    InputError {
        message: String,
        preceding_input: usize,
    },
}

impl PendingReturn {
    fn from_stdin(
        stdin: StdinEvent,
        return_code: u8,
        preceding_input: usize,
    ) -> Result<Self, StdinEvent> {
        match stdin {
            StdinEvent::Line(line) if line.is_empty() => Ok(Self::Blank {
                code: return_code,
                preceding_input,
                observed_preceding_input: false,
            }),
            StdinEvent::Eof => Ok(Self::Eof {
                code: return_code,
                preceding_input,
                observed_preceding_input: false,
            }),
            StdinEvent::Error(message) => Ok(Self::InputError {
                message,
                preceding_input: 0,
            }),
            stdin @ StdinEvent::Line(_) => Err(stdin),
        }
    }

    fn code(&self) -> u8 {
        match self {
            Self::Blank { code, .. } | Self::Eof { code, .. } => *code,
            Self::InputError { .. } => 1,
        }
    }

    fn refresh_code_after_preceding_input(&mut self, return_code: u8) {
        match self {
            Self::Blank {
                code,
                observed_preceding_input: true,
                ..
            }
            | Self::Eof {
                code,
                observed_preceding_input: true,
                ..
            } => *code = return_code,
            Self::Blank { .. } | Self::Eof { .. } | Self::InputError { .. } => {}
        }
    }

    fn discards_pane_output(&self) -> bool {
        matches!(self, Self::Blank { .. } | Self::Eof { .. })
    }

    fn has_preceding_input(&self) -> bool {
        self.preceding_input() != 0
    }

    fn preceding_input(&self) -> usize {
        match self {
            Self::Blank {
                preceding_input, ..
            }
            | Self::Eof {
                preceding_input, ..
            }
            | Self::InputError {
                preceding_input, ..
            } => *preceding_input,
        }
    }

    fn consume_preceding_input(&mut self) {
        match self {
            Self::Blank {
                preceding_input,
                observed_preceding_input,
                ..
            }
            | Self::Eof {
                preceding_input,
                observed_preceding_input,
                ..
            } => {
                if *preceding_input != 0 {
                    *preceding_input -= 1;
                    *observed_preceding_input = true;
                }
            }
            Self::InputError {
                preceding_input, ..
            } => *preceding_input = preceding_input.saturating_sub(1),
        }
    }

    fn observe_preceding_input(&mut self) {
        match self {
            Self::Blank {
                observed_preceding_input,
                ..
            }
            | Self::Eof {
                observed_preceding_input,
                ..
            } => *observed_preceding_input = true,
            Self::InputError { .. } => {}
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
enum ParsedLine {
    Return,
    Ignore,
    Commands(Vec<CommandInvocation>),
    Error(String),
}

struct ControlTerminal {
    #[cfg(unix)]
    original: Option<rustix::termios::Termios>,
}

impl ControlTerminal {
    fn enter(enabled: bool) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use rustix::termios::{
                ControlModes, InputModes, LocalModes, OptionalActions, OutputModes,
                SpecialCodeIndex,
            };

            if !enabled || !io::stdin().is_terminal() {
                return Ok(Self { original: None });
            }
            let original = rustix::termios::tcgetattr(io::stdin())?;
            let mut raw = original.clone();
            raw.make_raw();
            raw.input_modes = InputModes::ICRNL | InputModes::IXANY;
            raw.output_modes = OutputModes::OPOST | OutputModes::ONLCR;
            #[cfg(target_os = "macos")]
            {
                raw.local_modes = LocalModes::from_bits_retain(libc::NOKERNINFO);
            }
            #[cfg(not(target_os = "macos"))]
            {
                raw.local_modes = LocalModes::empty();
            }
            raw.control_modes = ControlModes::CREAD | ControlModes::CS8 | ControlModes::HUPCL;
            raw.special_codes[SpecialCodeIndex::VMIN] = 1;
            raw.special_codes[SpecialCodeIndex::VTIME] = 0;
            rustix::termios::tcsetattr(io::stdin(), OptionalActions::Now, &raw)?;
            Ok(Self {
                original: Some(original),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = enabled;
            Ok(Self {})
        }
    }
}

impl Drop for ControlTerminal {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(original) = self.original.as_ref() {
            let _ = rustix::termios::tcsetattr(
                io::stdin(),
                rustix::termios::OptionalActions::Flush,
                original,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn direct_input_preserves_split_utf8_cr_and_trailing_eof_once() {
        let (read, write) = rustix::pipe::pipe().unwrap();
        let mut input = ControlInput::new(read);
        rustix::io::write(&write, b"display-message -p \xc3").unwrap();
        input.read();
        assert!(!input.has_event());
        assert!(input.event().is_none());
        rustix::io::write(&write, b"\xa9\r\n\nlast line").unwrap();
        input.read();
        assert!(matches!(input.event(), Some(StdinEvent::Line(line))
            if line == "display-message -p é\r"));
        assert!(matches!(input.event(), Some(StdinEvent::Line(line)) if line.is_empty()));
        assert!(input.event().is_none());
        drop(write);
        input.read();
        assert!(matches!(input.event(), Some(StdinEvent::Line(line)) if line == "last line"));
        assert!(matches!(input.event(), Some(StdinEvent::Eof)));
        assert!(input.event().is_none());
        assert!(!input.can_read());
    }

    #[cfg(unix)]
    #[test]
    fn direct_input_invalid_utf8_stops_before_later_lines_and_eof() {
        let (read, write) = rustix::pipe::pipe().unwrap();
        let mut input = ControlInput::new(read);
        rustix::io::write(&write, b"\xff\nnever execute\n").unwrap();
        input.read();
        assert!(matches!(input.event(), Some(StdinEvent::Error(_))));
        assert!(input.event().is_none());
        assert!(!input.has_event());
        assert!(!input.can_read());
    }

    #[cfg(unix)]
    fn direct_control_fixture() -> (
        tempfile::TempDir,
        std::os::unix::net::UnixStream,
        ControlReceiver,
        OwnedFd,
    ) {
        use std::os::unix::net::UnixListener;
        let directory = tempfile::Builder::new()
            .prefix("zz-pump-")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = directory.path().join("s");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            assert!(matches!(
                zz_protocol::read_protocol_message(&mut stream).unwrap(),
                ProtocolMessage::Hello(_)
            ));
            let capabilities = [
                zz_protocol::PANE_FRAME_CAPABILITY.to_owned(),
                zz_protocol::CONTROL_CAPABILITY.to_owned(),
            ];
            let welcome = ProtocolMessage::Welcome(zz_protocol::Welcome {
                protocol_version: zz_protocol::PROTOCOL_VERSION,
                server_id: 1,
                client_id: zz_protocol::ClientId(1),
                client_instance_id: zz_protocol::ClientInstanceId(1),
                caps: zz_protocol::Welcome::caps_from_strings(&capabilities),
            });
            let initial = ProtocolMessage::Batch(
                zz_protocol::Batch::from_messages(
                    1,
                    [ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 1,
                        payload: EventPayload::ClientView(zz_protocol::ClientView::default()),
                    })],
                )
                .unwrap(),
            );
            for message in [welcome, initial] {
                stream
                    .write_all(&zz_protocol::encode_protocol_message(&message).unwrap())
                    .unwrap();
            }
            stream
        });
        let client = Arc::new(InteractiveClient::connect_control(&socket).unwrap());
        let server = server.join().unwrap();
        let (input, writer) = rustix::pipe::pipe().unwrap();
        let receiver = ControlReceiver {
            source: ControlSource::Direct(DirectControl::with_input(client, input).unwrap()),
            pending: VecDeque::new(),
        };
        (directory, server, receiver, writer)
    }

    #[cfg(unix)]
    #[test]
    fn direct_pump_handles_ready_stdin_while_a_protocol_frame_is_incomplete() {
        let (_directory, mut server, mut receiver, input) = direct_control_fixture();
        assert!(
            matches!(receiver.receive(None).unwrap(), Some(MainEvent::Protocol(message))
            if matches!(*message, ProtocolMessage::Event(zz_protocol::Event { payload: EventPayload::ClientView(_), .. })))
        );
        let messages = [
            ProtocolMessage::Attach {
                session: "first".to_owned(),
            },
            ProtocolMessage::Attach {
                session: "second".to_owned(),
            },
        ];
        let frame = zz_protocol::encode_protocol_message(&ProtocolMessage::Batch(
            zz_protocol::Batch::from_messages(2, messages.clone()).unwrap(),
        ))
        .unwrap();
        server.write_all(&frame[..2]).unwrap();
        rustix::io::write(&input, b"ready input\n").unwrap();
        assert!(
            matches!(receiver.receive(Some(std::time::Duration::from_millis(20))).unwrap(),
            Some(MainEvent::Stdin(StdinEvent::Line(line))) if line == "ready input")
        );
        assert!(receiver.receive(None).unwrap().is_none());
        server.write_all(&frame[2..]).unwrap();
        rustix::io::write(&input, b"next input\n").unwrap();
        for expected in messages {
            assert!(
                matches!(receiver.receive(Some(std::time::Duration::from_millis(20))).unwrap(),
                Some(MainEvent::Protocol(message)) if *message == expected)
            );
        }
        assert!(
            matches!(receiver.receive(None).unwrap(), Some(MainEvent::Stdin(StdinEvent::Line(line)))
            if line == "next input")
        );
        drop(input);
        assert!(matches!(
            receiver
                .receive(Some(std::time::Duration::from_millis(20)))
                .unwrap(),
            Some(MainEvent::Stdin(StdinEvent::Eof))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn direct_pump_polls_fresh_stdin_between_buffered_protocol_frames() {
        for buffered in [false, true] {
            let (_directory, mut server, mut receiver, input) = direct_control_fixture();
            assert!(matches!(
                receiver.receive(None).unwrap(),
                Some(MainEvent::Protocol(_))
            ));
            rustix::io::write(&input, b"first command\n").unwrap();
            assert!(
                matches!(receiver.receive(None).unwrap(), Some(MainEvent::Stdin(StdinEvent::Line(line)))
                if line == "first command")
            );
            let messages: Vec<_> = (0..4)
                .map(|index| ProtocolMessage::Attach {
                    session: format!("frame{index}"),
                })
                .collect();
            let bytes: Vec<_> = messages
                .iter()
                .flat_map(|message| zz_protocol::encode_protocol_message(message).unwrap())
                .collect();
            server.write_all(&bytes).unwrap();
            let before = std::time::Instant::now();
            assert!(
                matches!(receiver.receive(Some(std::time::Duration::from_secs(1))).unwrap(),
                Some(MainEvent::Protocol(message)) if *message == messages[0])
            );
            assert!(before.elapsed() < std::time::Duration::from_millis(500));
            rustix::io::write(&input, b"newly ready\n").unwrap();
            let mut writer = ControlWriter::new(Vec::new(), false);
            let event = if buffered {
                Some(receive_buffered_control_event(&mut receiver, &mut writer).unwrap())
            } else {
                receiver.receive(None).unwrap()
            };
            assert!(
                matches!(event, Some(MainEvent::Stdin(StdinEvent::Line(line)))
                if line == "newly ready")
            );
            for expected in &messages[1..] {
                assert!(
                    matches!(receiver.receive(None).unwrap(), Some(MainEvent::Protocol(message))
                    if *message == *expected)
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn cached_stdin_keeps_fresh_protocol_priority_with_open_and_closed_guards() {
        for (buffered, block_open) in [(false, false), (false, true), (true, false), (true, true)] {
            let (_directory, mut server, mut receiver, input) = direct_control_fixture();
            assert!(matches!(
                receiver.receive(None).unwrap(),
                Some(MainEvent::Protocol(_))
            ));
            rustix::io::write(&input, b"first command\nsecond command\n").unwrap();
            assert!(matches!(
                receiver.receive(None).unwrap(),
                Some(MainEvent::Stdin(StdinEvent::Line(line))) if line == "first command"
            ));
            let ControlSource::Direct(direct) = &receiver.source else {
                unreachable!();
            };
            assert!(direct.input.has_event());
            assert!(!direct.prefer_stdin);
            let messages = [
                ProtocolMessage::CommandResponse(CommandResponse::Success {
                    request_id: 1,
                    output: "ready".into(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                }),
                ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                    server_id: 1,
                    outcome: ExecOutcome::Ran,
                }),
            ];
            server
                .write_all(
                    &zz_protocol::encode_protocol_message(&ProtocolMessage::Batch(
                        zz_protocol::Batch::from_messages(2, messages.clone()).unwrap(),
                    ))
                    .unwrap(),
                )
                .unwrap();
            let mut writer = ControlWriter::new(Vec::new(), false);
            if block_open {
                writer.begin_buffered_at(21, 1).unwrap();
            }
            let mut next = || {
                if buffered {
                    Some(receive_buffered_control_event(&mut receiver, &mut writer).unwrap())
                } else {
                    receiver.receive(None).unwrap()
                }
            };
            for expected in messages {
                assert!(
                    matches!(next(), Some(MainEvent::Protocol(message)) if *message == expected),
                    "buffered={buffered}, block_open={block_open}"
                );
            }
            assert!(
                matches!(next(), Some(MainEvent::Stdin(StdinEvent::Line(line)))
                if line == "second command")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_open_guard_joins_a_kernel_ready_response_before_pending_exit_and_eof() {
        #[derive(Default)]
        struct Writes(Vec<Vec<u8>>);
        impl Write for Writes {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.push(bytes.to_vec());
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let (_directory, mut server, mut receiver, _input) = direct_control_fixture();
        assert!(matches!(
            receiver.receive(None).unwrap(),
            Some(MainEvent::Protocol(_))
        ));
        let response = ProtocolMessage::CommandResponse(CommandResponse::Success {
            request_id: 1,
            output: "body".into(),
            exit_code: 0,
            stderr: String::new(),
            stdout_claim: StdoutClaim::None,
        });
        let finished = ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            server_id: 1,
            outcome: ExecOutcome::Ran,
        });
        let notification = ProtocolMessage::Attach {
            session: "after-exit".to_owned(),
        };
        server
            .write_all(
                &zz_protocol::encode_protocol_message(&ProtocolMessage::Batch(
                    zz_protocol::Batch::from_messages(
                        2,
                        [response, finished.clone(), notification.clone()],
                    )
                    .unwrap(),
                ))
                .unwrap(),
            )
            .unwrap();
        server.shutdown(std::net::Shutdown::Write).unwrap();
        let mut writer = ControlWriter::new(io::BufWriter::new(Writes::default()), false);
        let frame = writer.begin_buffered_at(21, 1).unwrap();
        let MainEvent::Protocol(message) =
            receive_buffered_control_event(&mut receiver, &mut writer).unwrap()
        else {
            panic!("response expected");
        };
        let ProtocolMessage::CommandResponse(response) = *message else {
            panic!("response expected");
        };
        assert!(writer.output.get_ref().0.is_empty());
        writer.response(&frame, response).unwrap();
        assert_eq!(
            writer.output.get_ref().0,
            [b"%begin 21 1 1\nbody\n%end 21 1 1\n".to_vec()]
        );
        for expected in [finished, notification] {
            assert!(matches!(
                receive_buffered_control_event(&mut receiver, &mut writer).unwrap(),
                MainEvent::Protocol(message) if *message == expected
            ));
        }
        assert!(matches!(
            receive_buffered_control_event(&mut receiver, &mut writer).unwrap(),
            MainEvent::Disconnected
        ));
        assert_eq!(writer.output.get_ref().0.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_probe_continuation_receives_stdin_and_protocol_ready_at_flush() {
        struct FlushReady {
            bytes: Vec<u8>,
            flushes: usize,
            ready: Option<Box<dyn FnOnce()>>,
        }
        impl Write for FlushReady {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                if let Some(ready) = self.ready.take() {
                    ready();
                }
                Ok(())
            }
        }

        for (stdin_ready, block_open) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let (_directory, mut server, mut receiver, input) = direct_control_fixture();
            assert!(matches!(
                receiver.receive(None).unwrap(),
                Some(MainEvent::Protocol(_))
            ));
            assert!(receiver.receive(None).unwrap().is_none());
            let message = ProtocolMessage::Attach {
                session: "after-flush".to_owned(),
            };
            let frame = zz_protocol::encode_protocol_message(&message).unwrap();
            let continuation = frame[2..].to_vec();
            let mut later_server = server.try_clone().unwrap();
            let ready_input = input.as_fd().try_clone_to_owned().unwrap();
            let mut writer = ControlWriter::new(
                FlushReady {
                    bytes: Vec::new(),
                    flushes: 0,
                    ready: Some(Box::new(move || {
                        if stdin_ready {
                            server.write_all(&frame[..2]).unwrap();
                            rustix::io::write(&ready_input, b"after empty\n").unwrap();
                        } else {
                            server.write_all(&frame).unwrap();
                        }
                    })),
                },
                false,
            );
            if block_open {
                writer.begin_buffered_at(21, 1).unwrap();
            }
            let event = receive_buffered_control_event(&mut receiver, &mut writer).unwrap();
            assert_eq!(writer.output.flushes, 1);
            assert_eq!(
                writer.output.bytes,
                if block_open {
                    b"%begin 21 1 1\n".as_slice()
                } else {
                    b"".as_slice()
                }
            );
            if stdin_ready {
                assert!(matches!(event, MainEvent::Stdin(StdinEvent::Line(line))
                    if line == "after empty"));
                assert!(receiver.receive(None).unwrap().is_none());
                later_server.write_all(&continuation).unwrap();
                assert!(matches!(receive_control_event(&mut receiver).unwrap(),
                    MainEvent::Protocol(packet) if *packet == message));
            } else {
                assert!(matches!(event, MainEvent::Protocol(packet) if *packet == message));
            }
        }
    }

    #[test]
    fn pipelined_lines_preserve_order_and_stop_at_the_pending_return() {
        let pending = VecDeque::from([
            StdinEvent::Line("set-environment -g ORDER first".to_owned()),
            StdinEvent::Line("display-message -p $ORDER".to_owned()),
            StdinEvent::Line("set-environment -g ORDER after-return".to_owned()),
        ]);
        let mut submitted = 0;
        let mut lines = Vec::new();
        submit_pending_control_lines(&pending, &mut submitted, Some(2), |line| {
            lines.push(line.to_owned());
            Ok(())
        })
        .unwrap();
        submit_pending_control_lines(&pending, &mut submitted, Some(2), |line| {
            lines.push(line.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            lines,
            [
                "set-environment -g ORDER first",
                "display-message -p $ORDER"
            ]
        );
        assert_eq!(submitted, 2);
    }

    #[test]
    fn pipelined_lines_bound_outstanding_requests_and_keep_ignored_input_in_order() {
        let pending = (0..40)
            .map(|index| StdinEvent::Line(format!("display-message -p {index}")))
            .collect();
        let mut submitted = 0;
        let mut lines = Vec::new();
        submit_pending_control_lines(&pending, &mut submitted, None, |line| {
            lines.push(line.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(lines.len(), 32);
        assert_eq!(lines.last().unwrap(), "display-message -p 31");
        let pending = VecDeque::from([
            StdinEvent::Line("# ignored".to_owned()),
            StdinEvent::Line("display-message -p later".to_owned()),
        ]);
        submitted = 0;
        submit_pending_control_lines(&pending, &mut submitted, None, |_| {
            panic!("later input must wait for the ignored line to be consumed")
        })
        .unwrap();
        assert_eq!(submitted, 0);
    }

    #[test]
    fn raw_control_guards_preserve_bytes_even_when_deferred() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(17, 1).unwrap();
        writer
            .control_command_guard_bytes_at(18, b"a\xffb\n", false, 1)
            .unwrap();
        writer.end(&frame, false).unwrap();
        assert_eq!(
            writer.output,
            b"%begin 17 1 1\n%end 17 1 1\n%begin 18 2 1\na\xffb\n%end 18 2 1\n"
        );
    }

    #[test]
    fn native_frame_metadata_keeps_zero_max_and_raw_bytes() {
        println!(
            "Frame size={}, Option<Frame> size={}",
            std::mem::size_of::<Frame>(),
            std::mem::size_of::<Option<Frame>>()
        );
        for (time, number, flags, body) in [
            (0, 0, 0, b"0 0 0\n".as_slice()),
            (
                u64::MAX,
                u64::MAX,
                u8::MAX,
                b"18446744073709551615 18446744073709551615 255\n".as_slice(),
            ),
        ] {
            for error in [false, true] {
                let mut writer = ControlWriter::new(Vec::new(), false);
                writer.next_number = number;
                let frame = writer.begin_buffered_at(time, flags).unwrap();
                writer.payload(b"a\xffb").unwrap();
                writer.end(&frame, error).unwrap();
                assert_eq!(writer.next_number, number.saturating_add(1));

                let marker: &[u8] = if error { b"%error " } else { b"%end " };
                let mut expected = b"%begin ".to_vec();
                expected.extend_from_slice(body);
                expected.extend_from_slice(b"a\xffb\n");
                expected.extend_from_slice(marker);
                expected.extend_from_slice(body);
                if number == u64::MAX {
                    writer
                        .control_command_guard_bytes_at(time, b"c\xfe", error, flags)
                        .unwrap();
                    expected.extend_from_slice(b"%begin ");
                    expected.extend_from_slice(body);
                    expected.extend_from_slice(b"c\xfe\n");
                    expected.extend_from_slice(marker);
                    expected.extend_from_slice(body);
                    assert_eq!(writer.next_number, u64::MAX);
                }
                assert_eq!(writer.output, expected);
            }
        }
    }

    #[test]
    fn native_frame_metadata_preserves_partial_sink_errors() {
        let mut bytes = [0; 8];
        let mut writer = ControlWriter::new(bytes.as_mut_slice(), false);
        assert_eq!(
            writer.begin_at(0, 0).err().unwrap().kind(),
            io::ErrorKind::WriteZero
        );
        assert_eq!(writer.next_number, 2);
        drop(writer);
        assert_eq!(&bytes, b"%begin 0");

        for (error, expected) in [
            (false, b"%begin 0 1 0\n%end 0".as_slice()),
            (true, b"%begin 0 1 0\n%error 0".as_slice()),
        ] {
            let mut bytes = [0; 21];
            let mut writer = ControlWriter::new(&mut bytes[..expected.len()], false);
            let frame = writer.begin_at(0, 0).unwrap();
            assert_eq!(
                writer.end(&frame, error).unwrap_err().kind(),
                io::ErrorKind::WriteZero
            );
            drop(writer);
            assert_eq!(&bytes[..expected.len()], expected);
        }
    }

    #[test]
    fn started_response_preserves_raw_output_before_callback_guards() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(17, 1).unwrap();
        writer
            .control_command_guard_at(18, "child", false, 1)
            .unwrap();
        writer
            .response(
                &frame,
                CommandResponse::Success {
                    request_id: 1,
                    output: RawText::from_bytes(b"a\xffb\n".to_vec()),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        assert_eq!(
            writer.output,
            b"%begin 17 1 1\na\xffb\n%end 17 1 1\n%begin 18 2 1\nchild\n%end 18 2 1\n"
        );
    }

    #[test]
    fn batched_control_frames_keep_guards_before_notifications_and_exit() {
        let frames = [
            ProtocolMessage::Event(zz_protocol::Event {
                sequence: 1,
                payload: EventPayload::ControlCommandGuard {
                    output: "ready\n".into(),
                    error: false,
                    sticky_failure: false,
                    flags: 1,
                },
            }),
            ProtocolMessage::Event(zz_protocol::Event {
                sequence: 2,
                payload: EventPayload::HookEvent {
                    name: "session-created".to_owned(),
                    variables: BTreeMap::new(),
                },
            }),
            ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                server_id: 9,
                outcome: ExecOutcome::Ran,
            }),
        ]
        .iter()
        .map(|message| zz_protocol::encode_protocol_message(message).unwrap())
        .collect();
        let (events, receiver) = mpsc::sync_channel(4);
        forward_protocol_message(
            ProtocolMessage::Batch(zz_protocol::Batch {
                sequence: 2,
                frames,
            }),
            &events,
        )
        .unwrap();
        events
            .send(MainEvent::Stdin(StdinEvent::Line("next".to_owned())))
            .unwrap();
        let mut receiver = ControlReceiver::new(receiver);
        let mut writer = ControlWriter::new(Vec::new(), false);
        let mut state = ControlState {
            attached_session: Some(SessionId(1)),
            ..ControlState::default()
        };
        for _ in 0..2 {
            let MainEvent::Protocol(message) = receive_control_event(&mut receiver).unwrap() else {
                panic!("expected protocol message");
            };
            handle_protocol(*message, &mut state, &mut writer).unwrap();
        }
        let text = String::from_utf8(writer.output.clone()).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert!(lines[0].starts_with("%begin "));
        assert_eq!(lines[1], "ready");
        assert!(lines[2].starts_with("%end "));
        assert_eq!(lines[3], "%sessions-changed");
        assert!(
            matches!(receive_control_event(&mut receiver).unwrap(), MainEvent::Protocol(message)
            if matches!(*message, ProtocolMessage::ExecExit(_)))
        );
        assert!(matches!(
            receive_control_event(&mut receiver).unwrap(),
            MainEvent::Stdin(StdinEvent::Line(line)) if line == "next"
        ));
    }

    #[test]
    fn control_batch_is_one_channel_item_and_retains_children_across_returns() {
        let started = |request_id| {
            ProtocolMessage::Event(zz_protocol::Event {
                sequence: request_id,
                payload: EventPayload::ControlCommandStarted {
                    request_id,
                    flags: 1,
                    canonical_name: Some("display-message".to_owned()),
                    guard: true,
                },
            })
        };
        let response = ProtocolMessage::CommandResponse(CommandResponse::Success {
            request_id: 1,
            output: "ready".into(),
            exit_code: 0,
            stderr: String::new(),
            stdout_claim: StdoutClaim::None,
        });
        let finished = ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            server_id: 9,
            outcome: ExecOutcome::Ran,
        });
        let notification = ProtocolMessage::Event(zz_protocol::Event {
            sequence: 2,
            payload: EventPayload::HookEvent {
                name: "session-created".to_owned(),
                variables: BTreeMap::new(),
            },
        });
        let frames = [response, finished, notification]
            .iter()
            .map(|message| zz_protocol::encode_protocol_message(message).unwrap())
            .collect();
        let (events, receiver) = mpsc::sync_channel(8);
        forward_protocol_message(started(1), &events).unwrap();
        forward_protocol_message(
            ProtocolMessage::Batch(zz_protocol::Batch {
                sequence: 2,
                frames,
            }),
            &events,
        )
        .unwrap();
        forward_protocol_message(started(2), &events).unwrap();
        events.send(MainEvent::Stdin(StdinEvent::Eof)).unwrap();
        let mut receiver = ControlReceiver::new(receiver);
        assert!(matches!(
            receive_control_event(&mut receiver).unwrap(),
            MainEvent::Protocol(message) if matches!(
                *message,
                ProtocolMessage::Event(zz_protocol::Event {
                    payload: EventPayload::ControlCommandStarted { request_id: 1, .. }, ..
                })
            )
        ));
        assert!(matches!(
            receive_control_event(&mut receiver).unwrap(),
            MainEvent::Protocol(message) if matches!(*message, ProtocolMessage::CommandResponse(_))
        ));
        assert_eq!(receiver.pending.len(), 2);
        assert!(matches!(
            receive_control_event(&mut receiver).unwrap(),
            MainEvent::Protocol(message) if matches!(*message, ProtocolMessage::ExecExit(_))
        ));
        assert_eq!(receiver.pending.len(), 1);
        assert!(matches!(
            receive_control_event(&mut receiver).unwrap(),
            MainEvent::Protocol(message) if matches!(
                *message,
                ProtocolMessage::Event(zz_protocol::Event {
                    payload: EventPayload::HookEvent { .. }, ..
                })
            )
        ));
        assert!(matches!(
            receive_control_event(&mut receiver).unwrap(),
            MainEvent::Protocol(message) if matches!(
                *message,
                ProtocolMessage::Event(zz_protocol::Event {
                    payload: EventPayload::ControlCommandStarted { request_id: 2, .. }, ..
                })
            )
        ));
        assert!(matches!(
            receive_control_event(&mut receiver).unwrap(),
            MainEvent::Stdin(StdinEvent::Eof)
        ));
    }

    #[test]
    fn malformed_control_batches_deliver_no_partial_guard() {
        let guard = ProtocolMessage::Event(zz_protocol::Event {
            sequence: 1,
            payload: EventPayload::ControlCommandGuard {
                output: "partial".into(),
                error: false,
                sticky_failure: false,
                flags: 1,
            },
        });
        let nested =
            zz_protocol::encode_protocol_message(&ProtocolMessage::Batch(zz_protocol::Batch {
                sequence: 1,
                frames: vec![zz_protocol::encode_protocol_message(&guard).unwrap()],
            }))
            .unwrap();
        for invalid in [vec![0], nested] {
            let (events, receiver) = mpsc::sync_channel(1);
            forward_protocol_message(
                ProtocolMessage::Batch(zz_protocol::Batch {
                    sequence: 2,
                    frames: vec![
                        zz_protocol::encode_protocol_message(&guard).unwrap(),
                        invalid,
                    ],
                }),
                &events,
            )
            .unwrap();
            let mut receiver = ControlReceiver::new(receiver);
            let Err(error) = receiver.receive(None) else {
                panic!("malformed batch must fail before delivering its guard");
            };
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(receiver.pending.is_empty());
        }
    }

    #[test]
    fn a_control_tree_base_mismatch_requests_sync_without_adopting_it() {
        let mut state = ControlState::default();
        let mut writer = ControlWriter::new(Vec::new(), false);
        handle_protocol(
            ProtocolMessage::Event(zz_protocol::Event {
                sequence: 1,
                payload: EventPayload::TreeDelta(zz_protocol::TreeDelta {
                    base: 5,
                    version: 6,
                    ops: Vec::new(),
                }),
            }),
            &mut state,
            &mut writer,
        )
        .unwrap();
        assert!(state.tree_sync_required);
        assert_eq!(state.tree.generation, 0);
        assert_eq!(state.snapshot.generation, 0);
        assert!(writer.output.is_empty());
    }

    #[test]
    fn termination_closes_pending_frame_and_releases_deferred_exit() {
        let mut writer = ControlWriter::new(Vec::new(), true);
        writer.start().unwrap();
        writer.hold_exit();
        writer.begin_at(17, 1).unwrap();
        writer
            .control_source_file(ControlSourceFileEvent::ReadError(
                "Bad file descriptor: -".to_owned(),
            ))
            .unwrap();
        writer.emit_exit(None).unwrap();
        writer.terminate().unwrap();
        assert_eq!(
            writer.output,
            b"\x1bP1000p%begin 17 1 1\n%end 17 1 1\nBad file descriptor: -\n%exit\n\x1b\\"
        );
        let completed = writer.output.clone();
        writer.terminate().unwrap();
        assert_eq!(writer.output, completed);
    }

    #[test]
    fn serializer_keeps_frame_identity_payload_and_error_shapes() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let first = writer.begin_at(17, 0).unwrap();
        assert_eq!(&first.body[..usize::from(first.length)], b"17 1 0\n");
        writer
            .response(
                &first,
                CommandResponse::Success {
                    request_id: 1,
                    output: "one\ntwo\n".into(),
                    exit_code: 7,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        let second = writer.begin_at(18, 1).unwrap();
        writer
            .response(
                &second,
                CommandResponse::Error {
                    request_id: 2,
                    error: ServerError::SessionNotFound("gone".to_owned()),
                    output: "hook\n\n".into(),
                },
            )
            .unwrap();
        let third = writer.begin_at(19, 1).unwrap();
        writer
            .response(
                &third,
                CommandResponse::Error {
                    request_id: 3,
                    error: ServerError::InvalidCommand("unknown command: bogus-command".to_owned()),
                    output: RawText::default(),
                },
            )
            .unwrap();
        let fourth = writer.begin_at(20, 1).unwrap();
        writer
            .response(
                &fourth,
                CommandResponse::Error {
                    request_id: 4,
                    error: ServerError::UnsupportedCommand("new-pane".to_owned()),
                    output: RawText::default(),
                },
            )
            .unwrap();
        let fifth = writer.begin_at(21, 0).unwrap();
        writer
            .response(
                &fifth,
                CommandResponse::Success {
                    request_id: 5,
                    output: "\n".into(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        assert_eq!(
            writer.output,
            b"%begin 17 1 0\none\ntwo\n%end 17 1 0\n%begin 18 2 1\nhook\n\ncan't find session: gone\n%error 18 2 1\n%begin 19 3 1\nunknown command: bogus-command\n%error 19 3 1\n%begin 20 4 1\nunsupported command: new-pane\n%error 20 4 1\n%begin 21 5 0\n\n%end 21 5 0\n"
        );
    }

    #[test]
    fn control_command_guards_defer_fifo_with_their_own_flags() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let direct = writer.begin_at(17, 1).unwrap();
        assert_eq!(
            writer
                .response(
                    &direct,
                    CommandResponse::Error {
                        request_id: 1,
                        error: ServerError::InvalidCommand(
                            "No such file or directory: direct.conf".to_owned(),
                        ),
                        output: RawText::default(),
                    },
                )
                .unwrap(),
            1
        );
        let outer = writer.begin_at(18, 1).unwrap();
        writer.control_command_guard_at(19, "", false, 0).unwrap();
        writer.control_command_guard_at(20, "", false, 1).unwrap();
        writer
            .control_command_guard_at(21, "No such file or directory: partial.conf", false, 0)
            .unwrap();
        writer
            .control_command_guard_at(22, "can't find session: missing-runtime", true, 1)
            .unwrap();
        assert_eq!(
            writer
                .response(
                    &outer,
                    CommandResponse::Success {
                        request_id: 2,
                        output: RawText::default(),
                        exit_code: 3,
                        stderr: String::new(),
                        stdout_claim: StdoutClaim::None,
                    },
                )
                .unwrap(),
            3
        );
        let fresh = writer.begin_at(23, 1).unwrap();
        writer
            .response(
                &fresh,
                CommandResponse::Success {
                    request_id: 3,
                    output: "fresh".into(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        assert_eq!(
            writer.output,
            b"%begin 17 1 1\nNo such file or directory: direct.conf\n%error 17 1 1\n\
              %begin 18 2 1\n%end 18 2 1\n\
              %begin 19 3 0\n%end 19 3 0\n\
              %begin 20 4 1\n%end 20 4 1\n\
              %begin 21 5 0\nNo such file or directory: partial.conf\n%end 21 5 0\n\
              %begin 22 6 1\ncan't find session: missing-runtime\n%error 22 6 1\n\
              %begin 23 7 1\nfresh\n%end 23 7 1\n"
        );
    }

    /// A control client's command block carries the daemon's output bytes, the
    /// way the plain CLI does. Measured against the pin on 2026-09-04 with
    /// `set-environment -g ZZBYTES a<0xff>b`:
    /// `printf 'show-environment -g ZZBYTES\\n' | <bin> -C` puts
    /// `5a5a42595445533d61ff620a` inside the block on both binaries, where a
    /// lossy `&str` sink put `5a5a42595445533d61efbfbd620a` there instead.
    #[test]
    fn a_command_block_carries_the_output_bytes_it_was_given() {
        let needle: &[u8] = b"ZZBYTES=a\xffb";
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin(1).unwrap();
        writer
            .response(
                &frame,
                CommandResponse::Success {
                    request_id: 9,
                    output: RawText::from_bytes(b"ZZBYTES=a\xffb\n".to_vec()),
                    stderr: String::new(),
                    exit_code: 0,
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        assert!(
            writer
                .output
                .windows(needle.len())
                .any(|window| window == needle),
            "control payload dropped the byte: {:?}",
            writer.output
        );
    }

    #[test]
    fn opaque_commands_render_children_and_only_fallback_before_them() {
        let mut empty = ControlWriter::new(Vec::new(), false);
        let empty_guard_frames = empty.command_guard_frames;
        assert_eq!(
            render_command_response(
                &mut empty,
                None,
                1,
                empty_guard_frames,
                CommandResponse::Success {
                    request_id: 1,
                    output: "outer output".into(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap(),
            0
        );
        assert!(empty.output.is_empty());

        let mut children = ControlWriter::new(Vec::new(), false);
        let child_guard_frames = children.command_guard_frames;
        children
            .control_command_guard_at(17, "first", false, 0)
            .unwrap();
        children
            .control_command_guard_at(18, "second", false, 1)
            .unwrap();
        let child_output = children.output.clone();
        assert_eq!(
            render_command_response(
                &mut children,
                None,
                1,
                child_guard_frames,
                CommandResponse::Error {
                    request_id: 2,
                    error: ServerError::InvalidCommand("outer failure".to_owned()),
                    output: "outer output".into(),
                },
            )
            .unwrap(),
            1
        );
        assert_eq!(children.output, child_output);

        let mut response_failure = ControlWriter::new(Vec::new(), false);
        let response_guard_frames = response_failure.command_guard_frames;
        assert_eq!(
            render_command_response(
                &mut response_failure,
                None,
                1,
                response_guard_frames,
                CommandResponse::Error {
                    request_id: 3,
                    error: ServerError::InvalidCommand("outer failure".to_owned()),
                    output: RawText::default(),
                },
            )
            .unwrap(),
            1
        );
        let lines = std::str::from_utf8(&response_failure.output)
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        assert_eq!(lines[1], "outer failure");
        assert!(lines[0].ends_with(" 1 1"));
        assert_eq!(
            lines[0].strip_prefix("%begin "),
            lines[2].strip_prefix("%error ")
        );

        let mut submission_failure = ControlWriter::new(Vec::new(), false);
        let submission_guard_frames = submission_failure.command_guard_frames;
        render_command_failure(
            &mut submission_failure,
            None,
            0,
            submission_guard_frames,
            "request failed",
        )
        .unwrap();
        let lines = std::str::from_utf8(&submission_failure.output)
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        assert_eq!(lines[1], "request failed");
        assert!(lines[0].ends_with(" 1 0"));
        assert_eq!(
            lines[0].strip_prefix("%begin "),
            lines[2].strip_prefix("%error ")
        );
    }

    #[test]
    fn standalone_diagnostics_only_set_source_read_return_codes() {
        let source_read = format!(
            "stream did not contain valid UTF-8: {}",
            std::env::temp_dir().join("invalid-source.conf").display()
        );
        for kind in [
            zz_protocol::ClientMessageKind::Error,
            zz_protocol::ClientMessageKind::Warning,
        ] {
            let mut source_writer = ControlWriter::new(Vec::new(), false);
            let mut source_state = ControlState::default();
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::ClientMessage {
                        pane: None,
                        kind,
                        text: source_read.clone(),
                    },
                }),
                &mut source_state,
                &mut source_writer,
            )
            .unwrap();
            assert_eq!(source_state.return_code, 1);
            assert_eq!(completed_exit_code(0, &source_state), 1);
            let source_lines = std::str::from_utf8(&source_writer.output)
                .unwrap()
                .lines()
                .collect::<Vec<_>>();
            assert_eq!(source_lines[1], source_read);
            assert!(source_lines[2].starts_with("%error "));
        }

        let mut unrelated_writer = ControlWriter::new(Vec::new(), false);
        let mut unrelated_state = ControlState::default();
        handle_protocol(
            ProtocolMessage::Event(zz_protocol::Event {
                sequence: 2,
                payload: EventPayload::ClientMessage {
                    pane: None,
                    kind: zz_protocol::ClientMessageKind::Error,
                    text: "background worker failed".to_owned(),
                },
            }),
            &mut unrelated_state,
            &mut unrelated_writer,
        )
        .unwrap();
        assert_eq!(unrelated_state.return_code, 0);
        assert_eq!(completed_exit_code(0, &unrelated_state), 0);
        assert!(
            std::str::from_utf8(&unrelated_writer.output)
                .unwrap()
                .contains("background worker failed")
        );
    }

    #[test]
    fn control_command_guard_status_is_independent_of_its_terminator() {
        for (flags, sticky_failure, error, output, expected) in [
            (0, false, false, "diagnostic", 0),
            (1, false, true, "diagnostic", 0),
            (
                0,
                false,
                true,
                "No such file or directory: missing-source.conf",
                1,
            ),
            (1, true, false, "diagnostic", 1),
        ] {
            let mut writer = ControlWriter::new(Vec::new(), false);
            let mut state = ControlState::default();
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::ControlCommandGuard {
                        output: output.into(),
                        error,
                        sticky_failure,
                        flags,
                    },
                }),
                &mut state,
                &mut writer,
            )
            .unwrap();
            assert_eq!(state.return_code, expected);
            let lines = std::str::from_utf8(&writer.output)
                .unwrap()
                .lines()
                .collect::<Vec<_>>();
            assert_eq!(lines[1], output);
            assert!(lines[0].ends_with(&format!(" {flags}")));
            let terminator = if error { "%error " } else { "%end " };
            assert_eq!(
                lines[0].strip_prefix("%begin "),
                lines[2].strip_prefix(terminator)
            );
        }

        let mut writer = ControlWriter::new(Vec::new(), false);
        let mut state = ControlState::default();
        for (sequence, sticky_failure) in [(1, true), (2, false)] {
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence,
                    payload: EventPayload::ControlCommandGuard {
                        output: "diagnostic".into(),
                        error: false,
                        sticky_failure,
                        flags: 0,
                    },
                }),
                &mut state,
                &mut writer,
            )
            .unwrap();
            assert_eq!(state.return_code, 1);
        }
    }

    #[test]
    fn control_source_file_events_defer_raw_errors_and_consume_hidden_numbers() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let mut state = ControlState::default();
        let direct = writer.begin_at(18, 1).unwrap();
        for (sequence, payload) in [
            (
                1,
                EventPayload::ControlCommandGuard {
                    output: "".into(),
                    error: false,
                    sticky_failure: false,
                    flags: 1,
                },
            ),
            (
                2,
                EventPayload::ControlSourceFile {
                    event: ControlSourceFileEvent::ReadError(
                        "Is a directory: nested.conf".to_owned(),
                    ),
                },
            ),
            (
                3,
                EventPayload::ControlSourceFile {
                    event: ControlSourceFileEvent::Complete,
                },
            ),
            (
                4,
                EventPayload::ControlCommandGuard {
                    output: "AFTER".into(),
                    error: false,
                    sticky_failure: false,
                    flags: 1,
                },
            ),
        ] {
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event { sequence, payload }),
                &mut state,
                &mut writer,
            )
            .unwrap();
        }
        writer
            .response(
                &direct,
                CommandResponse::Success {
                    request_id: 1,
                    output: RawText::default(),
                    exit_code: 1,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        assert_eq!(state.return_code, 1);
        let lines = std::str::from_utf8(&writer.output)
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 8);
        assert_eq!(lines[0], "%begin 18 1 1");
        assert_eq!(lines[1], "%end 18 1 1");
        assert!(lines[2].starts_with("%begin "));
        assert!(lines[2].ends_with(" 2 1"));
        assert_eq!(
            lines[2].strip_prefix("%begin "),
            lines[3].strip_prefix("%end ")
        );
        assert_eq!(lines[4], "Is a directory: nested.conf");
        assert!(lines[5].starts_with("%begin "));
        assert!(lines[5].ends_with(" 4 1"));
        assert_eq!(lines[6], "AFTER");
        assert_eq!(
            lines[5].strip_prefix("%begin "),
            lines[7].strip_prefix("%end ")
        );
    }

    #[test]
    fn legacy_diagnostic_errors_still_defer_after_open_commands() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let nested = writer.begin_at(18, 1).unwrap();
        writer
            .diagnostic_error_at(
                19,
                "No such file or directory: nested-a.conf\nNo such file or directory: nested-b.conf",
            )
            .unwrap();
        assert_eq!(
            writer
                .response(
                    &nested,
                    CommandResponse::Success {
                        request_id: 2,
                        output: RawText::default(),
                        exit_code: 0,
                        stderr: String::new(),
                        stdout_claim: StdoutClaim::None,
                    },
                )
                .unwrap(),
            0
        );
        assert_eq!(
            writer.output,
            b"%begin 18 1 1\n%end 18 1 1\n%begin 19 2 1\nNo such file or directory: nested-a.conf\nNo such file or directory: nested-b.conf\n%error 19 2 1\n"
        );
    }

    #[test]
    fn generic_nonzero_success_ends_and_continues() {
        let success = CommandResponse::Success {
            request_id: 1,
            output: RawText::default(),
            exit_code: 3,
            stderr: String::new(),
            stdout_claim: StdoutClaim::None,
        };
        assert!(!response_aborts_line(&success));
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(17, 1).unwrap();
        assert_eq!(writer.response(&frame, success).unwrap(), 3);
        assert_eq!(writer.output, b"%begin 17 1 1\n%end 17 1 1\n");
        assert!(response_aborts_line(&CommandResponse::Error {
            request_id: 2,
            error: ServerError::InvalidCommand("failed".to_owned()),
            output: RawText::default(),
        }));
        let parse = CommandResponse::Error {
            request_id: 3,
            error: ServerError::CommandParse("unknown flag -Z".to_owned()),
            output: RawText::default(),
        };
        assert!(response_aborts_line(&parse));
        assert!(!response_sets_return_code(Some("list-sessions"), &parse));
        assert!(response_sets_return_code(
            Some("kill-session"),
            &CommandResponse::Error {
                request_id: 4,
                error: ServerError::SessionNotFound("missing".to_owned()),
                output: RawText::default(),
            }
        ));
        let direct = CommandResponse::Error {
            request_id: 5,
            error: ServerError::InvalidCommand("can't find session: missing".to_owned()),
            output: RawText::default(),
        };
        assert!(!response_is_post_admission_callback_failure(&direct));
        let callback = CommandResponse::Error {
            request_id: 6,
            error: ServerError::PostAdmissionCallback(Box::new(ServerError::SessionNotFound(
                "missing".to_owned(),
            ))),
            output: RawText::default(),
        };
        assert!(response_is_post_admission_callback_failure(&callback));
        let nonzero = CommandResponse::Success {
            request_id: 7,
            output: RawText::default(),
            exit_code: 3,
            stderr: String::new(),
            stdout_claim: StdoutClaim::None,
        };
        assert!(!response_sets_return_code(Some("run-shell"), &nonzero));
        assert!(response_sets_return_code(Some("source-file"), &nonzero));
        assert!(!response_aborts_line(&nonzero));
        assert_eq!(completed_exit_code(3, &ControlState::default()), 3);
    }

    #[test]
    fn initial_eof_waits_behind_every_queued_input() {
        let mut pending_return = None;
        let mut pending_stdin = VecDeque::new();
        let mut writer = ControlWriter::new(Vec::new(), false);
        capture_pending_return(
            StdinEvent::Line("run-shell 'sleep 1'".to_owned()),
            0,
            &mut pending_return,
            &mut pending_stdin,
            &mut writer,
        );
        capture_pending_return(
            StdinEvent::Line("display-message -p SECOND".to_owned()),
            0,
            &mut pending_return,
            &mut pending_stdin,
            &mut writer,
        );
        capture_pending_return(
            StdinEvent::Eof,
            0,
            &mut pending_return,
            &mut pending_stdin,
            &mut writer,
        );
        capture_pending_return(
            StdinEvent::Line(String::new()),
            1,
            &mut pending_return,
            &mut pending_stdin,
            &mut writer,
        );
        assert_eq!(pending_return.as_ref().map(PendingReturn::code), Some(0));
        assert!(
            pending_return
                .as_ref()
                .is_some_and(PendingReturn::has_preceding_input)
        );
        assert!(matches!(
            pending_stdin.pop_front(),
            Some(StdinEvent::Line(line)) if line == "run-shell 'sleep 1'"
        ));
        let pending_return = pending_return.as_mut().expect("pending return");
        pending_return.consume_preceding_input();
        assert!(pending_return.has_preceding_input());
        assert!(matches!(
            pending_stdin.pop_front(),
            Some(StdinEvent::Line(line)) if line == "display-message -p SECOND"
        ));
        assert!(matches!(
            pending_stdin.pop_front(),
            Some(StdinEvent::Line(line)) if line.is_empty()
        ));
        assert!(pending_stdin.is_empty());
        pending_return.consume_preceding_input();
        assert!(!pending_return.has_preceding_input());

        let mut state = ControlState {
            pending_return: Some(PendingReturn::Eof {
                code: 0,
                preceding_input: 2,
                observed_preceding_input: false,
            }),
            ..ControlState::default()
        };
        let mut queued = VecDeque::from([StdinEvent::Line("display-message -p LOST".to_owned())]);
        assert!(release_parked_queue_at_client_exit(&mut state, &mut queued));
        assert!(queued.is_empty());
        assert!(
            state
                .pending_return
                .as_ref()
                .is_some_and(|pending| !pending.has_preceding_input())
        );
    }

    #[test]
    fn preparation_error_releases_eof_after_the_last_queued_input() {
        let invalid = "bind-key -T { set-environment -g BIND_CONTROL_REJECT_FORBIDDEN yes } F11 display-message -p forbidden";
        let mut pending_stdin = VecDeque::from([StdinEvent::Line(invalid.to_owned())]);
        let mut state = ControlState {
            pending_return: Some(PendingReturn::Eof {
                code: 0,
                preceding_input: 1,
                observed_preceding_input: false,
            }),
            ..ControlState::default()
        };

        let Some(StdinEvent::Line(line)) = pending_stdin.pop_front() else {
            panic!("missing retained input");
        };
        state
            .pending_return
            .as_mut()
            .expect("pending EOF")
            .consume_preceding_input();
        let ParsedLine::Commands(mut commands) =
            parse_line(&line, &BTreeMap::new(), &BTreeMap::new())
        else {
            panic!("invalid bind-key did not parse for preparation");
        };
        let prepared = PreparedCommand {
            invocation: commands.remove(0),
            canonical_name: Some("bind-key".to_owned()),
            alias_matched: false,
            result: PreparedCommandResult::Error(ServerError::CommandParse(
                "command bind-key: -T argument must be a string".to_owned(),
            )),
        };

        assert!(prepared_error(std::slice::from_ref(&prepared)).is_some());
        assert!(pending_stdin.is_empty());
        let mut prepared_return = None;
        assert!(matches!(
            settle_preparation_error_return(&mut prepared_return, &mut state.pending_return),
            Some(PendingReturn::Eof { code: 0, .. })
        ));
        assert!(state.pending_return.is_none());

        let mut prepared_return = Some(PendingReturn::Blank {
            code: 1,
            preceding_input: 1,
            observed_preceding_input: false,
        });
        assert!(
            settle_preparation_error_return(&mut prepared_return, &mut state.pending_return)
                .is_none()
        );
        assert!(prepared_return.is_none());
        assert!(
            state
                .pending_return
                .as_ref()
                .is_some_and(PendingReturn::has_preceding_input)
        );

        state.pending_return = None;
        assert!(
            settle_preparation_error_return(&mut prepared_return, &mut state.pending_return)
                .is_none()
        );
        assert!(state.pending_return.is_none());
    }

    #[test]
    fn pending_return_waits_for_input_observed_before_it() {
        let mut pending_return = None;
        let mut pending_stdin = VecDeque::new();
        let mut writer = ControlWriter::new(Vec::new(), false);
        capture_pending_return(
            StdinEvent::Line("display-message -p queued".to_owned()),
            0,
            &mut pending_return,
            &mut pending_stdin,
            &mut writer,
        );
        capture_pending_return(
            StdinEvent::Line(String::new()),
            0,
            &mut pending_return,
            &mut pending_stdin,
            &mut writer,
        );

        let pending_return = pending_return.as_mut().expect("pending return");
        assert!(pending_return.has_preceding_input());
        pending_return.consume_preceding_input();
        assert!(!pending_return.has_preceding_input());
        pending_return.refresh_code_after_preceding_input(1);
        assert_eq!(pending_return.code(), 1);

        let Ok(mut in_flight) = PendingReturn::from_stdin(StdinEvent::Eof, 0, 0) else {
            panic!("EOF did not create a pending return");
        };
        in_flight.refresh_code_after_preceding_input(1);
        assert_eq!(in_flight.code(), 0);
        in_flight.observe_preceding_input();
        in_flight.refresh_code_after_preceding_input(1);
        assert_eq!(in_flight.code(), 1);
    }

    #[test]
    fn authoritative_caller_detach_discards_return_observed_while_it_waits() {
        let mut state = ControlState {
            return_code: 1,
            ..ControlState::default()
        };
        let mut deferred_return = Some(PendingReturn::Eof {
            code: 1,
            preceding_input: 0,
            observed_preceding_input: false,
        });
        settle_deferred_return(true, &mut deferred_return, &mut state);
        assert!(deferred_return.is_none());
        assert!(state.pending_return.is_none());

        deferred_return = Some(PendingReturn::Eof {
            code: 1,
            preceding_input: 0,
            observed_preceding_input: false,
        });
        settle_deferred_return(false, &mut deferred_return, &mut state);
        assert!(deferred_return.is_none());
        assert_eq!(
            state.pending_return.as_ref().map(PendingReturn::code),
            Some(1)
        );
    }

    #[test]
    fn legacy_nested_source_warning_defers_a_plain_error_without_config_error() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(21, 1).unwrap();
        let mut state = ControlState::default();
        handle_protocol(
            ProtocolMessage::Event(zz_protocol::Event {
                sequence: 1,
                payload: EventPayload::ClientMessage {
                    pane: None,
                    kind: zz_protocol::ClientMessageKind::Warning,
                    text: "No such file or directory: nested-a.conf\nNo such file or directory: nested-b.conf"
                        .to_owned(),
                },
            }),
            &mut state,
            &mut writer,
        )
        .unwrap();
        writer
            .response(
                &frame,
                CommandResponse::Success {
                    request_id: 1,
                    output: RawText::default(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();

        let output = std::str::from_utf8(&writer.output).unwrap();
        assert!(!output.contains("%config-error"));
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], "%begin 21 1 1");
        assert_eq!(lines[1], "%end 21 1 1");
        assert!(lines[2].starts_with("%begin "));
        assert_eq!(lines[3], "No such file or directory: nested-a.conf");
        assert_eq!(lines[4], "No such file or directory: nested-b.conf");
        assert_eq!(
            lines[2].strip_prefix("%begin "),
            lines[5].strip_prefix("%error ")
        );
        assert_eq!(lines.len(), 6);
    }

    #[test]
    fn typed_error_messages_use_standalone_frames_without_text_classification() {
        for text in [
            "No such file or directory: missing.conf",
            "Invalid argument: invalid.conf",
            "Cannot allocate memory: large.conf",
            "Pattern syntax error: invalid[.conf",
            "too many nested files",
            "/tmp/mux.conf:51: too many nested files",
            "Is a directory (os error 21): /tmp/a: b",
            "stream did not contain valid UTF-8: binary.conf",
            "No such file or directory: missing.conf\nstream did not contain valid UTF-8: binary.conf",
            "stream did not contain valid UTF-8: binary.conf\nNo such file or directory: missing.conf",
        ] {
            let mut writer = ControlWriter::new(Vec::new(), false);
            let mut state = ControlState::default();
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::ClientMessage {
                        pane: None,
                        kind: zz_protocol::ClientMessageKind::Error,
                        text: text.to_owned(),
                    },
                }),
                &mut state,
                &mut writer,
            )
            .unwrap();

            let lines = std::str::from_utf8(&writer.output)
                .unwrap()
                .lines()
                .collect::<Vec<_>>();
            let payload = text.lines().collect::<Vec<_>>();
            assert!(lines[0].starts_with("%begin "), "{text}");
            assert_eq!(&lines[1..=payload.len()], payload, "{text}");
            assert_eq!(
                lines[0].strip_prefix("%begin "),
                lines[payload.len() + 1].strip_prefix("%error "),
                "{text}"
            );
            assert_eq!(lines.len(), payload.len() + 2, "{text}");
        }
    }

    #[test]
    fn config_errors_route_on_their_payload_type_and_not_on_their_prose() {
        // The pin decides %config-error from where the diagnostic was raised
        // (cfg_add_cause, printed by cfg_print_causes and cfg_show_causes), not
        // from how the text reads, so every shape below routes the same way.
        for text in [
            "/tmp/mux.conf:51: unknown command: wibble",
            "skipped 1 unsupported tmux command: new-pane",
            "Is a directory (os error 21): relative-source.conf",
            "worker warning",
            "commande inconnue",
            "",
        ] {
            let mut writer = ControlWriter::new(Vec::new(), false);
            let mut state = ControlState::default();
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::ControlConfigError {
                        text: text.to_owned(),
                    },
                }),
                &mut state,
                &mut writer,
            )
            .unwrap();
            assert_eq!(writer.output, format!("%config-error {text}\n").as_bytes());
            assert_eq!(state.return_code, 0);
            assert_eq!(completed_exit_code(0, &state), 0);
        }

        // A generic warning that merely reads like a config diagnostic no longer
        // promotes itself.
        for text in [
            "/tmp/mux.conf:51: unknown command: wibble",
            "skipped 1 unsupported tmux command: new-pane",
            "stream did not contain valid UTF-8: binary.conf",
            "worker warning",
        ] {
            let mut writer = ControlWriter::new(Vec::new(), false);
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::ClientMessage {
                        pane: None,
                        kind: zz_protocol::ClientMessageKind::Warning,
                        text: text.to_owned(),
                    },
                }),
                &mut ControlState::default(),
                &mut writer,
            )
            .unwrap();
            assert!(writer.output.is_empty(), "{text}");
        }
    }

    #[test]
    fn separate_nested_source_warnings_defer_separate_error_frames() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(17, 1).unwrap();
        writer
            .diagnostic_error_at(18, "No such file or directory: first.conf")
            .unwrap();
        writer
            .diagnostic_error_at(19, "No such file or directory: second.conf")
            .unwrap();
        writer
            .response(
                &frame,
                CommandResponse::Success {
                    request_id: 1,
                    output: RawText::default(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        assert_eq!(
            writer.output,
            b"%begin 17 1 1\n%end 17 1 1\n%begin 18 2 1\nNo such file or directory: first.conf\n%error 18 2 1\n%begin 19 3 1\nNo such file or directory: second.conf\n%error 19 3 1\n"
        );
    }

    #[test]
    fn parser_distinguishes_return_ignores_chains_and_errors() {
        assert_eq!(
            parse_line("", &BTreeMap::new(), &BTreeMap::new()),
            ParsedLine::Return
        );
        assert_eq!(
            parse_line("   ", &BTreeMap::new(), &BTreeMap::new()),
            ParsedLine::Ignore
        );
        assert_eq!(
            parse_line(" # ignored", &BTreeMap::new(), &BTreeMap::new()),
            ParsedLine::Ignore
        );
        let ParsedLine::Commands(commands) =
            parse_line("ls ; list-panes", &BTreeMap::new(), &BTreeMap::new())
        else {
            panic!("semicolon chain was not parsed");
        };
        assert_eq!(
            commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            ["ls", "list-panes"]
        );
        let ParsedLine::Commands(commands) =
            parse_line("bogus-command", &BTreeMap::new(), &BTreeMap::new())
        else {
            panic!("unknown command was rejected before live preparation");
        };
        assert_eq!(commands[0].name, "bogus-command");
        let ParsedLine::Commands(commands) =
            parse_line("set 'oops", &BTreeMap::new(), &BTreeMap::new())
        else {
            panic!("open quote at EOF was rejected");
        };
        assert_eq!(commands[0].name, "set");
        assert_eq!(commands[0].args, ["oops"]);
        let ParsedLine::Commands(commands) = parse_line(
            "set-environment -g CONTROL_LITERAL $FOO",
            &BTreeMap::new(),
            &BTreeMap::new(),
        ) else {
            panic!("variable command was not parsed");
        };
        assert_eq!(
            commands[0].args,
            ["-g", "CONTROL_LITERAL", ""],
            "the pin's lexer expands an unset $NAME to nothing while the server parses the line"
        );
    }

    #[test]
    fn prepared_response_matching_ignores_stale_request_ids() {
        let stale = ProtocolMessage::PreparedCommandList {
            request_id: 8,
            commands: Vec::new(),
        };
        assert!(matches!(
            match_prepared_response(stale, 9),
            Err(ProtocolMessage::PreparedCommandList { request_id: 8, .. })
        ));
        let command = PreparedCommand {
            invocation: CommandInvocation::new("list-sessions", [] as [&str; 0]),
            canonical_name: Some("list-sessions".to_owned()),
            alias_matched: false,
            result: PreparedCommandResult::Ready,
        };
        assert_eq!(
            match_prepared_response(
                ProtocolMessage::PreparedCommandList {
                    request_id: 9,
                    commands: vec![command.clone()],
                },
                9,
            )
            .unwrap(),
            [command]
        );
    }

    #[test]
    fn expansion_answers_are_matched_by_kind_and_request_id() {
        let homes = PendingExpansion {
            variables: false,
            request_id: 9,
        };
        let variables = PendingExpansion {
            variables: true,
            request_id: 9,
        };
        let home_answer = ProtocolMessage::HomeDirectoryResponse {
            request_id: 9,
            homes: vec![Some("/server/home".to_owned())],
        };
        let variable_answer = ProtocolMessage::EnvironmentResponse {
            request_id: 9,
            values: vec![Some("EXPANDED".to_owned())],
        };
        assert!(homes.answers(&home_answer));
        assert!(!homes.answers(&variable_answer));
        assert!(variables.answers(&variable_answer));
        assert!(!variables.answers(&home_answer));
        assert!(!homes.answers(&ProtocolMessage::HomeDirectoryResponse {
            request_id: 8,
            homes: Vec::new(),
        }));
        assert!(!variables.answers(&ProtocolMessage::EnvironmentResponse {
            request_id: 8,
            values: Vec::new(),
        }));
        assert_eq!(
            expansion_answer(home_answer),
            [Some("/server/home".to_owned())]
        );
        assert_eq!(
            expansion_answer(variable_answer),
            [Some("EXPANDED".to_owned())]
        );
    }

    /// Derived from pinned tmux d77c9dc6. `yylex_token_variable` expands a
    /// `$NAME` on a control line through `environ_find(global_environ, name)`
    /// in the server, so the value an earlier `set-environment -g` stored
    /// reaches the next line, an unset name expands to nothing, and a
    /// single-quoted `\'$NAME\'` stays literal. Measured over `-C` on the pin:
    /// `set-environment -g NOTIFY_ENV EXPANDED` then `display-message -p
    /// "$NOTIFY_ENV"` prints `EXPANDED`, `display-message -p "$NOPE_UNSET"`
    /// prints an empty line, and `display-message -p \'$NOTIFY_ENV\'` prints
    /// `$NOTIFY_ENV`.
    #[test]
    fn control_lines_expand_variables_from_the_daemon_global_environment() {
        let variables = BTreeMap::from([("NOTIFY_ENV".to_owned(), "EXPANDED".to_owned())]);
        let ParsedLine::Commands(commands) = parse_line(
            "display-message -p \"$NOTIFY_ENV\" $NOTIFY_ENV '$NOTIFY_ENV' \"$NOPE_UNSET\"",
            &BTreeMap::new(),
            &variables,
        ) else {
            panic!("variable line was not parsed");
        };
        assert_eq!(
            commands[0].args,
            ["-p", "EXPANDED", "EXPANDED", "$NOTIFY_ENV", ""]
        );
    }

    #[test]
    fn control_lines_expand_tildes_from_the_daemon_answer() {
        let homes = BTreeMap::from([
            (String::new(), "/server/home".to_owned()),
            ("alice".to_owned(), "/users/alice".to_owned()),
        ]);
        let ParsedLine::Commands(commands) = parse_line(
            "display-message -p ~ ~/x ~alice/y $UNSET",
            &homes,
            &BTreeMap::new(),
        ) else {
            panic!("tilde line was not parsed");
        };
        assert_eq!(
            commands[0].args,
            ["-p", "/server/home", "/server/home/x", "/users/alice/y", ""]
        );
        assert_eq!(
            parse_line("display-message -p ~nobody", &homes, &BTreeMap::new()),
            ParsedLine::Error("parse error: syntax error".to_owned())
        );
        let ParsedLine::Commands(commands) =
            parse_line("display-message -p '~'", &homes, &BTreeMap::new())
        else {
            panic!("single-quoted tilde was not parsed");
        };
        assert_eq!(commands[0].args, ["-p", "~"]);
    }

    #[test]
    fn double_control_wraps_the_stream() {
        let mut writer = ControlWriter::new(Vec::new(), true);
        writer.start().unwrap();
        writer.emit_exit(None).unwrap();
        writer.finish().unwrap();
        assert_eq!(writer.output, b"\x1bP1000p%exit\n\x1b\\");
    }

    #[test]
    fn notifications_defer_while_a_block_is_open() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        writer.notify(b"%sessions-changed").unwrap();
        let frame = writer.begin_at(21, 1).unwrap();
        writer.notify(b"%window-add @3").unwrap();
        writer.notify(b"%window-renamed @3 shell").unwrap();
        writer
            .response(
                &frame,
                CommandResponse::Success {
                    request_id: 5,
                    output: "body\n".into(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        writer.notify(b"%session-renamed $1 dev").unwrap();
        assert_eq!(
            writer.output,
            b"%sessions-changed\n%begin 21 1 1\nbody\n%end 21 1 1\n%window-add @3\n%window-renamed @3 shell\n%session-renamed $1 dev\n"
        );
    }

    #[test]
    fn blank_and_eof_discard_queued_and_future_pane_output_only() {
        for stdin in [StdinEvent::Line(String::new()), StdinEvent::Eof] {
            let pane = zz_protocol::PaneId(7);
            let mut state = ControlState::default();
            let mut writer = ControlWriter::new(Vec::new(), false);
            let event = |sequence, payload| {
                ProtocolMessage::Event(zz_protocol::Event { sequence, payload })
            };

            handle_protocol(
                event(
                    1,
                    EventPayload::PaneOutput {
                        pane,
                        bytes: b"before".to_vec(),
                    },
                ),
                &mut state,
                &mut writer,
            )
            .unwrap();
            let frame = writer.begin_at(21, 1).unwrap();
            for (sequence, payload) in [
                (
                    2,
                    EventPayload::PaneOutput {
                        pane,
                        bytes: b"queued".to_vec(),
                    },
                ),
                (
                    3,
                    EventPayload::PaneOutputAged {
                        pane,
                        age_ms: 42,
                        bytes: b"queued-aged".to_vec(),
                    },
                ),
                (4, EventPayload::PaneOutputState { pane, paused: true }),
            ] {
                handle_protocol(event(sequence, payload), &mut state, &mut writer).unwrap();
            }
            writer.notify(b"%window-add @3").unwrap();
            writer.diagnostic_error_at(22, "diagnostic").unwrap();
            writer
                .control_command_guard_at(23, "guard", false, 0)
                .unwrap();
            writer
                .control_source_file(ControlSourceFileEvent::ReadError("source".to_owned()))
                .unwrap();
            writer.control_command_output("command").unwrap();

            let mut pending_return = None;
            let mut pending_stdin = VecDeque::new();
            capture_pending_return(
                stdin,
                0,
                &mut pending_return,
                &mut pending_stdin,
                &mut writer,
            );
            for (sequence, payload) in [
                (
                    5,
                    EventPayload::PaneOutput {
                        pane,
                        bytes: b"after".to_vec(),
                    },
                ),
                (
                    6,
                    EventPayload::PaneOutputAged {
                        pane,
                        age_ms: 84,
                        bytes: b"after-aged".to_vec(),
                    },
                ),
                (
                    7,
                    EventPayload::PaneOutputState {
                        pane,
                        paused: false,
                    },
                ),
            ] {
                handle_protocol(event(sequence, payload), &mut state, &mut writer).unwrap();
            }
            writer
                .response(
                    &frame,
                    CommandResponse::Success {
                        request_id: 1,
                        output: "body\n".into(),
                        exit_code: 0,
                        stderr: String::new(),
                        stdout_claim: StdoutClaim::None,
                    },
                )
                .unwrap();
            writer.emit_exit(None).unwrap();

            assert!(pending_return.is_some());
            assert!(pending_stdin.is_empty());
            assert_eq!(
                writer.output,
                b"%output %7 before\n\
                  %begin 21 1 1\nbody\n%end 21 1 1\n\
                  %pause %7\n%window-add @3\n\
                  %begin 22 2 1\ndiagnostic\n%error 22 2 1\n\
                  %begin 23 3 0\nguard\n%end 23 3 0\n\
                  source\ncommand\n%continue %7\n%exit\n"
            );
        }
    }

    #[test]
    fn input_error_keeps_pane_output_enabled() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(21, 1).unwrap();
        writer.pane_output(b"%output %7 queued").unwrap();
        let mut pending_return = None;
        let mut pending_stdin = VecDeque::new();
        capture_pending_return(
            StdinEvent::Error("input failed".to_owned()),
            0,
            &mut pending_return,
            &mut pending_stdin,
            &mut writer,
        );
        writer
            .pane_output(b"%extended-output %7 42 : after")
            .unwrap();
        writer.end(&frame, false).unwrap();

        assert!(matches!(
            pending_return,
            Some(PendingReturn::InputError { .. })
        ));
        assert_eq!(
            writer.output,
            b"%begin 21 1 1\n%end 21 1 1\n\
              %output %7 queued\n%extended-output %7 42 : after\n"
        );
    }

    #[test]
    fn control_command_output_follows_the_guard_raw_and_command_mode_stays_inside() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let mut state = ControlState::default();
        let shell = writer.begin_at(21, 1).unwrap();
        writer.notify(b"%window-renamed @3 shell").unwrap();
        handle_protocol(
            ProtocolMessage::Event(zz_protocol::Event {
                sequence: 1,
                payload: EventPayload::ControlCommandOutput {
                    output: "child output\n%begin-fake\n%exit\n'exit 3' returned 3\npartial"
                        .to_owned(),
                },
            }),
            &mut state,
            &mut writer,
        )
        .unwrap();
        writer
            .response(
                &shell,
                CommandResponse::Success {
                    request_id: 5,
                    output: RawText::default(),
                    exit_code: 3,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        let command_mode = writer.begin_at(22, 1).unwrap();
        writer
            .response(
                &command_mode,
                CommandResponse::Success {
                    request_id: 6,
                    output: "command mode".into(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();
        assert_eq!(
            writer.output,
            b"%begin 21 1 1\n%end 21 1 1\n%window-renamed @3 shell\n\
              child output\n%begin-fake\n%exit\n'exit 3' returned 3\npartial\n\
              %begin 22 2 1\ncommand mode\n%end 22 2 1\n"
        );
    }

    #[test]
    fn startup_config_causes_are_immediate_and_prefix_each_element_once() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let mut state = ControlState::default();
        let event = |sequence, causes| {
            ProtocolMessage::Event(zz_protocol::Event {
                sequence,
                payload: EventPayload::StartupConfigCauses { causes },
            })
        };

        assert_eq!(
            handle_protocol(
                event(1, vec!["first\ncontinued".to_owned(), "second".to_owned()],),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::None
        );
        let frame = writer.begin_at(21, 1).unwrap();
        writer.notify(b"%window-add @3").unwrap();
        assert_eq!(
            handle_protocol(
                event(2, vec!["inside\nstill inside".to_owned()]),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::None
        );
        writer
            .response(
                &frame,
                CommandResponse::Success {
                    request_id: 5,
                    output: "body\n".into(),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                },
            )
            .unwrap();

        assert_eq!(
            writer.output,
            b"%config-error first\ncontinued\n%config-error second\n\
              %begin 21 1 1\n%config-error inside\nstill inside\nbody\n\
              %end 21 1 1\n%window-add @3\n"
        );
    }

    #[test]
    fn window_notifications_use_live_membership_instead_of_the_event_session() {
        let mut state = layout_notification_state();
        let variables = BTreeMap::from([
            ("hook_session".to_owned(), SessionId(2).to_string()),
            ("hook_window".to_owned(), WindowId(3).to_string()),
            ("hook_window_name".to_owned(), "shell".to_owned()),
        ]);
        for (hook, expected) in [
            ("window-linked", "%window-add @3"),
            ("window-unlinked", "%window-close @3"),
            ("window-renamed", "%window-renamed @3 shell"),
        ] {
            assert_eq!(
                render_hook(&state, hook, &variables).as_deref(),
                Some(expected)
            );
        }
        state.snapshot.sessions[0].windows.clear();
        for (hook, expected) in [
            ("window-linked", "%unlinked-window-add @3"),
            ("window-unlinked", "%unlinked-window-close @3"),
            ("window-renamed", "%unlinked-window-renamed @3 shell"),
        ] {
            assert_eq!(
                render_hook(&state, hook, &variables).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn layout_notifications_ignore_windows_only_linked_to_another_session() {
        let mut state = layout_notification_state();
        state.attached_session = Some(SessionId(2));
        let variables = BTreeMap::from([("hook_window".to_owned(), "@3".to_owned())]);
        assert!(render_hook(&state, "window-layout-changed", &variables).is_none());
    }

    #[test]
    fn output_escaping_preserves_del_and_raw_eight_bit_bytes() {
        assert_eq!(
            render_pane_output(zz_protocol::PaneId(7), &[0, 0x1f, b'\\', 0x7f, 0x80, 0xff]),
            b"%output %7 \\000\\037\\134\x7f\x80\xff"
        );
        assert_eq!(
            render_pane_output_aged(
                zz_protocol::PaneId(7),
                42,
                &[0, 0x1f, b'\\', 0x7f, 0x80, 0xff]
            ),
            b"%extended-output %7 42 : \\000\\037\\134\x7f\x80\xff"
        );
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(22, 1).unwrap();
        writer.notify(&[b'%', 0xff]).unwrap();
        writer.end(&frame, false).unwrap();
        assert_eq!(writer.output, b"%begin 22 1 1\n%end 22 1 1\n%\xff\n");
    }

    #[test]
    fn pane_output_state_and_flags_follow_the_control_event_channel() {
        let pane = zz_protocol::PaneId(7);
        let mut state = ControlState::default();
        let mut writer = ControlWriter::new(Vec::new(), false);
        for payload in [
            EventPayload::PaneOutputState { pane, paused: true },
            EventPayload::PaneOutputState {
                pane,
                paused: false,
            },
            EventPayload::ControlFlags {
                wait_exit: true,
                pause_after_ms: Some(1000),
                no_output: false,
                new_layouts: true,
            },
        ] {
            assert_eq!(
                handle_protocol(
                    ProtocolMessage::Event(zz_protocol::Event {
                        sequence: 1,
                        payload,
                    }),
                    &mut state,
                    &mut writer,
                )
                .unwrap(),
                ExitSignal::None
            );
        }
        assert!(state.wait_exit);
        assert!(state.new_layouts);
        assert_eq!(writer.output, b"%pause %7\n%continue %7\n");
    }

    #[test]
    fn detach_and_server_stop_keep_distinct_exit_signals() {
        let session = SessionId(7);
        let mut state = ControlState {
            attached_session: Some(session),
            ..ControlState::default()
        };
        let mut writer = ControlWriter::new(Vec::new(), false);
        assert_eq!(
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::detached_requested(session, None),
                }),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::Detached
        );
        assert_eq!(state.attached_session, None);

        let mut writer = ControlWriter::new(Vec::new(), false);
        assert_eq!(
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 2,
                    payload: EventPayload::ServerStopping,
                }),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::Clean
        );
        writer
            .control_command_guard_at(3, "late child", false, 0)
            .unwrap();
        writer.emit_exit(None).unwrap();
        assert_eq!(
            writer.output,
            b"%exit\n%begin 3 1 0\nlate child\n%end 3 1 0\n"
        );
    }

    #[test]
    fn logical_command_holds_server_exit_until_child_events_finish() {
        let mut state = ControlState::default();
        let mut writer = ControlWriter::new(Vec::new(), false);
        writer.hold_exit();
        writer
            .control_command_guard_at(1, "source child one", false, 0)
            .unwrap();
        writer
            .control_command_guard_at(2, "source child two", false, 0)
            .unwrap();
        assert_eq!(
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 3,
                    payload: EventPayload::ServerStopping,
                }),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::Clean
        );
        assert!(!writer.output.ends_with(b"%exit\n"));
        writer
            .control_command_guard_at(4, "blocked child", false, 0)
            .unwrap();
        writer.notify(b"%sessions-changed").unwrap();
        writer.release_exit().unwrap();
        writer.emit_exit(None).unwrap();
        assert_eq!(
            writer.output,
            b"%begin 1 1 0\nsource child one\n%end 1 1 0\n\
              %begin 2 2 0\nsource child two\n%end 2 2 0\n\
              %begin 4 3 0\nblocked child\n%end 4 3 0\n\
              %sessions-changed\n%exit\n"
        );
    }

    #[test]
    fn clean_control_exit_releases_only_the_initiating_alias() {
        let mut state = ControlState::default();
        let mut writer = ControlWriter::new(Vec::new(), false);
        writer.hold_exit();
        assert_eq!(
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 1,
                    payload: EventPayload::ControlExit {
                        reason: "unknown".to_owned(),
                    },
                }),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::None
        );
        writer
            .control_command_guard_at(2, "kill child", false, 0)
            .unwrap();
        assert_eq!(
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 3,
                    payload: EventPayload::ControlExit {
                        reason: String::new(),
                    },
                }),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::Clean
        );
        assert_eq!(
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 4,
                    payload: EventPayload::ServerStopping,
                }),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::Clean
        );
        writer
            .control_command_guard_at(5, "late child", false, 0)
            .unwrap();
        assert_eq!(
            writer.output,
            b"%begin 2 1 0\nkill child\n%end 2 1 0\n%exit\n\
              %begin 5 2 0\nlate child\n%end 5 2 0\n"
        );
    }

    #[test]
    fn open_guard_holds_server_exit_until_deferred_children_finish() {
        let mut state = ControlState::default();
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(4, 1).unwrap();
        assert_eq!(
            handle_protocol(
                ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 3,
                    payload: EventPayload::ServerStopping,
                }),
                &mut state,
                &mut writer,
            )
            .unwrap(),
            ExitSignal::Clean
        );
        assert_eq!(writer.output, b"%begin 4 1 1\n");
        writer
            .control_command_guard_at(5, "child", false, 0)
            .unwrap();
        writer.emit_exit(None).unwrap();
        writer.end(&frame, false).unwrap();
        writer.emit_exit(None).unwrap();
        assert_eq!(
            writer.output,
            b"%begin 4 1 1\n%end 4 1 1\n%begin 5 2 0\nchild\n%end 5 2 0\n%exit\n"
        );
    }

    #[test]
    fn subscription_changes_render_the_three_pin_shapes_byte_exact() {
        let session = SessionId(1);
        let window = WindowId(2);
        let pane = zz_protocol::PaneId(3);
        assert_eq!(
            render_subscription_changed(
                "pane-watch",
                session,
                Some(window),
                Some(4),
                Some(pane),
                "one\ntwo",
            ),
            "%subscription-changed pane-watch $1 @2 4 %3 : one\ntwo"
        );
        assert_eq!(
            render_subscription_changed(
                "window-watch",
                session,
                Some(window),
                Some(4),
                None,
                "value",
            ),
            "%subscription-changed window-watch $1 @2 4 - : value"
        );
        assert_eq!(
            render_subscription_changed("session-watch", session, None, None, None, "value"),
            "%subscription-changed session-watch $1 - - - : value"
        );
    }

    #[test]
    fn exit_drain_keeps_wait_exit_acknowledgements() {
        let (sender, receiver) = mpsc::sync_channel(32);
        sender
            .send(MainEvent::Stdin(StdinEvent::Line(String::new())))
            .unwrap();
        sender.send(MainEvent::Stdin(StdinEvent::Eof)).unwrap();
        drop(sender);
        let mut receiver = ControlReceiver::new(receiver);
        let mut pending = VecDeque::new();
        let mut state = ControlState::default();
        let mut output = ControlWriter::new(Vec::new(), false);
        drain_before_exit(&mut receiver, &mut state, &mut output, &mut pending).unwrap();
        assert_eq!(pending.len(), 2);
        wait_for_exit_input(&mut receiver, &mut pending);
        assert!(matches!(pending.pop_front(), Some(StdinEvent::Eof)));
    }

    #[test]
    fn wait_exit_releases_on_empty_line_and_eof() {
        let (_sender, receiver) = mpsc::sync_channel(32);
        let mut receiver = ControlReceiver::new(receiver);
        let mut pending = VecDeque::from([
            StdinEvent::Line("keep draining".to_owned()),
            StdinEvent::Line(String::new()),
        ]);
        wait_for_exit_input(&mut receiver, &mut pending);
        assert!(pending.is_empty());

        let mut pending = VecDeque::from([StdinEvent::Eof]);
        wait_for_exit_input(&mut receiver, &mut pending);
        assert!(pending.is_empty());
    }

    fn layout_notification_state() -> ControlState {
        let pane = zz_protocol::PaneId(5);
        let window = zz_protocol::WindowSnapshot {
            id: WindowId(3),
            index: 0,
            name: "shell".to_owned(),
            automatic_rename: true,
            active_pane: pane,
            zoomed_pane: Some(pane),
            layout: zz_protocol::LayoutNode::Pane(pane),
            panes: BTreeMap::from([(
                pane,
                zz_protocol::PaneSnapshot {
                    id: pane,
                    title: "shell".to_owned(),
                    kind: zz_protocol::PaneKindSnapshot::Terminal,
                    synchronized_input: false,
                    bell: true,
                    dead: false,
                    dead_status: None,
                    border_colour: None,
                    active_border_colour: None,
                    border_status_text: String::new(),
                    mode: None,
                    status: None,
                },
            )]),
            layout_dump: "abcd,80x24,0,0,5".to_owned(),
            visible_layout_dump: "ef01,80x24,0,0,5".to_owned(),
            status_label: String::new(),
            activity: false,
            silence: false,
            pane_border_status: zz_protocol::PaneBorderStatus::Off,
            pane_border_lines: zz_protocol::PaneBorderLines::Single,
            pane_border_indicators: zz_protocol::PaneBorderIndicators::Colour,
            pane_order: Vec::new(),
            pane_z_order: Vec::new(),
        };
        let mut state = ControlState::default();
        state.attach(
            SessionId(1),
            MuxSnapshot {
                generation: 1,
                sessions: vec![zz_protocol::SessionSnapshot {
                    id: SessionId(1),
                    name: "work".to_owned(),
                    active_window: window.id,
                    windows: vec![window],
                    viewers: vec![zz_protocol::SessionViewer {
                        name: "device-9".to_owned(),
                        window: WindowId(3),
                        is_self: true,
                    }],
                }],
                focused_window: Some(WindowId(3)),
            },
        );
        state.last_windows.insert(SessionId(1), WindowId(3));
        state
    }

    #[test]
    fn layout_notifications_use_snapshot_dumps_and_raw_flag_order() {
        let state = layout_notification_state();
        let variables = BTreeMap::from([("hook_window".to_owned(), "@3".to_owned())]);
        assert_eq!(
            render_hook(&state, "window-layout-changed", &variables).as_deref(),
            Some("%layout-change @3 abcd,80x24,0,0,5 ef01,80x24,0,0,5 !*-Z")
        );
    }

    #[test]
    fn layout_notifications_print_v1_until_the_client_sets_new_layouts() {
        let mut state = layout_notification_state();
        let layout = r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"p","w":39,"h":24,"x":41,"y":0,"a":true,"i":1,"I":"%5"}]}}"#;
        let visible = r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":1,"I":"%5"}}"#;
        let window = &mut state.snapshot.sessions[0].windows[0];
        window.layout_dump = layout.to_owned();
        window.visible_layout_dump = visible.to_owned();
        let variables = BTreeMap::from([("hook_window".to_owned(), "@3".to_owned())]);
        assert_eq!(
            render_hook(&state, "window-layout-changed", &variables).as_deref(),
            Some(
                "%layout-change @3 8207,80x24,0,0{40x24,0,0,0,39x24,41,0,5} b262,80x24,0,0,5 !*-Z"
            )
        );
        state.new_layouts = true;
        assert_eq!(
            render_hook(&state, "window-layout-changed", &variables),
            Some(format!("%layout-change @3 {layout} {visible} !*-Z"))
        );
    }

    #[test]
    fn layout_notifications_reduce_each_compact_flag_change() {
        for (kind, expected) in [
            ("activity", "#"),
            ("bell", "!"),
            ("silence", "~"),
            ("zoom", "Z"),
            ("last", "-"),
            ("current", "*"),
        ] {
            let mut state = layout_notification_state();
            let mut before = state.tree.clone();
            let session = &mut before.sessions[0];
            session.active_window = if kind == "last" {
                WindowId(3)
            } else {
                WindowId(7)
            };
            let window = &mut session.windows[0];
            window.activity = false;
            window.silence = false;
            window.zoomed_pane = None;
            window.panes.get_mut(&zz_protocol::PaneId(5)).unwrap().bell = false;
            state.last_windows.clear();
            state.attach(SessionId(1), before.clone());
            state.last_windows.clear();
            let mut after = before.clone();
            after.generation += 1;
            let session = &mut after.sessions[0];
            let window = &mut session.windows[0];
            match kind {
                "activity" => window.activity = true,
                "bell" => window.panes.get_mut(&zz_protocol::PaneId(5)).unwrap().bell = true,
                "silence" => window.silence = true,
                "zoom" => window.zoomed_pane = Some(zz_protocol::PaneId(5)),
                "last" => session.active_window = WindowId(7),
                "current" => session.active_window = WindowId(3),
                _ => unreachable!(),
            }
            let delta = zz_protocol::TreeDelta::between(&before, &after);
            let mut output = ControlWriter::new(Vec::new(), false);
            for payload in [
                EventPayload::TreeDelta(delta),
                EventPayload::HookEvent {
                    name: "window-layout-changed".to_owned(),
                    variables: BTreeMap::from([("hook_window".to_owned(), "@3".to_owned())]),
                },
            ] {
                let encoded = zz_protocol::encode_protocol_message(&ProtocolMessage::Event(
                    zz_protocol::Event {
                        sequence: 1,
                        payload,
                    },
                ))
                .unwrap();
                let message = zz_protocol::decode_protocol_frame(&encoded).unwrap();
                handle_protocol(message, &mut state, &mut output).unwrap();
            }
            assert_eq!(
                std::str::from_utf8(&output.output).unwrap(),
                format!("%layout-change @3 abcd,80x24,0,0,5 ef01,80x24,0,0,5 {expected}\n"),
                "{kind}",
            );
        }
    }

    #[test]
    fn blank_and_eof_suppress_layout_notifications_while_live_clients_keep_them() {
        for stdin in [StdinEvent::Line(String::new()), StdinEvent::Eof] {
            let mut draining_state = layout_notification_state();
            let mut live_state = layout_notification_state();
            let mut draining = ControlWriter::new(Vec::new(), false);
            let mut live = ControlWriter::new(Vec::new(), false);
            let mut pending_return = None;
            let mut pending_stdin = VecDeque::new();
            capture_pending_return(
                stdin,
                0,
                &mut pending_return,
                &mut pending_stdin,
                &mut draining,
            );
            let mut snapshot = live_state.snapshot.clone();
            let window = &mut snapshot.sessions[0].windows[0];
            window.layout_dump = "aafd,120x40,0,0,0".to_owned();
            window.visible_layout_dump = "aafd,120x40,0,0,0".to_owned();
            for (state, writer) in [
                (&mut draining_state, &mut draining),
                (&mut live_state, &mut live),
            ] {
                for (sequence, payload) in [
                    (1, EventPayload::Snapshot(snapshot.clone())),
                    (
                        2,
                        EventPayload::HookEvent {
                            name: "window-layout-changed".to_owned(),
                            variables: BTreeMap::from([(
                                "hook_window".to_owned(),
                                "@3".to_owned(),
                            )]),
                        },
                    ),
                ] {
                    handle_protocol(
                        ProtocolMessage::Event(zz_protocol::Event { sequence, payload }),
                        state,
                        writer,
                    )
                    .unwrap();
                }
            }
            assert!(pending_return.is_some());
            assert!(draining.output.is_empty());
            assert_eq!(
                draining_state.snapshot.sessions[0].windows[0].layout_dump,
                "aafd,120x40,0,0,0"
            );
            assert_eq!(
                live.output,
                b"%layout-change @3 aafd,120x40,0,0,0 aafd,120x40,0,0,0 !*-Z\n"
            );
        }
    }

    #[test]
    fn message_escaping_is_separate_from_output_escaping() {
        assert_eq!(
            render_message("a\\b\t\n\r\u{7}\u{1b}é"),
            b"a\\b\t\n\\r\\a\\033\xc3\xa9"
        );
    }

    #[test]
    fn daemon_source_read_failures_keep_their_own_channel() {
        let root = std::env::temp_dir();
        for text in [
            "No such file or directory: /tmp/mux.conf".to_owned(),
            "Invalid argument: /tmp/mux.conf".to_owned(),
            "Cannot allocate memory: /tmp/mux.conf".to_owned(),
            "Pattern syntax error: /tmp/[".to_owned(),
            "too many nested files".to_owned(),
            format!(
                "Is a directory (os error 21): {}: b",
                root.join("a").display()
            ),
            format!(
                "stream did not contain valid UTF-8: {}",
                root.join("binary.conf").display()
            ),
        ] {
            assert!(is_source_error_message(&text), "{text}");
        }
        assert!(!is_source_error_message(
            "/tmp/mux.conf:51: too many nested files"
        ));
        for text in [
            "stream did not contain valid UTF-8: binary.conf",
            "worker warning (os error 21)",
            "No such file or directory: missing.conf\nstream did not contain valid UTF-8: binary.conf",
        ] {
            assert!(!is_source_error_message(text), "{text}");
        }
    }

    #[test]
    fn ready_control_response_shares_a_flush_and_waiting_exposes_begin() {
        struct Recorded {
            bytes: Vec<u8>,
            flushes: usize,
            flushed: Option<mpsc::Sender<()>>,
        }
        impl Write for Recorded {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                if let Some(flushed) = self.flushed.take() {
                    flushed.send(()).unwrap();
                }
                Ok(())
            }
        }
        let mut writer = ControlWriter::new(
            Recorded {
                bytes: Vec::new(),
                flushes: 0,
                flushed: None,
            },
            false,
        );
        let frame = writer.begin_buffered_at(21, 1).unwrap();
        let (sender, receiver) = mpsc::channel();
        sender
            .send(MainEvent::Protocol(Box::new(
                ProtocolMessage::CommandResponse(CommandResponse::Success {
                    request_id: 1,
                    output: RawText::from("body"),
                    exit_code: 0,
                    stderr: String::new(),
                    stdout_claim: StdoutClaim::None,
                }),
            )))
            .unwrap();
        let mut receiver = ControlReceiver::new(receiver);
        let MainEvent::Protocol(message) =
            receive_buffered_control_event(&mut receiver, &mut writer).unwrap()
        else {
            panic!("response expected");
        };
        let ProtocolMessage::CommandResponse(response) = *message else {
            panic!("response expected");
        };
        assert_eq!(writer.output.flushes, 0);
        writer.response(&frame, response).unwrap();
        assert_eq!(writer.output.flushes, 1);
        assert_eq!(writer.output.bytes, b"%begin 21 1 1\nbody\n%end 21 1 1\n");

        let (flushed, flush_receiver) = mpsc::channel();
        writer.output.flushed = Some(flushed);
        writer.begin_buffered_at(22, 1).unwrap();
        let producer = thread::spawn(move || {
            flush_receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            sender.send(MainEvent::Disconnected).unwrap();
        });
        assert!(matches!(
            receive_buffered_control_event(&mut receiver, &mut writer).unwrap(),
            MainEvent::Disconnected
        ));
        producer.join().unwrap();
        assert_eq!(writer.output.flushes, 2);
        assert!(writer.output.bytes.ends_with(b"%begin 22 2 1\n"));
    }

    #[test]
    fn dropping_a_double_writer_without_exit_still_terminates_the_dcs() {
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buffer);
                Ok(buffer.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = ControlWriter::new(Shared(std::sync::Arc::clone(&output)), true);
        writer.start().unwrap();
        drop(writer);
        assert_eq!(*output.lock().unwrap(), b"\x1bP1000p\x1b\\");
    }
}
