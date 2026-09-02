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
    CommandInvocation, CommandResponse, ControlSourceFileEvent, EventPayload, MuxSnapshot,
    PreparedCommand, PreparedCommandResult, ProtocolMessage, ServerError, SessionId, WindowId,
};

use super::{
    SocketSelectionSource, connect_or_spawn_daemon, format_local_command_error,
    tmux_command_starts_server, tmux_label_creation_error,
};

const CONTROL_PARSE_SOURCE: &str = "<control>";
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
    let mut state = ControlState::default();
    let mut pending_stdin = VecDeque::new();
    ensure_stdin_reader(&events, &mut stdin_started);
    let prepared = prepare_command_unit(
        client.as_ref(),
        &receiver,
        output,
        vec![initial],
        &mut state,
        &mut pending_stdin,
        None,
    )?;
    if prepared.exit.is_some() {
        finish_exit(
            output,
            prepared.exit.reason(),
            state.wait_exit,
            false,
            &events,
            &mut stdin_started,
            &receiver,
            &mut pending_stdin,
        )?;
        return Ok(match prepared.exit {
            ExitSignal::Clean => state.return_code,
            ExitSignal::Detached => 0,
            _ => 1,
        });
    }
    if let Some(error) = prepared_error(&prepared.commands) {
        output.write_line(&error.tmux_message())?;
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
        return Ok(1);
    }
    let initial_result = execute_prepared_command(
        client.as_ref(),
        &receiver,
        output,
        prepared.commands.into_iter().next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing prepared command")
        })?,
        0,
        &mut state,
        &mut pending_stdin,
        prepared.pending_return,
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
            &events,
            &mut stdin_started,
            &receiver,
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
            &events,
            &mut stdin_started,
            &receiver,
            &mut pending_stdin,
        );
    }
    loop {
        let event = pending_stdin.pop_front().map_or_else(
            || receiver.recv().unwrap_or(MainEvent::Disconnected),
            |stdin| {
                if let Some(pending_return) = state.pending_return.as_mut() {
                    pending_return.consume_preceding_input();
                }
                MainEvent::Stdin(stdin)
            },
        );
        match event {
            MainEvent::Stdin(StdinEvent::Line(line)) => {
                let mut resolved = resolve_home_directories(
                    client.as_ref(),
                    &receiver,
                    output,
                    &line,
                    &mut state,
                    &mut pending_stdin,
                )?;
                if resolved.exit.is_some() {
                    finish_exit(
                        output,
                        resolved.exit.reason(),
                        state.wait_exit,
                        false,
                        &events,
                        &mut stdin_started,
                        &receiver,
                        &mut pending_stdin,
                    )?;
                    return Ok(match resolved.exit {
                        ExitSignal::Clean => state.return_code,
                        ExitSignal::Detached => 0,
                        _ => 1,
                    });
                }
                match parse_line(&line, &resolved.homes) {
                    ParsedLine::Return => {
                        return finish_control_return(
                            client.as_ref(),
                            PendingReturn::Blank {
                                code: state.return_code,
                                preceding_input: 0,
                                observed_preceding_input: false,
                            },
                            output,
                            &mut state,
                            &events,
                            &mut stdin_started,
                            &receiver,
                            &mut pending_stdin,
                        );
                    }
                    ParsedLine::Ignore => {
                        if let Some(pending_return) =
                            settle_home_directory_return(&mut resolved, &mut state)
                        {
                            return finish_control_return(
                                client.as_ref(),
                                pending_return,
                                output,
                                &mut state,
                                &events,
                                &mut stdin_started,
                                &receiver,
                                &mut pending_stdin,
                            );
                        }
                    }
                    ParsedLine::Error(error) => {
                        output.parse_error(&error)?;
                        if let Some(pending_return) =
                            settle_home_directory_return(&mut resolved, &mut state)
                        {
                            return finish_control_return(
                                client.as_ref(),
                                pending_return,
                                output,
                                &mut state,
                                &events,
                                &mut stdin_started,
                                &receiver,
                                &mut pending_stdin,
                            );
                        }
                    }
                    ParsedLine::Commands(commands) => {
                        let mut prepared = prepare_command_unit(
                            client.as_ref(),
                            &receiver,
                            output,
                            commands,
                            &mut state,
                            &mut pending_stdin,
                            resolved.pending_return.take(),
                        )?;
                        if prepared.exit.is_some() {
                            finish_exit(
                                output,
                                prepared.exit.reason(),
                                state.wait_exit,
                                false,
                                &events,
                                &mut stdin_started,
                                &receiver,
                                &mut pending_stdin,
                            )?;
                            return Ok(match prepared.exit {
                                ExitSignal::Clean => state.return_code,
                                ExitSignal::Detached => 0,
                                _ => 1,
                            });
                        }
                        if let Some(error) = prepared_error(&prepared.commands) {
                            output
                                .parse_error(&format!("parse error: {}", error.tmux_message()))?;
                            if let Some(pending_return) = settle_preparation_error_return(
                                &mut prepared.pending_return,
                                &mut state.pending_return,
                            ) {
                                return finish_control_return(
                                    client.as_ref(),
                                    pending_return,
                                    output,
                                    &mut state,
                                    &events,
                                    &mut stdin_started,
                                    &receiver,
                                    &mut pending_stdin,
                                );
                            }
                            continue;
                        }
                        let first_is_detach = prepared
                            .commands
                            .first()
                            .is_some_and(prepared_command_is_detach);
                        if !first_is_detach && state.pending_return.is_none() {
                            state.pending_return = prepared.pending_return.take();
                        }
                        for (index, command) in prepared.commands.into_iter().enumerate() {
                            let result = execute_prepared_command(
                                client.as_ref(),
                                &receiver,
                                output,
                                command,
                                1,
                                &mut state,
                                &mut pending_stdin,
                                if index == 0 && first_is_detach {
                                    prepared.pending_return.take()
                                } else {
                                    None
                                },
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
                                return Ok(match result.exit {
                                    ExitSignal::Detached => 0,
                                    ExitSignal::Clean => state.return_code,
                                    _ => result.exit_code,
                                });
                            }
                            if result.abort_line {
                                break;
                            }
                        }
                        if let Some(pending_return) =
                            take_ready_pending_return(&mut state.pending_return)
                        {
                            return finish_control_return(
                                client.as_ref(),
                                pending_return,
                                output,
                                &mut state,
                                &events,
                                &mut stdin_started,
                                &receiver,
                                &mut pending_stdin,
                            );
                        }
                    }
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
                    &events,
                    &mut stdin_started,
                    &receiver,
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
                    &events,
                    &mut stdin_started,
                    &receiver,
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
                        &events,
                        &mut stdin_started,
                        &receiver,
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

fn prepared_error(commands: &[PreparedCommand]) -> Option<&ServerError> {
    commands.iter().find_map(|command| match &command.result {
        PreparedCommandResult::Ready => None,
        PreparedCommandResult::Error(error) => Some(error),
    })
}

fn prepared_command_is_detach(command: &PreparedCommand) -> bool {
    matches!(command.result, PreparedCommandResult::Ready)
        && command.canonical_name.as_deref() == Some("detach-client")
}

fn prepare_command_unit<W: Write>(
    client: &InteractiveClient,
    receiver: &mpsc::Receiver<MainEvent>,
    output: &mut ControlWriter<W>,
    commands: Vec<CommandInvocation>,
    state: &mut ControlState,
    pending_stdin: &mut VecDeque<StdinEvent>,
    mut pending_return: Option<PendingReturn>,
) -> io::Result<PreparedUnit> {
    let expected = commands.len();
    let request_id = client
        .prepare_commands(commands)
        .map_err(io::Error::other)?;
    let mut exit = ExitSignal::None;
    loop {
        match receiver.recv().unwrap_or(MainEvent::Disconnected) {
            MainEvent::Protocol(message) => match match_prepared_response(*message, request_id) {
                Ok(commands) => {
                    if commands.len() != expected {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "prepared command count mismatch",
                        ));
                    }
                    return Ok(PreparedUnit {
                        commands,
                        exit,
                        pending_return,
                    });
                }
                Err(message) => {
                    let signal = handle_protocol(message, state, output)?;
                    if signal.is_some() && exit != ExitSignal::Detached {
                        exit = signal;
                    }
                }
            },
            MainEvent::Stdin(stdin) => {
                capture_pending_return(
                    stdin,
                    state.return_code,
                    &mut pending_return,
                    pending_stdin,
                    output,
                );
            }
            MainEvent::Disconnected => {
                return Ok(PreparedUnit {
                    commands: Vec::new(),
                    exit: if exit.is_some() {
                        exit
                    } else {
                        ExitSignal::Unexpected
                    },
                    pending_return,
                });
            }
        }
    }
}

fn match_home_directory_response(
    message: ProtocolMessage,
    request_id: u64,
) -> Result<Vec<Option<String>>, ProtocolMessage> {
    match message {
        ProtocolMessage::HomeDirectoryResponse {
            request_id: response_id,
            homes,
        } if response_id == request_id => Ok(homes),
        message => Err(message),
    }
}

fn resolve_home_directories<W: Write>(
    client: &InteractiveClient,
    receiver: &mpsc::Receiver<MainEvent>,
    output: &mut ControlWriter<W>,
    line: &str,
    state: &mut ControlState,
    pending_stdin: &mut VecDeque<StdinEvent>,
) -> io::Result<HomeUnit> {
    let mut unit = HomeUnit::default();
    if !line.contains('~') {
        return Ok(unit);
    }
    let users = zz_mux::config_home_directory_names(CONTROL_PARSE_SOURCE, line);
    if users.is_empty() {
        return Ok(unit);
    }
    let users: Vec<String> = users.into_iter().collect();
    let request_id = client
        .request_home_directories(users.clone())
        .map_err(io::Error::other)?;
    loop {
        match receiver.recv().unwrap_or(MainEvent::Disconnected) {
            MainEvent::Protocol(message) => {
                match match_home_directory_response(*message, request_id) {
                    Ok(homes) => {
                        if homes.len() != users.len() {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "home directory count mismatch",
                            ));
                        }
                        unit.homes = users
                            .into_iter()
                            .zip(homes)
                            .filter_map(|(user, home)| home.map(|home| (user, home)))
                            .collect();
                        return Ok(unit);
                    }
                    Err(message) => {
                        let signal = handle_protocol(message, state, output)?;
                        if signal.is_some() && unit.exit != ExitSignal::Detached {
                            unit.exit = signal;
                        }
                    }
                }
            }
            MainEvent::Stdin(stdin) => {
                capture_pending_return(
                    stdin,
                    state.return_code,
                    &mut unit.pending_return,
                    pending_stdin,
                    output,
                );
            }
            MainEvent::Disconnected => {
                unit.exit = if unit.exit.is_some() {
                    unit.exit
                } else {
                    ExitSignal::Unexpected
                };
                return Ok(unit);
            }
        }
    }
}

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

