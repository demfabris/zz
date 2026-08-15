//! Streaming soak for the daemon-owned agent runtime: performance gate 2 of
//! knowledge/designs/agent-daemon-runtime.md.
//!
//! The fake adapter is a POSIX awk program rather than the in-process fixture:
//! an integration test only sees the crate's public surface, and
//! `agent::fixture` is private to the library's own test build. A real child
//! over real stdio costs nothing here and covers one more seam — the ACP
//! decode — than the in-process one would.

#![cfg(all(unix, feature = "agent"))]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::BufReader,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use zz_daemon::{CommandClient, Daemon, DaemonError};
use zz_protocol::{
    ClientHello, ClientKind, CommandInvocation, Event, EventPayload, InputMessage,
    PROTOCOL_VERSION, PaneId, ProtocolMessage, agent_update_batch_bytes, read_protocol_message,
    write_protocol_message,
};

/// A fake ACP adapter: answers `initialize` and `session/new`, then streams
/// `items` session updates per prompt before ending the turn. Four in five are
/// text deltas; the fifth is a tool call carrying a JSON blob, so the mix on
/// the wire matches what a real turn coalesces.
const ADAPTER: &str = r#"
function request_id(line) {
  if (match(line, /"id":"[^"]*"/)) {
    return substr(line, RSTART + 5, RLENGTH - 5)
  }
  if (match(line, /"id":[0-9]+/)) {
    return substr(line, RSTART + 5, RLENGTH - 5)
  }
  return "null"
}
BEGIN {
  session = "zz-soak-session"
  while (length(text) < text_bytes) { text = text "the quick brown fox jumps over the lazy dog " }
  text = substr(text, 1, text_bytes)
  while (length(blob) < blob_bytes) { blob = blob "0123456789abcdef" }
  blob = substr(blob, 1, blob_bytes)
}
/"method":"initialize"/ {
  printf "{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"protocolVersion\":1,\"agentInfo\":{\"name\":\"zz-soak\",\"version\":\"1\"}}}\n", request_id($0)
  fflush()
  next
}
/"method":"session\/new"/ {
  printf "{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"sessionId\":\"%s\"}}\n", request_id($0), session
  fflush()
  next
}
/"method":"session\/prompt"/ {
  id = request_id($0)
  for (i = 1; i <= items; i++) {
    if (i % 5 == 0) {
      printf "{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"%s\",\"update\":{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"call-%d\",\"title\":\"read chunk %d\",\"kind\":\"read\",\"status\":\"pending\",\"rawInput\":{\"path\":\"/work/zz/crates/zz-daemon/src/agent/fanout.rs\",\"offset\":%d,\"blob\":\"%s\"}}}}\n", session, i, i, i, blob
    } else {
      printf "{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"%s\",\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"%d %s\"}}}}\n", session, i, text
    }
  }
  printf "{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"stopReason\":\"end_turn\"}}\n", id
  fflush()
  next
}
"#;

const SOAK_ITEMS: usize = 50_000;
const SOAK_TEXT_BYTES: usize = 120;
const SOAK_BLOB_BYTES: usize = 900;
const SLOW_ITEMS: usize = 1_500;
const SLOW_TEXT_BYTES: usize = 8_000;
const SLOW_BLOB_BYTES: usize = 8_000;

/// A terminal that echoes what is typed at it, so the terminal lane can be
/// watched while the agent lane is jammed.
const TERMINAL_COMMAND: &str = "printf 'zz-soak-ready\\r\\n'; exec /bin/cat";

const SESSION: &str = "soak";

struct Soak {
    socket: PathBuf,
    script: PathBuf,
    commands: CommandClient,
    daemon: Option<JoinHandle<Result<(), DaemonError>>>,
    terminal: PaneId,
    agent: PaneId,
}

