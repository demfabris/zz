//! The GTK client's protocol half against real in-process daemons: attach,
//! render, type, split, resize, flood, and outlive a daemon restart. Nothing
//! here touches a widget, which is the point — the engine is the layer a
//! display-less machine can still prove.
//!
//! Every wait is content-driven and deadline-bounded. A sleep is never an
//! assertion: the status line republishes about once a second forever, so
//! "wait until nothing arrives" would never return.

#![cfg(unix)]

use std::{
    collections::HashSet,
    ffi::OsString,
    fmt::Write as _,
    io::ErrorKind,
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use zz_client::{ClientCore, Outbound};
use zz_daemon::{CommandClient, Daemon, Endpoint, HostEntry, InteractiveClient};
use zz_gtk::engine::{Engine, EngineEvent, HostId, HostState};
use zz_protocol::{CommandInvocation, InputMessage, PaneId, ProtocolMessage};
use zz_terminal::{
    Glyph, KeyAction, KeyCode, KeyInput, Modifiers, PackedCell, PackedStyle, TerminalColorScheme,
    TerminalViewport,
};

const FIXTURE: &str = "printf 'zz-gtk-ready\\r\\n'; exec /bin/cat";
const REBORN: &str = "printf 'zz-gtk-reborn\\r\\n'; exec /bin/cat";
const FLOOD: &str = "seq 1 20000; printf 'zz-gtk-flooded\\r\\n'; exec /bin/cat";
const SESSION: &str = "gtk";
/// Generous on purpose: the flood tests share the machine with up to a dozen
/// concurrently booted daemons under `cargo test`, and one minute has proven
/// too tight under that load while never being approached solo.
const DEADLINE: Duration = Duration::from_mins(3);
const POLL: Duration = Duration::from_millis(10);
const COLUMNS: u16 = 80;
const ROWS: u16 = 24;
const CELL_WIDTH_PX: u32 = 8;
const CELL_HEIGHT_PX: u32 = 16;

#[test]
fn the_engine_attaches_renders_and_echoes_what_it_sends() {
    let daemon = Fixture::boot("attach", FIXTURE);
    let engine = connect(&daemon);
    let mut watch = Watch::default();

    let pane = watch.poll(&engine, "a terminal pane in the attached session", |_| {
        first_terminal_pane(&engine)
    });
    let view = engine.session_view().expect("an attached session view");
    assert_eq!(
        view.name, SESSION,
        "the default attach must land on the only session the fixture left behind"
    );
    assert!(
        watch.saw("attached"),
        "the engine never published its attachment"
    );

    engine.resize_terminal(pane, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the fixture banner", |_| {
        contains(&engine, pane, "zz-gtk-ready")
    });
    assert!(
        watch.saw("frames"),
        "viewport content arrived without a frame notification"
    );

    engine.send_key(pane, typed('!'), false);
    engine.send_text(pane, "gtk-echo".to_owned());
    watch.poll(&engine, "the echoed key and text", |_| {
        contains(&engine, pane, "!gtk-echo")
    });

    daemon.shutdown();
}

