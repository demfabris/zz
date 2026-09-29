use std::io::{self, IoSlice, Write};

use zz_protocol::{decode_protocol_frame, encode_protocol_message};

use super::tests::{
    QUIET_PANE_COMMAND, switch_test_session, take_reliable_messages, terminal_patch_test_message,
    terminal_test_message,
};
use super::*;

const COMMAND_CLIENT: ClientId = ClientId(u64::MAX);

fn command(shared: &Arc<Shared>, context: &mut ExecutionContext, args: &[&str]) {
    shared
        .execute(
            COMMAND_CLIENT,
            ClientKind::Command,
            context,
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap_or_else(|error| panic!("{args:?}: {error:?}"));
}

fn interactive(shared: &Arc<Shared>, client: ClientId, args: &[&str]) {
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap_or_else(|error| panic!("{args:?}: {error:?}"));
}

/// A raw-terminal client: it has a terminal and named its size and cell in
/// the hello, the facts `handle_connection` records from the capabilities.
fn terminal_client(shared: &Arc<Shared>, size: (u16, u16)) -> (ClientId, Arc<OutboundMailbox>) {
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    let capabilities = [
        format!(
            "{}{}x{}",
            ClientHello::CLIENT_SIZE_CAPABILITY_PREFIX,
            size.0,
            size.1
        ),
        format!("{}8x16", ClientHello::CLIENT_CELL_CAPABILITY_PREFIX),
    ];
    {
        let mut inner = shared.inner.lock();
        inner
            .client_sizes
            .insert(client, client_size_fact(&capabilities).expect("size fact"));
        inner.client_cell_pixels.insert(
            client,
            attach::client_cell_fact(&capabilities).expect("cell fact"),
        );
    }
    (client, mailbox)
}

/// Pops ready frames the way the writer does, without blocking, until
/// `done` holds for what was taken and then `quiet` passes with nothing new.
fn drain_until(
    mailbox: &OutboundMailbox,
    quiet: Duration,
    done: impl Fn(&[ProtocolMessage]) -> bool,
) -> Vec<ProtocolMessage> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut messages = Vec::new();
    let mut last = Instant::now();
    loop {
        let frame = pop_ready_frame(&mut mailbox.state.lock());
        if let Some(frame) = frame {
            messages.push(decode_protocol_frame(&frame).expect("decode outbound frame"));
            last = Instant::now();
            continue;
        }
        if done(&messages) && last.elapsed() >= quiet {
            return messages;
        }
        assert!(
            Instant::now() < deadline,
            "outbound frames never settled: {:?}",
            messages.iter().map(message_name).collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn message_name(message: &ProtocolMessage) -> String {
    match message {
        ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalViewport { pane, viewport },
            ..
        }) => format!("Full {pane} {}x{}", viewport.columns, viewport.rows),
        ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalPatch { pane, .. },
            ..
        }) => format!("Patch {pane}"),
        ProtocolMessage::Event(Event { payload, .. }) => {
            format!("{payload:?}").chars().take(40).collect()
        }
        other => format!("{other:?}").chars().take(40).collect(),
    }
}

fn fulls(messages: &[ProtocolMessage]) -> Vec<(PaneId, u16, u16)> {
    messages
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalViewport { pane, viewport },
                ..
            }) => Some((*pane, viewport.columns, viewport.rows)),
            _ => None,
        })
        .collect()
}

fn after_last_attached(messages: &[ProtocolMessage]) -> &[ProtocolMessage] {
    let last = messages
        .iter()
        .rposition(|message| matches!(message, ProtocolMessage::Attached { .. }))
        .expect("an Attached was written");
    &messages[last..]
}

fn every_pane_has_a_full(panes: &[PaneId]) -> impl Fn(&[ProtocolMessage]) -> bool + '_ {
    move |messages| {
        messages
            .iter()
            .any(|message| matches!(message, ProtocolMessage::Attached { .. }))
            && panes.iter().all(|pane| {
                fulls(after_last_attached(messages))
                    .iter()
                    .any(|(full, _, _)| full == pane)
            })
    }
}

fn window_panes(shared: &Shared, pane: PaneId) -> Vec<PaneId> {
    let inner = shared.inner.lock();
    let window = inner.engine.state.window_for_pane(pane).expect("window");
    inner.engine.state.windows[&window]
        .panes
        .keys()
        .copied()
        .collect()
}

