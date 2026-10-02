use super::*;

fn fixture() -> (Publisher, TerminalEvents, Arc<EventQueueState>) {
    let state = Arc::new(EventQueueState::new());
    let (event_tx, events) = terminal_event_channel(&state);
    let publisher = Publisher {
        event_tx,
        latest: Arc::new(RwLock::new(PublishedViewports::new(
            TerminalViewport::blank(1, 1, SessionStatus::Running),
        ))),
        state: Arc::clone(&state),
    };
    (publisher, events, state)
}

#[test]
fn deferred_events_preserve_four_reliable_slots_across_channel_transfer() {
    let (publisher, events, state) = fixture();
    let (sender, receiver) = crossbeam_channel::unbounded();
    for _ in 0..MAX_PENDING_RELIABLE_EVENTS {
        publisher
            .send_reliable(TerminalEvent::ClipboardSet {
                target: ClipboardTarget::Clipboard,
                text: "charged while transferred".to_owned(),
            })
            .unwrap();
        sender
            .send(events.recv_deferred_blocking().unwrap())
            .unwrap();
    }
    assert_eq!(events.receiver.len(), 0);
    assert_eq!(
        state.pending_reliable.load(Ordering::Acquire),
        MAX_PENDING_RELIABLE_EVENTS
    );
    let bytes = state.pending_reliable_bytes.load(Ordering::Acquire);
    assert!(bytes > 0);
    assert!(matches!(
        publisher.send_reliable(TerminalEvent::Bell),
        Err(WorkerError::EventBackpressure)
    ));
    receiver.recv().unwrap().into_event();
    publisher.send_reliable(TerminalEvent::Bell).unwrap();
    sender
        .send(events.recv_deferred_blocking().unwrap())
        .unwrap();
    assert_eq!(
        state.pending_reliable.load(Ordering::Acquire),
        MAX_PENDING_RELIABLE_EVENTS
    );
    drop(receiver);
    assert_eq!(state.pending_reliable.load(Ordering::Acquire), 0);
    assert_eq!(state.pending_reliable_bytes.load(Ordering::Acquire), 0);
}

#[test]
fn deferred_notification_coalesces_activity_until_loop_consumption() {
    let (publisher, events, state) = fixture();
    publisher.publish(TerminalViewport::blank(1, 1, SessionStatus::Running));
    let transferred = events.recv_deferred_blocking().unwrap();
    for _ in 0..8 {
        publisher.mark_output_activity();
        publisher.publish(TerminalViewport::blank(1, 1, SessionStatus::Running));
    }
    assert_eq!(events.receiver.len(), 0);
    assert!(state.notification_pending.load(Ordering::Acquire));
    assert!(matches!(
        transferred.into_event(),
        TerminalEvent::ViewportReady {
            output_activity: true
        }
    ));
    assert!(!state.notification_pending.load(Ordering::Acquire));
    publisher.publish(TerminalViewport::blank(1, 1, SessionStatus::Running));
    assert_eq!(events.receiver.len(), 1);
    drop(events.recv_deferred_blocking().unwrap());
    assert!(!state.notification_pending.load(Ordering::Acquire));
    assert!(!state.output_activity_pending.load(Ordering::Acquire));
}
