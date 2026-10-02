use super::*;

#[test]
fn a_departed_client_cannot_insert_a_file_waiter_after_its_initial_check() {
    let shared = Arc::new(Shared::new(1));
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    assert_eq!(
        shared.read_client(client, |c| c.and_then(|c| c.kind)),
        Some(ClientKind::Command)
    );
    shared.unregister(client);
    let result = shared.insert_client_file_waiter(ClientFileWaiter {
        client,
        path: PathBuf::from("-"),
        pane: None,
        complete: Box::new(|_, _| panic!("departed client must not retain a completion")),
    });
    assert!(result.is_err());
    assert!(shared.inner.lock().client_file_waiters.is_empty());
}

#[test]
fn unregister_removes_the_client_before_completing_its_file_waiters() {
    let shared = Arc::new(Shared::new(1));
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let (completed, result) = std::sync::mpsc::channel();
    shared
        .insert_client_file_waiter(ClientFileWaiter {
            client,
            path: PathBuf::from("-"),
            pane: None,
            complete: Box::new(move |shared, failure| {
                assert!(failure.is_err());
                assert!(!shared.inner.lock().clients.contains_key(&client));
                assert!(
                    shared
                        .insert_client_file_waiter(ClientFileWaiter {
                            client,
                            path: PathBuf::from("-"),
                            pane: None,
                            complete: Box::new(|_, _| panic!("reentrant waiter leaked")),
                        })
                        .is_err()
                );
                completed.send(()).unwrap();
            }),
        })
        .unwrap();
    shared.unregister(client);
    result.try_recv().unwrap();
    assert!(shared.inner.lock().client_file_waiters.is_empty());
}

pub(super) fn drive_loop_until(
    shared: &Arc<Shared>,
    event_loop: &mut event_loop::EventLoop,
    ready: impl Fn() -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "queued loop work did not finish");
        event_loop.shell_test_turn(shared);
    }
}

pub(super) fn execute_and_drive_loop(
    shared: &Arc<Shared>,
    event_loop: &mut event_loop::EventLoop,
    client: ClientId,
    kind: ClientKind,
    context: &mut ExecutionContext,
    command: &CommandInvocation,
) -> Result<Execution, DaemonError> {
    thread::scope(|scope| {
        let command = scope.spawn(|| shared.execute(client, kind, context, command));
        drive_loop_until(shared, event_loop, || command.is_finished());
        command.join().unwrap()
    })
}
