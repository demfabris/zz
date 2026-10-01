use super::*;
use mio::{Poll, Token, Waker};

fn loop_timers(shared: &Shared) -> (Poll, Arc<Waker>, LoopTimers) {
    let poll = Poll::new().unwrap();
    let waker = Arc::new(Waker::new(poll.registry(), Token(1)).unwrap());
    let timers = LoopTimers::new(shared, &waker);
    (poll, waker, timers)
}

#[test]
fn replacement_and_equal_deadline_order_keep_one_entry_per_key() {
    let now = Instant::now();
    let client = ClientId(1);
    let mut deadlines = Deadlines::default();
    deadlines.insert(TimerKey::Rename, now, Expiry::Rename);
    deadlines.insert(TimerKey::PublishFlush, now, Expiry::PublishFlush);
    deadlines.insert(TimerKey::KeyTable(client), now, Expiry::KeyTable(client));
    deadlines.insert(
        TimerKey::KeyTable(client),
        now + Duration::from_secs(1),
        Expiry::KeyTable(client),
    );
    assert_eq!(deadlines.order.len(), 3);
    assert!(matches!(deadlines.pop_due(now), Some(Expiry::Rename)));
    assert!(matches!(deadlines.pop_due(now), Some(Expiry::PublishFlush)));
    assert!(deadlines.pop_due(now).is_none());
    assert!(
        matches!(deadlines.pop_due(now + Duration::from_secs(1)), Some(Expiry::KeyTable(id)) if id == client)
    );
    assert!(deadlines.next().is_none());
    assert!(deadlines.entries.is_empty());
}

#[test]
fn rename_and_publication_keep_the_earliest_deadline() {
    let shared = Shared::new(1);
    let now = Instant::now();
    let mut deadlines = Deadlines::default();
    for deadline in [
        now + Duration::from_secs(2),
        now,
        now + Duration::from_secs(1),
    ] {
        shared.schedule_timer(
            &mut deadlines,
            &TimerInput::Timer(TimerCommand::Rename(deadline)),
        );
        shared.schedule_timer(
            &mut deadlines,
            &TimerInput::Timer(TimerCommand::PublishFlush(deadline)),
        );
    }
    assert_eq!(deadlines.order.len(), 2);
    assert_eq!(deadlines.next(), Some(now));
    assert!(matches!(deadlines.pop_due(now), Some(Expiry::Rename)));
    assert!(matches!(deadlines.pop_due(now), Some(Expiry::PublishFlush)));
    assert!(deadlines.next().is_none());
}

#[test]
fn stale_cancellations_leave_newer_display_silence_and_message_deadlines() {
    let shared = Shared::new(1);
    let client = ClientId(1);
    let window = WindowId(1);
    let now = Instant::now();
    let display = DisplayPanesDeadline {
        client,
        token: 2,
        deadline: now,
    };
    let silence = SilenceDeadline {
        window,
        token: 2,
        deadline: now,
    };
    let message = ClientMessageDeadline {
        client,
        token: 2,
        deadline: now,
    };
    let mut deadlines = Deadlines::default();
    deadlines.insert(
        TimerKey::DisplayPanes(client),
        now,
        Expiry::DisplayPanes(display),
    );
    deadlines.insert(TimerKey::Silence(window), now, Expiry::Silence(silence));
    deadlines.insert(
        TimerKey::ClientMessage(client),
        now,
        Expiry::ClientMessage(message),
    );
    for token in [1, 2] {
        for input in [
            TimerInput::DisplayPanes(DisplayPanesDeadlineCommand::Cancel { client, token }),
            TimerInput::Silence(SilenceDeadlineCommand::Cancel { window, token }),
            TimerInput::ClientMessage(ClientMessageDeadlineCommand::Cancel { client, token }),
        ] {
            shared.schedule_timer(&mut deadlines, &input);
        }
        assert_eq!(deadlines.order.len(), if token == 1 { 3 } else { 0 });
    }
}

