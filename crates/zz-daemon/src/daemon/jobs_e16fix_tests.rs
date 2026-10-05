use super::*;
use std::{io::Read as _, process::Stdio, sync::mpsc, time::Duration};

#[test]
fn closed_child_stdin_preserves_its_exit_status_without_cancellation() {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "exec 0<&-; printf ready; sleep 0.05; exit 7"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = [0; 5];
    child.stdout.take().unwrap().read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"ready");
    let input = child.stdin.take().unwrap();
    let (completed, results) = mpsc::channel();
    let launch = Launch {
        child: child.into(),
        descriptors: vec![Descriptor {
            fd: input.into(),
            read: false,
            input: Some(vec![b'x'; 1024 * 1024]),
            socket: false,
        }],
        policy: CompletionPolicy::ChildExit,
        deadline: Some(Instant::now() + Duration::from_secs(5)),
        process_group: false,
        output_limit: None,
        stream: None,
        pipe: None,
        cancel: None,
        complete: Box::new(move |result| completed.send(result).unwrap()),
    };
    let poll = mio::Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = 4;
    let id = jobs.register(poll.registry(), &mut token, launch).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while jobs.contains(id) {
        assert!(Instant::now() < deadline, "child exit was not collected");
        jobs.child_signal(poll.registry());
        std::thread::sleep(Duration::from_millis(2));
    }
    let result = results.try_recv().unwrap();
    assert!(!result.cancelled);
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.status.unwrap().code(), Some(7));
    assert!(jobs.tokens.is_empty());
    let pid = rustix::process::Pid::from_raw(result.pid as i32).unwrap();
    assert_eq!(
        rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG).unwrap_err(),
        rustix::io::Errno::CHILD
    );
}
