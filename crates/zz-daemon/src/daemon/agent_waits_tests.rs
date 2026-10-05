use super::*;

#[cfg(feature = "agent")]
fn native_agent() -> (Arc<Shared>, Arc<AgentRuntime>, PaneId) {
    use crate::agent::fixture::{Behavior, fixture_runner};
    let shared = Arc::new(Shared::new(1818));
    let runtime = shared.build_agent_runtime(None).unwrap();
    runtime.set_runner_factory(Box::new(|_| {
        fixture_runner(
            AgentProvider::Codex,
            Behavior::Hang,
            zz_protocol::AgentAutoApprove::Off,
            true,
        )
    }));
    let mut context = ExecutionContext::default();
    for command in [
        CommandInvocation::new("new-session", ["-d", "-s", "e18"]),
        CommandInvocation::new("set-option", ["-g", "experimental-agent-pane", "on"]),
        CommandInvocation::new("split-window", ["--kind", "picker", "-v"]),
    ] {
        shared
            .execute(
                ClientId(99),
                ClientKind::Interactive,
                &mut context,
                &command,
            )
            .unwrap();
    }
    let pane = context.pane.unwrap();
    shared
        .execute(
            ClientId(99),
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("select-pane-kind", ["-t", &pane.to_string(), "agent"]),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !runtime
        .wire_state(pane)
        .is_some_and(|state| matches!(state.phase, zz_protocol::AgentConnectionPhase::Ready))
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    (shared, runtime, pane)
}

#[cfg(feature = "agent")]
#[test]
fn twenty_parked_agent_waits_add_zero_command_workers() {
    let (shared, runtime, pane) = native_agent();
    let mut panes = vec![pane];
    for _ in 0..4 {
        let mut context =
            ExecutionContext::for_pane(&shared.inner.lock().engine.state, pane).unwrap();
        shared
            .execute(
                ClientId(99),
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("split-window", ["--kind", "picker", "-h"]),
            )
            .unwrap();
        let next = context.pane.unwrap();
        shared
            .execute(
                ClientId(99),
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("select-pane-kind", ["-t", &next.to_string(), "agent"]),
            )
            .unwrap();
        panes.push(next);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !panes.iter().all(|pane| {
        runtime
            .wire_state(*pane)
            .is_some_and(|state| matches!(state.phase, zz_protocol::AgentConnectionPhase::Ready))
    }) {
        assert!(
            Instant::now() < deadline,
            "agent panes never all reached Ready: {:?}",
            panes
                .iter()
                .map(|pane| runtime.wire_state(*pane).map(|state| state.phase))
                .collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(2));
    }
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    let mut tasks = (0..20)
        .map(|index| {
            let pane = panes[index as usize / 4];
            let command = CommandInvocation::new(
                "agent-send",
                ["-t", &pane.to_string(), "--wait", "--timeout", "0", "hello"],
            );
            assert!(wait_queue::can_run_inline(
                &shared,
                &ExecutionContext::default(),
                &command
            ));
            let mut task = wait_queue::CommandTask::new(
                &shared,
                ClientId(100 + index),
                ClientKind::Command,
                &ExecutionContext::default(),
                index,
                &command,
                false,
            )
            .unwrap();
            assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
            task
        })
        .collect::<Vec<_>>();
    while !panes.iter().all(|pane| {
        runtime.wire_state(*pane).is_some_and(|state| {
            state.queued_prompts == 3
                && matches!(state.phase, zz_protocol::AgentConnectionPhase::Running)
        })
    }) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    assert!(tasks.iter().all(|task| !task.ready()));
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
    shared.shutdown_agents();
    while tasks.iter().any(|task| !task.ready()) {
        shared.terminal_requests.turn(&shared);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    for task in &mut tasks {
        assert!(matches!(task.run(true), wait_queue::Progress::Done));
    }
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[cfg(feature = "agent")]
#[test]
fn agent_wait_timeout_leaves_the_native_turn_running() {
    let (shared, runtime, pane) = native_agent();
    let result = shared.execute(
        ClientId(99),
        ClientKind::Command,
        &mut ExecutionContext::default(),
        &CommandInvocation::new(
            "agent-send",
            [
                "-t",
                &pane.to_string(),
                "--wait",
                "--timeout",
                "0.05",
                "hello",
            ],
        ),
    );
    assert!(matches!(
        result,
        Err(DaemonError::CommandExit { exit_code: 124, .. })
    ));
    assert!(matches!(
        runtime.wire_state(pane).unwrap().phase,
        zz_protocol::AgentConnectionPhase::Running
    ));
    assert_eq!(shared.connection_threads.worker_count(), 0);
    shared.shutdown_agents();
}

#[test]
fn typed_reply_completes_once_and_timeout_removes_the_deadline() {
    let shared = Arc::new(Shared::new(1819));
    let item = shared.command_item(None);
    item.command_item.as_ref().unwrap().lock().loop_wait = true;
    let (wait, reply) = reply(
        &item,
        Some(Duration::from_secs(30)),
        |_, outcome| match outcome {
            ReplyOutcome::Ready(value) => Ok(Execution {
                output: value,
                effects: Vec::new(),
            }),
            _ => panic!("expected reply"),
        },
    );
    let state = Arc::clone(&wait.state);
    wait.finish(&item, Execution::default()).unwrap();
    reply.try_send(RawText::from("first"));
    reply.try_send(RawText::from("second"));
    assert_eq!(shared.terminal_requests.turn(&shared), None);
    assert!(state.continuation.ready());
    let mut result = Ok(Execution::default());
    state.apply(&mut result);
    assert_eq!(result.unwrap().output, "first");
}

#[test]
fn terminal_state_wait_observes_a_short_turn_before_submission_finishes() {
    let shared = Arc::new(Shared::new(1820));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(99),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "e18-terminal"]),
        )
        .unwrap();
    let pane = context.pane.unwrap();
    shared.write_pane_agent_state(pane, "idle");
    let item = shared.command_item(None);
    item.command_item.as_ref().unwrap().lock().loop_wait = true;
    let wait = CommandWait::new(&item);
    let state = Arc::clone(&wait.state);
    let terminal = TerminalWait::subscribe(
        &item,
        ClientId(99),
        pane,
        &ParsedAgentSend::default(),
        wait.start(),
        "idle",
    );
    wait.finish(&item, Execution::default()).unwrap();
    shared.write_pane_agent_state(pane, "working");
    shared.write_pane_agent_state(pane, "idle");
    terminal.submitted(Ok(()));
    assert_eq!(shared.terminal_requests.turn(&shared), None);
    assert!(state.continuation.ready());
    assert!(shared.agent_state_waits.lock().is_empty());
    let mut result = Ok(Execution::default());
    state.apply(&mut result);
    assert!(result.is_ok());
}

#[test]
fn gui_reply_from_another_client_preserves_the_owner_request() {
    let shared = Arc::new(Shared::new(1821));
    let (send, response) = crossbeam_channel::bounded(1);
    shared.inner.lock().pending_gui_requests.insert(
        7,
        PendingGuiRequest {
            client: ClientId(1),
            reply: send.into(),
        },
    );
    shared.complete_gui_request(
        ClientId(2),
        GuiResponse::Success {
            request_id: 7,
            output: "wrong".to_owned(),
        },
    );
    assert!(shared.inner.lock().pending_gui_requests.contains_key(&7));
    assert!(response.try_recv().is_err());
    shared.complete_gui_request(
        ClientId(1),
        GuiResponse::Success {
            request_id: 7,
            output: "owner".to_owned(),
        },
    );
    assert_eq!(response.try_recv().unwrap(), Ok("owner".to_owned()));
}

#[test]
fn gui_delivery_failure_keeps_the_original_error_after_queue_resume() {
    let shared = Arc::new(Shared::new(1822));
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "e18-gui"]),
        )
        .unwrap();
    let pane = context.pane.unwrap();
    shared.attach(client, context.session.unwrap()).unwrap();
    mailbox.close();
    let item = shared.command_item(None);
    item.command_item.as_ref().unwrap().lock().loop_wait = true;
    let mut result = item.request_from_gui(pane, |request_id| EventPayload::AgentCommand {
        pane,
        request_id,
        command: AgentCommand::ComposerAppend {
            text: "hello".to_owned(),
        },
    });
    shared.terminal_requests.turn(&shared);
    let item = item.command_item.as_ref().unwrap().lock();
    let state = item
        .pending_wait
        .as_ref()
        .unwrap()
        .terminal
        .as_ref()
        .unwrap();
    assert!(state.continuation.ready());
    state.apply(&mut result);
    assert!(
        matches!(result, Err(DaemonError::Server(ServerError::PaneNotAttached(target))) if target == pane)
    );
    assert!(shared.inner.lock().pending_gui_requests.is_empty());
}
