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
fn twenty_file_operations_park_without_workers_and_reject_wrong_owners() {
    let shared = Arc::new(Shared::new(111));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let mut peers = Vec::new();
    for index in 0..20 {
        let (token, mut peer) = pair(&mut event_loop);
        let command = request(vec![
            CommandInvocation::new(
                "load-buffer",
                ["-b", &format!("file-{index}"), "/tmp/zzpc-e11-buffer"],
            ),
            CommandInvocation::new("display-message", ["-p", "completed"]),
        ]);
        peer.write_all(&encode_protocol_message(&command).unwrap())
            .unwrap();
        event_loop.read_ready(token, &shared);
        peers.push((token, peer, Inbound::default(), None, 0));
    }
    until(&mut event_loop, &shared, || {
        for (_, peer, input, file, _) in &mut peers {
            for message in io_tests::messages(peer, input) {
                if let ProtocolMessage::ClientFileRequest(request) = message {
                    *file = Some(request);
                }
            }
        }
        peers.iter().all(|(_, _, _, file, _)| file.is_some())
    });
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert_eq!(shared.inner.lock().client_file_waiters.len(), 20);
    let wrong = ProtocolMessage::ClientFileResponse(ClientFileResponse {
        request_id: peers[0].3.as_ref().unwrap().request_id,
        data: b"wrong".to_vec(),
        error: None,
    });
    peers[1]
        .1
        .write_all(&encode_protocol_message(&wrong).unwrap())
        .unwrap();
    event_loop.read_ready(peers[1].0, &shared);
    assert_eq!(shared.inner.lock().client_file_waiters.len(), 20);
    assert!(shared.inner.lock().paste_buffers.is_empty());
    for (token, peer, _, file, _) in &mut peers {
        let reply = ProtocolMessage::ClientFileResponse(ClientFileResponse {
            request_id: file.as_ref().unwrap().request_id,
            data: b"a\0b\xff".to_vec(),
            error: None,
        });
        let encoded = encode_protocol_message(&reply).unwrap();
        peer.write_all(&encoded).unwrap();
        peer.write_all(&encoded).unwrap();
        event_loop.read_ready(*token, &shared);
    }
    until(&mut event_loop, &shared, || {
        for (_, peer, input, _, exits) in &mut peers {
            *exits += io_tests::messages(peer, input)
                .iter()
                .filter(|message| matches!(message, ProtocolMessage::ExecExit(_)))
                .count();
        }
        peers.iter().all(|(_, _, _, _, exits)| *exits == 1)
    });
    assert_eq!(shared.connection_threads.worker_count(), 0);
    let inner = shared.inner.lock();
    assert!(inner.client_file_waiters.is_empty());
    assert_eq!(inner.paste_buffers.len(), 20);
    assert!(
        inner
            .paste_buffers
            .iter()
            .all(|buffer| buffer.data.as_ref() == b"a\0b\xff")
    );
    drop(inner);
    for (token, _, _, _, _) in peers {
        event_loop.remove(token, &shared);
    }
}

#[test]
fn source_replay_parks_at_a_client_file_and_resumes_its_own_frame() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("root.conf");
    fs::write(
        &path,
        "set -g @before yes\nload-buffer -b nested -\nset -g @after yes\n",
    )
    .unwrap();
    let shared = Arc::new(Shared::new(112));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let ProtocolMessage::Exec(mut command) = request(vec![
        CommandInvocation::new("source-file", [path.to_str().unwrap()]),
        CommandInvocation::new("display-message", ["-p", "#{@after}"]),
    ]) else {
        unreachable!()
    };
    command.flags.set(ExecFlags::STDIN_AVAILABLE, true);
    peer.write_all(&encode_protocol_message(&ProtocolMessage::Exec(command)).unwrap())
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
    assert_eq!(shared.connection_threads.worker_count(), 0);
    let before = shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("display-message", ["-p", "#{@before}:#{@after}"]),
        )
        .unwrap();
    assert_eq!(before.output, "yes:");
    peer.write_all(
        &encode_protocol_message(&ProtocolMessage::ClientFileResponse(ClientFileResponse {
            request_id: file.unwrap().request_id,
            data: b"nested bytes".to_vec(),
            error: None,
        }))
        .unwrap(),
    )
    .unwrap();
    event_loop.read_ready(token, &shared);
    let mut output = Vec::new();
    until(&mut event_loop, &shared, || {
        output.extend(io_tests::messages(&mut peer, &mut input));
        output
            .iter()
            .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
    });
    assert!(output.iter().any(|message| matches!(message, ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }) if output == "yes")));
    assert_eq!(
        shared.inner.lock().paste_buffers[0].data.as_ref(),
        b"nested bytes"
    );
    event_loop.remove(token, &shared);
}

