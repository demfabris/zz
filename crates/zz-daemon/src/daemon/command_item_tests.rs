use super::*;

#[test]
fn item_park_survives_a_thread_change_and_reports_once() {
    let shared = Arc::new(Shared::new(1));
    let client = ClientId(7);
    let mailbox = OutboundMailbox::new();
    shared
        .client_writers
        .lock()
        .insert(client, Arc::clone(&mailbox));
    let item = shared.command_item(Some((client, 42)));
    thread::spawn(move || {
        item.report_command_queue_park();
        item.report_command_queue_park();
    })
    .join()
    .unwrap();
    let messages = tests::take_reliable_messages(&mailbox);
    assert_eq!(messages.len(), 1);
    assert!(matches!(
        messages[0],
        ProtocolMessage::CommandQueueParked { request_id: 42 }
    ));
    assert!(shared.command_item.is_none());
}

#[test]
fn items_on_one_thread_keep_depth_notifications_and_park_separate() {
    let shared = Arc::new(Shared::new(1));
    let first = shared.command_item(Some((ClientId(1), 10)));
    let second = shared.command_item(Some((ClientId(2), 20)));
    let _hold = first.defer_control_notifications();
    first
        .command_item
        .as_ref()
        .unwrap()
        .lock()
        .client_key_injection_depth = MAX_CLIENT_KEY_INJECTION_DEPTH;
    first.run_event_hooks(vec![PendingHookEvent::paste_buffer(
        "paste-buffer-changed",
        "item-buffer".to_owned(),
    )]);
    let first_context = first.command_item.as_ref().unwrap().lock();
    let second_context = second.command_item.as_ref().unwrap().lock();
    assert_eq!(
        first_context.client_key_injection_depth,
        MAX_CLIENT_KEY_INJECTION_DEPTH
    );
    assert_eq!(
        first_context
            .deferred_control_notifications
            .as_ref()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(second_context.client_key_injection_depth, 0);
    assert!(second_context.deferred_control_notifications.is_none());
    assert_eq!(first_context.park, Some((ClientId(1), 10)));
    assert_eq!(second_context.park, Some((ClientId(2), 20)));
}

#[test]
fn nested_execution_retains_the_item_and_workers_retain_the_owner() {
    let shared = Arc::new(Shared::new(1));
    let item = shared.command_item(Some((ClientId(1), 10)));
    assert!(Arc::ptr_eq(&item, &item.execution_item()));
    let owner = item.server_owner();
    let observer = Arc::downgrade(&owner);
    assert!(Arc::ptr_eq(&shared, &owner));
    drop(item);
    assert!(observer.upgrade().is_some());
}

#[test]
fn unwinding_discards_only_the_items_held_notifications() {
    let shared = Arc::new(Shared::new(1));
    let item = shared.command_item(None);
    let other = shared.command_item(None);
    let _other_hold = other.defer_control_notifications();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _hold = item.defer_control_notifications();
        panic!("abandon this command");
    }));
    assert!(result.is_err());
    assert!(
        item.command_item
            .as_ref()
            .unwrap()
            .lock()
            .deferred_control_notifications
            .is_none()
    );
    assert!(
        other
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .deferred_control_notifications
            .is_some()
    );
}

#[test]
fn interleaved_items_for_one_client_keep_captures_and_scopes_separate() {
    let shared = Arc::new(Shared::new(1));
    let client = ClientId(7);
    let first = shared.command_item(Some((client, 10)));
    let second = shared.command_item(Some((client, 20)));
    let first_capture = first.begin_control_command_event_capture(client);
    let second_capture = second.begin_control_command_event_capture(client);
    let first_hooks = ctrl::HookNotificationsOnlyScope::new(&first, true);
    let nested_hooks = ctrl::HookNotificationsOnlyScope::new(&first, false);
    assert!(
        first
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .hook_notifications_only
    );
    drop(nested_hooks);
    let first_keys = timers::KeyTablePublishHold::enter(&first);
    let first_focus = hook_events::InputFocusScope::open(&first, &mut shared.inner.lock());
    let second_focus = hook_events::InputFocusScope::open(&second, &mut shared.inner.lock());
    first.publish_control_command_guard(Some((client, 0)), "first".into(), false, false);
    second.publish_control_command_guard(Some((client, 0)), "second".into(), false, false);
    assert!(
        first
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .hook_notifications_only
    );
    assert!(
        !second
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .hook_notifications_only
    );
    assert!(timers::KeyTablePublishHold::active(&first));
    assert!(!timers::KeyTablePublishHold::active(&second));
    first.report_command_queue_park();
    let token = match first.command_item.as_ref().unwrap().lock().state() {
        cmdq::State::Waiting(token) => token,
        state => panic!("unexpected item state: {state:?}"),
    };
    assert_eq!(
        second.command_item.as_ref().unwrap().lock().state(),
        cmdq::State::Ready
    );
    drop(first_hooks);
    drop(first_keys);
    assert!(!timers::KeyTablePublishHold::active(&first));
    assert!(first.command_item.as_ref().unwrap().lock().resume(token));
    assert!(first_focus.close(&shared.inner.lock()).is_some());
    assert!(second_focus.close(&shared.inner.lock()).is_some());
    let second_events = second_capture.finish();
    let first_events = first_capture.finish();
    assert!(
        matches!(first_events.events.as_slice(), [EventPayload::ControlCommandGuard { output, .. }] if output == "first")
    );
    assert!(
        matches!(second_events.events.as_slice(), [EventPayload::ControlCommandGuard { output, .. }] if output == "second")
    );
    assert!(
        shared
            .inner
            .lock()
            .control_command_event_captures
            .is_empty()
    );
}

#[test]
fn duplicate_queue_completion_finishes_hooks_jobs_and_blockers_once() {
    let shared = Arc::new(Shared::new(1)).command_item(None);
    let count = Arc::new(AtomicU64::new(0));
    let job_count = Arc::clone(&count);
    let execution = shared.command_queue_execution(CommandExecutionState {
        draining: false,
        wait_yields: false,
        detached: false,
        deferred_shutdown: Cell::new(DeferredShutdown::None),
        deferred_control_exit: Cell::new(None),
        yielded: Cell::new(CommandQueueYield::None),
        yield_boundary: false,
        shutdown_blocker: RefCell::new(ShutdownBlocker::acquire(&shared, false)),
        pending_event_hooks: RefCell::new(Vec::new()),
        deferred_shell_jobs: RefCell::new(vec![Box::new(move |_| {
            job_count.fetch_add(1, Ordering::Relaxed);
        })]),
        callback_parse_failures: RefCell::new(Vec::new()),
        deferred_config_replay_issues: RefCell::new(Vec::new()),
        reported_failures: Cell::new(false),
        suppress_after_hooks: Cell::new(false),
        suppress_output: Cell::new(false),
    });
    assert_eq!(shared.active_shutdown_blockers(), 1);
    let _hold = shared.defer_control_notifications();
    execution
        .pending_event_hooks
        .borrow_mut()
        .push(PendingHookEvent::paste_buffer(
            "paste-buffer-changed",
            "one".to_owned(),
        ));
    shared.finish_command_queue_execution(&execution, None);
    shared.finish_command_queue_execution(&execution, None);
    assert_eq!(execution.item.state(), cmdq::State::Done);
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(shared.active_shutdown_blockers(), 0);
    assert_eq!(
        shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .deferred_control_notifications
            .as_ref()
            .unwrap()
            .len(),
        1
    );
}
