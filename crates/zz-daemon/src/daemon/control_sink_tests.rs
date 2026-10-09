use super::tests::take_reliable_messages;
use super::*;

#[derive(Debug)]
enum Seen {
    Output(PaneId, Vec<u8>),
    Closed(String),
    Hook(String),
    Reply,
    Exit,
}

fn run(shared: &Arc<Shared>, client: ClientId, args: &[&str]) {
    shared
        .execute(
            client,
            ClientKind::Control,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap_or_else(|error| panic!("{args:?}: {error}"));
}

fn session(shared: &Shared, name: &str) -> (SessionId, PaneId, WindowId) {
    let inner = shared.inner.lock();
    let (id, session) = inner
        .engine
        .state
        .sessions
        .iter()
        .find(|(_, session)| session.name == name)
        .expect("session");
    let window = session.active_window;
    (*id, inner.engine.state.windows[&window].active_pane, window)
}

fn control(shared: &Arc<Shared>) -> (ClientId, Arc<OutboundMailbox>) {
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Control, None, None, Arc::clone(&mailbox));
    (client, mailbox)
}

fn observe(message: ProtocolMessage, seen: &mut Vec<Seen>) {
    match message {
        ProtocolMessage::Batch(batch) => {
            for message in batch.messages().expect("batch messages") {
                observe(message, seen);
            }
        }
        ProtocolMessage::Event(Event { payload, .. }) => match payload {
            EventPayload::PaneOutput { pane, bytes }
            | EventPayload::PaneOutputAged { pane, bytes, .. } => {
                seen.push(Seen::Output(pane, bytes));
            }
            EventPayload::HookEvent { name, variables } if name == "window-unlinked" => {
                seen.push(Seen::Closed(
                    variables.get("hook_window").cloned().unwrap_or_default(),
                ));
            }
            EventPayload::HookEvent { name, .. } => seen.push(Seen::Hook(name)),
            EventPayload::Detached { .. } | EventPayload::ControlExit { .. } => {
                seen.push(Seen::Exit);
            }
            _ => {}
        },
        ProtocolMessage::CommandResponse(_) => seen.push(Seen::Reply),
        _ => {}
    }
}

fn collect(mailbox: &OutboundMailbox, seen: &mut Vec<Seen>) {
    for message in take_reliable_messages(mailbox) {
        observe(message, seen);
    }
}