impl Soak {
    fn start(name: &str, items: usize, text_bytes: usize, blob_bytes: usize) -> Self {
        let socket = PathBuf::from(format!("/tmp/zz-{name}-{}.sock", std::process::id()));
        let script = PathBuf::from(format!("/tmp/zz-{name}-{}.awk", std::process::id()));
        let _ = fs::remove_file(&socket);
        fs::write(&script, ADAPTER).expect("write the fake adapter");

        let daemon = Daemon::new(&socket).without_user_config();
        let handle = thread::spawn(move || daemon.run_foreground());
        let mut commands = connect_command_retry(&socket);

        let adapter = format!(
            "{} -v items={items} -v text_bytes={text_bytes} -v blob_bytes={blob_bytes} -f {}",
            awk_invocation(),
            script.display(),
        );
        for command in [
            CommandInvocation::new("new-session", ["-d", "-s", SESSION, TERMINAL_COMMAND]),
            CommandInvocation::new("set-option", ["-g", "experimental-agent-pane", "on"]),
            CommandInvocation::new("set-option", ["-g", "--", "agent-command", &adapter]),
            CommandInvocation::new(
                "set-option",
                ["-g", "--", "agent-claude-code-command", &adapter],
            ),
        ] {
            commands.execute(command).expect("workspace command");
        }

        let before = panes(&mut commands);
        let terminal = *before.first().expect("the session's terminal pane");
        commands
            .execute(CommandInvocation::new(
                "new-pane",
                ["-v", "-t", &terminal.to_string()],
            ))
            .expect("split a pane for the agent");
        let agent = *panes(&mut commands)
            .difference(&before)
            .next()
            .expect("the new pane");
        commands
            .execute(CommandInvocation::new(
                "select-pane-kind",
                ["-t", &agent.to_string(), "agent"],
            ))
            .expect("materialize the agent pane");

        Self {
            socket,
            script,
            commands,
            daemon: Some(handle),
            terminal,
            agent,
        }
    }

    fn attach(&self) -> Peer {
        let mut peer = Peer::connect(&self.socket);
        peer.send(&ProtocolMessage::Attach {
            session: SESSION.to_owned(),
        });
        peer.send(&ProtocolMessage::Input(InputMessage::ResizeTerminal {
            pane: self.terminal,
            columns: 80,
            rows: 24,
            cell_width_px: 8,
            cell_height_px: 16,
        }));
        peer.send(&ProtocolMessage::AgentReplay {
            pane: self.agent,
            from_seq: 0,
        });
        peer
    }

    fn prompt(&self, peer: &mut Peer, text: &str) {
        peer.send(&ProtocolMessage::AgentPrompt {
            pane: self.agent,
            text: text.to_owned(),
            images: Vec::new(),
        });
    }
}

impl Drop for Soak {
    fn drop(&mut self) {
        let _ = self
            .commands
            .execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        if let Some(daemon) = self.daemon.take() {
            let _ = daemon.join();
        }
        let _ = fs::remove_file(&self.script);
        let _ = fs::remove_file(&self.socket);
    }
}

/// A headless interactive client. The published `InteractiveClient` cannot
/// send agent messages, so the soak speaks the wire itself.
struct Peer {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Peer {
    fn connect(socket: &Path) -> Self {
        let stream = UnixStream::connect(socket).expect("connect to the daemon");
        stream
            .set_read_timeout(Some(Duration::from_mins(2)))
            .expect("set a read deadline");
        let writer = stream.try_clone().expect("clone the client stream");
        let mut peer = Self {
            reader: BufReader::new(stream),
            writer,
        };
        peer.send(&ProtocolMessage::ClientHello(ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_instance_id: zz_protocol::ClientInstanceId(1),
            kind: ClientKind::Interactive,
            device_name: Some("soak".to_owned()),
            capabilities: Vec::new(),
            color_scheme: None,
            origin: None,
        }));
        assert!(
            matches!(peer.recv(), ProtocolMessage::ServerHello(_)),
            "the daemon answers a hello with a hello"
        );
        peer
    }

    fn send(&mut self, message: &ProtocolMessage) {
        write_protocol_message(&mut self.writer, message).expect("send a client message");
    }

