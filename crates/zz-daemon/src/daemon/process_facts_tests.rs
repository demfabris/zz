use super::*;

fn run(shared: &Arc<Shared>, args: &[&str]) -> String {
    shared
        .execute(
            ClientId(1),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .expect("command")
        .output
        .to_string()
}

fn wait_for(what: &str, mut current: impl FnMut() -> String, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let value = current();
        if value == expected {
            return;
        }
        assert!(Instant::now() < deadline, "{what}: {value:?}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_symlinked_agent_binary_keeps_its_invoked_name() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let versions = directory.path().join("versions");
    fs::create_dir_all(&versions).expect("versions directory");
    std::os::unix::fs::symlink("/bin/bash", versions.join("2.1.99")).expect("versioned binary");
    let agent = directory.path().join("claude");
    std::os::unix::fs::symlink(Path::new("versions").join("2.1.99"), &agent)
        .expect("agent symlink");
    let script = directory.path().join("agent.sh");
    fs::write(
        &script,
        "printf 'ready\\n'\nread -r _\nprintf '\\033]9;4;3\\007working\\n'\nwhile :; do sleep 1; done\n",
    )
    .expect("agent script");

    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    let command = format!("exec '{}' '{}'", agent.display(), script.display());
    shared
        .execute(
            ClientId(1),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "agent", command.as_str()]),
        )
        .expect("session");
    let pane = context.pane.expect("pane").to_string();
    let current_command = || {
        run(
            &shared,
            &[
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{pane_current_command}",
            ],
        )
        .trim()
        .to_owned()
    };
    wait_for(
        "the pane never saw the agent in the foreground",
        current_command,
        "claude",
    );

    run(&shared, &["send-keys", "-t", &pane, "Enter"]);
    wait_for(
        "agent_state never became working",
        || {
            run(
                &shared,
                &["show-options", "-p", "-qv", "-t", &pane, "@agent_state"],
            )
            .trim()
            .to_owned()
        },
        "working",
    );
    assert_eq!(current_command(), "claude");
    shared.request_shutdown();
}

#[cfg(all(feature = "agent", unix))]
#[test]
fn a_pane_without_a_process_never_requests_a_peer_probe() {
    let shared = Arc::new(Shared::new(1));
    let pane = shared
        .inner
        .lock()
        .engine
        .state
        .create_session("s")
        .unwrap()
        .2;
    let terminal = Arc::new(TerminalSession::spawn_empty_pane(
        2000,
        Arc::new(TerminalAppearance::default()),
        None,
    ));
    shared
        .inner
        .lock()
        .terminals_mut()
        .insert(pane, Arc::clone(&terminal));
    assert!(terminal.process_id().is_none());
    shared.synchronize_pane_runtime(pane, &terminal, "", None, true, Instant::now());
    assert!(!shared.peer_probe.load(Ordering::Acquire));
    shared.request_shutdown();
}
