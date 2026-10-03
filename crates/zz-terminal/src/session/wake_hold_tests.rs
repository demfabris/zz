use super::*;

fn shard_wake() -> (ActorWake, std::os::fd::OwnedFd) {
    let (read, write) = configured_actor_wake_pipe().expect("wake pipe");
    (
        ActorWake {
            ready: None,
            pipe: Some(Arc::new(write)),
            pending: Some(Arc::new(AtomicBool::new(false))),
        },
        read,
    )
}

fn queued(read: &std::os::fd::OwnedFd) -> usize {
    let mut buffer = [0_u8; 64];
    match rustix::io::read(read, &mut buffer) {
        Ok(read) => read,
        Err(rustix::io::Errno::AGAIN) => 0,
        Err(error) => panic!("wake pipe read failed: {error}"),
    }
}

#[test]
fn held_actor_wakes_write_one_byte_per_pipe_when_the_hold_ends() {
    let (wake, read) = shard_wake();
    let actor = ActorWake {
        pending: None,
        ..wake.clone()
    };
    let hold = hold_actor_wakes();
    wake.notify();
    actor.notify();
    actor.notify();
    assert_eq!(queued(&read), 0);
    drop(hold);
    assert_eq!(queued(&read), 1);
    wake.pending
        .as_ref()
        .unwrap()
        .store(false, Ordering::Release);
    wake.notify();
    assert_eq!(queued(&read), 1);
}

#[test]
fn a_hold_leaves_wakes_from_other_threads_alone() {
    let (wake, read) = shard_wake();
    let hold = hold_actor_wakes();
    wake.notify();
    assert_eq!(queued(&read), 0);
    let remote = wake.clone();
    std::thread::spawn(move || remote.notify())
        .join()
        .expect("remote notify");
    assert_eq!(queued(&read), 1);
    wake.pending
        .as_ref()
        .unwrap()
        .store(false, Ordering::Release);
    drop(hold);
    assert_eq!(queued(&read), 1);
}

#[test]
fn nested_holds_flush_only_when_the_outer_hold_ends() {
    let (wake, read) = shard_wake();
    let outer = hold_actor_wakes();
    let inner = hold_actor_wakes();
    wake.notify();
    drop(inner);
    assert_eq!(queued(&read), 0);
    drop(outer);
    assert_eq!(queued(&read), 1);
}

#[test]
fn a_blocking_request_releases_held_wakes_and_keeps_holding_later_ones() {
    let (wake, read) = shard_wake();
    let hold = hold_actor_wakes();
    wake.notify();
    release_held_wakes();
    assert_eq!(queued(&read), 1);
    wake.pending
        .as_ref()
        .unwrap()
        .store(false, Ordering::Release);
    wake.notify();
    assert_eq!(queued(&read), 0);
    drop(hold);
    assert_eq!(queued(&read), 1);
}

#[test]
fn wake_drain_stops_after_a_short_read() {
    let (read, write) = configured_actor_wake_pipe().expect("wake pipe");
    rustix::io::write(&write, &[1_u8; 3]).expect("fill wake pipe");
    drain_wake_pipe(&read).expect("drain wake pipe");
    assert_eq!(queued(&read), 0);
    rustix::io::write(&write, &[1_u8; 100]).expect("fill wake pipe");
    drain_wake_pipe(&read).expect("drain wake pipe");
    assert_eq!(queued(&read), 0);
}
