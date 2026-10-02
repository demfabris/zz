use super::*;

pub(super) fn start_loop(shared: &Arc<Shared>) -> Result<(), DaemonError> {
    let _start = shared.shell_loop_start.lock();
    if shared.loop_active.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut event_loop = event_loop::EventLoop::empty(shared)?;
    shared.helpers.set_loop_thread(None);
    let owner = Arc::downgrade(shared);
    thread::Builder::new()
        .name("zz-test-shell-loop".into())
        .spawn(move || {
            if let Some(shared) = owner.upgrade() {
                shared.helpers.set_loop_thread(Some(thread::current().id()));
            }
            while let Some(shared) = owner.upgrade() {
                event_loop.shell_test_turn(&shared);
                drop(shared);
                thread::sleep(Duration::from_millis(2));
            }
        })
        .map(drop)
        .map_err(|error| DaemonError::Thread(error.to_string()))
}

#[test]
fn shell_job_closes_stdin_and_ignores_inherited_output_descriptors_after_exit() {
    let shared = Arc::new(Shared::new(1));
    let started = Instant::now();
    let result = shared.execute(
        ClientId(7),
        ClientKind::Command,
        &mut ExecutionContext::default(),
        &CommandInvocation::new(
            "run-shell",
            ["if read value; then exit 9; fi; trap '' HUP; sleep 30 & printf '%s detached' \"$!\""],
        ),
    ).expect("shell job");
    assert!(started.elapsed() < Duration::from_secs(2));
    let pid = result
        .output
        .strip_suffix(" detached")
        .unwrap()
        .parse::<i32>()
        .unwrap();
    if let Some(pid) = rustix::process::Pid::from_raw(pid) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    }
}

#[test]
fn twenty_shell_jobs_add_no_workers_and_admission_is_capped_at_256() {
    let shared = Arc::new(Shared::new(14));
    let mut event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let before = crate::process_info::sample(std::process::id())
        .unwrap()
        .threads;
    let mut tasks = Vec::new();
    for index in 0..20 {
        let (client, _) =
            shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
        let command = if index % 2 == 0 {
            CommandInvocation::new("run-shell", ["sleep 0.2; printf done"])
        } else {
            CommandInvocation::new("if-shell", ["sleep 0.2; true", "display-message -p done"])
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
        tasks.push(task);
    }
    assert_eq!(shared.inner.lock().active_shell_jobs, 20);
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        crate::process_info::sample(std::process::id())
            .unwrap()
            .threads
            <= before
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while tasks.iter().any(|task| !task.ready()) {
        assert!(Instant::now() < deadline);
        event_loop.pipe_test_turn(&shared);
        thread::sleep(Duration::from_millis(2));
    }
    for mut task in tasks {
        assert!(matches!(task.run(true), wait_queue::Progress::Done));
        let (response, _, _, _) = task.finish();
        assert!(matches!(response, CommandResponse::Success { output, .. } if output == "done"));
    }
    assert_eq!(shared.inner.lock().active_shell_jobs, 0);
    assert_eq!(shared.connection_threads.worker_count(), 0);
    let permits = (0..MAX_SHELL_JOBS)
        .map(|_| ShellJobPermit::acquire(&shared, false, false, None).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(MAX_SHELL_JOBS, 256);
    assert_eq!(shared.inner.lock().active_shell_jobs, 256);
    assert!(ShellJobPermit::acquire(&shared, false, false, None).is_none());
    let error = shared
        .execute(
            ClientId(257),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("run-shell", ["printf never"]),
        )
        .expect_err("job admission cap");
    assert!(
        matches!(error, DaemonError::Server(ServerError::InvalidCommand(message))
        if message == "failed to run command: printf never")
    );
    assert_eq!(shared.inner.lock().active_shell_jobs, 256);
    drop(permits);
    assert_eq!(shared.inner.lock().active_shell_jobs, 0);
}

#[test]
fn ready_background_jobs_use_ticket_order_without_waiting_for_slower_jobs() {
    let shared = Arc::new(Shared::new(14));
    let mut event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    let applied = Arc::new(Mutex::new(Vec::new()));
    let slow = shared.background_insertion_ticket();
    let first = shared.background_insertion_ticket();
    let second = shared.background_insertion_ticket();
    for ticket in [second, first] {
        let applied = Arc::clone(&applied);
        shared.apply_background_insertion(ticket, move || applied.lock().push(ticket));
    }
    assert!(applied.lock().is_empty());
    event_loop.pipe_test_turn(&shared);
    assert_eq!(*applied.lock(), vec![first, second]);
    let recorded = Arc::clone(&applied);
    shared.apply_background_insertion(slow, move || recorded.lock().push(slow));
    event_loop.pipe_test_turn(&shared);
    assert_eq!(*applied.lock(), vec![first, second, slow]);
}
