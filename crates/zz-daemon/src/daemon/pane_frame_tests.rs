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
            .client_entry(client)
            .size
            .replace(client_size_fact(&capabilities).expect("size fact"));
        inner
            .client_entry(client)
            .cell_pixels
            .replace(attach::client_cell_fact(&capabilities).expect("cell fact"));
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

fn output(shared: &Arc<Shared>, context: &mut ExecutionContext, args: &[&str]) -> String {
    shared
        .execute(
            COMMAND_CLIENT,
            ClientKind::Command,
            context,
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap_or_else(|error| panic!("{args:?}: {error:?}"))
        .output
        .to_string()
}

#[cfg(unix)]
#[test]
fn an_oversized_pane_streams_what_its_client_shows_and_keeps_its_own_size() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &[
            "new-session",
            "-d",
            "-s",
            "big",
            "-x",
            "80",
            "-y",
            "24",
            QUIET_PANE_COMMAND,
        ],
    );
    let pane = context.pane.expect("pane");
    let target = pane.to_string();
    command(
        &shared,
        &mut context,
        &["set-option", "-g", "status", "off"],
    );
    let (client, mailbox) = attached_client(&shared, "big", (80, 24));
    let mut retained = TerminalViewport::blank(1, 1, zz_terminal::SessionStatus::Running);
    let mut follow = |what: &str, columns: u16, rows: u16, marker: &str| {
        wait_for(what, || {
            let frames = drain(&mailbox, Duration::from_millis(30), |_| true);
            apply_frames(&mut retained, &frames, pane);
            (retained.columns, retained.rows) == (columns, rows)
                && screen_text(&retained).contains(marker)
        });
        retained.clone()
    };
    follow("the attach frame", 80, 24, "");
    command(
        &shared,
        &mut context,
        &["resize-window", "-t", "big", "-x", "400", "-y", "300"],
    );
    command(
        &shared,
        &mut context,
        &[
            "send-keys",
            "-t",
            &target,
            r"printf 'ZZ_%s\n\033[290;380HZZ_%s' NEAR FAR",
            "Enter",
        ],
    );
    let terminal = pane_terminal(&shared, pane);
    let near = follow(
        "a client-sized frame of the oversized pane",
        80,
        24,
        "ZZ_NEAR",
    );
    assert!(!screen_text(&near).contains("ZZ_FAR"));
    assert!(near.cursor.is_none());
    assert_eq!(near.scrollbar.len, 300);
    assert_eq!(terminal.size(), (400, 300));
    assert_eq!(
        output(
            &shared,
            &mut context,
            &[
                "display-message",
                "-p",
                "-t",
                &target,
                "#{pane_width}x#{pane_height}"
            ],
        ),
        "400x300"
    );
    let capture = output(
        &shared,
        &mut context,
        &["capture-pane", "-p", "-t", &target],
    );
    let lines = capture.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 300);
    assert_eq!(lines[289], format!("{}ZZ_FAR", " ".repeat(379)));

    assert!(shared.apply_client_size_report(
        client,
        ClientKind::Interactive,
        &ExecutionContext::default(),
        390,
        295,
        None,
    ));
    let grown = follow("a frame the size of the grown client", 390, 295, "ZZ_FAR");
    assert!(screen_text(&grown).contains("ZZ_NEAR"));
    assert_eq!(
        grown.cursor.map(|cursor| (cursor.column(), cursor.row())),
        Some((385, 289))
    );
    let latest = terminal
        .latest_viewport_for(TerminalViewId(client.0))
        .expect("the client's view frame");
    assert_eq!(retained.cells, latest.cells);
    assert_eq!(terminal.size(), (400, 300));

    command(
        &shared,
        &mut context,
        &["send-keys", "-t", &target, "seq 1 400", "Enter"],
    );
    wait_for("history above the oversized screen", || {
        terminal.latest_viewport().scrollbar.total > 350
    });
    mailbox.state.lock().reliable.clear();
    shared.send_history(client, pane, 0, 20, &mailbox);
    let (rows, columns) = mailbox
        .state
        .lock()
        .reliable
        .drain(..)
        .find_map(|frame| match decode_protocol_frame(&frame) {
            Ok(ProtocolMessage::Event(Event {
                payload: EventPayload::HistoryChunk { rows, columns, .. },
                ..
            })) => Some((rows, columns)),
            _ => None,
        })
        .expect("a history chunk");
    assert_eq!(columns, 390);
    assert_eq!(rows.len(), 20);
    assert!(rows.iter().all(|row| row.len() == 390));
}

