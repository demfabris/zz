use zz_protocol::decode_protocol_frame;

use super::tests::QUIET_PANE_COMMAND;
use super::*;

const COMMAND_CLIENT: ClientId = ClientId(u64::MAX);

struct Frame {
    bytes: usize,
    message: ProtocolMessage,
}

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

fn attached_client(
    shared: &Arc<Shared>,
    session: &str,
    size: (u16, u16),
) -> (ClientId, Arc<OutboundMailbox>) {
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
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("attach-session", ["-t", session]),
        )
        .expect("attach-session");
    (client, mailbox)
}

fn drain(
    mailbox: &OutboundMailbox,
    quiet: Duration,
    done: impl Fn(&[Frame]) -> bool,
) -> Vec<Frame> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut frames = Vec::new();
    let mut last = Instant::now();
    loop {
        let frame = pop_ready_frame(&mut mailbox.state.lock());
        if let Some(frame) = frame {
            frames.push(Frame {
                bytes: frame.len(),
                message: decode_protocol_frame(&frame).expect("decode outbound frame"),
            });
            last = Instant::now();
            continue;
        }
        if done(&frames) && last.elapsed() >= quiet {
            return frames;
        }
        assert!(Instant::now() < deadline, "outbound frames never settled");
        thread::sleep(Duration::from_millis(5));
    }
}

fn full_for(frames: &[Frame], pane: PaneId) -> Option<&TerminalViewport> {
    frames.iter().rev().find_map(|frame| match &frame.message {
        ProtocolMessage::Event(Event {
            payload:
                EventPayload::TerminalViewport {
                    pane: target,
                    viewport,
                },
            ..
        }) if *target == pane => Some(viewport),
        _ => None,
    })
}

fn apply_frames(retained: &mut TerminalViewport, frames: &[Frame], pane: PaneId) -> Vec<usize> {
    let mut patch_bytes = Vec::new();
    for frame in frames {
        match &frame.message {
            ProtocolMessage::Event(Event {
                payload:
                    EventPayload::TerminalViewport {
                        pane: target,
                        viewport,
                    },
                ..
            }) if *target == pane => *retained = viewport.clone(),
            ProtocolMessage::Event(Event {
                payload:
                    EventPayload::TerminalPatch {
                        pane: target,
                        patch,
                    },
                ..
            }) if *target == pane => {
                retained
                    .apply_patch(patch.clone())
                    .expect("a streamed patch applies to the retained frame");
                patch_bytes.push(frame.bytes);
            }
            _ => {}
        }
    }
    patch_bytes
}

fn screen_text(viewport: &TerminalViewport) -> String {
    let mut text = String::new();
    for row in 0..viewport.rows {
        for cell in viewport.row(row).unwrap_or_default() {
            viewport.push_glyph(*cell, &mut text);
        }
        text.push('\n');
    }
    text
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(10));
    }
}

fn pane_terminal(shared: &Shared, pane: PaneId) -> Arc<TerminalSession> {
    Arc::clone(&shared.inner.lock().terminals[&pane])
}

#[cfg(unix)]
#[test]
fn a_typed_key_reaches_an_attached_client_as_a_patch_of_a_few_dozen_bytes() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &[
            "new-session",
            "-d",
            "-s",
            "echo",
            "-x",
            "120",
            "-y",
            "40",
            QUIET_PANE_COMMAND,
        ],
    );
    let pane = context.pane.expect("pane");
    let (client, mailbox) = attached_client(&shared, "echo", (120, 41));
    let frames = drain(&mailbox, Duration::from_millis(250), |frames| {
        full_for(frames, pane).is_some()
    });
    let mut retained = full_for(&frames, pane).expect("a full frame").clone();
    let full_bytes = frames
        .iter()
        .filter(|frame| {
            matches!(
                frame.message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::TerminalViewport { .. },
                    ..
                })
            )
        })
        .map(|frame| frame.bytes)
        .max()
        .expect("a full frame");
    assert!(
        full_bytes < 256,
        "a nearly blank 120x40 full frame took {full_bytes} bytes"
    );
    apply_frames(
        &mut retained,
        &frames[frames
            .iter()
            .rposition(|frame| {
                matches!(
                    frame.message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::TerminalViewport { .. },
                        ..
                    })
                )
            })
            .expect("a full frame")..],
        pane,
    );

    let target = pane.to_string();
    let mut patch_bytes = Vec::new();
    for key in ["z", "y", "x"] {
        command(
            &shared,
            &mut context,
            &["send-keys", "-t", &target, "-l", key],
        );
        let frames = drain(&mailbox, Duration::from_millis(150), |frames| {
            frames.iter().any(|frame| {
                matches!(
                    frame.message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::TerminalPatch { .. },
                        ..
                    })
                )
            })
        });
        patch_bytes.extend(apply_frames(&mut retained, &frames, pane));
    }
    assert!(
        screen_text(&retained).starts_with("zyx"),
        "the retained grid misses the echo: {:?}",
        screen_text(&retained).lines().next()
    );
    assert!(
        patch_bytes.iter().all(|bytes| *bytes <= 64),
        "echo patches took {patch_bytes:?} bytes"
    );
    let latest = pane_terminal(&shared, pane)
        .latest_viewport_for(TerminalViewId(client.0))
        .expect("the client's view frame");
    assert_eq!(retained.cells, latest.cells);
    assert_eq!(retained.cursor, latest.cursor);
    assert_eq!(retained.scrollbar, latest.scrollbar);
}

