use super::*;

fn raw_request(line: &str) -> zz_protocol::ExecRequest {
    zz_protocol::ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: zz_protocol::ExecFlags::default(),
        client_instance_id: ClientInstanceId(919),
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
        commands: Vec::new(),
        raw_control_line: Some(line.to_owned()),
    }
}

#[test]
fn compact_control_dispatch_errors_keep_preceding_effects_and_command_error_hooks() {
    let shared = Arc::new(Shared::new(34));
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Control, None, None, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new(
                "set-hook",
                [
                    "-g",
                    "command-error",
                    "set-environment -g CONTROL_ERROR_HOOK yes",
                ],
            ),
        )
        .expect("error hook");
    tests::take_reliable_messages(&mailbox);
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        raw_request(
            "set-environment -g CONTROL_BEFORE yes ; list-sessions -Z ; set-environment -g CONTROL_AFTER yes",
        ),
        &mailbox,
    );
    let inner = shared.inner.lock();
    assert_eq!(
        inner
            .engine
            .global_environment_variable("CONTROL_BEFORE")
            .as_deref(),
        Some("yes")
    );
    assert_eq!(
        inner
            .engine
            .global_environment_variable("CONTROL_ERROR_HOOK")
            .as_deref(),
        Some("yes")
    );
    assert!(
        inner
            .engine
            .global_environment_variable("CONTROL_AFTER")
            .is_none()
    );
    drop(inner);
    let messages = tests::take_reliable_messages(&mailbox)
        .into_iter()
        .flat_map(|message| match message {
            ProtocolMessage::Batch(batch) => batch.messages().expect("batch"),
            message => vec![message],
        })
        .collect::<Vec<_>>();
    assert!(
        messages.iter().any(|message| matches!(message,
            ProtocolMessage::CommandResponse(CommandResponse::Error { request_id: 2, error, .. })
            if error.tmux_message() == "command list-sessions: unknown flag -Z"
        )),
        "{messages:?}"
    );
    assert!(matches!(
        messages.last(),
        Some(ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            outcome: zz_protocol::ExecOutcome::Ran,
            ..
        }))
    ));
}

#[test]
fn compact_control_typed_argument_errors_reject_the_entire_line_before_dispatch() {
    let shared = Arc::new(Shared::new(35));
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Control, None, None, Arc::clone(&mailbox));
    tests::take_reliable_messages(&mailbox);
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut ExecutionContext::default(),
        raw_request(
            "set-environment -g CONTROL_BEFORE yes ; bind-key -T { set-environment -g CONTROL_FORBIDDEN yes } F11 display-message",
        ),
        &mailbox,
    );
    let inner = shared.inner.lock();
    assert!(
        inner
            .engine
            .global_environment_variable("CONTROL_BEFORE")
            .is_none()
    );
    assert!(
        inner
            .engine
            .global_environment_variable("CONTROL_FORBIDDEN")
            .is_none()
    );
    drop(inner);
    assert!(matches!(tests::take_reliable_messages(&mailbox).as_slice(),
        [ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            outcome: zz_protocol::ExecOutcome::Rejected(error),
            ..
        })] if error.tmux_message() == "command bind-key: -T argument must be a string"
    ));
}
