use super::*;

#[test]
fn fanout_keeps_one_shared_encoded_frame() {
    let first = OutboundMailbox::new();
    let second = OutboundMailbox::new();
    let frame: Arc<[u8]> = Arc::from(
        zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync).expect("encode"),
    );
    assert!(first.enqueue_encoded_reliable(Arc::clone(&frame)));
    assert!(second.enqueue_encoded_reliable(Arc::clone(&frame)));
    let first = first.state.lock();
    let second = second.state.lock();
    let (OutboundFrame::Shared(left), OutboundFrame::Shared(right)) =
        (&first.reliable[0], &second.reliable[0])
    else {
        panic!("fanout copied its shared bytes")
    };
    assert!(Arc::ptr_eq(left, right));
}

#[test]
fn compact_batch_flush_keeps_existing_groups_flat() {
    let mailbox = OutboundMailbox::new();
    let child = zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync).expect("encode");
    assert!(mailbox.enqueue_control_group(vec![child]));
    assert!(mailbox.flush_control_batch(false));
    assert!(mailbox.flush_control_batch(false));
    let messages = tests::take_reliable_messages(&mailbox);
    let [ProtocolMessage::Batch(batch)] = messages.as_slice() else {
        panic!("expected one flat batch: {messages:?}")
    };
    assert!(matches!(
        batch.messages().expect("flat group").as_slice(),
        [ProtocolMessage::TreeSync]
    ));
}

#[test]
fn quiet_control_query_sends_one_flat_completion_batch() {
    let shared = Arc::new(Shared::new(50));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .inner
        .lock()
        .client_kinds
        .insert(client, ClientKind::Control);
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut ExecutionContext::default(),
        compact_exec_request(vec![CommandInvocation::new(
            "display-message",
            ["-p", "quiet result"],
        )]),
        &mailbox,
    );
    let messages = tests::take_reliable_messages(&mailbox);
    if !*hook_events::READONLY_SKIP {
        assert_eq!(messages.len(), 2);
        return;
    }
    let [ProtocolMessage::Batch(batch)] = messages.as_slice() else {
        panic!("expected one completion batch: {messages:?}")
    };
    assert!(matches!(
        batch.messages().expect("flat completion").as_slice(),
        [
            ProtocolMessage::Event(Event {
                payload: EventPayload::ControlCommandStarted { request_id: 1, .. },
                ..
            }),
            ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }),
            ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                outcome: zz_protocol::ExecOutcome::Ran,
                ..
            }),
        ] if output.as_bytes() == b"quiet result"
    ));
}

#[test]
fn quiet_control_release_cannot_flush_an_attach_collector() {
    let shared = Arc::new(Shared::new(51));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .client_writers
        .lock()
        .insert(client, Arc::clone(&mailbox));
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_reliable_with_wakeup(&ProtocolMessage::TreeSync, false));
    mailbox.hold_terminals();
    mailbox.release_terminals();
    {
        let mut state = mailbox.state.lock();
        assert!(state.ctrl_collecting == ControlCollection::Quiet);
        assert!(pop_ready_frame(&mut state).is_none());
    }
    mailbox.collect_control_attach();
    assert!(!mailbox.collect_control_query());
    for index in 0..MAX_RELIABLE_MESSAGES / 2 {
        let admitted = if index % 2 == 0 {
            mailbox.enqueue_reliable(&ProtocolMessage::TreeSync)
        } else {
            mailbox.enqueue_control_group(vec![
                zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync).expect("encode"),
            ])
        };
        assert!(admitted);
    }
    mailbox.hold_terminals();
    mailbox.release_terminals();
    shared.wake_control_queue(client, ClientKind::Control);
    mailbox.release_control_query();
    {
        let mut state = mailbox.state.lock();
        assert!(state.ctrl_collecting == ControlCollection::Attach);
        assert!(pop_ready_frame(&mut state).is_none());
    }
    assert!(mailbox.flush_control_batch(false));
    let messages = tests::take_reliable_messages(&mailbox);
    let [ProtocolMessage::Batch(batch)] = messages.as_slice() else {
        panic!("attach collection was split: {messages:?}")
    };
    let children = batch.messages().expect("flat attach collection");
    assert_eq!(children.len(), MAX_RELIABLE_MESSAGES / 2 + 1);
    assert!(
        children
            .iter()
            .all(|message| *message == ProtocolMessage::TreeSync)
    );
}

#[test]
fn quiet_control_close_and_overflow_release_the_held_frames() {
    let mailbox = OutboundMailbox::new();
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_reliable_with_wakeup(&ProtocolMessage::TreeSync, false));
    mailbox.close_after_flush();
    let encoded = mailbox.recv().expect("closed collection drains");
    let ProtocolMessage::Batch(batch) =
        zz_protocol::decode_protocol_frame(&encoded).expect("decode")
    else {
        panic!("expected final collection")
    };
    assert_eq!(
        batch.messages().expect("flat group"),
        [ProtocolMessage::TreeSync]
    );
    assert!(mailbox.recv().is_none());

    let mailbox = OutboundMailbox::new();
    assert!(mailbox.collect_control_query());
    for _ in 0..MAX_RELIABLE_MESSAGES {
        assert!(mailbox.enqueue_reliable_with_wakeup(&ProtocolMessage::TreeSync, false));
    }
    assert!(!mailbox.enqueue_reliable(&ProtocolMessage::TreeSync));
    let encoded = mailbox.recv().expect("overflow collection drains");
    assert!(matches!(
        zz_protocol::decode_protocol_frame(&encoded).expect("decode"),
        ProtocolMessage::Event(Event {
            payload: EventPayload::ControlExit { reason },
            ..
        }) if reason == "too far behind"
    ));
    assert!(mailbox.recv().is_none());
}

