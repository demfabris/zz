use super::*;
use zz_protocol::{ExecFlags, ExecRequest, encode_protocol_message};

fn request(command: CommandInvocation) -> ProtocolMessage {
    ProtocolMessage::Exec(ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: ExecFlags::default(),
        client_instance_id: ClientInstanceId(420),
        origin: None,
        working_directory: None,
        tty: None,
        size: None,
        features: 0,
        startup_reentry: None,
        spawned_server_id: None,
        expect_server_id: None,
        process_id: std::process::id(),
        environment: ClientEnvironmentBlob::default(),
        commands: vec![command],
        raw_control_line: None,
    })
}

fn waiting_event_hook(event: &str, command: CommandInvocation, worker: bool) {
    let directory = tempfile::tempdir().unwrap();
    let started = directory.path().join("started");
    struct Release(PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let release = Release(directory.path().join("release"));
    let hook = format!(
        "run-shell true ; run-shell 'touch {}; while [ ! -f {} ]; do sleep 0.01; done' ; set-option -g @hookpanic finished",
        started.display(),
        release.0.display()
    );
    let shared = Arc::new(Shared::new(420));
    let mut context = ExecutionContext::default();
    for command in [
        CommandInvocation::new(
            "new-session",
            ["-d", "-s", "hookpanic", "exec sleep 1000000"],
        ),
        CommandInvocation::new("set-option", ["-wg", "automatic-rename", "off"]),
        CommandInvocation::new("set-hook", ["-g", event, &hook]),
    ] {
        shared
            .execute(ClientId(1), ClientKind::Command, &mut context, &command)
            .unwrap();
    }
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (mut peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    if worker {
        let ProtocolMessage::Exec(mut request) = request(command) else {
            unreachable!()
        };
        let outbound = Arc::clone(&event_loop.connections[&token].outbound);
        let mut execution = exec::LoopExec::register(
            &shared,
            &mut request,
            &outbound,
            &Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let mut prepared = execution.prepare(request);
        event_loop.connections.get_mut(&token).unwrap().exec_mode = true;
        event_loop.execute_work(token, &shared, move |_| {
            while execution.run(&mut prepared, false).is_none() {
                assert!(prepared.ready_on_loop);
            }
            Ok(Completed::Exec(Some(Box::new(execution))))
        });
    } else {
        peer.write_all(&encode_protocol_message(&request(command)).unwrap())
            .unwrap();
        event_loop.read_ready(token, &shared);
    }
    let mut input = Inbound::default();
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "event hook did not park: {received:?}"
        );
        event_loop.turn(&shared).unwrap();
        received.extend(io_tests::messages(&mut peer, &mut input));
        let parked = started.exists() && !event_loop.timers.hooks.is_empty();
        if parked
            && received
                .iter()
                .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        received.as_slice(),
        [
            ProtocolMessage::CommandResponse(CommandResponse::Success { exit_code: 0, .. }),
            ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                outcome: zz_protocol::ExecOutcome::Ran,
                ..
            })
        ]
    ));
    assert_eq!(
        shared
            .inner
            .lock()
            .engine
            .format_user_option("", "", "", "@hookpanic"),
        None
    );
    drop(release);
    peer.write_all(
        &encode_protocol_message(&request(CommandInvocation::new(
            "display-message",
            ["-p", "responsive"],
        )))
        .unwrap(),
    )
    .unwrap();
    event_loop.read_ready(token, &shared);
    received.clear();
    loop {
        assert!(Instant::now() < deadline, "event hook did not finish");
        event_loop.turn(&shared).unwrap();
        received.extend(io_tests::messages(&mut peer, &mut input));
        let finished = shared
            .inner
            .lock()
            .engine
            .format_user_option("", "", "", "@hookpanic")
            == Some("finished");
        if finished
            && received
                .iter()
                .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        received.as_slice(),
        [
            ProtocolMessage::CommandResponse(CommandResponse::Success { output, exit_code: 0, .. }),
            ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                outcome: zz_protocol::ExecOutcome::Ran,
                ..
            })
        ] if output == "responsive"
    ));
    assert!(!shared.stopping.load(Ordering::Acquire));
    event_loop.remove(token, &shared);
    shared.request_shutdown();
}

#[test]
fn loop_split_with_waiting_layout_hook_returns_response_and_exit() {
    waiting_event_hook(
        "window-layout-changed",
        CommandInvocation::new(
            "split-window",
            ["-d", "-t", "hookpanic:", "exec sleep 1000000"],
        ),
        false,
    );
}

#[test]
fn worker_rename_with_waiting_event_hook_returns_response_and_exit() {
    waiting_event_hook(
        "window-renamed",
        CommandInvocation::new("rename-window", ["-t", "hookpanic:", "renamed"]),
        true,
    );
}
