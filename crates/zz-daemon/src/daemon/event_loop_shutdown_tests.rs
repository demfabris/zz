use super::*;

fn registered(
    event_loop: &mut EventLoop,
    shared: &Arc<Shared>,
    kind: ClientKind,
) -> (Token, UnixStream, ClientId, Arc<OutboundMailbox>) {
    let (peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    let connection = event_loop.connections.get_mut(&token).unwrap();
    let ProtocolMessage::ClientHello(mut hello) = io_tests::hello() else {
        unreachable!()
    };
    hello.kind = kind;
    let session = connection::Session::register(
        shared,
        ProtocolMessage::ClientHello(hello),
        &connection.outbound,
        &connection.cancel,
    )
    .unwrap()
    .unwrap();
    let client = session.client;
    connection.client = Some(client);
    connection.kind = Some(kind);
    connection.released = Some(Arc::clone(&session.released));
    connection.initialized = true;
    connection.session = Some(Box::new(session));
    (token, peer, client, Arc::clone(&connection.outbound))
}

#[test]
fn writer_completion_releases_registration_before_blocked_session_cleanup() {
    for kind in [
        ClientKind::Control,
        ClientKind::Command,
        ClientKind::Interactive,
    ] {
        let shared = Arc::new(Shared::new(57));
        let mut event_loop = EventLoop::empty(&shared).unwrap();
        let (token, mut peer, client, outbound) = registered(&mut event_loop, &shared, kind);
        let inner = shared.inner.lock();
        event_loop.remove(token, &shared);
        assert!(outbound.wait_writer_finished(Instant::now()));
        assert!(!shared.client_writers.lock().contains_key(&client));
        let mut byte = [0];
        assert_eq!(peer.read(&mut byte).unwrap(), 0);
        drop(inner);
    }
}

#[test]
fn detached_session_keeps_partial_output_registered_until_writer_completion() {
    let shared = Arc::new(Shared::new(58));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer, client, outbound) =
        registered(&mut event_loop, &shared, ClientKind::Command);
    event_loop.connections[&token]
        .stream
        .set_send_buffer_size(4096)
        .unwrap();
    let output = ProtocolMessage::CommandResponse(CommandResponse::Success {
        request_id: 9,
        output: RawText::from("x".repeat(1024 * 1024)),
        exit_code: 0,
        stderr: String::new(),
        stdout_claim: StdoutClaim::default(),
    });
    assert!(outbound.enqueue_reliable(&output));
    event_loop.turn(&shared).unwrap();
    assert!(outbound.state.lock().writer_inflight_bytes > 0);
    event_loop.disconnect(token, &shared);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut state = outbound.state.lock();
    while !state.closed {
        assert!(!outbound.ready.wait_until(&mut state, deadline).timed_out());
    }
    assert!(!state.writer_finished);
    drop(state);
    assert!(shared.client_writers.lock().contains_key(&client));
    assert!(!shared.drain_client_writers_for_shutdown(Duration::ZERO));
    let mut input = Inbound::default();
    let mut received = Vec::new();
    while event_loop.connections.contains_key(&token) {
        assert!(Instant::now() < deadline);
        received.extend(io_tests::messages(&mut peer, &mut input));
        event_loop.turn(&shared).unwrap();
    }
    received.extend(io_tests::messages(&mut peer, &mut input));
    assert!(received.iter().any(|message| message == &output));
    assert!(shared.drain_client_writers_for_shutdown(Duration::ZERO));
    assert!(outbound.wait_writer_finished(Instant::now()));
    assert!(shared.client_writers.lock().is_empty());
}