#[test]
fn compact_callback_guard_preserves_raw_output() {
    let shared = Arc::new(Shared::new(38));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared.publish_control_command_guard(
        Some((client, 1)),
        RawText::from_bytes(vec![0xff, b'\n']),
        false,
        false,
    );
    let messages = reliable_children(&mailbox);
    assert!(
        matches!(messages.as_slice(), [ProtocolMessage::Event(Event { payload: EventPayload::ControlCommandGuardRaw { output, flags: 1, .. }, .. })] if output.as_bytes() == [0xff, b'\n'])
    );
}

#[test]
fn control_query_wakeup_excludes_hooks_and_pending_events() {
    let shared = Shared::new(47);
    let mut context = ExecutionContext::default();
    let command = |name: &str, args: &[&str]| PreparedCommand {
        invocation: CommandInvocation::new(name, args.iter().copied()),
        canonical_name: Some(name.to_owned()),
        alias_matched: false,
        result: PreparedCommandResult::Ready,
    };
    let query = command("display-message", &["-p", "#{session_name}"]);
    let mut inner = shared.inner.lock();
    for name in [
        "has-session",
        "list-buffers",
        "list-clients",
        "list-commands",
        "list-keys",
        "list-panes",
        "list-sessions",
        "list-windows",
        "show-buffer",
        "show-environment",
        "show-hooks",
        "show-messages",
        "show-options",
        "show-prompt-history",
        "show-window-options",
        "start-server",
    ] {
        assert_eq!(
            ctrl::control_query_can_defer_wakeup(&inner, &context, &command(name, &[])),
            *hook_events::READONLY_SKIP,
            "{name}"
        );
    }
    assert_eq!(
        ctrl::control_query_can_defer_wakeup(&inner, &context, &query),
        *hook_events::READONLY_SKIP
    );
    assert_eq!(
        ctrl::control_query_can_defer_wakeup(&inner, &context, &command("capture-pane", &["-p"]),),
        *hook_events::READONLY_SKIP
    );
    for (name, args) in [
        ("display-message", &["-I"][..]),
        ("display-message", &["-d", "100", "x"][..]),
        ("capture-pane", &[][..]),
        ("wait-for", &["release"][..]),
        ("run-shell", &["sleep 30"][..]),
        ("agent-send", &["--wait", "x"][..]),
        ("capture-browser", &[][..]),
    ] {
        assert!(!ctrl::control_query_can_defer_wakeup(
            &inner,
            &context,
            &command(name, args),
        ));
    }
    for hook in ["after-display-message", "command-error"] {
        inner
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-hook", ["-g", hook, "wait-for release"]),
            )
            .expect("set blocking hook");
        assert!(!ctrl::control_query_can_defer_wakeup(
            &inner, &context, &query,
        ));
        inner
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-hook", ["-gu", hook]),
            )
            .expect("unset hook");
    }
    inner.deferred_event_hooks.push(PendingHookEvent {
        name: "session-renamed",
        context: context.clone(),
        variables: BTreeMap::new(),
        exclude_client: None,
    });
    assert!(!ctrl::control_query_can_defer_wakeup(
        &inner, &context, &query,
    ));
}