#[cfg(unix)]
#[test]
fn a_client_without_a_terminal_size_bounds_frames_by_the_window_its_reports_imply() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &[
            "new-session",
            "-d",
            "-s",
            "gui",
            "-x",
            "80",
            "-y",
            "24",
            QUIET_PANE_COMMAND,
        ],
    );
    let left = context.pane.expect("left pane");
    command(
        &shared,
        &mut context,
        &["split-window", "-h", "-t", "gui", QUIET_PANE_COMMAND],
    );
    let right = context.pane.expect("right pane");
    command(
        &shared,
        &mut context,
        &["set-option", "-g", "status", "off"],
    );
    command(
        &shared,
        &mut context,
        &["resize-window", "-t", "gui", "-x", "400", "-y", "300"],
    );
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("attach-session", ["-t", "gui"]),
        )
        .expect("attach-session");
    let report = |pane: PaneId, columns: u16, rows: u16| {
        let geometry = TerminalGeometry {
            columns,
            rows,
            cell_width_px: 8,
            cell_height_px: 16,
        };
        assert!(
            shared
                .apply_terminal_size_report(client, pane, geometry, None)
                .expect("size report")
        );
    };
    let frame_extent = |pane: PaneId| {
        pane_terminal(&shared, pane)
            .latest_viewport_for(TerminalViewId(client.0))
            .map(|viewport| (viewport.columns, viewport.rows))
    };
    let implied = |pane: PaneId, columns: u16, rows: u16| {
        shared
            .inner
            .lock()
            .engine
            .window_extent_for_pane_geometry(pane, columns, rows)
            .expect("an implied window extent")
    };
    report(left, 50, 30);
    report(right, 49, 30);
    let small = implied(left, 50, 30).max(implied(right, 49, 30));
    let bounded_by = |extent: (u16, u16)| {
        [left, right].into_iter().all(|pane| {
            let geometry = shared
                .inner
                .lock()
                .engine
                .pane_geometry(pane)
                .expect("geometry");
            frame_extent(pane) == Some((extent.0.min(geometry.0), extent.1.min(geometry.1)))
        })
    };
    wait_for("frames the size of the implied window", || {
        bounded_by(small)
    });
    report(right, 99, 60);
    let large = implied(left, 50, 30);
    let large = (
        large.0.max(implied(right, 99, 60).0),
        large.1.max(implied(right, 99, 60).1),
    );
    assert!(large.0 > small.0 && large.1 > small.1);
    wait_for("both frames following the larger implied window", || {
        bounded_by(large)
    });
    assert_eq!(
        shared
            .inner
            .lock()
            .engine
            .pane_geometry(right)
            .map(|(_, rows)| rows),
        Some(300)
    );
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
fn a_pane_frame_sequence_keeps_growing_across_respawn_pane() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &["new-session", "-d", "-s", "streams", QUIET_PANE_COMMAND],
    );
    let pane = context.pane.expect("pane");
    let target = pane.to_string();
    let (_, mailbox) = attached_client(&shared, "streams", (100, 30));
    let mut frames = drain(&mailbox, Duration::from_millis(200), |frames| {
        full_for(frames, pane).is_some()
    });
    command(
        &shared,
        &mut context,
        &["send-keys", "-t", &target, "-l", "abc"],
    );
    frames.extend(drain(&mailbox, Duration::from_millis(200), |_| true));
    let before = pane_terminal(&shared, pane);
    let respawned_at = frames.len();
    command(
        &shared,
        &mut context,
        &["respawn-pane", "-k", "-t", &target, QUIET_PANE_COMMAND],
    );
    wait_for("the respawned terminal", || {
        !Arc::ptr_eq(&pane_terminal(&shared, pane), &before)
    });
    frames.extend(drain(&mailbox, Duration::from_millis(200), |_| true));
    command(
        &shared,
        &mut context,
        &["send-keys", "-t", &target, "-l", "def"],
    );
    frames.extend(drain(&mailbox, Duration::from_millis(200), |_| true));
    let sequences = |frames: &[Frame]| {
        frames
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
            .collect::<Vec<_>>()
    };
    assert!(
        !sequences(&frames[respawned_at..]).is_empty(),
        "no frames after the respawn"
    );
    let sequences = sequences(&frames);
    assert!(sequences.len() >= 4, "{sequences:?}");
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "{pane} frame sequences went back: {sequences:?}"
    );
}

