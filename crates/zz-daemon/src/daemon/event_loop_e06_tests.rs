use super::*;
use zz_protocol::{ClientEnvironmentBlob, ExecFlags, ExecRequest, encode_protocol_message};

fn pair(event_loop: &mut EventLoop) -> (Token, UnixStream) {
    let (server, peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    (token, peer)
}

fn request(commands: Vec<CommandInvocation>) -> ProtocolMessage {
    ProtocolMessage::Exec(ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: ExecFlags::default(),
        client_instance_id: ClientInstanceId(606),
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

fn send(
    event_loop: &mut EventLoop,
    shared: &Arc<Shared>,
    commands: Vec<CommandInvocation>,
) -> (Token, UnixStream) {
    let (token, mut peer) = pair(event_loop);
    peer.write_all(&encode_protocol_message(&request(commands)).unwrap())
        .unwrap();
    event_loop.read_ready(token, shared);
    (token, peer)
}

fn signal(shared: &Arc<Shared>, flag: &str, name: &str) {
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("wait-for", [flag, name]),
        )
        .unwrap();
}

fn responses(messages: &[ProtocolMessage]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }) => {
                Some(output.to_string())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn twenty_signal_waiters_add_no_workers_and_complete_once_each() {
    let shared = Arc::new(Shared::new(606));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    let mut peers = (0..20)
        .map(|_| {
            send(
                &mut event_loop,
                &shared,
                vec![CommandInvocation::new("wait-for", ["e06-all"])],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        shared.inner.lock().wait_channels["e06-all"].waiters.len(),
        20
    );
    let continuations = shared.inner.lock().wait_channels["e06-all"]
        .waiters
        .iter()
        .map(|item| item.continuation.clone())
        .collect::<Vec<_>>();
    assert_eq!(shared.connection_threads.worker_count(), 0);
    for (token, _) in &peers {
        assert!(!event_loop.connections[token].busy);
    }
    signal(&shared, "-S", "e06-all");
    event_loop.turn(&shared).unwrap();
    event_loop.turn(&shared).unwrap();
    for (_, peer) in &mut peers {
        let messages = io_tests::messages(peer, &mut Inbound::default());
        assert_eq!(responses(&messages), [String::new()]);
        assert_eq!(
            messages
                .iter()
                .filter(|message| matches!(message, ProtocolMessage::ExecExit(_)))
                .count(),
            1
        );
    }
    assert!(
        continuations
            .iter()
            .all(|continuation| continuation.ready() && !continuation.complete())
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
}

#[test]
fn twenty_lock_waiters_resume_fifo_without_workers() {
    let shared = Arc::new(Shared::new(607));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (_owner, mut owner) = send(
        &mut event_loop,
        &shared,
        vec![CommandInvocation::new("wait-for", ["-L", "e06-lock"])],
    );
    event_loop.turn(&shared).unwrap();
    assert_eq!(
        responses(&io_tests::messages(&mut owner, &mut Inbound::default())),
        [String::new()]
    );
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    let mut peers = (0..20)
        .map(|index| {
            send(
                &mut event_loop,
                &shared,
                vec![
                    CommandInvocation::new("wait-for", ["-L", "e06-lock"]),
                    CommandInvocation::new("display-message", ["-p".to_owned(), index.to_string()]),
                ],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        shared.inner.lock().wait_channels["e06-lock"].lockers.len(),
        20
    );
    for index in 0..20 {
        signal(&shared, "-U", "e06-lock");
        event_loop.turn(&shared).unwrap();
        let messages = io_tests::messages(&mut peers[index].1, &mut Inbound::default());
        assert_eq!(responses(&messages), [String::new(), index.to_string()]);
        assert_eq!(
            messages
                .iter()
                .filter(|message| matches!(message, ProtocolMessage::ExecExit(_)))
                .count(),
            1
        );
        for (_, peer) in peers.iter_mut().skip(index + 1) {
            assert!(responses(&io_tests::messages(peer, &mut Inbound::default())).is_empty());
        }
    }
    signal(&shared, "-U", "e06-lock");
    assert!(!shared.inner.lock().wait_channels["e06-lock"].locked);
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
}

#[test]
fn nested_alias_wait_preserves_frames_without_a_worker() {
    let shared = Arc::new(Shared::new(608));
    for (index, alias) in [
        "third=display-message -p three-before ; wait-for e06-alias ; display-message -p three-after",
        "second=display-message -p two-before ; if-shell -F 1 'third' ; display-message -p two-after",
        "first=display-message -p one-before ; run-shell -C 'second' ; display-message -p one-after",
    ].into_iter().enumerate() {
        shared.execute(ClientId(u64::MAX), ClientKind::Command, &mut ExecutionContext::default(),
            &CommandInvocation::new("set-option", ["-s".to_owned(), format!("command-alias[{}]", 100 + index), alias.to_owned()])).unwrap();
    }
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    let (_, mut peer) = send(
        &mut event_loop,
        &shared,
        vec![CommandInvocation::new("first", [] as [&str; 0])],
    );
    assert_eq!(
        shared.inner.lock().wait_channels["e06-alias"].waiters.len(),
        1
    );
    event_loop.turn(&shared).unwrap();
    let before = io_tests::messages(&mut peer, &mut Inbound::default());
    assert!(responses(&before).is_empty());
    let (_, mut unrelated) = send(
        &mut event_loop,
        &shared,
        vec![CommandInvocation::new(
            "display-message",
            ["-p", "unrelated"],
        )],
    );
    event_loop.turn(&shared).unwrap();
    assert_eq!(
        responses(&io_tests::messages(&mut unrelated, &mut Inbound::default())),
        ["unrelated"]
    );
    signal(&shared, "-S", "e06-alias");
    event_loop.turn(&shared).unwrap();
    let messages = io_tests::messages(&mut peer, &mut Inbound::default());
    assert_eq!(
        responses(&messages)[0].lines().collect::<Vec<_>>(),
        [
            "one-before",
            "two-before",
            "three-before",
            "three-after",
            "two-after",
            "one-after"
        ]
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
}

#[test]
fn cancellation_between_unlock_and_resume_transfers_the_lock() {
    let shared = Arc::new(Shared::new(609));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (_, _owner) = send(
        &mut event_loop,
        &shared,
        vec![CommandInvocation::new("wait-for", ["-L", "race"])],
    );
    let (first, _first_peer) = send(
        &mut event_loop,
        &shared,
        vec![CommandInvocation::new("wait-for", ["-L", "race"])],
    );
    let (_second, mut second_peer) = send(
        &mut event_loop,
        &shared,
        vec![CommandInvocation::new("wait-for", ["-L", "race"])],
    );
    signal(&shared, "-U", "race");
    event_loop.connections.get_mut(&first).unwrap().exec_request = None;
    event_loop
        .connections
        .get_mut(&first)
        .unwrap()
        .command
        .take()
        .unwrap()
        .finish();
    event_loop.turn(&shared).unwrap();
    assert_eq!(
        responses(&io_tests::messages(
            &mut second_peer,
            &mut Inbound::default()
        )),
        [String::new()]
    );
    assert!(shared.inner.lock().wait_channels["race"].locked);
    signal(&shared, "-U", "race");
    assert!(!shared.inner.lock().wait_channels["race"].locked);
}

#[test]
fn twenty_inserted_waiters_complete_once_on_the_loop_without_workers() {
    let shared = Arc::new(Shared::new(610));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let completed = Arc::new(Mutex::new(Vec::new()));
    let mut peers = Vec::new();
    for index in 0..20 {
        let (token, peer) = send(
            &mut event_loop,
            &shared,
            vec![CommandInvocation::new(
                "display-message",
                ["-p", "registered"],
            )],
        );
        let client = event_loop.connections[&token].client.unwrap();
        peers.push(peer);
        let completed = Arc::clone(&completed);
        shared.enqueue_inserted_task(
            client,
            ClientKind::Command,
            &ExecutionContext::default(),
            &InsertedCommandSource::String(format!(
                "wait-for inserted-all ; display-message -p {index}"
            )),
            "<e06-inserted>",
            None,
            false,
            None,
            Box::new(move |_, _, result| {
                assert_eq!(result.unwrap().output, index.to_string());
                completed.lock().push(index);
            }),
        );
    }
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    event_loop.turn(&shared).unwrap();
    assert_eq!(
        shared.inner.lock().wait_channels["inserted-all"]
            .waiters
            .len(),
        20
    );
    assert!(completed.lock().is_empty());
    assert_eq!(shared.connection_threads.worker_count(), 0);
    signal(&shared, "-S", "inserted-all");
    event_loop.turn(&shared).unwrap();
    event_loop.turn(&shared).unwrap();
    assert_eq!(*completed.lock(), (0..20).collect::<Vec<_>>());
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
}

#[test]
fn owned_request_frames_preserve_reported_source_failures() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("reported.conf");
    fs::write(
        &source,
        "run-shell -C '\"{ display-message -p quoted }\"'\ndisplay-message -p after\n",
    )
    .unwrap();
    let shared = Arc::new(Shared::new(611));
    let callback = format!("source-file '{}'", source.display());
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(
                "set-option",
                ["-s", "command-alias[111]", &format!("reported={callback}")],
            ),
        )
        .unwrap();
    let commands = [
        CommandInvocation::new("source-file", [source.to_str().unwrap()]),
        CommandInvocation::new("run-shell", ["-C", &callback]),
        CommandInvocation::new("if-shell", ["-F", "1", &callback]),
        CommandInvocation::new("reported", [] as [&str; 0]),
    ];
    for kind in [ClientKind::Command, ClientKind::Control] {
        let mailbox = OutboundMailbox::new();
        let (client, _) = shared.register_subscribed(kind, None, None, mailbox);
        for command in &commands {
            let expected = shared.execute_command_request(
                client,
                kind,
                &mut ExecutionContext::default(),
                611,
                command,
            );
            let mut task = wait_queue::CommandTask::new(
                &shared,
                client,
                kind,
                &ExecutionContext::default(),
                611,
                command,
                false,
            )
            .unwrap();
            loop {
                match task.run(false) {
                    wait_queue::Progress::Done => break,
                    wait_queue::Progress::Ready | wait_queue::Progress::Worker => {}
                    wait_queue::Progress::Waiting => panic!("unexpected source wait"),
                }
            }
            let (actual, _, _, _) = task.finish();
            assert_eq!(actual, expected, "{} {kind:?}", command.name);
        }
    }
}

#[test]
fn shell_job_leaves_are_dispatched_to_workers() {
    let shared = Arc::new(Shared::new(612));
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    for command in [
        CommandInvocation::new("run-shell", ["true"]),
        CommandInvocation::new("if-shell", ["true", "display-message -p after"]),
    ] {
        let mut task = wait_queue::CommandTask::new(
            &shared,
            client,
            ClientKind::Command,
            &ExecutionContext::default(),
            612,
            &command,
            false,
        )
        .unwrap();
        assert!(matches!(task.run(true), wait_queue::Progress::Worker));
    }
}
