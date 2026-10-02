use super::*;
use zz_protocol::decode_protocol_frame;

const MARK: &str = "ATTACHFRAMESMARK";

fn run(shared: &Arc<Shared>, context: &mut ExecutionContext, name: &str, args: &[&str]) {
    shared
        .execute(
            ClientId(900),
            ClientKind::Command,
            context,
            &CommandInvocation::new(name, args.iter().copied()),
        )
        .unwrap();
}

fn viewport_text(viewport: &TerminalViewport) -> String {
    viewport
        .cells
        .iter()
        .map(|cell| viewport.cell_text(*cell))
        .collect()
}

fn wait_for_marks(shared: &Shared, panes: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let marked = shared
            .inner
            .lock()
            .terminals
            .values()
            .filter(|terminal| viewport_text(&terminal.latest_viewport()).contains(MARK))
            .count();
        if marked == panes {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{marked} of {panes} panes printed"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn hello(session: &str) -> ProtocolMessage {
    let mut hello = zz_protocol::Hello::from_client(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(931),
        kind: ClientKind::Interactive,
        device_name: None,
        capabilities: vec![
            zz_protocol::PANE_FRAME_CAPABILITY.to_owned(),
            ClientHello::CLIENT_TERMINAL_CAPABILITY.to_owned(),
        ],
        color_scheme: None,
        origin: None,
        working_directory: None,
        environment: Vec::new(),
        process_id: std::process::id(),
    });
    hello.subscriptions = zz_protocol::Subscriptions::terminal();
    hello.viewport = Some(zz_protocol::ClientViewport {
        columns: 180,
        rows: 50,
        cell_width_px: 8,
        cell_height_px: 16,
    });
    hello.attach = Some(zz_protocol::AttachOperation::Commands(vec![
        PreparedCommand {
            invocation: CommandInvocation::new("attach-session", ["-t", session]),
            canonical_name: Some("attach-session".to_owned()),
            alias_matched: false,
            result: PreparedCommandResult::Ready,
        },
    ]));
    ProtocolMessage::Hello(hello)
}

fn viewports(message: &ProtocolMessage) -> usize {
    let children = match message {
        ProtocolMessage::Batch(batch) => batch.messages().unwrap(),
        message => vec![message.clone()],
    };
    children
        .iter()
        .filter(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::TerminalViewport { .. },
                    ..
                })
            )
        })
        .count()
}

fn drain(shared: &Arc<Shared>, outbound: &OutboundMailbox, panes: usize) -> Vec<ProtocolMessage> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut frames = Vec::new();
    let mut quiet = None;
    while quiet.is_none_or(|since: Instant| since.elapsed() < Duration::from_millis(100)) {
        assert!(Instant::now() < deadline, "{:?}", summary(&frames));
        shared.terminal_requests.turn(shared);
        while let Some(frame) = pop_ready_frame(&mut outbound.state.lock()) {
            frames.push(decode_protocol_frame(&frame).unwrap());
        }
        if quiet.is_none() && frames.iter().map(viewports).sum::<usize>() >= panes {
            quiet = Some(Instant::now());
        }
        thread::sleep(Duration::from_millis(1));
    }
    frames
}

fn summary(frames: &[ProtocolMessage]) -> Vec<String> {
    frames
        .iter()
        .map(|frame| match frame {
            ProtocolMessage::Batch(batch) => format!(
                "batch of {} with {} panes",
                batch.messages().unwrap().len(),
                viewports(frame)
            ),
            frame => format!("{frame:?}").chars().take(60).collect(),
        })
        .collect()
}

fn settled(
    shared: &Arc<Shared>,
    outbound: &OutboundMailbox,
    panes: usize,
    attach: impl FnOnce(),
) -> Vec<ProtocolMessage> {
    shared
        .terminal_requests
        .paused
        .store(true, Ordering::Release);
    attach();
    let early = drain(shared, outbound, 0);
    shared
        .terminal_requests
        .paused
        .store(false, Ordering::Release);
    assert!(
        early.is_empty(),
        "written before its panes settled: {:?}",
        summary(&early)
    );
    drain(shared, outbound, panes)
}

fn frames_session(
    shared: &Arc<Shared>,
    context: &mut ExecutionContext,
    name: &str,
    live: usize,
    dead: usize,
) {
    let hold = format!("echo {MARK}; exec sleep 1000000");
    let exit = format!("echo {MARK}; sleep 0.2");
    run(
        shared,
        context,
        "set-option",
        &["-g", "remain-on-exit", "on"],
    );
    run(
        shared,
        context,
        "new-session",
        &["-d", "-s", name, "-x", "180", "-y", "50", &hold],
    );
    let target = format!("{name}:0");
    for index in 1..live + dead {
        let command = if index < live { &hold } else { &exit };
        run(
            shared,
            context,
            "split-window",
            &["-d", "-t", &target, command],
        );
        run(shared, context, "select-layout", &["-t", &target, "tiled"]);
    }
    wait_for_marks(shared, live + dead);
    let deadline = Instant::now() + Duration::from_secs(5);
    while {
        let inner = shared.inner.lock();
        inner
            .terminals
            .keys()
            .filter(|pane| {
                inner
                    .engine
                    .state
                    .pane(**pane)
                    .is_some_and(|pane| pane.dead)
            })
            .count()
            < dead
    } {
        assert!(Instant::now() < deadline, "panes never died");
        thread::sleep(Duration::from_millis(5));
    }
}

