use std::io::Read as _;
use std::os::unix::net::UnixStream;

use mio::{Events, Poll, Token};
use zz_protocol::{decode_protocol_frame, encode_protocol_message};

use super::tests::{QUIET_PANE_COMMAND, terminal_patch_test_message, terminal_test_message};
use super::*;

const COMMAND_CLIENT: ClientId = ClientId(u64::MAX);

fn direct(mailbox: &OutboundMailbox) -> UnixStream {
    let (peer, server) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    server.set_nonblocking(true).unwrap();
    #[cfg(target_vendor = "apple")]
    rustix::net::sockopt::set_socket_nosigpipe(&server, true).unwrap();
    mailbox.state.lock().direct_socket = Some(server.into());
    peer
}

fn loop_waker(mailbox: &OutboundMailbox) -> Poll {
    let poll = Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), Token(7)).unwrap());
    let owner = thread::spawn(|| thread::current().id()).join().unwrap();
    *mailbox.loop_waker.lock() = Some((waker, owner));
    poll
}

fn woken(poll: &mut Poll) -> bool {
    let mut events = Events::with_capacity(4);
    poll.poll(&mut events, Some(Duration::ZERO)).unwrap();
    !events.is_empty()
}

fn available(peer: &mut UnixStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match peer.read(&mut buffer) {
            Ok(0) => return bytes,
            Ok(read) => bytes.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == ErrorKind::WouldBlock => return bytes,
            Err(error) => panic!("peer read: {error}"),
        }
    }
}

fn drain(mailbox: &OutboundMailbox) -> Vec<OutboundFrame> {
    let mut frames = Vec::new();
    mailbox.try_recv_batch(&mut frames, usize::MAX);
    frames
}

#[test]
fn idle_mailbox_writes_terminal_frames_to_the_socket_without_waking_the_loop() {
    let mailbox = OutboundMailbox::new();
    let mut poll = loop_waker(&mailbox);
    let mut peer = direct(&mailbox);
    let pane = PaneId(3);
    let full = terminal_test_message(pane, 1, 1);
    let patch = terminal_patch_test_message(pane, 2, 1, 2);

    assert_eq!(
        mailbox.enqueue_terminal(pane, &full),
        TerminalEnqueue::Queued
    );
    let first = available(&mut peer);
    assert_eq!(decode_protocol_frame(&first).unwrap(), full);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &patch),
        TerminalEnqueue::Queued
    );
    let second = available(&mut peer);
    assert_eq!(second, encode_protocol_message(&patch).unwrap());

    let state = mailbox.state.lock();
    assert_eq!(state.queued_bytes, 0);
    assert!(state.reliable.is_empty() && state.terminals.is_empty());
    assert_eq!(state.written_bytes, (first.len() + second.len()) as u64);
    assert_eq!(state.delivered_terminals[&pane].content, 2);
    drop(state);
    assert!(!woken(&mut poll));
    assert!(drain(&mailbox).is_empty());
}

#[test]
fn short_direct_write_leaves_the_rest_to_the_loop_ahead_of_newer_frames() {
    let mailbox = OutboundMailbox::new();
    let mut poll = loop_waker(&mailbox);
    let mut peer = direct(&mailbox);
    let pane = PaneId(4);
    let full = terminal_test_message(pane, 1, 1);
    let transition = terminal_transition(pane, &full).unwrap();
    let large = (0..8 * 1024 * 1024)
        .map(|byte: usize| byte as u8)
        .collect::<Arc<[u8]>>();

    let encoded = Arc::clone(&large);
    assert_eq!(
        mailbox.enqueue_terminal_with(pane, transition, TerminalDelivery::Foreground, || Ok(
            encoded
        )),
        TerminalEnqueue::Queued
    );
    let sent = {
        let state = mailbox.state.lock();
        let Some(OutboundFrame::Partial { offset, .. }) = state.reliable.front() else {
            panic!("short write left no partial frame");
        };
        assert!(*offset > 0 && *offset < large.len());
        assert_eq!(state.reliable.len(), 1);
        assert!(state.terminals.is_empty());
        assert_eq!(state.queued_bytes, large.len() - offset);
        assert_eq!(state.written_bytes, *offset as u64);
        assert_eq!(state.delivered_terminals[&pane].content, 1);
        *offset
    };
    assert!(woken(&mut poll));

    let patch = terminal_patch_test_message(pane, 2, 1, 2);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &patch),
        TerminalEnqueue::Queued
    );
    assert!(mailbox.state.lock().terminals.contains_key(&pane));

    let received = available(&mut peer);
    assert_eq!(received.len(), sent);
    let frames = drain(&mailbox);
    assert_eq!(frames.len(), 2);
    let mut stream = received;
    stream.extend_from_slice(&frames[0]);
    assert_eq!(stream[..], large[..]);
    assert_eq!(&frames[1][..], encode_protocol_message(&patch).unwrap());
}

