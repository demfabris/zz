use super::*;

fn workspace() -> (Arc<Shared>, ExecutionContext) {
    let shared = Arc::new(Shared::new(7));
    let context = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("overlays").unwrap();
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    (shared, context)
}

fn target(shared: &Arc<Shared>, context: &ExecutionContext, index: u64) -> ClientId {
    let (client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some(format!("overlay-{index}")),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(client, context.session.unwrap()).unwrap();
    client
}

fn command(name: &str, index: u64) -> CommandInvocation {
    let target = format!("overlay-{index}");
    match name {
        "command-prompt" => {
            CommandInvocation::new(name, ["-t", &target, "set-environment -g ANSWER %%"])
        }
        "confirm-before" => {
            CommandInvocation::new(name, ["-t", &target, "set-environment -g CONFIRMED yes"])
        }
        _ => unreachable!(),
    }
}

fn task(
    shared: &Arc<Shared>,
    context: &ExecutionContext,
    name: &str,
    index: u64,
) -> (ClientId, wait_queue::CommandTask) {
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut task = wait_queue::CommandTask::new(
        shared,
        client,
        ClientKind::Command,
        context,
        index,
        &command(name, index),
        false,
    )
    .unwrap();
    assert!(
        matches!(task.run(true), wait_queue::Progress::Waiting),
        "{name}"
    );
    assert!(!task.ready());
    (client, task)
}

fn continuation(shared: &Shared, target: ClientId, name: &str) -> cmdq::WaitContinuation {
    let inner = shared.inner.lock();
    let client = inner.client(target).unwrap();
    let waiter = match name {
        "command-prompt" => client
            .command_prompt
            .as_ref()
            .unwrap()
            .waiter
            .as_ref()
            .unwrap(),
        "confirm-before" => match &client.confirm.as_ref().unwrap().execution {
            ConfirmExecution::Blocking { waiter } => waiter,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    waiter.0.continuation.clone()
}

#[test]
fn twenty_parked_overlays_add_zero_workers() {
    if !super::solo_tests::rerun_alone(
        "daemon::overlay_queue_e07_tests::twenty_parked_overlays_add_zero_workers",
    ) {
        return;
    }
    for name in ["command-prompt", "confirm-before"] {
        let (shared, context) = workspace();
        let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
        let targets = (0..20)
            .map(|index| target(&shared, &context, index))
            .collect::<Vec<_>>();
        let before = crate::process_info::sample(std::process::id())
            .unwrap()
            .threads;
        let mut tasks = (0..20)
            .map(|index| task(&shared, &context, name, index).1)
            .collect::<Vec<_>>();
        assert_eq!(shared.connection_threads.worker_count(), 0);
        assert!(
            crate::process_info::sample(std::process::id())
                .unwrap()
                .threads
                <= before
        );
        for client in targets {
            shared.detach(client);
        }
        for task in &mut tasks {
            assert!(task.ready());
            assert!(matches!(task.run(true), wait_queue::Progress::Done));
        }
        for task in tasks {
            let (response, _, _, _) = task.finish();
            assert!(
                matches!(response, CommandResponse::Success { exit_code, .. } if exit_code == u8::from(name == "confirm-before"))
            );
        }
        assert_eq!(shared.connection_threads.worker_count(), 0);
    }
}

#[test]
fn detach_and_disconnect_resume_each_overlay_once() {
    for name in ["command-prompt", "confirm-before"] {
        for disconnect in [false, true] {
            let (shared, context) = workspace();
            let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
            let target = target(&shared, &context, 0);
            let (_, mut task) = task(&shared, &context, name, 0);
            let continuation = continuation(&shared, target, name);
            if disconnect {
                shared.unregister(target);
                shared.unregister(target);
            } else {
                shared.detach(target);
                shared.detach(target);
            }
            assert!(task.ready());
            assert!(!continuation.complete());
            assert!(matches!(task.run(true), wait_queue::Progress::Done));
            let (response, _, _, _) = task.finish();
            assert!(
                matches!(response, CommandResponse::Success { exit_code, .. } if exit_code == u8::from(name == "confirm-before"))
            );
        }
    }
}

#[test]
fn issuing_client_disconnect_wakes_overlay_on_another_client() {
    for name in ["command-prompt", "confirm-before"] {
        let (shared, context) = workspace();
        let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
        let target = target(&shared, &context, 0);
        let (client, task) = task(&shared, &context, name, 0);
        let continuation = continuation(&shared, target, name);
        shared.unregister(client);
        assert!(task.ready());
        assert!(!continuation.complete());
    }
}

#[test]
fn accepted_confirm_uses_child_frames_before_the_parent_resumes() {
    let (shared, context) = workspace();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let target = target(&shared, &context, 0);
    let (_, mut task) = task(&shared, &context, "confirm-before", 0);
    shared.input_confirm(target, &context, ConfirmAction::Reply(true));
    assert!(task.ready());
    assert!(matches!(task.run(true), wait_queue::Progress::Worker));
    finish(&mut task);
    let (response, _, _, _) = task.finish();
    assert!(matches!(
        response,
        CommandResponse::Success { exit_code: 0, .. }
    ));
    assert_eq!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("CONFIRMED"),
        Some("yes".to_owned())
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

fn finish(task: &mut wait_queue::CommandTask) {
    for _ in 0..100 {
        match task.run(false) {
            wait_queue::Progress::Done => return,
            wait_queue::Progress::Ready | wait_queue::Progress::Worker => {}
            wait_queue::Progress::Waiting => panic!("unexpected second wait"),
        }
    }
    panic!("overlay queue did not finish");
}

#[test]
fn replacement_and_cancellation_resume_overlays_once() {
    let (shared, context) = workspace();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let target = target(&shared, &context, 0);
    for name in ["command-prompt", "confirm-before"] {
        let (_, mut pending) = self::task(&shared, &context, name, 0);
        let continuation = self::continuation(&shared, target, name);
        shared
            .execute(
                target,
                ClientKind::Interactive,
                &mut context.clone(),
                &CommandInvocation::new("confirm-before", ["-b", "display-message replaced"]),
            )
            .unwrap();
        assert!(pending.ready());
        assert!(!continuation.complete());
        finish(&mut pending);
        pending.finish();
        shared.input_confirm(target, &context, ConfirmAction::Reply(false));

        let (_, pending) = self::task(&shared, &context, name, 0);
        let continuation = self::continuation(&shared, target, name);
        drop(pending);
        assert!(continuation.ready());
        assert!(!continuation.complete());
        shared.detach(target);
        shared.attach(target, context.session.unwrap()).unwrap();
    }
}

#[test]
fn prompt_answer_runs_before_the_issuing_queue_finishes() {
    let (shared, context) = workspace();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let target = target(&shared, &context, 0);
    let (_, mut task) = task(&shared, &context, "command-prompt", 0);
    shared
        .input_command_prompt_action(
            target,
            ClientKind::Interactive,
            &mut context.clone(),
            CommandPromptAction::Submit {
                input: "answer".to_owned(),
            },
        )
        .unwrap();
    assert!(task.ready());
    assert_eq!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("ANSWER"),
        Some("answer".to_owned())
    );
    finish(&mut task);
    task.finish();
}
