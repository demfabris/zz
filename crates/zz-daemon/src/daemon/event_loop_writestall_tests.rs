use super::*;
use zz_protocol::encode_protocol_message;

#[test]
fn ordinary_client_drains_more_than_two_write_batches_without_external_events() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (mut peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    event_loop.connections.get_mut(&token).unwrap().kind = Some(ClientKind::Interactive);
    event_loop
        .poll
        .poll(&mut event_loop.events, Some(Duration::ZERO))
        .unwrap();
    assert!(event_loop.events.is_empty());

    let outbound = Arc::clone(&event_loop.connections[&token].outbound);
    let mut expected = Vec::new();
    for request_id in 0..3 {
        let message = ProtocolMessage::CommandResponse(CommandResponse::Success {
            request_id,
            output: RawText::from("x".repeat(attach::MAX_BATCHED_WRITE_BYTES)),
            exit_code: 0,
            stderr: String::new(),
            stdout_claim: StdoutClaim::default(),
        });
        expected.extend(encode_protocol_message(&message).unwrap());
        assert!(outbound.enqueue_reliable(&message));
    }
    assert!(expected.len() > 2 * attach::MAX_BATCHED_WRITE_BYTES);

    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        event_loop.turn(&shared).unwrap();
        let mut buffer = [0; 8192];
        loop {
            match peer.read(&mut buffer) {
                Ok(0) => panic!("client closed before draining output"),
                Ok(read) => received.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => panic!("client read: {error}"),
            }
        }
        if received.len() == expected.len() {
            break;
        }
        assert!(Instant::now() < deadline, "output drain deadline");
        event_loop
            .poll
            .poll(
                &mut event_loop.events,
                Some(deadline.saturating_duration_since(Instant::now())),
            )
            .unwrap();
        assert!(
            !event_loop.events.is_empty(),
            "output stalled after {} of {} bytes",
            received.len(),
            expected.len()
        );
        assert!(event_loop.events.iter().all(|event| {
            event.token() == WAKE || (event.token() == token && event.is_writable())
        }));
    }
    assert_eq!(received, expected);
    assert_eq!(outbound.state.lock().queued_bytes, 0);
    assert_eq!(outbound.state.lock().writer_inflight_bytes, 0);
    assert!(!event_loop.connections[&token].writable);
    event_loop.remove(token, &shared);
}