#[cfg(unix)]
#[test]
fn control_query_started_is_visible_before_a_blocking_after_hook_releases() {
    use std::os::unix::net::UnixStream;

    let shared = Arc::new(Shared::new(48));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .client_writers
        .lock()
        .insert(client, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new(
                "set-hook",
                ["-g", "after-display-message", "wait-for ctrl-hook-release"],
            ),
        )
        .expect("blocking after hook");
    let (mut reader, mut server) = UnixStream::pair().expect("pair");
    reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read deadline");
    let writer = {
        let mailbox = Arc::clone(&mailbox);
        thread::spawn(move || write_outbound(&mut server, &mailbox, &Weak::new(), client))
    };
    let (finished, completion) = crossbeam_channel::bounded(1);
    let worker = {
        let shared = Arc::clone(&shared);
        let mailbox = Arc::clone(&mailbox);
        thread::spawn(move || {
            shared.execute_compact_request(
                client,
                ClientKind::Control,
                &mut context,
                compact_exec_request(vec![CommandInvocation::new(
                    "display-message",
                    ["-p", "query result"],
                )]),
                &mailbox,
            );
            let _ = finished.send(());
        })
    };
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut reader).expect("visible Started"),
        ProtocolMessage::Event(Event {
            payload: EventPayload::ControlCommandStarted { request_id: 1, .. },
            ..
        })
    ));
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut reader).expect("after hook parked"),
        ProtocolMessage::CommandQueueParked { request_id: 1 }
    ));
    assert!(matches!(
        completion.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
    shared.signal_wait_channel("ctrl-hook-release");
    completion
        .recv_timeout(Duration::from_secs(2))
        .expect("query completed after release");
    worker.join().expect("query worker");
    mailbox.close_after_flush();
    writer.join().expect("socket writer");
}

#[test]
fn a_late_gui_hook_wakes_quiet_started_before_its_reply() {
    let shared = Arc::new(Shared::new(49));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .client_writers
        .lock()
        .insert(client, Arc::clone(&mailbox));
    let gui_mailbox = OutboundMailbox::new();
    let (gui_client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        None,
        None,
        Arc::clone(&gui_mailbox),
    );
    let (session, pane, mut context) = {
        let mut inner = shared.inner.lock();
        let (session, _, pane) = inner
            .engine
            .state
            .create_session("late-gui-hook")
            .expect("model session");
        inner.engine.state.pane_mut(pane).expect("pane").kind = PaneKind::Browser(
            zz_protocol::BrowserDescriptor::single("about:blank".to_owned(), "default".to_owned()),
        );
        let context = ExecutionContext::for_pane(&inner.engine.state, pane).expect("context");
        (session, pane, context)
    };
    shared.attach(gui_client, session).expect("GUI attach");
    reliable_children(&gui_mailbox);
    reliable_children(&mailbox);
    let query = PreparedCommand {
        invocation: CommandInvocation::new("display-message", ["-p", "query result"]),
        canonical_name: Some("display-message".to_owned()),
        alias_matched: false,
        result: PreparedCommandResult::Ready,
    };
    assert!(ctrl::control_query_can_defer_wakeup(
        &shared.inner.lock(),
        &context,
        &query,
    ));
    let (armed, waiting) = crossbeam_channel::bounded(1);
    let (observable, visible) = crossbeam_channel::bounded(1);
    let observer = {
        let mailbox = Arc::clone(&mailbox);
        thread::spawn(move || {
            let mut state = mailbox.state.lock();
            let _ = armed.send(());
            let frame = if mailbox
                .ready
                .wait_for(&mut state, Duration::from_secs(2))
                .timed_out()
            {
                None
            } else {
                pop_ready_frame(&mut state)
            };
            let _ = observable.send(frame);
        })
    };
    waiting
        .recv_timeout(Duration::from_secs(2))
        .expect("observer waiting");
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_reliable_with_wakeup(
        &Shared::event(EventPayload::ControlCommandStarted {
            request_id: 1,
            flags: 1,
            canonical_name: query.canonical_name.clone(),
            guard: true,
        }),
        false,
    ));
    let hook = format!("capture-browser -t {pane} -o /tmp/zz-ctrl-late-hook.png");
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("set-hook", ["-g", "after-display-message", &hook]),
        )
        .expect("install hook after eligibility");
    let worker = {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            shared.run_command_hook(
                client,
                ClientKind::Control,
                &context,
                &query.invocation,
                "after-display-message",
                None,
            )
        })
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let request_id = loop {
        let request =
            reliable_children(&gui_mailbox)
                .into_iter()
                .find_map(|message| match message {
                    ProtocolMessage::Event(Event {
                        payload:
                            EventPayload::BrowserCommand {
                                command: BrowserCommand::Screenshot { request_id, .. },
                                ..
                            },
                        ..
                    }) => Some(request_id),
                    _ => None,
                });
        if request.is_some() || Instant::now() >= deadline {
            break request;
        }
        thread::yield_now();
    };
    let started = visible.recv_timeout(Duration::from_secs(2)).ok().flatten();
    let pending = request_id.is_some_and(|request_id| {
        shared
            .inner
            .lock()
            .pending_gui_requests
            .contains_key(&request_id)
    });
    if let Some(request_id) = request_id {
        shared.complete_gui_request(
            gui_client,
            GuiResponse::Success {
                request_id,
                output: "saved".to_owned(),
            },
        );
    } else {
        shared.fail_gui_requests_for(gui_client);
    }
    worker.join().expect("hook worker");
    observer.join().expect("Started observer");
    assert!(pending, "hook was waiting for its GUI reply");
    let Some(ProtocolMessage::Batch(batch)) =
        started.map(|frame| zz_protocol::decode_protocol_frame(&frame).expect("decode Started"))
    else {
        panic!("late hook did not release the quiet batch")
    };
    assert!(matches!(
        batch.messages().expect("flat Started").as_slice(),
        [ProtocolMessage::Event(Event {
            payload: EventPayload::ControlCommandStarted { request_id: 1, .. },
            ..
        })]
    ));
}

#[test]
fn legacy_resize_accepts_v2_reports_without_a_compact_generation() {
    let shared = Arc::new(Shared::new(43));
    let mailbox = OutboundMailbox::new();
    let (client, _) = shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
    let mut inner = shared.inner.lock();
    assert!(matches!(
        ctrl::normalize_resize(
            &mut inner,
            client,
            InputMessage::ClientTerminalSizeV2 {
                columns: 97,
                rows: 31,
                layout_generation: 0,
            },
        ),
        Some(InputMessage::ClientTerminalSize {
            columns: 97,
            rows: 31
        })
    ));
    assert!(matches!(
        ctrl::normalize_resize(
            &mut inner,
            client,
            InputMessage::ResizeTerminalV2 {
                pane: PaneId(1),
                columns: 97,
                rows: 31,
                cell_width_px: 8,
                cell_height_px: 16,
                layout_generation: 0,
            },
        ),
        Some(InputMessage::ResizeTerminal {
            pane: PaneId(1),
            columns: 97,
            rows: 31,
            cell_width_px: 8,
            cell_height_px: 16,
        })
    ));
    assert!(!inner.ctrl_layouts.contains_key(&client));
}

#[test]
fn stale_layout_size_report_is_dropped_before_geometry_changes() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-resize", "sleep 30"]),
        )
        .expect("session");
    let mailbox = OutboundMailbox::new();
    let (client, _) = shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
    shared
        .attach_target(client, ClientKind::Interactive, &mut context, "ctrl-resize")
        .expect("attach");
    let pane = context.pane.expect("pane");
    {
        let mut inner = shared.inner.lock();
        inner
            .ctrl_subscriptions
            .insert(client, zz_protocol::Subscriptions::terminal());
        ctrl::normalize_resize(
            &mut inner,
            client,
            InputMessage::ClientTerminalSizeV2 {
                columns: 80,
                rows: 24,
                layout_generation: 1,
            },
        )
        .expect("initial generation");
    }
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("split-window", ["-d", "sleep 30"]),
        )
        .expect("split");
    compact_command(
        &shared,
        &mut context,
        "resize-pane",
        &["-Z", "-t", &pane.to_string()],
    );
    shared.compact_tree_messages(client, true);
    let generation = shared.inner.lock().ctrl_layouts[&client].1;
    compact_command(
        &shared,
        &mut context,
        "resize-pane",
        &["-Z", "-t", &pane.to_string()],
    );
    let geometry = shared.inner.lock().engine.pane_geometry(pane);
    shared
        .input(
            client,
            ClientKind::Interactive,
            &mut context,
            InputMessage::ResizeTerminalV2 {
                pane,
                columns: 1,
                rows: 1,
                cell_width_px: 8,
                cell_height_px: 16,
                layout_generation: generation,
            },
        )
        .expect("stale report");
    assert_eq!(shared.inner.lock().engine.pane_geometry(pane), geometry);
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("kill-session", ["-t", "ctrl-resize"]),
        )
        .expect("cleanup");
}

