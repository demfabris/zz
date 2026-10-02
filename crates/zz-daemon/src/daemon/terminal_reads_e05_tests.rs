use super::super::watchers::LoopWatchers;
use super::*;

fn setup() -> (Arc<Shared>, LoopWatchers, ClientId, ExecutionContext) {
    let shared = Arc::new(Shared::new(505));
    let watchers = LoopWatchers::new(&shared);
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "e05", ""]),
        )
        .unwrap();
    (shared, watchers, client, context)
}

fn queued(shared: &Arc<Shared>) -> Arc<Shared> {
    let item = shared.command_item(None);
    item.command_item.as_ref().unwrap().lock().loop_wait = true;
    item
}

fn state(item: &Shared) -> Arc<CommandState> {
    Arc::clone(
        item.command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .as_ref()
            .unwrap()
            .terminal
            .as_ref()
            .unwrap(),
    )
}

fn drain(shared: &Arc<Shared>, state: &CommandState) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let _round_trips = zz_terminal::forbid_actor_round_trips();
    while !state.continuation.ready() {
        assert!(Instant::now() < deadline);
        shared.terminal_requests.turn(shared);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn capture_reply_for_a_respawned_terminal_is_rejected_before_buffer_insertion() {
    let (shared, _watchers, client, context) = setup();
    let pane = context.pane.unwrap();
    let item = queued(&shared);
    let _round_trips = zz_terminal::forbid_actor_round_trips();
    let mut result = item.capture_pane(
        client,
        &context,
        "capture-pane",
        &["-b".into(), "stale".into()],
    );
    let state = state(&item);
    assert!(!state.continuation.ready());
    Arc::make_mut(&mut shared.inner.lock().terminals).insert(
        pane,
        Arc::new(TerminalSession::spawn_output_view(
            "replacement".to_owned(),
            "new text".to_owned(),
        )),
    );
    drain(&shared, &state);
    state.apply(&mut result);
    assert!(matches!(result, Err(DaemonError::Server(ServerError::PaneExited(id))) if id == pane));
    assert!(shared.inner.lock().paste_buffers.is_empty());
    shared.request_shutdown();
}

#[test]
fn capture_reply_for_a_departed_client_is_dropped_before_buffer_insertion() {
    let (shared, _watchers, client, context) = setup();
    let item = queued(&shared);
    let mut result = item.capture_pane(
        client,
        &context,
        "capture-pane",
        &["-b".into(), "stale".into()],
    );
    let state = state(&item);
    shared.inner.lock().clients.remove(&client);
    drain(&shared, &state);
    state.apply(&mut result);
    assert!(result.unwrap().output.is_empty());
    assert!(shared.inner.lock().paste_buffers.is_empty());
    shared.request_shutdown();
}

#[test]
fn history_reply_is_dropped_when_the_client_leaves_or_the_terminal_changes() {
    for replace_terminal in [false, true] {
        let (shared, _watchers, client, context) = setup();
        let pane = context.pane.unwrap();
        shared.loop_active.store(true, Ordering::Release);
        let replies = OutboundMailbox::new();
        let _round_trips = zz_terminal::forbid_actor_round_trips();
        shared.send_history(client, pane, 0, 10, &replies);
        if replace_terminal {
            Arc::make_mut(&mut shared.inner.lock().terminals).insert(
                pane,
                Arc::new(TerminalSession::spawn_output_view(
                    "replacement".to_owned(),
                    String::new(),
                )),
            );
        } else {
            shared.inner.lock().clients.remove(&client);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while shared.terminal_requests.turn(&shared).is_some() || shared.terminal_requests.pending()
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert!(super::super::tests::take_reliable_messages(&replies).is_empty());
        shared.request_shutdown();
    }
}

#[test]
fn capture_command_parks_on_the_loop_without_an_execution_worker() {
    let (shared, _watchers, client, context) = setup();
    let mut task = wait_queue::CommandTask::new(
        &shared,
        client,
        ClientKind::Command,
        &context,
        505,
        &CommandInvocation::new("capture-pane", ["-p", "-S", "-", "-J"]),
        false,
    )
    .unwrap_or_else(|_| panic!("capture task"));
    let _round_trips = zz_terminal::forbid_actor_round_trips();
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !task.ready() {
        assert!(Instant::now() < deadline);
        shared.terminal_requests.turn(&shared);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    assert!(matches!(task.finish().0, CommandResponse::Success { .. }));
    shared.request_shutdown();
}

#[test]
fn a_replaced_terminal_cannot_fill_the_kitty_image_cache() {
    let (shared, _watchers, _client, context) = setup();
    let pane = context.pane.unwrap();
    let terminal = Arc::new(TerminalSession::spawn_output_view(
        "image".to_owned(),
        "\x1b_Ga=T,f=24,s=1,v=1,i=77;/wAA\x1b\\".to_owned(),
    ));
    Arc::make_mut(&mut shared.inner.lock().terminals).insert(pane, Arc::clone(&terminal));
    let _round_trips = zz_terminal::forbid_actor_round_trips();
    let request = terminal.kitty_image_request(77, shared.terminal_requests.notifier());
    let done = Arc::new(AtomicBool::new(false));
    let completed = Arc::clone(&done);
    shared
        .terminal_requests
        .submit(request, move |shared, result| {
            if shared.is_current_image_terminal(pane, &terminal)
                && let Ok(Some(image)) = result
            {
                shared.store_kitty_image_frames(pane, &terminal, &image);
            }
            completed.store(true, Ordering::Release);
        });
    Arc::make_mut(&mut shared.inner.lock().terminals).insert(
        pane,
        Arc::new(TerminalSession::spawn_output_view(
            "replacement".to_owned(),
            String::new(),
        )),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while !done.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        shared.terminal_requests.turn(&shared);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(shared.kitty_image_frames.lock().is_empty());
    shared.request_shutdown();
}

#[test]
fn connection_capture_dispatches_to_a_loop_continuation() {
    let (shared, _watchers, _client, context) = setup();
    let outbound = OutboundMailbox::new();
    let hello = ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(505),
        kind: ClientKind::Command,
        device_name: None,
        capabilities: Vec::new(),
        color_scheme: None,
        origin: None,
        environment: Vec::new(),
        working_directory: None,
        process_id: 0,
    };
    let mut session = connection::Session::register(
        &shared,
        ProtocolMessage::ClientHello(hello),
        &outbound,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
    .unwrap();
    session.start_message(ProtocolMessage::CommandRequest(CommandRequest {
        request_id: 505,
        command: CommandInvocation::new(
            "capture-pane",
            ["-p", "-t", &context.pane.unwrap().to_string()],
        ),
        prepared: false,
    }));
    let _round_trips = zz_terminal::forbid_actor_round_trips();
    assert!(matches!(
        session.run_message(&shared, &outbound, true),
        connection::MessageProgress::Wait
    ));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !session.command_wait_ready() {
        assert!(Instant::now() < deadline);
        shared.terminal_requests.turn(&shared);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        session.run_message(&shared, &outbound, true),
        connection::MessageProgress::Done
    ));
    shared.request_shutdown();
}

#[test]
fn a_stale_image_removal_preserves_the_newly_delivered_generation() {
    let pane = PaneId(505);
    let replies = OutboundMailbox::new();
    replies
        .state
        .lock()
        .delivered_images
        .entry(pane)
        .or_default()
        .insert(77, 2);
    replies.enqueue_kitty_image_removed_if_generation(pane, 77, 1);
    assert!(super::super::tests::take_reliable_messages(&replies).is_empty());
    assert_eq!(replies.state.lock().delivered_images[&pane][&77], 2);
    replies.enqueue_kitty_image_removed_if_generation(pane, 77, 2);
    assert!(
        matches!(super::super::tests::take_reliable_messages(&replies).as_slice(), [ProtocolMessage::Event(Event { payload: EventPayload::KittyImagesRemoved { pane: removed_pane, image_ids }, .. })] if *removed_pane == pane && image_ids == &[77])
    );
    assert!(!replies.state.lock().delivered_images.contains_key(&pane));
}
