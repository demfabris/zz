use super::*;

fn fixture() -> (Arc<Shared>, ClientId, ExecutionContext) {
    let shared = Arc::new(Shared::new(9));
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "exit-e09", "exec sleep 30"]),
        )
        .unwrap();
    (shared, client, context)
}

fn task(
    shared: &Arc<Shared>,
    client: ClientId,
    context: &ExecutionContext,
    command: &CommandInvocation,
) -> wait_queue::CommandTask {
    wait_queue::CommandTask::new(
        shared,
        client,
        ClientKind::Command,
        context,
        9,
        command,
        false,
    )
    .unwrap()
}

#[test]
fn twenty_exit_waits_add_no_workers_or_polling_deadlines() {
    let (shared, client, context) = fixture();
    shared.terminal_requests.turn(&shared);
    let mut tasks = (0..20)
        .map(|_| {
            let mut task = task(
                &shared,
                client,
                &context,
                &CommandInvocation::new("wait-pane", ["--exit", "--timeout", "0"]),
            );
            assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
            assert!(!task.ready());
            task
        })
        .collect::<Vec<_>>();
    assert_eq!(
        shared.inner.lock().pane_exit_waits[&context.pane.unwrap()]
            .current
            .waiters(),
        20
    );
    assert!(shared.terminal_requests.turn(&shared).is_none());
    assert_eq!(shared.connection_threads.worker_count(), 0);
    Shared::wake_pane_exit_wait(&mut shared.inner.lock(), context.pane.unwrap(), 7);
    for task in &mut tasks {
        assert!(task.ready());
        assert!(matches!(task.run(true), wait_queue::Progress::Done));
    }
    for task in tasks {
        assert!(matches!(
            task.finish().0,
            CommandResponse::Success { exit_code: 7, .. }
        ));
    }
    shared.request_shutdown();
}

#[test]
fn client_loss_cancels_a_parked_exit_wait_once() {
    let (shared, client, context) = fixture();
    let mut task = task(
        &shared,
        client,
        &context,
        &CommandInvocation::new("wait-pane", ["--exit", "--timeout", "0"]),
    );
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    shared.unregister(client);
    assert!(task.ready());
    assert_eq!(
        shared.inner.lock().pane_exit_waits[&context.pane.unwrap()]
            .current
            .waiters(),
        0
    );
    Shared::wake_pane_exit_wait(&mut shared.inner.lock(), context.pane.unwrap(), 7);
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    assert!(matches!(task.finish().0, CommandResponse::Success { .. }));
    assert!(shared.terminal_requests.turn(&shared).is_none());
    shared.request_shutdown();
}

#[test]
fn pane_exit_notifications_broadcast_and_preserve_pending_split_status() {
    let (shared, client, context) = fixture();
    let pane = context.pane.unwrap();
    let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
    let completion = pane_exit::Completion::new(&terminal);
    shared.inner.lock().pane_exit_waits.insert(
        pane,
        PaneExitWait {
            current: Arc::clone(&completion),
            command_wait: Some(Arc::clone(&completion)),
        },
    );
    let mut subscribers = (0..3)
        .map(|_| {
            let mut task = task(
                &shared,
                client,
                &context,
                &CommandInvocation::new("wait-pane", ["--exit", "--timeout", "0"]),
            );
            assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
            task
        })
        .collect::<Vec<_>>();
    Shared::wake_pane_exit_wait(&mut shared.inner.lock(), pane, 7);
    Shared::wake_pane_exit_wait(&mut shared.inner.lock(), pane, 0);
    for task in &mut subscribers {
        assert!(task.ready());
        assert!(matches!(task.run(true), wait_queue::Progress::Done));
    }
    for task in subscribers {
        assert!(matches!(
            task.finish().0,
            CommandResponse::Success { exit_code: 7, .. }
        ));
    }
    let (result, status) = shared.wait_for_pane_command(
        client,
        ClientKind::Command,
        Ok(Execution {
            output: RawText::default(),
            effects: vec![MuxEffect::PaneWaitForExit { pane }],
        }),
    );
    assert!(result.is_ok());
    assert_eq!(status.unwrap().load(Ordering::Acquire), 7);
    assert!(!shared.inner.lock().pane_exit_waits.contains_key(&pane));
    shared.request_shutdown();
}

#[test]
fn wait_pane_exit_after_respawn_preserves_an_unconsumed_split_wait() {
    let (shared, client, context) = fixture();
    let pane = context.pane.unwrap();
    let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
    let mut entry = PaneExitWait::new(&terminal);
    entry.command_wait = Some(Arc::clone(&entry.current));
    shared.inner.lock().pane_exit_waits.insert(pane, entry);
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context.clone(),
            &CommandInvocation::new("respawn-pane", ["-k", "sleep 30"]),
        )
        .unwrap();
    let result = shared.execute(
        client,
        ClientKind::Command,
        &mut context.clone(),
        &CommandInvocation::new("wait-pane", ["--exit", "--timeout", "0.05"]),
    );
    assert!(matches!(
        result,
        Err(DaemonError::CommandExit { exit_code: 124, .. })
    ));
    let (result, status) = shared.wait_for_pane_command(
        client,
        ClientKind::Command,
        Ok(Execution {
            output: RawText::default(),
            effects: vec![MuxEffect::PaneWaitForExit { pane }],
        }),
    );
    assert!(result.is_ok());
    assert_eq!(status.unwrap().load(Ordering::Acquire), 0);
    shared.request_shutdown();
}

#[test]
fn client_loss_cancels_a_split_wait_without_taking_a_later_exit_status() {
    let (shared, client, context) = fixture();
    let mut task = task(
        &shared,
        client,
        &context,
        &CommandInvocation::new("split-window", ["-d", "-W", "exec sleep 30"]),
    );
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    let pane = *shared.inner.lock().pane_exit_waits.keys().next().unwrap();
    shared.unregister(client);
    assert!(task.ready());
    Shared::wake_pane_exit_wait(&mut shared.inner.lock(), pane, 9);
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    assert!(matches!(
        task.finish().0,
        CommandResponse::Success { exit_code: 0, .. }
    ));
    shared.request_shutdown();
}

#[test]
fn split_wait_preserves_signal_status_and_attached_client_retval() {
    for attached in [false, true] {
        let (shared, client, context) = fixture();
        if attached {
            shared.attach(client, context.session.unwrap()).unwrap();
        }
        let mut task = task(
            &shared,
            client,
            &context,
            &CommandInvocation::new("split-window", ["-d", "-W", "kill -TERM $$"]),
        );
        assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !task.ready() {
            assert!(Instant::now() < deadline);
            shared.terminal_requests.turn(&shared);
            thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(task.run(true), wait_queue::Progress::Done));
        assert!(
            matches!(task.finish().0, CommandResponse::Success { exit_code, .. } if exit_code == if attached { 0 } else { 143 })
        );
        shared.request_shutdown();
    }
}

#[test]
fn finishing_an_exit_wait_removes_its_timeout_deadline() {
    let (shared, client, context) = fixture();
    let mut task = task(
        &shared,
        client,
        &context,
        &CommandInvocation::new("wait-pane", ["--exit", "--timeout", "30"]),
    );
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    assert!(shared.terminal_requests.turn(&shared).is_some());
    Shared::wake_pane_exit_wait(&mut shared.inner.lock(), context.pane.unwrap(), 0);
    assert!(task.ready());
    assert!(shared.terminal_requests.turn(&shared).is_none());
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    assert!(matches!(
        task.finish().0,
        CommandResponse::Success { exit_code: 0, .. }
    ));
    shared.request_shutdown();
}
