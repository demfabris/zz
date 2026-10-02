use super::*;

fn queued_pane_focus_hooks(shared: &Shared) -> Vec<&'static str> {
    let inputs = shared.timer_rx.lock();
    let inputs = inputs.as_ref().expect("timer inputs");
    std::iter::from_fn(|| inputs.try_recv().ok())
        .filter_map(|input| match input {
            timers::TimerInput::Hooks(events) => Some(events),
            _ => None,
        })
        .flatten()
        .map(|event| event.name)
        .filter(|name| name.starts_with("pane-focus"))
        .collect()
}

#[test]
fn display_panes_moves_pane_focus_out_before_its_queue_waits() {
    let shared = Arc::new(Shared::new(7));
    let context = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("focus").unwrap();
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    let pane = context.pane.unwrap();
    let (target, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some("focus-target".to_owned()),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(target, context.session.unwrap()).unwrap();
    assert_eq!(shared.inner.lock().pane_focus, BTreeSet::from([pane]));
    queued_pane_focus_hooks(&shared);

    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut task = wait_queue::CommandTask::new(
        &shared,
        client,
        ClientKind::Command,
        &context,
        1,
        &CommandInvocation::new("display-panes", ["-t", "focus-target", "-d", "0"]),
        false,
    )
    .unwrap();
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    assert!(shared.inner.lock().pane_focus.is_empty());
    assert_eq!(queued_pane_focus_hooks(&shared), ["pane-focus-out"]);

    shared
        .input(
            target,
            ClientKind::Interactive,
            &mut context.clone(),
            InputMessage::DisplayPanes {
                action: DisplayPanesAction::Dismiss,
            },
        )
        .unwrap();
    assert_eq!(shared.inner.lock().pane_focus, BTreeSet::from([pane]));
    assert!(task.ready());
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    assert!(queued_pane_focus_hooks(&shared).is_empty());
    assert!(matches!(
        task.finish().0,
        CommandResponse::Success { exit_code: 0, .. }
    ));
}

#[test]
fn confirmed_chooser_kill_keeps_the_cursor_row_after_the_queued_kill() {
    let shared = Arc::new(Shared::new(7));
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let (session, windows, pane) = {
        let mut inner = shared.inner.lock();
        let (session, first, pane) = inner.engine.state.create_session("ckill").unwrap();
        let mut windows = vec![first];
        for name in ["w1", "w2", "w3"] {
            let (window, _) = inner
                .engine
                .state
                .create_window(session, Some(name.to_owned()), PaneKind::Terminal)
                .unwrap();
            windows.push(window);
        }
        (session, windows, pane)
    };
    let mut context = ExecutionContext::for_pane(&shared.inner.lock().engine.state, pane).unwrap();
    let (client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some("chooser".to_owned()),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(client, session).unwrap();
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("choose-tree", ["-w"]),
        )
        .unwrap();
    let cursor = |shared: &Shared| {
        let inner = shared.inner.lock();
        let chooser = inner.clients[&client].choose_tree.as_ref().unwrap();
        (chooser.rendered.selected, chooser.selected)
    };
    let row = shared.inner.lock().clients[&client]
        .choose_tree
        .as_ref()
        .unwrap()
        .rendered
        .items
        .iter()
        .position(|item| item.target == ChooseTreeTarget::Window(windows[1]))
        .and_then(|row| u32::try_from(row).ok())
        .unwrap();
    for action in [
        ChooseTreeAction::Select(row),
        ChooseTreeAction::KillCurrent,
        ChooseTreeAction::Key(tests::test_key(
            zz_terminal::KeyCode::Character('y'),
            zz_terminal::Modifiers::default(),
            Some("y"),
        )),
    ] {
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut context,
                InputMessage::ChooseTree { action },
            )
            .unwrap();
    }
    let mut tasks = std::mem::take(&mut *shared.pending_wait_queues.lock());
    assert_eq!(tasks.len(), 1);
    for task in &mut tasks {
        for _ in 0..100 {
            if matches!(task.run(false), wait_queue::Progress::Done) {
                break;
            }
        }
    }
    assert!(
        !shared
            .inner
            .lock()
            .engine
            .state
            .windows
            .contains_key(&windows[1])
    );
    shared.refresh_choose_trees();
    assert_eq!(
        cursor(&shared),
        (row, Some(ChooseTreeTarget::Window(windows[2])))
    );
}
