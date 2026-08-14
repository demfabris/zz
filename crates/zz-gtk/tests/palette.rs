//! The command palette against a real in-process daemon.
//!
//! The palette's own keyboard cannot be driven without a display, so this
//! proves the two halves it sits between instead: that the keys the prefix
//! interceptor forwards are what open a prompt, and that the model's edits and
//! submissions reach the mux. Everything in between — ranking, cursors,
//! navigation — is pinned by the unit tests beside the code.

#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use zz_daemon::{CommandClient, Daemon, Endpoint};
use zz_gtk::{
    engine::{Engine, EngineEvent},
    ui::palette::{PaletteEnter, PaletteModel, PaletteSync},
};
use zz_protocol::{
    CommandInvocation, CommandPromptAction, CommandPromptKind, InputMessage, canonical_key,
    input_key_name,
};
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers, TerminalColorScheme};

const FIXTURE: &str = "printf 'palette-ready\\r\\n'; exec /bin/cat";
const SESSION: &str = "palette";
const DEADLINE: Duration = Duration::from_mins(1);
const POLL: Duration = Duration::from_millis(10);

/// The whole round trip the palette owns: the prefix chord and `:` open a
/// command prompt, the model completes into it, and a submission runs the
/// command the daemon parsed out of the text the client sent.
#[test]
fn the_prefix_chord_opens_a_prompt_the_model_completes_and_submits() {
    let daemon = Fixture::boot("prompt");
    let engine = connect(&daemon);
    let pane = wait(&engine, "a terminal pane", |engine| {
        engine
            .active_pane()
            .filter(|_| engine.session_view().is_some())
    });

    let chord = engine
        .prefix_chord()
        .expect("the daemon publishes a prefix");
    assert_eq!(chord, "C-b", "the fixture daemon runs the default prefix");
    engine.send_key(pane, chord_key(&chord), false);
    wait(&engine, "the prefix to arm", |engine| {
        engine.prefix_armed().then_some(())
    });

    engine.send_key(pane, typed(':'), false);
    let state = wait(&engine, "a command prompt", Engine::command_prompt);
    assert_eq!(state.kind, CommandPromptKind::Command);
    assert_eq!(state.prompt, ":");
    assert_eq!(state.input, "");

    let mut model = PaletteModel::new();
    assert_eq!(
        model.sync(Some(&state), engine.snapshot()),
        PaletteSync::Opened
    );

    assert!(model.edit("rename-w".to_owned(), 8));
    let (completed, cursor) = model.accept().expect("a completion for rename-w");
    assert_eq!(completed, "rename-window ", "the top match is the command");
    engine.send(InputMessage::CommandPrompt {
        action: CommandPromptAction::Update {
            input: completed,
            cursor: u32::try_from(cursor).expect("a small cursor"),
        },
    });

    assert!(model.edit("rename-window notes".to_owned(), 19));
    let PaletteEnter::Submit(input) = model.enter() else {
        panic!("nothing is highlighted, so Enter runs");
    };
    engine.send(InputMessage::CommandPrompt {
        action: CommandPromptAction::Submit { input },
    });

    wait(&engine, "the renamed window", |engine| {
        engine
            .session_view()
            .filter(|view| view.windows.iter().any(|window| window.name == "notes"))
    });
    wait(&engine, "the prompt to close", |engine| {
        engine.command_prompt().is_none().then_some(())
    });

    daemon.shutdown();
}

/// Escape closes the prompt without running anything, and the daemon's own
/// state is what tells the surface to go away.
#[test]
fn closing_a_prompt_leaves_the_mux_alone() {
    let daemon = Fixture::boot("close");
    let engine = connect(&daemon);
    let pane = wait(&engine, "a terminal pane", |engine| {
        engine
            .active_pane()
            .filter(|_| engine.session_view().is_some())
    });

    engine.send_key(pane, chord_key("C-b"), false);
    engine.send_key(pane, typed(':'), false);
    let state = wait(&engine, "a command prompt", Engine::command_prompt);

    let mut model = PaletteModel::new();
    model.sync(Some(&state), engine.snapshot());
    assert!(model.is_open());
    assert!(
        !model.suggestions().is_empty(),
        "an empty command prompt offers the catalog"
    );

    engine.send(InputMessage::CommandPrompt {
        action: CommandPromptAction::Close,
    });
    wait(&engine, "the prompt to close", |engine| {
        engine.command_prompt().is_none().then_some(())
    });
    assert_eq!(model.sync(None, engine.snapshot()), PaletteSync::Closed);
    assert!(!model.is_open());
    assert_eq!(
        engine.session_view().map(|view| view.windows.len()),
        Some(1),
        "closing a prompt runs nothing"
    );

    daemon.shutdown();
}