fn is_absent_overlay(payload: &EventPayload) -> bool {
    matches!(
        payload,
        EventPayload::CommandPrompt { state: None }
            | EventPayload::Popup { state: None }
            | EventPayload::Menu { state: None }
            | EventPayload::Confirm { state: None }
            | EventPayload::ChooseTree { state: None }
            | EventPayload::ChooseBuffer { state: None }
            | EventPayload::ChooserPresentation { presentation: None }
            | EventPayload::DisplayPanes { state: None }
            | EventPayload::CommandOutput { viewport: None, .. }
    )
}

#[test]
fn a_fresh_attach_repeats_neither_the_snapshot_nor_an_absent_overlay() {
    let shared = Arc::new(Shared::new(1));
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    let (session, _, _) = switch_test_session(&shared, "fresh-attach");
    let snapshot = shared.attach(client, session).expect("attach");
    take_reliable_messages(&mailbox);

    assert!(shared.send_attached(client, &mailbox, session, snapshot));
    let messages = take_reliable_messages(&mailbox);
    assert!(matches!(
        messages.first(),
        Some(ProtocolMessage::Attached { .. })
    ));
    for message in &messages[1..] {
        let ProtocolMessage::Event(Event { payload, .. }) = message else {
            continue;
        };
        assert!(
            !matches!(payload, EventPayload::Snapshot(_)) && !is_absent_overlay(payload),
            "fresh attach repeated {payload:?}"
        );
    }

    shared.send_resync(client, &mailbox);
    let resync = take_reliable_messages(&mailbox);
    let payloads = resync
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event { payload, .. }) => Some(payload),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(matches!(payloads.first(), Some(EventPayload::Snapshot(_))));
    assert_eq!(
        payloads
            .iter()
            .filter(|payload| is_absent_overlay(payload))
            .count(),
        9,
        "a requested resync still clears every overlay"
    );
}

#[test]
fn a_fresh_attach_still_sends_the_overlays_that_exist() {
    let shared = Arc::new(Shared::new(1));
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    let (session, _, pane) = switch_test_session(&shared, "fresh-attach-overlay");
    let snapshot = shared.attach(client, session).expect("attach");
    shared
        .open_command_output(client, Some(pane), "list".to_owned(), "one\ntwo\n")
        .expect("open command output");
    let output = Arc::clone(&shared.inner.lock().command_outputs[&client].terminal);
    let deadline = Instant::now() + Duration::from_secs(30);
    while output
        .latest_viewport_for(TerminalViewId(client.0))
        .is_none()
    {
        assert!(Instant::now() < deadline, "command output never published");
        thread::sleep(Duration::from_millis(5));
    }
    take_reliable_messages(&mailbox);
    mailbox.state.lock().command_output = None;

    assert!(shared.send_attached(client, &mailbox, session, snapshot));
    let replayed = mailbox
        .state
        .lock()
        .command_output
        .take()
        .map(|pending| decode_protocol_frame(&pending.encoded).expect("decode output"));
    assert!(matches!(
        replayed,
        Some(ProtocolMessage::Event(Event {
            payload: EventPayload::CommandOutput {
                viewport: Some(_),
                ..
            },
            ..
        }))
    ));
}

#[test]
fn equal_generations_are_dropped_against_the_queued_and_the_written_frame() {
    let mailbox = OutboundMailbox::new();
    let pane = PaneId(3);
    let full = terminal_test_message(pane, 1, 5);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &full),
        TerminalEnqueue::Queued
    );
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 2, 5)),
        TerminalEnqueue::Dropped
    );
    assert!(!mailbox.replace_terminal(pane, &terminal_test_message(pane, 3, 5)));
    assert_eq!(
        decode_protocol_frame(&mailbox.recv().expect("queued full")).expect("decode"),
        full
    );

    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 4, 5)),
        TerminalEnqueue::Dropped
    );
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_patch_test_message(pane, 5, 5, 5)),
        TerminalEnqueue::Dropped
    );
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_patch_test_message(pane, 6, 5, 6)),
        TerminalEnqueue::Queued
    );
    assert!(mailbox.state.lock().terminals.contains_key(&pane));
}