fn compact_hello(kind: ClientKind) -> zz_protocol::Hello {
    zz_protocol::Hello::from_client(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(919),
        kind,
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
    })
}

fn compact_command(
    shared: &Arc<Shared>,
    context: &mut ExecutionContext,
    name: &str,
    args: &[&str],
) {
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            context,
            &CommandInvocation::new(name, args.iter().copied()),
        )
        .expect(name);
}

fn compact_registered(
    shared: &Arc<Shared>,
    subscriptions: zz_protocol::Subscriptions,
) -> (ClientId, Arc<OutboundMailbox>) {
    let mut hello = compact_hello(ClientKind::Interactive);
    hello.subscriptions = subscriptions;
    let (client, _) = shared.register_welcome(&hello).expect("register");
    let mailbox = OutboundMailbox::new();
    shared.subscribe(client, Arc::clone(&mailbox));
    shared.inner.lock().ctrl_initializing.remove(&client);
    (client, mailbox)
}

fn reliable_children(mailbox: &OutboundMailbox) -> Vec<ProtocolMessage> {
    tests::take_reliable_messages(mailbox)
        .into_iter()
        .flat_map(|message| match message {
            ProtocolMessage::Batch(batch) => batch.messages().expect("children"),
            message => vec![message],
        })
        .collect()
}

fn bounded_reliable_children(mailbox: &OutboundMailbox, bound: usize) -> Vec<ProtocolMessage> {
    let envelopes = tests::take_reliable_messages(mailbox);
    let bytes = envelopes
        .iter()
        .map(|message| {
            zz_protocol::encode_protocol_message(message)
                .expect("encode")
                .len()
        })
        .sum::<usize>();
    eprintln!("CTRL publication total_wire_bytes={bytes} bound={bound}");
    assert!(
        bytes <= bound,
        "publication {bytes} bytes including every batch and view"
    );
    envelopes
        .into_iter()
        .flat_map(|message| match message {
            ProtocolMessage::Batch(batch) => batch.messages().expect("children"),
            message => vec![message],
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn compact_attach_sends_one_initial_batch_with_every_final_viewport() {
    assert_initial_compact_attach(false);
    assert_initial_compact_attach(true);
}

#[cfg(unix)]
fn assert_initial_compact_attach(commands: bool) {
    use std::os::unix::net::UnixStream;
    let shared = Arc::new(Shared::new(31));
    let mut context = ExecutionContext::default();
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &[
            "-d",
            "-s",
            "ctrl-initial",
            "printf CTRL_INITIAL_MARKER; exec sleep 30",
        ],
    );
    for _ in 0..3 {
        compact_command(
            &shared,
            &mut context,
            "split-window",
            &[
                "-d",
                "-t",
                "ctrl-initial",
                "printf CTRL_INITIAL_MARKER; exec sleep 30",
            ],
        );
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ready = shared.inner.lock().terminals.values().all(|terminal| {
            let viewport = terminal.latest_viewport();
            viewport
                .cells
                .iter()
                .map(|cell| viewport.cell_text(*cell))
                .collect::<String>()
                .contains("CTRL_INITIAL_MARKER")
        });
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "initial pane content did not arrive"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let (mut client, server) = UnixStream::pair().expect("pair");
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let worker_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || handle_connection(server, &worker_shared));
    let mut hello = compact_hello(ClientKind::Interactive);
    hello.subscriptions = zz_protocol::Subscriptions::terminal();
    hello.viewport = Some(zz_protocol::ClientViewport {
        columns: 97,
        rows: 31,
        cell_width_px: 8,
        cell_height_px: 16,
    });
    hello.attach = Some(if commands {
        zz_protocol::AttachOperation::Commands(vec![PreparedCommand {
            invocation: CommandInvocation::new("attach-session", ["-t", "ctrl-initial"]),
            canonical_name: Some("attach-session".to_owned()),
            alias_matched: false,
            result: PreparedCommandResult::Ready,
        }])
    } else {
        zz_protocol::AttachOperation::Session("ctrl-initial".to_owned())
    });
    zz_protocol::write_protocol_message(&mut client, &ProtocolMessage::Hello(hello))
        .expect("hello");
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut client).expect("welcome"),
        ProtocolMessage::Welcome(_)
    ));
    let ProtocolMessage::Batch(batch) =
        zz_protocol::read_protocol_message(&mut client).expect("initial batch")
    else {
        panic!("initial state is not a batch")
    };
    let messages = batch.messages().expect("messages");
    assert_eq!(
        messages
            .iter()
            .filter(|message| matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(_),
                    ..
                })
            ))
            .count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientView(zz_protocol::ClientView {
                        session: Some(_),
                        ..
                    }),
                    ..
                })
            ))
            .count(),
        1,
        "{messages:?}"
    );
    let viewports = messages
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalViewport { pane, viewport },
                ..
            }) => Some((*pane, viewport)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        viewports.len(),
        4,
        "initial batch must include all visible pane frames: {messages:?}"
    );
    for (pane, viewport) in viewports {
        let geometry = shared
            .inner
            .lock()
            .engine
            .pane_geometry(pane)
            .expect("geometry");
        assert_eq!((viewport.columns, viewport.rows), geometry);
        assert!(
            viewport
                .cells
                .iter()
                .map(|cell| viewport.cell_text(*cell))
                .collect::<String>()
                .contains("CTRL_INITIAL_MARKER")
        );
    }
    drop(client);
    worker
        .join()
        .expect("connection thread")
        .expect("connection");
    compact_command(
        &shared,
        &mut context,
        "kill-session",
        &["-t", "ctrl-initial"],
    );
}

