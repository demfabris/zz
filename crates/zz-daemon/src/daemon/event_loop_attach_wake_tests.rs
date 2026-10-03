use std::time::Duration;

use mio::{Events, Poll, Token, Waker};

use crate::transport::{LoopThread, clear_loop_again, wake_loop};

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
