use super::*;
use mio::{Events, Poll, Token, Waker};

#[test]
fn unattached_subscriptions_and_unregistration_do_not_wake_timers() {
    let shared = Arc::new(Shared::new(1));
    let mut poll = Poll::new().unwrap();
    let waker = Arc::new(Waker::new(poll.registry(), Token(1)).unwrap());
    let mut timers = LoopTimers::new(&shared, &waker);
    let (client, _) =
        shared.register_subscribed(ClientKind::Control, None, None, OutboundMailbox::new());
    assert!(timers.inputs.as_ref().unwrap().is_empty());
    shared.unregister(client);
    timers.turn(&shared, &waker).unwrap();
    assert!(timers.next(Instant::now()).is_none());
    let mut events = Events::with_capacity(8);
    poll.poll(&mut events, Some(Duration::from_millis(10)))
        .unwrap();
    assert!(events.is_empty());
}

#[test]
fn command_deadlines_change_only_for_status_options_and_monitor_presence() {
    let shared = Arc::new(Shared::new(1));
    let client = ClientId(1);
    let mut context = ExecutionContext::default();
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
    let mut poll = Poll::new().unwrap();
    let waker = Arc::new(Waker::new(poll.registry(), Token(1)).unwrap());
    let mut timers = LoopTimers::new(&shared, &waker);
    let original = timers.deadlines.next();
    for args in [
        vec!["set", "-g", "@unrelated", "yes"],
        vec!["select-pane", "-t", "0"],
        vec!["show-options", "-gqv", "status"],
    ] {
        shared
            .execute(
                client,
                ClientKind::Control,
                &mut context,
                &CommandInvocation::new(args[0], args[1..].iter().copied()),
            )
            .unwrap();
        assert!(timers.inputs.as_ref().unwrap().is_empty(), "{args:?}");
        timers.turn(&shared, &waker).unwrap();
        assert_eq!(timers.deadlines.next(), original);
    }
    let mut events = Events::with_capacity(8);
    poll.poll(&mut events, Some(Duration::from_millis(10)))
        .unwrap();
    assert!(events.is_empty());
    for (args, status, monitor) in [
        (vec!["set", "-g", "status", "off"], false, false),
        (
            vec![
                "set-hook",
                "-B",
                "@b6fix:@*:#{window_name}",
                "set -g @fired yes",
            ],
            false,
            true,
        ),
        (vec!["set-hook", "-u", "-B", "@b6fix"], false, false),
        (vec!["set", "-g", "status-interval", "30"], false, false),
        (vec!["set", "-g", "status", "on"], true, false),
    ] {
        shared
            .execute(
                client,
                ClientKind::Control,
                &mut context,
                &CommandInvocation::new(args[0], args[1..].iter().copied()),
            )
            .unwrap();
        assert!(!timers.inputs.as_ref().unwrap().is_empty(), "{args:?}");
        timers.turn(&shared, &waker).unwrap();
        assert_eq!(
            timers
                .deadlines
                .get(TimerKey::Status(context.session.unwrap()))
                .is_some(),
            status
        );
        assert_eq!(timers.deadlines.get(TimerKey::Monitors).is_some(), monitor);
    }
    assert_eq!(
        timers.clients.intervals[&context.session.unwrap()],
        Duration::from_secs(30)
    );
}
