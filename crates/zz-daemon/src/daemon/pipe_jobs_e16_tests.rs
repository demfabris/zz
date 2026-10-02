use super::*;

#[test]
fn twenty_active_pipes_deliver_on_the_loop_without_reader_threads() {
    let shared = Arc::new(Shared::new(16));
    let _loop = pipe_jobs::Driver::new(&shared);
    let directory = tempfile::tempdir().unwrap();
    let mut panes = Vec::new();
    for index in 0..20 {
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "new-session",
                    ["-d", "-s", &format!("e16-{index}"), "exec /bin/cat"],
                ),
            )
            .unwrap();
        let pane = context.pane.unwrap();
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        panes.push((
            pane,
            terminal,
            directory.path().join(format!("{index}.bin")),
        ));
    }
    let before = crate::process_info::sample(std::process::id())
        .unwrap()
        .threads;
    for (pane, _, output) in &panes {
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut ExecutionContext::default(),
                &CommandInvocation::new(
                    "pipe-pane",
                    [
                        "-t",
                        &pane.to_string(),
                        &format!(
                            "cat > {}",
                            crate::endpoint::shell_quote(&output.to_string_lossy())
                        ),
                    ],
                ),
            )
            .unwrap();
    }
    {
        let inner = shared.inner.lock();
        assert_eq!(inner.pane_pipes.len(), 20);
        assert_eq!(inner.control_output_taps.len(), 20);
    }
    assert!(
        crate::process_info::sample(std::process::id())
            .unwrap()
            .threads
            <= before
    );
    for (_, terminal, _) in &panes {
        assert!(terminal.send_raw_input(Arc::from(b"e16-bytes\n".as_slice())));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while panes.iter().any(|(_, _, output)| {
        !fs::read(output)
            .unwrap_or_default()
            .windows(9)
            .any(|bytes| bytes == b"e16-bytes")
    }) {
        assert!(Instant::now() < deadline, "twenty pipe deliveries stalled");
        thread::sleep(Duration::from_millis(2));
    }
    assert!(
        crate::process_info::sample(std::process::id())
            .unwrap()
            .threads
            <= before
    );
    shared.request_shutdown();
}

#[test]
fn copy_pipe_registry_caps_eight_live_children() {
    let shared = Arc::new(Shared::new(16));
    let _loop = pipe_jobs::Driver::new(&shared);
    for _ in 0..8 {
        shared.spawn_copy_pipe(
            PaneId(1),
            ClientId(1),
            "sleep 60".into(),
            "selection".into(),
        );
    }
    assert_eq!(shared.inner.lock().active_copy_pipes, 8);
    shared.spawn_copy_pipe(
        PaneId(1),
        ClientId(1),
        "sleep 60".into(),
        "selection".into(),
    );
    assert_eq!(shared.inner.lock().active_copy_pipes, 8);
    assert_eq!(COPY_PIPE_TIMEOUT, Duration::from_secs(30));
    drop(_loop);
    assert_eq!(shared.inner.lock().active_copy_pipes, 0);
}

#[test]
fn dropping_unregistered_pipe_jobs_reaps_their_children() {
    let client = pipe_jobs::Client::default();
    let environment = CopyPipeEnvironment {
        variables: Vec::new(),
        default_terminal: "screen".into(),
        tmux: String::new(),
        zz_socket: PathBuf::from("/tmp/e16.sock"),
    };
    let launch = launch_copy_pipe(
        "exec sleep 60",
        String::new(),
        &environment,
        COPY_PIPE_TIMEOUT,
        Box::new(|_| {}),
    )
    .unwrap();
    let pid = rustix::process::Pid::from_child(&launch.child);
    client.launch(launch).unwrap();
    drop(client);
    assert_eq!(
        rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG).unwrap_err(),
        rustix::io::Errno::CHILD
    );
}