#[test]
fn forced_attachment_stays_after_an_older_pending_publication() {
    let shared = Arc::new(Shared::new(39));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-order"]),
        )
        .expect("model session");
    let session = context.session.expect("session");
    let (client, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            tree: zz_protocol::TreeSubscription::All,
            ..zz_protocol::Subscriptions::control()
        },
    );
    shared
        .inner
        .lock()
        .client_kinds
        .insert(client, ClientKind::Control);
    let publication_lock = shared.snapshot_order.lock();
    let older = shared.compact_tree_messages(client, true);
    {
        let mut inner = shared.inner.lock();
        inner.attached.entry(session).or_default().insert(client);
        inner.ctrl_attachments.insert(client, 1);
    }
    let (started, ready) = crossbeam_channel::bounded(1);
    let (finished, completion) = crossbeam_channel::bounded(1);
    let force = {
        let shared = Arc::clone(&shared);
        let mailbox = Arc::clone(&mailbox);
        thread::spawn(move || {
            started.send(()).expect("start forced attachment");
            shared.send_compact_state(client, &mailbox, true);
            finished.send(()).expect("finish forced attachment");
        })
    };
    ready
        .recv_timeout(Duration::from_secs(2))
        .expect("forced sender started");
    thread::sleep(Duration::from_millis(20));
    let still_unattached = shared.inner.lock().ctrl_views[&client].session.is_none();
    let older = older
        .iter()
        .map(|message| zz_protocol::encode_protocol_message(message).expect("old publication"))
        .collect();
    assert!(mailbox.enqueue_control_group(older));
    drop(publication_lock);
    completion
        .recv_timeout(Duration::from_secs(2))
        .expect("forced attachment completed");
    force.join().expect("forced attachment");
    let views = reliable_children(&mailbox)
        .into_iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::ClientView(view),
                ..
            }) => Some(view),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        still_unattached,
        "forced attachment advanced the cursor before the older publication queued"
    );
    assert_eq!(views.len(), 2, "{views:?}");
    assert!(views[0].session.is_none());
    assert!(
        views[1..]
            .iter()
            .all(|view| view.session == Some(session) && view.attachment_generation == 1),
        "{views:?}"
    );
}

#[test]
fn compact_subscription_rename_is_bounded_and_empty_diff_sends_nothing() {
    let shared = Arc::new(Shared::new(32));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-tree"]),
        )
        .expect("model session");
    let session = context.session.expect("session");
    let (client, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            tree: zz_protocol::TreeSubscription::Attached,
            status: false,
            options: 0,
            keys: zz_protocol::KeySubscription::None,
            pane_stream: false,
        },
    );
    shared
        .attach_target(client, ClientKind::Interactive, &mut context, "ctrl-tree")
        .expect("attach");
    shared.compact_tree_messages(client, true);
    reliable_children(&mailbox);
    compact_command(
        &shared,
        &mut context,
        "rename-session",
        &["-t", "ctrl-tree", "renamed"],
    );
    shared.publish_compact_trees();
    let messages = bounded_reliable_children(&mailbox, 100);
    let deltas = messages
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TreeDelta(delta),
                ..
            }) => Some(delta),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(deltas.len(), 1, "{messages:?}");
    assert!(deltas[0].ops.iter().any(|op| matches!(op, zz_protocol::TreeOp::SessionName { session: changed, name } if *changed == session && name == "renamed")));
    let encoded = zz_protocol::encode_protocol_message(&Shared::event(EventPayload::TreeDelta(
        deltas[0].clone(),
    )))
    .expect("encode");
    assert!(encoded.len() <= 100, "rename frame {} bytes", encoded.len());
    shared.publish_compact_trees();
    assert!(reliable_children(&mailbox).is_empty());
    compact_command(&shared, &mut context, "kill-session", &["-t", "renamed"]);
}

#[test]
fn all_subscriber_unattached_session_rename_is_bounded_without_a_view_update() {
    let shared = Arc::new(Shared::new(40));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-own"]),
        )
        .expect("model session");
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-other"]),
        )
        .expect("model session");
    let (client, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            tree: zz_protocol::TreeSubscription::All,
            status: false,
            options: 0,
            keys: zz_protocol::KeySubscription::None,
            pane_stream: false,
        },
    );
    shared
        .attach_target(client, ClientKind::Interactive, &mut context, "ctrl-own")
        .expect("attach own session");
    shared.send_compact_state(client, &mailbox, true);
    reliable_children(&mailbox);
    compact_command(
        &shared,
        &mut context,
        "rename-session",
        &["-t", "ctrl-other", "ctrl-else"],
    );
    shared.publish_snapshot_state();
    let messages = bounded_reliable_children(&mailbox, 100);
    assert!(
        matches!(messages.as_slice(), [ProtocolMessage::Event(Event { payload: EventPayload::TreeDelta(delta), .. })] if !delta.ops.is_empty())
    );
    shared.publish_snapshot_state();
    assert!(reliable_children(&mailbox).is_empty());
    compact_command(&shared, &mut context, "kill-session", &["-t", "ctrl-else"]);
    compact_command(&shared, &mut context, "kill-session", &["-t", "ctrl-own"]);
}

