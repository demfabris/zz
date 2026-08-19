use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, BufRead as _, IsTerminal as _, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, mpsc},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use zz_daemon::InteractiveClient;
use zz_protocol::{
    CommandInvocation, CommandResponse, EventPayload, MuxSnapshot, ProtocolMessage, ServerError,
    SessionId, WindowId,
};

use super::{
    SocketSelectionSource, connect_or_spawn_daemon, format_local_command_error,
    tmux_command_starts_server, tmux_label_creation_error,
};

const DCS: &[u8] = b"\x1bP1000p";
const ST: &[u8] = b"\x1b\\";

pub(crate) fn run(
    socket_path: &Path,
    socket_source: SocketSelectionSource,
    mux_config_files: &[PathBuf],
    no_start_server: bool,
    level: u8,
    arguments: Vec<String>,
) -> ExitCode {
    let mut arguments = arguments.into_iter();
    let name = arguments.next().unwrap_or_else(|| "new-session".to_owned());
    let command = CommandInvocation::new(name, arguments);
    let start_server = !no_start_server && tmux_command_starts_server(&command.name);
    if let Some(error) = tmux_label_creation_error(socket_path, socket_source, start_server) {
        eprintln!("{}", error.message);
        return ExitCode::FAILURE;
    }
    let client = if start_server {
        connect_or_spawn_daemon(
            socket_path,
            None,
            mux_config_files,
            || InteractiveClient::connect_control(socket_path),
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
    let terminal = match ControlTerminal::enter(level >= 2) {
        Ok(terminal) => terminal,
        Err(error) => {
            eprintln!("zz: {error}");
            return ExitCode::FAILURE;
        }
    };
    let stdout = io::stdout();
    let mut output = ControlWriter::new(stdout.lock(), level >= 2);
    let result = output
        .start()
        .and_then(|()| drive(&client, command, &mut output));
    drop(terminal);
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("zz: {error}");
            ExitCode::FAILURE
        }
    }
}

