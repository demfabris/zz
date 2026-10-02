use super::*;
use zz_protocol::{ClientEnvironmentBlob, ExecFlags, ExecRequest, encode_protocol_message};

fn request(commands: Vec<CommandInvocation>) -> ProtocolMessage {
    ProtocolMessage::Exec(ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: ExecFlags::default(),
        client_instance_id: ClientInstanceId(31),
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

fn display(text: &str) -> ProtocolMessage {
    request(vec![CommandInvocation::new(
        "display-message",
        ["-p", text],
    )])
}

fn pair(event_loop: &mut EventLoop) -> (Token, UnixStream) {
    let (peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    (token, peer)
}

fn until(event_loop: &mut EventLoop, shared: &Arc<Shared>, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "Exec loop progress deadline");
        event_loop.turn(shared).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn twenty_idle_exec_connections_have_no_workers_after_retirement() {
    let shared = Arc::new(Shared::new(51));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let mut peers = Vec::new();
    for _ in 0..20 {
        let (token, mut peer) = pair(&mut event_loop);
        peer.write_all(&encode_protocol_message(&display("ready")).unwrap())
            .unwrap();
        event_loop.read_ready(token, &shared);
        peers.push((token, peer, Inbound::default(), false));
    }
    until(&mut event_loop, &shared, || {
        for (_, peer, input, exited) in &mut peers {
            *exited |= io_tests::messages(peer, input)
                .iter()
                .any(|message| matches!(message, ProtocolMessage::ExecExit(_)));
        }
        peers.iter().all(|(_, _, _, exited)| *exited)
    });
    until(&mut event_loop, &shared, || {
        shared.connection_threads.worker_count() == 0
    });
    assert_eq!(shared.connection_threads.idle_count(), 0);
    assert_eq!(event_loop.connections.len(), 20);
    assert_eq!(shared.inner.lock().clients.len(), 20);
    for (token, _, _, _) in peers {
        event_loop.remove(token, &shared);
    }
    until(&mut event_loop, &shared, || {
        shared.inner.lock().clients.is_empty()
    });
}

#[test]
fn probes_do_not_register_or_start_workers() {
    let shared = Arc::new(Shared::new(52));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    peer.write_all(&encode_protocol_message(&request(Vec::new())).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    event_loop.turn(&shared).unwrap();
    assert!(matches!(
        io_tests::messages(&mut peer, &mut Inbound::default()).as_slice(),
        [ProtocolMessage::ExecExit(_)]
    ));
    assert!(shared.inner.lock().clients.is_empty());
    assert_eq!(shared.connection_threads.worker_count(), 0);
    event_loop.remove(token, &shared);
}

#[test]
fn file_reply_bypasses_a_queued_exec_chain() {
    let shared = Arc::new(Shared::new(53));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let load = request(vec![CommandInvocation::new(
        "load-buffer",
        ["-b", "from-client", "/tmp/zzpc-client-buffer"],
    )]);
    peer.write_all(&encode_protocol_message(&load).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    let mut input = Inbound::default();
    let mut file = None;
    until(&mut event_loop, &shared, || {
        for message in io_tests::messages(&mut peer, &mut input) {
            if let ProtocolMessage::ClientFileRequest(request) = message {
                file = Some(request);
            }
        }
        file.is_some()
    });
    let file = file.unwrap();
    let messages = [
        display("after"),
        ProtocolMessage::ClientFileResponse(ClientFileResponse {
            request_id: file.request_id,
            data: b"client data\n".to_vec(),
            error: None,
        }),
    ];
    peer.write_all(
        &messages
            .iter()
            .flat_map(|message| encode_protocol_message(message).unwrap())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    event_loop.read_ready(token, &shared);
    let mut received = Vec::new();
    until(&mut event_loop, &shared, || {
        received.extend(io_tests::messages(&mut peer, &mut input));
        received
            .iter()
            .filter(|message| matches!(message, ProtocolMessage::ExecExit(_)))
            .count()
            == 2
    });
    assert!(
        matches!(received.as_slice(), [ProtocolMessage::CommandResponse(CommandResponse::Success { .. }), ProtocolMessage::ExecExit(_), ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }), ProtocolMessage::ExecExit(_)] if output == "after")
    );
    assert_eq!(
        shared.inner.lock().paste_buffers[0].data.as_ref(),
        b"client data\n"
    );
    event_loop.remove(token, &shared);
    until(&mut event_loop, &shared, || {
        shared.inner.lock().clients.is_empty()
    });
}

#[test]
fn coalesced_resume_and_hello_preserve_the_socket_and_exit_order() {
    resume_and_hello(ClientKind::Interactive);
}

#[test]
fn coalesced_resume_and_control_hello_preserve_the_socket_and_exit_order() {
    resume_and_hello(ClientKind::Control);
}

fn resume_and_hello(kind: ClientKind) {
    let shared = Arc::new(Shared::new(54));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let ProtocolMessage::Exec(mut resume) = request(vec![CommandInvocation::new(
        "attach-session",
        [] as [&str; 0],
    )]) else {
        unreachable!()
    };
    resume.flags.set(ExecFlags::RESUME, true);
    resume.flags.set(ExecFlags::LAST, true);
    let ProtocolMessage::ClientHello(mut client) = io_tests::hello() else {
        unreachable!()
    };
    client.kind = kind;
    let hello = ProtocolMessage::Hello(zz_protocol::Hello::from_client(client));
    let encoded = [ProtocolMessage::Exec(resume), hello, display("after-hello")]
        .iter()
        .flat_map(|message| encode_protocol_message(message).unwrap())
        .collect::<Vec<_>>();
    peer.write_all(&encoded).unwrap();
    event_loop.read_ready(token, &shared);
    let mut input = Inbound::default();
    let mut received = Vec::new();
    until(&mut event_loop, &shared, || {
        for message in io_tests::messages(&mut peer, &mut input) {
            if let ProtocolMessage::Batch(batch) = message {
                received.extend(batch.messages().unwrap());
            } else {
                received.push(message);
            }
        }
        received
            .iter()
            .filter(|message| matches!(message, ProtocolMessage::ExecExit(_)))
            .count()
            == 2
    });
    assert!(matches!(
        received.first(),
        Some(ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            outcome: zz_protocol::ExecOutcome::Resume(_),
            ..
        }))
    ));
    assert!(matches!(received.get(1), Some(ProtocolMessage::Welcome(_))));
    assert_eq!(shared.inner.lock().clients.len(), 1);
    assert_eq!(event_loop.connections.len(), 1);
    assert!(!event_loop.connections[&token].exec_mode);
    event_loop.remove(token, &shared);
    until(&mut event_loop, &shared, || {
        shared.inner.lock().clients.is_empty()
    });
}

#[test]
fn a_parked_chain_flushes_earlier_output_before_it_completes() {
    let shared = Arc::new(Shared::new(55));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let chain = request(vec![
        CommandInvocation::new("display-message", ["-p", "before"]),
        CommandInvocation::new("wait-for", ["b3-output-gate"]),
        CommandInvocation::new("display-message", ["-p", "after"]),
    ]);
    peer.write_all(&encode_protocol_message(&chain).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    let mut input = Inbound::default();
    let mut received = Vec::new();
    until(&mut event_loop, &shared, || {
        received.extend(io_tests::messages(&mut peer, &mut input));
        received.iter().any(|message| matches!(message, ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }) if output == "before"))
    });
    assert!(
        !received
            .iter()
            .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
    );
    shared
        .execute(
            ClientId(900),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("wait-for", ["-S", "b3-output-gate"]),
        )
        .unwrap();
    until(&mut event_loop, &shared, || {
        received.extend(io_tests::messages(&mut peer, &mut input));
        received
            .iter()
            .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
    });
    assert!(received.iter().any(|message| matches!(message, ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }) if output == "after")));
    event_loop.remove(token, &shared);
    until(&mut event_loop, &shared, || {
        shared.inner.lock().clients.is_empty()
    });
}