#[test]
fn disconnected_control_exec_cannot_detach_the_next_active_client() {
    let shared = Arc::new(Shared::new(41));
    let mut context = ExecutionContext::default();
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "ctrl-cancel", "sleep 30"],
    );
    let (old, old_mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .inner
        .lock()
        .client_kinds
        .insert(old, ClientKind::Control);
    shared
        .attach_target(old, ClientKind::Control, &mut context, "ctrl-cancel")
        .expect("attach old control");
    let cancel = Arc::new(AtomicBool::new(false));
    shared
        .command_queue_cancels
        .lock()
        .insert(old, Arc::clone(&cancel));
    let directory = tempfile::Builder::new()
        .prefix("zz-ctrl-cancel-")
        .tempdir_in("/tmp")
        .expect("barrier directory");
    let ready = directory.path().join("ready");
    let release = directory.path().join("release");
    let script = format!(
        "touch {}; while test ! -e {}; do sleep 0.01; done",
        ready.display(),
        release.display()
    );
    let request = compact_exec_request(vec![
        CommandInvocation::new("run-shell", [script]),
        CommandInvocation::new("detach-client", [] as [&str; 0]),
    ]);
    let worker = {
        let shared = Arc::clone(&shared);
        let old_mailbox = Arc::clone(&old_mailbox);
        thread::spawn(move || {
            shared.execute_compact_request(
                old,
                ClientKind::Control,
                &mut context,
                request,
                &old_mailbox,
            );
        })
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "foreground control command did not start"
        );
        thread::sleep(Duration::from_millis(1));
    }
    cancel.store(true, Ordering::Release);
    shared.detach(old);
    shared.unregister(old);
    let (next, next_mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .inner
        .lock()
        .client_kinds
        .insert(next, ClientKind::Control);
    let mut next_context = ExecutionContext::default();
    let (session, _) = shared
        .attach_target(next, ClientKind::Control, &mut next_context, "ctrl-cancel")
        .expect("attach next control");
    reliable_children(&next_mailbox);
    fs::write(release, []).expect("release foreground command");
    worker.join().expect("cancelled execution");
    shared.execute_compact_request(
        old,
        ClientKind::Control,
        &mut ExecutionContext::default(),
        compact_exec_request(vec![CommandInvocation::new(
            "detach-client",
            [] as [&str; 0],
        )]),
        &old_mailbox,
    );
    assert_eq!(
        client_attached_session(&shared.inner.lock(), next),
        Some(session)
    );
    assert!(
        !reliable_children(&next_mailbox)
            .iter()
            .any(|message| matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Detached { .. },
                    ..
                })
            ))
    );
    shared.command_queue_cancels.lock().remove(&old);
    compact_command(
        &shared,
        &mut next_context,
        "kill-session",
        &["-t", "ctrl-cancel"],
    );
}

#[test]
fn new_full_subscriber_does_not_hide_pending_key_patch() {
    let shared = Arc::new(Shared::new(33));
    let (first, first_mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::default());
    shared.send_compact_keys(first, &first_mailbox);
    shared.publish_key_tables_if_changed();
    reliable_children(&first_mailbox);
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut ExecutionContext::default(),
            &CommandInvocation::new(
                "bind-key",
                ["-T", "ctrl-test", "x", "display-message", "changed"],
            ),
        )
        .expect("bind");
    let (second, second_mailbox) =
        compact_registered(&shared, zz_protocol::Subscriptions::default());
    shared.send_compact_keys(second, &second_mailbox);
    shared.publish_key_tables_if_changed();
    assert!(reliable_children(&first_mailbox).iter().any(|message| matches!(message, ProtocolMessage::Event(Event { payload: EventPayload::KeyTablesPatched { tables, .. }, .. }) if tables.iter().any(|table| table.name == "ctrl-test"))));
}

#[test]
fn first_full_subscriber_does_not_hide_a_pending_hash_update() {
    let shared = Arc::new(Shared::new(42));
    let (hash_client, hash_mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            keys: zz_protocol::KeySubscription::Hash,
            ..zz_protocol::Subscriptions::control()
        },
    );
    shared.send_compact_keys(hash_client, &hash_mailbox);
    shared.publish_key_tables_if_changed();
    reliable_children(&hash_mailbox);
    let generation = {
        let mut inner = shared.inner.lock();
        inner
            .engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new(
                    "bind-key",
                    ["-T", "ctrl-hash-pending", "x", "display-message", "changed"],
                ),
            )
            .expect("pending bind");
        inner.engine.keys.generation()
    };
    let (full_client, full_mailbox) =
        compact_registered(&shared, zz_protocol::Subscriptions::default());
    shared.send_compact_keys(full_client, &full_mailbox);
    reliable_children(&full_mailbox);
    shared.publish_key_tables_if_changed();
    let messages = bounded_reliable_children(&hash_mailbox, 64);
    assert!(
        matches!(messages.as_slice(), [ProtocolMessage::Event(Event { payload: EventPayload::KeyTablesHashChanged { hash, .. }, .. })] if *hash == generation)
    );
    assert!(reliable_children(&full_mailbox).is_empty());
}

