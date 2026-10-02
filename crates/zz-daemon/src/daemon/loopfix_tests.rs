use super::*;

fn on_loop(scenario: impl FnOnce(&Arc<Shared>, &mut event_loop::EventLoop) + Send + 'static) {
    let (done, finished) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let shared = Arc::new(Shared::new(1));
        let mut event_loop = event_loop::EventLoop::empty(&shared).unwrap();
        assert!(shared.helpers.on_loop_thread());
        scenario(&shared, &mut event_loop);
        let _ = done.send(());
    });
    match finished.recv_timeout(Duration::from_secs(20)) {
        Ok(()) => handle.join().unwrap(),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(handle.join().unwrap_err())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("the scenario blocked the mux loop thread")
        }
    }
}

fn attached(shared: &Arc<Shared>) -> (ClientId, PaneId, ExecutionContext) {
    let context = {
        let mut inner = shared.inner.lock();
        let (_, _, pane) = inner.engine.state.create_session("loopfix").unwrap();
        ExecutionContext::for_pane(&inner.engine.state, pane).unwrap()
    };
    let (client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some("loopfix".to_owned()),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(client, context.session.unwrap()).unwrap();
    (client, context.pane.unwrap(), context)
}

fn press(shared: &Arc<Shared>, command: CommandInvocation) -> ExecutionContext {
    let (client, pane, mut context) = attached(shared);
    shared
        .execute_key_commands(
            client,
            ClientKind::Interactive,
            &mut context,
            pane,
            &[command],
            false,
        )
        .unwrap();
    context
}

fn environment(shared: &Shared, name: &str) -> Option<String> {
    shared.inner.lock().engine.global_environment_variable(name)
}

fn drive_until(
    shared: &Arc<Shared>,
    event_loop: &mut event_loop::EventLoop,
    ready: impl Fn() -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "handed-off loop work did not finish"
        );
        event_loop.shell_test_turn(shared);
    }
}

fn run_queued(
    shared: &Arc<Shared>,
    event_loop: &mut event_loop::EventLoop,
    context: &ExecutionContext,
    command: &CommandInvocation,
) -> CommandResponse {
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut task = wait_queue::CommandTask::new(
        shared,
        client,
        ClientKind::Command,
        context,
        1,
        command,
        false,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "queued command did not finish");
        if task.ready() {
            let progress = match task.run(true) {
                wait_queue::Progress::Worker => task.run(false),
                progress => progress,
            };
            if matches!(progress, wait_queue::Progress::Done) {
                break;
            }
        }
        event_loop.shell_test_turn(shared);
    }
    task.finish().0
}

fn select_mux_config(shared: &Shared, path: &Path) {
    *shared.mux_config_selection.lock() = (true, Some(vec![path.to_owned()]));
}

#[test]
fn a_key_bound_source_file_on_the_loop_runs_through_a_queued_task() {
    on_loop(|shared, event_loop| {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("bound.conf");
        fs::write(&config, "set-environment -g LOOPFIX_BOUND yes\n").unwrap();
        press(
            shared,
            CommandInvocation::new("source-file", [config.display().to_string()]),
        );
        drive_until(shared, event_loop, || {
            environment(shared, "LOOPFIX_BOUND").as_deref() == Some("yes")
        });
    });
}

#[test]
fn a_queued_reload_config_on_the_loop_parks_until_its_worker_replays() {
    on_loop(|shared, event_loop| {
        let directory = tempfile::tempdir().unwrap();
        let mux = directory.path().join("mux.conf");
        fs::write(&mux, "set-environment -g LOOPFIX_RELOAD loaded\n").unwrap();
        select_mux_config(shared, &mux);
        let (_, _, context) = attached(shared);
        let response = run_queued(
            shared,
            event_loop,
            &context,
            &CommandInvocation::new("reload-config", [] as [&str; 0]),
        );
        assert!(
            matches!(response, CommandResponse::Success { exit_code: 0, .. }),
            "{response:?}"
        );
        assert_eq!(
            environment(shared, "LOOPFIX_RELOAD").as_deref(),
            Some("loaded")
        );
    });
}

#[test]
fn a_key_bound_reload_config_on_the_loop_replays_on_a_worker() {
    on_loop(|shared, event_loop| {
        let directory = tempfile::tempdir().unwrap();
        let mux = directory.path().join("mux.conf");
        fs::write(&mux, "set-environment -g LOOPFIX_KEY_RELOAD loaded\n").unwrap();
        select_mux_config(shared, &mux);
        press(
            shared,
            CommandInvocation::new("reload-config", [] as [&str; 0]),
        );
        drive_until(shared, event_loop, || {
            environment(shared, "LOOPFIX_KEY_RELOAD").as_deref() == Some("loaded")
        });
    });
}