/// The reconnect contract, end to end: the engine must keep answering with the
/// dead connection's state — that is what leaves the last frame on screen —
/// re-ingest the new hello without the reset that would blank the workspace,
/// and converge on the replacement session.
#[test]
fn a_restarted_daemon_is_reconnected_to_without_blanking_the_last_frame() {
    let daemon = Fixture::boot("recon", FIXTURE);
    let relay = Relay::start("relay", &daemon.socket);
    let engine = Engine::connect(
        &Endpoint::Local(relay.front.clone()),
        "",
        TerminalColorScheme::Dark,
    )
    .expect("connect the engine through the relay");
    let mut watch = Watch::default();

    let pane = watch.poll(&engine, "the first pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(pane, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the fixture banner", |_| {
        contains(&engine, pane, "zz-gtk-ready")
    });
    let frozen = engine.viewport(pane).map(|viewport| signature(&viewport));
    let session = engine.attached_session().expect("an attached session");

    relay.cut();
    daemon.stop();
    watch.poll(&engine, "the first reconnect attempt", |watch| {
        watch.saw("reconnecting").then_some(())
    });

    assert_eq!(
        engine.viewport(pane).map(|viewport| signature(&viewport)),
        frozen,
        "the dropped connection must leave the last frame exactly as it was"
    );
    assert_eq!(
        engine.attached_session(),
        Some(session),
        "the remembered session is what the engine re-attaches, so it must survive the drop"
    );
    assert_eq!(
        engine.session_view().map(|view| view.name),
        Some(SESSION.to_owned()),
        "a dropped connection must not clear the attachment"
    );

    daemon.respawn(REBORN);
    relay.restore();
    let reborn = watch.poll(&engine, "the replacement pane", |_| {
        first_terminal_pane(&engine)
            .filter(|pane| contains(&engine, *pane, "zz-gtk-reborn").is_some())
    });
    assert!(
        watch.saw("reconnected"),
        "the engine converged without announcing the reconnect"
    );
    assert_eq!(
        engine.session_view().map(|view| view.name),
        Some(SESSION.to_owned()),
        "the replacement session is the one the engine must land on"
    );

    engine.resize_terminal(reborn, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    engine.send_text(reborn, "after-reconnect\r\n".to_owned());
    watch.poll(&engine, "input on the reconnected pane", |_| {
        contains(&engine, reborn, "after-reconnect")
    });

    relay.stop();
    daemon.shutdown();
}

/// A transport that blinks — the common case over ssh — must land the client
/// back on the session it was attached to. The daemon here keeps its boot
/// session, so the default attach target and the remembered one are different
/// sessions: a client whose reconnect clears the attachment instead of adopting
/// the hello alone has nothing left to re-attach and lands on the default.
#[test]
fn a_transport_blip_reattaches_the_remembered_session_not_the_default() {
    let daemon = Fixture::boot_beside_the_default("blip", FIXTURE);
    let relay = Relay::start("blip-relay", &daemon.socket);
    let engine = Engine::connect(
        &Endpoint::Local(relay.front.clone()),
        SESSION,
        TerminalColorScheme::Dark,
    )
    .expect("connect the engine through the relay");
    let mut watch = Watch::default();

    let pane = watch.poll(&engine, "the fixture pane", |_| {
        first_terminal_pane(&engine)
    });
    let session = engine.attached_session().expect("an attached session");
    assert!(engine.resize_terminal(pane, 90, 28, CELL_WIDTH_PX, CELL_HEIGHT_PX));
    watch.poll(&engine, "the fixture banner", |_| {
        contains(&engine, pane, "zz-gtk-ready")
    });

    watch.forget("attached");
    relay.cut();
    watch.poll(&engine, "the first reconnect attempt", |watch| {
        watch.saw("reconnecting").then_some(())
    });
    relay.restore();
    watch.poll(&engine, "the re-attachment", |watch| {
        watch.saw("attached").then_some(())
    });

    assert_eq!(
        engine.attached_session(),
        Some(session),
        "a reconnect must re-attach the remembered session, not the daemon's default"
    );
    assert_eq!(
        engine.session_view().map(|view| view.name),
        Some(SESSION.to_owned()),
        "the fixture session is the one the engine was attached to"
    );
    assert!(
        !engine.resize_terminal(pane, 90, 28, CELL_WIDTH_PX, CELL_HEIGHT_PX),
        "a reconnect must republish the geometry the widgets last asked for, \
         because only a fresh allocation would ever ask again"
    );

    engine.send_text(pane, "after-the-blip\r\n".to_owned());
    watch.poll(&engine, "input on the re-attached pane", |_| {
        contains(&engine, pane, "after-the-blip")
    });

    relay.stop();
    daemon.shutdown();
}

/// Frame fanout is per pane, and so is forgetting one: a killed pane must leave
/// nothing behind in the inbox for a UI that has not drained yet.
#[test]
fn a_split_publishes_both_panes_and_a_kill_forgets_the_dead_one() {
    let daemon = Fixture::boot("split", FIXTURE);
    let engine = connect(&daemon);
    let mut watch = Watch::default();

    let first = watch.poll(&engine, "the first pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(first, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the first pane's banner", |_| {
        contains(&engine, first, "zz-gtk-ready")
    });

    engine.execute(CommandInvocation::new(
        "split-window",
        ["-t", &first.to_string(), FIXTURE],
    ));
    let second = watch.poll(&engine, "the split pane", |_| {
        terminal_panes(&engine)
            .into_iter()
            .find(|pane| *pane != first)
    });
    engine.resize_terminal(second, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the split pane's banner", |_| {
        contains(&engine, second, "zz-gtk-ready")
    });

    watch.hold_frames = true;
    engine.send_text(second, "doomed\r\n".to_owned());
    watch.poll(&engine, "output queued for the doomed pane", |_| {
        contains(&engine, second, "doomed")
    });

    engine.kill_pane(second);
    watch.poll(&engine, "the snapshot to drop the dead pane", |_| {
        (!terminal_panes(&engine).contains(&second)).then_some(())
    });

    assert!(
        engine.viewport(second).is_none(),
        "the core kept a viewport for a pane the daemon removed"
    );
    assert!(
        engine
            .take_frames()
            .iter()
            .all(|frame| frame.pane != second),
        "the inbox handed the UI a frame for a pane that no longer exists"
    );

    daemon.shutdown();
}

