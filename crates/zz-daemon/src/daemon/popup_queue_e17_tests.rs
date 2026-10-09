use super::*;

fn workspace() -> (Arc<Shared>, ExecutionContext, event_loop::EventLoop) {
    let shared = Arc::new(Shared::new(7));
    let context = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("popups").unwrap();
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    let event_loop = event_loop::EventLoop::empty(&shared).unwrap();
    (shared, context, event_loop)
}

fn target(shared: &Arc<Shared>, context: &ExecutionContext, index: u64) -> ClientId {
    let (client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some(format!("popup-{index}")),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(client, context.session.unwrap()).unwrap();
    client
}

fn task(
    shared: &Arc<Shared>,
    context: &ExecutionContext,
    index: u64,
    flags: &[&str],
    command: &str,
) -> (ClientId, wait_queue::CommandTask) {
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut args = vec!["-c".to_owned(), format!("popup-{index}")];
    args.extend(flags.iter().map(|value| (*value).to_owned()));
    args.push(command.to_owned());
    let mut task = wait_queue::CommandTask::new(
        shared,
        client,
        ClientKind::Command,
        context,
        index,
        &CommandInvocation::new("display-popup", args),
        false,
    )
    .unwrap();
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    assert!(!task.ready());
    (client, task)
}

fn terminal(shared: &Shared, client: ClientId) -> Arc<TerminalSession> {
    Arc::clone(
        &shared.inner.lock().clients[&client]
            .popup
            .as_ref()
            .unwrap()
            .terminal,
    )
}

fn turn_until(
    shared: &Arc<Shared>,
    event_loop: &mut event_loop::EventLoop,
    condition: impl Fn() -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "popup wait timed out");
        event_loop.shell_test_turn(shared);
        thread::sleep(Duration::from_millis(2));
    }
}

fn finish(task: &mut wait_queue::CommandTask) {
    assert!(task.ready());
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
}

#[test]
fn twenty_popup_waits_add_zero_watcher_or_command_workers_beyond_shards() {
    if !super::solo_tests::rerun_alone(
        "daemon::popup_queue_e17_tests::twenty_popup_waits_add_zero_watcher_or_command_workers_beyond_shards",
    ) {
        return;
    }
    let (shared, context, mut event_loop) = workspace();
    let targets = (0..20)
        .map(|index| target(&shared, &context, index))
        .collect::<Vec<_>>();
    let shards = std::env::var("ZZ_PTY_SHARDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_else(|| {
            thread::available_parallelism()
                .map_or(1, std::num::NonZeroUsize::get)
                .min(4)
        });
    assert!(shards > 0);
    let mut warmup = Vec::new();
    for target in &targets {
        shared
            .execute(
                *target,
                ClientKind::Interactive,
                &mut context.clone(),
                &CommandInvocation::new("display-popup", ["sleep 30"]),
            )
            .unwrap();
        warmup.push(terminal(&shared, *target));
    }
    turn_until(&shared, &mut event_loop, || {
        warmup.iter().all(|terminal| {
            terminal.process_id().is_some()
                && matches!(
                    terminal.latest_viewport().status,
                    zz_terminal::SessionStatus::Running
                )
        })
    });
    assert_eq!(shared.connection_threads.worker_count(), 0);
    let before = zz_daemon_client::process_info::sample(std::process::id())
        .unwrap()
        .threads;
    for target in &targets {
        shared.close_popup(*target, true);
    }
    turn_until(&shared, &mut event_loop, || {
        warmup
            .iter()
            .all(|terminal| terminal.completion().is_some())
    });
    let mut tasks = (0..20)
        .map(|index| task(&shared, &context, index, &[], "sleep 30").1)
        .collect::<Vec<_>>();
    let terminals = targets
        .iter()
        .map(|client| terminal(&shared, *client))
        .collect::<Vec<_>>();
    turn_until(&shared, &mut event_loop, || {
        terminals.iter().all(|terminal| {
            terminal.process_id().is_some()
                && matches!(
                    terminal.latest_viewport().status,
                    zz_terminal::SessionStatus::Running
                )
        })
    });
    assert!(tasks.iter().all(|task| !task.ready()));
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(
        zz_daemon_client::process_info::sample(std::process::id())
            .unwrap()
            .threads
            <= before
    );
    for client in targets {
        shared.close_popup(client, true);
    }
    for task in &mut tasks {
        finish(task);
    }
    for task in tasks {
        assert!(matches!(
            task.finish().0,
            CommandResponse::Success { exit_code: 129, .. }
        ));
    }
    turn_until(&shared, &mut event_loop, || {
        terminals
            .iter()
            .all(|terminal| terminal.completion().is_some())
    });
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[test]
fn loop_popup_exit_and_retained_failure_keep_child_status() {
    for (flags, code, retained) in [
        (&["-E"][..], 0, false),
        (&["-E"][..], 3, false),
        (&["-EE"][..], 0, false),
        (&["-EE"][..], 3, true),
        (&[][..], 3, true),
    ] {
        let (shared, context, mut event_loop) = workspace();
        let target = target(&shared, &context, 0);
        let (_, mut task) = task(&shared, &context, 0, flags, &format!("exit {code}"));
        let terminal = terminal(&shared, target);
        turn_until(&shared, &mut event_loop, || {
            shared.read_client(target, |client| {
                client
                    .and_then(|client| client.popup.as_ref())
                    .is_none_or(|popup| popup.state.dead)
            })
        });
        assert_eq!(terminal.completion().unwrap().code, code);
        if retained {
            assert!(!task.ready());
            shared.close_popup(target, true);
        }
        finish(&mut task);
        assert!(
            matches!(task.finish().0, CommandResponse::Success { exit_code, .. } if exit_code == code as u8)
        );
        assert_eq!(shared.connection_threads.worker_count(), 0);
    }
}