#[test]
fn a_queued_import_tmux_config_on_the_loop_reports_its_import() {
    on_loop(|shared, event_loop| {
        let directory = tempfile::tempdir().unwrap();
        let donor = directory.path().join("tmux.conf");
        fs::write(&donor, "set-environment -g LOOPFIX_IMPORT copied\n").unwrap();
        let mux = directory.path().join("mux.conf");
        *shared.zz_mux_config_path.lock() = Some(mux.clone());
        select_mux_config(shared, &mux);
        let (_, _, context) = attached(shared);
        let response = run_queued(
            shared,
            event_loop,
            &context,
            &CommandInvocation::new("import-tmux-config", [donor.display().to_string()]),
        );
        let CommandResponse::Success { output, .. } = &response else {
            panic!("{response:?}");
        };
        assert!(output.contains("Imported"), "{response:?}");
        assert!(fs::read_to_string(&mux).unwrap().contains("LOOPFIX_IMPORT"));
        assert_eq!(
            environment(shared, "LOOPFIX_IMPORT").as_deref(),
            Some("copied")
        );
    });
}

#[test]
fn a_key_bound_foreground_run_shell_parks_in_a_queued_task() {
    on_loop(|shared, event_loop| {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("ran");
        press(
            shared,
            CommandInvocation::new(
                "run-shell",
                [format!("printf ran > '{}'", marker.display())],
            ),
        );
        drive_until(shared, event_loop, || {
            fs::read_to_string(&marker).is_ok_and(|text| text == "ran")
        });
    });
}

#[test]
fn a_key_bound_shell_if_shell_parks_in_a_queued_task() {
    on_loop(|shared, event_loop| {
        press(
            shared,
            CommandInvocation::new(
                "if-shell",
                [
                    "true",
                    "set-environment -g LOOPFIX_IF yes",
                    "set-environment -g LOOPFIX_IF no",
                ],
            ),
        );
        drive_until(shared, event_loop, || {
            environment(shared, "LOOPFIX_IF").as_deref() == Some("yes")
        });
    });
}

#[test]
fn a_key_bound_save_buffer_parks_in_a_queued_task() {
    on_loop(|shared, event_loop| {
        let directory = tempfile::tempdir().unwrap();
        let saved = directory.path().join("saved");
        let (client, pane, mut context) = attached(shared);
        shared
            .execute_key_commands(
                client,
                ClientKind::Interactive,
                &mut context,
                pane,
                &[
                    CommandInvocation::new("set-buffer", ["-b", "loopfix", "kept"]),
                    CommandInvocation::new(
                        "save-buffer",
                        ["-b", "loopfix", &saved.display().to_string()],
                    ),
                ],
                false,
            )
            .unwrap();
        drive_until(shared, event_loop, || {
            fs::read_to_string(&saved).is_ok_and(|text| text == "kept")
        });
    });
}

#[test]
fn a_key_bound_command_prompt_waits_for_unsettled_history_in_a_queued_task() {
    on_loop(|shared, event_loop| {
        let directory = tempfile::tempdir().unwrap();
        let history = directory.path().join("history");
        fs::write(&history, "").unwrap();
        shared.prompt_history_source.lock().source = Some((history, 10));
        shared
            .prompt_history_settled
            .store(false, Ordering::Release);
        let (client, pane, mut context) = attached(shared);
        shared
            .execute_key_commands(
                client,
                ClientKind::Interactive,
                &mut context,
                pane,
                &[CommandInvocation::new(
                    "command-prompt",
                    ["-p", "loopfix", "display-message %%"],
                )],
                false,
            )
            .unwrap();
        drive_until(shared, event_loop, || {
            shared.prompt_history_settled.load(Ordering::Acquire)
                && shared
                    .inner
                    .lock()
                    .client(client)
                    .is_some_and(|c| c.command_prompt.is_some())
        });
    });
}

#[test]
fn a_key_bound_command_mode_run_shell_parks_in_a_queued_task() {
    on_loop(|shared, event_loop| {
        press(
            shared,
            CommandInvocation::new("run-shell", ["-C", "set-environment -g LOOPFIX_C yes"]),
        );
        drive_until(shared, event_loop, || {
            environment(shared, "LOOPFIX_C").as_deref() == Some("yes")
        });
    });
}

#[test]
fn a_handed_off_key_binding_keeps_its_command_order() {
    on_loop(|shared, event_loop| {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("ordered.conf");
        fs::write(
            &config,
            "set-environment -g LOOPFIX_ORDER first\nset-environment -g LOOPFIX_SOURCED yes\n",
        )
        .unwrap();
        let (client, pane, mut context) = attached(shared);
        shared
            .execute_key_commands(
                client,
                ClientKind::Interactive,
                &mut context,
                pane,
                &[
                    CommandInvocation::new("source-file", [config.display().to_string()]),
                    CommandInvocation::new("set-environment", ["-g", "LOOPFIX_ORDER", "second"]),
                ],
                false,
            )
            .unwrap();
        drive_until(shared, event_loop, || {
            environment(shared, "LOOPFIX_SOURCED").as_deref() == Some("yes")
                && environment(shared, "LOOPFIX_ORDER").as_deref() == Some("second")
        });
        for _ in 0..10 {
            event_loop.shell_test_turn(shared);
        }
        assert_eq!(
            environment(shared, "LOOPFIX_ORDER").as_deref(),
            Some("second")
        );
    });
}
