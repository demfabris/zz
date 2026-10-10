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
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    (client, task)
}

fn menu_open(shared: &Shared, target: ClientId) -> bool {
    shared
        .inner
        .lock()
        .clients
        .get(&target)
        .is_some_and(|client| client.menu.is_some())
}

#[test]
fn twenty_open_menus_return_at_once_and_add_zero_workers() {
    if !super::solo_tests::rerun_alone(
        "daemon::menu_queue_e08_tests::twenty_open_menus_return_at_once_and_add_zero_workers",
    ) {
        return;
    }
    let (shared, context) = workspace();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let targets = (0..20)
        .map(|index| target(&shared, &context, index))
        .collect::<Vec<_>>();
    let before = zz_daemon_client::process_info::sample(std::process::id())
        .unwrap()
        .threads;
    let tasks = (0..20)
        .map(|index| task(&shared, &context, index, "display-message chosen").1)
        .collect::<Vec<_>>();
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        zz_daemon_client::process_info::sample(std::process::id())
            .unwrap()
            .threads
            <= before
    );
    for task in tasks {
        assert!(matches!(
            task.finish().0,
            CommandResponse::Success { exit_code: 0, .. }
        ));
    }
    for client in targets {
        assert!(menu_open(&shared, client));
        shared.input_menu(client, &context, MenuAction::Cancel);
        assert!(!menu_open(&shared, client));
    }
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[test]
fn detach_and_disconnect_close_the_menu() {
    for disconnect in [false, true] {
        let (shared, context) = workspace();
        let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
        let target = target(&shared, &context, 0);
        let (_, task) = task(&shared, &context, 0, "display-message chosen");
        assert!(menu_open(&shared, target));
        if disconnect {
            shared.unregister(target);
            shared.unregister(target);
        } else {
            shared.detach(target);
            shared.detach(target);
        }
        assert!(!menu_open(&shared, target));
        assert!(matches!(
            task.finish().0,
            CommandResponse::Success { exit_code: 0, .. }
        ));
    }
}

#[test]
fn issuing_client_disconnect_leaves_the_menu_up() {
    let (shared, context) = workspace();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let target = target(&shared, &context, 0);
    let (client, task) = task(&shared, &context, 0, "display-message chosen");
    drop(task);
    shared.unregister(client);
    assert!(menu_open(&shared, target));
}

#[test]
fn selected_action_is_queued_after_close_against_the_saved_target() {
    let (shared, context) = workspace();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let target = target(&shared, &context, 0);
    let (_, task) = task(
        &shared,
        &context,
        0,
        "set-environment -g MENU_TARGET '#{pane_id}'",
    );
    assert!(matches!(
        task.finish().0,
        CommandResponse::Success { exit_code: 0, .. }
    ));
    let other = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("other").unwrap();
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    shared.input_menu(target, &other, MenuAction::Choose(0));
    assert!(!menu_open(&shared, target));
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
}

#[test]
fn a_centred_menu_centres_on_a_window_smaller_than_the_client() {
    let (shared, mut context) = workspace();
    let client = target(&shared, &context, 0);
    shared.inner.lock().client_mut(client).expect("client").size = Some((80, 24));
    for command in [
        CommandInvocation::new("set-option", ["-w", "window-size", "manual"]),
        CommandInvocation::new("resize-window", ["-x", "40", "-y", "10"]),
        CommandInvocation::new(
            "display-menu",
            [
                "-x",
                "C",
                "-y",
                "C",
                "-T",
                "MENU",
                "Alpha",
                "a",
                "set -g @x 1",
                "Beta",
                "b",
                "set -g @x 2",
            ],
        ),
    ] {
        shared
            .execute(client, ClientKind::Interactive, &mut context, &command)
            .expect("command");
    }
    let inner = shared.inner.lock();
    let state = &inner.clients[&client].menu.as_ref().expect("menu").state;
    assert_eq!(
        (state.left, state.top, state.width, state.height),
        (13, 2, 13, 4)
    );
}