/// Widgets re-measure on every allocation, so the geometry filter is the only
/// thing between a window drag and a resize storm. The return value is the
/// seam: true means the geometry reached the wire, and the daemon-side reflow
/// proves the wire message was real.
#[test]
fn an_identical_resize_never_reaches_the_wire_twice() {
    let daemon = Fixture::boot("resize", FIXTURE);
    let engine = connect(&daemon);
    let mut watch = Watch::default();

    let pane = watch.poll(&engine, "the first pane", |_| first_terminal_pane(&engine));

    assert!(
        engine.resize_terminal(pane, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX),
        "the first geometry a pane ever gets must be published"
    );
    watch.poll(&engine, "the pane to reflow to 80x24", |_| {
        sized(&engine, pane, COLUMNS, ROWS)
    });
    assert!(
        !engine.resize_terminal(pane, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX),
        "a geometry the daemon already has must be swallowed"
    );
    assert!(
        !engine.resize_terminal(pane, 0, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX),
        "an empty allocation must never reach the wire"
    );

    assert!(
        engine.resize_terminal(pane, 100, 30, CELL_WIDTH_PX, CELL_HEIGHT_PX),
        "a changed geometry must be published"
    );
    watch.poll(&engine, "the pane to reflow to 100x30", |_| {
        sized(&engine, pane, 100, 30)
    });
    assert!(
        !engine.resize_terminal(pane, 100, 30, CELL_WIDTH_PX, CELL_HEIGHT_PX),
        "the filter must track the newest geometry, not the first"
    );
    assert!(
        engine.resize_terminal(pane, 100, 30, CELL_WIDTH_PX * 2, CELL_HEIGHT_PX),
        "cell pixels are part of the geometry the daemon needs"
    );

    daemon.shutdown();
}

/// The strongest oracle a new client has: two independent reductions of the
/// same stream — this engine and a plain [`ClientCore`] — must resolve to the
/// same screen. Content is compared by resolved glyph and style value, never by
/// style id: a patched viewport and a fresh full frame intern differently.
#[test]
fn the_engine_and_a_plain_core_converge_on_the_same_content() {
    let daemon = Fixture::boot("cross", FIXTURE);
    let engine = connect(&daemon);
    let mut reference = Reference::attach(&daemon.socket, SESSION);
    let mut watch = Watch::default();

    let pane = watch.poll(&engine, "the first pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(pane, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    reference.resize(pane);
    watch.poll(&engine, "the fixture banner", |_| {
        contains(&engine, pane, "zz-gtk-ready")
    });

    for chunk in 0..20 {
        let mut lines = String::new();
        for line in 0..100 {
            let _ = writeln!(lines, "flood-{:05}\r", chunk * 100 + line);
        }
        engine.send_text(pane, lines);
    }

    let deadline = Instant::now() + DEADLINE;
    loop {
        watch.drain(&engine);
        reference.pump();
        let ours = engine.viewport(pane).map(|viewport| signature(&viewport));
        let theirs = reference.viewport(pane).map(signature);
        if ours.is_some() && ours == theirs && contains(&engine, pane, "flood-01999").is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the engine and a plain core never converged on the flooded pane"
        );
        thread::sleep(POLL);
    }

    daemon.shutdown();
}

/// A pane that dumps more than a compositor could ever paint must not strand
/// the engine: the inbox keeps the newest frame per pane, so a drain hands the
/// UI a bounded batch however far behind it is, and the pane still answers
/// afterwards.
#[test]
fn a_flooded_pane_stays_live_and_drains_in_bounded_batches() {
    let daemon = Fixture::boot("flood", FLOOD);
    let engine = connect(&daemon);
    let mut watch = Watch::default();

    let pane = watch.poll(&engine, "the first pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(pane, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the end of the flood", |_| {
        contains(&engine, pane, "zz-gtk-flooded")
    });

    assert!(watch.frames > 0, "the flood produced no frames at all");
    assert!(
        watch.widest_batch <= 1,
        "a single pane coalesces to one frame per drain, but a batch carried {}",
        watch.widest_batch
    );

    engine.send_text(pane, "still-here\r\n".to_owned());
    watch.poll(&engine, "the pane to answer after the flood", |_| {
        contains(&engine, pane, "still-here")
    });

    daemon.shutdown();
}