    fn recv(&mut self) -> ProtocolMessage {
        read_protocol_message(&mut self.reader).expect("read a daemon message")
    }
}

#[derive(Default)]
struct Transcript {
    applied: BTreeMap<u64, Vec<u8>>,
    frames: usize,
    bytes: usize,
    lagged: Vec<u64>,
    terminal_frames: usize,
    widest_frame: usize,
    largest_frame: usize,
}

impl Transcript {
    fn observe(&mut self, message: ProtocolMessage, agent: PaneId, terminal: PaneId) {
        let ProtocolMessage::Event(Event { payload, .. }) = message else {
            return;
        };
        match payload {
            EventPayload::AgentUpdates {
                pane,
                first_seq,
                items,
            } if pane == agent => {
                self.frames += 1;
                let bytes = agent_update_batch_bytes(&items);
                self.bytes += bytes;
                self.widest_frame = self.widest_frame.max(items.len());
                self.largest_frame = self.largest_frame.max(bytes);
                for (index, item) in items.into_iter().enumerate() {
                    self.applied
                        .insert(first_seq + u64::try_from(index).unwrap_or(0), item);
                }
            }
            EventPayload::AgentLagged { pane, next_seq } if pane == agent => {
                self.lagged.push(next_seq);
            }
            EventPayload::TerminalViewport { pane, .. }
            | EventPayload::TerminalPatch { pane, .. }
                if pane == terminal =>
            {
                self.terminal_frames += 1;
            }
            _ => {}
        }
    }

    /// The last sequence the client can claim to have applied: the end of the
    /// gapless prefix, which is exactly what a replay resumes from.
    fn contiguous(&self) -> u64 {
        let mut seq = 0;
        while self.applied.contains_key(&(seq + 1)) {
            seq += 1;
        }
        seq
    }

    fn kind(&self, seq: u64) -> Option<String> {
        let item = self.applied.get(&seq)?;
        let value = serde_json::from_slice::<serde_json::Value>(item).ok()?;
        Some(value.get("item")?.as_str()?.to_owned())
    }

    fn kinds(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for seq in self.applied.keys() {
            let kind = self.kind(*seq).unwrap_or_else(|| "?".to_owned());
            *counts.entry(kind).or_default() += 1;
        }
        counts
    }

    /// Whether the gapless prefix ends on the item that closes the turn.
    fn turn_finished(&self) -> bool {
        let last = self.contiguous();
        last > 0 && self.kind(last).as_deref() == Some("promptFinished")
    }
}

#[track_caller]
fn drain_until(
    peer: &mut Peer,
    transcript: &mut Transcript,
    soak: &Soak,
    what: &str,
    timeout: Duration,
    done: impl Fn(&Transcript) -> bool,
) {
    let deadline = Instant::now() + timeout;
    let mut answered = transcript.lagged.len();
    while !done(transcript) {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: applied={} frames={} lagged={:?}",
            transcript.contiguous(),
            transcript.frames,
            transcript.lagged,
        );
        let message = peer.recv();
        transcript.observe(message, soak.agent, soak.terminal);
        // What a real client does with a lag marker, and the only way a
        // transcript with a hole in it can converge.
        if transcript.lagged.len() > answered {
            answered = transcript.lagged.len();
            let from_seq = transcript.contiguous() + 1;
            peer.send(&ProtocolMessage::AgentReplay {
                pane: soak.agent,
                from_seq,
            });
        }
    }
}