#[derive(Default)]
struct ClientState {
    retained: BTreeMap<PaneId, TerminalViewport>,
    patches: usize,
    fulls: usize,
    rejected: usize,
    refetch: Vec<PaneId>,
    last_frame: Option<Instant>,
}

struct Client {
    id: ClientId,
    mailbox: Arc<OutboundMailbox>,
    state: Arc<Mutex<ClientState>>,
    stop: Arc<AtomicBool>,
}

impl Client {
    fn new(id: ClientId, mailbox: Arc<OutboundMailbox>) -> Self {
        let state = Arc::new(Mutex::new(ClientState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            let mailbox = Arc::clone(&mailbox);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let frame = pop_ready_frame(&mut mailbox.state.lock());
                    let Some(frame) = frame else {
                        thread::sleep(Duration::from_micros(300));
                        continue;
                    };
                    let mut state = state.lock();
                    state.last_frame = Some(Instant::now());
                    match decode_protocol_frame(&frame).expect("decode outbound frame") {
                        ProtocolMessage::Event(Event {
                            payload: EventPayload::TerminalViewport { pane, viewport },
                            ..
                        }) => {
                            state.fulls += 1;
                            state.retained.insert(pane, viewport);
                        }
                        ProtocolMessage::Event(Event {
                            payload: EventPayload::TerminalPatch { pane, patch },
                            ..
                        }) => {
                            state.patches += 1;
                            let applied = state
                                .retained
                                .get_mut(&pane)
                                .map(|retained| retained.apply_patch(patch));
                            if !matches!(applied, Some(Ok(()))) {
                                eprintln!(
                                    "client {} rejected a patch for {pane}: {applied:?}",
                                    id.0
                                );
                                state.rejected += 1;
                                state.retained.remove(&pane);
                                state.refetch.push(pane);
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
        Self {
            id,
            mailbox,
            state,
            stop,
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn describe(left: &TerminalViewport, right: &TerminalViewport) -> String {
    let mut out = Vec::new();
    if left.generation != right.generation || left.view_generation != right.view_generation {
        out.push(format!(
            "gen {}/{} vs {}/{}",
            left.generation, left.view_generation, right.generation, right.view_generation
        ));
    }
    if left.cells != right.cells {
        let columns = usize::from(right.columns.max(1));
        let first = left
            .cells
            .iter()
            .zip(right.cells.iter())
            .position(|(a, b)| a != b);
        out.push(format!(
            "cells differ at {:?} (row {:?})",
            first,
            first.map(|index| index / columns)
        ));
    }
    if left.dictionary != right.dictionary {
        out.push("dictionary".to_owned());
    }
    if left.cursor != right.cursor {
        out.push(format!("cursor {:?} vs {:?}", left.cursor, right.cursor));
    }
    if left.scrollbar != right.scrollbar {
        out.push(format!(
            "scrollbar {:?} vs {:?}",
            left.scrollbar, right.scrollbar
        ));
    }
    if left.presentation != right.presentation {
        out.push(format!(
            "presentation {:?} vs {:?}",
            left.presentation, right.presentation
        ));
    }
    if left.mode != right.mode {
        out.push(format!("mode {:?} vs {:?}", left.mode, right.mode));
    }
    if left.overlays != right.overlays {
        out.push("overlays".to_owned());
    }
    if left.search != right.search {
        out.push("search".to_owned());
    }
    if left.unseen_output != right.unseen_output {
        out.push("unseen".to_owned());
    }
    if left.status != right.status {
        out.push("status".to_owned());
    }
    if left.mouse_tracking != right.mouse_tracking || left.kitty_keyboard != right.kitty_keyboard {
        out.push("input modes".to_owned());
    }
    if left.kitty_placements != right.kitty_placements {
        out.push("kitty".to_owned());
    }
    if left.foreground != right.foreground || left.background != right.background {
        out.push("colors".to_owned());
    }
    out.join("; ")
}

static COMPARED: AtomicU64 = AtomicU64::new(0);

fn settle(shared: &Arc<Shared>, clients: &mut [Client], panes: &[PaneId], label: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let mut mismatches = Vec::new();
        let mut recent = false;
        for client in clients.iter() {
            let refetch = std::mem::take(&mut client.state.lock().refetch);
            for pane in refetch {
                shared.request_full(client.id, pane, &client.mailbox);
            }
            let state = client.state.lock();
            recent |= state
                .last_frame
                .is_some_and(|last| last.elapsed() < Duration::from_millis(150));
            let streamed = shared
                .read_client(client.id, |c| {
                    c.and_then(|c| c.streamed_terminals.as_ref()).cloned()
                })
                .unwrap_or_default();
            for pane in panes.iter().filter(|pane| streamed.contains_key(pane)) {
                let Some(terminal) = shared.inner.lock().terminals.get(pane).cloned() else {
                    continue;
                };
                let Some(latest) = terminal.latest_viewport_for(TerminalViewId(client.id.0)) else {
                    continue;
                };
                COMPARED.fetch_add(1, Ordering::Relaxed);
                match state.retained.get(pane) {
                    Some(retained) if *retained == *latest => {}
                    Some(retained) => mismatches.push(format!(
                        "client {} pane {pane}: {}",
                        client.id.0,
                        describe(retained, &latest)
                    )),
                    None => mismatches.push(format!(
                        "client {} pane {pane}: nothing retained",
                        client.id.0
                    )),
                }
            }
        }
        if mismatches.is_empty() && !recent {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{label}: client state never converged: {mismatches:#?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn two_clients_rebuild_every_view_exactly_from_streamed_frames() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    command(
        &shared,
        &mut context,
        &[
            "new-session",
            "-d",
            "-s",
            "torture",
            "-x",
            "80",
            "-y",
            "24",
            QUIET_PANE_COMMAND,
        ],
    );
    let first = context.pane.expect("pane");
    let (a, a_mailbox) = attached_client(&shared, "torture", (80, 25));
    let (b, b_mailbox) = attached_client(&shared, "torture", (80, 25));
    let mut clients = vec![Client::new(a, a_mailbox), Client::new(b, b_mailbox)];
    let mut panes = vec![first];
    settle(&shared, &mut clients, &panes, "attach");
    let lines: &[&str] = &[
        r"printf '\033[31mred\033[0m \033[44mblue-bg\033[K\033[0m\n'",
        r"printf '\344\270\255\346\226\207wide\n'",
        r"printf 'e\314\201 \360\237\221\215\360\237\217\275 zwj \360\237\221\250\342\200\215\360\237\221\251\n'",
        r"printf '\033]8;;http://example.test/\033\\link\033]8;;\033\\ after\n'",
        r"printf '\033]2;title-one\007'",
        r"printf '\033]7;file://host/tmp/review\007'",
        "seq 1 60",
        r"printf '\033[5;15r\033[5;1H\033M\033M\033[r'",
        r"printf '\033[3;1Habcdefghij\033[3;3H\033[4@\033[3;1H\033[2P'",
        r"printf '\033[6;1H\033[2L\033[8;1H\033[3M'",
        r"printf '\033[?1049h\033[2J\033[1;1Halt screen\033[10;70H\344\270\255'",
        r"printf '\033[?1049l'",
        r"printf '\033[5 q\033[?25l'",
        r"printf '\033[?25h\033[2 q'",
        r"printf '\033[?1000h'",
        r"printf '\033[?1000l'",
        r"printf '\033[4;79H\344\270\255X\n'",
        r"printf '\033[4;1H\344\270\255\344\270\255\033[4;2Hx'",
        r"printf '\033[S\033[T\033[2S'",
        r"printf '%0200d\n' 0",
        r"printf '\033[38;5;196mX\033[48;2;1;2;3mY\033[0m\n'",
        r"printf '\033[4:3mcurly\033[0m \033[58;5;4m\033[4mcolored\033[0m\n'",
        r"printf 'a\tb\tc\n'",
        r"printf '\033[44m\033[2J\033[0m'",
        r"printf '\033[1;1H\033[0J'",
        r"printf '\033]2;title-two\007\033]8;;http://x.test/\033\\\033[45m  \033[K\033]8;;\033\\\033[0m\n'",
        "seq 1 200",
        r"printf '\033[2J\033[3J\033[H'",
        r"printf '\033[10;1H\033[1;31m\342\224\200\342\224\200\342\224\200\342\224\200\342\224\200\342\224\200\342\224\200\342\224\200\033[0m\n'",
    ];
    for key in "echo typed".chars() {
        command(
            &shared,
            &mut context,
            &[
                "send-keys",
                "-t",
                &first.to_string(),
                "-l",
                &key.to_string(),
            ],
        );
        settle(&shared, &mut clients, &panes, &format!("key {key}"));
    }
    command(
        &shared,
        &mut context,
        &["send-keys", "-t", &first.to_string(), "Enter"],
    );
    settle(&shared, &mut clients, &panes, "typed enter");
    for (index, line) in lines.iter().enumerate() {
        command(
            &shared,
            &mut context,
            &["send-keys", "-t", &first.to_string(), line, "Enter"],
        );
        settle(
            &shared,
            &mut clients,
            &panes,
            &format!("line {index}: {line}"),
        );
    }
    let target = first.to_string();
    for args in [
        vec!["copy-mode", "-t", target.as_str()],
        vec!["send-keys", "-t", target.as_str(), "-X", "cursor-up"],
        vec!["send-keys", "-t", target.as_str(), "-X", "page-up"],
        vec!["send-keys", "-t", target.as_str(), "-X", "begin-selection"],
        vec!["send-keys", "-t", target.as_str(), "-X", "cursor-down"],
        vec![
            "send-keys",
            "-t",
            target.as_str(),
            "-X",
            "search-backward",
            "1",
        ],
        vec!["send-keys", "-t", target.as_str(), "-X", "cancel"],
    ] {
        command(&shared, &mut context, &args);
        settle(&shared, &mut clients, &panes, &format!("{args:?}"));
    }
    command(
        &shared,
        &mut context,
        &["split-window", "-t", "torture", "-h", QUIET_PANE_COMMAND],
    );
    panes = {
        let inner = shared.inner.lock();
        let window = inner.engine.state.window_for_pane(first).expect("window");
        inner.engine.state.windows[&window]
            .panes
            .keys()
            .copied()
            .collect()
    };
    settle(&shared, &mut clients, &panes, "split");
    for pane in panes.clone() {
        command(
            &shared,
            &mut context,
            &["send-keys", "-t", &pane.to_string(), "seq 1 40", "Enter"],
        );
    }
    settle(&shared, &mut clients, &panes, "split output");
    command(
        &shared,
        &mut context,
        &["resize-window", "-t", "torture", "-x", "100", "-y", "30"],
    );
    settle(&shared, &mut clients, &panes, "resize");
    command(
        &shared,
        &mut context,
        &["resize-pane", "-t", &target, "-L", "7"],
    );
    settle(&shared, &mut clients, &panes, "resize-pane");
    for pane in panes.clone() {
        command(
            &shared,
            &mut context,
            &[
                "send-keys",
                "-t",
                &pane.to_string(),
                r"printf '\033[2;1H\344\270\255\033[31mafter\033[0m\033[K\n'; seq 1 5",
                "Enter",
            ],
        );
    }
    settle(&shared, &mut clients, &panes, "after resize output");
    command(&shared, &mut context, &["resize-pane", "-Z", "-t", &target]);
    settle(&shared, &mut clients, &panes, "zoom");
    command(&shared, &mut context, &["resize-pane", "-Z", "-t", &target]);
    settle(&shared, &mut clients, &panes, "unzoom");
    command(
        &shared,
        &mut context,
        &["respawn-pane", "-k", "-t", &target, QUIET_PANE_COMMAND],
    );
    settle(&shared, &mut clients, &panes, "respawn");
    command(
        &shared,
        &mut context,
        &[
            "send-keys",
            "-t",
            &target,
            r"printf 'after respawn\n'",
            "Enter",
        ],
    );
    settle(&shared, &mut clients, &panes, "after respawn");
    eprintln!("compared {} view frames", COMPARED.load(Ordering::Relaxed));
    for client in &clients {
        let state = client.state.lock();
        eprintln!(
            "client {}: {} patches, {} fulls, {} rejected",
            client.id.0, state.patches, state.fulls, state.rejected
        );
        assert_eq!(state.rejected, 0, "client {} rejected a patch", client.id.0);
        assert!(state.patches > 0, "client {} applied no patch", client.id.0);
    }
}
