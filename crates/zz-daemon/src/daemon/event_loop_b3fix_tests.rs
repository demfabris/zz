use super::*;
use zz_protocol::{ExecFlags, ExecRequest, encode_protocol_message};

fn pair(event_loop: &mut EventLoop) -> (Token, UnixStream) {
    let (peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    (token, peer)
}

fn request(commands: Vec<CommandInvocation>, last: bool) -> ProtocolMessage {
    let mut flags = ExecFlags::default();
    flags.set(ExecFlags::LAST, last);
    ProtocolMessage::Exec(ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags,
        client_instance_id: ClientInstanceId(71),
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
        commands,
        raw_control_line: None,
    })
}

fn display(text: &str) -> CommandInvocation {
    CommandInvocation::new("display-message", ["-p", text])
}

#[test]
fn query_chains_and_last_reply_run_without_a_worker() {
    let shared = Arc::new(Shared::new(71));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    let messages = [
        request(vec![display("first"), display("second")], false),
        request(vec![display("last")], true),
    ];
    peer.write_all(
        &messages
            .iter()
            .flat_map(|message| encode_protocol_message(message).unwrap())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    event_loop.read_ready(token, &shared);
    event_loop.turn(&shared).unwrap();
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
    let received = io_tests::messages(&mut peer, &mut Inbound::default());
    assert!(
        matches!(received.as_slice(), [
        ProtocolMessage::CommandResponse(CommandResponse::Success { request_id: 1, output: first, .. }),
        ProtocolMessage::CommandResponse(CommandResponse::Success { request_id: 2, output: second, .. }),
        ProtocolMessage::ExecExit(_),
        ProtocolMessage::CommandResponse(CommandResponse::Success { request_id: 1, output: last, .. }),
        ProtocolMessage::ExecExit(_),
    ] if first == "first" && second == "second" && last == "last"),
        "{received:?}"
    );
    assert!(shared.inner.lock().clients.is_empty());
    assert!(!event_loop.connections.contains_key(&token));
}

#[test]
fn query_output_pressure_parks_remaining_replies_on_a_continuation() {
    let shared = Arc::new(Shared::new(72));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let commands = (0..100).map(|index| display(&index.to_string())).collect();
    peer.write_all(&encode_protocol_message(&request(commands, true)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    let connection = &event_loop.connections[&token];
    assert!(!connection.busy);
    assert!(connection.output_wait);
    assert!(matches!(
        connection.command.as_ref().unwrap().state(),
        cmdq::State::Waiting(_)
    ));
    assert!(connection.exec_request.is_some());
    assert_eq!(shared.connection_threads.worker_count(), 0);
    {
        let mut admissions = shared.response_admissions.lock();
        assert_eq!(admissions.active, 1);
        admissions.frozen = true;
    }
    let mut input = Inbound::default();
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        event_loop.turn(&shared).unwrap();
        received.extend(io_tests::messages(&mut peer, &mut input));
        if received
            .iter()
            .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let replies = received
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::CommandResponse(CommandResponse::Success {
                request_id,
                output,
                ..
            }) => Some((*request_id, output.to_string())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        replies,
        (0..100)
            .map(|index| (index + 1, index.to_string()))
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        received.last(),
        Some(ProtocolMessage::ExecExit(_))
    ));
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[test]
fn plain_input_and_unchanged_selection_need_no_worker() {
    let shared = Arc::new(Shared::new(73));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(900),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "b3fix", "exec sleep 1000000"]),
        )
        .unwrap();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    let commands = vec![
        display("before"),
        CommandInvocation::new("send-keys", ["-t", "b3fix:0.0", "hello"]),
        CommandInvocation::new("select-pane", ["-t", "missing", "-t", "b3fix:0.0"]),
        display("after"),
    ];
    peer.write_all(&encode_protocol_message(&request(commands, true)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    event_loop.turn(&shared).unwrap();
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
    let received = io_tests::messages(&mut peer, &mut Inbound::default());
    assert_eq!(received.len(), 5, "{received:?}");
    assert!(matches!(
        received.last(),
        Some(ProtocolMessage::ExecExit(_))
    ));
    shared.request_shutdown();
}

#[test]
fn command_hooks_keep_plain_input_on_a_worker() {
    let shared = Arc::new(Shared::new(74));
    let mut context = ExecutionContext::default();
    for command in [
        CommandInvocation::new(
            "new-session",
            ["-d", "-s", "b3fix-hook", "exec sleep 1000000"],
        ),
        CommandInvocation::new("set-hook", ["-g", "after-send-keys", "wait-for b3fix-hook"]),
    ] {
        shared
            .execute(ClientId(900), ClientKind::Command, &mut context, &command)
            .unwrap();
    }
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    peer.write_all(
        &encode_protocol_message(&request(
            vec![CommandInvocation::new(
                "send-keys",
                ["-t", "b3fix-hook:0.0", "hello"],
            )],
            true,
        ))
        .unwrap(),
    )
    .unwrap();
    event_loop.read_ready(token, &shared);
    assert!(event_loop.connections[&token].busy);
    assert!(shared.connection_threads.worker_count() > 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while shared
        .inner
        .lock()
        .wait_channels
        .get("b3fix-hook")
        .is_none_or(|channel| channel.waiters.is_empty())
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    shared
        .execute(
            ClientId(900),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("wait-for", ["-S", "b3fix-hook"]),
        )
        .unwrap();
    let mut input = Inbound::default();
    let mut exited = false;
    while !exited {
        assert!(Instant::now() < deadline);
        event_loop.turn(&shared).unwrap();
        exited |= io_tests::messages(&mut peer, &mut input)
            .iter()
            .any(|message| matches!(message, ProtocolMessage::ExecExit(_)));
        thread::sleep(Duration::from_millis(1));
    }
    shared.request_shutdown();
}

#[test]
fn relative_pane_selection_keeps_the_command_specific_target_on_a_worker() {
    let shared = Arc::new(Shared::new(75));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(900),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new(
                "new-session",
                ["-d", "-s", "b3fix-relative", "exec sleep 1000000"],
            ),
        )
        .unwrap();
    let first = context.pane.unwrap();
    let window = context.window.unwrap();
    for command in [
        CommandInvocation::new(
            "split-window",
            ["-d", "-t", "b3fix-relative:0", "exec sleep 1000000"],
        ),
        CommandInvocation::new(
            "new-window",
            ["-d", "-t", "b3fix-relative:", "exec sleep 1000000"],
        ),
    ] {
        shared
            .execute(ClientId(900), ClientKind::Command, &mut context, &command)
            .unwrap();
    }
    let expected = shared.inner.lock().engine.state.next_pane(first).unwrap();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let ProtocolMessage::Exec(mut relative) = request(
        vec![CommandInvocation::new("select-pane", ["-t", ":+"])],
        true,
    ) else {
        unreachable!()
    };
    relative.origin = Some(first);
    peer.write_all(&encode_protocol_message(&ProtocolMessage::Exec(relative)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    assert!(event_loop.connections[&token].busy);
    assert!(shared.connection_threads.worker_count() > 0);
    let mut input = Inbound::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        event_loop.turn(&shared).unwrap();
        if io_tests::messages(&mut peer, &mut input)
            .iter()
            .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        shared.inner.lock().engine.state.windows[&window].active_pane,
        expected
    );
    shared.request_shutdown();
}

#[test]
fn last_query_cleanup_that_can_run_hooks_leaves_the_loop_available() {
    let shared = Arc::new(Shared::new(76));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(900),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new(
                "new-session",
                ["-d", "-s", "b3fix-cleanup", "exec sleep 1000000"],
            ),
        )
        .unwrap();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("set-option", ["-g", "destroy-unattached", "on"]),
        )
        .unwrap();
    let (reached, seen) = crossbeam_channel::bounded(1);
    let (release, released) = crossbeam_channel::bounded(1);
    *shared.destroy_unattached_hook.lock() = Some(ResponseAdmissionHook {
        reached,
        release: released,
    });
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    peer.write_all(&encode_protocol_message(&request(vec![display("last")], true)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    seen.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(event_loop.connections[&token].busy);
    let (other_token, mut other) = pair(&mut event_loop);
    other
        .write_all(&encode_protocol_message(&request(vec![display("other")], false)).unwrap())
        .unwrap();
    event_loop.read_ready(other_token, &shared);
    event_loop.turn(&shared).unwrap();
    let replies = io_tests::messages(&mut other, &mut Inbound::default());
    assert!(
        matches!(replies.as_slice(), [
        ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }),
        ProtocolMessage::ExecExit(_),
    ] if output == "other"),
        "{replies:?}"
    );
    let replies = io_tests::messages(&mut peer, &mut Inbound::default());
    assert!(
        matches!(replies.as_slice(), [
        ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }),
        ProtocolMessage::ExecExit(_),
    ] if output == "last"),
        "{replies:?}"
    );
    assert!(event_loop.connections.contains_key(&token));
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while event_loop.connections.contains_key(&token) {
        assert!(Instant::now() < deadline);
        event_loop.turn(&shared).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
    event_loop.remove(other_token, &shared);
    shared.request_shutdown();
}
