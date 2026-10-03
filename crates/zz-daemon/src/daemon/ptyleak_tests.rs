use super::*;

struct Probe {
    terminal: Weak<TerminalSession>,
    pid: rustix::process::Pid,
}

fn run(shared: &Arc<Shared>, control: ClientId, args: &[&str]) {
    shared
        .execute(
            control,
            ClientKind::Control,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap_or_else(|error| panic!("{args:?}: {error}"));
}

fn session_id(shared: &Shared, name: &str) -> SessionId {
    shared
        .inner
        .lock()
        .engine
        .state
        .sessions
        .iter()
        .find(|(_, session)| session.name == name)
        .map(|(id, _)| *id)
        .expect("session")
}

fn fixture(name: &str) -> (Arc<Shared>, ClientId) {
    let shared = Arc::new(Shared::new(1));
    let (control, _) =
        shared.register_subscribed(ClientKind::Control, None, None, OutboundMailbox::new());
    run(
        &shared,
        control,
        &["new-session", "-d", "-s", name, "exec cat"],
    );
    shared
        .attach(control, session_id(&shared, name))
        .expect("control attach");
    (shared, control)
}

fn panes(shared: &Shared) -> BTreeSet<PaneId> {
    shared.inner.lock().terminals.keys().copied().collect()
}

fn probe(shared: &Shared, pane: PaneId) -> Probe {
    let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
    assert!(terminal.wait_for_identity(Duration::from_secs(5)));
    let pid = terminal.process_id().expect("pane child pid");
    Probe {
        terminal: Arc::downgrade(&terminal),
        pid: rustix::process::Pid::from_raw(pid as i32).expect("pid"),
    }
}

fn assert_tapped(shared: &Shared, pane: PaneId) {
    assert!(output_routed(&shared.inner.lock(), pane));
}

fn created(shared: &Shared, before: &BTreeSet<PaneId>) -> (PaneId, Probe) {
    let pane = *panes(shared)
        .difference(before)
        .next()
        .expect("new terminal pane");
    (pane, probe(shared, pane))
}

fn assert_released(shared: &Arc<Shared>, probes: &[Probe]) {
    let alive = || {
        probes
            .iter()
            .filter(|probe| {
                probe.terminal.strong_count() != 0
                    || rustix::process::test_kill_process(probe.pid).is_ok()
            })
            .count()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive() != 0 {
        assert!(
            Instant::now() < deadline,
            "{} of {} removed panes still hold their terminal",
            alive(),
            probes.len()
        );
        shared.terminal_requests.turn(shared);
        shared.turn_control_output(true);
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn removed_panes_release_their_terminals_with_a_control_client_attached() {
    let (shared, control) = fixture("ptyleak");
    let mut probes = Vec::new();
    for _ in 0..5 {
        let before = panes(&shared);
        run(
            &shared,
            control,
            &["new-window", "-d", "-n", "cw", "exec cat"],
        );
        let (pane, window) = created(&shared, &before);
        probes.push(window);
        assert_tapped(&shared, pane);
        run(&shared, control, &["kill-window", "-t", "ptyleak:cw"]);

        let before = panes(&shared);
        run(
            &shared,
            control,
            &["split-window", "-d", "-t", "ptyleak:0", "exec cat"],
        );
        let (pane, split) = created(&shared, &before);
        probes.push(split);
        assert_tapped(&shared, pane);
        run(&shared, control, &["kill-pane", "-t", &pane.to_string()]);

        let before = panes(&shared);
        run(
            &shared,
            control,
            &["new-window", "-d", "-n", "rw", "exec cat"],
        );
        let (pane, replaced) = created(&shared, &before);
        probes.push(replaced);
        assert_tapped(&shared, pane);
        run(
            &shared,
            control,
            &["respawn-pane", "-k", "-t", "ptyleak:rw", "exec cat"],
        );
        probes.push(probe(&shared, pane));
        run(&shared, control, &["kill-window", "-t", "ptyleak:rw"]);

        let before = panes(&shared);
        run(
            &shared,
            control,
            &["new-session", "-d", "-s", "gone", "exec cat"],
        );
        let (pane, session) = created(&shared, &before);
        probes.push(session);
        shared
            .attach(control, session_id(&shared, "gone"))
            .expect("control attach");
        assert_tapped(&shared, pane);
        run(&shared, control, &["kill-session", "-t", "gone"]);
        shared
            .attach(control, session_id(&shared, "ptyleak"))
            .expect("control attach");

        let before = panes(&shared);
        run(
            &shared,
            control,
            &["new-window", "-d", "-n", "ew", "read line; exit 0"],
        );
        let (pane, exited) = created(&shared, &before);
        probes.push(exited);
        assert_tapped(&shared, pane);
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        assert!(terminal.send_raw_input(Arc::from(&b"\r"[..])));
        drop(terminal);
    }
    assert_released(&shared, &probes);
    assert_eq!(panes(&shared).len(), 1);
    shared.request_shutdown();
}