/// The scrollback ring, end to end against a real daemon: ask for history,
/// absorb what comes back, and hand a scroll the rows it needs to paint without
/// another round trip. The ring is deliberately fed only by these requests —
/// nothing about it runs while frames do — so this is the whole contract.
#[test]
fn scrollback_is_backfilled_on_request_and_answers_a_scrolled_window() {
    let daemon = Fixture::boot("history", FLOOD);
    let engine = connect(&daemon);
    let mut watch = Watch::default();

    let pane = watch.poll(&engine, "the first pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(pane, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the end of the flood", |_| {
        contains(&engine, pane, "zz-gtk-flooded")
    });
    let scrollbar = watch.poll(&engine, "a pane with scrollback to walk", |_| {
        let scrollbar = engine.viewport(pane)?.scrollbar;
        (scrollbar.total > scrollbar.len).then_some(scrollbar)
    });
    assert_eq!(
        engine.history_rows(pane, &engine.viewport(pane).expect("a viewport")),
        0,
        "the ring must start empty: no frame ever fills it"
    );

    // One screen back is what a wheel notch would be reaching for. Asking the
    // ring where it stands is what anchors it, so a scroll always does that
    // first; a request against an unanchored ring has nowhere to aim.
    let target = scrollbar.offset.saturating_sub(u32::from(ROWS));
    engine.history_rows(pane, &engine.viewport(pane).expect("a viewport"));
    engine.request_history(pane, target);
    let retained = watch.poll(&engine, "the backfilled scrollback", |_| {
        let viewport = engine.viewport(pane)?;
        let retained = engine.history_rows(pane, &viewport);
        (retained >= usize::from(ROWS)).then_some(retained)
    });
    assert!(
        retained <= zz_gtk::engine::MAX_HISTORY_ROWS,
        "the ring grew past its cap: {retained} rows"
    );

    let window = engine.history_window(pane, target, ROWS);
    assert_eq!(window.len(), usize::from(ROWS));
    assert!(
        window.iter().all(Option::is_some),
        "a covered window must not leave rows for the painter to shim"
    );
    let columns = engine.viewport(pane).expect("a viewport").columns;
    for row in window.iter().flatten() {
        assert_eq!(
            row.cells.len(),
            usize::from(columns),
            "every scrollback row is exactly as wide as the pane"
        );
    }
    assert!(
        engine
            .history_window(pane, 0, ROWS)
            .iter()
            .any(Option::is_none),
        "rows the walk has not reached yet must read as absent, not as blanks"
    );

    // New output moves the scrollback underneath the retained indices, and a
    // shifted index paints the wrong row — so the ring retires rather than lie.
    engine.send_text(pane, "after-the-ring\r\n".to_owned());
    watch.poll(&engine, "output past the ring's anchor", |_| {
        contains(&engine, pane, "after-the-ring")
    });
    assert_eq!(
        engine.history_rows(pane, &engine.viewport(pane).expect("a viewport")),
        0,
        "output must retire the rows rather than leave them misaligned"
    );

    daemon.shutdown();
}

/// The two surfaces the daemon opens on this client's behalf, driven by the
/// same key path the widget uses. Neither is a chord the client resolves: the
/// prefix table turns `?` into `list-keys`, and copy mode turns the search
/// binding into a request that the client show a search prompt.
#[test]
fn the_daemon_opens_the_output_pager_and_asks_for_the_search_prompt() {
    let daemon = Fixture::boot("surfaces", FIXTURE);
    let engine = connect(&daemon);
    let mut watch = Watch::default();

    let pane = watch.poll(&engine, "the first pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(pane, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the fixture banner", |_| {
        contains(&engine, pane, "zz-gtk-ready")
    });
    assert!(engine.command_output().is_none());

    engine.send_key(pane, control('b'), false);
    engine.send_key(pane, typed('?'), false);
    let (anchor, viewport) = watch.poll(&engine, "the command-output view", |_| {
        engine.command_output()
    });
    assert_eq!(
        anchor, pane,
        "the pager is anchored to the pane it opened over"
    );
    assert!(
        matches!(viewport.mode, zz_terminal::TerminalMode::View { .. }),
        "the pager is a frozen view, which is what makes its keys copy-mode keys"
    );

    // The pager has geometry of its own, on a lane that carries no pane id.
    engine.send(InputMessage::ResizeCommandOutput {
        columns: COLUMNS,
        rows: ROWS,
        cell_width_px: CELL_WIDTH_PX,
        cell_height_px: CELL_HEIGHT_PX,
    });
    watch.poll(&engine, "the key table the pager was opened for", |_| {
        let (_, viewport) = engine.command_output()?;
        (viewport.columns == COLUMNS
            && viewport.rows == ROWS
            && resolved_text(&viewport).contains("bind-key"))
        .then_some(())
    });

    engine.send_key(pane, typed('q'), false);
    watch.poll(&engine, "the pager to close", |_| {
        engine.command_output().is_none().then_some(())
    });

    // `C-s` is the emacs copy-mode search binding, and `mode-keys` defaults to
    // emacs; the vi table spells the same command `/`.
    engine.send_key(pane, control('b'), false);
    engine.send_key(pane, typed('['), false);
    engine.send_key(pane, control('s'), false);
    let opened = watch.poll(&engine, "the daemon's search prompt request", |watch| {
        watch.search_prompt
    });
    assert_eq!(opened, pane);

    daemon.shutdown();
}

