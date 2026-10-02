use super::super::{ClientKind, CommandInvocation, ExecutionContext, Shared};
use super::*;

#[test]
fn steady_send_keys_at_twenty_panes_starts_no_helpers() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    let run = |context: &mut ExecutionContext, name: &str, args: &[&str]| {
        shared
            .execute(
                ClientId(1),
                ClientKind::Command,
                context,
                &CommandInvocation::new(name, args.iter().copied()),
            )
            .unwrap();
    };
    run(
        &mut context,
        "new-session",
        &["-d", "-s", "s", "exec sleep 1000000"],
    );
    for _ in 0..19 {
        run(
            &mut context,
            "new-window",
            &["-d", "-t", "s:", "exec sleep 1000000"],
        );
    }
    run(&mut context, "send-keys", &["-t", "s:0.0", ""]);
    let (starts, submitted) = {
        let queue = shared.helpers.state.queue.lock();
        (queue.starts, queue.submitted)
    };
    for _ in 0..20 {
        run(&mut context, "send-keys", &["-t", "s:0.0", ""]);
    }
    {
        let queue = shared.helpers.state.queue.lock();
        eprintln!(
            "send-keys p20: starts={} tasks={}",
            queue.starts - starts,
            queue.submitted - submitted
        );
        assert_eq!(queue.starts, starts);
        assert_eq!(queue.submitted, submitted);
    }
    shared.request_shutdown();
}

#[cfg(unix)]
#[test]
fn a_full_helper_queue_never_parks_the_mux_loop() {
    let pool = Pool::default();
    *pool.state.loop_thread.lock() = Some(thread::current().id());
    let (started, ready) = std::sync::mpsc::sync_channel(2);
    let mut releases = Vec::new();
    for _ in 0..MAX_WORKERS + MAX_PENDING {
        let (release, wait) = std::sync::mpsc::channel();
        releases.push(release);
        pool.submit(Task::Hold(wait, started.clone())).unwrap();
        if releases.len() <= MAX_WORKERS {
            ready.recv_timeout(Duration::from_secs(1)).unwrap();
        }
    }
    let (_, wait) = std::sync::mpsc::channel();
    let before = std::time::Instant::now();
    assert_eq!(
        pool.submit_wait(Task::Hold(wait, started))
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    assert!(before.elapsed() < Duration::from_millis(50));
    drop(ready);
    for release in releases {
        release.send(()).unwrap();
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while pool.active() {
        assert!(std::time::Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(all(feature = "agent", unix))]
#[test]
fn a_peer_probe_leaves_no_idle_worker() {
    let pool = Pool::default();
    pool.submit(Task::Peers {
        panes: Vec::new(),
        always: false,
        reply: None,
        completed: None,
    })
    .unwrap();
    assert!(matches!(
        pool.results.recv_timeout(Duration::from_secs(1)).unwrap(),
        Result::Peers(_)
    ));
    let mut queue = pool.state.queue.lock();
    let deadline = std::time::Instant::now() + Duration::from_millis(50);
    while queue.workers != 0 {
        assert!(
            !pool
                .state
                .available
                .wait_until(&mut queue, deadline)
                .timed_out()
        );
    }
}
