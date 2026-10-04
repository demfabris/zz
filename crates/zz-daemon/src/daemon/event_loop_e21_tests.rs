use super::*;

fn until(event_loop: &mut EventLoop, shared: &Arc<Shared>, done: impl Fn(&EventLoop) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(event_loop) {
        assert!(Instant::now() < deadline);
        event_loop.poll_test_turn(shared, Duration::from_millis(1));
    }
}

#[test]
fn shutdown_runs_close_hooks_before_freezing_admissions() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("closed");
    let shared = Arc::new(Shared::new(721));
    let mut context = ExecutionContext::default();
    for command in [
        CommandInvocation::new("new-session", ["-d", "-s", "e21", "sleep 1000000"]),
        CommandInvocation::new(
            "set-hook",
            [
                "-g",
                "session-closed",
                &format!("run-shell 'sleep 0.05; touch {}'", marker.display()),
            ],
        ),
    ] {
        shared
            .execute(ClientId(900), ClientKind::Command, &mut context, &command)
            .unwrap();
    }
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    event_loop.signals = Some(SignalPipes::new(&event_loop.poll).unwrap());
    shared.request_shutdown();
    event_loop.turn(&shared).unwrap();
    assert!(!shared.response_admissions.lock().frozen);
    until(&mut event_loop, &shared, EventLoop::shutdown_completed);
    assert!(marker.exists());
    assert!(shared.inner.lock().engine.state.sessions.is_empty());
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[test]
fn removal_releases_the_writer_before_the_owner_cleans_client_state() {
    let shared = Arc::new(Shared::new(722));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, _peer, client, outbound) =
        shutdown_tests::registered(&mut event_loop, &shared, ClientKind::Command);
    {
        let inner = shared.inner.lock();
        event_loop.remove(token, &shared);
        assert!(outbound.state.lock().writer_finished);
        assert!(inner.clients.contains_key(&client));
    }
    event_loop.turn(&shared).unwrap();
    assert!(!shared.inner.lock().clients.contains_key(&client));
    assert_eq!(shared.connection_threads.worker_count(), 0);
}