/// Two daemons, dialled as "local" and as a configured fleet host. The ssh tier
/// is bypassed on purpose: a host is an `Endpoint`, and `unix://` is one the
/// desktop already accepts in a `host-` line, so a second socket exercises
/// every layer above the transport without needing a second machine.
#[test]
fn a_configured_host_joins_the_tree_and_activating_it_moves_the_workspace() {
    let local = Fixture::boot("fleet-local", FIXTURE);
    let remote = Fixture::boot("fleet-remote", REBORN);
    let engine = connect(&local);
    let mut watch = Watch::default();

    let here = watch.poll(&engine, "the local pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(here, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the local banner", |_| {
        contains(&engine, here, "zz-gtk-ready")
    });

    assert!(
        engine.set_fleet_hosts(&[host_entry("remote", &remote.socket)]),
        "a host the fleet did not have is a change"
    );
    let host = watch.poll(&engine, "the host to connect", |_| {
        let hosts = engine.hosts();
        let host = hosts.get(1)?;
        (host.state == HostState::Connected && !host.snapshot.sessions.is_empty())
            .then_some(host.id)
    });
    assert_ne!(host, HostId::LOCAL);
    assert!(
        !engine.set_fleet_hosts(&[host_entry("remote", &remote.socket)]),
        "the same file must not re-dial a host that is already connected"
    );

    let hosts = engine.hosts();
    assert_eq!(hosts.len(), 2, "the local daemon and one host");
    assert_eq!(hosts[0].id, HostId::LOCAL);
    assert_eq!(hosts[1].name, "remote");
    assert!(
        hosts[0].attached.is_some(),
        "the local daemon is the one the workspace starts on"
    );
    assert!(
        hosts[1].attached.is_none(),
        "a host is connected but unattached until one of its rows is activated: \
         an attachment is what makes a daemon stream frames"
    );
    assert_eq!(
        engine.active_host(),
        HostId::LOCAL,
        "adding a host must not move the workspace off the machine it is on"
    );

    let session = hosts[1].snapshot.sessions[0].id;
    engine.attach_host_session(host, session);
    assert_eq!(engine.active_host(), host);
    let there = watch.poll(&engine, "the host's pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(there, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the host's banner", |_| {
        contains(&engine, there, "zz-gtk-reborn")
    });
    engine.send_text(there, "over-there\r\n".to_owned());
    watch.poll(&engine, "input on the host's pane", |_| {
        contains(&engine, there, "over-there")
    });
    assert_eq!(
        engine.hosts()[1].attached,
        Some(session),
        "activating a row is what attaches its host"
    );

    local.shutdown();
    remote.shutdown();
}

/// The property that makes a fleet worth having: one machine going away is one
/// machine going away. The dead host keeps the frames it had, the live one keeps
/// answering, and nothing quietly re-points the workspace at the local daemon.
#[test]
fn a_dead_host_freezes_only_itself_and_removing_it_returns_to_the_local_daemon() {
    let local = Fixture::boot("fleet-live", FIXTURE);
    let remote = Fixture::boot("fleet-dead", REBORN);
    let relay = Relay::start("fleet-relay", &remote.socket);
    let engine = connect(&local);
    let mut watch = Watch::default();

    let here = watch.poll(&engine, "the local pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(here, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the local banner", |_| {
        contains(&engine, here, "zz-gtk-ready")
    });

    engine.set_fleet_hosts(&[host_entry("remote", &relay.front)]);
    let (host, session) = watch.poll(&engine, "the host to connect", |_| {
        let hosts = engine.hosts();
        let host = hosts.get(1)?;
        let session = host.snapshot.sessions.first()?;
        (host.state == HostState::Connected).then_some((host.id, session.id))
    });
    engine.attach_host_session(host, session);
    let there = watch.poll(&engine, "the host's pane", |_| first_terminal_pane(&engine));
    engine.resize_terminal(there, COLUMNS, ROWS, CELL_WIDTH_PX, CELL_HEIGHT_PX);
    watch.poll(&engine, "the host's banner", |_| {
        contains(&engine, there, "zz-gtk-reborn")
    });
    let frozen = engine.viewport(there).map(|viewport| signature(&viewport));

    relay.cut();
    let attempt = watch.poll(&engine, "the host's own retry ladder", |_| {
        match engine.hosts().get(1).map(|host| host.state.clone()) {
            Some(HostState::Reconnecting { attempt }) => Some(attempt),
            _ => None,
        }
    });
    assert!(attempt >= 1);
    assert_eq!(
        engine.active_host(),
        host,
        "a host that failed must never hand the workspace back to the local daemon"
    );
    assert_eq!(
        engine.viewport(there).map(|viewport| signature(&viewport)),
        frozen,
        "the dead host's last frame is what stays on screen"
    );
    assert_eq!(
        engine.hosts()[0].state,
        HostState::Connected,
        "the local daemon is a different connection and did not notice"
    );

    // The live daemon is not merely marked live: it still answers.
    engine.execute_on(
        HostId::LOCAL,
        CommandInvocation::new("new-window", [] as [&str; 0]),
    );
    watch.poll(&engine, "the local daemon to act on a command", |_| {
        let hosts = engine.hosts();
        (hosts[0].snapshot.sessions[0].windows.len() == 2).then_some(())
    });

    assert!(
        engine.set_fleet_hosts(&[]),
        "a host the file dropped is a change"
    );
    assert_eq!(engine.hosts().len(), 1);
    assert_eq!(
        engine.active_host(),
        HostId::LOCAL,
        "closing the host the workspace was on is the one time it falls back"
    );
    watch.poll(&engine, "the workspace to be local again", |_| {
        contains(&engine, here, "zz-gtk-ready")
    });

    relay.stop();
    local.shutdown();
    remote.shutdown();
}