#[test]
fn the_client_wiping_its_viewports_lets_the_same_generation_through_again() {
    let mailbox = OutboundMailbox::new();
    let pane = PaneId(4);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 1, 7)),
        TerminalEnqueue::Queued
    );
    mailbox.recv().expect("first full");

    mailbox.forget_delivered_terminal(pane);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 2, 7)),
        TerminalEnqueue::Queued
    );
    mailbox.recv().expect("requested full");

    let attached = ProtocolMessage::Attached {
        session: SessionId(1),
        snapshot: MuxSnapshot::default(),
        read_only: false,
        client_flags: String::new(),
    };
    assert!(mailbox.enqueue_attached(&attached));
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 3, 7)),
        TerminalEnqueue::Queued
    );
    assert!(matches!(
        decode_protocol_frame(&mailbox.recv().expect("attached")),
        Ok(ProtocolMessage::Attached { .. })
    ));
    assert!(matches!(
        decode_protocol_frame(&mailbox.recv().expect("full after attached")),
        Ok(ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalViewport { .. },
            ..
        }))
    ));

    mailbox.forget_delivered_terminals();
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 4, 7)),
        TerminalEnqueue::Queued
    );
}

fn attached_message() -> ProtocolMessage {
    ProtocolMessage::Attached {
        session: SessionId(1),
        snapshot: MuxSnapshot::default(),
        read_only: false,
        client_flags: String::new(),
    }
}

#[test]
fn an_attach_is_written_as_one_batch_once_its_hold_is_released() {
    let mailbox = OutboundMailbox::new();
    let pane = PaneId(5);
    mailbox.hold_terminals();
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 1, 2)),
        TerminalEnqueue::Queued
    );
    assert!(mailbox.enqueue_reliable(&Shared::event(EventPayload::ServerStopping)));
    assert!(mailbox.enqueue_attached(&attached_message()));
    assert!(mailbox.enqueue_reliable(&Shared::event(EventPayload::ServerStopping)));
    assert!(
        pop_ready_frame(&mut mailbox.state.lock()).is_none(),
        "nothing is written while the attach holds the mailbox"
    );

    mailbox.release_terminals();
    let mut batch = Vec::new();
    assert!(mailbox.recv_batch(&mut batch, attach::MAX_BATCHED_WRITE_BYTES));
    let decoded = batch
        .iter()
        .map(|frame| decode_protocol_frame(frame).expect("decode batch"))
        .collect::<Vec<_>>();
    assert!(
        matches!(
            decoded.as_slice(),
            [
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ServerStopping,
                    ..
                }),
                ProtocolMessage::Attached { .. },
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ServerStopping,
                    ..
                }),
                ProtocolMessage::Event(Event {
                    payload: EventPayload::TerminalViewport { .. },
                    ..
                })
            ]
        ),
        "{:?}",
        decoded.iter().map(message_name).collect::<Vec<_>>()
    );

    mailbox.hold_terminals();
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 2, 3)),
        TerminalEnqueue::Queued
    );
    mailbox.release_terminals();
    assert!(
        mailbox.recv().is_some(),
        "a failed attach releases the hold"
    );
}

#[test]
fn a_held_attach_lets_a_long_reliable_queue_drain_but_keeps_its_frames() {
    let mailbox = OutboundMailbox::new();
    let pane = PaneId(6);
    mailbox.hold_terminals();
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 1, 2)),
        TerminalEnqueue::Queued
    );
    for _ in 0..=MAX_RELIABLE_MESSAGES / 2 {
        assert!(mailbox.enqueue_reliable(&Shared::event(EventPayload::ServerStopping)));
    }
    let mut written = 0;
    while let Some(frame) = pop_ready_frame(&mut mailbox.state.lock()) {
        assert!(!is_full(&decode_protocol_frame(&frame).expect("decode")));
        written += 1;
    }
    assert_eq!(written, MAX_RELIABLE_MESSAGES / 2 + 1);
    assert!(mailbox.enqueue_attached(&attached_message()));
    assert!(matches!(
        decode_protocol_frame(&mailbox.recv().expect("attached")),
        Ok(ProtocolMessage::Attached { .. })
    ));
    assert!(is_full(
        &decode_protocol_frame(&mailbox.recv().expect("frame")).expect("decode")
    ));
}

/// Writes at most `limit` bytes per call, as a full socket buffer does.
struct Trickle {
    written: Vec<u8>,
    limit: usize,
    calls: usize,
}

