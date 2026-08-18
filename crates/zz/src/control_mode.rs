use std::{
    collections::VecDeque,
    io::{self, BufRead as _, IsTerminal as _, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, mpsc},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use zz_daemon::InteractiveClient;
use zz_protocol::{
    COMMAND_SPECS, CommandInvocation, CommandResponse, CommandSpec, DAEMON_COMMAND_SPECS,
    EventPayload, ProtocolMessage, ServerError,
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
    let (events, receiver) = mpsc::channel();
    spawn_protocol_reader(Arc::clone(client), events.clone());
    if let Some(error) = unknown_command(&initial.name) {
        output.write_line(&error)?;
        output.exit(None)?;
        return Ok(1);
    }

    let mut attached = false;
    let mut pending_stdin = VecDeque::new();
    let initial_result = execute_command(
        client.as_ref(),
        &receiver,
        output,
        initial,
        0,
        &mut attached,
        &mut pending_stdin,
    )?;
    if initial_result.exit.is_some() {
        output.exit(initial_result.exit.reason())?;
        return Ok(initial_result.exit_code);
    }
    if initial_result.exit_code != 0 || !attached {
        output.exit(None)?;
        return Ok(initial_result.exit_code);
    }

    spawn_stdin_reader(events);
    loop {
        let event = pending_stdin.pop_front().map_or_else(
            || receiver.recv().unwrap_or(MainEvent::Disconnected),
            MainEvent::Stdin,
        );
        match event {
            MainEvent::Stdin(StdinEvent::Line(line)) => match parse_line(&line) {
                ParsedLine::Detach => {
                    let _ = client.detach();
                    output.exit(None)?;
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
                            &mut attached,
                            &mut pending_stdin,
                        )?;
                        if result.exit.is_some() {
                            output.exit(result.exit.reason())?;
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
                output.exit(None)?;
                return Ok(0);
            }
            MainEvent::Stdin(StdinEvent::Error(error)) => {
                eprintln!("zz: {error}");
                let _ = client.detach();
                output.exit(None)?;
                return Ok(1);
            }
            MainEvent::Protocol(message) => match *message {
                ProtocolMessage::Attached { .. } => attached = true,
                ProtocolMessage::Event(event) => match event.payload {
                    EventPayload::Detached { .. } | EventPayload::ServerStopping => {
                        output.exit(None)?;
                        return Ok(0);
                    }
                    _ => {}
                },
                _ => {}
            },
            MainEvent::Disconnected => {
                output.exit(Some("server exited unexpectedly"))?;
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
    attached: &mut bool,
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
                ProtocolMessage::Attached { .. } => *attached = true,
                ProtocolMessage::Event(event) => match event.payload {
                    EventPayload::Detached { .. } => {
                        *attached = false;
                        exit = ExitSignal::Clean;
                    }
                    EventPayload::ServerStopping => exit = ExitSignal::Clean,
                    _ => {}
                },
                _ => {}
            },
            MainEvent::Stdin(stdin) => pending_stdin.push_back(stdin),
            MainEvent::Disconnected => {
                output.error(&frame, "server exited unexpectedly")?;
                return Ok(CommandResult {
                    exit_code: 1,
                    exit: if exit == ExitSignal::Clean {
                        ExitSignal::Clean
                    } else {
                        ExitSignal::Unexpected
                    },
                    abort_line: true,
                });
            }
        }
    }
}

fn response_request_id(response: &CommandResponse) -> u64 {
    match response {
        CommandResponse::Success { request_id, .. } | CommandResponse::Error { request_id, .. } => {
            *request_id
        }
    }
}

fn spawn_protocol_reader(client: Arc<InteractiveClient>, events: mpsc::Sender<MainEvent>) {
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

fn spawn_stdin_reader(events: mpsc::Sender<MainEvent>) {
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
    let known = COMMAND_SPECS
        .iter()
        .chain(DAEMON_COMMAND_SPECS)
        .any(|spec| spec.name == name || spec.aliases.contains(&name))
        || CommandSpec::UNIMPLEMENTED_TMUX_COMMANDS.contains(&name);
    (!known).then(|| format!("unknown command: {name}"))
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct ControlWriter<W> {
    output: W,
    double: bool,
    next_number: u64,
}

impl<W: Write> ControlWriter<W> {
    fn new(output: W, double: bool) -> Self {
        Self {
            output,
            double,
            next_number: 1,
        }
    }

    fn start(&mut self) -> io::Result<()> {
        if self.double {
            self.output.write_all(DCS)?;
            self.output.flush()?;
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
                    ServerError::InvalidCommand(message)
                    | ServerError::UnsupportedCommand(message) => self.write_line(&message)?,
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
        self.output.flush()
    }

    fn exit(&mut self, reason: Option<&str>) -> io::Result<()> {
        match reason {
            Some(reason) => writeln!(self.output, "%exit {reason}")?,
            None => self.output.write_all(b"%exit\n")?,
        }
        if self.double {
            self.output.write_all(ST)?;
        }
        self.output.flush()
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExitSignal {
    None,
    Clean,
    Unexpected,
}

impl ExitSignal {
    fn is_some(self) -> bool {
        self != Self::None
    }

    fn reason(self) -> Option<&'static str> {
        (self == Self::Unexpected).then_some("server exited unexpectedly")
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
            b"%begin 17 1 0\none\ntwo\n%end 17 1 0\n%begin 18 2 1\nhook\n\ncan't find session: gone\n%error 18 2 1\n%begin 19 3 1\nunknown command: bogus-command\n%error 19 3 1\n%begin 20 4 1\nnew-pane\n%error 20 4 1\n"
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
        writer.exit(None).unwrap();
        assert_eq!(writer.output, b"\x1bP1000p%exit\n\x1b\\");
    }
}
