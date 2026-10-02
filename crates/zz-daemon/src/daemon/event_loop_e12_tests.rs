use super::*;
use std::process::{Command, Stdio};

#[test]
fn child_signal_reaps_only_registered_children_with_coalesced_exits() {
    let socket = PathBuf::from(format!("/tmp/zz-e12-{}.sock", server_id()));
    let listener = LocalTransport::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _socket_guard = SocketGuard::new(socket);
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::new::<LocalTransport>(&listener, &shared).unwrap();
    let (completed, results) = mpsc::channel();
    let count = Arc::new(AtomicU64::new(0));
    let mut worker_child = Command::new("/bin/sh")
        .args(["-c", "exit 17"])
        .spawn()
        .unwrap();
    for _ in 0..20 {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 0.02; printf registered; exit 21"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let fd = child.stdout.take().unwrap().into();
        let shared = Arc::clone(&shared);
        let count = Arc::clone(&count);
        let completed = completed.clone();
        event_loop
            .register_job(jobs::Launch {
                child,
                descriptors: vec![jobs::Descriptor {
                    fd,
                    read: true,
                    input: None,
                    socket: false,
                }],
                policy: jobs::CompletionPolicy::ChildExitAndEof,
                deadline: Some(Instant::now() + Duration::from_secs(5)),
                process_group: false,
                output_limit: None,
                stream: None,
                pipe: None,
                cancel: None,
                complete: Box::new(move |result| {
                    completed.send(result).unwrap();
                    if count.fetch_add(1, Ordering::SeqCst) == 19 {
                        shared.request_shutdown();
                    }
                }),
            })
            .unwrap();
    }
    thread::sleep(Duration::from_millis(50));
    event_loop
        .run::<LocalTransport>(&listener, &shared, || {})
        .unwrap();
    assert_eq!(worker_child.wait().unwrap().code(), Some(17));
    let results = results.try_iter().collect::<Vec<_>>();
    assert_eq!(results.len(), 20);
    for result in results {
        assert_eq!(result.status.unwrap().code(), Some(21));
        assert_eq!(result.output, vec![b"registered".to_vec()]);
        assert!(result.error.is_none());
        assert!(!result.cancelled);
        let pid = rustix::process::Pid::from_raw(i32::try_from(result.pid).unwrap()).unwrap();
        assert_eq!(
            rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG).unwrap_err(),
            rustix::io::Errno::CHILD
        );
    }
    assert_eq!(event_loop.poll_timeout(Instant::now()), None);
}
