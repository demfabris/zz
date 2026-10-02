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

#[test]
fn notification_sink_catches_publication_before_registration() {
    let (publisher, events, _) = fixture();
    publisher.send_reliable(TerminalEvent::Bell).unwrap();
    let (sender, receiver) = crossbeam_channel::unbounded();
    events.install_notification_sink(move || {
        let _ = sender.send(());
    });
    receiver.try_recv().unwrap();
    assert!(matches!(events.try_recv().unwrap(), TerminalEvent::Bell));
}

#[test]
fn notification_sink_retains_enqueue_during_drain() {
    let (publisher, events, _) = fixture();
    let notified = Arc::new(AtomicBool::new(false));
    let pending = Arc::clone(&notified);
    let (sender, receiver) = crossbeam_channel::unbounded();
    events.install_notification_sink(move || {
        if !pending.swap(true, Ordering::AcqRel) {
            let _ = sender.send(());
        }
    });
    publisher.send_reliable(TerminalEvent::Bell).unwrap();
    receiver.try_recv().unwrap();
    notified.store(false, Ordering::Release);
    let first = events.try_recv_deferred().unwrap();
    publisher.send_reliable(TerminalEvent::Bell).unwrap();
    assert!(matches!(first.into_event(), TerminalEvent::Bell));
    assert!(notified.load(Ordering::Acquire));
    receiver.try_recv().unwrap();
    assert!(matches!(events.try_recv().unwrap(), TerminalEvent::Bell));
    assert!(events.try_recv().unwrap_err().is_empty());
}

#[test]
fn notification_sink_reports_closure_without_a_final_event() {
    let (publisher, events, _) = fixture();
    let (sender, receiver) = crossbeam_channel::unbounded();
    events.install_notification_sink(move || {
        let _ = sender.send(());
    });
    receiver.try_recv().unwrap();
    let other = publisher.clone();
    drop(publisher);
    assert!(receiver.try_recv().is_err());
    drop(other);
    receiver.try_recv().unwrap();
    assert!(matches!(events.try_recv_deferred(), Err(error) if error.is_closed()));
}

#[test]
fn notification_sink_catches_closure_before_registration() {
    let (publisher, events, _) = fixture();
    drop(publisher);
    let (sender, receiver) = crossbeam_channel::unbounded();
    events.install_notification_sink(move || {
        let _ = sender.send(());
    });
    receiver.try_recv().unwrap();
    assert!(matches!(events.try_recv_deferred(), Err(error) if error.is_closed()));
}

#[test]
fn concurrent_last_publishers_notify_closure_once() {
    let (publisher, events, _) = fixture();
    let (sender, receiver) = crossbeam_channel::unbounded();
    events.install_notification_sink(move || {
        let _ = sender.send(());
    });
    receiver.try_recv().unwrap();
    let other = publisher.clone();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let first_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        first_barrier.wait();
        drop(publisher);
    });
    let other_barrier = Arc::clone(&barrier);
    let second = thread::spawn(move || {
        other_barrier.wait();
        drop(other);
    });
    barrier.wait();
    first.join().unwrap();
    second.join().unwrap();
    receiver.try_recv().unwrap();
    assert!(receiver.try_recv().is_err());
    assert!(matches!(events.try_recv_deferred(), Err(error) if error.is_closed()));
}
