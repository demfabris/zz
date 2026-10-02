use super::*;

fn setup() -> (
    Arc<Shared>,
    watchers::LoopWatchers,
    ClientId,
    ExecutionContext,
) {
    let shared = Arc::new(Shared::new(1010));
    let poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(0)).unwrap());
    let watchers = watchers::LoopWatchers::new(&shared);
    shared.terminal_requests.install(waker);
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "e10", "sleep 30"]),
        )
        .unwrap();
    (shared, watchers, client, context)
}

#[test]
fn twenty_waiting_commands_add_zero_workers_and_zero_periodic_scan_timers() {
    let (shared, _watchers, client, context) = setup();
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    let mut tasks = (0..20)
        .map(|request_id| {
            let command = CommandInvocation::new(
                "wait-pane",
                ["--until", "e10-never-written", "--timeout", "0"],
            );
            let mut task = wait_queue::CommandTask::new(
                &shared,
                client,
                ClientKind::Command,
                &context,
                request_id,
                &command,
                false,
            )
            .unwrap();
            assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
            task
        })
        .collect::<Vec<_>>();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let next = shared.terminal_requests.turn(&shared);
        if next.is_none() && !shared.terminal_requests.pending() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..10 {
        assert_eq!(shared.terminal_requests.turn(&shared), None);
        assert!(!shared.terminal_requests.pending());
    }
    assert!(tasks.iter().all(|task| !task.ready()));
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
    shared.request_shutdown();
    assert!(shared.terminal_requests.pending());
    while tasks.iter().any(|task| !task.ready()) {
        shared.terminal_requests.turn(&shared);
        assert!(Instant::now() < deadline);
    }
    for task in &mut tasks {
        assert!(matches!(task.run(true), wait_queue::Progress::Done));
    }
}

#[test]
fn output_between_capture_and_registration_resumes_without_a_timer() {
    let (shared, _watchers, client, context) = setup();
    let pane = context.pane.unwrap();
    let item = shared.command_item(None);
    item.command_item.as_ref().unwrap().lock().loop_wait = true;
    let wait = terminal_requests::CommandWait::new(&item);
    let state = Arc::clone(&wait.state);
    let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
    let target = Target::new(&shared, client, pane, terminal, wait.start());
    let generation = target.generation();
    pane_changed(&mut shared.inner.lock(), pane);
    observe_output(&shared, target, generation, None, |_, target| {
        target.state.resolve(Ok(Execution::default()));
    });
    wait.finish(&item, Execution::default()).unwrap();
    assert_eq!(shared.terminal_requests.turn(&shared), None);
    assert!(state.continuation.ready());
    shared.request_shutdown();
}

#[test]
fn parked_marker_and_echo_scans_have_no_deadline_without_a_timeout() {
    let (shared, _watchers, client, context) = setup();
    let pane = context.pane.unwrap();
    let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
    for marker in [false, true] {
        let item = shared.command_item(None);
        item.command_item.as_ref().unwrap().lock().loop_wait = true;
        let wait = terminal_requests::CommandWait::new(&item);
        let state = Arc::clone(&wait.state);
        let target = Target::new(&item, client, pane, Arc::clone(&terminal), wait.start());
        if marker {
            poll_run(
                &item,
                target,
                Run {
                    parsed: parse_run_pane_args(&["--timeout".into(), "0".into(), "true".into()])
                        .unwrap(),
                    marker: "ZZRUN-e10".to_owned(),
                    started: Instant::now(),
                    output: String::new(),
                    collecting: false,
                },
            );
        } else {
            let generation = target.generation();
            poll_paste(
                &item,
                target,
                Paste {
                    sinks: vec![Arc::clone(&terminal)],
                    bytes: Arc::from([]),
                    tail: "e10-unseen-echo".to_owned(),
                    collapsed_before: 0,
                    generation,
                    started: Instant::now(),
                    timeout: Duration::ZERO,
                    enter: true,
                    finish: Box::new(|_, target, result| {
                        target.state.resolve(result.map(|()| Execution::default()));
                    }),
                },
            );
        }
        wait.finish(&item, Execution::default()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let next = shared.terminal_requests.turn(&shared);
            if next.is_none() && !shared.terminal_requests.pending() {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert!(!state.continuation.ready());
        for _ in 0..10 {
            assert_eq!(shared.terminal_requests.turn(&shared), None);
            assert!(!shared.terminal_requests.pending());
        }
    }
    shared.request_shutdown();
}

#[test]
fn old_echo_does_not_submit_a_new_paste_before_output_changes() {
    let (shared, _watchers, client, context) = setup();
    let pane = context.pane.unwrap();
    let terminal = Arc::new(TerminalSession::spawn_output_view(
        "old echo".to_owned(),
        "repeated payload".to_owned(),
    ));
    shared
        .inner
        .lock()
        .terminals_mut()
        .insert(pane, Arc::clone(&terminal));
    let item = shared.command_item(None);
    item.command_item.as_ref().unwrap().lock().loop_wait = true;
    let wait = terminal_requests::CommandWait::new(&item);
    let state = Arc::clone(&wait.state);
    let target = Target::new(&item, client, pane, terminal, wait.start());
    let generation = target.generation();
    poll_paste(
        &item,
        target,
        Paste {
            sinks: Vec::new(),
            bytes: Arc::from([]),
            tail: "repeated payload".to_owned(),
            collapsed_before: 0,
            generation,
            started: Instant::now(),
            timeout: Duration::ZERO,
            enter: false,
            finish: Box::new(|_, target, result| {
                target.state.resolve(result.map(|()| Execution::default()));
            }),
        },
    );
    wait.finish(&item, Execution::default()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let next = shared.terminal_requests.turn(&shared);
        if next.is_none() && !shared.terminal_requests.pending() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!state.continuation.ready());
    pane_changed(&mut shared.inner.lock(), pane);
    while !state.continuation.ready() {
        shared.terminal_requests.turn(&shared);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    shared.request_shutdown();
}
