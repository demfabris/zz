use super::*;

fn fixture() -> (
    Arc<Shared>,
    ClientId,
    Arc<OutboundMailbox>,
    MuxSnapshot,
    WindowId,
) {
    let shared = Arc::new(Shared::new(71));
    let mut context = ExecutionContext::default();
    {
        let mut inner = shared.inner.lock();
        for command in [
            CommandInvocation::new("new-session", ["-d", "-s", "ctrl-flags"]),
            CommandInvocation::new("new-window", ["-d", "-t", "ctrl-flags"]),
        ] {
            inner
                .engine
                .execute(&mut context, &command)
                .expect("model window");
        }
    }
    let session = context.session.expect("session");
    let window = shared.inner.lock().engine.state.sessions[&session].windows[1];
    let mut hello = zz_protocol::Hello::from_client(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(971),
        kind: ClientKind::Control,
        device_name: None,
        capabilities: Vec::new(),
        color_scheme: None,
        origin: None,
        working_directory: None,
        environment: Vec::new(),
        process_id: std::process::id(),
    });
    hello.subscriptions = zz_protocol::Subscriptions {
        tree: zz_protocol::TreeSubscription::Attached,
        ..zz_protocol::Subscriptions::control()
    };
    let (client, _) = shared.register_welcome(&hello).expect("register control");
    let mailbox = OutboundMailbox::new();
    shared.subscribe(client, Arc::clone(&mailbox));
    {
        let mut inner = shared.inner.lock();
        if let Some(client) = inner.client_mut(client) {
            client.ctrl_initializing = false;
        }
        inner.attached.entry(session).or_default().insert(client);
    }
    shared.send_compact_state(client, &mailbox, true);
    let initial = messages(&mailbox)
        .into_iter()
        .find_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::Snapshot(snapshot),
                ..
            }) => Some(snapshot),
            _ => None,
        })
        .expect("initial tree");
    (shared, client, mailbox, initial, window)
}

fn messages(mailbox: &OutboundMailbox) -> Vec<ProtocolMessage> {
    tests::take_reliable_messages(mailbox)
        .into_iter()
        .flat_map(|message| match message {
            ProtocolMessage::Batch(batch) => batch.messages().expect("flat control group"),
            message => vec![message],
        })
        .collect()
}

fn layout_hook(window: WindowId) -> EventPayload {
    EventPayload::HookEvent {
        name: "window-layout-changed".to_owned(),
        variables: BTreeMap::from([("hook_window".to_owned(), window.to_string())]),
    }
}

fn assert_activity_before_hook(
    mut tree: MuxSnapshot,
    messages: Vec<ProtocolMessage>,
    window: WindowId,
) {
    let mut delta_seen = false;
    let mut hook_seen = false;
    for message in messages {
        match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TreeDelta(delta),
                ..
            }) => {
                delta.apply(&mut tree).expect("ordered tree delta");
                delta_seen = true;
            }
            ProtocolMessage::Event(Event {
                payload: EventPayload::HookEvent { name, .. },
                ..
            }) if name == "window-layout-changed" => {
                hook_seen = true;
                assert!(delta_seen, "layout hook overtook its tree delta");
                assert!(
                    tree.sessions
                        .iter()
                        .flat_map(|session| &session.windows)
                        .find(|entry| entry.id == window)
                        .expect("window")
                        .activity,
                    "layout hook saw stale activity"
                );
            }
            ProtocolMessage::Event(Event {
                payload: EventPayload::Snapshot(_),
                ..
            }) => panic!("flag publication fell back to a full tree"),
            _ => {}
        }
    }
    assert!(hook_seen, "missing layout hook");
}

#[test]
fn layout_hook_waits_for_pending_activity_delta() {
    let (shared, client, mailbox, initial, window) = fixture();
    let order = shared.snapshot_order.lock();
    shared
        .inner
        .lock()
        .engine
        .state
        .set_window_activity_flag(window, true);
    let pending = shared.compact_tree_messages(client, false);
    let (started, ready) = crossbeam_channel::bounded(1);
    let worker = {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            started.send(()).expect("hook sender started");
            shared.publish_to_control_clients(layout_hook(window), None, true);
        })
    };
    ready
        .recv_timeout(Duration::from_secs(2))
        .expect("hook sender");
    thread::sleep(Duration::from_millis(20));
    let frames = pending
        .iter()
        .map(|message| {
            zz_protocol::encode_protocol_message(message)
                .expect("pending delta")
                .into()
        })
        .collect();
    assert!(mailbox.enqueue_control_group(frames));
    drop(order);
    worker.join().expect("hook sender completed");
    assert_activity_before_hook(initial, messages(&mailbox), window);
}

#[test]
fn layout_hook_publishes_unpublished_activity_delta() {
    let (shared, _, mailbox, initial, window) = fixture();
    shared
        .inner
        .lock()
        .engine
        .state
        .set_window_activity_flag(window, true);
    shared.publish_to_control_clients(layout_hook(window), None, true);
    assert_activity_before_hook(initial, messages(&mailbox), window);
}