fn host_entry(name: &str, socket: &Path) -> HostEntry {
    HostEntry {
        name: name.to_owned(),
        endpoint: Endpoint::Local(socket.to_owned()),
    }
}

struct Fixture {
    socket: PathBuf,
}

impl Fixture {
    /// The daemon is never session-less: it boots session "0" and an empty
    /// attach target resolves to that default. The fixture session therefore
    /// replaces it outright, so the client's own default attach is what the
    /// tests exercise. Sockets are named per test because these daemons run in
    /// parallel, and they live directly under `/tmp` because `sun_path` is
    /// short.
    fn boot(tag: &str, command: &str) -> Self {
        let fixture = Self::start(tag, command);
        let mut commands = connect_commands(&fixture.socket);
        commands
            .execute(CommandInvocation::new("kill-session", ["-t", "0"]))
            .expect("retire the boot session");
        fixture
    }

    /// Leave the boot session in place, so the fixture session exists but is
    /// not what an empty attach target resolves to — the default is the lowest
    /// session id, which is always the boot session. That difference is what
    /// tells a client that re-attaches what it remembers from one that forgot
    /// its attachment and fell back to the default.
    fn boot_beside_the_default(tag: &str, command: &str) -> Self {
        Self::start(tag, command)
    }

    fn start(tag: &str, command: &str) -> Self {
        let socket = PathBuf::from(format!("/tmp/zzgtk-{}-{tag}.sock", std::process::id()));
        let fixture = Self { socket };
        let _ = std::fs::remove_file(&fixture.socket);
        let _ = std::fs::remove_file(identity_path(&fixture.socket));
        fixture.spawn();
        connect_commands(&fixture.socket)
            .execute(CommandInvocation::new(
                "new-session",
                ["-d", "-s", SESSION, command],
            ))
            .expect("create the fixture session");
        fixture
    }

    /// Stop the daemon and wait for it to let go of the socket path: the
    /// listener removes it on the way out, and a replacement that bound before
    /// that happened would have its own socket deleted underneath it.
    fn stop(&self) {
        if let Ok(mut commands) = CommandClient::connect(&self.socket) {
            let _ = commands.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        }
        let deadline = Instant::now() + DEADLINE;
        while self.socket.exists() {
            assert!(
                Instant::now() < deadline,
                "the daemon never released {}",
                self.socket.display()
            );
            thread::sleep(POLL);
        }
    }

    /// Bring a replacement up on the same path. Its session is built out of the
    /// boot session the daemon always creates rather than beside it: a client
    /// that is already retrying can reconnect at any point during the rebuild,
    /// and there must never be a session for it to land on that this is about
    /// to kill.
    fn respawn(&self, command: &str) {
        self.spawn();
        let mut commands = connect_commands(&self.socket);
        commands
            .execute(CommandInvocation::new(
                "rename-session",
                ["-t", "0", SESSION],
            ))
            .expect("rename the boot session");
        commands
            .execute(CommandInvocation::new("split-window", ["-t", "0", command]))
            .expect("add the fixture pane beside the boot pane");
        commands
            .execute(CommandInvocation::new("kill-pane", ["-t", "0"]))
            .expect("retire the boot pane");
    }

    fn spawn(&self) {
        let daemon = Daemon::new(&self.socket).without_user_config();
        thread::Builder::new()
            .name("zz-gtk-test-daemon".to_owned())
            .spawn(move || {
                let _ = daemon.run_foreground();
            })
            .expect("spawn the test daemon");
    }

    fn shutdown(self) {
        self.stop();
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(identity_path(&self.socket));
    }
}

/// A unix socket the test can cut. An in-process daemon's per-connection
/// threads outlive `kill-server` — a real daemon closes its sockets by exiting
/// the process, these only notice when the client hangs up — so a client wired
/// straight to one never sees the drop a reconnect needs. The reconnect test
/// dials this relay instead and breaks the transport by hand.
struct Relay {
    front: PathBuf,
    open: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    live: Arc<Mutex<Vec<UnixStream>>>,
}

