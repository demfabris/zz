use super::super::*;

fn hold_helpers(shared: &Arc<Shared>) -> Vec<mpsc::Sender<()>> {
    let (started, ready) = mpsc::sync_channel(2);
    let mut releases = Vec::new();
    for _ in 0..2 {
        let (release, wait) = mpsc::channel();
        releases.push(release);
        shared
            .helpers
            .submit(super::Task::Hold(wait, started.clone()))
            .unwrap();
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    releases
}

#[cfg(unix)]
#[test]
fn busy_helpers_park_history_without_stopping_another_client() {
    let shared = Arc::new(Shared::new(7));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history");
    fs::write(&path, "command:from-file\nsearch:needle\n").unwrap();
    shared.prompt_history_source.lock().source = Some((path, 100));
    shared
        .prompt_history_settled
        .store(false, Ordering::Release);
    let context = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("helperwait").unwrap();
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    let (target, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some("helperwait-target".into()),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(target, context.session.unwrap()).unwrap();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let releases = hold_helpers(&shared);
    let command = CommandInvocation::new("command-prompt", ["-b", "-t", "helperwait-target"]);
    let mut prompt = wait_queue::CommandTask::new(
        &shared,
        ClientId(21),
        ClientKind::Command,
        &context,
        1,
        &command,
        false,
    )
    .unwrap();
    let start = Instant::now();
    assert!(matches!(prompt.run(true), wait_queue::Progress::Waiting));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!prompt.ready());
    assert!(
        shared
            .inner
            .lock()
            .client(target)
            .unwrap()
            .command_prompt
            .is_none()
    );
    let mut display = wait_queue::CommandTask::new(
        &shared,
        ClientId(22),
        ClientKind::Command,
        &context,
        2,
        &CommandInvocation::new("display-message", ["-p", "loop-alive"]),
        false,
    )
    .unwrap();
    assert!(matches!(display.run(true), wait_queue::Progress::Done));
    assert!(
        matches!(display.finish().0, CommandResponse::Success { output, .. } if output == "loop-alive")
    );
    assert_eq!(shared.helpers.state.queue.lock().busy, 2);
    for release in releases {
        release.send(()).unwrap();
    }
    let result = shared
        .helpers
        .results
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    shared.apply_helper_result(result);
    assert!(prompt.ready());
    assert!(matches!(prompt.run(true), wait_queue::Progress::Done));
    assert!(matches!(prompt.finish().0, CommandResponse::Success { .. }));
    assert!(
        shared
            .inner
            .lock()
            .client(target)
            .unwrap()
            .command_prompt
            .is_some()
    );
    assert_eq!(shared.inner.lock().command_history, ["from-file"]);
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[cfg(unix)]
#[test]
fn synchronous_file_helpers_refuse_the_loop_thread() {
    let shared = Arc::new(Shared::new(7));
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let releases = hold_helpers(&shared);
    assert_eq!(
        shared
            .helpers
            .read(Path::new("/tmp/helperwait-missing"))
            .unwrap_err()
            .kind(),
        ErrorKind::WouldBlock
    );
    assert!(
        matches!(shared.helpers.import_source(Path::new("/tmp/helperwait-missing")), Err(DaemonError::Io(error)) if error.kind() == ErrorKind::WouldBlock)
    );
    #[cfg(feature = "agent")]
    shared.sync_claude_peer_states();
    assert_eq!(shared.helpers.state.queue.lock().busy, 2);
    drop(releases);
}

#[cfg(unix)]
#[test]
fn busy_helpers_preserve_record_clear_order_and_allow_a_loaded_prompt() {
    let shared = Arc::new(Shared::new(7));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history");
    let context = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("helperwait").unwrap();
        inner
            .engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new(
                    "set-option",
                    ["-s", "history-file", path.to_str().unwrap()],
                ),
            )
            .unwrap();
        inner.search_history.push("needle".into());
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    let (target, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some("helperwait-target".into()),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(target, context.session.unwrap()).unwrap();
    let _event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let releases = hold_helpers(&shared);
    shared.record_prompt_history(CommandPromptType::Command, "before-clear");
    let mut clear = wait_queue::CommandTask::new(
        &shared,
        ClientId(21),
        ClientKind::Command,
        &context,
        1,
        &CommandInvocation::new("clear-prompt-history", ["-T", "command"]),
        false,
    )
    .unwrap();
    assert!(matches!(clear.run(true), wait_queue::Progress::Waiting));
    shared.record_prompt_history(CommandPromptType::Command, "after-clear");
    let mut prompt = wait_queue::CommandTask::new(
        &shared,
        ClientId(22),
        ClientKind::Command,
        &context,
        2,
        &CommandInvocation::new("command-prompt", ["-b", "-t", "helperwait-target"]),
        false,
    )
    .unwrap();
    assert!(matches!(prompt.run(true), wait_queue::Progress::Done));
    assert!(matches!(prompt.finish().0, CommandResponse::Success { .. }));
    let display = shared
        .execute(
            ClientId(23),
            ClientKind::Command,
            &mut context.clone(),
            &CommandInvocation::new("display-message", ["-p", "loop-alive"]),
        )
        .unwrap();
    assert_eq!(display.output, "loop-alive");
    assert_eq!(shared.helpers.state.queue.lock().busy, 2);
    for release in releases {
        release.send(()).unwrap();
    }
    for _ in 0..3 {
        let result = shared
            .helpers
            .results
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        shared.apply_helper_result(result);
    }
    assert!(clear.ready());
    assert!(matches!(clear.run(true), wait_queue::Progress::Done));
    assert!(matches!(clear.finish().0, CommandResponse::Success { .. }));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "command:after-clear\nsearch:needle\n"
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[cfg(unix)]
#[test]
fn loop_completion_hooks_park_config_reads_on_execution_workers() {
    let shared = Arc::new(Shared::new(7));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mux.conf");
    fs::write(&path, "set -g @helper-hook-complete yes\n").unwrap();
    *shared.mux_config_selection.lock() = (true, Some(vec![path]));
    shared
        .execute(
            ClientId(21),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("set-hook", ["-g", "after-load-buffer", "reload-config"]),
        )
        .unwrap();
    let poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(1)).unwrap());
    shared.helpers.install(Arc::clone(&waker));
    shared.loop_active.store(true, Ordering::Release);
    let mut timers = timers::LoopTimers::new(&shared, &waker);
    let releases = hold_helpers(&shared);
    shared.run_event_hooks(vec![PendingHookEvent {
        name: "after-load-buffer",
        context: ExecutionContext::default(),
        variables: BTreeMap::new(),
        exclude_client: None,
        control_notified: false,
    }]);
    let start = Instant::now();
    timers.turn(&shared, &waker).unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!timers.hooks.is_empty());
    let display = shared
        .execute(
            ClientId(22),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("display-message", ["-p", "loop-alive"]),
        )
        .unwrap();
    assert_eq!(display.output, "loop-alive");
    assert_eq!(shared.helpers.state.queue.lock().busy, 2);
    drop(releases);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !timers.hooks.is_empty() {
        assert!(Instant::now() < deadline);
        timers.turn(&shared, &waker).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
    let result = shared
        .execute(
            ClientId(22),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("show-options", ["-gqv", "@helper-hook-complete"]),
        )
        .unwrap();
    assert_eq!(result.output, "yes");
    let deadline = Instant::now() + Duration::from_secs(2);
    while shared.connection_threads.worker_count() != 0 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}