impl Write for Trickle {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.write_vectored(&[IoSlice::new(buffer)])
    }

    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        self.calls += 1;
        let mut taken = 0;
        for buffer in buffers {
            let take = buffer.len().min(self.limit - taken);
            self.written.extend_from_slice(&buffer[..take]);
            taken += take;
            if taken == self.limit {
                break;
            }
        }
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn kitty_image_chunks_precede_the_frame_that_places_them_in_one_batched_write() {
    let mailbox = OutboundMailbox::new();
    let pane = PaneId(9);
    let (image_id, generation) = (4, 2);
    let chunks = [
        EventPayload::KittyImageBegin {
            pane,
            image_id,
            generation,
            width: 1,
            height: 1,
            total_bytes: 4,
        },
        EventPayload::KittyImageChunk {
            pane,
            image_id,
            generation,
            bytes: vec![1, 2, 3, 4],
        },
    ]
    .map(|payload| encode_protocol_message(&Shared::event(payload)).expect("encode chunk"));
    let placing = terminal_test_message(pane, 3, 1);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &placing),
        TerminalEnqueue::Queued
    );
    assert_eq!(
        mailbox.enqueue_kitty_image(pane, image_id, generation, &chunks),
        KittyImageEnqueue::Queued
    );

    let mut batch = Vec::new();
    assert!(mailbox.recv_batch(&mut batch, attach::MAX_BATCHED_WRITE_BYTES));
    assert_eq!(batch.len(), 3);
    assert_eq!(batch[..2], chunks);
    assert_eq!(
        decode_protocol_frame(&batch[2]).expect("decode placing frame"),
        placing
    );

    let mut socket = Trickle {
        written: Vec::new(),
        limit: 7,
        calls: 0,
    };
    attach::write_frames(&mut socket, &batch).expect("batched write");
    assert_eq!(socket.written, batch.concat());
    let mut whole = Trickle {
        written: Vec::new(),
        limit: usize::MAX,
        calls: 0,
    };
    attach::write_frames(&mut whole, &batch).expect("one write");
    assert_eq!(whole.calls, 1, "a batch that fits is one writev");
}

#[test]
fn a_terminal_client_hello_names_the_options_its_terminal_is_armed_from() {
    let shared = Arc::new(Shared::new(1));
    let options = |hello: &ServerHello| {
        hello
            .capabilities
            .iter()
            .filter_map(|capability| capability.strip_prefix(SERVER_OPTION_CAPABILITY_PREFIX))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let (_, hello) =
        shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());
    assert_eq!(options(&hello), ["extended-keys=off", "focus-events=off"]);

    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &["set-option", "-s", "focus-events", "on"],
    );
    command(
        &shared,
        &mut context,
        &["set-option", "-s", "extended-keys", "always"],
    );
    let (_, hello) =
        shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());
    assert_eq!(options(&hello), ["extended-keys=always", "focus-events=on"]);

    let (_, hello) =
        shared.register_subscribed(ClientKind::Control, None, None, OutboundMailbox::new());
    assert!(options(&hello).is_empty());
}

#[test]
fn a_cell_fact_needs_two_positive_pixel_extents() {
    let fact = |value: &str| {
        attach::client_cell_fact(&[format!(
            "{}{value}",
            ClientHello::CLIENT_CELL_CAPABILITY_PREFIX
        )])
    };
    assert_eq!(fact("9x18"), Some((9, 18)));
    assert_eq!(fact("0x18"), None);
    assert_eq!(fact("9"), None);
    assert_eq!(fact("9x-1"), None);
    assert_eq!(
        attach::client_cell_fact(&["client-size-v1:9x18".to_owned()]),
        None
    );
}