fn wait_until(
    shared: &Shared,
    mailbox: &OutboundMailbox,
    seen: &mut Vec<Seen>,
    mut done: impl FnMut(&[Seen]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        shared.pump_control_output_at(Instant::now());
        collect(mailbox, seen);
        if done(seen) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "control stream stalled: {seen:?} open {} lines {}",
            mailbox.is_open(),
            mailbox
                .control_feed()
                .map_or(0, shard_sink::ControlFeed::queued_lines)
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn output_of(seen: &[Seen], pane: PaneId) -> Vec<u8> {
    seen.iter()
        .filter_map(|seen| match seen {
            Seen::Output(from, bytes) if *from == pane => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}

fn last_output(seen: &[Seen], pane: PaneId) -> Option<usize> {
    seen.iter()
        .rposition(|seen| matches!(seen, Seen::Output(from, _) if *from == pane))
}

#[test]
fn final_output_of_a_pane_that_prints_and_exits_in_one_read_precedes_exit() {
    let shared = Arc::new(Shared::new(1));
    let (client, mailbox) = control(&shared);
    for round in 0..100 {
        let name = format!("exit-{round}");
        let marker = format!("final-{round}");
        let command = format!("read _; printf {marker}");
        run(
            &shared,
            client,
            &["new-session", "-d", "-s", &name, &command],
        );
        let (id, pane, _) = session(&shared, &name);
        shared.attach(client, id).expect("control attach");
        take_reliable_messages(&mailbox);
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        assert!(output_routed(&shared.inner.lock(), pane));
        assert!(terminal.send_raw_input(Arc::from(b"\n".as_slice())));
        let mut seen = Vec::new();
        wait_until(&shared, &mailbox, &mut seen, |seen| {
            seen.iter().any(|seen| matches!(seen, Seen::Exit))
        });
        let exit = seen
            .iter()
            .position(|seen| matches!(seen, Seen::Exit))
            .expect("exit");
        let output = last_output(&seen, pane);
        assert!(
            output.is_some_and(|output| output < exit),
            "round {round}: %output must precede %exit: {seen:?}"
        );
        let delivered = output_of(&seen, pane);
        assert!(
            delivered
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()),
            "round {round}: final bytes missing: {seen:?}"
        );
        assert!(seen.iter().all(|entry| match entry {
            Seen::Closed(_) => output.is_some_and(|output| output < exit),
            _ => true,
        }));
        let deadline = Instant::now() + Duration::from_secs(10);
        while client_attached_session(&shared.inner.lock(), client).is_some()
            || shared.inner.lock().terminals.contains_key(&pane)
        {
            assert!(
                Instant::now() < deadline,
                "round {round}: client stayed attached"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn output_read_before_kill_pane_precedes_the_window_close() {
    let shared = Arc::new(Shared::new(1));
    let (client, mailbox) = control(&shared);
    run(
        &shared,
        client,
        &["new-session", "-d", "-s", "kill-after", "exec cat"],
    );
    let (id, _, _) = session(&shared, "kill-after");
    shared.attach(client, id).expect("control attach");
    for round in 0..20 {
        let before = shared
            .inner
            .lock()
            .terminals
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        let marker = format!("killed-{round}");
        let command = format!("read _; printf {marker}; exec cat");
        run(
            &shared,
            client,
            &["new-window", "-d", "-t", "kill-after:", &command],
        );
        let pane = *shared
            .inner
            .lock()
            .terminals
            .keys()
            .find(|pane| !before.contains(pane))
            .expect("new pane");
        let window = shared
            .inner
            .lock()
            .engine
            .state
            .window_for_pane(pane)
            .expect("pane window");
        assert!(output_routed(&shared.inner.lock(), pane));
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        assert!(terminal.send_raw_input(Arc::from(b"\n".as_slice())));
        let feed = mailbox.control_feed().expect("control feed");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !feed
            .queued_bytes(pane)
            .windows(marker.len())
            .any(|bytes| bytes == marker.as_bytes())
        {
            assert!(Instant::now() < deadline, "round {round}: no output read");
            thread::sleep(Duration::from_millis(1));
        }
        let mut seen = Vec::new();
        collect(&mailbox, &mut seen);
        run(&shared, client, &["kill-pane", "-t", &pane.to_string()]);
        let window = window.to_string();
        wait_until(&shared, &mailbox, &mut seen, |seen| {
            seen.iter()
                .any(|seen| matches!(seen, Seen::Closed(closed) if *closed == window))
        });
        let closed = seen
            .iter()
            .position(|seen| matches!(seen, Seen::Closed(closed) if *closed == window))
            .expect("window close");
        assert!(
            last_output(&seen, pane).is_some_and(|output| output < closed),
            "round {round}: %output must precede %window-close: {seen:?}"
        );
        assert!(
            output_of(&seen, pane)
                .windows(marker.len())
                .any(|bytes| bytes == marker.as_bytes())
        );
    }
}

#[test]
fn a_full_control_queue_parks_the_pane_without_dropping_bytes() {
    let shared = Arc::new(Shared::new(1));
    let (client, mailbox) = control(&shared);
    let total = 300_000;
    let command = format!("read _; head -c {total} /dev/zero | tr '\\0' x; printf END; exec cat");
    run(
        &shared,
        client,
        &["new-session", "-d", "-s", "parked", &command],
    );
    let (id, pane, _) = session(&shared, "parked");
    shared.attach(client, id).expect("control attach");
    let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
    assert!(terminal.send_raw_input(Arc::from(b"\n".as_slice())));
    let feed = mailbox.control_feed().expect("control feed");
    let mut seen = Vec::new();
    let mut peak = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !output_of(&seen, pane).ends_with(b"END") {
        assert!(Instant::now() < deadline, "parked pane never finished");
        thread::sleep(Duration::from_millis(5));
        let queued = feed.queued(pane);
        assert!(queued <= CONTROL_PENDING_CHUNKS_PER_PANE);
        peak = peak.max(queued);
        shared.pump_control_output_at(Instant::now());
        collect(&mailbox, &mut seen);
    }
    assert_eq!(peak, CONTROL_PENDING_CHUNKS_PER_PANE);
    let delivered = output_of(&seen, pane);
    let start = delivered
        .iter()
        .position(|byte| *byte == b'x')
        .expect("payload");
    let mut expected = vec![b'x'; total];
    expected.extend_from_slice(b"END");
    assert_eq!(delivered[start..], expected[..]);
}

fn four_panes(shared: &Arc<Shared>, client: ClientId, name: &str, command: &str) -> Vec<PaneId> {
    run(shared, client, &["new-session", "-d", "-s", name, command]);
    let target = format!("{name}:");
    for _ in 0..3 {
        run(
            shared,
            client,
            &["new-window", "-d", "-t", &target, command],
        );
    }
    let (id, _, _) = session(shared, name);
    shared.attach(client, id).expect("control attach");
    let inner = shared.inner.lock();
    inner.engine.state.sessions[&id]
        .windows
        .iter()
        .map(|window| inner.engine.state.windows[window].active_pane)
        .collect()
}

fn renamed(seen: &Seen, round: usize) -> bool {
    matches!(seen, Seen::Hook(name) if *name == format!("window-renamed-{round}"))
}

#[test]
fn rename_hooks_behind_four_full_panes_wait_in_order_without_closing_the_client() {
    let shared = Arc::new(Shared::new(1));
    let (client, mailbox) = control(&shared);
    let panes = four_panes(&shared, client, "renamed", "exec cat");
    take_reliable_messages(&mailbox);
    let feed = mailbox.control_feed().expect("control feed");
    let chunk = vec![b'r'; 64 * 1024];
    for round in 0..2 {
        for pane in &panes {
            for _ in 0..CONTROL_PENDING_CHUNKS_PER_PANE {
                feed.queue_at(*pane, &chunk, Instant::now());
            }
        }
        let hook = Shared::event(EventPayload::HookEvent {
            name: format!("window-renamed-{round}"),
            variables: BTreeMap::new(),
        });
        assert!(mailbox.enqueue_reliable(&hook));
        assert!(feed.queued_lines() > round);
    }
    let mut seen = Vec::new();
    collect(&mailbox, &mut seen);
    assert!(!seen.iter().any(|seen| renamed(seen, 0) || renamed(seen, 1)));
    let mut pumps = 0;
    while !seen.iter().any(|seen| renamed(seen, 1)) {
        assert!(mailbox.is_open(), "client closed after {pumps} pumps");
        pumps += 1;
        assert!(pumps < 100_000, "hooks never delivered");
        shared.pump_control_output_at(Instant::now());
        let (_, queued) = mailbox.queued_reliable().expect("open mailbox");
        assert!(
            queued < CONTROL_PENDING_MESSAGE_LIMIT + panes.len() + 4,
            "pump queued {queued} messages"
        );
        collect(&mailbox, &mut seen);
    }
    assert!(mailbox.is_open());
    for round in 0..2 {
        let at = seen
            .iter()
            .position(|seen| renamed(seen, round))
            .expect("hook");
        for pane in &panes {
            assert_eq!(
                output_of(&seen[..at], *pane).len(),
                (round + 1) * CONTROL_PENDING_CHUNKS_PER_PANE * chunk.len()
            );
        }
    }
}

fn raw_request(line: &str) -> zz_protocol::ExecRequest {
    zz_protocol::ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: zz_protocol::ExecFlags::default(),
        client_instance_id: ClientInstanceId(920),
        origin: None,
        working_directory: None,
        tty: None,
        size: None,
        features: 0,
        startup_reentry: None,
        spawned_server_id: None,
        expect_server_id: None,
        process_id: std::process::id(),
        environment: ClientEnvironmentBlob::default(),
        commands: Vec::new(),
        raw_control_line: Some(line.to_owned()),
    }
}

#[test]
fn control_replies_keep_arriving_while_four_panes_flood() {
    let shared = Arc::new(Shared::new(1));
    let (client, mailbox) = control(&shared);
    let panes = four_panes(
        &shared,
        client,
        "flood",
        "exec yes flood-flood-flood-flood-flood-flood-flood-flood",
    );
    let feed = mailbox.control_feed().expect("control feed");
    let started = Instant::now();
    while panes
        .iter()
        .any(|pane| feed.queued(*pane) < CONTROL_PENDING_CHUNKS_PER_PANE)
    {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "panes never filled"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let mut context = ExecutionContext::default();
    let mut delivered = BTreeMap::<PaneId, usize>::new();
    let mut slowest = Duration::ZERO;
    for round in 0..20 {
        let sent = Instant::now();
        shared.execute_compact_request(
            client,
            ClientKind::Control,
            &mut context,
            raw_request("display-message -p tick"),
            &mailbox,
        );
        let mut replied = false;
        while !replied {
            assert!(mailbox.is_open(), "round {round}: client closed");
            assert!(
                sent.elapsed() < Duration::from_secs(5),
                "round {round}: no reply"
            );
            thread::sleep(Duration::from_millis(1));
            shared.pump_control_output_at(Instant::now());
            let mut seen = Vec::new();
            collect(&mailbox, &mut seen);
            for entry in seen {
                match entry {
                    Seen::Output(pane, bytes) => *delivered.entry(pane).or_default() += bytes.len(),
                    Seen::Reply => replied = true,
                    _ => {}
                }
            }
        }
        slowest = slowest.max(sent.elapsed());
    }
    run(&shared, client, &["kill-session", "-t", "flood"]);
    assert!(
        slowest < Duration::from_secs(5),
        "slowest reply {slowest:?}"
    );
    assert!(
        panes
            .iter()
            .all(|pane| delivered.get(pane).is_some_and(|bytes| *bytes > 0)),
        "{delivered:?}"
    );
}

#[cfg(target_os = "linux")]
fn thread_names() -> Vec<String> {
    let mut names = std::fs::read_dir("/proc/self/task")
        .expect("process threads")
        .filter_map(Result::ok)
        .filter_map(|task| std::fs::read_to_string(task.path().join("comm")).ok())
        .map(|name| name.trim_end().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
fn thread_names() -> Vec<String> {
    const PROC_PIDLISTTHREADS: libc::c_int = 6;
    let pid = libc::pid_t::try_from(std::process::id()).expect("pid");
    let mut handles = vec![0_u64; 4096];
    let capacity = libc::c_int::try_from(handles.len() * 8).expect("buffer size");
    let listed = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDLISTTHREADS,
            0,
            handles.as_mut_ptr().cast(),
            capacity,
        )
    };
    handles.truncate(usize::try_from(listed).unwrap_or(0) / 8);
    let size = std::mem::size_of::<libc::proc_threadinfo>();
    let mut names = handles
        .into_iter()
        .filter_map(|handle| {
            let mut info = std::mem::MaybeUninit::<libc::proc_threadinfo>::zeroed();
            let read = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTHREADINFO,
                    handle,
                    info.as_mut_ptr().cast(),
                    libc::c_int::try_from(size).ok()?,
                )
            };
            (usize::try_from(read).ok()? == size).then(|| {
                let info = unsafe { info.assume_init() };
                unsafe { std::ffi::CStr::from_ptr(info.pth_name.as_ptr()) }
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn a_control_client_streaming_output_adds_no_threads() {
    if !crate::daemon::solo_tests::rerun_alone(
        "daemon::control_sink_tests::a_control_client_streaming_output_adds_no_threads",
    ) {
        return;
    }
    let shared = Arc::new(Shared::new(1));
    let (client, mailbox) = control(&shared);
    run(
        &shared,
        client,
        &["new-session", "-d", "-s", "threads", "exec cat"],
    );
    let (id, pane, _) = session(&shared, "threads");
    let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
    let stream = |line: &[u8]| {
        shared.attach(client, id).expect("control attach");
        assert!(terminal.send_raw_input(Arc::from(line)));
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !output_of(&seen, pane)
            .windows(line.len() - 1)
            .any(|bytes| bytes == &line[..line.len() - 1])
        {
            assert!(Instant::now() < deadline, "no control output: {seen:?}");
            thread::sleep(Duration::from_millis(5));
            shared.pump_control_output_at(Instant::now());
            collect(&mailbox, &mut seen);
        }
    };
    stream(b"warm\n");
    shared.detach(client);
    let before = thread_names();
    stream(b"measured\n");
    let after = thread_names();
    assert!(after.len() <= before.len(), "{before:?} -> {after:?}");
    assert!(
        after.iter().all(|name| before.contains(name)),
        "{before:?} -> {after:?}"
    );
    assert!(after.iter().all(|name| !name.starts_with("zz-control")));
}

fn guard_and_rename_order(message: ProtocolMessage, order: &mut Vec<&'static str>) {
    match message {
        ProtocolMessage::Batch(batch) => {
            for message in batch.messages().expect("batch messages") {
                guard_and_rename_order(message, order);
            }
        }
        ProtocolMessage::Event(Event { payload, .. }) => match payload {
            EventPayload::ControlCommandGuard { .. } | EventPayload::ControlCommandGuardRaw { .. } => {
                order.push("guard");
            }
            EventPayload::HookEvent { name, .. } if name == "window-renamed" => {
                order.push("window-renamed");
            }
            EventPayload::PaneOutputState { paused: true, .. } => order.push("pause"),
            _ => {}
        },
        ProtocolMessage::CommandResponse(_) => order.push("reply"),
        _ => {}
    }
}

#[test]
fn an_inserted_command_holds_its_notifications_until_its_own_guard_closes() {
    let shared = Arc::new(Shared::new(1));
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("new-session", ["-d", "-s", "inserted-order"]),
        )
        .unwrap();
    let (client, mailbox) = control(&shared);
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Control,
            &mut context,
            &CommandInvocation::new("attach-session", ["-t", "inserted-order"]),
        )
        .unwrap();
    take_reliable_messages(&mailbox);
    let command = CommandInvocation::new(
        "run-shell",
        ["-C", "rename-window -t inserted-order:0 renamed"],
    )
    .with_source(SourceSpan {
        source: "<control>".to_owned(),
        line: 1,
        column: 1,
    });
    shared.execute_command_request(client, ClientKind::Control, &mut context, 1, &command);
    let mut order = Vec::new();
    for message in take_reliable_messages(&mailbox) {
        guard_and_rename_order(message, &mut order);
    }
    assert_eq!(order, ["guard", "window-renamed"], "{order:?}");
}

#[test]
fn a_pause_inside_nested_inserted_commands_follows_the_earlier_notification() {
    let shared = Arc::new(Shared::new(1));
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("new-session", ["-d", "-s", "nested-order"]),
        )
        .unwrap();
    let (_, pane, _) = session(&shared, "nested-order");
    let (client, mailbox) = control(&shared);
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Control,
            &mut context,
            &CommandInvocation::new("attach-session", ["-t", "nested-order"]),
        )
        .unwrap();
    take_reliable_messages(&mailbox);
    let inner = format!(
        "run-shell -C {{ rename-window -t nested-order:0 renamed ; refresh-client -A '{pane}:pause' }}"
    );
    let command = CommandInvocation::new("run-shell", ["-C", inner.as_str()]).with_source(
        SourceSpan {
            source: "<control>".to_owned(),
            line: 1,
            column: 1,
        },
    );
    shared.execute_command_request(client, ClientKind::Control, &mut context, 1, &command);
    let mut order = Vec::new();
    for message in take_reliable_messages(&mailbox) {
        guard_and_rename_order(message, &mut order);
    }
    let renamed = order.iter().position(|seen| *seen == "window-renamed");
    let paused = order.iter().position(|seen| *seen == "pause");
    assert!(
        renamed.is_some() && paused.is_some() && renamed < paused,
        "{order:?}"
    );
    assert_eq!(order.last(), Some(&"pause"), "{order:?}");
}
