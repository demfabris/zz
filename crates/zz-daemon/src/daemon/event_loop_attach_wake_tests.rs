use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use mio::{Events, Poll, Token, Waker};
use zz_protocol::ClientHello;

use crate::daemon::tests::QUIET_PANE_COMMAND;
use crate::daemon::{
    ClientId, ClientKind, CommandInvocation, ExecutionContext, OutboundMailbox, Shared, attach,
    client_size_fact,
};
use crate::wake::{LoopThread, clear_loop_again, wake_loop};

const WAKE: Token = Token(7);

fn woken(poll: &mut Poll, events: &mut Events) -> usize {
    poll.poll(events, Some(Duration::from_millis(20)))
        .expect("poll");
    events.iter().filter(|event| event.token() == WAKE).count()
}

#[test]
fn loop_thread_self_wakes_trigger_once_per_turn() {
    let mut poll = Poll::new().expect("poll");
    let mut events = Events::with_capacity(8);
    let waker = std::sync::Arc::new(Waker::new(poll.registry(), WAKE).expect("waker"));
    let guard = LoopThread::enter(&waker);
    wake_loop(&waker).expect("first self-wake");
    wake_loop(&waker).expect("coalesced self-wake");
    assert_eq!(woken(&mut poll, &mut events), 1);
    wake_loop(&waker).expect("self-wake before the turn ends");
    assert_eq!(woken(&mut poll, &mut events), 0);
    clear_loop_again();
    wake_loop(&waker).expect("self-wake in the next turn");
    assert_eq!(woken(&mut poll, &mut events), 1);
    drop(guard);
    clear_loop_again();
}

#[test]
fn wakes_from_other_threads_and_other_loops_are_never_coalesced() {
    let mut poll = Poll::new().expect("poll");
    let mut events = Events::with_capacity(8);
    let waker = std::sync::Arc::new(Waker::new(poll.registry(), WAKE).expect("waker"));
    let other_poll = Poll::new().expect("other poll");
    let other = std::sync::Arc::new(Waker::new(other_poll.registry(), WAKE).expect("other waker"));
    let guard = LoopThread::enter(&other);
    wake_loop(&other).expect("own wake");
    wake_loop(&waker).expect("first foreign wake");
    assert_eq!(woken(&mut poll, &mut events), 1);
    wake_loop(&waker).expect("second foreign wake");
    assert_eq!(woken(&mut poll, &mut events), 1);
    let remote = std::sync::Arc::clone(&other);
    std::thread::spawn(move || {
        wake_loop(&remote).expect("remote wake");
    })
    .join()
    .expect("remote thread");
    drop(guard);
    let mut other_poll = other_poll;
    assert_eq!(woken(&mut other_poll, &mut events), 1);
}

#[test]
fn leaving_the_loop_restores_real_wakes() {
    let mut poll = Poll::new().expect("poll");
    let mut events = Events::with_capacity(8);
    let waker = std::sync::Arc::new(Waker::new(poll.registry(), WAKE).expect("waker"));
    drop(LoopThread::enter(&waker));
    wake_loop(&waker).expect("wake after the loop");
    assert_eq!(woken(&mut poll, &mut events), 1);
    wake_loop(&waker).expect("second wake after the loop");
    assert_eq!(woken(&mut poll, &mut events), 1);
}

fn run(shared: &Arc<Shared>, client: ClientId, kind: ClientKind, args: &[&str]) {
    shared
        .execute(
            client,
            kind,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap_or_else(|error| panic!("{args:?}: {error:?}"));
}

fn idle_in_copy_mode(sessions: &[&str]) -> (Arc<Shared>, ClientId, Arc<OutboundMailbox>) {
    let shared = Arc::new(Shared::new(1));
    for name in sessions {
        run(
            &shared,
            ClientId(u64::MAX),
            ClientKind::Command,
            &["new-session", "-d", "-s", name, QUIET_PANE_COMMAND],
        );
    }
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    let capabilities = [
        format!("{}100x30", ClientHello::CLIENT_SIZE_CAPABILITY_PREFIX),
        format!("{}8x16", ClientHello::CLIENT_CELL_CAPABILITY_PREFIX),
    ];
    {
        let mut inner = shared.inner.lock();
        inner.client_entry(client).size = client_size_fact(&capabilities);
        inner.client_entry(client).cell_pixels = attach::client_cell_fact(&capabilities);
    }
    run(
        &shared,
        client,
        ClientKind::Interactive,
        &["attach-session", "-t", sessions[0]],
    );
    run(&shared, client, ClientKind::Interactive, &["copy-mode"]);
    thread::sleep(Duration::from_millis(1500));
    (shared, client, mailbox)
}

fn returns_promptly(shared: &Arc<Shared>, client: ClientId, args: &'static [&'static str]) {
    let shared = Arc::clone(shared);
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        run(&shared, client, ClientKind::Interactive, args);
        let _ = done.send(());
    });
    assert!(
        finished.recv_timeout(Duration::from_secs(5)).is_ok(),
        "{args:?} waited on a shard wake its own hold kept back"
    );
}

#[cfg(unix)]
#[test]
fn detaching_from_copy_mode_on_an_idle_pane_returns_at_once() {
    let (shared, client, _mailbox) = idle_in_copy_mode(&["a"]);
    returns_promptly(&shared, client, &["detach-client"]);
}

#[cfg(unix)]
#[test]
fn switching_away_from_copy_mode_on_an_idle_pane_returns_at_once() {
    let (shared, client, _mailbox) = idle_in_copy_mode(&["a", "b"]);
    returns_promptly(&shared, client, &["switch-client", "-t", "b"]);
}