#[cfg(unix)]
#[test]
fn a_client_that_named_its_size_gets_its_first_frame_at_that_size() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &[
            "new-session",
            "-d",
            "-s",
            "presize",
            "-x",
            "80",
            "-y",
            "24",
            QUIET_PANE_COMMAND,
        ],
    );
    let first = context.pane.expect("first pane");
    command(
        &shared,
        &mut context,
        &[
            "split-window",
            "-d",
            "-h",
            "-t",
            "presize",
            QUIET_PANE_COMMAND,
        ],
    );
    let panes = window_panes(&shared, first);
    let (client, mailbox) = terminal_client(&shared, (120, 40));

    interactive(&shared, client, &["attach-session", "-t", "presize"]);
    let messages = drain_until(
        &mailbox,
        Duration::from_millis(250),
        every_pane_has_a_full(&panes),
    );

    let before = messages
        .iter()
        .position(|message| matches!(message, ProtocolMessage::Attached { .. }))
        .expect("attached");
    assert!(
        fulls(&messages[..before]).is_empty(),
        "no frame is written before Attached"
    );
    let expected = {
        let inner = shared.inner.lock();
        panes
            .iter()
            .map(|pane| {
                let (columns, rows) = inner.engine.pane_geometry(*pane).expect("laid out");
                (*pane, columns, rows)
            })
            .collect::<Vec<_>>()
    };
    let rows = {
        let inner = shared.inner.lock();
        let window = inner.engine.state.window_for_pane(first).expect("window");
        inner
            .engine
            .window_extent(window, zz_protocol::Axis::Vertical)
    };
    assert_eq!(
        rows,
        Some(39),
        "the window takes the client's rows less the status line"
    );
    let mut delivered = fulls(&messages);
    delivered.sort();
    let mut expected = expected;
    expected.sort();
    assert_eq!(delivered, expected, "one Full per pane, at the final size");
}

#[cfg(unix)]
#[test]
fn switching_away_and_back_and_reattaching_repaint_every_visible_pane() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &["new-session", "-d", "-s", "home", QUIET_PANE_COMMAND],
    );
    let first = context.pane.expect("first pane");
    command(
        &shared,
        &mut context,
        &["split-window", "-d", "-v", "-t", "home", QUIET_PANE_COMMAND],
    );
    let home = window_panes(&shared, first);
    assert_eq!(home.len(), 2);
    command(
        &shared,
        &mut context,
        &["new-session", "-d", "-s", "away", QUIET_PANE_COMMAND],
    );
    let away = context.pane.expect("away pane");
    let (client, mailbox) = terminal_client(&shared, (100, 30));

    interactive(&shared, client, &["attach-session", "-t", "home"]);
    drain_until(
        &mailbox,
        Duration::from_millis(150),
        every_pane_has_a_full(&home),
    );

    interactive(&shared, client, &["switch-client", "-t", "away"]);
    drain_until(
        &mailbox,
        Duration::from_millis(150),
        every_pane_has_a_full(&[away]),
    );

    interactive(&shared, client, &["switch-client", "-t", "home"]);
    let back = drain_until(
        &mailbox,
        Duration::from_millis(150),
        every_pane_has_a_full(&home),
    );
    assert_no_repeated_full(after_last_attached(&back));

    interactive(&shared, client, &["attach-session", "-t", "home"]);
    let again = drain_until(
        &mailbox,
        Duration::from_millis(150),
        every_pane_has_a_full(&home),
    );
    assert_no_repeated_full(after_last_attached(&again));
}

fn assert_no_repeated_full(messages: &[ProtocolMessage]) {
    let generations = messages
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalViewport { pane, viewport },
                ..
            }) => Some((*pane, viewport.generation, viewport.view_generation)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let unique = generations.iter().collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), generations.len(), "{generations:?}");
}

#[cfg(unix)]
#[test]
fn a_full_the_client_asks_for_is_sent_at_the_generation_it_already_had() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &["new-session", "-d", "-s", "bad-patch", QUIET_PANE_COMMAND],
    );
    let pane = context.pane.expect("pane");
    let (client, mailbox) = terminal_client(&shared, (90, 20));
    interactive(&shared, client, &["attach-session", "-t", "bad-patch"]);
    drain_until(
        &mailbox,
        Duration::from_millis(150),
        every_pane_has_a_full(&[pane]),
    );

    shared.send_full(client, pane, &mailbox);
    assert!(
        mailbox.state.lock().terminals.is_empty(),
        "the daemon does not resend what the client holds"
    );

    shared.request_full(client, pane, &mailbox);
    let repaint = drain_until(&mailbox, Duration::ZERO, |messages| {
        !fulls(messages).is_empty()
    });
    assert_eq!(fulls(&repaint).len(), 1);
}

fn is_full(message: &ProtocolMessage) -> bool {
    matches!(
        message,
        ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalViewport { .. },
            ..
        })
    )
}

