use super::*;

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
        .inner
        .lock()
        .engine
        .state
        .select_window(session, windows[0])
        .unwrap();
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
