use super::*;
use std::{io::Read as _, os::unix::net::UnixStream, sync::mpsc, time::Duration};

#[test]
fn closed_job_input_keeps_socket_and_stream_jobs_alive_until_child_exit() {
    for (socket, stream) in [(true, false), (false, true)] {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec 0<&-; printf ready; sleep 0.2; exit 7"]);
        command.stdout(Stdio::piped());
        let socket_input = if socket {
            let (input, child_input) = UnixStream::pair().unwrap();
            command.stdin(std::os::fd::OwnedFd::from(child_input));
            Some(std::os::fd::OwnedFd::from(input))
        } else {
            command.stdin(Stdio::piped());
            None
        };
        let mut child = command.spawn().unwrap();
        let mut ready = [0; 5];
        child.stdout.take().unwrap().read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"ready");
        let input = socket_input.unwrap_or_else(|| child.stdin.take().unwrap().into());
        let feed = super::super::shard_sink::PipeFeed::new(|| {});
        let receiver = feed.reader();
        let pipe = stream.then(|| {
            PipeIo::new(
                Arc::new(parking_lot::Mutex::new(Arc::new(
                    zz_terminal::TerminalSession::spawn_output_view(
                        "job input".to_owned(),
                        String::new(),
                    ),
                ))),
                Some(receiver),
                std::task::Waker::noop().clone(),
            )
        });
        let (completed, results) = mpsc::channel();
        let launch = Launch {
            child,
            descriptors: vec![Descriptor {
                fd: input,
                read: false,
                input: Some(vec![b'x'; 1024 * 1024]),
                socket,
            }],
            policy: CompletionPolicy::ChildExit,
            deadline: Some(Instant::now() + Duration::from_secs(5)),
            process_group: false,
            output_limit: None,
            stream: None,
            pipe,
            cancel: None,
            complete: Box::new(move |result| completed.send(result).unwrap()),
        };
        let poll = mio::Poll::new().unwrap();
        let mut jobs = JobRegistry::default();
        let mut token = 4;
        let id = jobs.register(poll.registry(), &mut token, launch).unwrap();
        let input_token = *jobs.jobs[&id].ports.keys().next().unwrap();
        jobs.ready(poll.registry(), input_token, false, true);
        if stream {
            assert!(jobs.jobs[&id].pipe.as_ref().unwrap().input.is_none());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while jobs.contains(id) {
            assert!(Instant::now() < deadline, "closed input job did not finish");
            jobs.turn(poll.registry(), Instant::now());
            jobs.child_signal(poll.registry());
        }
        let result = results.try_recv().unwrap();
        assert!(!result.cancelled);
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(result.status.unwrap().code(), Some(7));
        assert!(jobs.tokens.is_empty());
    }
}
