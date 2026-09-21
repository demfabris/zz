use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use zz_daemon::InteractiveClient;
use zz_protocol::{CommandInvocation, CommandResponse, EventPayload, ProtocolMessage, RawText};

use super::{
    SocketSelectionSource, connect_or_spawn_daemon, format_local_command_error,
    tmux_command_starts_server, tmux_label_creation_error,
};

pub(crate) fn run(
    socket_path: &Path,
    socket_source: SocketSelectionSource,
    mux_config_files: &[PathBuf],
    no_start_server: bool,
    arguments: &[RawText],
) -> ExitCode {
    let spec = zz_protocol::catalog_command_spec("events").expect("events catalog");
    let parsed = match zz_protocol::parse_tmux_options(spec, arguments).and_then(|parsed| {
        spec.validate_positional_maximum(parsed.positionals.len())?;
        Ok(parsed)
    }) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("events: {}", error.tmux_message());
            return ExitCode::from(2);
        }
    };
    let target = parsed.options.iter().rev().find_map(|option| match option {
        zz_protocol::TmuxOption::Value("-t", value) => Some((*value).to_owned()),
        _ => None,
    });
    let start_server = !no_start_server && tmux_command_starts_server("attach-session");
    if let Some(error) = tmux_label_creation_error(socket_path, socket_source, start_server) {
        eprintln!("events: {}", error.message);
        return ExitCode::FAILURE;
    }
    let stdout = io::stdout();
    let mut output = EventWriter {
        output: stdout.lock(),
        next_seq: 0,
        target,
    };
    let mut startup_gaps = 0;
    loop {
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
                eprintln!("events: {}", format_local_command_error(socket_path, error));
                return ExitCode::FAILURE;
            }
        };
        match drive(&client, &mut output, &mut startup_gaps) {
            Err(StreamError::Gap) if output.next_seq == 0 => {
                startup_gaps += 1;
            }
            Err(StreamError::Gap) => {
                if let Err(error) = output.line("gap", &BTreeMap::new()) {
                    eprintln!("events: {error}");
                    return ExitCode::FAILURE;
                }
            }
            Err(StreamError::Disconnected(reason)) => {
                eprintln!("events: {reason}");
                return ExitCode::FAILURE;
            }
            Ok(()) => return ExitCode::FAILURE,
        }
    }
}

enum StreamError {
    Gap,
    Disconnected(String),
}

impl From<io::Error> for StreamError {
    fn from(error: io::Error) -> Self {
        Self::Disconnected(error.to_string())
    }
}

fn drive<W: Write>(
    client: &Arc<InteractiveClient>,
    output: &mut EventWriter<W>,
    startup_gaps: &mut u64,
) -> Result<(), StreamError> {
    let (receiver, pending) = subscribe(client, output.target.as_deref())?;
    if output.next_seq == 0 {
        output.line("ready", &BTreeMap::new())?;
        for _ in 0..std::mem::take(startup_gaps) {
            output.line("gap", &BTreeMap::new())?;
        }
    }
    for payload in pending {
        output.hook(payload)?;
    }
    loop {
        if let ProtocolMessage::Event(event) = receive(&receiver)? {
            output.hook(event.payload)?;
        }
    }
}

fn subscribe(
    client: &Arc<InteractiveClient>,
    target: Option<&str>,
) -> Result<
    (
        mpsc::Receiver<Result<ProtocolMessage, String>>,
        Vec<EventPayload>,
    ),
    StreamError,
