use super::*;

#[test]
fn copy_and_clock_deadlines_follow_consumers_with_fifty_ms_copy_cadence() {
    let shared = Arc::new(Shared::new(320));
    let now = Instant::now();
    let mut clients = ClientTimers::default();
    let mut deadlines = Deadlines::default();
    clients.sync(&shared, &mut deadlines, now);
    assert!(deadlines.entries.is_empty());
    let client = ClientId(1);
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "exec sleep 1000000"]),
        )
        .unwrap();
    let pane = context.pane.unwrap();
    {
        let mut inner = shared.inner.lock();
        inner.client_entry(client).copy_session = Some(CopySession {
            pane,
            observed: true,
            scroll_exit: false,
            kill: false,
            refresh: true,
            unseen: false,
            sourced: false,
            exiting: false,
        });
        inner.pane_modes.insert(pane, vec![PaneModeRequest::Clock]);
    }
    clients.sync(&shared, &mut deadlines, now);
    assert_eq!(
        deadlines.entries[&TimerKey::CopyRefresh].0,
        now + Duration::from_millis(50)
    );
    assert!(deadlines.get(TimerKey::ClockMode).is_none());
    {
        let mut inner = shared.inner.lock();
        inner.client_entry(client).subscriber = Some(OutboundMailbox::new());
        inner
            .attached
            .entry(context.session.unwrap())
            .or_default()
            .insert(client);
    }
    clients.sync(&shared, &mut deadlines, now);
    assert!(deadlines.get(TimerKey::ClockMode).is_some());
    clients.sync(&shared, &mut deadlines, now + Duration::from_millis(10));
    assert_eq!(
        deadlines.entries[&TimerKey::CopyRefresh].0,
        now + Duration::from_millis(50)
    );
    deadlines.remove(TimerKey::ClockMode);
    let tick = now + Duration::from_millis(50);
    assert!(matches!(deadlines.pop_due(tick), Some(Expiry::CopyRefresh)));
    shared.run_timer_expiry(Expiry::CopyRefresh, tick);
    clients.sync(&shared, &mut deadlines, tick);
    assert_eq!(
        deadlines.entries[&TimerKey::CopyRefresh].0,
        now + Duration::from_millis(100)
    );
    {
        let mut inner = shared.inner.lock();
        inner
            .client_entry(client)
            .copy_session
            .as_mut()
            .unwrap()
            .refresh = false;
        inner.pane_modes.clear();
    }
    clients.sync(&shared, &mut deadlines, tick);
    assert!(deadlines.get(TimerKey::CopyRefresh).is_none());
    assert!(deadlines.get(TimerKey::ClockMode).is_none());
    shared.request_shutdown();
}

#[test]
fn event_hook_commands_run_on_the_loop_without_starting_a_worker() {
    let shared = Arc::new(Shared::new(321));
    let mut context = ExecutionContext::default();
    {
        let mut inner = shared.inner.lock();
        for command in [
            CommandInvocation::new("new-session", ["-d"]),
            CommandInvocation::new("set-hook", ["-g", "window-renamed", "set -g @e20 yes"]),
        ] {
            inner.engine.execute(&mut context, &command).unwrap();
        }
    }
    let poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(1)).unwrap());
    let mut timers = LoopTimers::new(&shared, &waker);
    let event = PendingHookEvent::live_pane(
        "window-renamed",
        context.pane.unwrap(),
        &shared.inner.lock().engine,
    )
    .unwrap();
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    shared.enqueue_event_hooks(vec![event]);
    timers.turn(&shared, &waker).unwrap();
    assert!(timers.hooks.is_empty());
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert_eq!(
        shared
            .inner
            .lock()
            .engine
            .format_user_option("", "", "", "@e20"),
        Some("yes")
    );
}

