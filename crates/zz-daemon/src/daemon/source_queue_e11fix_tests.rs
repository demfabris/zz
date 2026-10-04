use super::*;
use zz_protocol::{ClientEnvironmentBlob, ExecFlags, ExecRequest, encode_protocol_message};

struct Release(PathBuf);

impl Drop for Release {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "");
    }
}

#[test]
fn sourced_commands_run_once_when_their_new_hooks_park() {
    for (index, config) in [
        "set-hook -g after-bind-key 'set-buffer called ; run-shell \"while [ ! -f GATE ]; do sleep 0.01; done\"'\nbind-key -T e11fix a display-message binding\nset -g @after yes\n",
        "set -g after-set-option 'set-hook -gu after-set-option ; set-buffer called ; run-shell \"while [ ! -f GATE ]; do sleep 0.01; done\"'\nset -g @after yes\n",
    ]
    .into_iter()
    .enumerate()
    {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hooks.conf");
        let release = Release(directory.path().join("release"));
        fs::write(&path, config.replace("GATE", release.0.to_str().unwrap())).unwrap();
        let shared = Arc::new(Shared::new(120 + index as u64));
        let mut event_loop = EventLoop::empty(&shared).unwrap();
        event_loop.signals = Some(SignalPipes::new(&event_loop.poll).unwrap());
        let (mut peer, server) = UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
        let request = ProtocolMessage::Exec(ExecRequest {
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
            commands: vec![
                CommandInvocation::new("source-file", [path.to_str().unwrap()]),
                CommandInvocation::new("display-message", ["-p", "#{@after}"]),
            ],
            raw_control_line: None,
        });
        peer.write_all(&encode_protocol_message(&request).unwrap())
            .unwrap();
        event_loop.read_ready(token, &shared);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut input = Inbound::default();
        let mut output = Vec::new();
        while shared.inner.lock().paste_buffers.is_empty() {
            assert!(Instant::now() < deadline, "case {index}: {output:?}");
            event_loop.poll_test_turn(&shared, Duration::from_millis(1));
            output.extend(io_tests::messages(&mut peer, &mut input));
        }
        let mut context = ExecutionContext::default();
        let after = shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("display-message", ["-p", "#{@after}"]),
            )
            .unwrap();
        assert!(after.output.to_string().trim().is_empty(), "case {index}: {output:?}");
        assert!(!output.iter().any(|message| matches!(message, ProtocolMessage::ExecExit(_))));
        assert_eq!(shared.inner.lock().paste_buffers.len(), 1);
        fs::write(&release.0, "").unwrap();
        while !output
            .iter()
            .any(|message| matches!(message, ProtocolMessage::ExecExit(_)))
        {
            assert!(Instant::now() < deadline);
            event_loop.poll_test_turn(&shared, Duration::from_millis(1));
            output.extend(io_tests::messages(&mut peer, &mut input));
        }
        assert!(output.iter().any(|message| matches!(message, ProtocolMessage::CommandResponse(CommandResponse::Success { output, exit_code: 0, .. }) if output == "yes")));
        assert_eq!(shared.inner.lock().paste_buffers.len(), 1);
        assert_eq!(shared.connection_threads.worker_count(), 0);
        event_loop.remove(token, &shared);
    }
}
