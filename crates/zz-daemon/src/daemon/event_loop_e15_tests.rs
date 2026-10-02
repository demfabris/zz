use super::*;

fn pump(event_loop: &mut EventLoop, id: jobs::JobId) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while event_loop.jobs.contains(id) {
        assert!(Instant::now() < deadline, "status job did not complete");
        event_loop
            .jobs
            .turn(event_loop.poll.registry(), Instant::now());
        event_loop.jobs.child_signal(event_loop.poll.registry());
        match event_loop
            .poll
            .poll(&mut event_loop.events, Some(Duration::from_millis(2)))
        {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => panic!("status job poll failed: {error}"),
        }
        let tokens = event_loop
            .events
            .iter()
            .map(mio::event::Event::token)
            .collect::<Vec<_>>();
        for token in tokens {
            event_loop
                .jobs
                .ready(event_loop.poll.registry(), token, true, false);
        }
    }
}

#[test]
fn status_job_alive_at_loop_shutdown_is_cancelled_and_reaped_once() {
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let client = shared.status.lock().job_client();
    let updates = client
        .launch(
            1,
            crate::shell_process("printf 'ready\\n'; exec sleep 30"),
            None,
        )
        .unwrap();
    event_loop.turn_status_jobs();
    let id = event_loop.status_jobs[&1];
    let deadline = Instant::now() + Duration::from_secs(3);
    let pid = loop {
        event_loop.jobs.child_signal(event_loop.poll.registry());
        if let Ok(update) = updates.try_recv()
            && update.latest.as_deref() == Some("ready")
        {
            break update.pid;
        }
        assert!(Instant::now() < deadline, "status job did not publish");
        thread::sleep(Duration::from_millis(1));
    };
    event_loop.start_shutdown(&shared);
    event_loop.jobs.cancel_all(event_loop.poll.registry());
    pump(&mut event_loop, id);
    event_loop.jobs.child_signal(event_loop.poll.registry());
    let update = updates.try_recv().unwrap();
    assert!(update.complete);
    assert!(updates.try_recv().is_err());
    let pid = rustix::process::Pid::from_raw(pid as i32).unwrap();
    loop {
        match rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG) {
            Err(rustix::io::Errno::INTR) => {}
            result => {
                assert_eq!(result.unwrap_err(), rustix::io::Errno::CHILD);
                break;
            }
        }
    }
    assert!(
        client
            .launch(2, crate::shell_process("exit 0"), None)
            .is_err()
    );
}

#[test]
fn cancellation_kills_inherited_status_output_after_the_shell_exits() {
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let client = shared.status.lock().job_client();
    let updates = client
        .launch(
            1,
            crate::shell_process("sleep 30 & printf 'ready\\n'"),
            None,
        )
        .unwrap();
    event_loop.turn_status_jobs();
    let id = event_loop.status_jobs[&1];
    let deadline = Instant::now() + Duration::from_secs(3);
    let pid = loop {
        event_loop.jobs.child_signal(event_loop.poll.registry());
        if let Ok(update) = updates.try_recv()
            && update.latest.as_deref() == Some("ready")
        {
            break update.pid;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    };
    thread::sleep(Duration::from_millis(20));
    event_loop.jobs.child_signal(event_loop.poll.registry());
    assert!(event_loop.jobs.contains(id));
    client.cancel(1);
    event_loop.turn_status_jobs();
    pump(&mut event_loop, id);
    assert!(updates.try_recv().unwrap().complete);
    let pid = rustix::process::Pid::from_raw(pid as i32).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while rustix::process::test_kill_process_group(pid).is_ok() {
        assert!(
            Instant::now() < deadline,
            "inherited status pipe survived cancellation"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