> {
    let (sender, receiver) = mpsc::sync_channel(32);
    let reader = Arc::clone(client);
    thread::Builder::new()
        .name("zz-events-protocol".to_owned())
        .spawn(move || {
            loop {
                let message = reader.recv().map_err(|error| error.to_string());
                let finished = message.is_err()
                    || matches!(
                        &message,
                        Ok(ProtocolMessage::Event(zz_protocol::Event {
                            payload: EventPayload::ControlExit { .. }
                                | EventPayload::ServerStopping
                                | EventPayload::Detached { .. },
                            ..
                        }))
                    );
                if sender.send(message).is_err() || finished {
                    break;
                }
            }
        })?;
    let mut arguments = Vec::<RawText>::new();
    if let Some(target) = target {
        arguments.extend(["-t".into(), target.into()]);
    }
    let mut pending = Vec::new();
    for command in [
        CommandInvocation::new("attach-session", arguments),
        CommandInvocation::new("refresh-client", ["-f", "no-output,ignore-size"]),
    ] {
        let request_id = client
            .execute(command)
            .map_err(|error| StreamError::Disconnected(error.to_string()))?;
        loop {
            match receive(&receiver)? {
                ProtocolMessage::CommandResponse(CommandResponse::Success {
                    request_id: response_id,
                    exit_code,
                    stderr,
                    ..
                }) if response_id == request_id => {
                    if exit_code != 0 {
                        return Err(StreamError::Disconnected(stderr));
                    }
                    break;
                }
                ProtocolMessage::CommandResponse(CommandResponse::Error {
                    request_id: response_id,
                    error,
                    ..
                }) if response_id == request_id => {
                    return Err(StreamError::Disconnected(error.tmux_message()));
                }
                ProtocolMessage::Event(event) => {
                    if matches!(&event.payload, EventPayload::HookEvent { .. }) {
                        pending.push(event.payload);
                    }
                }
                _ => {}
            }
        }
    }
    Ok((receiver, pending))
}

fn receive(
    receiver: &mpsc::Receiver<Result<ProtocolMessage, String>>,
) -> Result<ProtocolMessage, StreamError> {
    check_message(
        receiver
            .recv()
            .map_err(|_| StreamError::Disconnected("disconnected".to_owned()))?,
    )
}

fn check_message(message: Result<ProtocolMessage, String>) -> Result<ProtocolMessage, StreamError> {
    let message = message.map_err(StreamError::Disconnected)?;
    if let ProtocolMessage::Event(event) = &message {
        match &event.payload {
            EventPayload::ControlExit { reason } if reason == "too far behind" => {
                return Err(StreamError::Gap);
            }
            EventPayload::ControlExit { reason } => {
                return Err(StreamError::Disconnected(if reason.is_empty() {
                    "disconnected".to_owned()
                } else {
                    reason.clone()
                }));
            }
            EventPayload::ServerStopping => {
                return Err(StreamError::Disconnected("server stopped".to_owned()));
            }
            EventPayload::Detached { .. } => {
                return Err(StreamError::Disconnected("detached".to_owned()));
            }
            _ => {}
        }
    }
    Ok(message)
}

struct EventWriter<W> {
    output: W,
    next_seq: u64,
    target: Option<String>,
}

impl<W: Write> EventWriter<W> {
    fn line(&mut self, name: &str, variables: &BTreeMap<String, String>) -> io::Result<()> {
        let mut line = serde_json::Map::new();
        for (key, value) in variables {
            line.insert(key.clone(), serde_json::Value::String(value.clone()));
        }
        line.insert("seq".to_owned(), self.next_seq.into());
        line.insert("event".to_owned(), name.into());
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        line.insert("time".to_owned(), time.into());
        serde_json::to_writer(&mut self.output, &line)?;
        self.output.write_all(b"\n")?;
        self.output.flush()?;
        self.next_seq += 1;
        Ok(())
    }

    fn hook(&mut self, payload: EventPayload) -> io::Result<()> {
        let EventPayload::HookEvent { name, variables } = payload else {
            return Ok(());
        };
        if let Some(target) = &self.target {
            let key = match target.as_bytes().first() {
                Some(b'%') => "hook_pane",
                Some(b'@') => "hook_window",
                Some(b'$') => "hook_session",
                _ => "hook_session_name",
            };
            if variables.get(key) != Some(target) {
                return Ok(());
            }
        }
        self.line(&name, &variables)
    }
}

pub(crate) fn progress_target(arguments: &[RawText]) -> Result<Option<String>, &'static str> {
    let mut progress = false;
    let mut wait = false;
    let mut target = None;
    let mut explicit = false;
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        if argument == "--" {
            break;
        }
        match argument.as_str() {
            "--progress" => progress = true,
            "--wait" => wait = true,
            _ => {
                for name in ["-t", "--timeout", "--context", "--on-block"] {
                    let value = if argument == name {
                        index += 1;
                        arguments.get(index).map(RawText::as_str)
                    } else if name == "-t" {
                        argument
                            .strip_prefix(name)
                            .filter(|value| !value.is_empty())
                    } else {
                        argument
                            .strip_prefix(name)
                            .and_then(|value| value.strip_prefix('='))
                    };
                    if let Some(value) = value {
                        if name == "-t" {
                            target = Some(value.to_owned());
                            explicit = true;
                        }
                        break;
                    }
                }
            }
        }
        index += 1;
    }
    if !progress {
        return Ok(None);
    }
    if !wait
        || !explicit
        || !target.as_deref().is_some_and(|target| {
            target
                .strip_prefix('%')
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|ch| ch.is_ascii_digit()))
        })
    {
        return Err("agent-send: --progress requires --wait and an explicit -t %N");
    }
    Ok(target)
}

