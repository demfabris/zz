use super::*;

fn workspace() -> (Arc<Shared>, ExecutionContext) {
    let shared = Arc::new(Shared::new(7));
    let context = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("menus").unwrap();
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    (shared, context)
}

fn target(shared: &Arc<Shared>, context: &ExecutionContext, index: u64) -> ClientId {
    let (client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some(format!("menu-{index}")),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(client, context.session.unwrap()).unwrap();
    client
}

fn task(
    shared: &Arc<Shared>,
    context: &ExecutionContext,
    index: u64,
    action: &str,
) -> (ClientId, wait_queue::CommandTask) {
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut task = wait_queue::CommandTask::new(
        shared,
        client,
        ClientKind::Command,
        context,
        index,
        &CommandInvocation::new(
            "display-menu",
            ["-c", &format!("menu-{index}"), "row", "r", action],
        ),
        false,
    )
    .unwrap();
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    assert!(!task.ready());
    (client, task)
}

fn continuation(shared: &Shared, target: ClientId) -> cmdq::WaitContinuation {
    shared.inner.lock().clients[&target]
        .menu
        .as_ref()
        .unwrap()
        .waiter
        .as_ref()
        .unwrap()
        .0
        .continuation
        .clone()
}

fn finish(task: &mut wait_queue::CommandTask) {
    assert!(task.ready());
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
}

#[test]
fn twenty_open_blocking_menus_add_zero_workers() {
    let (shared, context) = workspace();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let targets = (0..20)
        .map(|index| target(&shared, &context, index))
        .collect::<Vec<_>>();
    let before = crate::process_info::sample(std::process::id())
        .unwrap()
        .threads;
    let mut tasks = (0..20)
        .map(|index| task(&shared, &context, index, "display-message chosen").1)
        .collect::<Vec<_>>();
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        crate::process_info::sample(std::process::id())
            .unwrap()
            .threads
            <= before
    );
    for client in targets {
        shared.input_menu(client, &context, MenuAction::Cancel);
    }
    for task in &mut tasks {
        finish(task);
    }
    for task in tasks {
        assert!(matches!(
            task.finish().0,
            CommandResponse::Success { exit_code: 0, .. }
        ));
    }
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[test]
fn detach_and_disconnect_resume_a_menu_once() {
    for disconnect in [false, true] {
        let (shared, context) = workspace();
        let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
        let target = target(&shared, &context, 0);
        let (_, mut task) = task(&shared, &context, 0, "display-message chosen");
        let continuation = continuation(&shared, target);
        if disconnect {
            shared.unregister(target);
            shared.unregister(target);
        } else {
            shared.detach(target);
            shared.detach(target);
        }
        finish(&mut task);
        assert!(!continuation.complete());
        assert!(matches!(
            task.finish().0,
            CommandResponse::Success { exit_code: 0, .. }
        ));
    }
}

#[test]
fn issuing_client_disconnect_and_task_drop_resume_once() {
    for cancel_task in [false, true] {
        let (shared, context) = workspace();
        let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
        let target = target(&shared, &context, 0);
        let (client, task) = task(&shared, &context, 0, "display-message chosen");
        let continuation = continuation(&shared, target);
        if cancel_task {
            drop(task);
        } else {
            shared.unregister(client);
            assert!(task.ready());
        }
        assert!(continuation.ready());
        assert!(!continuation.complete());
    }
}

#[test]
fn selected_action_is_queued_after_close_against_the_saved_target() {
    let (shared, context) = workspace();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let target = target(&shared, &context, 0);
    let (_, mut task) = task(
        &shared,
        &context,
        0,
        "set-environment -g MENU_TARGET '#{pane_id}'",
    );
    let other = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("other").unwrap();
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    shared.input_menu(target, &other, MenuAction::Choose(0));
    assert!(shared.inner.lock().clients[&target].menu.is_none());
    assert!(task.ready());
    let mut actions = std::mem::take(&mut *shared.pending_wait_queues.lock());
    assert_eq!(actions.len(), 1);
    for action in &mut actions {
        for _ in 0..100 {
            if matches!(action.run(false), wait_queue::Progress::Done) {
                break;
            }
        }
    }
    assert_eq!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("MENU_TARGET"),
        Some(context.pane.unwrap().to_string())
    );
    finish(&mut task);
    assert!(matches!(
        task.finish().0,
        CommandResponse::Success { exit_code: 0, .. }
    ));
}