#[test]
fn raw_control_output_publishes_activity_before_output_and_layout() {
    for quiet in [false, true] {
        let (shared, _, mailbox, initial, window) = fixture();
        if quiet {
            assert!(mailbox.collect_control_query());
        }
        let pane = {
            let mut inner = shared.inner.lock();
            inner
                .engine
                .execute(
                    &mut ExecutionContext::default(),
                    &CommandInvocation::new(
                        "set-window-option",
                        ["-t", &window.to_string(), "monitor-activity", "on"],
                    ),
                )
                .expect("monitor activity");
            inner.engine.state.windows[&window].active_pane
        };
        shared.publish_control_output_for_pane(pane, &Arc::from(b"activity\r\n".as_slice()));
        let activity_raised = shared.inner.lock().engine.state.windows[&window].activity_flag;
        shared.publish_to_control_clients(layout_hook(window), None, true);
        if quiet {
            mailbox.finish_control_query();
        }
        let messages = messages(&mailbox);
        let delta_index = messages.iter().position(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::TreeDelta(_),
                    ..
                })
            )
        });
        let output_index = messages
            .iter()
            .position(|message| {
                matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::PaneOutput { .. },
                        ..
                    })
                )
            })
            .expect("control output");
        assert!(
            activity_raised,
            "raw output reached control before the viewport worker raised activity"
        );
        assert!(
            delta_index.is_some_and(|index| index < output_index),
            "activity delta arrived after raw output"
        );
        assert_activity_before_hook(initial, messages, window);
    }
}

#[test]
fn raw_activity_preserves_one_viewport_alert_without_reinstating_cleared_flags() {
    let (shared, _, mailbox, _, window) = fixture();
    let pane = {
        let mut inner = shared.inner.lock();
        inner
            .engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new(
                    "set-window-option",
                    ["-t", &window.to_string(), "monitor-activity", "on"],
                ),
            )
            .expect("monitor activity");
        inner.engine.state.windows[&window].active_pane
    };
    shared.publish_control_output_for_pane(pane, &Arc::from(b"activity".as_slice()));
    assert!(messages(&mailbox).iter().all(|message| !matches!(message, ProtocolMessage::Event(Event { payload: EventPayload::HookEvent { name, .. }, .. }) if name == "alert-activity")));
    shared
        .inner
        .lock()
        .engine
        .state
        .set_window_activity_flag(window, false);
    shared.raise_window_activity(window, pane);
    assert!(!shared.inner.lock().engine.state.windows[&window].activity_flag);
    let alerts = messages(&mailbox).into_iter().filter(|message| matches!(message, ProtocolMessage::Event(Event { payload: EventPayload::HookEvent { name, .. }, .. }) if name == "alert-activity")).count();
    assert_eq!(alerts, 1);
    assert!(!shared.inner.lock().control_activity_pending.contains(&pane));
}

#[test]
fn raw_activity_respects_monitoring_and_the_attached_current_window() {
    let (shared, _, mailbox, _, window) = fixture();
    let pane = shared.inner.lock().engine.state.windows[&window].active_pane;
    shared.publish_control_output_for_pane(pane, &Arc::from(b"unmonitored".as_slice()));
    assert!(!shared.inner.lock().engine.state.windows[&window].activity_flag);
    {
        let mut inner = shared.inner.lock();
        inner
            .engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new(
                    "set-window-option",
                    ["-t", &window.to_string(), "monitor-activity", "on"],
                ),
            )
            .expect("monitor activity");
        let session = inner.engine.state.windows[&window].session;
        inner
            .engine
            .state
            .select_window(session, window)
            .expect("select current");
    }
    shared.publish_control_output_for_pane(pane, &Arc::from(b"current".as_slice()));
    assert!(!shared.inner.lock().engine.state.windows[&window].activity_flag);
    assert!(shared.inner.lock().control_activity_pending.is_empty());
    messages(&mailbox);
}

#[test]
fn layout_hook_reduces_alert_zoom_and_current_deltas_before_rendering() {
    let (shared, _, mailbox, mut tree, window) = fixture();
    let (session, pane) = {
        let mut inner = shared.inner.lock();
        let session = inner.engine.state.windows[&window].session;
        inner
            .engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new("split-window", ["-d", "-t", &window.to_string()]),
            )
            .expect("model split");
        let pane = inner.engine.state.windows[&window].active_pane;
        inner
            .engine
            .state
            .select_window(session, window)
            .expect("select current");
        inner.engine.state.toggle_zoom(pane).expect("zoom");
        inner.engine.state.set_window_activity_flag(window, true);
        inner.engine.state.set_window_silence_flag(window, true);
        inner.engine.state.set_pane_bell(pane, true);
        (session, pane)
    };
    shared.publish_to_control_clients(layout_hook(window), None, true);
    let mut hook_seen = false;
    for message in messages(&mailbox) {
        match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TreeDelta(delta),
                ..
            }) => delta.apply(&mut tree).expect("flag delta"),
            ProtocolMessage::Event(Event {
                payload: EventPayload::HookEvent { name, .. },
                ..
            }) if name == "window-layout-changed" => {
                hook_seen = true;
                let session = tree
                    .sessions
                    .iter()
                    .find(|entry| entry.id == session)
                    .expect("session");
                let window = session
                    .windows
                    .iter()
                    .find(|entry| entry.id == window)
                    .expect("window");
                assert!(window.activity);
                assert!(window.silence);
                assert!(window.panes[&pane].bell);
                assert_eq!(window.zoomed_pane, Some(pane));
                assert_eq!(session.active_window, window.id);
            }
            ProtocolMessage::Event(Event {
                payload: EventPayload::Snapshot(_),
                ..
            }) => panic!("full tree replaced a flag delta"),
            _ => {}
        }
    }
    assert!(hook_seen, "missing layout hook");
}