pub(crate) struct Progress {
    stop: Arc<AtomicBool>,
    client: Arc<InteractiveClient>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Progress {
    pub(crate) fn start(socket_path: &Path, pane: String) -> Result<Self, String> {
        let client = Arc::new(
            InteractiveClient::connect_control(socket_path)
                .map_err(|error| format_local_command_error(socket_path, error))?,
        );
        let mut progress = Self {
            stop: Arc::new(AtomicBool::new(false)),
            client,
            worker: None,
        };
        let (receiver, pending) =
            subscribe(&progress.client, Some(&pane)).map_err(|error| match error {
                StreamError::Gap => "event stream overflowed during attach".to_owned(),
                StreamError::Disconnected(reason) => reason,
            })?;
        let stop = Arc::clone(&progress.stop);
        progress.worker = Some(
            thread::Builder::new()
                .name("zz-agent-progress".to_owned())
                .spawn(move || {
                    let mut output = ProgressWriter::new(io::stderr(), pane);
                    let started = Instant::now();
                    for payload in pending {
                        if output.hook(payload, started.elapsed()).is_err() {
                            return;
                        }
                    }
                    loop {
                        if !stop.load(Ordering::Acquire)
                            && output.heartbeat(started.elapsed()).is_err()
                        {
                            break;
                        }
                        let message = if stop.load(Ordering::Acquire) {
                            match receiver.try_recv() {
                                Ok(message) => message,
                                Err(_) => break,
                            }
                        } else {
                            match receiver.recv_timeout(Duration::from_millis(100)) {
                                Ok(message) => message,
                                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        };
                        match check_message(message) {
                            Ok(ProtocolMessage::Event(event)) => {
                                if output.hook(event.payload, started.elapsed()).is_err() {
                                    break;
                                }
                            }
                            Ok(_) => {}
                            Err(error) => {
                                if !stop.load(Ordering::Acquire) {
                                    let reason = match error {
                                        StreamError::Gap => "event stream overflowed".to_owned(),
                                        StreamError::Disconnected(reason) => reason,
                                    };
                                    eprintln!("agent-send: --progress: {reason}");
                                }
                                break;
                            }
                        }
                    }
                })
                .map_err(|error| error.to_string())?,
        );
        Ok(progress)
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        #[cfg(unix)]
        let _ = self.client.shutdown();
        #[cfg(not(unix))]
        let _ = self.client.detach();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct ProgressWriter<W> {
    output: W,
    pane: String,
    last_line: Duration,
    tool_calls: usize,
    last_title: String,
}

impl<W: Write> ProgressWriter<W> {
    fn new(output: W, pane: String) -> Self {
        Self {
            output,
            pane,
            last_line: Duration::ZERO,
            tool_calls: 0,
            last_title: String::new(),
        }
    }

    fn line(&mut self, elapsed: Duration, text: &str) -> io::Result<()> {
        let seconds = elapsed.as_secs();
        writeln!(
            self.output,
            "[zz {} +{:02}:{:02}] {text}",
            self.pane,
            seconds / 60,
            seconds % 60
        )?;
        self.output.flush()?;
        self.last_line = elapsed;
        Ok(())
    }

    fn hook(&mut self, payload: EventPayload, elapsed: Duration) -> io::Result<()> {
        let EventPayload::HookEvent { name, variables } = payload else {
            return Ok(());
        };
        if variables.get("hook_pane") != Some(&self.pane) {
            return Ok(());
        }
        let value = |key: &str| variables.get(key).map(String::as_str).unwrap_or_default();
        match name.as_str() {
            "agent-tool-call" => {
                let title = value("tool_title")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(120)
                    .collect::<String>();
                self.last_title.clone_from(&title);
                let status = value("tool_status");
                let label = if matches!(status, "completed" | "failed") {
                    status
                } else {
                    self.tool_calls += 1;
                    match value("tool_kind") {
                        "" => "call",
                        kind => kind,
                    }
                };
                self.line(
                    elapsed,
                    &format!("tool_call {label} {}", serde_json::to_string(&title)?),
                )
            }
            "agent-state-changed" => {
                let mut state = value("agent_state").to_owned();
                let permission = value("agent_pending_permission");
                if !permission.is_empty() {
                    state.push_str(" permission=");
                    state.push_str(permission);
                }
                self.line(elapsed, &state)
            }
            _ => Ok(()),
        }
    }

    fn heartbeat(&mut self, elapsed: Duration) -> io::Result<()> {
        if elapsed.saturating_sub(self.last_line) >= Duration::from_mins(1) {
            self.line(
                elapsed,
                &format!(
                    "working tool_calls={} last={}",
                    self.tool_calls,
                    serde_json::to_string(&self.last_title)?
                ),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook() -> EventPayload {
        EventPayload::HookEvent {
            name: "agent-state-changed".to_owned(),
            variables: BTreeMap::from([
                ("hook".to_owned(), "agent-state-changed".to_owned()),
                ("hook_pane".to_owned(), "%2".to_owned()),
                ("hook_window".to_owned(), "@3".to_owned()),
                ("hook_session".to_owned(), "$4".to_owned()),
                ("hook_session_name".to_owned(), "work".to_owned()),
                ("agent_state".to_owned(), "working".to_owned()),
                ("agent_pending_permission".to_owned(), String::new()),
            ]),
        }
    }

    fn lines(output: &[u8]) -> Vec<serde_json::Value> {
        std::str::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn progress_formats_tools_states_permissions_and_heartbeat() {
        let mut writer = ProgressWriter::new(Vec::new(), "%7".to_owned());
        let tool = |status: &str| EventPayload::HookEvent {
            name: "agent-tool-call".to_owned(),
            variables: BTreeMap::from([
                ("hook_pane".to_owned(), "%7".to_owned()),
                ("tool_call_id".to_owned(), "one".to_owned()),
                ("tool_kind".to_owned(), "execute".to_owned()),
                (
                    "tool_title".to_owned(),
                    "cargo test -p zz-daemon agent".to_owned(),
                ),
                ("tool_status".to_owned(), status.to_owned()),
            ]),
        };
        writer
            .hook(tool("pending"), Duration::from_secs(12))
            .unwrap();
        writer
            .hook(tool("completed"), Duration::from_secs(41))
            .unwrap();
        let before_heartbeat = writer.output.len();
        writer.heartbeat(Duration::from_secs(100)).unwrap();
        assert_eq!(writer.output.len(), before_heartbeat);
        writer.heartbeat(Duration::from_secs(101)).unwrap();
        writer
            .hook(tool("failed"), Duration::from_secs(102))
            .unwrap();
        for (state, permission, seconds) in [("working", "42", 103), ("idle", "", 873)] {
            writer
                .hook(
                    EventPayload::HookEvent {
                        name: "agent-state-changed".to_owned(),
                        variables: BTreeMap::from([
                            ("hook_pane".to_owned(), "%7".to_owned()),
                            ("agent_state".to_owned(), state.to_owned()),
                            ("agent_pending_permission".to_owned(), permission.to_owned()),
                        ]),
                    },
                    Duration::from_secs(seconds),
                )
                .unwrap();
        }
        assert_eq!(
            String::from_utf8(writer.output).unwrap(),
            concat!(
                "[zz %7 +00:12] tool_call execute \"cargo test -p zz-daemon agent\"\n",
                "[zz %7 +00:41] tool_call completed \"cargo test -p zz-daemon agent\"\n",
                "[zz %7 +01:41] working tool_calls=1 last=\"cargo test -p zz-daemon agent\"\n",
                "[zz %7 +01:42] tool_call failed \"cargo test -p zz-daemon agent\"\n",
                "[zz %7 +01:43] working permission=42\n",
                "[zz %7 +14:33] idle\n",
            )
        );
    }

    #[test]
    fn progress_caps_and_quotes_titles_and_filters_events() {
        let mut writer = ProgressWriter::new(Vec::new(), "%7".to_owned());
        writer.hook(hook(), Duration::from_secs(1)).unwrap();
        writer
            .hook(EventPayload::ServerStopping, Duration::from_secs(1))
            .unwrap();
        writer
            .hook(
                EventPayload::HookEvent {
                    name: "pane-title-changed".to_owned(),
                    variables: BTreeMap::from([("hook_pane".to_owned(), "%7".to_owned())]),
                },
                Duration::from_secs(1),
            )
            .unwrap();
        assert!(writer.output.is_empty());
        writer.heartbeat(Duration::from_mins(1)).unwrap();
        assert_eq!(
            String::from_utf8(writer.output.clone()).unwrap(),
            "[zz %7 +01:00] working tool_calls=0 last=\"\"\n"
        );
        writer
            .hook(
                EventPayload::HookEvent {
                    name: "agent-tool-call".to_owned(),
                    variables: BTreeMap::from([
                        ("hook_pane".to_owned(), "%7".to_owned()),
                        ("tool_kind".to_owned(), "execute".to_owned()),
                        ("tool_status".to_owned(), "in_progress".to_owned()),
                        (
                            "tool_title".to_owned(),
                            format!("\n\"quoted\" \\ {}", "界".repeat(130)),
                        ),
                    ]),
                },
                Duration::from_secs(61),
            )
            .unwrap();
        let output = String::from_utf8(writer.output).unwrap();
        let title: String = serde_json::from_str(
            output
                .lines()
                .last()
                .unwrap()
                .split_once("execute ")
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(title.chars().count(), 120);
        assert!(title.starts_with("\"quoted\" \\ "));
        assert_eq!(writer.last_line, Duration::from_secs(61));
    }

    #[test]
    fn progress_argument_gate_handles_values_and_payloads() {
        for args in [
            vec!["--wait", "--progress", "-t", "%7", "hi"],
            vec!["hi", "-t%7", "--wait", "--progress"],
        ] {
            assert_eq!(
                progress_target(&args.into_iter().map(RawText::from).collect::<Vec<_>>()),
                Ok(Some("%7".to_owned()))
            );
        }
        for args in [
            vec!["--", "--progress"],
            vec!["--context", "--progress", "hi"],
            vec!["--context=--progress", "hi"],
        ] {
            assert_eq!(
                progress_target(&args.into_iter().map(RawText::from).collect::<Vec<_>>()),
                Ok(None)
            );
        }
        assert!(
            progress_target(&["--wait", "--progress", "--target", "%7"].map(RawText::from))
                .is_err()
        );
        for target in ["%", "%x", "%1:2", "work", "@1"] {
            assert!(
                progress_target(&["--wait", "--progress", "-t", target].map(RawText::from))
                    .is_err()
            );
        }
    }

    #[test]
    fn ready_hooks_and_gap_use_connection_sequence_and_copy_variables() {
        let mut writer = EventWriter {
            output: Vec::new(),
            next_seq: 0,
            target: None,
        };
        writer.line("ready", &BTreeMap::new()).unwrap();
        writer.hook(hook()).unwrap();
        writer.line("gap", &BTreeMap::new()).unwrap();
        writer.hook(hook()).unwrap();
        let rows = lines(&writer.output);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["event"], "ready");
        assert_eq!(rows[0].as_object().unwrap().len(), 3);
        assert_eq!(rows[2]["event"], "gap");
        for (seq, row) in rows.iter().enumerate() {
            assert_eq!(row["seq"], seq as u64);
            assert!(row["time"].as_u64().unwrap() > 0);
        }
        let EventPayload::HookEvent { name, variables } = hook() else {
            unreachable!();
        };
        assert_eq!(rows[1]["event"], name);
        for (key, value) in variables {
            assert_eq!(rows[1][&key], value);
        }
    }

    #[test]
    fn filters_match_exact_target_fields_and_keep_ready_and_gap() {
        for target in ["%2", "@3", "$4", "work", "%20", "@30", "$40", "wor"] {
            let mut writer = EventWriter {
                output: Vec::new(),
                next_seq: 0,
                target: Some(target.to_owned()),
            };
            writer.line("ready", &BTreeMap::new()).unwrap();
            writer.hook(hook()).unwrap();
            writer
                .hook(EventPayload::HookEvent {
                    name: "session-created".to_owned(),
                    variables: BTreeMap::new(),
                })
                .unwrap();
            writer.hook(EventPayload::ServerStopping).unwrap();
            writer.line("gap", &BTreeMap::new()).unwrap();
            let rows = lines(&writer.output);
            let matched = matches!(target, "%2" | "@3" | "$4" | "work");
            assert_eq!(rows.len(), if matched { 3 } else { 2 }, "{target}");
            assert_eq!(rows[0]["event"], "ready");
            assert_eq!(rows.last().unwrap()["event"], "gap");
            assert_eq!(rows.last().unwrap()["seq"], rows.len() as u64 - 1);
        }
    }
}
