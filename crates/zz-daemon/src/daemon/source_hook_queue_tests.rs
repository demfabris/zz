use super::*;

fn wait_until_parked(shared: &Shared, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if shared
            .inner
            .lock()
            .wait_channels
            .get(name)
            .is_some_and(|channel| !channel.waiters.is_empty())
        {
            return;
        }
        assert!(Instant::now() < deadline, "queue did not park on {name}");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn foreground_callback_waits_for_its_after_hook_before_resuming_the_alias() {
    let shared = Arc::new(Shared::new(304));
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new(
                "set-option",
                [
                    "-s",
                    "command-alias[101]",
                    "hook-parent=if-shell -F 1 'set-option -g @trigger yes' ; display-message -p parent-after",
                ],
            ),
        )
        .unwrap();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new(
                "set-hook",
                [
                    "-g",
                    "after-set-option",
                    "run-shell -C 'display-message -p hook-before ; wait-for hook-resume ; display-message -p hook-after'",
                ],
            ),
        )
        .unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || {
        sender
            .send(worker_shared.execute_command_request(
                client,
                ClientKind::Command,
                &mut context,
                1,
                &CommandInvocation::new("hook-parent", [] as [&str; 0]),
            ))
            .unwrap();
    });
    wait_until_parked(&shared, "hook-resume");
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let mut signal_context = ExecutionContext::default();
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut signal_context,
            &CommandInvocation::new("wait-for", ["-S", "hook-resume"]),
        )
        .unwrap();
    let response = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.join().unwrap();
    assert!(
        matches!(&response, CommandResponse::Success { output, exit_code: 0, .. }
        if output == "hook-before\nhook-after\nparent-after")
    );
}

#[cfg(unix)]
#[test]
fn nested_source_frames_keep_current_file_and_frozen_aliases_across_a_wait() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root.conf");
    let child = directory.path().join("child.conf");
    let grandchild = directory.path().join("grandchild.conf");
    let ready = directory.path().join("ready");
    let resume = directory.path().join("resume");
    fs::write(
        &root,
        format!(
            "display-message -p 'root-before:#{{current_file}}'\nsource-file '{}'\nfrozen\ndisplay-message -p 'root-after:#{{current_file}}'\n",
            child.display()
        ),
    )
    .unwrap();
    fs::write(
        &child,
        format!(
            "display-message -p 'child-before:#{{current_file}}'\nsource-file '{}'\ndisplay-message -p 'child-after:#{{current_file}}'\n",
            grandchild.display()
        ),
    )
    .unwrap();
    fs::write(
        &grandchild,
        format!(
            "run-shell 'printf ready > {}; while [ ! -f {} ]; do sleep 0.01; done'\nset-option -s command-alias[102] 'frozen=display-message -p changed'\ndisplay-message -p 'grandchild:#{{current_file}}'\n",
            ready.display(), resume.display()
        ),
    )
    .unwrap();
    let shared = Arc::new(Shared::new(305));
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new(
                "set-option",
                [
                    "-s",
                    "command-alias[102]",
                    "frozen=display-message -p original",
                ],
            ),
        )
        .unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker_shared = Arc::clone(&shared);
    let worker_root = root.clone();
    let worker = thread::spawn(move || {
        sender
            .send(worker_shared.execute_command_request(
                client,
                ClientKind::Command,
                &mut context,
                1,
                &CommandInvocation::new("source-file", [worker_root.display().to_string()]),
            ))
            .unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "source shell did not start");
        thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    fs::write(&resume, "resume").unwrap();
    let response = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.join().unwrap();
    let expected = format!(
        "root-before:{}\nchild-before:{}\ngrandchild:{}\nchild-after:{}\noriginal\nroot-after:{}\n",
        root.display(),
        child.display(),
        grandchild.display(),
        child.display(),
        root.display()
    );
    assert!(
        matches!(&response, CommandResponse::Success { output, exit_code: 0, .. }
        if output == &expected),
        "response: {response:?}, expected: {expected:?}"
    );
}

#[test]
fn nested_source_batch_keeps_construction_and_parse_warnings_in_file_order() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("construction.conf");
    let second = directory.path().join("parse.conf");
    let root = directory.path().join("root.conf");
    fs::write(&first, "wibble\n").unwrap();
    fs::write(&second, "%endif\n").unwrap();
    fs::write(
        &root,
        format!("source-file '{}' '{}'\n", first.display(), second.display()),
    )
    .unwrap();
    let shared = Arc::new(Shared::new(306));
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("new-session", ["-d", "-s", "warning-order"]),
        )
        .unwrap();
    let mailbox = OutboundMailbox::new();
    let (control, _) =
        shared.register_subscribed(ClientKind::Control, None, None, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            control,
            ClientKind::Control,
            &mut context,
            &CommandInvocation::new("attach-session", ["-t", "warning-order"]),
        )
        .unwrap();
    super::tests::take_reliable_messages(&mailbox);
    let source = CommandInvocation::new("source-file", [root.display().to_string()]).with_source(
        SourceSpan {
            source: "<control>".to_owned(),
            line: 1,
            column: 1,
        },
    );
    shared.execute_command_request(control, ClientKind::Control, &mut context, 1, &source);
    let warnings = super::tests::take_reliable_messages(&mailbox)
        .into_iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::ControlConfigError { text },
                ..
            }) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 2, "warnings: {warnings:?}");
    assert!(warnings[0].starts_with(&first.display().to_string()));
    assert!(warnings[1].starts_with(&second.display().to_string()));
}