#[test]
fn stale_schedules_and_key_table_cancellation_leave_current_deadlines() {
    let shared = Shared::new(1);
    let client = ClientId(1);
    let window = WindowId(1);
    let now = Instant::now();
    let live = now + Duration::from_secs(1);
    {
        let mut inner = shared.inner.lock();
        inner.client_entry(client).key_table_deadline = Some(live);
        inner.client_entry(client).message = Some(ActiveClientMessage {
            token: 2,
            deadline: Some(live),
            freeze: false,
        });
        inner.silence_deadlines.insert(
            window,
            SilenceDeadline {
                window,
                token: 2,
                deadline: live,
            },
        );
    }
    let mut deadlines = Deadlines::default();
    for input in [
        TimerInput::KeyTable(KeyTableDeadlineCommand::Schedule(client, Some(live))),
        TimerInput::ClientMessage(ClientMessageDeadlineCommand::Schedule(
            ClientMessageDeadline {
                client,
                token: 2,
                deadline: live,
            },
        )),
        TimerInput::Silence(SilenceDeadlineCommand::Schedule(SilenceDeadline {
            window,
            token: 2,
            deadline: live,
        })),
        TimerInput::KeyTable(KeyTableDeadlineCommand::Schedule(client, None)),
        TimerInput::KeyTable(KeyTableDeadlineCommand::Schedule(client, Some(now))),
        TimerInput::ClientMessage(ClientMessageDeadlineCommand::Schedule(
            ClientMessageDeadline {
                client,
                token: 1,
                deadline: now,
            },
        )),
        TimerInput::Silence(SilenceDeadlineCommand::Schedule(SilenceDeadline {
            window,
            token: 1,
            deadline: now,
        })),
    ] {
        shared.schedule_timer(&mut deadlines, &input);
    }
    assert_eq!(deadlines.order.len(), 3);
    assert_eq!(deadlines.next(), Some(live));
    assert!(deadlines.pop_due(now).is_none());
    shared.inner.lock().client_entry(client).key_table_deadline = None;
    shared.schedule_timer(
        &mut deadlines,
        &TimerInput::KeyTable(KeyTableDeadlineCommand::Schedule(client, None)),
    );
    assert_eq!(deadlines.order.len(), 2);
}

#[test]
fn scheduling_wakes_an_empty_poll_after_enqueueing() {
    let shared = Arc::new(Shared::new(1));
    let (mut poll, waker, mut timers) = loop_timers(&shared);
    assert!(timers.next(Instant::now()).is_none());
    let producer = Arc::clone(&shared);
    let started = Instant::now();
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        producer
            .timer_tx
            .send(TimerInput::Timer(TimerCommand::Rename(
                Instant::now() + Duration::from_secs(1),
            )))
            .unwrap();
    });
    let mut events = mio::Events::with_capacity(8);
    poll.poll(&mut events, Some(Duration::from_secs(2)))
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(events.iter().any(|event| event.token() == Token(1)));
    assert_eq!(timers.inputs.as_ref().unwrap().len(), 1);
    timers.turn(&shared, &waker).unwrap();
    assert!(timers.next(Instant::now()).is_some());
    thread.join().unwrap();
}

#[test]
fn scheduling_and_expiry_bursts_leave_work_for_the_next_turn() {
    let shared = Arc::new(Shared::new(1));
    let (_poll, waker, mut timers) = loop_timers(&shared);
    let now = Instant::now();
    let future = now + Duration::from_secs(1);
    for _ in 0..=TIMER_INPUT_BURST {
        shared
            .timer_tx
            .send(TimerInput::Timer(TimerCommand::Rename(future)))
            .unwrap();
    }
    timers.turn(&shared, &waker).unwrap();
    assert_eq!(timers.inputs.as_ref().unwrap().len(), 1);
    assert!(timers.next(Instant::now()).unwrap() < future);
    timers.turn(&shared, &waker).unwrap();
    assert_eq!(timers.next(Instant::now()), Some(future));
    timers.deadlines.remove(TimerKey::Rename);
    for id in 0..=TIMER_EXPIRY_BURST {
        let client = ClientId(id as u64);
        timers
            .deadlines
            .insert(TimerKey::KeyTable(client), now, Expiry::KeyTable(client));
    }
    timers.turn(&shared, &waker).unwrap();
    assert_eq!(timers.deadlines.order.len(), 1);
    assert_eq!(timers.next(Instant::now()), Some(now));
    timers.turn(&shared, &waker).unwrap();
    assert!(timers.next(Instant::now()).is_none());
}

#[test]
fn tree_publication_wins_over_runtime_facts() {
    let mut flush = PublishFlush {
        tree: true,
        runtime_facts: true,
        ..PublishFlush::default()
    };
    assert_eq!(flush.take(), Some(PublishReason::Tree));
    assert_eq!(flush.take(), None);
}