/// A value prompt — the shape every rename flow uses — arrives pre-filled and
/// carries no suggestions, so the palette is a plain field for it.
#[test]
fn a_rename_prompt_arrives_pre_filled_and_unsuggested() {
    let daemon = Fixture::boot("rename");
    let engine = connect(&daemon);
    let pane = wait(&engine, "a terminal pane", |engine| {
        engine
            .active_pane()
            .filter(|_| engine.session_view().is_some())
    });

    engine.send_key(pane, chord_key("C-b"), false);
    engine.send_key(pane, typed(','), false);
    let state = wait(&engine, "a rename prompt", Engine::command_prompt);

    assert_eq!(state.kind, CommandPromptKind::Value);
    assert!(
        state.history.is_empty(),
        "history belongs to command prompts alone"
    );

    let mut model = PaletteModel::new();
    model.sync(Some(&state), engine.snapshot());
    assert!(model.suggestions().is_empty());
    assert_eq!(model.input(), state.input);

    engine.send(InputMessage::CommandPrompt {
        action: CommandPromptAction::Submit {
            input: "renamed".to_owned(),
        },
    });
    wait(&engine, "the renamed window", |engine| {
        engine
            .session_view()
            .filter(|view| view.windows.iter().any(|window| window.name == "renamed"))
    });

    daemon.shutdown();
}

/// The chord the interceptor claims is the one the daemon publishes, spelled
/// the way the shared fold spells a press. A rebind has to move both together.
#[test]
fn the_claimed_chord_is_the_one_the_daemon_arms_on() {
    let daemon = Fixture::boot("rebind");
    let engine = connect(&daemon);
    let pane = wait(&engine, "a terminal pane", |engine| {
        engine
            .active_pane()
            .filter(|_| engine.session_view().is_some())
    });

    let mut commands = connect_commands(&daemon.socket);
    commands
        .execute(CommandInvocation::new(
            "set-option",
            ["-g", "prefix", "C-a"],
        ))
        .expect("rebind the prefix");
    let chord = wait(&engine, "the published rebind", |engine| {
        engine.prefix_chord().filter(|chord| chord == "C-a")
    });

    assert_eq!(
        input_key_name(&chord_key(&chord)).as_str(),
        chord,
        "the claim matches by the same fold the daemon looks the binding up with"
    );
    engine.send_key(pane, chord_key(&chord), false);
    wait(&engine, "the rebound prefix to arm", |engine| {
        engine.prefix_armed().then_some(())
    });

    daemon.shutdown();
}

/// The press the interceptor would build for a canonical `C-<letter>` chord.
fn chord_key(chord: &str) -> KeyInput {
    let canonical = canonical_key(chord);
    let base = canonical
        .strip_prefix("C-")
        .expect("the fixtures only use control chords");
    let character = base.chars().next().expect("a single-character chord");
    KeyInput {
        action: KeyAction::Press,
        key: KeyCode::Character(character),
        modifiers: Modifiers::new(false, true, false, false),
        text: None,
        unshifted_codepoint: Some(character),
    }
}

fn typed(character: char) -> KeyInput {
    KeyInput {
        action: KeyAction::Press,
        key: KeyCode::Character(character),
        modifiers: Modifiers::new(false, false, false, false),
        text: Some(character.to_string().into_boxed_str()),
        unshifted_codepoint: Some(character),
    }
}

/// Drain the engine's fan-out while waiting for a content condition. Waiting
/// for silence would never return: the status line carries a clock.
fn wait<T>(engine: &Engine, what: &str, mut ready: impl FnMut(&Engine) -> Option<T>) -> T {
    let deadline = Instant::now() + DEADLINE;
    let events = engine.events();
    loop {
        while let Ok(event) = events.try_recv() {
            match event {
                EngineEvent::FramesReady => drop(engine.take_frames()),
                EngineEvent::Disconnected(error) => panic!("the engine gave up: {error}"),
                EngineEvent::Detached => panic!("the engine was detached"),
                _ => {}
            }
        }
        if let Some(value) = ready(engine) {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(POLL);
    }
}

struct Fixture {
    socket: PathBuf,
}

impl Fixture {
    /// The daemon is never session-less, so the boot session is retired and the
    /// fixture session is what an empty attach resolves to. Sockets live
    /// directly under `/tmp` because `sun_path` is short.
    fn boot(tag: &str) -> Self {
        let socket = PathBuf::from(format!("/tmp/zzgtkp-{}-{tag}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let _ = std::fs::remove_file(identity_path(&socket));
        let fixture = Self { socket };
        let daemon = Daemon::new(&fixture.socket).without_user_config();
        thread::Builder::new()
            .name("zz-gtk-palette-daemon".to_owned())
            .spawn(move || {
                let _ = daemon.run_foreground();
            })
            .expect("spawn the test daemon");
        let mut commands = connect_commands(&fixture.socket);
        commands
            .execute(CommandInvocation::new(
                "new-session",
                ["-d", "-s", SESSION, FIXTURE],
            ))
            .expect("create the fixture session");
        commands
            .execute(CommandInvocation::new("kill-session", ["-t", "0"]))
            .expect("retire the boot session");
        fixture
    }

    fn shutdown(self) {
        if let Ok(mut commands) = CommandClient::connect(&self.socket) {
            let _ = commands.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        }
        let deadline = Instant::now() + DEADLINE;
        while self.socket.exists() {
            assert!(
                Instant::now() < deadline,
                "the daemon never let the socket go"
            );
            thread::sleep(POLL);
        }
        let _ = std::fs::remove_file(identity_path(&self.socket));
    }
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

fn identity_path(socket: &Path) -> PathBuf {
    let mut path = std::ffi::OsString::from(socket);
    path.push(".identity");
    PathBuf::from(path)
}