#[test]
fn parked_shell_hooks_resume_owned_frames_in_enqueue_order_without_workers() {
    let shared = Arc::new(Shared::new(322));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(&mut context, &CommandInvocation::new("new-session", ["-d"]))
        .unwrap();
    let poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(1)).unwrap());
    let mut timers = LoopTimers::new(&shared, &waker);
    let _jobs = pipe_jobs::Driver::new(&shared);
    let directory = tempfile::tempdir().unwrap();
    let started = directory.path().join("started");
    struct Release(PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let release = Release(directory.path().join("release"));
    let body = format!(
        "run-shell 'touch {}; while [ ! -f {} ]; do sleep 0.01; done' ; set -g @first yes",
        started.display(),
        release.0.display()
    );
    for commands in [
        vec![CommandInvocation::new("run-shell", ["-C", &body])],
        vec![CommandInvocation::new(
            "set-option",
            ["-g", "@second", "yes"],
        )],
    ] {
        shared
            .timer_tx
            .send(TimerInput::MonitorHook {
                context: Box::new(context.clone()),
                commands: vec![commands],
                variables: BTreeMap::new(),
            })
            .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while !started.exists() {
        assert!(Instant::now() < deadline, "shell hook did not start");
        timers.turn(&shared, &waker).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        shared
            .inner
            .lock()
            .engine
            .format_user_option("", "", "", "@second"),
        None
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
    assert!(!timers.hooks.ready());
    drop(release);
    while !timers.hooks.is_empty() {
        assert!(Instant::now() < deadline, "shell hook did not resume");
        timers.turn(&shared, &waker).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
    let inner = shared.inner.lock();
    assert_eq!(
        inner.engine.format_user_option("", "", "", "@first"),
        Some("yes")
    );
    assert_eq!(
        inner.engine.format_user_option("", "", "", "@second"),
        Some("yes")
    );
    assert_eq!(inner.active_shell_jobs, 0);
    assert!(inner.shell_jobs.is_empty());
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[cfg(feature = "agent")]
#[test]
fn helper_peer_samples_and_their_hooks_apply_as_loop_queue_work() {
    let shared = Arc::new(Shared::new(323));
    let mut context = ExecutionContext::default();
    {
        let mut inner = shared.inner.lock();
        for command in [
            CommandInvocation::new("new-session", ["-d"]),
            CommandInvocation::new(
                "set-hook",
                ["-g", "after-set-option", "set -g @peer-hook yes"],
            ),
        ] {
            inner.engine.execute(&mut context, &command).unwrap();
        }
    }
    let poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(1)).unwrap());
    let mut timers = LoopTimers::new(&shared, &waker);
    shared.loop_active.store(true, Ordering::Release);
    let pane = context.pane.unwrap();
    shared.apply_helper_result(helpers::Result::Peers(Ok(vec![(
        pane,
        None,
        Some("working".to_owned()),
    )])));
    {
        let inner = shared.inner.lock();
        assert_eq!(inner.claude_peer_states[&pane], "working");
        assert_eq!(
            inner
                .engine
                .format_user_option(&pane.to_string(), "", "", "@agent_state"),
            None
        );
    }
    timers.turn(&shared, &waker).unwrap();
    let inner = shared.inner.lock();
    assert_eq!(
        inner
            .engine
            .format_user_option(&pane.to_string(), "", "", "@agent_state"),
        Some("working")
    );
    assert_eq!(
        inner.engine.format_user_option("", "", "", "@peer-hook"),
        Some("yes")
    );
    assert_eq!(shared.connection_threads.worker_count(), 0);
}

#[test]
fn mode_exits_cancel_copy_and_clock_deadlines_before_the_next_tick() {
    let shared = Arc::new(Shared::new(324));
    let client = ClientId(1);
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "exec sleep 1000000"]),
        )
        .unwrap();
    let pane = context.pane.unwrap();
    let terminal = {
        let mut inner = shared.inner.lock();
        inner.client_entry(client).subscriber = Some(OutboundMailbox::new());
        inner
            .attached
            .entry(context.session.unwrap())
            .or_default()
            .insert(client);
        inner.client_entry(client).copy_session = Some(CopySession {
            pane,
            observed: true,
            scroll_exit: false,
            kill: false,
            refresh: true,
            unseen: false,
            sourced: false,
            exiting: false,
        });
        inner.pane_modes.insert(pane, vec![PaneModeRequest::Clock]);
        Arc::clone(&inner.terminals[&pane])
    };
    let poll = mio::Poll::new().unwrap();
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(1)).unwrap());
    let mut timers = LoopTimers::new(&shared, &waker);
    assert!(timers.deadlines.get(TimerKey::CopyRefresh).is_some());
    assert!(timers.deadlines.get(TimerKey::ClockMode).is_some());
    assert!(shared.pane_mode_input(client, &mut context, pane, PaneModeInput::Key("x")));
    shared.publish_terminal_for_pane(
        pane,
        client,
        None,
        &terminal.latest_viewport(),
        &terminal,
        &mut PaneFrameFanout::new(),
    );
    timers.turn(&shared, &waker).unwrap();
    assert!(timers.deadlines.get(TimerKey::CopyRefresh).is_none());
    assert!(timers.deadlines.get(TimerKey::ClockMode).is_none());
    shared.request_shutdown();
}
