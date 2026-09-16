use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, mpsc},
    thread,
    time::{SystemTime, UNIX_EPOCH},
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
    if let Some(target) = &output.target {
        arguments.extend(["-t".into(), target.clone().into()]);
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

fn receive(
    receiver: &mpsc::Receiver<Result<ProtocolMessage, String>>,
) -> Result<ProtocolMessage, StreamError> {
    let message = receiver
        .recv()
        .map_err(|_| StreamError::Disconnected("disconnected".to_owned()))?
        .map_err(StreamError::Disconnected)?;
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
