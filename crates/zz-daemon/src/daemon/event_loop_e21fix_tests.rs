use super::*;
use zz_protocol::{ExecFlags, ExecRequest, encode_protocol_message};

#[test]
fn completed_exec_removal_leaves_no_cleanup_wake() {
    let shared = Arc::new(Shared::new(723));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (mut peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    let mut flags = ExecFlags::default();
    flags.set(ExecFlags::LAST, true);
    let request = ProtocolMessage::Exec(ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags,
        client_instance_id: ClientInstanceId(723),
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
        commands: vec![CommandInvocation::new("display-message", ["-p", "done"])],
        raw_control_line: None,
    });
    peer.write_all(&encode_protocol_message(&request).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    event_loop
        .poll
        .poll(&mut event_loop.events, Some(Duration::ZERO))
        .unwrap();
    event_loop.turn(&shared).unwrap();
    let messages = io_tests::messages(&mut peer, &mut Inbound::default());
    assert!(matches!(
        &messages[0],
        ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. })
            if output == "done"
    ));
    assert!(matches!(messages[1], ProtocolMessage::ExecExit(_)));
    assert!(!event_loop.connections.contains_key(&token));
    assert!(shared.inner.lock().clients.is_empty());
    event_loop
        .poll
        .poll(&mut event_loop.events, Some(Duration::ZERO))
        .unwrap();
    assert!(event_loop.events.is_empty());
}
