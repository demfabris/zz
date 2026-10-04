use super::*;

const CHILD: &str = "ZZ_TEST_PEER_SKIP";

fn backdate(path: &Path) {
    fs::File::open(path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(30))
        .unwrap();
}

fn probe(
    shared: &Arc<Shared>,
    completed: &crossbeam_channel::Sender<TimerCompletion>,
    completions: &crossbeam_channel::Receiver<TimerCompletion>,
) -> bool {
    let started = shared.start_peer_scan(completed);
    if started {
        completions.recv_timeout(Duration::from_secs(10)).unwrap();
        let result = shared
            .helpers
            .results
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        shared.apply_helper_result(result);
    }
    started
}

fn record(pane: &str, status: &str) -> Vec<u8> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    serde_json::to_vec(&serde_json::json!({
        "pid": std::process::id(), "sessionId": "peer-skip", "cwd": "/",
        "startedAt": now, "procStart": "", "version": "2.1.273", "peerProtocol": 1,
        "peerFeatures": [], "kind": "interactive", "entrypoint": "cli", "pidDomain": "linux",
        "tmux": format!("peerskip:@0.{pane}"), "messagingSocketPath": "/tmp/cc-socks/peerskip.sock",
        "name": "peer-skip", "status": status, "updatedAt": now, "statusUpdatedAt": now
    }))
    .unwrap()
}

#[test]
fn an_unchanged_registry_skips_the_peer_scan_and_any_change_runs_it() {
    let Some(root) = std::env::var_os(CHILD).map(PathBuf::from) else {
        let directory = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "daemon::timers::peerskip_tests::an_unchanged_registry_skips_the_peer_scan_and_any_change_runs_it",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, directory.path())
            .env("CLAUDE_CONFIG_DIR", directory.path())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("isolated peer skip test stalled");
            }
            thread::sleep(Duration::from_millis(10));
        }
    };
    let sessions = root.join("sessions");
    let shared = Arc::new(Shared::new(1));
    let pane = shared
        .inner
        .lock()
        .engine
        .state
        .create_session("peerskip")
        .unwrap()
        .2;
    let set_pid = |pid: u32| {
        shared.inner.lock().engine.set_pane_runtime_facts(
            pane,
            PaneRuntimeFacts {
                pid: Some(pid),
                ..PaneRuntimeFacts::default()
            },
        );
    };
    set_pid(std::process::id());
    let state = || shared.inner.lock().claude_peer_states.get(&pane).cloned();
    let (completed, completions) = crossbeam_channel::unbounded();
    let probe = || probe(&shared, &completed, &completions);

    assert!(probe());
    assert!(!probe());
    assert!(!probe());

    fs::create_dir(&sessions).unwrap();
    assert!(probe());
    assert!(probe());
    backdate(&sessions);
    assert!(probe());
    assert!(!probe());

    let file = sessions.join(format!("{}.json", std::process::id()));
    fs::write(&file, record("%99", "busy")).unwrap();
    backdate(&file);
    backdate(&sessions);
    assert!(probe());
    assert_eq!(state(), None);
    assert!(!probe());
    set_pid(1);
    assert!(probe());
    assert!(!probe());
    set_pid(std::process::id());
    assert!(probe());
    assert!(!probe());

    let option = |args: &[&str]| {
        shared
            .inner
            .lock()
            .engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new("set-option", args.iter().copied()),
            )
            .unwrap();
    };
    let target = pane.to_string();
    option(&["-p", "-t", &target, "@agent-peer-state", "off"]);
    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&file)
        .unwrap()
        .write_all(&record(&target, "busy"))
        .unwrap();
    assert!(probe());
    assert_eq!(state(), None);
    backdate(&file);
    assert!(probe());
    assert!(probe());
    assert_eq!(state(), None);
    option(&["-p", "-u", "-t", &target, "@agent-peer-state"]);
    assert!(probe());
    assert_eq!(state().as_deref(), Some("working"));
    assert!(probe());
    assert!(probe());

    fs::write(&file, record("%99", "busy")).unwrap();
    backdate(&file);
    backdate(&sessions);
    assert!(probe());
    assert_eq!(state(), None);
    assert!(!probe());

    fs::remove_file(&file).unwrap();
    assert!(probe());
    backdate(&sessions);
    assert!(probe());
    assert!(!probe());

    set_pid(1);
    assert!(!probe());
    set_pid(std::process::id());
    assert!(!probe());
    shared.request_shutdown();
}