#[test]
fn pane_stdin_requests_one_chunk_at_a_time_and_cancels_on_target_loss() {
    let shared = Arc::new(Shared::new(113));
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, Arc::clone(&mailbox));
    let _writer = ClientWriterRegistrationGuard::new(&shared, client, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "e11stream", "exec sleep 30"]),
        )
        .unwrap();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let mut command = CommandInvocation::new("split-window", ["-d", "-I", "-t", "e11stream:"]);
    command.set_stdin_available(true);
    let mut task = wait_queue::CommandTask::new(
        &shared,
        client,
        ClientKind::Command,
        &context,
        1,
        &command,
        false,
    )
    .unwrap_or_else(|_| panic!("stdin task"));
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    let (request, pane) = {
        let inner = shared.inner.lock();
        assert_eq!(inner.client_file_waiters.len(), 1);
        let (&request, waiter) = inner.client_file_waiters.first_key_value().unwrap();
        (request, waiter.pane.unwrap())
    };
    shared.complete_client_file(
        client,
        ClientFileResponse {
            request_id: request,
            data: b"first\0chunk\xff".to_vec(),
            error: None,
        },
    );
    assert!(!task.ready());
    let next = {
        let inner = shared.inner.lock();
        assert_eq!(inner.client_file_waiters.len(), 1);
        *inner.client_file_waiters.first_key_value().unwrap().0
    };
    assert_ne!(request, next);
    shared.complete_client_file(
        client,
        ClientFileResponse {
            request_id: request,
            data: b"duplicate".to_vec(),
            error: None,
        },
    );
    assert_eq!(shared.inner.lock().client_file_waiters.len(), 1);
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("kill-pane", ["-t", &pane.to_string()]),
        )
        .unwrap();
    assert!(task.ready());
    assert!(shared.inner.lock().client_file_waiters.is_empty());
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    let (_, client_exit, _, _) = task.finish();
    assert!(client_exit);
    event_loop.turn(&shared).unwrap();
    shared.request_shutdown();
}

#[test]
fn new_pane_stdin_streams_into_the_float_it_creates() {
    let shared = Arc::new(Shared::new(113));
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, Arc::clone(&mailbox));
    let _writer = ClientWriterRegistrationGuard::new(&shared, client, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "e11float", "exec sleep 30"]),
        )
        .unwrap();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let mut command = CommandInvocation::new("new-pane", ["-d", "-I", "-t", "e11float:"]);
    command.set_stdin_available(true);
    let mut task = wait_queue::CommandTask::new(
        &shared,
        client,
        ClientKind::Command,
        &context,
        1,
        &command,
        false,
    )
    .unwrap_or_else(|_| panic!("stdin task"));
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    let pane = {
        let inner = shared.inner.lock();
        assert_eq!(inner.client_file_waiters.len(), 1);
        let pane = inner
            .client_file_waiters
            .first_key_value()
            .unwrap()
            .1
            .pane
            .unwrap();
        assert!(
            inner
                .engine
                .state
                .windows
                .values()
                .any(|window| window.is_floating(pane))
        );
        pane
    };
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("kill-pane", ["-t", &pane.to_string()]),
        )
        .unwrap();
    assert!(task.ready());
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    event_loop.turn(&shared).unwrap();
    shared.request_shutdown();
}

#[test]
fn sourced_alias_keeps_raw_stdout_claim_across_a_file_reply() {
    let shared = Arc::new(Shared::new(114));
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(
                "set-option",
                [
                    "-s",
                    "command-alias[95]",
                    "writer=save-buffer -b initial - ; display-message -p alias-hidden",
                ],
            ),
        )
        .unwrap();
    shared
        .buffer_command(
            &ExecutionContext::default(),
            "set-buffer",
            &["-b", "initial", "first\n"].map(RawText::from),
        )
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("raw.conf");
    fs::write(
        &path,
        "writer\nload-buffer -b nested -\ndisplay-message -p hidden\nsave-buffer -b nested -\n",
    )
    .unwrap();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer) = pair(&mut event_loop);
    let ProtocolMessage::Exec(mut command) = request(vec![CommandInvocation::new(
        "source-file",
        [path.to_str().unwrap()],
    )]) else {
        unreachable!()
    };
    command.flags.set(ExecFlags::STDIN_AVAILABLE, true);
    peer.write_all(&encode_protocol_message(&ProtocolMessage::Exec(command)).unwrap())
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
    peer.write_all(
        &encode_protocol_message(&ProtocolMessage::ClientFileResponse(ClientFileResponse {
            request_id: file.unwrap().request_id,
            data: b"second\0\xff".to_vec(),
            error: None,
        }))
        .unwrap(),
    )
    .unwrap();
    event_loop.read_ready(token, &shared);
    let mut output = Vec::new();
    until(&mut event_loop, &shared, || {
        output.extend(io_tests::messages(&mut peer, &mut input));
        output
            .iter()
            .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
    });
    assert!(output.iter().any(|message| matches!(message, ProtocolMessage::CommandResponse(CommandResponse::Success { output, exit_code: 1, .. }) if output == "first\n")));
    event_loop.remove(token, &shared);
}