fn execute_prepared_command<W: Write>(
    client: &InteractiveClient,
    receiver: &mpsc::Receiver<MainEvent>,
    output: &mut ControlWriter<W>,
    command: PreparedCommand,
    flags: u8,
    state: &mut ControlState,
    pending_stdin: &mut VecDeque<StdinEvent>,
    pending_return: Option<PendingReturn>,
) -> io::Result<CommandResult> {
    let PreparedCommand {
        invocation,
        canonical_name,
        result: PreparedCommandResult::Ready,
        ..
    } = command
    else {
        unreachable!()
    };
    execute_command(
        client,
        receiver,
        output,
        invocation,
        flags,
        state,
        pending_stdin,
        canonical_name.as_deref(),
        pending_return,
    )
}

fn execute_command<W: Write>(
    client: &InteractiveClient,
    receiver: &mpsc::Receiver<MainEvent>,
    output: &mut ControlWriter<W>,
    command: CommandInvocation,
    flags: u8,
    state: &mut ControlState,
    pending_stdin: &mut VecDeque<StdinEvent>,
    canonical_name: Option<&str>,
    mut deferred_return: Option<PendingReturn>,
) -> io::Result<CommandResult> {
    let detach_command = canonical_name == Some("detach-client");
    let alias_group = zz_mux::MuxEngine::is_command_alias_group(&command);
    let command_guard_frames = output.command_guard_frames;
    let frame = if alias_group {
        output.hold_exit();
        None
    } else {
        Some(output.begin(flags)?)
    };
    let exit_held = alias_group;
    let request_id = match client.execute_prepared(command) {
        Ok(request_id) => request_id,
        Err(error) => {
            render_command_failure(
                output,
                frame.as_ref(),
                flags,
                command_guard_frames,
                &error.to_string(),
            )?;
            if exit_held {
                output.release_exit()?;
            }
            return Ok(CommandResult {
                exit_code: 1,
                exit: ExitSignal::Unexpected,
                abort_line: true,
            });
        }
    };
    let mut exit = ExitSignal::None;
    let mut parked = false;
    loop {
        match receiver.recv().unwrap_or(MainEvent::Disconnected) {
            MainEvent::Protocol(message) => match *message {
                ProtocolMessage::CommandQueueParked {
                    request_id: parked_request,
                } if parked_request == request_id => {
                    parked = true;
                    if release_parked_queue_at_client_exit(state, pending_stdin) {
                        return close_parked_request(
                            output,
                            frame.as_ref(),
                            flags,
                            command_guard_frames,
                            request_id,
                            exit_held,
                            exit,
                        );
                    }
                }
                ProtocolMessage::CommandResponse(response)
                    if response_request_id(&response) == request_id =>
                {
                    let abort_line = response_aborts_line(&response);
                    let updates_return_code = response_sets_return_code(canonical_name, &response);
                    let response_sets_new_failure = updates_return_code && state.return_code == 0;
                    if updates_return_code {
                        state.return_code = 1;
                    }
                    settle_deferred_return(
                        exit == ExitSignal::Detached,
                        &mut deferred_return,
                        state,
                    );
                    if let Some(pending_return) = state.pending_return.as_mut() {
                        if (response_sets_new_failure
                            && !response_is_post_admission_callback_failure(&response))
                            || (updates_return_code && canonical_name == Some("source-file"))
                        {
                            pending_return.observe_preceding_input();
                        }
                        pending_return.refresh_code_after_preceding_input(state.return_code);
                    }
                    let exit_code = render_command_response(
                        output,
                        frame.as_ref(),
                        flags,
                        command_guard_frames,
                        response,
                    )?;
                    if exit_held {
                        output.release_exit()?;
                    }
                    return Ok(CommandResult {
                        exit_code,
                        exit,
                        abort_line,
                    });
                }
                message => {
                    let signal = handle_protocol(message, state, output)?;
                    if signal == ExitSignal::TooFarBehind {
                        render_command_failure(
                            output,
                            frame.as_ref(),
                            flags,
                            command_guard_frames,
                            "too far behind",
                        )?;
                        if exit_held {
                            output.release_exit()?;
                        }
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
            MainEvent::Stdin(stdin) => {
                if detach_command {
                    capture_pending_return(
                        stdin,
                        state.return_code,
                        &mut deferred_return,
                        pending_stdin,
                        output,
                    );
                } else {
                    capture_pending_return(
                        stdin,
                        state.return_code,
                        &mut state.pending_return,
                        pending_stdin,
                        output,
                    );
                }
                if parked && release_parked_queue_at_client_exit(state, pending_stdin) {
                    return close_parked_request(
                        output,
                        frame.as_ref(),
                        flags,
                        command_guard_frames,
                        request_id,
                        exit_held,
                        exit,
                    );
                }
            }
            MainEvent::Disconnected => {
                render_command_failure(
                    output,
                    frame.as_ref(),
                    flags,
                    command_guard_frames,
                    "server exited unexpectedly",
                )?;
                if exit_held {
                    output.release_exit()?;
                }
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
    return_code: u8,
    pending_return: Option<PendingReturn>,
    parked_queue_released: bool,
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
        ProtocolMessage::Attached {
            session, snapshot, ..
        } => state.attach(session, snapshot),
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
            EventPayload::ControlCommandGuard {
                output: text,
                error,
                sticky_failure,
                flags,
            } => {
                if sticky_failure || (error && is_source_error_message(&text)) {
                    state.return_code = 1;
                }
                output.control_command_guard(&text, error, flags)?;
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
    match PendingReturn::from_stdin(stdin, return_code, pending_stdin.len()) {
        Ok(return_event) => {
            if return_event.discards_pane_output() {
                output.discard_pane_output();
            }
            if pending_return.is_none() {
                *pending_return = Some(return_event);
            }
        }
        Err(stdin) => pending_stdin.push_back(stdin),
    }
}

/// The pin prints a parked command's `%end` when the command fires, with
/// nothing in the block, and the client that leaves at end of file never sees
/// what the command produces later. Everything queued behind it dies with the
/// queue, which is what `abort_line` says.
fn close_parked_request<W: Write>(
    output: &mut ControlWriter<W>,
    frame: Option<&Frame>,
    flags: u8,
    command_guard_frames: u64,
    request_id: u64,
    exit_held: bool,
    exit: ExitSignal,
) -> io::Result<CommandResult> {
    render_command_response(
        output,
        frame,
        flags,
        command_guard_frames,
        CommandResponse::Success {
            request_id,
            output: String::new(),
            exit_code: 0,
            stderr: String::new(),
        },
    )?;
    if exit_held {
        output.release_exit()?;
    }
    Ok(CommandResult {
        exit_code: 0,
        exit,
        abort_line: true,
    })
}

/// A parked request holds the daemon's queue for this client, so the input that
/// is already queued behind it never runs. tmux frees that queue when the client
/// exits, and both end of file and a blank Return exit it immediately, so stop
/// waiting for the parked request and let the pending return finish the client.
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

fn settle_preparation_error_return(
    prepared_return: &mut Option<PendingReturn>,
    state_return: &mut Option<PendingReturn>,
) -> Option<PendingReturn> {
    if state_return.is_none() {
        *state_return = prepared_return.take();
    }
    take_ready_pending_return(state_return)
}

fn settle_home_directory_return(
    resolved: &mut HomeUnit,
    state: &mut ControlState,
) -> Option<PendingReturn> {
    resolved.pending_return.as_ref()?;
    settle_preparation_error_return(&mut resolved.pending_return, &mut state.pending_return)
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

fn finish_control_return<W: Write>(
    client: &InteractiveClient,
    pending_return: PendingReturn,
    output: &mut ControlWriter<W>,
    state: &mut ControlState,
    events: &mpsc::SyncSender<MainEvent>,
    stdin_started: &mut bool,
    receiver: &mpsc::Receiver<MainEvent>,
    pending_stdin: &mut VecDeque<StdinEvent>,
) -> io::Result<u8> {
    if pending_return.discards_pane_output() {
        output.discard_pane_output();
    }
    let code = pending_return.code();
    let (input_closed, input_error) = match pending_return {
        PendingReturn::Blank { .. } => (false, None),
        PendingReturn::Eof { .. } => (true, None),
        PendingReturn::InputError { message, .. } => (true, Some(message)),
    };
    if let Some(error) = input_error.as_deref() {
        eprintln!("zz: {error}");
    }
    let _ = client.detach();
    if input_error.is_none() && !state.parked_queue_released {
        drain_before_exit(receiver, state, output)?;
    }
    finish_exit(
        output,
        None,
        state.wait_exit,
        input_closed,
        events,
        stdin_started,
        receiver,
        pending_stdin,
    )?;
    Ok(code)
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

fn parse_line(line: &str, homes: &BTreeMap<String, String>) -> ParsedLine {
    if line.is_empty() {
        return ParsedLine::Return;
    }
    let parsed = zz_mux::parse_config_with_home_directories(CONTROL_PARSE_SOURCE, line, homes);
    if let Some(diagnostic) = parsed.diagnostics.first() {
        return ParsedLine::Error(format!("parse error: {}", diagnostic.message));
    }
    if parsed.commands.is_empty() {
        ParsedLine::Ignore
    } else {
        ParsedLine::Commands(parsed.commands)
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
        output: String,
        error: bool,
        flags: u8,
    },
    ControlSourceFile(ControlSourceFileEvent),
    ControlCommandOutput(String),
}

struct ControlWriter<W: Write> {
    output: W,
    double: bool,
    next_number: u64,
    command_guard_frames: u64,
    block_open: bool,
    deferred: VecDeque<DeferredOutput>,
    pane_output_enabled: bool,
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
            deferred: VecDeque::new(),
            pane_output_enabled: true,
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
        if !self.pane_output_enabled {
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

    fn discard_pane_output(&mut self) {
        self.pane_output_enabled = false;
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
                    let frame = self.allocate_frame(time, 1);
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
                DeferredOutput::ControlCommandOutput(output) => self.payload(&output)?,
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
        let frame = self.allocate_frame(time, 1);
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
        if self.block_open {
            self.deferred
                .push_back(DeferredOutput::ControlCommandGuard {
                    time,
                    output: output.to_owned(),
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
        output: &str,
        error: bool,
        flags: u8,
    ) -> io::Result<()> {
        let frame = self.allocate_frame(time, flags);
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
        self.payload(output)?;
        self.output.flush()
    }

    fn begin(&mut self, flags: u8) -> io::Result<Frame> {
        self.begin_at(unix_timestamp(), flags)
    }

    fn begin_at(&mut self, time: u64, flags: u8) -> io::Result<Frame> {
        let frame = self.allocate_frame(time, flags);
        self.block_open = true;
        self.write_frame_begin(&frame)?;
        self.output.flush()?;
        Ok(frame)
    }

    fn allocate_frame(&mut self, time: u64, flags: u8) -> Frame {
        let frame = Frame {
            time,
            number: self.next_number,
            flags,
        };
        self.next_number = self.next_number.saturating_add(1);
        frame
    }

    fn write_frame_begin(&mut self, frame: &Frame) -> io::Result<()> {
        writeln!(
            self.output,
            "%begin {} {} {}",
            frame.time, frame.number, frame.flags
        )
    }

    fn write_frame_end(&mut self, frame: &Frame, error: bool) -> io::Result<()> {
        let marker = if error { "%error" } else { "%end" };
        writeln!(
            self.output,
            "{marker} {} {} {}",
            frame.time, frame.number, frame.flags
        )
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

    fn payload(&mut self, output: &str) -> io::Result<()> {
        if !output.is_empty() {
            self.output.write_all(output.as_bytes())?;
            if !output.ends_with('\n') {
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

struct PreparedUnit {
    commands: Vec<PreparedCommand>,
    exit: ExitSignal,
    pending_return: Option<PendingReturn>,
}

#[derive(Default)]
struct HomeUnit {
    homes: BTreeMap<String, String>,
    exit: ExitSignal,
    pending_return: Option<PendingReturn>,
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
        match self {
            Self::Blank {
                preceding_input, ..
            }
            | Self::Eof {
                preceding_input, ..
            }
            | Self::InputError {
                preceding_input, ..
            } => *preceding_input != 0,
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
                    stderr: String::new(),
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
        let fifth = writer.begin_at(21, 0).unwrap();
        writer
            .response(
                &fifth,
                CommandResponse::Success {
                    request_id: 5,
                    output: "\n".to_owned(),
                    exit_code: 0,
                    stderr: String::new(),
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
                        output: String::new(),
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
                        output: String::new(),
                        exit_code: 3,
                        stderr: String::new(),
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
                    output: "fresh".to_owned(),
                    exit_code: 0,
                    stderr: String::new(),
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
                    output: "outer output".to_owned(),
                    exit_code: 0,
                    stderr: String::new(),
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
                    output: "outer output".to_owned(),
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
                    output: String::new(),
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
        let source_read = "stream did not contain valid UTF-8: /tmp/invalid-source.conf";
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
                        text: source_read.to_owned(),
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
                        output: output.to_owned(),
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
                        output: "diagnostic".to_owned(),
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
                    output: String::new(),
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
                    output: "AFTER".to_owned(),
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
                    output: String::new(),
                    exit_code: 1,
                    stderr: String::new(),
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
                        output: String::new(),
                        exit_code: 0,
                        stderr: String::new(),
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
            output: String::new(),
            exit_code: 3,
            stderr: String::new(),
        };
        assert!(!response_aborts_line(&success));
        let mut writer = ControlWriter::new(Vec::new(), false);
        let frame = writer.begin_at(17, 1).unwrap();
        assert_eq!(writer.response(&frame, success).unwrap(), 3);
        assert_eq!(writer.output, b"%begin 17 1 1\n%end 17 1 1\n");
        assert!(response_aborts_line(&CommandResponse::Error {
            request_id: 2,
            error: ServerError::InvalidCommand("failed".to_owned()),
            output: String::new(),
        }));
        let parse = CommandResponse::Error {
            request_id: 3,
            error: ServerError::CommandParse("unknown flag -Z".to_owned()),
            output: String::new(),
        };
        assert!(response_aborts_line(&parse));
        assert!(!response_sets_return_code(Some("list-sessions"), &parse));
        assert!(response_sets_return_code(
            Some("kill-session"),
            &CommandResponse::Error {
                request_id: 4,
                error: ServerError::SessionNotFound("missing".to_owned()),
                output: String::new(),
            }
        ));
        let direct = CommandResponse::Error {
            request_id: 5,
            error: ServerError::InvalidCommand("can't find session: missing".to_owned()),
            output: String::new(),
        };
        assert!(!response_is_post_admission_callback_failure(&direct));
        let callback = CommandResponse::Error {
            request_id: 6,
            error: ServerError::PostAdmissionCallback(Box::new(ServerError::SessionNotFound(
                "missing".to_owned(),
            ))),
            output: String::new(),
        };
        assert!(response_is_post_admission_callback_failure(&callback));
        let nonzero = CommandResponse::Success {
            request_id: 7,
            output: String::new(),
            exit_code: 3,
            stderr: String::new(),
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
        let ParsedLine::Commands(mut commands) = parse_line(&line, &BTreeMap::new()) else {
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
                    output: String::new(),
                    exit_code: 0,
                    stderr: String::new(),
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
                    output: String::new(),
                    exit_code: 0,
                    stderr: String::new(),
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
        assert_eq!(parse_line("", &BTreeMap::new()), ParsedLine::Return);
        assert_eq!(parse_line("   ", &BTreeMap::new()), ParsedLine::Ignore);
        assert_eq!(
            parse_line(" # ignored", &BTreeMap::new()),
            ParsedLine::Ignore
        );
        let ParsedLine::Commands(commands) = parse_line("ls ; list-panes", &BTreeMap::new()) else {
            panic!("semicolon chain was not parsed");
        };
        assert_eq!(
            commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            ["ls", "list-panes"]
        );
        let ParsedLine::Commands(commands) = parse_line("bogus-command", &BTreeMap::new()) else {
            panic!("unknown command was rejected before live preparation");
        };
        assert_eq!(commands[0].name, "bogus-command");
        let ParsedLine::Commands(commands) = parse_line("set 'oops", &BTreeMap::new()) else {
            panic!("open quote at EOF was rejected");
        };
        assert_eq!(commands[0].name, "set");
        assert_eq!(commands[0].args, ["oops"]);
        let ParsedLine::Commands(commands) =
            parse_line("set-environment -g CONTROL_LITERAL $FOO", &BTreeMap::new())
        else {
            panic!("literal variable command was not parsed");
        };
        assert_eq!(commands[0].args, ["-g", "CONTROL_LITERAL", "$FOO"]);
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
    fn home_directory_response_matching_ignores_stale_request_ids() {
        let stale = ProtocolMessage::HomeDirectoryResponse {
            request_id: 8,
            homes: Vec::new(),
        };
        assert!(matches!(
            match_home_directory_response(stale, 9),
            Err(ProtocolMessage::HomeDirectoryResponse { request_id: 8, .. })
        ));
        assert_eq!(
            match_home_directory_response(
                ProtocolMessage::HomeDirectoryResponse {
                    request_id: 9,
                    homes: vec![Some("/server/home".to_owned()), None],
                },
                9,
            )
            .unwrap(),
            [Some("/server/home".to_owned()), None]
        );
    }

    #[test]
    fn control_lines_expand_tildes_from_the_daemon_answer() {
        let homes = BTreeMap::from([
            (String::new(), "/server/home".to_owned()),
            ("alice".to_owned(), "/users/alice".to_owned()),
        ]);
        let ParsedLine::Commands(commands) =
            parse_line("display-message -p ~ ~/x ~alice/y $LITERAL", &homes)
        else {
            panic!("tilde line was not parsed");
        };
        assert_eq!(
            commands[0].args,
            [
                "-p",
                "/server/home",
                "/server/home/x",
                "/users/alice/y",
                "$LITERAL"
            ]
        );
        assert_eq!(
            parse_line("display-message -p ~nobody", &homes),
            ParsedLine::Error("parse error: syntax error".to_owned())
        );
        let ParsedLine::Commands(commands) = parse_line("display-message -p '~'", &homes) else {
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
                    output: "body\n".to_owned(),
                    exit_code: 0,
                    stderr: String::new(),
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
                        output: "body\n".to_owned(),
                        exit_code: 0,
                        stderr: String::new(),
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
                    output: String::new(),
                    exit_code: 3,
                    stderr: String::new(),
                },
            )
            .unwrap();
        let command_mode = writer.begin_at(22, 1).unwrap();
        writer
            .response(
                &command_mode,
                CommandResponse::Success {
                    request_id: 6,
                    output: "command mode".to_owned(),
                    exit_code: 0,
                    stderr: String::new(),
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
                    output: "body\n".to_owned(),
                    exit_code: 0,
                    stderr: String::new(),
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
                    border_colour: None,
                    active_border_colour: None,
                    border_status_text: String::new(),
                },
            )]),
            layout_dump: "abcd,80x24,0,0,5".to_owned(),
            visible_layout_dump: "ef01,80x24,0,0,5".to_owned(),
            status_label: String::new(),
            activity: false,
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
    fn daemon_source_read_failures_keep_their_own_channel() {
        for text in [
            "No such file or directory: /tmp/mux.conf",
            "Invalid argument: /tmp/mux.conf",
            "Cannot allocate memory: /tmp/mux.conf",
            "Pattern syntax error: /tmp/[",
            "too many nested files",
            "Is a directory (os error 21): /tmp/a: b",
            "stream did not contain valid UTF-8: /tmp/binary.conf",
        ] {
            assert!(is_source_error_message(text), "{text}");
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
