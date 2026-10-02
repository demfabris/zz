use super::*;
use mio::{Events, Poll, Token, Waker};

fn fixture() -> (
    Arc<Shared>,
    ExecutionContext,
    ClientId,
    Poll,
    Arc<Waker>,
    LoopTimers,
) {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    let client = ClientId(1);
    {
        let mut inner = shared.inner.lock();
        inner
            .engine
            .execute(&mut context, &CommandInvocation::new("new-session", ["-d"]))
            .unwrap();
        inner.client_entry(client).subscriber = Some(OutboundMailbox::new());
        inner
            .attached
            .entry(context.session.unwrap())
            .or_default()
            .insert(client);
    }
    let poll = Poll::new().unwrap();
    let waker = Arc::new(Waker::new(poll.registry(), Token(1)).unwrap());
    let timers = LoopTimers::new(&shared, &waker);
    (shared, context, client, poll, waker, timers)
}

fn model(shared: &Shared, context: &mut ExecutionContext, args: &[&str]) {
    shared
        .inner
        .lock()
        .engine
        .execute(
            context,
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap();
}

fn sync(shared: &Arc<Shared>, timers: &mut LoopTimers, waker: &Arc<Waker>) {
    shared.nudge_client_timers();
    timers.turn(shared, waker).unwrap();
}

#[test]
fn status_deadlines_follow_enabled_nonzero_intervals_and_last_consumers() {
    let (shared, mut context, client, _poll, waker, mut timers) = fixture();
    let session = context.session.unwrap();
    assert_eq!(timers.deadlines.entries.len(), 1);
    let original = timers.deadlines.next().unwrap();
    let other = ClientId(2);
    {
        let mut inner = shared.inner.lock();
        inner.client_entry(other).subscriber = Some(OutboundMailbox::new());
        inner.attached.entry(session).or_default().insert(other);
    }
    sync(&shared, &mut timers, &waker);
    assert_eq!(timers.deadlines.entries.len(), 1);
    assert_eq!(timers.deadlines.next(), Some(original));
    model(
        &shared,
        &mut context,
        &["set", "-g", "status-interval", "30"],
    );
    sync(&shared, &mut timers, &waker);
    assert_eq!(timers.clients.intervals[&session], Duration::from_secs(30));
    assert!(timers.deadlines.next().unwrap() > original);
    model(&shared, &mut context, &["set", "-g", "status", "off"]);
    sync(&shared, &mut timers, &waker);
    assert!(timers.deadlines.entries.is_empty());
    model(&shared, &mut context, &["set", "-g", "status", "on"]);
    model(
        &shared,
        &mut context,
        &["set", "-g", "status-interval", "0"],
    );
    sync(&shared, &mut timers, &waker);
    assert!(timers.deadlines.entries.is_empty());
    model(
        &shared,
        &mut context,
        &["set", "-g", "status-interval", "2"],
    );
    sync(&shared, &mut timers, &waker);
    assert_eq!(timers.deadlines.entries.len(), 1);
    shared.inner.lock().attached.clear();
    sync(&shared, &mut timers, &waker);
    assert!(timers.deadlines.entries.is_empty());
    assert!(timers.next(Instant::now()).is_none());
    assert!(
        shared
            .inner
            .lock()
            .client(client)
            .unwrap()
            .subscriber
            .is_some()
    );
}

#[test]
fn subscriptions_and_monitors_keep_separate_deadlines_with_hidden_status() {
    let (shared, mut context, client, _poll, waker, mut timers) = fixture();
    model(&shared, &mut context, &["set", "-g", "status", "off"]);
    shared.update_control_subscription(client, "query::#{session_name}");
    model(
        &shared,
        &mut context,
        &[
            "set-hook",
            "-B",
            "@b6:@*:#{window_name}",
            "set -g @fired yes",
        ],
    );
    sync(&shared, &mut timers, &waker);
    assert_eq!(timers.deadlines.entries.len(), 2);
    assert!(timers.deadlines.get(TimerKey::Subscriptions).is_some());
    assert!(timers.deadlines.get(TimerKey::Monitors).is_some());
    shared.update_control_subscription(client, "query");
    sync(&shared, &mut timers, &waker);
    assert_eq!(timers.deadlines.entries.len(), 1);
    model(&shared, &mut context, &["set-hook", "-u", "-B", "@b6"]);
    sync(&shared, &mut timers, &waker);
    assert!(timers.deadlines.entries.is_empty());
}

#[test]
fn clock_labels_keep_a_deadline_when_status_is_disabled() {
    let (shared, mut context, client, _poll, waker, mut timers) = fixture();
    model(&shared, &mut context, &["set", "-g", "status", "off"]);
    model(
        &shared,
        &mut context,
        &["set", "-g", "window-status-format", "%S"],
    );
    sync(&shared, &mut timers, &waker);
    assert_eq!(timers.deadlines.entries.len(), 1);
    assert!(timers.deadlines.get(TimerKey::Labels).is_some());
    shared.inner.lock().client_entry(client).ctrl_subscriptions =
        Some(zz_protocol::Subscriptions::control());
    sync(&shared, &mut timers, &waker);
    assert!(timers.deadlines.entries.is_empty());
}

#[test]
fn read_only_queries_leave_client_deadlines_and_waker_idle() {
    let (shared, mut context, client, mut poll, waker, mut timers) = fixture();
    shared.inner.lock().client_entry(client).ctrl_subscriptions =
        Some(zz_protocol::Subscriptions::control());
    sync(&shared, &mut timers, &waker);
    let mut events = Events::with_capacity(8);
    poll.poll(&mut events, Some(Duration::ZERO)).unwrap();
    for _ in 0..20 {
        shared
            .execute(
                client,
                ClientKind::Control,
                &mut context,
                &CommandInvocation::new("display-message", ["-p", "query"]),
            )
            .unwrap();
    }
    assert!(timers.inputs.as_ref().unwrap().is_empty());
    assert!(timers.next(Instant::now()).is_none());
    poll.poll(&mut events, Some(Duration::from_millis(10)))
        .unwrap();
    assert!(events.is_empty());
    model(
        &shared,
        &mut context,
        &[
            "set-hook",
            "-B",
            "@b6:@*:#{window_name}",
            "set -g @fired yes",
        ],
    );
    shared.nudge_client_timers();
    poll.poll(&mut events, Some(Duration::from_millis(100)))
        .unwrap();
    assert!(!events.is_empty());
    timers.turn(&shared, &waker).unwrap();
    assert_eq!(timers.deadlines.entries.len(), 1);
}

#[test]
fn pending_modes_wake_the_loop_without_a_periodic_status_deadline() {
    let (shared, mut context, client, mut poll, waker, mut timers) = fixture();
    model(
        &shared,
        &mut context,
        &["set", "-g", "status-interval", "0"],
    );
    sync(&shared, &mut timers, &waker);
    let mut events = Events::with_capacity(8);
    poll.poll(&mut events, Some(Duration::ZERO)).unwrap();
    shared
        .status
        .lock()
        .request_mode_refresh(BTreeSet::from([client]));
    poll.poll(&mut events, Some(Duration::from_millis(100)))
        .unwrap();
    assert!(!events.is_empty());
    timers.turn(&shared, &waker).unwrap();
    assert!(shared.status.lock().take_pending_modes().is_empty());
    assert!(timers.deadlines.entries.is_empty());
}

#[test]
fn status_job_output_reaches_clients_without_a_periodic_deadline() {
    let (shared, mut context, client, mut poll, waker, mut timers) = fixture();
    let _jobs = status_jobs::tests::Driver::new(shared.status.lock().job_client());
    model(
        &shared,
        &mut context,
        &["set", "-g", "status-interval", "0"],
    );
    model(
        &shared,
        &mut context,
        &["set", "-g", "status-left", "#(printf b6-ready)"],
    );
    model(&shared, &mut context, &["set", "-g", "status-right", ""]);
    sync(&shared, &mut timers, &waker);
    let mailbox = shared
        .inner
        .lock()
        .client(client)
        .unwrap()
        .subscriber
        .clone()
        .unwrap();
    shared.refresh_status();
    let mut events = Events::with_capacity(8);
    let deadline = Instant::now() + Duration::from_millis(750);
    loop {
        let messages = crate::daemon::tests::take_reliable_messages(&mailbox);
        if messages.iter().any(|message| {
            matches!(message,
            ProtocolMessage::Event(Event { payload: EventPayload::StatusChanged { status }, .. })
                if status.left.ends_with("b6-ready"))
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "status job update did not reach the client"
        );
        loop {
            match poll.poll(
                &mut events,
                Some(deadline.saturating_duration_since(Instant::now())),
            ) {
                Ok(()) => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("status poll failed: {error}"),
            }
        }
        timers.turn(&shared, &waker).unwrap();
    }
    assert!(timers.deadlines.entries.is_empty());
}

#[cfg(feature = "agent")]
#[test]
fn peer_requests_throttle_and_follow_up_without_client_timers() {
    let shared = Arc::new(Shared::new(1));
    let now = Instant::now();
    let mut clients = ClientTimers::default();
    let mut deadlines = Deadlines::default();
    clients.sync(&shared, &mut deadlines, now);
    assert!(deadlines.entries.is_empty());
    shared.request_peer_probe();
    clients.sync(&shared, &mut deadlines, now);
    assert_eq!(deadlines.next(), Some(now));
    assert!(matches!(deadlines.pop_due(now), Some(Expiry::PeerProbe)));
    shared.prepare_peer_probe(&mut clients.probe, now);
    clients.sync(&shared, &mut deadlines, now);
    assert_eq!(deadlines.next(), Some(now + PEER_PROBE_INTERVAL));
    shared.request_peer_probe();
    clients.sync(&shared, &mut deadlines, now + Duration::from_millis(10));
    assert_eq!(deadlines.next(), Some(now + PEER_PROBE_INTERVAL));
    shared.prepare_peer_probe(&mut clients.probe, now + PEER_PROBE_INTERVAL);
    deadlines.remove(TimerKey::PeerProbe);
    clients.sync(&shared, &mut deadlines, now + PEER_PROBE_INTERVAL);
    assert_eq!(deadlines.next(), Some(now + PEER_PROBE_INTERVAL * 2));
    shared.prepare_peer_probe(&mut clients.probe, now + PEER_PROBE_INTERVAL * 2);
    clients.sync(&shared, &mut deadlines, now + PEER_PROBE_INTERVAL * 2);
    assert!(deadlines.entries.is_empty());
}

#[test]
fn a_blocked_monitor_hook_does_not_delay_subscription_deadlines() {
    let (shared, mut context, client, _poll, waker, mut timers) = fixture();
    let _jobs = pipe_jobs::Driver::new(&shared);
    model(&shared, &mut context, &["set", "-g", "status", "off"]);
    shared.update_control_subscription(client, "query::#{session_name}");
    struct Release(PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = std::fs::write(&self.0, "release");
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let started_path = directory.path().join("started");
    let release = Release(directory.path().join("release"));
    let body = format!(
        "run-shell 'touch {}; while [ ! -e {} ]; do sleep 0.01; done'",
        started_path.display(),
        release.0.display()
    );
    model(
        &shared,
        &mut context,
        &["set-hook", "-B", "@b6:@*:#{window_name}", &body],
    );
    shared.run_format_monitors();
    model(&shared, &mut context, &["rename-window", "changed"]);
    sync(&shared, &mut timers, &waker);
    timers
        .deadlines
        .insert(TimerKey::Monitors, Instant::now(), Expiry::Monitors);
    timers.turn(&shared, &waker).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !started_path.exists() {
        timers.turn(&shared, &waker).unwrap();
        assert!(Instant::now() < deadline, "monitor hook did not park");
        thread::sleep(Duration::from_millis(1));
    }
    timers.deadlines.insert(
        TimerKey::Subscriptions,
        Instant::now(),
        Expiry::Subscriptions,
    );
    let started = Instant::now();
    timers.turn(&shared, &waker).unwrap();
    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(shared.connection_threads.worker_count(), 0);
    let samples = shared
        .inner
        .lock()
        .client(client)
        .unwrap()
        .control_output
        .as_ref()
        .unwrap()
        .subscriptions["query"]
        .previous
        .len();
    drop(release);
    assert_eq!(samples, 1);
    while !timers.hooks.is_empty() {
        assert!(Instant::now() < deadline, "monitor did not finish");
        timers.turn(&shared, &waker).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
}