#[test]
fn last_exec_flushes_blocked_output_and_exit_during_shutdown() {
    let shared = Arc::new(Shared::new(56));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    event_loop.connections[&token]
        .stream
        .set_send_buffer_size(4096)
        .unwrap();
    let output = "z".repeat(1024 * 1024);
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut ExecutionContext::default(),
            &CommandInvocation::new("set-option", ["-g", "@b3-output", output.as_str()]),
        )
        .unwrap();
    let ProtocolMessage::Exec(mut last) = display("#{@b3-output}") else {
        unreachable!()
    };
    last.flags.set(ExecFlags::LAST, true);
    peer.write_all(&encode_protocol_message(&ProtocolMessage::Exec(last)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    let outbound = Arc::clone(&event_loop.connections[&token].outbound);
    until(&mut event_loop, &shared, || {
        outbound.state.lock().writer_inflight_bytes != 0
    });
    event_loop.start_shutdown(&shared);
    let mut input = Inbound::default();
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !event_loop.shutdown_completed() {
        assert!(Instant::now() < deadline);
        received.extend(io_tests::messages(&mut peer, &mut input));
        event_loop.turn(&shared).unwrap();
        thread::sleep(Duration::from_millis(2));
    }
    received.extend(io_tests::messages(&mut peer, &mut input));
    assert!(
        matches!(received.as_slice(), [ProtocolMessage::CommandResponse(CommandResponse::Success { output: actual, .. }), ProtocolMessage::ExecExit(zz_protocol::ExecExit { outcome: zz_protocol::ExecOutcome::Ran, .. })] if actual.as_bytes() == output.as_bytes())
    );
    assert!(outbound.state.lock().writer_finished);
    assert!(shared.client_writers.lock().is_empty());
    assert!(shared.inner.lock().clients.is_empty());
}