#[test]
fn compact_raw_control_preflights_line_and_resolves_daemon_environment() {
    let shared = Arc::new(Shared::new(34));
    let mut hello = compact_hello(ClientKind::Control);
    hello.subscriptions = zz_protocol::Subscriptions::control();
    let (client, _) = shared.register_welcome(&hello).expect("register");
    let mailbox = OutboundMailbox::new();
    shared.subscribe(client, Arc::clone(&mailbox));
    shared.inner.lock().ctrl_initializing.remove(&client);
    let mut context = ExecutionContext::default();
    let request = |line: &str| zz_protocol::ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: zz_protocol::ExecFlags::default(),
        client_instance_id: ClientInstanceId(919),
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
    };
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        request("set-environment -g CTRL_MUST_NOT_RUN yes ; unknown-ctrl-command"),
        &mailbox,
    );
    assert!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("CTRL_MUST_NOT_RUN")
            .is_none()
    );
    assert!(matches!(
        reliable_children(&mailbox).as_slice(),
        [ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            outcome: zz_protocol::ExecOutcome::Rejected(_),
            ..
        })]
    ));
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        request(r"set-environment -g CTRL_MUST_NOT_RUN yes ; display-message -p \400"),
        &mailbox,
    );
    assert!(matches!(
        reliable_children(&mailbox).as_slice(),
        [ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            outcome: zz_protocol::ExecOutcome::Rejected(ServerError::CommandParse(message)),
            ..
        })] if message == "invalid octal escape"
    ));
    assert!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("CTRL_MUST_NOT_RUN")
            .is_none()
    );
    shared
        .inner
        .lock()
        .engine
        .seed_global_environment([("CTRL_EXPANSION", "server-value"), ("HOME", "/server-home")]);
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        request("display-message -p ~/$CTRL_EXPANSION"),
        &mailbox,
    );
    assert!(matches!(
        mailbox.state.lock().reliable.back(),
        Some(OutboundFrame::Grouped { frames, .. }) if frames.len() == if *hook_events::READONLY_SKIP { 3 } else { 2 }
    ));
    let messages = reliable_children(&mailbox);
    let started = messages.iter().position(|message| matches!(message, ProtocolMessage::Event(Event { payload: EventPayload::ControlCommandStarted { request_id: 1, flags: 1, guard: true, canonical_name: Some(name) }, .. }) if name == "display-message")).unwrap_or_else(|| panic!("command start missing: {messages:?}"));
    let response = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }) if output.as_bytes() == b"/server-home/server-value"
            )
        })
        .expect("response");
    assert!(started < response);
    assert!(matches!(
        messages.last(),
        Some(ProtocolMessage::ExecExit(_))
    ));
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new(
                "set-option",
                [
                    "-s",
                    "command-alias[90]",
                    "ctrlalias=display-message -p \"~/$CTRL_EXPANSION\"",
                ],
            ),
        )
        .expect("alias whose body needs expansion");
    for (line, expected) in [
        ("ctrlalias", "/server-home/server-value"),
        (
            "set-environment -g CTRL_EXPANSION changed ; ctrlalias",
            "/server-home/server-value",
        ),
        ("ctrlalias", "/server-home/changed"),
    ] {
        assert!(!line.contains(['$', '~']));
        shared.execute_compact_request(
            client,
            ClientKind::Control,
            &mut context,
            request(line),
            &mailbox,
        );
        assert!(
            reliable_children(&mailbox).iter().any(|message| matches!(
                message,
                ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. })
                    if output.as_bytes() == expected.as_bytes()
            )),
            "{line}"
        );
    }
}

#[test]
fn empty_control_expansion_resolvers_complete_without_the_state_lock() {
    let shared = Arc::new(Shared::new(52));
    let inner = shared.inner.lock();
    let (finished, completion) = crossbeam_channel::bounded(1);
    let worker = {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let values = shared.resolve_environment(&[]);
            let homes = shared.resolve_home_directories(&[]);
            let _ = finished.send((values, homes));
        })
    };
    let result = completion.recv_timeout(Duration::from_secs(2));
    drop(inner);
    worker.join().expect("empty expansion worker");
    let (values, homes) = result.expect("empty resolvers do not acquire the state lock");
    assert!(values.is_empty() && homes.is_empty());
}

fn compact_exec_request(commands: Vec<CommandInvocation>) -> zz_protocol::ExecRequest {
    zz_protocol::ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: zz_protocol::ExecFlags::default(),
        client_instance_id: ClientInstanceId(919),
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
        commands,
        raw_control_line: None,
    }
}

#[cfg(unix)]
#[test]
fn exec_resume_upgrades_the_same_socket_and_places_tail_error_after_attachment() {
    use std::os::unix::net::UnixStream;
    let shared = Arc::new(Shared::new(35));
    let (mut client, server) = UnixStream::pair().expect("pair");
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let worker_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || handle_connection(server, &worker_shared));
    let mut request = compact_exec_request(vec![
        CommandInvocation::new("display-message", ["-p", "prefix"]),
        CommandInvocation::new("new-session", ["-s", "ctrl-upgrade", "sleep 30"]),
        CommandInvocation::new("select-window", ["-t", "missing-ctrl-window"]),
    ]);
    request.flags.set(zz_protocol::ExecFlags::RESUME, true);
    request.flags.set(zz_protocol::ExecFlags::LAST, true);
    zz_protocol::write_protocol_message(&mut client, &ProtocolMessage::Exec(request))
        .expect("exec");
    let ProtocolMessage::ExecExit(zz_protocol::ExecExit {
        outcome: zz_protocol::ExecOutcome::Resume(resume),
        ..
    }) = zz_protocol::read_protocol_message(&mut client).expect("resume")
    else {
        panic!("expected resume")
    };
    let mut hello = compact_hello(ClientKind::Interactive);
    hello.subscriptions = zz_protocol::Subscriptions::terminal();
    hello.viewport = Some(zz_protocol::ClientViewport {
        columns: 80,
        rows: 24,
        cell_width_px: 8,
        cell_height_px: 16,
    });
    hello.attach = Some(zz_protocol::AttachOperation::Commands(resume.commands));
    zz_protocol::write_protocol_message(&mut client, &ProtocolMessage::Hello(hello))
        .expect("upgrade hello");
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut client).expect("welcome"),
        ProtocolMessage::Welcome(_)
    ));
    let ProtocolMessage::Batch(batch) =
        zz_protocol::read_protocol_message(&mut client).expect("batch")
    else {
        panic!("expected batch")
    };
    let messages = batch.messages().expect("children");
    let prefix = messages.iter().position(|message| matches!(message, ProtocolMessage::CommandResponse(CommandResponse::Success { request_id: 1, output, .. }) if output.as_bytes() == b"prefix")).expect("prefix output");
    let attached = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientView(zz_protocol::ClientView {
                        session: Some(_),
                        ..
                    }),
                    ..
                })
            )
        })
        .expect("attachment");
    let tail = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::CommandResponse(CommandResponse::Error { request_id: 3, .. })
            )
        })
        .expect("tail error");
    assert!(prefix < attached);
    assert!(attached < tail);
    assert_eq!(shared.inner.lock().client_instances.len(), 1);
    drop(client);
    worker.join().expect("worker").expect("connection");
    compact_command(
        &shared,
        &mut ExecutionContext::default(),
        "kill-session",
        &["-t", "ctrl-upgrade"],
    );
}