impl Relay {
    fn start(tag: &str, back: &Path) -> Self {
        let front = PathBuf::from(format!("/tmp/zzgtk-{}-{tag}.sock", std::process::id()));
        let _ = std::fs::remove_file(&front);
        let listener = UnixListener::bind(&front).expect("bind the relay socket");
        listener
            .set_nonblocking(true)
            .expect("poll the relay listener");
        let relay = Self {
            front,
            open: Arc::new(AtomicBool::new(true)),
            running: Arc::new(AtomicBool::new(true)),
            live: Arc::new(Mutex::new(Vec::new())),
        };
        let open = Arc::clone(&relay.open);
        let running = Arc::clone(&relay.running);
        let live = Arc::clone(&relay.live);
        let back = back.to_owned();
        thread::Builder::new()
            .name("zz-gtk-relay".to_owned())
            .spawn(move || {
                while running.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((client, _)) => {
                            client.set_nonblocking(false).expect("block on the relay");
                            let daemon = open
                                .load(Ordering::Acquire)
                                .then(|| UnixStream::connect(&back).ok())
                                .flatten();
                            let Some(daemon) = daemon else {
                                let _ = client.shutdown(Shutdown::Both);
                                continue;
                            };
                            live.lock()
                                .expect("relay poisoned")
                                .extend([clone(&client), clone(&daemon)]);
                            pipe(clone(&client), clone(&daemon));
                            pipe(daemon, client);
                        }
                        Err(error) if error.kind() == ErrorKind::WouldBlock => thread::sleep(POLL),
                        Err(_) => return,
                    }
                }
            })
            .expect("spawn the relay");
        relay
    }

    /// Break every live connection and refuse new ones, the way a machine that
    /// went away does.
    fn cut(&self) {
        self.open.store(false, Ordering::Release);
        for stream in self.live.lock().expect("relay poisoned").drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    fn restore(&self) {
        self.open.store(true, Ordering::Release);
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Release);
        let _ = std::fs::remove_file(&self.front);
    }
}

fn clone(stream: &UnixStream) -> UnixStream {
    stream.try_clone().expect("clone a relayed socket")
}

fn pipe(mut from: UnixStream, mut to: UnixStream) {
    thread::Builder::new()
        .name("zz-gtk-relay-pipe".to_owned())
        .spawn(move || {
            let _ = std::io::copy(&mut from, &mut to);
            let _ = to.shutdown(Shutdown::Both);
            let _ = from.shutdown(Shutdown::Both);
        })
        .expect("spawn a relay pipe");
}

fn identity_path(socket: &Path) -> PathBuf {
    let mut path = OsString::from(socket);
    path.push(".identity");
    PathBuf::from(path)
}

fn connect(fixture: &Fixture) -> Arc<Engine> {
    Engine::connect(
        &Endpoint::Local(fixture.socket.clone()),
        "",
        TerminalColorScheme::Dark,
    )
    .expect("connect the engine")
}

fn connect_commands(socket: &Path) -> CommandClient {
    let deadline = Instant::now() + DEADLINE;
    loop {
        match CommandClient::connect(socket) {
            Ok(client) => return client,
            Err(error) => {
                assert!(Instant::now() < deadline, "daemon did not start: {error}");
                thread::sleep(POLL);
            }
        }
    }
}

/// What the engine has announced so far, plus what draining its inbox cost.
#[derive(Default)]
struct Watch {
    seen: HashSet<&'static str>,
    frames: usize,
    widest_batch: usize,
    /// The pane the daemon last asked to show a search prompt over.
    search_prompt: Option<PaneId>,
    /// Leave the inbox alone, so a test can prove what a pending frame's pane
    /// removal does to it.
    hold_frames: bool,
}

impl Watch {
    fn saw(&self, what: &'static str) -> bool {
        self.seen.contains(what)
    }

    /// Drop a notification so the next one of its kind is what a wait sees.
    fn forget(&mut self, what: &'static str) {
        self.seen.remove(what);
    }

