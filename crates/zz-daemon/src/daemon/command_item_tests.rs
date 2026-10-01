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