#[test]
fn a_requested_full_replaces_a_patch_queued_at_its_generation() {
    let mailbox = OutboundMailbox::new();
    let pane = PaneId(11);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 1, 5)),
        TerminalEnqueue::Queued
    );
    mailbox.recv().expect("first full");
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_patch_test_message(pane, 2, 5, 6)),
        TerminalEnqueue::Queued
    );
    mailbox.forget_delivered_terminal(pane);
    assert!(mailbox.replace_terminal(pane, &terminal_test_message(pane, 3, 6)));
    let written = decode_protocol_frame(&mailbox.recv().expect("frame")).expect("decode");
    assert!(is_full(&written), "got {}", message_name(&written));
}

#[test]
fn a_patch_queued_before_attached_never_follows_it() {
    let mailbox = OutboundMailbox::new();
    let pane = PaneId(12);
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 1, 5)),
        TerminalEnqueue::Queued
    );
    mailbox.recv().expect("first full");
    mailbox.hold_terminals();
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_patch_test_message(pane, 2, 5, 6)),
        TerminalEnqueue::Queued
    );
    let attached = ProtocolMessage::Attached {
        session: SessionId(1),
        snapshot: MuxSnapshot::default(),
        read_only: false,
        client_flags: String::new(),
    };
    assert!(mailbox.enqueue_attached(&attached));
    assert!(
        mailbox.state.lock().terminals.is_empty(),
        "the patch lost its base"
    );
    assert_eq!(
        mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 3, 6)),
        TerminalEnqueue::Queued
    );
    mailbox.release_terminals();
    let first = decode_protocol_frame(&mailbox.recv().expect("attached")).expect("decode");
    assert!(matches!(first, ProtocolMessage::Attached { .. }));
    let written = decode_protocol_frame(&mailbox.recv().expect("frame")).expect("decode");
    assert!(is_full(&written), "got {}", message_name(&written));
}

#[test]
fn a_resync_drops_queued_patches_and_keeps_queued_fulls() {
    let mailbox = OutboundMailbox::new();
    let (patched, whole) = (PaneId(13), PaneId(14));
    for pane in [patched, whole] {
        assert_eq!(
            mailbox.enqueue_terminal(pane, &terminal_test_message(pane, 1, 5)),
            TerminalEnqueue::Queued
        );
    }
    mailbox.recv().expect("first full");
    mailbox.recv().expect("second full");
    assert_eq!(
        mailbox.enqueue_terminal(patched, &terminal_patch_test_message(patched, 2, 5, 6)),
        TerminalEnqueue::Queued
    );
    assert!(mailbox.replace_terminal(whole, &terminal_test_message(whole, 2, 6)));
    mailbox.forget_delivered_terminals();
    let state = mailbox.state.lock();
    assert!(!state.terminals.contains_key(&patched));
    assert!(
        state
            .terminals
            .get(&whole)
            .is_some_and(|pending| pending.full)
    );
}

#[cfg(unix)]
#[test]
fn a_request_full_on_a_live_pane_with_a_patch_queued_is_answered_with_a_full() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &["new-session", "-d", "-s", "patched", QUIET_PANE_COMMAND],
    );
    let pane = context.pane.expect("pane");
    let (client, mailbox) = terminal_client(&shared, (90, 20));
    interactive(&shared, client, &["attach-session", "-t", "patched"]);
    drain_until(
        &mailbox,
        Duration::from_millis(150),
        every_pane_has_a_full(&[pane]),
    );
    command(
        &shared,
        &mut context,
        &["send-keys", "-t", "patched", "-l", "x"],
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut settled: Option<Instant> = None;
    loop {
        let patch_queued = mailbox
            .state
            .lock()
            .terminals
            .get(&pane)
            .is_some_and(|pending| !pending.full);
        if patch_queued {
            if settled.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(150) {
                break;
            }
        } else {
            settled = None;
        }
        assert!(Instant::now() < deadline, "no patch settled in the queue");
        thread::sleep(Duration::from_millis(5));
    }
    shared.request_full(client, pane, &mailbox);
    let written = drain_until(&mailbox, Duration::from_millis(150), |messages| {
        !messages.is_empty()
    });
    let names = written.iter().map(message_name).collect::<Vec<_>>();
    assert!(
        !fulls(&written).is_empty(),
        "RequestFull answered with {names:?}"
    );
}
