use super::*;

#[test]
fn the_control_wake_skips_the_waker_while_the_loop_is_awake() {
    let mut poll = mio::Poll::new().expect("poll");
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(7)).expect("waker"));
    let wake = ControlWake::new();
    wake.install(Arc::clone(&waker));
    let mut events = mio::Events::with_capacity(4);
    let mut fired = || {
        poll.poll(&mut events, Some(Duration::ZERO)).expect("poll");
        !events.is_empty()
    };

    wake.notify();
    assert!(fired());
    assert!(wake.take());
    wake.unpark();
    wake.notify();
    assert!(!fired());
    assert!(wake.park());
    wake.unpark();
    assert!(wake.take());
    assert!(!wake.park());
    wake.notify();
    assert!(fired());
    wake.unpark();
    assert!(wake.take());

    wake.again(false);
    assert!(!wake.park());
    assert!(!fired());
    wake.unpark();
    wake.again(true);
    assert!(!fired());
    assert!(wake.park());
    wake.unpark();
    assert!(wake.take());
    wake.kick();
    assert!(!fired());
    assert!(wake.park());
    assert!(wake.take());
    wake.kick();
    assert!(fired());
}

#[test]
fn a_reused_drain_buffer_carries_only_the_drained_bytes() {
    let now = Instant::now();
    let mut pending = VecDeque::from([
        PendingControlOutput {
            bytes: Arc::from(&b"hello"[..]),
            offset: 0,
            enqueued_at: now,
            seq: 0,
        },
        PendingControlOutput {
            bytes: Arc::from(&b"world"[..]),
            offset: 0,
            enqueued_at: now,
            seq: 1,
        },
    ]);
    let (_, bytes) = drain_control_pane_output(&mut pending, 7, u64::MAX, now, b"stale".to_vec());
    assert_eq!(bytes, b"hellowo");
    let (_, bytes) = drain_control_pane_output(&mut pending, 64, u64::MAX, now, bytes);
    assert_eq!(bytes, b"rld");
    assert!(pending.is_empty());
}

#[test]
fn small_chunks_merge_into_the_untouched_tail_until_a_line_or_the_cap() {
    let feed = ControlFeed::new(Arc::new(ControlWake::new()));
    let mailbox = OutboundMailbox::new();
    let pane = PaneId(3);
    let other = PaneId(4);
    let chunk: Arc<[u8]> = Arc::from(vec![b'a'; 1024]);
    for _ in 0..8 {
        assert!(feed.push(pane, &chunk, None));
    }
    assert_eq!(feed.queued(pane), 1);
    assert_eq!(feed.queued_bytes(pane), vec![b'a'; 8 * 1024]);
    assert!(feed.push(pane, &chunk, None));
    assert_eq!(feed.queued(pane), 2);

    assert!(feed.order(&mailbox, ControlOrder::AfterOutput, |_| true));
    assert!(feed.push(pane, &chunk, None));
    assert_eq!(feed.queued(pane), 3);
    assert!(feed.push(pane, &chunk, None));
    assert_eq!(feed.queued(pane), 3);

    assert!(feed.push(other, &chunk, None));
    assert!(feed.push(pane, &chunk, None));
    assert_eq!(feed.queued(pane), 4);

    feed.configure(false, Some(1000), BTreeSet::new());
    assert!(feed.push(pane, &chunk, None));
    assert_eq!(feed.queued(pane), 5);
    assert_eq!(feed.queued_bytes(pane).len(), 13 * 1024);
}