fn drive<W: Write>(
    client: &Arc<InteractiveClient>,
    initial: CommandInvocation,
    output: &mut ControlWriter<W>,
) -> io::Result<u8> {
    let (events, receiver) = mpsc::sync_channel(32);
    spawn_protocol_reader(Arc::clone(client), events.clone());
    let mut stdin_started = false;
    if let Some(error) = unknown_command(&initial.name) {
        output.write_line(&error)?;
        finish_exit(
            output,
            None,
            false,
            false,
            &events,
            &mut stdin_started,
            &receiver,
            &mut VecDeque::new(),
        )?;
        return Ok(1);
    }

    let mut state = ControlState::default();
    let mut pending_stdin = VecDeque::new();
    let initial_result = execute_command(
        client.as_ref(),
        &receiver,
        output,
        initial,
        0,
        &mut state,
        &mut pending_stdin,
    )?;
    if initial_result.exit.is_some() {
        finish_exit(
            output,
            initial_result.exit.reason(),
            state.wait_exit,
            false,
            &events,
            &mut stdin_started,
            &receiver,
            &mut pending_stdin,
        )?;
        return Ok(initial_result.exit_code);
    }
    if initial_result.exit_code != 0 || state.attached_session.is_none() {
        finish_exit(
            output,
            None,
            state.wait_exit,
            false,
            &events,
            &mut stdin_started,
            &receiver,
            &mut pending_stdin,
        )?;
        return Ok(initial_result.exit_code);
    }

    ensure_stdin_reader(&events, &mut stdin_started);
    loop {
        let event = pending_stdin.pop_front().map_or_else(
            || receiver.recv().unwrap_or(MainEvent::Disconnected),
            MainEvent::Stdin,
        );
        match event {
            MainEvent::Stdin(StdinEvent::Line(line)) => match parse_line(&line) {
                ParsedLine::Detach => {
                    let _ = client.detach();
                    drain_before_exit(&receiver, &mut state, output)?;
                    finish_exit(
                        output,
                        None,
                        state.wait_exit,
                        false,
                        &events,
                        &mut stdin_started,
                        &receiver,
                        &mut pending_stdin,
                    )?;
                    return Ok(0);
                }
                ParsedLine::Ignore => {}
                ParsedLine::Error(error) => output.parse_error(&error)?,
                ParsedLine::Commands(commands) => {
                    for command in commands {
                        let result = execute_command(
                            client.as_ref(),
                            &receiver,
                            output,
                            command,
                            1,
                            &mut state,
                            &mut pending_stdin,
                        )?;
                        if result.exit.is_some() {
                            finish_exit(
                                output,
                                result.exit.reason(),
                                state.wait_exit,
                                false,
                                &events,
                                &mut stdin_started,
                                &receiver,
                                &mut pending_stdin,
                            )?;
                            return Ok(result.exit_code);
                        }
                        if result.abort_line {
                            break;
                        }
                    }
                }
            },
            MainEvent::Stdin(StdinEvent::Eof) => {
                let _ = client.detach();
                drain_before_exit(&receiver, &mut state, output)?;
                finish_exit(
                    output,
                    None,
                    state.wait_exit,
                    true,
                    &events,
                    &mut stdin_started,
                    &receiver,
                    &mut pending_stdin,
                )?;
                return Ok(0);
            }
            MainEvent::Stdin(StdinEvent::Error(error)) => {
                eprintln!("zz: {error}");
                let _ = client.detach();
                finish_exit(
                    output,
                    None,
                    state.wait_exit,
                    true,
                    &events,
                    &mut stdin_started,
                    &receiver,
                    &mut pending_stdin,
                )?;
                return Ok(1);
            }
            MainEvent::Protocol(message) => {
                let exit = handle_protocol(*message, &mut state, output)?;
                if exit.is_some() {
                    finish_exit(
                        output,
                        exit.reason(),
                        state.wait_exit,
                        false,
                        &events,
                        &mut stdin_started,
                        &receiver,
                        &mut pending_stdin,
                    )?;
                    return Ok(u8::from(exit != ExitSignal::Clean));
                }
            }
            MainEvent::Disconnected => {
                finish_exit(
                    output,
                    Some("server exited unexpectedly"),
                    state.wait_exit,
                    false,
                    &events,
                    &mut stdin_started,
                    &receiver,
                    &mut pending_stdin,
                )?;
                return Ok(1);
            }
        }
    }
}

fn execute_command<W: Write>(
    client: &InteractiveClient,
    receiver: &mpsc::Receiver<MainEvent>,
    output: &mut ControlWriter<W>,
    command: CommandInvocation,
    flags: u8,
    state: &mut ControlState,
    pending_stdin: &mut VecDeque<StdinEvent>,
) -> io::Result<CommandResult> {
    let frame = output.begin(flags)?;
    let request_id = match client.execute(command) {
        Ok(request_id) => request_id,
        Err(error) => {
            output.error(&frame, &error.to_string())?;
            return Ok(CommandResult {
                exit_code: 1,
                exit: ExitSignal::Unexpected,
                abort_line: true,
            });
        }
    };
    let mut exit = ExitSignal::None;
    loop {
        match receiver.recv().unwrap_or(MainEvent::Disconnected) {
            MainEvent::Protocol(message) => match *message {
                ProtocolMessage::CommandResponse(response)
                    if response_request_id(&response) == request_id =>
                {
                    let abort_line = matches!(&response, CommandResponse::Error { .. });
                    let exit_code = output.response(&frame, response)?;
                    return Ok(CommandResult {
                        exit_code,
                        exit,
                        abort_line,
                    });
                }
                message => {
                    let signal = handle_protocol(message, state, output)?;
                    if signal == ExitSignal::TooFarBehind {
                        output.error(&frame, "too far behind")?;
                        return Ok(CommandResult {
                            exit_code: 1,
                            exit: signal,
                            abort_line: true,
                        });
                    }
                    if signal.is_some() {
                        exit = signal;
                    }
                }
            },
            MainEvent::Stdin(stdin) => pending_stdin.push_back(stdin),
            MainEvent::Disconnected => {
                output.error(&frame, "server exited unexpectedly")?;
                return Ok(CommandResult {
                    exit_code: 1,
                    exit: if exit.is_some() {
                        exit
                    } else {
                        ExitSignal::Unexpected
                    },
                    abort_line: true,
                });
            }
        }
    }
}