fn register(shared: &Arc<Shared>, session: &str) -> (connection::Session, Arc<OutboundMailbox>) {
    let outbound = OutboundMailbox::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let connection = connection::Session::register(shared, hello(session), &outbound, &cancel)
        .unwrap()
        .unwrap();
    (connection, outbound)
}

#[test]
fn an_attach_writes_every_pane_in_its_initial_batch() {
    let shared = Arc::new(Shared::new(931));
    let mut context = ExecutionContext::default();
    frames_session(&shared, &mut context, "frames", 4, 0);
    shared.loop_active.store(true, Ordering::Release);
    let (mut connection, outbound) = register(&shared, "frames");
    let attached = settled(&shared, &outbound, 4, || {
        connection.initialize(&shared, &outbound);
    });
    assert_eq!(attached.len(), 2, "{:?}", summary(&attached));
    assert!(matches!(attached[0], ProtocolMessage::Welcome(_)));
    assert_eq!(viewports(&attached[1]), 4, "{:?}", summary(&attached));
    let reattached = settled(&shared, &outbound, 4, || {
        shared
            .execute(
                connection.client,
                ClientKind::Interactive,
                &mut ExecutionContext::default(),
                &CommandInvocation::new("attach-session", ["-t", "frames"]),
            )
            .unwrap();
    });
    assert_eq!(reattached.len(), 1, "{:?}", summary(&reattached));
    assert_eq!(viewports(&reattached[0]), 4, "{:?}", summary(&reattached));
    shared.loop_active.store(false, Ordering::Release);
    drop(connection);
    run(&shared, &mut context, "kill-session", &["-t", "frames"]);
}

#[test]
fn an_attach_repaints_a_dead_pane_kept_by_remain_on_exit() {
    let shared = Arc::new(Shared::new(932));
    let mut context = ExecutionContext::default();
    frames_session(&shared, &mut context, "remains", 1, 1);
    let dead = {
        let inner = shared.inner.lock();
        inner
            .terminals
            .iter()
            .find(|(pane, _)| {
                inner
                    .engine
                    .state
                    .pane(**pane)
                    .is_some_and(|pane| pane.dead)
            })
            .map(|(_, terminal)| Arc::clone(terminal))
            .unwrap()
    };
    dead.terminate();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut probe = dead.settle_request(Arc::new(|| {}));
    while !matches!(
        probe.poll(Instant::now()),
        Some(Err(zz_terminal::TerminalRequestError::ActorStopped))
    ) {
        assert!(
            Instant::now() < deadline,
            "the dead pane's actor never stopped"
        );
        thread::sleep(Duration::from_millis(5));
        probe = dead.settle_request(Arc::new(|| {}));
    }
    shared.loop_active.store(true, Ordering::Release);
    let (mut connection, outbound) = register(&shared, "remains");
    let attached = settled(&shared, &outbound, 2, || {
        connection.initialize(&shared, &outbound);
    });
    assert_eq!(attached.len(), 2, "{:?}", summary(&attached));
    assert_eq!(viewports(&attached[1]), 2, "{:?}", summary(&attached));
    shared.loop_active.store(false, Ordering::Release);
    drop(connection);
    run(&shared, &mut context, "kill-session", &["-t", "remains"]);
}

#[test]
fn an_attach_flushes_at_its_bound_when_its_panes_never_settle() {
    let shared = Arc::new(Shared::new(933));
    let mut context = ExecutionContext::default();
    frames_session(&shared, &mut context, "bounded", 2, 0);
    shared.start_timers().unwrap();
    let (mut connection, outbound) = register(&shared, "bounded");
    shared
        .terminal_requests
        .paused
        .store(true, Ordering::Release);
    let started = Instant::now();
    connection.initialize(&shared, &outbound);
    let deadline = started + Duration::from_secs(3);
    let mut early = Vec::new();
    while early.is_empty() {
        assert!(Instant::now() < deadline, "the attach never flushed");
        while let Some(frame) = pop_ready_frame(&mut outbound.state.lock()) {
            early.push(decode_protocol_frame(&frame).unwrap());
        }
        thread::sleep(Duration::from_millis(1));
    }
    let flushed = started.elapsed();
    shared
        .terminal_requests
        .paused
        .store(false, Ordering::Release);
    assert!(flushed >= ATTACH_SETTLE_BOUND, "flushed after {flushed:?}");
    assert_eq!(early.len(), 2, "{:?}", summary(&early));
    assert!(matches!(early[0], ProtocolMessage::Welcome(_)));
    let ready = viewports(&early[1]);
    let late = drain(&shared, &outbound, 2 - ready);
    assert_eq!(
        ready + late.iter().map(viewports).sum::<usize>(),
        2,
        "{:?} then {:?}",
        summary(&early),
        summary(&late)
    );
    drop(connection);
    run(&shared, &mut context, "kill-session", &["-t", "bounded"]);
    shared.request_shutdown();
}
