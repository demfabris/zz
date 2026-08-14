//! The GTK client's protocol half against a real in-process daemon: attach,
//! render, type. Nothing here touches a widget, which is the point — the engine
//! is the layer a display-less machine can still prove.

#![cfg(unix)]

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use zz_daemon::{CommandClient, Daemon, Endpoint};
use zz_gtk::engine::{Engine, EngineEvent};
use zz_protocol::{CommandInvocation, PaneId};
use zz_terminal::{
    Glyph, KeyAction, KeyCode, KeyInput, Modifiers, TerminalColorScheme, TerminalViewport,
};

const FIXTURE: &str = "printf 'zz-gtk-ready\\r\\n'; exec /bin/cat";
const SESSION: &str = "gtk";
const DEADLINE: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(10);

#[test]
fn the_engine_attaches_renders_and_echoes_what_it_sends() {
    let daemon = Fixture::boot();
    let engine = Engine::connect(&Endpoint::Local(daemon.socket.clone()), "", scheme())
        .expect("connect the engine");

    let mut seen = HashSet::new();
    let pane = poll(
        &engine,
        &mut seen,
        "a terminal pane in the attached session",
        |_| first_terminal_pane(&engine),
    );
    let view = engine.session_view().expect("an attached session view");
    assert_eq!(
        view.name, SESSION,
        "the default attach must land on the only session the fixture left behind"
    );
    assert!(
        seen.contains("attached"),
        "the engine never published its attachment"
    );

    engine.resize_terminal(pane, 80, 24, 8, 16);
    poll(&engine, &mut seen, "the fixture banner", |_| {
        contains(&engine, pane, "zz-gtk-ready")
    });
    assert!(
        seen.contains("frames"),
        "viewport content arrived without a frame notification"
    );

    engine.send_key(pane, typed('!'), false);
    engine.send_text(pane, "gtk-echo".to_owned());
    poll(&engine, &mut seen, "the echoed key and text", |_| {
        contains(&engine, pane, "!gtk-echo")
    });

    daemon.shutdown();
}

struct Fixture {
    socket: PathBuf,
}

impl Fixture {
    /// The daemon is never session-less: it boots session "0" and an empty
    /// attach target resolves to the default. The fixture session therefore
    /// replaces it outright, so the client's own default attach is what the
    /// test exercises.
    fn boot() -> Self {
        let socket = PathBuf::from(format!("/tmp/zzgtk-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let daemon = Daemon::new(&socket).without_user_config();
        thread::Builder::new()
            .name("zz-gtk-test-daemon".to_owned())
            .spawn(move || {
                let _ = daemon.run_foreground();
            })
            .expect("spawn the test daemon");

        let mut commands = connect_commands(&socket);
        commands
            .execute(CommandInvocation::new(
                "new-session",
                ["-d", "-s", SESSION, FIXTURE],
            ))
            .expect("create the fixture session");
        commands
            .execute(CommandInvocation::new("kill-session", ["-t", "0"]))
            .expect("retire the boot session");
        Self { socket }
    }

    fn shutdown(self) {
        if let Ok(mut commands) = CommandClient::connect(&self.socket) {
            let _ = commands.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(self.socket.with_extension("sock.identity"));
    }
}

fn connect_commands(socket: &Path) -> CommandClient {
    let deadline = Instant::now() + Duration::from_secs(30);
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

/// Drains the engine's fan-out while waiting, recording which notifications
/// arrived. The status line republishes about once a second forever, so waiting
/// for silence would never return; every wait here is content-driven.
fn poll<T>(
    engine: &Engine,
    seen: &mut HashSet<&'static str>,
    what: &str,
    mut ready: impl FnMut(&mut HashSet<&'static str>) -> Option<T>,
) -> T {
    let deadline = Instant::now() + DEADLINE;
    loop {
        drain(engine, seen);
        if let Some(value) = ready(seen) {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(POLL);
    }
}

fn drain(engine: &Engine, seen: &mut HashSet<&'static str>) {
    let events = engine.events();
    while let Ok(event) = events.try_recv() {
        match event {
            EngineEvent::Attached(_) => {
                seen.insert("attached");
            }
            EngineEvent::SnapshotChanged => {
                seen.insert("snapshot");
            }
            EngineEvent::StatusChanged => {
                seen.insert("status");
            }
            EngineEvent::FramesReady => {
                if !engine.take_frames().is_empty() {
                    seen.insert("frames");
                }
            }
            EngineEvent::Disconnected(error) => panic!("the engine disconnected: {error}"),
            _ => {}
        }
    }
}

fn first_terminal_pane(engine: &Engine) -> Option<PaneId> {
    engine.session_view()?.terminal_panes().next()
}

/// Compares resolved glyph text, never raw style ids: a patched viewport and a
/// fresh full frame intern their dictionaries differently.
fn contains(engine: &Engine, pane: PaneId, needle: &str) -> Option<()> {
    let viewport = engine.viewport(pane)?;
    resolved_text(&viewport).contains(needle).then_some(())
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

fn typed(character: char) -> KeyInput {
    KeyInput {
        action: KeyAction::Press,
        key: KeyCode::Character(character),
        modifiers: Modifiers::default(),
        text: Some(character.to_string().into_boxed_str()),
        unshifted_codepoint: Some(character),
    }
}

const fn scheme() -> TerminalColorScheme {
    TerminalColorScheme::Dark
}
