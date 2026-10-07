use std::{
    io::{BufRead as _, BufReader, Write as _},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use async_channel::{Receiver, Sender};
use serde_json::{Value, json};
use zz_protocol::MAX_AGENT_RESULT_BYTES;

use crate::{
    agent::{
        runtime::{StderrTail, validate_payload},
        stream::{AgentPromptOutcome, AgentStreamPayload},
    },
    unmasked::SpawnUnmasked as _,
};

const REAP_GRACE: Duration = Duration::from_secs(2);
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

pub(crate) enum Input<C, K> {
    Command(Result<C, async_channel::RecvError>),
    Control(Result<K, async_channel::RecvError>),
    Child(Result<ChildEvent, async_channel::RecvError>),
    Deadline,
}

pub(crate) async fn next_input<C, K>(
    commands: &Receiver<C>,
    controls: &Receiver<K>,
    child: &Receiver<ChildEvent>,
    deadline: Option<Instant>,
) -> Input<C, K> {
    let timer = async {
        match deadline {
            Some(deadline) => {
                smol::Timer::at(deadline).await;
            }
            None => futures_lite::future::pending::<()>().await,
        }
        Input::Deadline
    };
    let controls = async {
        if controls.is_closed() && controls.is_empty() {
            futures_lite::future::pending::<()>().await;
        }
        Input::Control(controls.recv().await)
    };
    futures_lite::future::or(
        futures_lite::future::or(controls, async { Input::Child(child.recv().await) }),
        futures_lite::future::or(async { Input::Command(commands.recv().await) }, timer),
    )
    .await
}

pub(crate) enum ChildEvent {
    Line(u64, Vec<u8>),
    Closed(u64),
}

pub(crate) struct Process {
    child: Option<Child>,
    stdin: Sender<String>,
}

impl Process {
    pub(crate) fn send(&self, frame: &Value) {
        let _ = self.stdin.try_send(frame.to_string());
    }

    pub(crate) fn exit_detail(&mut self) -> Option<String> {
        let child = self.child.as_mut()?;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status.to_string()),
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                _ => return None,
            }
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.stdin.close();
        let Some(mut child) = self.child.take() else {
            return;
        };
        let spawned = thread::Builder::new()
            .name("zz-claude-reap".to_owned())
            .spawn(move || reap(&mut child));
        if let Err(error) = spawned {
            log::warn!(target: "zz::agent", "could not reap Claude Code: {error}");
        }
    }
}

fn reap(child: &mut Child) {
    let deadline = Instant::now() + REAP_GRACE;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    signal_group(child, Signal::Terminate);
    let deadline = Instant::now() + REAP_GRACE;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    signal_group(child, Signal::Kill);
    let _ = child.wait();
}

#[derive(Clone, Copy)]
enum Signal {
    Terminate,
    Kill,
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "signal the process group zz created for this Claude Code child"
)]
fn signal_group(child: &mut Child, signal: Signal) {
    let Ok(pid) = libc::pid_t::try_from(child.id()) else {
        return;
    };
    let signal = match signal {
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    unsafe {
        libc::killpg(pid, signal);
    }
}

#[cfg(not(unix))]
fn signal_group(child: &mut Child, _signal: Signal) {
    let _ = child.kill();
}

impl Process {
    pub(crate) fn launch(
        command: &mut Command,
        agent: &str,
        generation: u64,
        events: &Sender<ChildEvent>,
        tail: &StderrTail,
    ) -> Result<Self, String> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            command.process_group(0);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn_unmasked()
            .map_err(|error| format!("could not start {agent}: {error}"))?;
        let (stdin_tx, stdin_rx) = async_channel::unbounded::<String>();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let process = Self {
            child: Some(child),
            stdin: stdin_tx,
        };
        if let Some(mut stdin) = stdin {
            spawn_thread("zz-agent-in", move || {
                while let Ok(line) = stdin_rx.recv_blocking() {
                    if stdin
                        .write_all(line.as_bytes())
                        .and_then(|()| stdin.write_all(b"\n"))
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        }
        if let Some(stdout) = stdout {
            let events = events.clone();
            let agent = agent.to_owned();
            spawn_thread("zz-agent-out", move || {
                let mut reader = BufReader::with_capacity(1 << 16, stdout);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match reader.read_until(b'\n', &mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) if line.len() > MAX_FRAME_BYTES => {
                            log::warn!(target: "zz::agent", "dropping a {} byte {agent} frame", line.len());
                        }
                        Ok(_) => {
                            if events
                                .send_blocking(ChildEvent::Line(
                                    generation,
                                    std::mem::take(&mut line),
                                ))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                let _ = events.send_blocking(ChildEvent::Closed(generation));
            })?;
        }
        if let Some(stderr) = stderr {
            let tail = tail.clone();
            spawn_thread("zz-agent-err", move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::warn!(target: "zz::agent::stderr", "{line}");
                    tail.push(&line);
                }
            })?;
        }
        Ok(process)
    }
}

pub(crate) fn spawn_thread(name: &str, body: impl FnOnce() + Send + 'static) -> Result<(), String> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(|_| ())
        .map_err(|error| format!("could not start {name}: {error}"))
}

pub(crate) fn cancelled() -> AgentPromptOutcome {
    AgentPromptOutcome::Finished {
        stop_reason: Value::from("cancelled"),
    }
}

pub(crate) fn option(id: &str, name: &str, kind: &str) -> Value {
    json!({ "optionId": id, "name": name, "kind": kind })
}

pub(crate) fn fit_update(mut update: Value) -> Value {
    let fits = |update: &Value| {
        validate_payload(&AgentStreamPayload::Update {
            update: update.clone(),
        })
        .is_ok()
    };
    if fits(&update) {
        return update;
    }
    if let Some(fields) = update.as_object_mut() {
        fields.remove("rawInput");
        fields.remove("rawOutput");
    }
    if fits(&update) {
        return update;
    }
    if update.get("content").is_some_and(Value::is_array) {
        update["content"] = json!([{
            "type": "content",
            "content": { "type": "text", "text": format!("[output larger than {} KiB not shown]", MAX_AGENT_RESULT_BYTES / 1024) },
        }]);
    } else if update["content"]["type"] == "text"
        && let Some(text) = update["content"]["text"].as_str()
    {
        let mut end = MAX_AGENT_RESULT_BYTES / 2;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        update["content"]["text"] = Value::from(format!("{}…", &text[..end]));
    }
    update
}

pub(crate) fn random_u64() -> u64 {
    getrandom::u64().unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos() as u64)
    })
}

pub(crate) fn new_uuid() -> String {
    let high = random_u64();
    let low = random_u64();
    let high = (high & 0xffff_ffff_ffff_0fff) | 0x0000_0000_0000_4000;
    let low = (low & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        high >> 32,
        (high >> 16) & 0xffff,
        high & 0xffff,
        low >> 48,
        low & 0xffff_ffff_ffff
    )
}