fn connect_command_retry(socket: &Path) -> CommandClient {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match CommandClient::connect(socket) {
            Ok(client) => return client,
            Err(error) if Instant::now() >= deadline => panic!("daemon did not start: {error}"),
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn panes(commands: &mut CommandClient) -> BTreeSet<PaneId> {
    commands
        .execute(CommandInvocation::new(
            "list-panes",
            ["-t", "soak:0", "-F", "#{pane_id}"],
        ))
        .expect("list panes")
        .lines()
        .filter_map(|line| line.trim().parse::<PaneId>().ok())
        .collect()
}

fn awk_path() -> PathBuf {
    let path = std::env::var("PATH").unwrap_or_default();
    std::iter::once(PathBuf::from("/usr/bin/awk"))
        .chain(path.split(':').map(|entry| Path::new(entry).join("awk")))
        .find(|candidate| candidate.is_file())
        .expect("the soak adapter needs awk")
}

/// How to run that awk so it answers a request the moment one arrives.
///
/// mawk — Debian's and Ubuntu's `/usr/bin/awk` — fills its input buffer before
/// it splits records, so a peer whose stdin stays open, which is every ACP
/// connection, leaves a request sitting unread in the pipe until EOF. The
/// adapter would never answer `initialize` and the daemon would fail the pane
/// on the initialize timeout. `-W interactive` binds its reads to lines. The
/// BSD awk that macOS ships and gawk are line-bound already, and neither
/// reports itself as mawk here.
fn awk_invocation() -> String {
    let awk = awk_path();
    let is_mawk = Command::new(&awk)
        .args(["-W", "version"])
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|version| {
            [version.stdout, version.stderr]
                .iter()
                .any(|stream| String::from_utf8_lossy(stream).starts_with("mawk"))
        });
    if is_mawk {
        format!("{} -W interactive", awk.display())
    } else {
        awk.display().to_string()
    }
}

/// Cumulative CPU time of this process, which hosts both the daemon and the
/// client. `ps` is the only portable reach for it without a dependency; the
/// adapter child's own time is not included.
fn process_cpu_seconds() -> Option<f64> {
    let output = Command::new("ps")
        .args(["-o", "time=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let reported = String::from_utf8(output.stdout).ok()?;
    let mut seconds = 0.0;
    for field in reported.trim().split(':') {
        seconds = seconds * 60.0 + field.parse::<f64>().ok()?;
    }
    Some(seconds)
}

#[test]
#[ignore = "streaming soak: minutes of adapter output, run it explicitly"]
fn agent_stream_soak() {
    let soak = Soak::start("soak", SOAK_ITEMS, SOAK_TEXT_BYTES, SOAK_BLOB_BYTES);
    let mut peer = soak.attach();
    let mut transcript = Transcript::default();
    drain_until(
        &mut peer,
        &mut transcript,
        &soak,
        "the session to be ready",
        Duration::from_secs(30),
        |transcript| {
            (1..=transcript.contiguous())
                .any(|seq| transcript.kind(seq).as_deref() == Some("sessionReady"))
        },
    );
    let preamble = transcript.contiguous();

    let cpu_before = process_cpu_seconds();
    let started = Instant::now();
    soak.prompt(&mut peer, "soak the stream");
    drain_until(
        &mut peer,
        &mut transcript,
        &soak,
        "the whole turn",
        Duration::from_mins(5),
        Transcript::turn_finished,
    );
    let elapsed = started.elapsed();
    let cpu = cpu_before.zip(process_cpu_seconds());

    let applied = transcript.contiguous();
    let items = applied - preamble;
    let frames = u64::try_from(transcript.frames).unwrap_or(u64::MAX);
    let ratio = items as f64 / frames as f64;
    let rate = items as f64 / elapsed.as_secs_f64();
    let kinds = transcript.kinds();

    println!("--- agent stream soak ---");
    println!("preamble items      {preamble}");
    println!("turn items          {items}");
    println!("wall time           {:.3}s", elapsed.as_secs_f64());
    println!("items/sec           {rate:.0}");
    println!("wire frames         {frames}");
    println!("coalescing ratio    {ratio:.1} items/frame");
    println!(
        "wire bytes          {} ({:.1} MiB)",
        transcript.bytes,
        transcript.bytes as f64 / (1024.0 * 1024.0)
    );
    println!("widest frame        {} items", transcript.widest_frame);
    println!("largest frame       {} bytes", transcript.largest_frame);
    println!("lag markers         {}", transcript.lagged.len());
    println!("item kinds          {kinds:?}");
    match cpu {
        Some((before, after)) => println!("daemon+client cpu   {:.2}s", after - before),
        None => println!("daemon+client cpu   unavailable (ps)"),
    }
    println!(
        "peak lane depth     unavailable: the outbound mailbox is private to the daemon; \
         the widest frame above is the client-visible proxy"
    );
    println!(
        "SOAK items={items} secs={:.3} items_per_sec={rate:.0} frames={frames} ratio={ratio:.1} bytes={}",
        elapsed.as_secs_f64(),
        transcript.bytes,
    );

    assert_eq!(
        applied,
        u64::try_from(transcript.applied.len()).unwrap_or(u64::MAX),
        "the client applied a gapless transcript"
    );
    assert_eq!(
        items,
        u64::try_from(SOAK_ITEMS + 2).unwrap_or(u64::MAX),
        "the turn is its start, every streamed update, and the item that closes it"
    );
    assert_eq!(
        kinds.get("update").copied(),
        Some(SOAK_ITEMS),
        "every streamed update reached the client exactly once"
    );
    assert!(
        ratio >= 20.0,
        "the 25ms window must batch: {ratio:.1} items per frame"
    );
    assert!(
        transcript.lagged.is_empty(),
        "an actively draining client never lags: {:?}",
        transcript.lagged
    );
}

#[test]
fn agent_stream_soak_slow_client() {
    let mut soak = Soak::start("soak-slow", SLOW_ITEMS, SLOW_TEXT_BYTES, SLOW_BLOB_BYTES);
    let mut peer = soak.attach();
    let mut transcript = Transcript::default();
    drain_until(
        &mut peer,
        &mut transcript,
        &soak,
        "the session to be ready",
        Duration::from_secs(30),
        |transcript| {
            (1..=transcript.contiguous())
                .any(|seq| transcript.kind(seq).as_deref() == Some("sessionReady"))
        },
    );

    soak.prompt(&mut peer, "soak a client that stops reading");
    thread::sleep(Duration::from_secs(2));
    let terminal = soak.terminal.to_string();
    for keys in [
        ["-t", terminal.as_str(), "zz-soak-marker"],
        ["-t", terminal.as_str(), "Enter"],
    ] {
        soak.commands
            .execute(CommandInvocation::new("send-keys", keys))
            .expect("type at the terminal");
    }
    thread::sleep(Duration::from_secs(1));

    drain_until(
        &mut peer,
        &mut transcript,
        &soak,
        "the lag marker",
        Duration::from_mins(1),
        |transcript| !transcript.lagged.is_empty(),
    );
    let first_marker = transcript.lagged.len();
    drain_until(
        &mut peer,
        &mut transcript,
        &soak,
        "the replay to converge",
        Duration::from_mins(1),
        Transcript::turn_finished,
    );

    let applied = transcript.contiguous();
    println!(
        "slow client: applied={applied} frames={} lag markers={:?} terminal frames={}",
        transcript.frames, transcript.lagged, transcript.terminal_frames,
    );

    assert!(
        first_marker >= 1,
        "the bounded agent lane trips into a lag marker"
    );
    assert!(
        transcript.terminal_frames > 0,
        "the terminal lane keeps flowing while the agent lane overflows"
    );
    assert_eq!(
        transcript.kinds().get("update").copied(),
        Some(SLOW_ITEMS),
        "the replay converges the client on the whole transcript"
    );
    assert_eq!(
        applied,
        u64::try_from(transcript.applied.len()).unwrap_or(u64::MAX),
        "and leaves it gapless"
    );

    let captured = soak
        .commands
        .execute(CommandInvocation::new(
            "capture-pane",
            ["-p", "-t", terminal.as_str()],
        ))
        .expect("the daemon still answers commands");
    assert!(
        captured.contains("zz-soak-marker"),
        "the terminal pane kept running: {captured}"
    );
}
