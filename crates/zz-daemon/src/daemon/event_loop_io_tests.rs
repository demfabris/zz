use super::*;
use zz_protocol::encode_protocol_message;

pub(super) fn hello() -> ProtocolMessage {
    ProtocolMessage::ClientHello(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(21),
        kind: ClientKind::Command,
        device_name: None,
        capabilities: Vec::new(),
        color_scheme: None,
        origin: None,
        working_directory: None,
        environment: Vec::new(),
        process_id: std::process::id(),
    })
}

pub(super) fn command(request_id: u64, output: &str) -> ProtocolMessage {
    ProtocolMessage::CommandRequest(CommandRequest {
        request_id,
        command: CommandInvocation::new("display-message", ["-p", output]),
        prepared: false,
    })
}

fn pair(event_loop: &mut EventLoop) -> (Token, UnixStream) {
    let (peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    (token, peer)
}

pub(super) fn messages(peer: &mut UnixStream, input: &mut Inbound) -> Vec<ProtocolMessage> {
    let mut scratch = [0; 8192];
    loop {
        match peer.read(&mut scratch) {
            Ok(0) => break,
            Ok(read) => input.bytes.extend_from_slice(&scratch[..read]),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => panic!("peer read: {error}"),
        }
    }
    let mut messages = Vec::new();
    while let Some(message) = input.next().unwrap() {
        messages.push(message);
    }
    input.compact();
    messages
}

fn until(
    event_loop: &mut EventLoop,
    shared: &Arc<Shared>,
    mut done: impl FnMut(&EventLoop) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(event_loop) {
        assert!(Instant::now() < deadline, "loop progress deadline");
        event_loop.turn(shared).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn fragmentation_keeps_partial_prefix_and_body_without_starting_a_client() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let encoded = encode_protocol_message(&hello()).unwrap();
    for (index, byte) in encoded.iter().enumerate() {
        peer.write_all(&[*byte]).unwrap();
        event_loop.read_ready(token, &shared);
        if index + 1 < encoded.len() {
            assert!(!event_loop.connections[&token].busy);
            assert!(shared.inner.lock().clients.is_empty());
        }
    }
    until(&mut event_loop, &shared, |event_loop| {
        event_loop.connections[&token].initialized && !event_loop.connections[&token].busy
    });
    let mut input = Inbound::default();
    assert!(matches!(
        messages(&mut peer, &mut input).as_slice(),
        [ProtocolMessage::ServerHello(_)]
    ));
    assert!(event_loop.connections[&token].inbound.bytes.is_empty());
    event_loop.remove(token, &shared);
}

#[test]
fn coalescing_keeps_all_frames_and_serial_command_order() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let encoded = [hello(), command(1, "first"), command(2, "second")]
        .iter()
        .flat_map(|message| encode_protocol_message(message).unwrap())
        .collect::<Vec<_>>();
    peer.write_all(&encoded).unwrap();
    event_loop.read_ready(token, &shared);
    let mut input = Inbound::default();
    let mut received = Vec::new();
    until(&mut event_loop, &shared, |_| {
        received.extend(messages(&mut peer, &mut input));
        received
            .iter()
            .filter(|message| matches!(message, ProtocolMessage::CommandResponse(_)))
            .count()
            == 2
    });
    let responses = received
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
        responses,
        [(1, "first".to_owned()), (2, "second".to_owned())]
    );
    assert!(event_loop.connections[&token].inbound.bytes.is_empty());
    event_loop.remove(token, &shared);
}

#[test]
fn blocked_output_retains_offsets_while_another_client_responds() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (blocked, mut slow) = pair(&mut event_loop);
    event_loop.connections[&blocked]
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
    let expected = encode_protocol_message(&output).unwrap();
    let outbound = Arc::clone(&event_loop.connections[&blocked].outbound);
    assert!(outbound.enqueue_reliable(&output));
    event_loop.turn(&shared).unwrap();
    let connection = &event_loop.connections[&blocked];
    assert!(!connection.frames.is_empty());
    assert!(connection.write_offset > 0);
    assert!(connection.writable);
    assert!(outbound.state.lock().writer_inflight_bytes > 0);
    let (responsive, mut fast) = pair(&mut event_loop);
    fast.write_all(
        &[
            encode_protocol_message(&hello()).unwrap(),
            encode_protocol_message(&command(1, "responsive")).unwrap(),
        ]
        .concat(),
    )
    .unwrap();
    event_loop.read_ready(responsive, &shared);
    let mut input = Inbound::default();
    let mut received = Vec::new();
    until(&mut event_loop, &shared, |_| {
        received.extend(messages(&mut fast, &mut input));
        received.iter().any(|message| matches!(message, ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }) if output == "responsive"))
    });
    let mut written = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while written.len() < expected.len() {
        assert!(Instant::now() < deadline);
        let mut bytes = [0; 8192];
        match slow.read(&mut bytes) {
            Ok(read) => written.extend_from_slice(&bytes[..read]),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            other => panic!("slow reader: {other:?}"),
        }
        event_loop.turn(&shared).unwrap();
    }
    assert_eq!(written, expected);
    assert_eq!(outbound.state.lock().writer_inflight_bytes, 0);
    assert!(!event_loop.connections[&blocked].writable);
    event_loop.remove(blocked, &shared);
    event_loop.connections[&responsive]
        .stream
        .set_send_buffer_size(4096)
        .unwrap();
    let final_output = ProtocolMessage::CommandResponse(CommandResponse::Success {
        request_id: 10,
        output: RawText::from("y".repeat(64 * 1024)),
        exit_code: 0,
        stderr: String::new(),
        stdout_claim: StdoutClaim::default(),
    });
    let final_mailbox = Arc::clone(&event_loop.connections[&responsive].outbound);
    assert!(final_mailbox.enqueue_reliable(&final_output));
    event_loop.turn(&shared).unwrap();
    assert!(!event_loop.connections[&responsive].frames.is_empty());
    shared.request_shutdown();
    event_loop.start_shutdown(&shared);
    let mut final_messages = Vec::new();
    until(&mut event_loop, &shared, |event_loop| {
        final_messages.extend(messages(&mut fast, &mut input));
        event_loop.shutdown_completed()
    });
    final_messages.extend(messages(&mut fast, &mut input));
    assert!(
        final_messages
            .iter()
            .any(|message| message == &final_output)
    );
    assert!(final_mailbox.state.lock().writer_finished);
    assert!(!event_loop.connections.contains_key(&responsive));
}

#[test]
fn truncated_eof_rejects_partial_frames_and_oversized_prefixes() {
    for truncated in [
        vec![1],
        vec![10, 0, 0, 0, 1, 2, 3],
        ((zz_protocol::MAX_FRAME_BYTES + 1) as u32)
            .to_le_bytes()
            .to_vec(),
    ] {
        let shared = Arc::new(Shared::new(17));
        let mut event_loop = EventLoop::empty(&shared).unwrap();
        let (token, mut peer) = pair(&mut event_loop);
        peer.write_all(&truncated).unwrap();
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        event_loop.read_ready(token, &shared);
        event_loop.turn(&shared).unwrap();
        assert!(event_loop.connections.is_empty());
        assert!(shared.inner.lock().clients.is_empty());
    }
    let mut input = Inbound {
        bytes: vec![8, 0, 0, 0, 1],
        offset: 0,
    };
    assert!(input.next().unwrap().is_none());
    assert!(matches!(input.eof(), Err(ProtocolError::Truncated)));
}
