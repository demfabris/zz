use super::*;

fn fixture() -> (Arc<Shared>, PaneId, Arc<TerminalSession>, LoopWatchers) {
    let shared = Arc::new(Shared::new(1));
    let pane = shared
        .inner
        .lock()
        .engine
        .state
        .create_session("watcher-loop")
        .unwrap()
        .2;
    let terminal = Arc::new(TerminalSession::spawn_output_view(
        "forwarded title".to_owned(),
        String::new(),
    ));
    shared
        .inner
        .lock()
        .terminals_mut()
        .insert(pane, Arc::clone(&terminal));
    terminal.attach_view(TerminalViewId(1));
    let watchers = LoopWatchers::new(&shared);
    (shared, pane, terminal, watchers)
}

fn start(
    watchers: &mut LoopWatchers,
    shared: &Arc<Shared>,
    pane: PaneId,
    terminal: &Arc<TerminalSession>,
) {
    let mut watcher = Watcher::terminal(pane, terminal, false);
    watcher.id = 1;
    watchers
        .input(shared, Input::Started(Box::new(watcher)))
        .unwrap();
}

#[test]
fn relay_transfers_events_while_server_state_is_locked() {
    let (shared, pane, terminal, watchers) = fixture();
    let inputs = watchers.inputs.as_ref().unwrap().clone();
    let sender = shared.watcher_tx.clone();
    let watcher = Watcher::terminal(pane, &terminal, false);
    let events = terminal.events();
    let locked = shared.inner.lock();
    let relay = thread::spawn(move || sender.relay(watcher, &events, VecDeque::new()));
    assert!(matches!(
        inputs.recv_timeout(Duration::from_secs(2)).unwrap(),
        Input::Started(_)
    ));
    let event = inputs.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(event, Input::Event(_, _)));
    drop(locked);
    drop(event);
    shared.inner.lock().terminals_mut().remove(&pane);
    drop(terminal);
    relay.join().unwrap();
}

#[test]
fn queued_viewport_from_retired_terminal_cannot_rename_its_replacement() {
    let (shared, pane, terminal, mut watchers) = fixture();
    let event = terminal.events().recv_deferred_blocking().unwrap();
    start(&mut watchers, &shared, pane, &terminal);
    let replacement = Arc::new(TerminalSession::spawn_output_view(
        "replacement".to_owned(),
        String::new(),
    ));
    terminal.retire();
    shared
        .inner
        .lock()
        .terminals_mut()
        .insert(pane, replacement);
    let before = shared
        .inner
        .lock()
        .engine
        .state
        .pane(pane)
        .unwrap()
        .title
        .clone();
    watchers.input(&shared, Input::Event(1, event)).unwrap();
    assert_eq!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        before
    );
    assert!(!terminal.diagnostics().viewport_notification_pending);
    watchers.input(&shared, Input::Closed(1)).unwrap();
    assert!(watchers.surfaces.is_empty());
}

#[test]
fn worker_completion_admits_queued_final_frame_before_stream_closure() {
    let (shared, pane, terminal, mut watchers) = fixture();
    let event = terminal.events().recv_deferred_blocking().unwrap();
    start(&mut watchers, &shared, pane, &terminal);
    watchers.surfaces.get_mut(&1).unwrap().busy = true;
    watchers.input(&shared, Input::Event(1, event)).unwrap();
    watchers.input(&shared, Input::Closed(1)).unwrap();
    assert!(terminal.diagnostics().viewport_notification_pending);
    assert_ne!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "forwarded title"
    );
    watchers.input(&shared, Input::Completed(1)).unwrap();
    assert_eq!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "forwarded title"
    );
    assert!(!terminal.diagnostics().viewport_notification_pending);
    while !watchers.surfaces.is_empty() {
        let completion = watchers
            .inputs
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        watchers.input(&shared, completion).unwrap();
    }
    assert!(watchers.surfaces.is_empty());
}
