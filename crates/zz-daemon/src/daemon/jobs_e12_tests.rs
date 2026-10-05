use super::*;
use mio::{Events, Poll};
use std::{
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn launch(command: &str, policy: CompletionPolicy, complete: mpsc::Sender<Completion>) -> Launch {
    let mut child = Command::new("/bin/sh")
        .args(["-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let fd = child.stdout.take().unwrap().into();
    Launch {
        child,
        descriptors: vec![Descriptor {
            fd,
            read: true,
            input: None,
            socket: false,
        }],
        policy,
        deadline: None,
        process_group: false,
        output_limit: None,
        stream: None,
        pipe: None,
        cancel: None,
        complete: Box::new(move |result| complete.send(result).unwrap()),
    }
}

fn pump(poll: &mut Poll, jobs: &mut JobRegistry, until: impl Fn(&JobRegistry) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut events = Events::with_capacity(128);
    while !until(jobs) {
        assert!(Instant::now() < deadline, "jobs did not complete");
        jobs.turn(poll.registry(), Instant::now());
        match poll.poll(&mut events, Some(Duration::from_millis(2))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("poll failed: {error}"),
        }
        for event in &events {
            jobs.ready(
                poll.registry(),
                event.token(),
                event.is_readable() || event.is_read_closed(),
                event.is_writable() || event.is_write_closed(),
            );
        }
        jobs.child_signal(poll.registry());
    }
}

fn assert_reaped(pid: u32) {
    let pid = rustix::process::Pid::from_raw(i32::try_from(pid).unwrap()).unwrap();
    assert_eq!(
        rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG).unwrap_err(),
        rustix::io::Errno::CHILD
    );
}

#[test]
fn twenty_registered_jobs_add_no_monitor_threads_and_reap_once() {
    if !crate::daemon::solo_tests::rerun_alone(
        "daemon::jobs::e12_tests::twenty_registered_jobs_add_no_monitor_threads_and_reap_once",
    ) {
        return;
    }
    let mut poll = Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = 4;
    let (done, results) = mpsc::channel();
    let pid = std::process::id();
    let before = crate::process_info::sample(pid).unwrap().threads;
    for _ in 0..20 {
        jobs.register(
            poll.registry(),
            &mut token,
            launch(
                "sleep 0.1; printf done",
                CompletionPolicy::ChildExitAndEof,
                done.clone(),
            ),
        )
        .unwrap();
    }
    assert!(crate::process_info::sample(pid).unwrap().threads <= before);
    pump(&mut poll, &mut jobs, |jobs| jobs.jobs.is_empty());
    let results = results.try_iter().collect::<Vec<_>>();
    assert_eq!(results.len(), 20);
    let mut ids = BTreeSet::new();
    for result in results {
        assert!(ids.insert(result.id));
        assert_eq!(result.status.unwrap().code(), Some(0));
        assert_eq!(result.output, vec![b"done".to_vec()]);
        assert!(result.error.is_none());
        assert_reaped(result.pid);
    }
    jobs.child_signal(poll.registry());
    assert!(jobs.tokens.is_empty());
}

#[test]
fn immediate_exit_is_checked_after_descriptors_attach() {
    let poll = Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = 4;
    let (done, results) = mpsc::channel();
    let job = launch(
        "printf immediate; exit 23",
        CompletionPolicy::ChildExitAndEof,
        done,
    );
    std::thread::sleep(Duration::from_millis(30));
    jobs.register(poll.registry(), &mut token, job).unwrap();
    let result = results.try_recv().unwrap();
    assert_eq!(result.status.unwrap().code(), Some(23));
    assert_eq!(result.output, vec![b"immediate".to_vec()]);
    assert_reaped(result.pid);
    assert!(jobs.jobs.is_empty());
}

#[test]
fn eof_before_exit_keeps_child_registered() {
    let mut poll = Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = 4;
    let (done, results) = mpsc::channel();
    let id = jobs
        .register(
            poll.registry(),
            &mut token,
            launch(
                "exec 1>&-; sleep 0.1; exit 19",
                CompletionPolicy::ChildExitAndEof,
                done,
            ),
        )
        .unwrap();
    pump(&mut poll, &mut jobs, |jobs| {
        jobs.jobs
            .get(&id)
            .is_some_and(|job| job.ports.values().all(|port| port.eof))
    });
    assert!(results.try_recv().is_err());
    assert!(jobs.jobs[&id].child.is_some());
    pump(&mut poll, &mut jobs, |jobs| jobs.jobs.is_empty());
    let result = results.try_recv().unwrap();
    assert_eq!(result.status.unwrap().code(), Some(19));
    assert_reaped(result.pid);
}

#[test]
fn inherited_output_obeys_family_completion_policy() {
    for policy in [
        CompletionPolicy::ChildExit,
        CompletionPolicy::ChildExitAndEof,
    ] {
        let mut poll = Poll::new().unwrap();
        let mut jobs = JobRegistry::default();
        let mut token = 4;
        let (done, results) = mpsc::channel();
        let (parent, inherited) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut job = launch("exit 0", policy, done);
        job.descriptors = vec![Descriptor {
            fd: parent.into(),
            read: true,
            input: None,
            socket: true,
        }];
        let id = jobs.register(poll.registry(), &mut token, job).unwrap();
        pump(&mut poll, &mut jobs, |jobs| {
            jobs.jobs.get(&id).is_none_or(|job| job.child.is_none())
        });
        if matches!(policy, CompletionPolicy::ChildExitAndEof) {
            assert!(results.try_recv().is_err());
            drop(inherited);
            pump(&mut poll, &mut jobs, |jobs| jobs.jobs.is_empty());
        }
        let result = results.try_recv().unwrap();
        assert_reaped(result.pid);
    }
}

#[test]
fn pipe_backpressure_and_bounded_turns_preserve_output() {
    let mut poll = Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = 4;
    let (done, results) = mpsc::channel();
    let mut child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input = vec![b'x'; IO_BURST * 4];
    let descriptors = vec![
        Descriptor {
            fd: child.stdin.take().unwrap().into(),
            read: false,
            input: Some(input.clone()),
            socket: false,
        },
        Descriptor {
            fd: child.stdout.take().unwrap().into(),
            read: true,
            input: None,
            socket: false,
        },
    ];
    jobs.register(
        poll.registry(),
        &mut token,
        Launch {
            child,
            descriptors,
            policy: CompletionPolicy::ChildExitAndEof,
            deadline: None,
            process_group: false,
            output_limit: None,
            stream: None,
            pipe: None,
            cancel: None,
            complete: Box::new(move |result| done.send(result).unwrap()),
        },
    )
    .unwrap();
    pump(&mut poll, &mut jobs, |jobs| jobs.jobs.is_empty());
    let result = results.try_recv().unwrap();
    assert_eq!(result.output[1], input);
    assert!(result.error.is_none());
    assert_reaped(result.pid);
}

#[test]
fn deadline_cancels_and_reaps_without_monitor() {
    let mut poll = Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = 4;
    let (done, results) = mpsc::channel();
    let mut job = launch("exec sleep 10", CompletionPolicy::ChildExitAndEof, done);
    job.deadline = Some(Instant::now());
    jobs.register(poll.registry(), &mut token, job).unwrap();
    assert!(jobs.next(Instant::now()).is_some());
    pump(&mut poll, &mut jobs, |jobs| jobs.jobs.is_empty());
    let result = results.try_recv().unwrap();
    assert!(result.cancelled);
    assert_reaped(result.pid);
}

#[test]
fn socket_input_shutdown_preserves_read_half() {
    use std::os::unix::net::UnixStream;
    let mut poll = Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = 4;
    let (done, results) = mpsc::channel();
    let (parent, child_socket) = UnixStream::pair().unwrap();
    let child = Command::new("/bin/cat")
        .stdin(Stdio::from(OwnedFd::from(
            child_socket.try_clone().unwrap(),
        )))
        .stdout(Stdio::from(OwnedFd::from(
            child_socket.try_clone().unwrap(),
        )))
        .spawn()
        .unwrap();
    drop(child_socket);
    let input = vec![b's'; IO_BURST * 2];
    jobs.register(
        poll.registry(),
        &mut token,
        Launch {
            child,
            descriptors: vec![Descriptor {
                fd: parent.into(),
                read: true,
                input: Some(input.clone()),
                socket: true,
            }],
            policy: CompletionPolicy::ChildExitAndEof,
            deadline: None,
            process_group: false,
            output_limit: None,
            stream: None,
            pipe: None,
            cancel: None,
            complete: Box::new(move |result| done.send(result).unwrap()),
        },
    )
    .unwrap();
    pump(&mut poll, &mut jobs, |jobs| jobs.jobs.is_empty());
    let result = results.try_recv().unwrap();
    assert_eq!(result.output, vec![input]);
    assert!(result.error.is_none());
    assert_reaped(result.pid);
}

#[test]
fn stale_token_cannot_touch_reused_descriptor() {
    let mut poll = Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = 4;
    let (done, results) = mpsc::channel();
    let old_token = Token(token);
    jobs.register(
        poll.registry(),
        &mut token,
        launch(
            "printf old",
            CompletionPolicy::ChildExitAndEof,
            done.clone(),
        ),
    )
    .unwrap();
    pump(&mut poll, &mut jobs, |jobs| jobs.jobs.is_empty());
    assert_eq!(results.try_recv().unwrap().output, vec![b"old".to_vec()]);
    jobs.register(
        poll.registry(),
        &mut token,
        launch(
            "sleep 0.02; printf new",
            CompletionPolicy::ChildExitAndEof,
            done,
        ),
    )
    .unwrap();
    assert!(!jobs.contains_token(old_token));
    jobs.ready(poll.registry(), old_token, true, true);
    pump(&mut poll, &mut jobs, |jobs| jobs.jobs.is_empty());
    assert_eq!(results.try_recv().unwrap().output, vec![b"new".to_vec()]);
}

#[test]
fn failed_attachment_reaps_the_owned_child() {
    let poll = Poll::new().unwrap();
    let mut jobs = JobRegistry::default();
    let mut token = usize::MAX - 1;
    let (done, results) = mpsc::channel();
    let mut job = launch("exec sleep 10", CompletionPolicy::ChildExitAndEof, done);
    let pid = job.child.id();
    let (reader, _writer) = std::os::unix::net::UnixStream::pair().unwrap();
    job.descriptors.push(Descriptor {
        fd: reader.into(),
        read: true,
        input: None,
        socket: true,
    });
    assert!(jobs.register(poll.registry(), &mut token, job).is_err());
    assert!(jobs.jobs.is_empty());
    assert!(jobs.tokens.is_empty());
    let result = results.try_recv().unwrap();
    assert!(result.error.is_some());
    assert_eq!(result.pid, pid);
    assert!(results.try_recv().is_err());
    assert_reaped(pid);
}
