use super::*;
use std::process::Command;

#[test]
fn discovery_jobs_drain_cancel_limit_and_reap_on_the_loop() {
    let socket = PathBuf::from(format!("/tmp/zz-e19-{}.sock", server_id()));
    let listener = LocalTransport::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _socket_guard = SocketGuard::new(socket);
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::new::<LocalTransport>(&listener, &shared).unwrap();
    let jobs = shared.helpers.jobs.clone();
    let owner = Arc::clone(&shared);
    let worker = thread::spawn(move || {
        let run = |script: &str, limit, timeout, cancel: Arc<AtomicBool>| {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            jobs.output(command, limit, Instant::now() + timeout, cancel)
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let output = run(
                "i=0; while [ $i -lt 10000 ]; do printf 'error-output-line\\n' >&2; i=$((i + 1)); done; printf ok",
                1024 * 1024,
                Duration::from_secs(5),
                Arc::new(AtomicBool::new(false)),
            ).unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, b"ok");
            assert_eq!(output.stderr.len(), 180_000);
            let started = Instant::now();
            let cancel = Arc::new(AtomicBool::new(false));
            let signal = Arc::clone(&cancel);
            let notifier = jobs.clone();
            let cancellation = thread::spawn(move || {
                thread::sleep(Duration::from_millis(50));
                signal.store(true, Ordering::Release);
                notifier.notify();
            });
            let error = run("sleep 20", 1024, Duration::from_secs(5), cancel).unwrap_err();
            cancellation.join().unwrap();
            assert!(error.contains("cancelled"), "{error}");
            assert!(started.elapsed() < Duration::from_secs(1));
            let error = run(
                "head -c 10000 /dev/zero",
                1024,
                Duration::from_secs(2),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap_err();
            assert!(error.contains("output limit"), "{error}");
            let started = Instant::now();
            let error = run(
                "sleep 20",
                1024,
                Duration::from_millis(50),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap_err();
            assert!(error.contains("timed out"), "{error}");
            assert!(started.elapsed() < Duration::from_secs(1));
        }));
        owner.request_shutdown();
        outcome.unwrap();
    });
    event_loop
        .run::<LocalTransport>(&listener, &shared, || {})
        .unwrap();
    worker.join().unwrap();
    assert!(
        event_loop
            .helper_jobs
            .iter()
            .all(|(id, _)| !event_loop.jobs.contains(*id))
    );
    assert_eq!(event_loop.jobs.next(Instant::now()), None);
}