#[test]
fn hook_body_control_notification_follows_output_guard_without_recursive_hooks() {
    let shared = Arc::new(Shared::new(36));
    let mut context = ExecutionContext::default();
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "ctrl-hook", "-n", "initial", "sleep 30"],
    );
    compact_command(
        &shared,
        &mut context,
        "set-hook",
        &[
            "-g",
            "after-display-message",
            "rename-window -t ctrl-hook:0 hook-renamed",
        ],
    );
    compact_command(
        &shared,
        &mut context,
        "set-hook",
        &[
            "-g",
            "window-renamed",
            "set-environment -g CTRL_RECURSIVE_HOOK bad",
        ],
    );
    let (client, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            tree: zz_protocol::TreeSubscription::All,
            ..zz_protocol::Subscriptions::control()
        },
    );
    shared
        .inner
        .lock()
        .client_kinds
        .insert(client, ClientKind::Control);
    shared
        .attach_target(client, ClientKind::Control, &mut context, "ctrl-hook")
        .expect("attach");
    reliable_children(&mailbox);
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        compact_exec_request(vec![CommandInvocation::new(
            "display-message",
            ["-p", "outer"],
        )]),
        &mailbox,
    );
    let messages = reliable_children(&mailbox);
    let started = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ControlCommandStarted { guard: true, .. },
                    ..
                })
            )
        })
        .unwrap_or_else(|| panic!("command start missing: {messages:?}"));
    let notification = messages.iter().position(|message| matches!(message, ProtocolMessage::Event(Event { payload: EventPayload::HookEvent { name, .. }, .. }) if name == "window-renamed")).unwrap_or_else(|| panic!("rename notification missing: {messages:?}"));
    assert!(started < notification, "{messages:?}");
    assert!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("CTRL_RECURSIVE_HOOK")
            .is_none()
    );
    compact_command(&shared, &mut context, "kill-session", &["-t", "ctrl-hook"]);
}

#[test]
fn hash_subscriber_receives_bounded_mouse_hash_without_full_bindings() {
    let shared = Arc::new(Shared::new(37));
    let (_, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            keys: zz_protocol::KeySubscription::Hash,
            tree: zz_protocol::TreeSubscription::None,
            status: false,
            options: 0,
            pane_stream: false,
        },
    );
    compact_command(
        &shared,
        &mut ExecutionContext::default(),
        "bind-key",
        &["-T", "root", "x", "display-message", "changed"],
    );
    let messages = bounded_reliable_children(&mailbox, 64);
    assert!(!messages.iter().any(|message| matches!(
        message,
        ProtocolMessage::Event(Event {
            payload: EventPayload::KeyTablesChanged { .. } | EventPayload::KeyTablesPatched { .. },
            ..
        })
    )));
    let hash = messages
        .iter()
        .find(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::KeyTablesHashChanged { .. },
                    ..
                })
            )
        })
        .expect("hash");
    assert!(
        zz_protocol::encode_protocol_message(hash)
            .expect("encode")
            .len()
            <= 64
    );
}

#[test]
fn personalized_view_materializes_the_existing_stamped_tree() {
    let shared = Arc::new(Shared::new(38));
    let mut context = ExecutionContext::default();
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "ctrl-view-a", "-n", "first", "sleep 30"],
    );
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "ctrl-view-b", "-n", "second", "sleep 30"],
    );
    compact_command(
        &shared,
        &mut context,
        "set-option",
        &[
            "-gw",
            "window-status-format",
            "#{client_session}|#{pane_title}",
        ],
    );
    compact_command(
        &shared,
        &mut context,
        "set-option",
        &[
            "-gw",
            "window-status-current-format",
            "#{client_session}|#{pane_title}",
        ],
    );
    compact_command(
        &shared,
        &mut context,
        "set-option",
        &["-gw", "pane-border-status", "top"],
    );
    compact_command(
        &shared,
        &mut context,
        "set-option",
        &[
            "-gw",
            "pane-border-format",
            "#{client_session}|#{pane_title}",
        ],
    );
    let mut clients = Vec::new();
    for session in ["ctrl-view-a", "ctrl-view-b"] {
        let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::default());
        shared
            .attach_target(
                client,
                ClientKind::Interactive,
                &mut ExecutionContext::default(),
                session,
            )
            .expect("attach");
        clients.push((client, mailbox));
    }
    for (client, _) in &clients {
        let messages = shared.compact_tree_messages(*client, true);
        let mut raw = messages
            .iter()
            .find_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) => Some(snapshot.clone()),
                _ => None,
            })
            .expect("tree");
        let view = messages
            .iter()
            .find_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientView(view),
                    ..
                }) => Some(view),
                _ => None,
            })
            .expect("view");
        view.apply(&mut raw).expect("overlay");
        let inner = shared.inner.lock();
        let mut expected = inner.engine.state.snapshot();
        stamp_snapshot_for_client(&inner, *client, &mut expected, &snapshot_presence(&inner));
        expected.generation = raw.generation;
        assert_eq!(raw, expected);
    }
    compact_command(
        &shared,
        &mut context,
        "kill-session",
        &["-t", "ctrl-view-a"],
    );
    compact_command(
        &shared,
        &mut context,
        "kill-session",
        &["-t", "ctrl-view-b"],
    );
}
