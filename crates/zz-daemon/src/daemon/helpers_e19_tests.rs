use super::*;
use std::{sync::mpsc, time::Instant};

#[test]
fn helpers_are_bounded_and_retire_within_one_second() {
    let pool = Pool::default();
    assert_eq!(pool.state.queue.lock().workers, 0);
    let (started, ready) = mpsc::sync_channel(2);
    let mut releases = Vec::new();
    for _ in 0..2 {
        let (release, wait) = mpsc::channel();
        releases.push(release);
        pool.submit(Task::Hold(wait, started.clone())).unwrap();
    }
    for _ in 0..2 {
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    assert_eq!(pool.state.queue.lock().workers, 2);
    for _ in 0..MAX_PENDING {
        let (release, wait) = mpsc::channel();
        releases.push(release);
        pool.submit(Task::Hold(wait, started.clone())).unwrap();
    }
    let (_, wait) = mpsc::channel();
    assert_eq!(
        pool.submit(Task::Hold(wait, started)).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(pool.state.queue.lock().workers, 2);
    drop(ready);
    for release in releases {
        release.send(()).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    while pool.state.queue.lock().workers != 0 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let (release, wait) = mpsc::channel();
    let (started, ready) = mpsc::sync_channel(1);
    pool.submit(Task::Hold(wait, started)).unwrap();
    ready.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(pool.state.queue.lock().workers, 1);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while pool.active() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}