#[test]
fn queued_reliable_messages_and_an_inflight_writer_keep_frames_on_the_loop() {
    let mailbox = OutboundMailbox::new();
    let mut poll = loop_waker(&mailbox);
    let mut peer = direct(&mailbox);
    let pane = PaneId(5);
    let full = terminal_test_message(pane, 1, 1);

    assert!(mailbox.enqueue_reliable(&ProtocolMessage::TreeSync));
    assert!(woken(&mut poll));
    assert_eq!(
        mailbox.enqueue_terminal(pane, &full),
        TerminalEnqueue::Queued
    );
    assert!(available(&mut peer).is_empty());
    let frames = drain(&mailbox);
    assert_eq!(frames.len(), 2);
    assert_eq!(
        decode_protocol_frame(&frames[0]).unwrap(),
        ProtocolMessage::TreeSync
    );
    assert_eq!(decode_protocol_frame(&frames[1]).unwrap(), full);

    let patch = terminal_patch_test_message(pane, 2, 1, 2);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &patch),
        TerminalEnqueue::Queued
    );
    assert!(available(&mut peer).is_empty());
    assert!(mailbox.state.lock().terminals.contains_key(&pane));

    let mut written = frames;
    mailbox.record_write(written.iter().map(OutboundFrame::len).sum());
    mailbox.recycle_written_batch(&mut written);
    let frames = drain(&mailbox);
    assert_eq!(&frames[0][..], encode_protocol_message(&patch).unwrap());
    let mut written = frames;
    mailbox.record_write(written[0].len());
    mailbox.recycle_written_batch(&mut written);

    let next = terminal_patch_test_message(pane, 3, 2, 3);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &next),
        TerminalEnqueue::Queued
    );
    assert_eq!(
        available(&mut peer),
        encode_protocol_message(&next).unwrap()
    );
}

#[test]
fn attach_batches_settling_and_control_collection_disable_direct_writes() {
    let setups: [fn(&OutboundMailbox); 4] = [
        OutboundMailbox::hold_terminals,
        |mailbox| mailbox.state.lock().attach_batch = true,
        |mailbox| {
            mailbox.begin_attach_settles(1);
        },
        OutboundMailbox::collect_control_attach,
    ];
    for setup in setups {
        let mailbox = OutboundMailbox::new();
        let mut peer = direct(&mailbox);
        let pane = PaneId(6);
        setup(&mailbox);
        assert_eq!(
            mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 1, 1)),
            TerminalEnqueue::Queued
        );
        assert!(available(&mut peer).is_empty());
        let state = mailbox.state.lock();
        assert!(state.terminals.contains_key(&pane));
        assert!(!state.delivered_terminals.contains_key(&pane));
    }
}

#[test]
fn closing_client_falls_back_to_the_loop_and_closed_mailboxes_release_the_socket() {
    let mailbox = OutboundMailbox::new();
    drop(direct(&mailbox));
    let pane = PaneId(7);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 1, 1)),
        TerminalEnqueue::Queued
    );
    let state = mailbox.state.lock();
    assert!(state.direct_socket.is_none());
    assert!(state.terminals.contains_key(&pane));
    assert_eq!(state.written_bytes, 0);
    drop(state);

    let closes: [fn(&OutboundMailbox); 3] = [
        OutboundMailbox::close_after_flush,
        OutboundMailbox::close,
        OutboundMailbox::mark_writer_finished,
    ];
    for close in closes {
        let mailbox = OutboundMailbox::new();
        let mut peer = direct(&mailbox);
        close(&mailbox);
        assert!(mailbox.state.lock().direct_socket.is_none());
        assert_eq!(peer.read(&mut [0; 16]).unwrap(), 0);
    }
}

fn hello(kind: ClientKind) -> ProtocolMessage {
    ProtocolMessage::ClientHello(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(1),
        kind,
        device_name: None,
        capabilities: Vec::new(),
        color_scheme: None,
        origin: None,
        environment: Vec::new(),
        working_directory: None,
        process_id: 0,
    })
}

fn sole_writer(shared: &Shared) -> Arc<OutboundMailbox> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(writer) = shared.client_writers.lock().values().next() {
            return Arc::clone(writer);
        }
        assert!(Instant::now() < deadline, "client writer never registered");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn interactive_connections_get_a_direct_socket_that_closes_with_the_connection() {
    let shared = Arc::new(Shared::new(1));
    shared.initialize(false).expect("initialize daemon state");
    shared
        .execute(
            COMMAND_CLIENT,
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("new-session", ["-d", "-s", "dw", QUIET_PANE_COMMAND]),
        )
        .expect("new-session");

    let (mut control, server) = UnixStream::pair().unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let control_shared = Arc::clone(&shared);
    let control_connection = thread::spawn(move || handle_connection(server, &control_shared));
    zz_protocol::write_protocol_message(&mut control, &hello(ClientKind::Control)).unwrap();
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut control).unwrap(),
        ProtocolMessage::ServerHello(_)
    ));
    assert!(sole_writer(&shared).state.lock().direct_socket.is_none());
    drop(control);
    control_connection.join().unwrap().unwrap();

    let (mut client, server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let connection_shared = Arc::clone(&shared);
    let connection = thread::spawn(move || handle_connection(server, &connection_shared));
    zz_protocol::write_protocol_message(&mut client, &hello(ClientKind::Interactive)).unwrap();
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut client).unwrap(),
        ProtocolMessage::ServerHello(_)
    ));
    let mailbox = sole_writer(&shared);
    assert!(mailbox.state.lock().direct_socket.is_some());
    zz_protocol::write_protocol_message(
        &mut client,
        &ProtocolMessage::Attach {
            session: "dw".to_owned(),
        },
    )
    .unwrap();
    while !matches!(
        zz_protocol::read_protocol_message(&mut client).unwrap(),
        ProtocolMessage::Attached { .. }
    ) {}

    assert!(mailbox.state.lock().direct_socket.is_some());

    drop(client);
    connection.join().unwrap().unwrap();
    assert!(mailbox.state.lock().direct_socket.is_none());
}