#[cfg(unix)]
#[test]
fn history_chunks_travel_on_the_terminal_lane_with_blank_tails_dropped() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &[
            "new-session",
            "-d",
            "-s",
            "history",
            "-x",
            "80",
            "-y",
            "10",
            QUIET_PANE_COMMAND,
        ],
    );
    let pane = context.pane.expect("pane");
    let target = pane.to_string();
    command(
        &shared,
        &mut context,
        &["send-keys", "-t", &target, "seq 1 200", "Enter"],
    );
    let terminal = pane_terminal(&shared, pane);
    wait_for("the seq output", || {
        terminal.latest_viewport().scrollbar.total >= 200
    });
    let (client, mailbox) = attached_client(&shared, "history", (80, 11));
    drain(&mailbox, Duration::from_millis(200), |frames| {
        full_for(frames, pane).is_some()
    });
    shared.send_history(client, pane, 0, 50, &mailbox);
    let frames = mailbox.state.lock().reliable.drain(..).collect::<Vec<_>>();
    let frame = frames
        .iter()
        .find(|frame| frame.get(4) == Some(&1) && frame.get(8) == Some(&3))
        .expect("a history chunk on the terminal lane");
    assert!(
        frame.len() < 50 * 8,
        "50 short history rows took {} bytes",
        frame.len()
    );
    let ProtocolMessage::Event(Event {
        payload: EventPayload::HistoryChunk { rows, columns, .. },
        ..
    }) = decode_protocol_frame(frame).expect("decode the history chunk")
    else {
        panic!("expected a history chunk");
    };
    assert_eq!(columns, 80);
    assert_eq!(rows.len(), 50);
    assert!(rows.iter().all(|row| row.len() == 80));
    let text = rows
        .iter()
        .map(|row| {
            row.iter()
                .filter_map(|cell| char::from_u32(cell.glyph()).filter(|glyph| *glyph != '\0'))
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert!(
        text.iter().any(|line| line == "20"),
        "history rows: {text:?}"
    );
}

#[cfg(unix)]
#[test]
fn each_pane_numbers_its_own_frame_stream() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &["new-session", "-d", "-s", "streams", QUIET_PANE_COMMAND],
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
            "streams",
            QUIET_PANE_COMMAND,
        ],
    );
    let second = {
        let inner = shared.inner.lock();
        let window = inner.engine.state.window_for_pane(first).expect("window");
        inner.engine.state.windows[&window]
            .panes
            .keys()
            .copied()
            .find(|pane| *pane != first)
            .expect("second pane")
    };
    let (_, mailbox) = attached_client(&shared, "streams", (100, 30));
    let mut frames = drain(&mailbox, Duration::from_millis(200), |frames| {
        full_for(frames, first).is_some() && full_for(frames, second).is_some()
    });
    for pane in [first, second] {
        command(
            &shared,
            &mut context,
            &["send-keys", "-t", &pane.to_string(), "-l", "abc"],
        );
    }
    frames.extend(drain(&mailbox, Duration::from_millis(200), |_| true));
    for pane in [first, second] {
        let sequences = frames
            .iter()
            .filter_map(|frame| match &frame.message {
                ProtocolMessage::Event(Event {
                    sequence,
                    payload:
                        EventPayload::TerminalViewport { pane: target, .. }
                        | EventPayload::TerminalPatch { pane: target, .. },
                }) if *target == pane => Some(*sequence),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(sequences.len() >= 2, "{pane}: {sequences:?}");
        assert!(
            sequences.windows(2).all(|pair| pair[0] < pair[1]),
            "{pane} frames are out of order: {sequences:?}"
        );
        let next = pane_terminal(&shared, pane).next_stream_sequence();
        assert!(
            sequences.iter().all(|sequence| *sequence < next),
            "{pane} frames carry sequences its terminal never issued: {sequences:?} next {next}"
        );
    }
}
