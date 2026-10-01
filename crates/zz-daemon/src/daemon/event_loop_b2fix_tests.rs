use super::*;
use zz_protocol::{AttachOperation, Hello, InputMessage, encode_protocol_message};

fn hello(kind: ClientKind) -> ProtocolMessage {
    let ProtocolMessage::ClientHello(mut hello) = super::io_tests::hello() else {
        unreachable!()
    };
    hello.kind = kind;
    ProtocolMessage::Hello(Hello::from_client(hello))
}

fn pair(event_loop: &mut EventLoop) -> (Token, UnixStream) {
    let (peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    (
        event_loop.insert(server.receive_fd().unwrap()).unwrap(),
        peer,
    )
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

fn established(
    event_loop: &mut EventLoop,
    shared: &Arc<Shared>,
    kind: ClientKind,
) -> (Token, UnixStream) {
    let (token, mut peer) = pair(event_loop);
    peer.write_all(&encode_protocol_message(&hello(kind)).unwrap())
        .unwrap();
    event_loop.read_ready(token, shared);
    until(event_loop, shared, |event_loop| {
        event_loop.connections[&token].initialized && !event_loop.connections[&token].busy
    });
    (token, peer)
}

#[test]
fn cancelled_compact_initialization_does_not_run_attach_commands() {
    let shared = Arc::new(Shared::new(17));
    let ProtocolMessage::Hello(mut hello) = hello(ClientKind::Control) else {
        unreachable!()
    };
    hello.attach = Some(AttachOperation::Commands(shared.prepare_command_list(
        vec![CommandInvocation::new(
            "set-option",
            ["-g", "@cancelled", "ran"],
        )],
    )));
    let outbound = OutboundMailbox::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut session =
        connection::Session::register(&shared, ProtocolMessage::Hello(hello), &outbound, &cancel)
            .unwrap()
            .unwrap();
    cancel.store(true, Ordering::Release);
    session.initialize(&shared, &outbound);
    let response = shared.execute_command_request(
        session.client,
        ClientKind::Control,
        &mut ExecutionContext::default(),
        1,
        &CommandInvocation::new("show-options", ["-gqv", "@cancelled"]),
    );
    assert!(matches!(response, CommandResponse::Success { output, .. } if output.is_empty()));
}

#[test]
fn disconnected_initializer_stops_after_its_parked_command() {
    let shared = Arc::new(Shared::new(17));
    let ProtocolMessage::Hello(mut hello) = hello(ClientKind::Control) else {
        unreachable!()
    };
    hello.attach = Some(AttachOperation::Commands(shared.prepare_command_list(
        vec![
            CommandInvocation::new("wait-for", ["b2-init"]),
            CommandInvocation::new("set-option", ["-g", "@cancelled", "ran"]),
        ],
    )));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    peer.write_all(&encode_protocol_message(&ProtocolMessage::Hello(hello)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    until(&mut event_loop, &shared, |_| {
        shared
            .inner
            .lock()
            .wait_channels
            .get("b2-init")
            .is_some_and(|channel| !channel.waiters.is_empty())
    });
    peer.shutdown(std::net::Shutdown::Write).unwrap();
    event_loop.read_ready(token, &shared);
    until(&mut event_loop, &shared, |event_loop| {
        !event_loop.connections.contains_key(&token)
    });
    let response = shared.execute_command_request(
        ClientId(u64::MAX),
        ClientKind::Command,
        &mut ExecutionContext::default(),
        1,
        &CommandInvocation::new("show-options", ["-gqv", "@cancelled"]),
    );
    assert!(matches!(response, CommandResponse::Success { output, .. } if output.is_empty()));
}

#[test]
fn interactive_clean_eof_executes_received_input_and_drains_its_reply() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = established(&mut event_loop, &shared, ClientKind::Interactive);
    let input = ProtocolMessage::Input(InputMessage::Text {
        pane: PaneId(999),
        text: "accepted".to_owned(),
    });
    peer.write_all(&encode_protocol_message(&input).unwrap())
        .unwrap();
    peer.shutdown(std::net::Shutdown::Write).unwrap();
    event_loop.read_ready(token, &shared);
    until(&mut event_loop, &shared, |event_loop| {
        !event_loop.connections.contains_key(&token)
    });
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).unwrap();
    let mut inbound = Inbound { bytes, offset: 0 };
    let mut replied = false;
    while let Some(message) = inbound.next().unwrap() {
        replied |= matches!(
            message,
            ProtocolMessage::CommandResponse(CommandResponse::Error { .. })
        );
    }
    assert!(
        replied,
        "accepted input reaches the session before detachment"
    );
}

#[test]
fn compact_hello_eof_before_registration_releases_the_socket() {
    let shared = Arc::new(Shared::new(17));
    shared.begin_startup();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    peer.write_all(&encode_protocol_message(&hello(ClientKind::Control)).unwrap())
        .unwrap();
    peer.shutdown(std::net::Shutdown::Write).unwrap();
    event_loop.read_ready(token, &shared);
    shared.finish_startup();
    until(&mut event_loop, &shared, |event_loop| {
        !event_loop.connections.contains_key(&token)
    });
    assert!(shared.inner.lock().clients.is_empty());
}

#[test]
fn parked_worker_has_bounded_pending_messages() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = established(&mut event_loop, &shared, ClientKind::Control);
    let command = super::io_tests::command(1, "unused");
    let ProtocolMessage::CommandRequest(mut request) = command else {
        unreachable!()
    };
    request.command = CommandInvocation::new("wait-for", ["b2-queue"]);
    peer.write_all(&encode_protocol_message(&ProtocolMessage::CommandRequest(request)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    until(&mut event_loop, &shared, |_| {
        shared
            .inner
            .lock()
            .wait_channels
            .get("b2-queue")
            .is_some_and(|channel| !channel.waiters.is_empty())
    });
    let frames = encode_protocol_message(&ProtocolMessage::TreeSync)
        .unwrap()
        .repeat(100);
    for _ in 0..50 {
        if !event_loop.connections.contains_key(&token) {
            break;
        }
        peer.write_all(&frames).unwrap();
        event_loop.read_ready(token, &shared);
    }
    assert!(
        event_loop
            .connections
            .get(&token)
            .is_none_or(|connection| connection.read_closed),
        "excess pending admission closes the client"
    );
    until(&mut event_loop, &shared, |event_loop| {
        !event_loop.connections.contains_key(&token)
    });
}

#[test]
fn bulk_images_count_inflight_reliable_frames() {
    for pasted in [false, true] {
        let mailbox = OutboundMailbox::new();
        for _ in 0..255 {
            assert!(mailbox.enqueue_reliable(&ProtocolMessage::TreeSync));
        }
        let mut frames = Vec::new();
        mailbox.try_recv_batch(&mut frames, usize::MAX);
        assert_eq!(mailbox.state.lock().writer_inflight_messages, 255);
        let images = vec![vec![1], vec![2]];
        if pasted {
            assert_eq!(
                mailbox.enqueue_pasted_image(PaneId(1), 1, 1, &images),
                PastedImageEnqueue::Closed
            );
        } else {
            assert_eq!(
                mailbox.enqueue_kitty_image(PaneId(1), 1, 1, &images),
                KittyImageEnqueue::Closed
            );
        }
    }
}

struct BurstListener {
    accepted: std::sync::atomic::AtomicUsize,
    total: usize,
}

impl TransportListener for BurstListener {
    type Stream = UnixStream;
    fn set_nonblocking(&self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn accept(&self) -> io::Result<UnixStream> {
        if self.accepted.load(Ordering::Acquire) >= self.total {
            return Err(ErrorKind::WouldBlock.into());
        }
        self.accepted.fetch_add(1, Ordering::AcqRel);
        let (stream, _) = UnixStream::pair()?;
        Ok(stream)
    }
    fn raw_fd(&self) -> std::os::fd::RawFd {
        unreachable!()
    }
}

struct BurstTransport;
impl Transport for BurstTransport {
    type Endpoint = ();
    type Listener = BurstListener;
    type Stream = UnixStream;
    fn bind((): &()) -> io::Result<BurstListener> {
        unreachable!()
    }
    fn connect((): &()) -> io::Result<UnixStream> {
        unreachable!()
    }
}

#[test]
fn accept_bursts_yield_and_resume_without_a_new_listener_edge() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let listener = BurstListener {
        accepted: std::sync::atomic::AtomicUsize::new(0),
        total: 65,
    };
    event_loop
        .accept_ready::<BurstTransport>(&listener, &shared)
        .unwrap();
    assert_eq!(listener.accepted.load(Ordering::Acquire), 32);
    assert!(event_loop.accept_again);
    event_loop.poll_ready().unwrap();
    assert!(event_loop.events.iter().any(|event| event.token() == WAKE));
    let tokens = event_loop.connections.keys().copied().collect::<Vec<_>>();
    for token in tokens {
        event_loop.read_ready(token, &shared);
    }
    event_loop.turn(&shared).unwrap();
    assert!(event_loop.connections.is_empty());
    event_loop
        .accept_ready::<BurstTransport>(&listener, &shared)
        .unwrap();
    assert_eq!(listener.accepted.load(Ordering::Acquire), 64);
    event_loop
        .accept_ready::<BurstTransport>(&listener, &shared)
        .unwrap();
    assert_eq!(listener.accepted.load(Ordering::Acquire), 65);
    assert!(!event_loop.accept_again);
    for token in event_loop.connections.keys().copied().collect::<Vec<_>>() {
        event_loop.remove(token, &shared);
    }
}

#[test]
fn stopping_prevents_further_accepts() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let listener = BurstListener {
        accepted: std::sync::atomic::AtomicUsize::new(0),
        total: 65,
    };
    shared.request_shutdown();
    event_loop
        .accept_ready::<BurstTransport>(&listener, &shared)
        .unwrap();
    assert_eq!(listener.accepted.load(Ordering::Acquire), 0);
}

#[test]
fn worker_start_failure_posts_completion_and_releases_the_socket() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    peer.write_all(&encode_protocol_message(&hello(ClientKind::Control)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    event_loop.turn(&shared).unwrap();
    assert!(!event_loop.connections.contains_key(&token));
    assert!(event_loop.failure.is_some());
}

#[test]
fn safe_queries_keep_order_without_scheduling_a_worker() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = established(&mut event_loop, &shared, ClientKind::Command);
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    peer.write_all(
        &[
            super::io_tests::command(1, "first"),
            super::io_tests::command(2, "second"),
        ]
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
    let responses = super::io_tests::messages(&mut peer, &mut Inbound::default())
        .into_iter()
        .filter_map(|message| {
            if let ProtocolMessage::CommandResponse(CommandResponse::Success {
                request_id,
                output,
                ..
            }) = message
            {
                Some((request_id, output.to_string()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        responses,
        [(1, "first".to_owned()), (2, "second".to_owned())]
    );
    event_loop.remove(token, &shared);
}

#[test]
fn pending_bytes_are_bounded_before_the_message_count_limit() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, _) = pair(&mut event_loop);
    event_loop.connections.get_mut(&token).unwrap().busy = true;
    assert!(event_loop.dispatch(token, &shared, ProtocolMessage::TreeSync, MAX_PENDING_BYTES));
    assert!(!event_loop.dispatch(token, &shared, ProtocolMessage::TreeSync, 1));
    assert!(!event_loop.connections.contains_key(&token));
}

#[test]
fn control_output_reader_starts_only_when_bytes_arrive() {
    let shared = Arc::new(Shared::new(17));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let fixture = tempfile::NamedTempFile::new().unwrap();
    fs::write(fixture.path(), b"b2 raw output\n".repeat(300_000)).unwrap();
    let producer = format!(
        "sh -c 'read line; cat {}; printf TAP:%s \"$line\"; sleep 60'",
        fixture.path().display()
    );
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "b2-tap", producer.as_str()]),
        )
        .unwrap();
    let pane = context.pane.unwrap();
    let (token, mut peer) = established(&mut event_loop, &shared, ClientKind::Control);
    let ProtocolMessage::CommandRequest(mut request) = super::io_tests::command(1, "unused") else {
        unreachable!()
    };
    request.command = CommandInvocation::new("attach-session", ["-t", "b2-tap"]);
    peer.write_all(&encode_protocol_message(&ProtocolMessage::CommandRequest(request)).unwrap())
        .unwrap();
    event_loop.read_ready(token, &shared);
    until(&mut event_loop, &shared, |event_loop| {
        shared.inner.lock().control_output_taps.contains_key(&pane)
            && !event_loop.connections[&token].busy
    });
    let terminal = {
        let inner = shared.inner.lock();
        assert!(inner.control_output_taps[&pane].thread.is_none());
        assert!(inner.control_output_taps[&pane].receiver.is_some());
        Arc::clone(&inner.terminals[&pane])
    };
    assert!(terminal.send_raw_input(Arc::from(b"b2-bytes\n".as_slice())));
    let mut inbound = Inbound::default();
    let mut received = Vec::new();
    fn output(message: ProtocolMessage, bytes: &mut Vec<u8>) {
        match message {
            ProtocolMessage::Batch(batch) => {
                for message in batch.messages().unwrap() {
                    output(message, bytes);
                }
            }
            ProtocolMessage::Event(Event {
                payload: EventPayload::PaneOutput { bytes: chunk, .. },
                ..
            }) => bytes.extend_from_slice(&chunk),
            _ => {}
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "bulk raw output progress deadline"
        );
        event_loop.turn(&shared).unwrap();
        for message in super::io_tests::messages(&mut peer, &mut inbound) {
            output(message, &mut received);
        }
        if received[received.len().saturating_sub(64)..]
            .windows(b"TAP:b2-bytes".len())
            .any(|bytes| bytes == b"TAP:b2-bytes")
        {
            break;
        }
        event_loop.poll_ready().unwrap();
    }
    assert!(received.len() >= 4_200_000);
    assert!(
        shared.inner.lock().control_output_taps[&pane]
            .thread
            .is_some()
    );
    event_loop.remove(token, &shared);
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("kill-session", ["-t", "b2-tap"]),
        )
        .unwrap();
}

#[test]
fn quiet_worker_replies_wait_for_the_loop_writer() {
    let shared = Arc::new(Shared::new(17));
    let event_loop = EventLoop::empty(&shared).unwrap();
    let mailbox = OutboundMailbox::new();
    let (mut peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    mailbox.state.lock().quiet_socket = Some(server.receive_fd().unwrap());
    *mailbox.loop_waker.lock() = Some((Arc::clone(&event_loop.waker), thread::current().id()));
    let worker_mailbox = Arc::clone(&mailbox);
    thread::spawn(move || {
        assert!(worker_mailbox.collect_control_query());
        assert!(worker_mailbox.enqueue_reliable(&ProtocolMessage::TreeSync));
        worker_mailbox.finish_control_query();
    })
    .join()
    .unwrap();
    assert_eq!(
        peer.read(&mut [0; 4096]).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
    let mut bytes = Vec::new();
    mailbox.drain_reliable_into(&mut bytes);
    assert!(!bytes.is_empty());
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_reliable(&ProtocolMessage::TreeSync));
    mailbox.finish_control_query();
    assert!(peer.read(&mut [0; 4096]).unwrap() > 0);
}
