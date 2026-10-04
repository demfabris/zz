use super::*;

#[test]
fn startup_completion_queued_before_poll_runs_on_the_loop_thread() {
    let socket = PathBuf::from(format!(
        "/tmp/zz-b1-queued-{}-{}.sock",
        std::process::id(),
        server_id()
    ));
    let listener = LocalTransport::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _socket_guard = SocketGuard::new(socket);
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = event_loop::EventLoop::new::<LocalTransport>(&listener, &shared).unwrap();
    let loop_thread = thread::current().id();
    let notifier = event_loop.startup_notifier();
    thread::spawn(move || drop(notifier)).join().unwrap();
    let mut completed = false;
    event_loop
        .run::<LocalTransport>(&listener, &shared, || {
            assert_eq!(thread::current().id(), loop_thread);
            completed = true;
            shared.request_shutdown();
        })
        .unwrap();
    assert!(completed);
}

#[test]
fn stopping_wakes_a_loop_with_no_socket_traffic() {
    let socket = PathBuf::from(format!(
        "/tmp/zz-b1-stop-{}-{}.sock",
        std::process::id(),
        server_id()
    ));
    let listener = LocalTransport::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _socket_guard = SocketGuard::new(socket);
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = event_loop::EventLoop::new::<LocalTransport>(&listener, &shared).unwrap();
    let stopping_shared = Arc::clone(&shared);
    let stopper = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        stopping_shared.request_shutdown();
    });
    let started = Instant::now();
    event_loop
        .run::<LocalTransport>(&listener, &shared, || panic!("no startup completion"))
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    stopper.join().unwrap();
}

#[test]
fn foreground_loop_accepts_startup_reentry_and_keeps_ordinary_execs_parked() {
    use std::os::unix::net::UnixStream;
    use zz_protocol::{
        ExecFlags, ExecOutcome, ExecRequest, read_protocol_message, write_protocol_message,
    };

    fn send(stream: &mut UnixStream, command: CommandInvocation, startup_reentry: Option<u64>) {
        write_protocol_message(
            stream,
            &ProtocolMessage::Exec(ExecRequest {
                protocol_version: PROTOCOL_VERSION,
                flags: ExecFlags::default(),
                client_instance_id: ClientInstanceId(7),
                origin: None,
                working_directory: None,
                tty: None,
                size: None,
                features: 0,
                startup_reentry,
                spawned_server_id: None,
                expect_server_id: None,
                process_id: std::process::id(),
                environment: ClientEnvironmentBlob::default(),
                commands: vec![command],
                raw_control_line: None,
            }),
        )
        .unwrap();
    }

    fn response(stream: &mut UnixStream) -> RawText {
        let ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }) =
            read_protocol_message(stream).unwrap()
        else {
            panic!("command response")
        };
        assert!(
            matches!(read_protocol_message(stream).unwrap(), ProtocolMessage::ExecExit(exit) if exit.outcome == ExecOutcome::Ran)
        );
        output
    }

    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let marker = directory.path().join("started");
    let release = directory.path().join("release");
    let config = directory.path().join("mux.conf");
    fs::write(&config, format!(
        "run-shell 'touch {}; i=0; while [ ! -f {} ] && [ $i -lt 500 ]; do i=$((i+1)); sleep 0.01; done'\nset-option -g @initialized yes\n",
        marker.display(), release.display(),
    )).unwrap();
    let socket = PathBuf::from(format!(
        "/tmp/zz-b1-startup-{}-{}.sock",
        std::process::id(),
        server_id()
    ));
    let daemon = Daemon::new(&socket)
        .with_server_id(777)
        .with_mux_config_files([config]);
    let (ready, started) = mpsc::channel();
    let (done, ended) = mpsc::channel();
    let daemon_thread = thread::spawn(move || {
        let loop_thread = thread::current().id();
        let result = daemon.run_foreground_with_ready(|_| {
            assert_eq!(thread::current().id(), loop_thread);
            ready.send(()).unwrap();
        });
        let _ = done.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "startup shell started");
    let mut ordinary = UnixStream::connect(&socket).unwrap();
    ordinary
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    send(
        &mut ordinary,
        CommandInvocation::new("show-options", ["-gv", "@initialized"]),
        None,
    );
    let mut byte = [0; 1];
    assert!(
        matches!(ordinary.read(&mut byte), Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
    );
    let mut reentry = UnixStream::connect(&socket).unwrap();
    reentry
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    send(
        &mut reentry,
        CommandInvocation::new("display-message", ["-p", "child"]),
        Some(777),
    );
    assert_eq!(response(&mut reentry), "child");
    assert!(started.try_recv().is_err());
    fs::write(&release, "").unwrap();
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    ordinary
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(response(&mut ordinary), "yes");
    tests::connect_command_retry(&socket)
        .execute(CommandInvocation::new("kill-server", [] as [&str; 0]))
        .unwrap();
    ended
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    daemon_thread.join().unwrap();
    assert!(!socket.exists());
}

#[test]
fn a_panicking_startup_worker_still_posts_completion() {
    let socket = PathBuf::from(format!(
        "/tmp/zz-b1-panic-{}-{}.sock",
        std::process::id(),
        server_id()
    ));
    let listener = LocalTransport::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _socket_guard = SocketGuard::new(socket);
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = event_loop::EventLoop::new::<LocalTransport>(&listener, &shared).unwrap();
    let notifier = event_loop.startup_notifier();
    let startup = thread::spawn(move || {
        let _notifier = notifier;
        panic!("startup failed");
    });
    let mut result = Ok(());
    event_loop
        .run::<LocalTransport>(&listener, &shared, || {
            result = event_loop::join_startup(startup);
            shared.request_shutdown();
        })
        .unwrap();
    assert!(matches!(result, Err(DaemonError::Thread(_))));
}

#[test]
fn racing_starts_leave_one_daemon_and_no_start_lock() {
    let socket = PathBuf::from(format!(
        "/tmp/zz-b1-lock-{}-{}.sock",
        std::process::id(),
        server_id()
    ));
    let mut lock = socket.as_os_str().to_owned();
    lock.push(".lock");
    let lock = PathBuf::from(lock);
    fs::write(&lock, "").unwrap();
    let starters = 4;
    let barrier = Arc::new(std::sync::Barrier::new(starters));
    let (ready, started) = mpsc::channel();
    let (finished, results) = mpsc::channel();
    let threads = (0..starters)
        .map(|_| {
            let daemon = Daemon::new(&socket).without_user_config();
            let barrier = Arc::clone(&barrier);
            let ready = ready.clone();
            let finished = finished.clone();
            thread::spawn(move || {
                barrier.wait();
                let result = daemon.run_foreground_with_ready(|_| ready.send(()).unwrap());
                finished.send(result).unwrap();
            })
        })
        .collect::<Vec<_>>();
    started.recv_timeout(Duration::from_secs(20)).unwrap();
    for _ in 1..starters {
        let result = results.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(
            matches!(&result, Err(DaemonError::AlreadyRunning(path)) if *path == socket),
            "{result:?}"
        );
    }
    assert!(started.try_recv().is_err());
    assert!(!lock.exists());
    tests::connect_command_retry(&socket)
        .execute(CommandInvocation::new("kill-server", [] as [&str; 0]))
        .unwrap();
    results
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    for thread in threads {
        thread.join().unwrap();
    }
    assert!(!socket.exists());
    assert!(!lock.exists());
}