    /// Drain the engine's fan-out while waiting for a content condition.
    /// Waiting for silence instead would never return: the status line carries
    /// a clock and republishes about once a second forever.
    fn poll<T>(
        &mut self,
        engine: &Engine,
        what: &str,
        mut ready: impl FnMut(&Self) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + DEADLINE;
        loop {
            self.drain(engine);
            if let Some(value) = ready(self) {
                return value;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(POLL);
        }
    }

    fn drain(&mut self, engine: &Engine) {
        let events = engine.events();
        while let Ok(event) = events.try_recv() {
            match event {
                EngineEvent::Attached(_) => {
                    self.seen.insert("attached");
                }
                EngineEvent::SnapshotChanged => {
                    self.seen.insert("snapshot");
                }
                EngineEvent::StatusChanged => {
                    self.seen.insert("status");
                }
                EngineEvent::Reconnecting { .. } => {
                    self.seen.insert("reconnecting");
                }
                EngineEvent::Reconnected => {
                    self.seen.insert("reconnected");
                }
                EngineEvent::BeginSearch { pane, .. } => self.search_prompt = Some(pane),
                EngineEvent::FramesReady if !self.hold_frames => {
                    let batch = engine.take_frames();
                    if !batch.is_empty() {
                        self.seen.insert("frames");
                    }
                    self.frames += batch.len();
                    self.widest_batch = self.widest_batch.max(batch.len());
                }
                EngineEvent::Disconnected(error) => panic!("the engine gave up: {error}"),
                EngineEvent::Detached => panic!("the engine was detached"),
                _ => {}
            }
        }
    }
}

/// A second client on the same daemon, reduced straight through [`ClientCore`]
/// the way `crates/zz-client/tests/simulator.rs` does it.
struct Reference {
    client: Arc<InteractiveClient>,
    core: ClientCore,
    inbox: mpsc::Receiver<ProtocolMessage>,
}

impl Reference {
    fn attach(socket: &Path, session: &str) -> Self {
        let deadline = Instant::now() + DEADLINE;
        let client = loop {
            match InteractiveClient::connect(socket) {
                Ok(client) => break Arc::new(client),
                Err(error) => {
                    assert!(Instant::now() < deadline, "daemon did not start: {error}");
                    thread::sleep(POLL);
                }
            }
        };
        let (sender, inbox) = mpsc::channel();
        let reader = Arc::clone(&client);
        thread::Builder::new()
            .name("zz-gtk-reference".to_owned())
            .spawn(move || {
                while let Ok(message) = reader.recv() {
                    if sender.send(message).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn the reference reader");
        let mut core = ClientCore::new();
        core.handle_message(ProtocolMessage::ServerHello(client.server_hello().clone()));
        client.attach(session.to_owned()).expect("attach");
        Self {
            client,
            core,
            inbox,
        }
    }

    fn pump(&mut self) {
        while let Ok(message) = self.inbox.try_recv() {
            self.core.handle_message(message);
            while let Some(Outbound::RequestFull(pane)) = self.core.poll_outbound() {
                self.client.request_full(pane).expect("request full");
            }
        }
        while self.core.poll_event().is_some() {}
    }

    fn resize(&self, pane: PaneId) {
        self.client
            .send_input(InputMessage::ResizeTerminal {
                pane,
                columns: COLUMNS,
                rows: ROWS,
                cell_width_px: CELL_WIDTH_PX,
                cell_height_px: CELL_HEIGHT_PX,
            })
            .expect("resize the reference client's pane");
    }

    fn viewport(&self, pane: PaneId) -> Option<&TerminalViewport> {
        self.core.viewport(pane)
    }
}

fn first_terminal_pane(engine: &Engine) -> Option<PaneId> {
    engine.session_view()?.terminal_panes().next()
}

fn terminal_panes(engine: &Engine) -> Vec<PaneId> {
    engine
        .session_view()
        .map(|view| view.terminal_panes().collect())
        .unwrap_or_default()
}

/// Compares resolved glyph text, never raw style ids: a patched viewport and a
/// fresh full frame intern their dictionaries differently.
fn contains(engine: &Engine, pane: PaneId, needle: &str) -> Option<()> {
    let viewport = engine.viewport(pane)?;
    resolved_text(&viewport).contains(needle).then_some(())
}

fn sized(engine: &Engine, pane: PaneId, columns: u16, rows: u16) -> Option<()> {
    let viewport = engine.viewport(pane)?;
    (viewport.columns == columns && viewport.rows == rows).then_some(())
}

/// Rendering-equivalent content: resolved glyphs and style values per cell,
/// plus geometry and cursor.
fn signature(
    viewport: &TerminalViewport,
) -> (u16, u16, Vec<(String, Option<PackedStyle>)>, String) {
    let cells = viewport
        .cells
        .iter()
        .map(|cell| (glyph_text(viewport, *cell), viewport.style(*cell)))
        .collect();
    (
        viewport.columns,
        viewport.rows,
        cells,
        format!("{:?}", viewport.cursor),
    )
}

fn resolved_text(viewport: &TerminalViewport) -> String {
    let columns = usize::from(viewport.columns).max(1);
    let mut text = String::new();
    for (index, cell) in viewport.cells.iter().enumerate() {
        if index > 0 && index % columns == 0 {
            text.push('\n');
        }
        match viewport.glyph(*cell) {
            Glyph::Empty => text.push(' '),
            Glyph::Scalar(character) => text.push(character),
            Glyph::Grapheme(grapheme) => text.push_str(grapheme),
        }
    }
    text
}

fn glyph_text(viewport: &TerminalViewport, cell: PackedCell) -> String {
    match viewport.glyph(cell) {
        Glyph::Empty => String::new(),
        Glyph::Scalar(character) => character.to_string(),
        Glyph::Grapheme(grapheme) => grapheme.to_owned(),
    }
}

fn typed(character: char) -> KeyInput {
    KeyInput {
        action: KeyAction::Press,
        key: KeyCode::Character(character),
        modifiers: Modifiers::default(),
        text: Some(character.to_string().into_boxed_str()),
        unshifted_codepoint: Some(character),
    }
}

/// The prefix chord. A control chord carries no typed text — that is what keeps
/// it off a plain-character binding.
fn control(character: char) -> KeyInput {
    KeyInput {
        action: KeyAction::Press,
        key: KeyCode::Character(character),
        modifiers: Modifiers::new(false, true, false, false),
        text: None,
        unshifted_codepoint: Some(character),
    }
}
