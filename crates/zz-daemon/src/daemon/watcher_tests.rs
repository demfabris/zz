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
    terminal.fresh_viewport();
    let watchers = LoopWatchers::new(&shared);
    (shared, pane, terminal, watchers)
}

fn start(
    watchers: &mut LoopWatchers,
    shared: &Arc<Shared>,
    pane: PaneId,
    terminal: &Arc<TerminalSession>,
) {
    shared
        .watcher_tx
        .register(
            Watcher::terminal(pane, terminal, false),
            terminal.events(),
            VecDeque::new(),
        )
        .unwrap();
    let input = watchers
        .inputs
        .as_ref()
        .unwrap()
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(matches!(input, Input::Started(_)));
    watchers.input(shared, input);
}

#[test]
fn producer_notifies_while_server_state_is_locked() {
    let (shared, pane, terminal, mut watchers) = fixture();
    let locked = shared.inner.lock();
    start(&mut watchers, &shared, pane, &terminal);
    assert!(matches!(
        watchers
            .inputs
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        Input::Ready(1)
    ));
    drop(locked);
    watchers.input(&shared, Input::Ready(1));
    assert_eq!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "forwarded title"
    );
}

#[test]
fn publication_before_registration_is_drained_by_the_loop() {
    let (shared, pane, terminal, mut watchers) = fixture();
    assert!(terminal.diagnostics().viewport_notification_pending);
    start(&mut watchers, &shared, pane, &terminal);
    watchers.turn(&shared);
    assert_eq!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "forwarded title"
    );
    assert!(!terminal.diagnostics().viewport_notification_pending);
}

#[test]
fn queued_viewport_from_retired_terminal_cannot_rename_its_replacement() {
    let (shared, pane, terminal, mut watchers) = fixture();
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
    watchers.turn(&shared);
    assert_eq!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        before
    );
    assert!(!terminal.diagnostics().viewport_notification_pending);
    shared.inner.lock().terminals_mut().remove(&pane);
    drop(terminal);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !watchers.surfaces.is_empty() {
        watchers.turn(&shared);
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}

#[test]
fn worker_completion_admits_queued_final_frame_before_stream_closure() {
    let (shared, pane, terminal, mut watchers) = fixture();
    start(&mut watchers, &shared, pane, &terminal);
    watchers.surfaces.get_mut(&1).unwrap().busy = true;
    watchers.turn(&shared);

    assert!(terminal.diagnostics().viewport_notification_pending);
    assert_ne!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "forwarded title"
    );
    watchers.input(&shared, Input::Completed(1));
    watchers.turn(&shared);
    assert_eq!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "forwarded title"
    );
    assert!(!terminal.diagnostics().viewport_notification_pending);
    shared.inner.lock().terminals_mut().remove(&pane);
    drop(terminal);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !watchers.surfaces.is_empty() {
        watchers.turn(&shared);
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}

#[test]
fn repeated_readiness_keeps_one_entry_per_surface() {
    let (shared, pane, terminal, mut watchers) = fixture();
    start(&mut watchers, &shared, pane, &terminal);
    let watcher = watchers.surfaces.get(&1).unwrap();
    for _ in 0..1024 {
        LoopWatchers::schedule(&shared, watcher);
    }
    assert_eq!(watchers.inputs.as_ref().unwrap().len(), 1);
    let input = watchers.inputs.as_ref().unwrap().try_recv().unwrap();
    assert!(matches!(input, Input::Ready(1)));
    watchers.input(&shared, input);
    assert!(
        !watchers
            .surfaces
            .get(&1)
            .unwrap()
            .notified
            .load(Ordering::Acquire)
    );
}

#[cfg(unix)]
#[test]
fn drained_turn_rearms_the_coalesced_loop_wake() {
    let shared = Arc::new(Shared::new(1));
    let mut watchers = LoopWatchers::new(&shared);
    let mut poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(1)).unwrap());
    shared.watcher_tx.wake.install(waker);
    let mut events = mio::Events::with_capacity(8);
    for _ in 0..2 {
        shared.watcher_tx.send(Input::Completed(1));
        for _ in 0..1024 {
            shared.watcher_tx.notify_loop();
        }
        assert_eq!(watchers.inputs.as_ref().unwrap().len(), 1);
        poll.poll(&mut events, Some(Duration::from_secs(1)))
            .unwrap();
        assert!(!events.is_empty());
        watchers.turn(&shared);
        assert!(!shared.watcher_tx.pending_wake.load(Ordering::Acquire));
        poll.poll(&mut events, Some(Duration::from_millis(10)))
            .unwrap();
        assert!(events.is_empty());
    }
}

#[test]
fn closure_without_a_final_event_removes_the_loop_receiver() {
    let (shared, pane, terminal, mut watchers) = fixture();
    start(&mut watchers, &shared, pane, &terminal);
    watchers.turn(&shared);
    shared.inner.lock().terminals_mut().remove(&pane);
    drop(terminal);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !watchers.surfaces.is_empty() {
        watchers.turn(&shared);
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}