#[test]
fn popup_cleanup_resumes_once_after_target_or_issuer_disconnect() {
    for cleanup in 0..4 {
        let (shared, context, mut event_loop) = workspace();
        let target = target(&shared, &context, 0);
        let (issuer, mut task) = task(&shared, &context, 0, &[], "sleep 30");
        let terminal = terminal(&shared, target);
        turn_until(&shared, &mut event_loop, || terminal.process_id().is_some());
        match cleanup {
            0 => {
                shared.close_popup(target, true);
                shared.close_popup(target, true);
            }
            1 => {
                shared.detach(target);
                shared.detach(target);
            }
            2 => {
                shared.unregister(target);
                shared.unregister(target);
            }
            _ => {
                shared.unregister(issuer);
                shared.unregister(issuer);
            }
        }
        finish(&mut task);
        assert!(matches!(
            task.finish().0,
            CommandResponse::Success { exit_code: 129, .. }
        ));
        shared.close_popup(target, true);
        turn_until(&shared, &mut event_loop, || terminal.completion().is_some());
    }
}

#[test]
fn popup_reply_completes_once_and_drop_defaults_to_dismissal() {
    let (shared, _, _) = workspace();
    let (wait, waiter) = PopupWait::new(&shared, ClientId(900));
    waiter.reply.try_send(3);
    waiter.reply.try_send(0);
    drop(waiter);
    assert!(wait.continuation.ready());
    assert!(!wait.continuation.complete());
    assert!(matches!(
        wait.finish(),
        Err(DaemonError::CommandExit { exit_code: 3, .. })
    ));
    let (wait, waiter) = PopupWait::new(&shared, ClientId(901));
    drop(waiter);
    assert!(wait.continuation.ready());
    assert!(matches!(
        wait.finish(),
        Err(DaemonError::CommandExit { exit_code: 129, .. })
    ));
}

#[test]
fn loop_popup_signal_status_is_reaped_by_the_terminal_shard() {
    let (shared, context, mut event_loop) = workspace();
    let target = target(&shared, &context, 0);
    let (_, mut task) = task(&shared, &context, 0, &["-E"], "kill -TERM $$");
    let terminal = terminal(&shared, target);
    turn_until(&shared, &mut event_loop, || task.ready());
    assert_eq!(terminal.completion().unwrap().signal, Some(15));
    finish(&mut task);
    assert!(matches!(
        task.finish().0,
        CommandResponse::Success { exit_code: 15, .. }
    ));
    assert_eq!(shared.connection_threads.worker_count(), 0);
}
