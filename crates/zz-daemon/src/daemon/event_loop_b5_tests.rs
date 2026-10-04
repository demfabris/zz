use super::*;
use crate::daemon::tests::spawn_foreground_shell_job;

fn until(event_loop: &mut EventLoop, shared: &Arc<Shared>, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "shutdown phase did not complete");
        event_loop.poll_test_turn(shared, Duration::from_millis(2));
    }
}

#[test]
fn signal_shutdown_waits_for_a_foreground_job_that_ends_within_its_grace() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("finished");
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    event_loop.signals = Some(SignalPipes::new(&event_loop.poll).unwrap());
    let worker = spawn_foreground_shell_job(
        &shared,
        ClientId(301),
        format!(
            "sleep 0.2; printf finished > {}",
            crate::endpoint::shell_quote(marker.to_str().unwrap())
        ),
    );
    event_loop.request_signal_shutdown(&shared, Duration::from_secs(2));
    assert!(!shared.stopping.load(Ordering::Acquire));
    until(&mut event_loop, &shared, || {
        shared.shutdown_cleanup_complete.load(Ordering::Acquire)
            && shared.inner.lock().active_shell_jobs == 0
    });
    worker.join().unwrap().unwrap();
    assert_eq!(fs::read_to_string(marker).unwrap(), "finished");
}

#[test]
fn signal_shutdown_stops_a_foreground_job_that_outlives_its_grace() {
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    event_loop.signals = Some(SignalPipes::new(&event_loop.poll).unwrap());
    let worker = spawn_foreground_shell_job(&shared, ClientId(302), "sleep 30".to_owned());
    let grace = Duration::from_millis(200);
    let signalled = Instant::now();
    event_loop.request_signal_shutdown(&shared, grace);
    until(&mut event_loop, &shared, || {
        shared.shutdown_cleanup_complete.load(Ordering::Acquire)
            && shared.inner.lock().active_shell_jobs == 0
    });
    assert!(signalled.elapsed() >= grace);
    assert!(worker.join().unwrap().is_err());
    assert_eq!(shared.active_shutdown_blockers(), 0);
    assert!(shared.inner.lock().shell_jobs.is_empty());
}

#[test]
fn repeated_signal_stops_past_active_shutdown_blockers() {
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let blocker = ShutdownBlocker::acquire(&shared, false).unwrap();
    event_loop.request_signal_shutdown(&shared, Duration::from_secs(2));
    assert!(shared.shutdown_pending.load(Ordering::Acquire));
    assert!(!shared.stopping.load(Ordering::Acquire));
    let second = Instant::now();
    event_loop.request_signal_shutdown(&shared, Duration::from_secs(2));
    until(&mut event_loop, &shared, || {
        shared.shutdown_cleanup_complete.load(Ordering::Acquire)
            && shared.inner.lock().active_shell_jobs == 0
    });
    assert!(second.elapsed() < Duration::from_secs(1));
    assert!(ShutdownBlocker::acquire(&shared, false).is_none());
    drop(blocker);
    assert_eq!(shared.active_shutdown_blockers(), 0);
}

#[test]
fn response_admission_and_writer_drain_leave_socket_output_on_the_loop() {
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (token, mut peer, _, outbound) =
        shutdown_tests::registered(&mut event_loop, &shared, ClientKind::Command);
    let mut admission = ResponseAdmissionGuard::new(&shared).unwrap();
    shared.request_shutdown_without_hooks();
    event_loop.turn(&shared).unwrap();
    assert!(matches!(
        event_loop.shutdown_phase,
        ShutdownPhase::Admissions
    ));
    assert!(ResponseAdmissionGuard::new(&shared).is_none());
    io_tests::messages(&mut peer, &mut Inbound::default());
    let output = ProtocolMessage::TreeSync;
    assert!(outbound.enqueue_reliable(&output));
    event_loop.turn(&shared).unwrap();
    assert_eq!(
        io_tests::messages(&mut peer, &mut Inbound::default()),
        vec![output]
    );
    assert!(!event_loop.shutdown_completed());
    admission.finish();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !event_loop.shutdown_completed() {
        assert!(Instant::now() < deadline);
        event_loop.turn(&shared).unwrap();
        thread::sleep(Duration::from_millis(2));
    }
    assert!(!event_loop.connections.contains_key(&token));
    assert!(outbound.state.lock().writer_finished);
}

#[test]
fn child_notification_does_not_reap_a_worker_owned_child() {
    let poll = Poll::new().unwrap();
    let mut pipes = SignalPipes::new(&poll).unwrap();
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 17"])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while SignalPipes::drain(&mut pipes.child).unwrap() == 0 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(child.wait().unwrap().code(), Some(17));
}
