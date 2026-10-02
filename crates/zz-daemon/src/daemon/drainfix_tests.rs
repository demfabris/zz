use super::*;
use zz_protocol::encode_protocol_message;

#[test]
fn a_client_arriving_while_shutdown_drains_receives_the_stopping_refusal() {
    let socket = PathBuf::from(format!("/tmp/zz-drainfix-{}.sock", server_id()));
    let listener = LocalTransport::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _socket_guard = SocketGuard::new(socket.clone());
    let shared = Arc::new(Shared::new(241));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let blocker = ShutdownBlocker::acquire(&shared, false).unwrap();
    shared.request_shutdown();
    event_loop.turn(&shared).unwrap();
    assert!(shared.shutdown_pending.load(Ordering::Acquire));
    assert!(!shared.stopping.load(Ordering::Acquire));

    let mut client = UnixStream::connect(&socket).unwrap();
    client
        .write_all(&encode_protocol_message(&io_tests::hello()).unwrap())
        .unwrap();
    client.set_nonblocking(true).unwrap();
    event_loop
        .accept_ready::<LocalTransport>(&listener, &shared)
        .unwrap();
    assert_eq!(event_loop.connections.len(), 1);
    let mut input = Inbound::default();
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !event_loop.connections.is_empty() {
        assert!(Instant::now() < deadline, "late client stayed connected");
        event_loop.turn(&shared).unwrap();
        received.extend(io_tests::messages(&mut client, &mut input));
        thread::sleep(Duration::from_millis(1));
    }
    received.extend(io_tests::messages(&mut client, &mut input));
    assert_eq!(received, [server_stopping_response(0)]);

    drop(blocker);
    while !event_loop.shutdown_completed() {
        assert!(Instant::now() < deadline, "shutdown did not finish");
        event_loop.turn(&shared).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
    assert!(shared.stopping.load(Ordering::Acquire));
}
