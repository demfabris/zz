use super::*;

#[test]
fn twenty_delays_add_no_threads_and_only_their_deadlines() {
    if !crate::daemon::solo_tests::rerun_alone(
        "daemon::timers::delay_tests::twenty_delays_add_no_threads_and_only_their_deadlines",
    ) {
        return;
    }
    let shared = Arc::new(Shared::new(1));
    let poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(1)).unwrap());
    let mut timers = LoopTimers::new(&shared, &waker);
    shared.loop_active.store(true, Ordering::Release);
    let threads = zz_daemon_client::process_info::sample(std::process::id())
        .unwrap()
        .threads;
    assert_eq!(timers.next(Instant::now()), None);
    let start = Instant::now();
    let mut tasks = Vec::new();
    for index in 0..20 {
        let (client, _) =
            shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
        let command = if index % 2 == 0 {
            CommandInvocation::new("run-shell", ["-d", "30"])
        } else {
            CommandInvocation::new("run-shell", ["-Cd", "30", "display-message -p delayed"])
        };
        let mut task = wait_queue::CommandTask::new(
            &shared,
            client,
            ClientKind::Command,
            &ExecutionContext::default(),
            index,
            &command,
            false,
        )
        .unwrap();
        assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
        assert!(!task.ready());
        tasks.push(task);
    }
    timers.turn(&shared, &waker).unwrap();
    assert_eq!(timers.deadlines.entries.len(), 20);
    assert_eq!(timers.deadlines.callbacks.len(), 20);
    assert!(timers.deadlines.ready.is_empty());
    assert!(timers.next(start).unwrap() >= start + Duration::from_secs(30));
    assert!(timers.deadlines.entries.iter().all(|(key, (deadline, _))| {
        matches!(key, TimerKey::Callback(_)) && *deadline >= start + Duration::from_secs(30)
    }));
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        zz_daemon_client::process_info::sample(std::process::id())
            .unwrap()
            .threads
            <= threads
    );
    let now = Instant::now() + Duration::from_secs(31);
    while let Some(Expiry::Callback(id)) = timers.deadlines.pop_due(now) {
        timers.deadlines.run_callback(id);
    }
    for mut task in tasks {
        assert!(task.ready());
        for turn in 0..64 {
            match task.run(false) {
                wait_queue::Progress::Done => break,
                wait_queue::Progress::Ready => assert!(turn < 63),
                _ => panic!("delay resumed outside its queue"),
            }
        }
        let (response, _, _, _) = task.finish();
        assert!(matches!(response, CommandResponse::Success { .. }));
    }
    assert_eq!(timers.next(now), None);
}

#[test]
fn zero_delay_activates_ready_queue_without_a_deadline() {
    let shared = Arc::new(Shared::new(1));
    let poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(1)).unwrap());
    let mut timers = LoopTimers::new(&shared, &waker);
    shared.loop_active.store(true, Ordering::Release);
    let completed = Arc::new(AtomicBool::new(false));
    let callback = Arc::clone(&completed);
    shared
        .spawn_delay(Duration::ZERO, move || {
            callback.store(true, Ordering::Release);
        })
        .unwrap();
    assert!(!completed.load(Ordering::Acquire));
    let TimerInput::Callback { deadline, callback } =
        timers.inputs.as_ref().unwrap().try_recv().unwrap()
    else {
        panic!("callback input");
    };
    assert_eq!(deadline, None);
    timers.deadlines.callback(deadline, callback);
    assert!(timers.deadlines.order.is_empty());
    assert_eq!(timers.deadlines.ready.len(), 1);
    timers.turn(&shared, &waker).unwrap();
    assert!(completed.load(Ordering::Acquire));
    assert_eq!(timers.next(Instant::now()), None);
}
