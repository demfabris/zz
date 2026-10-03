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
fn releasing_held_wakes_keeps_holding_later_ones() {
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

fn idle_pane(shard: &shard::ShardHandle) -> TerminalSession {
    let session = TerminalSession::spawn_with_shard(
        100,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            command: Some(vec![
                "stty -echo; printf 'ready\\r\\n'; exec cat".to_owned(),
            ]),
            ..TerminalSpawn::default()
        },
        Ok(Some(shard.clone())),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !session
        .capture(CaptureOptions::default())
        .is_ok_and(|output| output.contains("ready"))
    {
        assert!(
            Instant::now() < deadline,
            "the pane never printed its marker"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_secs(1));
    session
}

fn returns_within_a_second(what: &str, work: impl FnOnce() + Send + 'static) {
    let (done, finished) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        work();
        let _ = done.send(());
    });
    assert!(
        finished.recv_timeout(Duration::from_secs(1)).is_ok(),
        "{what} waited on a wake its own hold kept back"
    );
}

#[test]
fn a_send_into_a_full_queue_under_a_hold_writes_the_held_wake_first() {
    let shard = shard::ShardHandle::start(110).expect("shard");
    let session = idle_pane(&shard);
    returns_within_a_second("a view action after attach_view", move || {
        let view = TerminalViewId(1);
        let _hold = hold_actor_wakes();
        session.attach_view(view);
        session.view_action(view, TerminalViewAction::ScrollBottom);
    });
}

#[test]
fn a_request_under_a_hold_wakes_the_actor_for_its_own_command() {
    let shard = shard::ShardHandle::start(111).expect("shard");
    let session = idle_pane(&shard);
    returns_within_a_second("a fresh viewport request", move || {
        let _hold = hold_actor_wakes();
        session.fresh_viewport();
    });
}