#[derive(Default)]
struct ControlState {
    attached_session: Option<SessionId>,
    snapshot: MuxSnapshot,
    last_windows: BTreeMap<SessionId, WindowId>,
    self_name: Option<String>,
    wait_exit: bool,
}

impl ControlState {
    fn attach(&mut self, session: SessionId, snapshot: MuxSnapshot) {
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
        self.attached_session
            .is_some_and(|session| variables.get("hook_session") == Some(&session.to_string()))
    }

    fn raw_window_flags(
        &self,
        session: &zz_protocol::SessionSnapshot,
        window: &zz_protocol::WindowSnapshot,
    ) -> String {
        let mut flags = String::new();
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
        flags
    }
}

fn handle_protocol<W: Write>(
    message: ProtocolMessage,
    state: &mut ControlState,
    output: &mut ControlWriter<W>,
) -> io::Result<ExitSignal> {
    match message {
        ProtocolMessage::Attached { session, snapshot } => state.attach(session, snapshot),
        ProtocolMessage::Event(event) => match event.payload {
            EventPayload::Snapshot(snapshot) => state.adopt_snapshot(snapshot),
            EventPayload::HookEvent { name, variables } => {
                if state.attached_session.is_some()
                    && let Some(line) = render_hook(state, &name, &variables)
                {
                    output.notify(line.as_bytes())?;
                }
            }
            EventPayload::PaneOutput { pane, bytes } => {
                output.notify(&render_pane_output(pane, &bytes))?;
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
                output.notify(&render_pane_output_aged(pane, age_ms, &bytes))?;
            }
            EventPayload::ControlFlags { wait_exit, .. } => state.wait_exit = wait_exit,
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
            EventPayload::ClientMessage { kind, text, .. }
                if kind == zz_protocol::ClientMessageKind::Warning && is_config_message(&text) =>
            {
                output.notify(format!("%config-error {text}").as_bytes())?;
            }
            EventPayload::Detached { .. } => {
                state.attached_session = None;
                return Ok(ExitSignal::Clean);
            }
            EventPayload::ServerStopping => return Ok(ExitSignal::Clean),
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
        "window-unlinked" => Some(format!("%unlinked-window-close {}", value("hook_window")?)),
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
            let window_id = value("hook_window")?;
            let (session, window) = state.window(window_id)?;
            Some(format!(
                "%layout-change {window_id} {} {} {}",
                window.layout_dump,
                window.visible_layout_dump,
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

fn is_config_message(text: &str) -> bool {
    text.starts_with("source-file ")
        || text.starts_with("no such file: ")
        || text.contains(" invalid line")
        || text.starts_with("invalid line")
        || (text.starts_with("skipped ") && text.contains("unsupported tmux command"))
        || text
            .split_once(": ")
            .and_then(|(location, _)| location.rsplit_once(':'))
            .is_some_and(|(_, line)| line.parse::<u32>().is_ok())
}

fn response_request_id(response: &CommandResponse) -> u64 {
    match response {
        CommandResponse::Success { request_id, .. } | CommandResponse::Error { request_id, .. } => {
            *request_id
        }
    }
}

fn drain_before_exit<W: Write>(
    receiver: &mpsc::Receiver<MainEvent>,
    state: &mut ControlState,
    output: &mut ControlWriter<W>,
) -> io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return Ok(());
        };
        match receiver.recv_timeout(remaining) {
            Ok(MainEvent::Protocol(message)) => {
                if handle_protocol(*message, state, output)?.is_some() {
                    return Ok(());
                }
            }
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
}

fn spawn_protocol_reader(client: Arc<InteractiveClient>, events: mpsc::SyncSender<MainEvent>) {
    let _ = thread::Builder::new()
        .name("zz-control-protocol".to_owned())
        .spawn(move || {
            loop {
                if let Ok(message) = client.recv() {
                    if events.send(MainEvent::Protocol(Box::new(message))).is_err() {
                        break;
                    }
                } else {
                    let _ = events.send(MainEvent::Disconnected);
                    break;
                }
            }
        });
}

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

fn ensure_stdin_reader(events: &mpsc::SyncSender<MainEvent>, started: &mut bool) {
    if !*started {
        spawn_stdin_reader(events.clone());
        *started = true;
    }
}

fn finish_exit<W: Write>(
    output: &mut ControlWriter<W>,
    reason: Option<&str>,
    wait_exit: bool,
    input_closed: bool,
    events: &mpsc::SyncSender<MainEvent>,
    stdin_started: &mut bool,
    receiver: &mpsc::Receiver<MainEvent>,
    pending_stdin: &mut VecDeque<StdinEvent>,
) -> io::Result<()> {
    output.emit_exit(reason)?;
    if wait_exit && !input_closed {
        ensure_stdin_reader(events, stdin_started);
        wait_for_exit_input(receiver, pending_stdin);
    }
    output.finish()
}

fn wait_for_exit_input(
    receiver: &mpsc::Receiver<MainEvent>,
    pending_stdin: &mut VecDeque<StdinEvent>,
) {
    loop {
        let event = pending_stdin
            .pop_front()
            .map(MainEvent::Stdin)
            .or_else(|| receiver.recv().ok());
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

fn parse_line(line: &str) -> ParsedLine {
    if line.is_empty() {
        return ParsedLine::Detach;
    }
    let parsed = zz_mux::parse_config("<control>", line);
    if let Some(diagnostic) = parsed.diagnostics.first() {
        return ParsedLine::Error(format!("parse error: {}", diagnostic.message));
    }
    if let Some(error) = parsed
        .commands
        .iter()
        .find_map(|command| unknown_command(&command.name))
    {
        return ParsedLine::Error(format!("parse error: {error}"));
    }
    if parsed.commands.is_empty() {
        ParsedLine::Ignore
    } else {
        ParsedLine::Commands(parsed.commands)
    }
}

fn unknown_command(name: &str) -> Option<String> {
    match zz_protocol::resolve_command(name) {
        zz_protocol::CommandResolution::Unknown => Some(format!("unknown command: {name}")),
        zz_protocol::CommandResolution::Ambiguous(message) => Some(message),
        _ => None,
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct ControlWriter<W: Write> {
    output: W,
    double: bool,
    next_number: u64,
    block_open: bool,
    deferred: VecDeque<Vec<u8>>,
    st_sent: bool,
}

impl<W: Write> ControlWriter<W> {
    fn new(output: W, double: bool) -> Self {
        Self {
            output,
            double,
            next_number: 1,
            block_open: false,
            deferred: VecDeque::new(),
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
            self.deferred.push_back(line.to_vec());
            return Ok(());
        }
        self.output.write_all(line)?;
        self.output.write_all(b"\n")?;
        self.output.flush()
    }

    fn flush_deferred(&mut self) -> io::Result<()> {
        while let Some(line) = self.deferred.pop_front() {
            self.output.write_all(&line)?;
            self.output.write_all(b"\n")?;
        }
        Ok(())
    }

    fn begin(&mut self, flags: u8) -> io::Result<Frame> {
        self.begin_at(unix_timestamp(), flags)
    }

    fn begin_at(&mut self, time: u64, flags: u8) -> io::Result<Frame> {
        let frame = Frame {
            time,
            number: self.next_number,
            flags,
        };
        self.next_number = self.next_number.saturating_add(1);
        self.block_open = true;
        writeln!(
            self.output,
            "%begin {} {} {}",
            frame.time, frame.number, frame.flags
        )?;
        self.output.flush()?;
        Ok(frame)
    }

    fn response(&mut self, frame: &Frame, response: CommandResponse) -> io::Result<u8> {
        match response {
            CommandResponse::Success {
                output, exit_code, ..
            } => {
                self.payload(&output)?;
                self.end(frame, false)?;
                Ok(exit_code)
            }
            CommandResponse::Error { error, output, .. } => {
                self.payload(&output)?;
                match error {
                    ServerError::InvalidCommand(message) => self.write_line(&message)?,
                    error => self.write_line(&error.to_string())?,
                }
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

    fn payload(&mut self, output: &str) -> io::Result<()> {
        let output = output.strip_suffix('\n').unwrap_or(output);
        if !output.is_empty() {
            self.output.write_all(output.as_bytes())?;
            self.output.write_all(b"\n")?;
        }
        Ok(())
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.output.write_all(line.as_bytes())?;
        self.output.write_all(b"\n")
    }

    fn end(&mut self, frame: &Frame, error: bool) -> io::Result<()> {
        let marker = if error { "%error" } else { "%end" };
        writeln!(
            self.output,
            "{marker} {} {} {}",
            frame.time, frame.number, frame.flags
        )?;
        self.block_open = false;
        self.flush_deferred()?;
        self.output.flush()
    }

    fn emit_exit(&mut self, reason: Option<&str>) -> io::Result<()> {
        self.block_open = false;
        self.flush_deferred()?;
        match reason {
            Some(reason) => writeln!(self.output, "%exit {reason}")?,
            None => self.output.write_all(b"%exit\n")?,
        }
        self.output.flush()
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
    time: u64,
    number: u64,
    flags: u8,
}

struct CommandResult {
    exit_code: u8,
    exit: ExitSignal,
    abort_line: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExitSignal {
    None,
    Clean,
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
            Self::None | Self::Clean => None,
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

#[derive(Debug, PartialEq, Eq)]
enum ParsedLine {
    Detach,
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

    #[test]
    fn serializer_keeps_frame_identity_payload_and_error_shapes() {
        let mut writer = ControlWriter::new(Vec::new(), false);
        let first = writer.begin_at(17, 0).unwrap();
        assert_eq!(first.number, 1);
        writer
            .response(
                &first,
                CommandResponse::Success {
                    request_id: 1,
                    output: "one\ntwo\n".to_owned(),
                    exit_code: 7,
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
                    output: "hook\n\n".to_owned(),
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
                    output: String::new(),
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
                    output: String::new(),
                },
            )
            .unwrap();
        assert_eq!(
            writer.output,
            b"%begin 17 1 0\none\ntwo\n%end 17 1 0\n%begin 18 2 1\nhook\n\ncan't find session: gone\n%error 18 2 1\n%begin 19 3 1\nunknown command: bogus-command\n%error 19 3 1\n%begin 20 4 1\nunsupported command: new-pane\n%error 20 4 1\n"
        );
    }

    #[test]
    fn parser_distinguishes_detach_ignores_chains_and_errors() {
        assert_eq!(parse_line(""), ParsedLine::Detach);
        assert_eq!(parse_line("   "), ParsedLine::Ignore);
        assert_eq!(parse_line(" # ignored"), ParsedLine::Ignore);
        let ParsedLine::Commands(commands) = parse_line("ls ; list-panes") else {
            panic!("semicolon chain was not parsed");
        };
        assert_eq!(
            commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            ["ls", "list-panes"]
        );
        assert_eq!(
            parse_line("bogus-command"),
            ParsedLine::Error("parse error: unknown command: bogus-command".to_owned())
        );
        assert_eq!(
            parse_line("set 'oops"),
            ParsedLine::Error("parse error: unterminated quote".to_owned())
        );
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
                    output: "body\n".to_owned(),
                    exit_code: 0,
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
    fn window_notifications_use_the_owning_session_for_unlinked_prefixes() {
        let state = ControlState {
            attached_session: Some(SessionId(1)),
            ..ControlState::default()
        };
        let variables = |session: SessionId| {
            BTreeMap::from([
                ("hook_session".to_owned(), session.to_string()),
                ("hook_window".to_owned(), WindowId(3).to_string()),
                ("hook_window_name".to_owned(), "shell".to_owned()),
            ])
        };
        assert_eq!(
            render_hook(&state, "window-linked", &variables(SessionId(1))).as_deref(),
            Some("%window-add @3")
        );
        assert_eq!(
            render_hook(&state, "window-linked", &variables(SessionId(2))).as_deref(),
            Some("%unlinked-window-add @3")
        );
        assert_eq!(
            render_hook(&state, "window-unlinked", &variables(SessionId(1))).as_deref(),
            Some("%unlinked-window-close @3")
        );
        assert_eq!(
            render_hook(&state, "window-unlinked", &variables(SessionId(2))).as_deref(),
            Some("%unlinked-window-close @3")
        );
        assert_eq!(
            render_hook(&state, "window-renamed", &variables(SessionId(2))).as_deref(),
            Some("%unlinked-window-renamed @3 shell")
        );
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
        assert_eq!(writer.output, b"%pause %7\n%continue %7\n");
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
    fn wait_exit_releases_on_empty_line_and_eof() {
        let (_sender, receiver) = mpsc::sync_channel(32);
        let mut pending = VecDeque::from([
            StdinEvent::Line("keep draining".to_owned()),
            StdinEvent::Line(String::new()),
        ]);
        wait_for_exit_input(&receiver, &mut pending);
        assert!(pending.is_empty());

        let mut pending = VecDeque::from([StdinEvent::Eof]);
        wait_for_exit_input(&receiver, &mut pending);
        assert!(pending.is_empty());
    }

    #[test]
    fn layout_notifications_use_snapshot_dumps_and_raw_flag_order() {
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
                },
            )]),
            layout_dump: "abcd,80x24,0,0,5".to_owned(),
            visible_layout_dump: "ef01,80x24,0,0,5".to_owned(),
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
        let variables = BTreeMap::from([("hook_window".to_owned(), "@3".to_owned())]);
        assert_eq!(
            render_hook(&state, "window-layout-changed", &variables).as_deref(),
            Some("%layout-change @3 abcd,80x24,0,0,5 ef01,80x24,0,0,5 !*-Z")
        );
    }

    #[test]
    fn message_escaping_is_separate_from_output_escaping() {
        assert_eq!(
            render_message("a\\b\t\n\r\u{7}\u{1b}é"),
            b"a\\b\t\n\\r\\a\\033\xc3\xa9"
        );
    }

    #[test]
    fn config_messages_include_the_source_line_shape() {
        assert!(is_config_message(
            "/tmp/mux.conf:1: unknown command: wibble"
        ));
        assert!(is_config_message(
            "skipped 1 unsupported tmux command: focus-events"
        ));
        assert!(is_config_message(
            "skipped 2 unsupported tmux commands: focus-events, status-keys"
        ));
        assert!(!is_config_message(
            "device-7 message: unknown command: wibble"
        ));
        assert!(!is_config_message(
            "skipped 2 deprecated options: focus-events, status-keys"
        ));
        assert!(!is_config_message(
            "prefix skipped 2 unsupported tmux commands: focus-events, status-keys"
        ));
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
