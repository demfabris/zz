use super::tests::{connect_command_retry, daemon_test_endpoint};
use super::*;
use crate::{CommandClient, ExecChain, ExecChainEnd};

struct RunningDaemon {
    socket: PathBuf,
    server_id: u64,
    thread: Option<thread::JoinHandle<Result<(), DaemonError>>>,
}

impl RunningDaemon {
    fn start(name: &str) -> Self {
        let socket = daemon_test_endpoint(name);
        let daemon = Daemon::new(&socket).without_user_config();
        let (ready, started) = mpsc::channel();
        let thread = thread::spawn(move || {
            daemon.run_foreground_with_ready(|server_id| {
                let _ = ready.send(server_id);
            })
        });
        let server_id = started
            .recv_timeout(Duration::from_secs(20))
            .expect("daemon ready");
        Self {
            socket,
            server_id,
            thread: Some(thread),
        }
    }

    fn client(&self) -> CommandClient {
        connect_command_retry(&self.socket)
    }
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        if let Ok(mut client) = CommandClient::connect(&self.socket) {
            let _ = client.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(client: &mut CommandClient, args: &[&str]) -> String {
    client
        .execute(CommandInvocation::new(args[0], args[1..].iter().copied()))
        .expect("command")
}

#[test]
fn one_connection_runs_repeated_execs_under_one_client() {
    let daemon = RunningDaemon::start("exec-repeat");
    let mut client = daemon.client();
    run(&mut client, &["new-session", "-d", "-s", "work"]);
    assert_eq!(client.server_id().expect("identity"), daemon.server_id);
    let first = run(&mut client, &["display-message", "-p", "#{session_name}"]);
    let second = run(&mut client, &["display-message", "-p", "#{window_index}"]);
    assert_eq!((first.as_str(), second.as_str()), ("work", "0"));
    let other = run(&mut daemon.client(), &["display-message", "-p", "x"]);
    assert_eq!(other, "x");
}

#[test]
fn a_zero_command_exec_is_a_probe_that_runs_nothing() {
    let daemon = RunningDaemon::start("exec-probe");
    let mut client = daemon.client();
    assert_eq!(client.server_id().expect("probe"), daemon.server_id);
    assert_eq!(client.server_id().expect("cached"), daemon.server_id);
    let sessions = run(&mut client, &["list-sessions", "-F", "#{session_name}"]);
    assert!(sessions.is_empty(), "{sessions:?}");
}

#[test]
fn a_mismatched_server_id_runs_nothing_and_the_right_one_runs_both_commands() {
    let daemon = RunningDaemon::start("exec-expect");
    let mut client = daemon.client();
    run(&mut client, &["new-session", "-d", "-s", "base"]);
    assert_eq!(
        client
            .execute_on_server(
                daemon.server_id.wrapping_add(1),
                CommandInvocation::new("new-session", ["-d", "-s", "stray"]),
            )
            .expect("mismatch is not an error"),
        None
    );
    assert!(
        client
            .execute(CommandInvocation::new("has-session", ["-t", "stray"]))
            .is_err()
    );
    let mut settings = daemon.client();
    assert_eq!(
        settings
            .execute_on_server(
                daemon.server_id,
                CommandInvocation::new("set-option", ["-g", "history-limit", "1234"]),
            )
            .expect("matching daemon"),
        Some(String::new())
    );
    assert_eq!(
        run(&mut settings, &["show-options", "-gqv", "history-limit"]),
        "1234"
    );
}

#[test]
fn a_chain_stops_at_the_first_failure_and_keeps_earlier_output() {
    let daemon = RunningDaemon::start("exec-chain");
    let mut client = daemon.client();
    let mut outputs = Vec::new();
    let end = client
        .exec_chain(
            ExecChain::new(vec![
                CommandInvocation::new("display-message", ["-p", "first"]),
                CommandInvocation::new("select-window", ["-t", "missing:9"]),
                CommandInvocation::new("display-message", ["-p", "never"]),
            ]),
            |outcome| {
                outputs.push(outcome.stdout.to_string());
                0
            },
        )
        .expect("daemon answered");
    assert_eq!(outputs, ["first"]);
    assert!(matches!(end, ExecChainEnd::Failed(_)), "{end:?}");
    let mut statuses = Vec::new();
    let end = client
        .exec_chain(
            ExecChain::new(vec![
                CommandInvocation::new("run-shell", ["exit 3"]),
                CommandInvocation::new("display-message", ["-p", "after"]),
            ]),
            |outcome| {
                statuses.push(outcome.exit_code);
                0
            },
        )
        .expect("daemon answered");
    assert_eq!(statuses, [3, 0]);
    assert!(matches!(end, ExecChainEnd::Ran { exit_code: 3 }), "{end:?}");
}

#[test]
fn a_chain_longer_than_the_mailbox_runs_to_the_end_and_returns_every_line() {
    let daemon = RunningDaemon::start("exec-long");
    let mut client = daemon.client();
    let wide = "w".repeat(4000);
    let mut commands = (1..=300)
        .map(|index| {
            CommandInvocation::new(
                "set-option",
                ["-g".to_owned(), format!("@x{index}"), "y".to_owned()],
            )
        })
        .collect::<Vec<_>>();
    commands.extend((1..=300).map(|index| {
        CommandInvocation::new(
            "display-message",
            ["-p".to_owned(), format!("line {index}")],
        )
    }));
    commands
        .extend((0..40).map(|_| CommandInvocation::new("display-message", ["-p", wide.as_str()])));
    let mut outputs = Vec::new();
    let end = client
        .exec_chain(ExecChain::new(commands), |outcome| {
            outputs.push(outcome.stdout.to_string());
            0
        })
        .expect("daemon answered");
    assert!(matches!(end, ExecChainEnd::Ran { exit_code: 0 }), "{end:?}");
    assert_eq!(outputs.len(), 640);
    assert_eq!(outputs[300], "line 1");
    assert_eq!(outputs[599], "line 300");
    assert!(outputs[600..].iter().all(|output| *output == wide));
    assert_eq!(run(&mut client, &["show-options", "-gqv", "@x300"]), "y");
}

#[cfg(unix)]
mod dropped_exec {
    use std::os::unix::net::{UnixListener, UnixStream};

    use super::*;
    use zz_protocol::{EXEC_CAPABILITY, read_protocol_message, write_protocol_message};

    #[derive(Clone, Copy)]
    enum First {
        RunThenDrop,
        DropUnread,
    }

    fn relay_hello(client: UnixStream, daemon: &Path, hide_exec: bool) {
        let Ok(mut upstream) = UnixStream::connect(daemon) else {
            return;
        };
        let (Ok(mut inbound), Ok(mut outbound)) = (client.try_clone(), upstream.try_clone()) else {
            return;
        };
        let forward = thread::spawn(move || {
            let _ = std::io::copy(&mut inbound, &mut outbound);
            let _ = outbound.shutdown(std::net::Shutdown::Write);
        });
        let mut client = client;
        while let Ok(mut message) = read_protocol_message(&mut upstream) {
            if hide_exec && let ProtocolMessage::ServerHello(hello) = &mut message {
                hello
                    .capabilities
                    .retain(|capability| capability != EXEC_CAPABILITY);
            }
            if write_protocol_message(&mut client, &message).is_err() {
                break;
            }
        }
        let _ = client.shutdown(std::net::Shutdown::Both);
        let _ = forward.join();
    }

    fn proxy(daemon: &Path, name: &str, first: First, hide_exec: bool) -> PathBuf {
        let path = daemon_test_endpoint(name);
        let listener = UnixListener::bind(&path).expect("bind proxy");
        let daemon = daemon.to_path_buf();
        thread::spawn(move || {
            let Ok((mut client, _)) = listener.accept() else {
                return;
            };
            let exec = read_protocol_message(&mut client).expect("exec frame");
            if let First::RunThenDrop = first {
                let mut upstream = UnixStream::connect(&daemon).expect("daemon");
                write_protocol_message(&mut upstream, &exec).expect("forward exec");
                while !matches!(
                    read_protocol_message(&mut upstream),
                    Ok(ProtocolMessage::ExecExit(_)) | Err(_)
                ) {}
            }
            drop(client);
            while let Ok((client, _)) = listener.accept() {
                let daemon = daemon.clone();
                thread::spawn(move || relay_hello(client, &daemon, hide_exec));
            }
        });
        path
    }

    fn windows_named(daemon: &RunningDaemon, name: &str) -> usize {
        run(
            &mut daemon.client(),
            &["list-windows", "-a", "-F", "#{window_name}"],
        )
        .lines()
        .filter(|line| *line == name)
        .count()
    }

    fn exec_through(path: &Path, name: &str) -> ExecChainEnd {
        let mut client = CommandClient::connect(path).expect("connect proxy");
        client
            .exec_chain(
                ExecChain::new(vec![CommandInvocation::new(
                    "new-window",
                    ["-d", "-n", name],
                )]),
                |_| 0,
            )
            .expect("chain ends")
    }

    #[test]
    fn an_exec_daemon_that_drops_the_reply_does_not_run_the_chain_twice() {
        let daemon = RunningDaemon::start("exec-dropped");
        run(&mut daemon.client(), &["new-session", "-d", "-s", "work"]);
        let path = proxy(
            &daemon.socket,
            "exec-dropped-proxy",
            First::RunThenDrop,
            false,
        );
        let end = exec_through(&path, "once");
        assert!(matches!(end, ExecChainEnd::Failed(_)), "{end:?}");
        assert_eq!(windows_named(&daemon, "once"), 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_daemon_without_exec_gets_the_chain_once_over_the_hello_path() {
        let daemon = RunningDaemon::start("exec-legacy");
        run(&mut daemon.client(), &["new-session", "-d", "-s", "work"]);
        let path = proxy(&daemon.socket, "exec-legacy-proxy", First::DropUnread, true);
        let end = exec_through(&path, "legacy");
        assert!(matches!(end, ExecChainEnd::Ran { exit_code: 0 }), "{end:?}");
        assert_eq!(windows_named(&daemon, "legacy"), 1);
        let _ = fs::remove_file(path);
    }
}

#[test]
fn a_preparation_error_rejects_the_whole_chain_before_it_runs() {
    let daemon = RunningDaemon::start("exec-reject");
    let mut client = daemon.client();
    let mut ran = 0;
    let end = client
        .exec_chain(
            ExecChain::new(vec![
                CommandInvocation::new("new-session", ["-d", "-s", "early"]),
                CommandInvocation::new("definitely-not-a-command", [] as [&str; 0]),
            ]),
            |_| {
                ran += 1;
                0
            },
        )
        .expect("daemon answered");
    assert_eq!(ran, 0);
    assert!(matches!(end, ExecChainEnd::Rejected(_)), "{end:?}");
    assert!(
        client
            .execute(CommandInvocation::new("has-session", ["-t", "early"]))
            .is_err()
    );
}

#[test]
fn an_attaching_chain_comes_back_as_a_resume_without_running() {
    let daemon = RunningDaemon::start("exec-resume");
    let mut client = daemon.client();
    let classify: crate::ExecClassifier<'_> = &exec_resume_kind;
    let mut chain = ExecChain::new(vec![
        CommandInvocation::new("new-session", ["-s", "later"]),
        CommandInvocation::new("display-message", ["-p", "x"]),
    ]);
    chain.resume = Some(classify);
    let end = client.exec_chain(chain, |_| 0).expect("daemon answered");
    let ExecChainEnd::Resume(resume) = end else {
        panic!("expected a resume, got {end:?}");
    };
    assert_eq!(resume.kind, zz_protocol::ExecResumeKind::NewSession);
    assert_eq!(resume.commands.len(), 2);
    assert!(
        client
            .execute(CommandInvocation::new("has-session", ["-t", "later"]))
            .is_err()
    );
}

#[test]
fn a_client_file_read_goes_live_and_the_connection_keeps_serving() {
    let daemon = RunningDaemon::start("exec-live");
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("buffer.txt");
    fs::write(&path, "from the client\n").expect("buffer file");
    let mut client = daemon.client();
    run(&mut client, &["new-session", "-d"]);
    let path = path.to_str().expect("UTF-8 path");
    run(&mut client, &["load-buffer", "-b", "live", path]);
    assert_eq!(
        run(&mut client, &["show-buffer", "-b", "live"]),
        "from the client\n"
    );
    assert_eq!(
        run(&mut client, &["display-message", "-p", "again"]),
        "again"
    );
}

#[test]
fn an_idle_connection_thread_serves_the_next_connection_then_retires() {
    let threads = Arc::new(exec::ConnectionThreads::default());
    let (ran, seen) = mpsc::channel();
    let first = ran.clone();
    threads
        .run(Box::new(move || {
            let _ = first.send(thread::current().id());
        }))
        .expect("first job");
    let first_thread = seen
        .recv_timeout(Duration::from_secs(5))
        .expect("first ran");
    let deadline = Instant::now() + Duration::from_secs(5);
    while threads.idle_count() == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(threads.idle_count(), 1);
    let second = ran.clone();
    threads
        .run(Box::new(move || {
            let _ = second.send(thread::current().id());
        }))
        .expect("second job");
    assert_eq!(
        seen.recv_timeout(Duration::from_secs(5))
            .expect("second ran"),
        first_thread
    );
    let (release, blocked) = mpsc::channel::<()>();
    let third = ran.clone();
    threads
        .run(Box::new(move || {
            let _ = third.send(thread::current().id());
            let _ = blocked.recv();
        }))
        .expect("third job");
    let busy = seen
        .recv_timeout(Duration::from_secs(5))
        .expect("third ran");
    threads
        .run(Box::new(move || {
            let _ = ran.send(thread::current().id());
        }))
        .expect("fourth job");
    assert_ne!(
        seen.recv_timeout(Duration::from_secs(5))
            .expect("fourth ran"),
        busy,
        "a busy connection thread never delays the next connection"
    );
    drop(release);
    let deadline = Instant::now() + Duration::from_secs(10);
    while threads.idle_count() != 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(threads.idle_count(), 0);
}

#[test]
fn prompt_history_loads_on_first_use_and_keeps_an_early_entry() {
    let directory = tempfile::tempdir().expect("history directory");
    let history = directory.path().join("history");
    fs::write(&history, "command:from-file\nsearch:needle\n").expect("history file");
    let shared = Arc::new(Shared::new(3));
    shared
        .execute(
            ClientId(9),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(
                "set-option",
                ["-s", "history-file", history.to_str().expect("UTF-8 path")],
            ),
        )
        .expect("history-file");
    shared.initialize(false).expect("startup");
    assert!(shared.inner.lock().command_history.is_empty());
    shared.record_prompt_history(CommandPromptType::Command, "early");
    assert_eq!(
        shared.inner.lock().command_history,
        ["from-file".to_owned(), "early".to_owned()]
    );
    assert_eq!(
        fs::read_to_string(&history).expect("persisted history"),
        "command:from-file\ncommand:early\nsearch:needle\n"
    );
    let shown = shared
        .execute(
            ClientId(9),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("show-prompt-history", ["-T", "search"]),
        )
        .expect("show history")
        .output;
    assert_eq!(shown, "History for search:\n\n1: needle\n\n");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn the_tmux_wrapper_runs_a_pinned_image_that_outlives_a_replaced_install() {
    let directory = tempfile::tempdir().expect("temporary install");
    let installed = directory.path().join("zz");
    fs::write(&installed, b"#!/bin/sh\nprintf 'old %s' \"$1\"\n").expect("write installed zz");
    fs::set_permissions(
        &installed,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("make installed zz executable");
    let shim = TmuxShimGuard::install(installed.clone(), true).expect("install pinned wrapper");
    let clone_directory = shim.directory.clone();
    if cfg!(target_os = "macos") {
        assert_eq!(shim.executable, clone_directory.join("zz"));
        fs::remove_file(&installed).expect("replace the install");
        fs::write(&installed, b"#!/bin/sh\nprintf 'new'\n").expect("write new zz");
        let output = std::process::Command::new(clone_directory.join("tmux"))
            .arg("-V")
            .env(
                crate::TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE,
                &shim.executable,
            )
            .env_remove("TMUX")
            .output()
            .expect("run pinned wrapper");
        assert_eq!(output.stdout, b"old -V");
    } else {
        assert_eq!(
            shim.executable,
            PathBuf::from(format!("/proc/{}/exe", std::process::id()))
        );
    }
    drop(shim);
    assert!(!clone_directory.exists());
}

#[cfg(unix)]
mod raw {
    use std::os::unix::net::UnixStream;

    use super::*;
    use zz_protocol::{
        ClientEnvironmentBlob, ExecExit, ExecFlags, ExecOutcome, ExecRequest,
        read_protocol_message, write_protocol_message,
    };

    fn request(commands: Vec<CommandInvocation>) -> ExecRequest {
        ExecRequest {
            protocol_version: PROTOCOL_VERSION,
            flags: ExecFlags::default(),
            client_instance_id: ClientInstanceId(7),
            origin: None,
            working_directory: None,
            tty: None,
            size: None,
            features: 0,
            startup_reentry: None,
            spawned_server_id: None,
            expect_server_id: None,
            process_id: std::process::id(),
            environment: ClientEnvironmentBlob::from_bytes(b"TERM=xterm\0".to_vec()),
            commands,
            raw_control_line: None,
        }
    }

    fn serve(shared: &Arc<Shared>) -> (UnixStream, thread::JoinHandle<Result<(), DaemonError>>) {
        let (client, server) = UnixStream::pair().expect("pair");
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("read timeout");
        let server_shared = Arc::clone(shared);
        let connection = thread::spawn(move || handle_connection(server, &server_shared));
        (client, connection)
    }

    fn display(text: &str) -> Vec<CommandInvocation> {
        vec![CommandInvocation::new("display-message", ["-p", text])]
    }

    fn read_exit(client: &mut UnixStream) -> (Vec<ProtocolMessage>, ExecExit) {
        let mut frames = Vec::new();
        loop {
            match read_protocol_message(client).expect("reply frame") {
                ProtocolMessage::ExecExit(exit) => return (frames, exit),
                frame => frames.push(frame),
            }
        }
    }

    #[test]
    fn one_command_is_one_response_then_the_exit_and_eof_unregisters() {
        let shared = Arc::new(Shared::new(31));
        let (mut client, connection) = serve(&shared);
        write_protocol_message(&mut client, &ProtocolMessage::Exec(request(display("x"))))
            .expect("exec");
        let (frames, exit) = read_exit(&mut client);
        assert_eq!(exit.server_id, 31);
        assert_eq!(exit.outcome, ExecOutcome::Ran);
        assert!(
            matches!(
                frames.as_slice(),
                [ProtocolMessage::CommandResponse(CommandResponse::Success { request_id: 1, output, .. })]
                    if output == "x"
            ),
            "{frames:?}"
        );
        client.set_nonblocking(true).expect("nonblocking");
        let mut rest = [0_u8; 1];
        assert!(matches!(
            std::io::Read::read(&mut client, &mut rest),
            Err(error) if error.kind() == ErrorKind::WouldBlock
        ));
        drop(client);
        connection
            .join()
            .expect("connection thread")
            .expect("clean exit");
        let inner = shared.inner.lock();
        assert!(inner.clients.is_empty());
        drop(inner);
        assert!(shared.client_writers.lock().is_empty());
        assert!(shared.exec_links.lock().is_empty());
        assert!(shared.command_queue_cancels.lock().is_empty());
    }

    #[test]
    fn a_last_exec_is_answered_then_closed_without_waiting_for_the_client() {
        let shared = Arc::new(Shared::new(38));
        let (mut client, connection) = serve(&shared);
        let mut last = request(display("bye"));
        last.flags.set(ExecFlags::LAST, true);
        write_protocol_message(&mut client, &ProtocolMessage::Exec(last)).expect("exec");
        let (frames, exit) = read_exit(&mut client);
        assert_eq!(exit.outcome, ExecOutcome::Ran);
        assert_eq!(frames.len(), 1, "{frames:?}");
        connection
            .join()
            .expect("connection thread")
            .expect("clean exit");
        let mut rest = [0_u8; 1];
        assert_eq!(std::io::Read::read(&mut client, &mut rest).expect("eof"), 0);
        assert!(shared.inner.lock().clients.is_empty());
        assert!(shared.client_writers.lock().is_empty());
    }

    #[test]
    fn an_exec_before_startup_parks_without_holding_its_thread() {
        let shared = Arc::new(Shared::new(32));
        shared.begin_startup();
        let (mut client, connection) = serve(&shared);
        write_protocol_message(
            &mut client,
            &ProtocolMessage::Exec(request(display("late"))),
        )
        .expect("exec");
        connection
            .join()
            .expect("connection thread")
            .expect("parked");
        assert_eq!(shared.pending_execs.lock().len(), 1);
        shared.finish_startup();
        let (frames, exit) = read_exit(&mut client);
        assert_eq!(exit.outcome, ExecOutcome::Ran);
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn a_startup_reentry_exec_runs_before_startup_completes() {
        let shared = Arc::new(Shared::new(33));
        shared.begin_startup();
        let (mut client, _connection) = serve(&shared);
        let mut exec = request(display("child"));
        exec.startup_reentry = Some(33);
        write_protocol_message(&mut client, &ProtocolMessage::Exec(exec)).expect("exec");
        let (frames, exit) = read_exit(&mut client);
        assert_eq!(exit.outcome, ExecOutcome::Ran);
        assert_eq!(frames.len(), 1);
        assert!(shared.pending_execs.lock().is_empty());
        shared.finish_startup();
    }

    #[test]
    fn stopping_during_startup_closes_parked_execs() {
        let shared = Arc::new(Shared::new(34));
        shared.begin_startup();
        let (mut client, connection) = serve(&shared);
        write_protocol_message(
            &mut client,
            &ProtocolMessage::Exec(request(display("never"))),
        )
        .expect("exec");
        connection
            .join()
            .expect("connection thread")
            .expect("parked");
        shared.request_shutdown();
        assert!(shared.pending_execs.lock().is_empty());
        assert!(read_protocol_message(&mut client).is_err());
    }

    #[test]
    fn a_spawned_server_id_aborts_a_bootstrap_daemon_whose_chain_fails_to_prepare() {
        let shared = Arc::new(Shared::new(35));
        shared.inner.lock().cold_bootstrap = ColdBootstrapLease::AwaitingFirstExternal;
        let (mut client, connection) = serve(&shared);
        let mut exec = request(vec![CommandInvocation::new(
            "no-such-command",
            [] as [&str; 0],
        )]);
        exec.spawned_server_id = Some(35);
        write_protocol_message(&mut client, &ProtocolMessage::Exec(exec)).expect("exec");
        let (frames, exit) = read_exit(&mut client);
        assert!(frames.is_empty());
        assert!(matches!(exit.outcome, ExecOutcome::Rejected(_)));
        drop(client);
        connection
            .join()
            .expect("connection thread")
            .expect("clean exit");
        assert!(shared.stopping.load(Ordering::Acquire));

        let committed = Arc::new(Shared::new(36));
        committed.inner.lock().cold_bootstrap = ColdBootstrapLease::AwaitingFirstExternal;
        let (mut client, connection) = serve(&committed);
        let mut exec = request(display("fine"));
        exec.spawned_server_id = Some(36);
        write_protocol_message(&mut client, &ProtocolMessage::Exec(exec)).expect("exec");
        let (_, exit) = read_exit(&mut client);
        assert_eq!(exit.outcome, ExecOutcome::Ran);
        drop(client);
        connection
            .join()
            .expect("connection thread")
            .expect("clean exit");
        assert_eq!(
            committed.inner.lock().cold_bootstrap,
            ColdBootstrapLease::Committed
        );
    }

    #[test]
    fn a_foreign_envelope_version_gets_the_mismatch_reply() {
        let shared = Arc::new(Shared::new(37));
        let (mut client, connection) = serve(&shared);
        let mut frame =
            zz_protocol::encode_protocol_message(&ProtocolMessage::Exec(request(display("x"))))
                .expect("encode");
        frame[6..8].copy_from_slice(&106_u16.to_le_bytes());
        std::io::Write::write_all(&mut client, &frame).expect("write");
        assert!(matches!(
            read_protocol_message(&mut client).expect("reply"),
            ProtocolMessage::CommandResponse(CommandResponse::Error {
                request_id: 0,
                error: ServerError::ProtocolMismatch { client: 106, .. },
                ..
            })
        ));
        assert!(connection.join().expect("connection thread").is_err());
    }
}
