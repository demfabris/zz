use super::*;

fn key(character: char, shift: bool, action: KeyAction) -> KeyInput {
    KeyInput {
        action,
        key: KeyCode::Character(character.to_ascii_lowercase()),
        modifiers: crate::Modifiers::new(shift, false, false, false),
        text: Some(character.to_string().into_boxed_str()),
        unshifted_codepoint: Some(character.to_ascii_lowercase()),
    }
}

fn enter() -> KeyInput {
    KeyInput {
        action: KeyAction::Press,
        key: KeyCode::Enter,
        modifiers: crate::Modifiers::default(),
        text: None,
        unshifted_codepoint: None,
    }
}

fn encoded(terminal: &Terminal<'_, '_>, input: KeyInput) -> Vec<u8> {
    let mut encoder = key::Encoder::new().expect("key encoder");
    let mut event = key::Event::new().expect("key event");
    let mut bytes = Vec::new();
    encode_key(
        terminal,
        &mut encoder,
        &mut event,
        input,
        Some(0x7f),
        &mut std::io::sink(),
        &mut bytes,
    )
    .expect("encode key");
    bytes
}

fn direct_keys() -> Vec<KeyInput> {
    let mut keys = Vec::new();
    for character in "qwzxjvkbmyup0123456789 `-=[];',./\\§é".chars() {
        keys.push(key(character, false, KeyAction::Press));
        keys.push(key(character, false, KeyAction::Repeat));
    }
    keys.push(enter());
    keys
}

const TRACKED_MODES: [&[u8]; 7] = [
    b"\x1b[?1h",
    b"\x1b=",
    b"\x1b[?1036h",
    b"\x1b[>4;2m",
    b"\x1b[?1035h",
    b"\x1b[?2004h",
    b"\x1b[20h",
];

#[test]
fn direct_keys_encode_as_the_encoder_does_in_every_tracked_mode() {
    for set in 0..1_u32 << TRACKED_MODES.len() {
        let mut terminal = new_terminal(20, 4, 1 << 16).expect("terminal");
        for (index, mode) in TRACKED_MODES.iter().enumerate() {
            if set & (1 << index) != 0 {
                terminal.vt_write(mode);
            }
        }
        for input in direct_keys() {
            let direct = direct_key_bytes(&input)
                .unwrap_or_else(|| panic!("{input:?} takes the direct path"))
                .to_vec();
            assert_eq!(
                direct,
                encoded(&terminal, input.clone()),
                "{input:?} with modes {set:#b}"
            );
        }
    }
}

#[test]
fn kitty_flags_change_what_a_direct_key_would_send() {
    let mut terminal = new_terminal(20, 4, 1 << 16).expect("terminal");
    terminal.vt_write(b"\x1b[>8u");
    assert!(!terminal.kitty_keyboard_flags().expect("flags").is_empty());
    let input = key('a', false, KeyAction::Press);
    assert_ne!(
        direct_key_bytes(&input).expect("direct").to_vec(),
        encoded(&terminal, input)
    );
}

#[test]
fn keys_the_encoder_shapes_stay_on_the_actor() {
    let control = KeyInput {
        modifiers: crate::Modifiers::new(false, true, false, false),
        ..key('c', false, KeyAction::Press)
    };
    let alt = KeyInput {
        modifiers: crate::Modifiers::new(false, false, true, false),
        ..key('a', false, KeyAction::Press)
    };
    let platform = KeyInput {
        modifiers: crate::Modifiers::new(false, false, false, true),
        ..key('a', false, KeyAction::Press)
    };
    let release = key('a', false, KeyAction::Release);
    let bare = KeyInput {
        text: None,
        ..key('a', false, KeyAction::Press)
    };
    let control_text = KeyInput {
        text: Some("\u{1b}".into()),
        ..key('a', false, KeyAction::Press)
    };
    let shifted_enter = KeyInput {
        modifiers: crate::Modifiers::new(true, false, false, false),
        ..enter()
    };
    let mut others = vec![
        control,
        alt,
        platform,
        release,
        bare,
        control_text,
        shifted_enter,
    ];
    for code in [
        KeyCode::Backspace,
        KeyCode::Tab,
        KeyCode::Escape,
        KeyCode::ArrowUp,
        KeyCode::Home,
        KeyCode::Function(1),
    ] {
        others.push(KeyInput {
            key: code,
            text: None,
            ..key('a', false, KeyAction::Press)
        });
    }
    for character in "QWZXJ!@#$%^&*()_+{}:\"<>?~|".chars() {
        others.push(key(character, true, KeyAction::Press));
    }
    for input in others {
        assert!(direct_key_bytes(&input).is_none(), "{input:?}");
    }
}

fn pipe() -> (std::os::fd::OwnedFd, filedescriptor::FileDescriptor) {
    let (read, write) = configured_actor_wake_pipe().expect("pipe");
    let write = filedescriptor::FileDescriptor::dup(&write).expect("dup the write end");
    rustix::io::ioctl_fionbio(&write, true).expect("nonblocking");
    (read, write)
}

fn read_all(read: &std::os::fd::OwnedFd) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match rustix::io::read(read, &mut buffer) {
            Ok(0) | Err(rustix::io::Errno::AGAIN) => return out,
            Ok(count) => out.extend_from_slice(&buffer[..count]),
            Err(error) => panic!("read: {error}"),
        }
    }
}

fn fill(writer: &mut PtyWriter) -> usize {
    let chunk = [b'f'; 1024];
    let mut written = 0;
    while !writer.has_pending() {
        writer.write_all(&chunk).expect("fill");
        written += chunk.len();
    }
    writer.flush_pending().expect("flush");
    written
}

#[test]
fn a_closed_path_sends_the_key_to_the_actor() {
    let (read, write) = pipe();
    let writer = PtyWriter::new(write);
    assert!(matches!(writer.direct.write(b"a"), DirectWrite::Closed));
    writer.direct.reopen(|| true);
    assert!(matches!(writer.direct.write(b"a"), DirectWrite::Written));
    assert_eq!(read_all(&read), b"a");
    assert!(writer.direct.take_echo());
    assert!(!writer.direct.take_echo());
}

#[test]
fn queued_actor_bytes_keep_the_key_behind_them() {
    let (read, write) = pipe();
    let mut writer = PtyWriter::new(write);
    let filled = fill(&mut writer);
    assert!(writer.has_pending());
    writer.direct.state.lock().open = true;
    assert!(matches!(writer.direct.write(b"k"), DirectWrite::Closed));
    writer.direct.reopen(|| true);
    assert!(!writer.direct.is_open());
    let mut seen = read_all(&read);
    while writer.has_pending() {
        writer.flush_pending().expect("flush");
        seen.extend(read_all(&read));
    }
    assert_eq!(seen.len(), filled);
    assert!(seen.iter().all(|byte| *byte == b'f'));
}

#[test]
fn a_full_pty_leaves_the_key_for_the_actor_once_and_in_order() {
    let (read, write) = pipe();
    let mut writer = PtyWriter::new(write);
    let filled = fill(&mut writer);
    let mut seen = read_all(&read);
    while writer.has_pending() {
        writer.flush_pending().expect("flush");
        seen.extend(read_all(&read));
    }
    let mut stuffed = 0;
    while let Ok(count) = rustix::io::write(writer.direct.state.lock().fd.as_ref().unwrap(), b"f") {
        stuffed += count;
    }
    writer.direct.reopen(|| true);
    assert!(matches!(
        writer.direct.write("§".as_bytes()),
        DirectWrite::Queued
    ));
    assert!(!writer.direct.is_open());
    assert!(matches!(writer.direct.write(b"x"), DirectWrite::Closed));
    seen.extend(read_all(&read));
    while writer.has_pending() {
        writer.flush_pending().expect("flush");
        seen.extend(read_all(&read));
    }
    let mut expected = vec![b'f'; filled + stuffed];
    expected.extend_from_slice("§".as_bytes());
    assert_eq!(seen, expected);
}

#[test]
fn a_failed_write_keeps_the_key_for_the_actor() {
    let (read, write) = pipe();
    let mut writer = PtyWriter::new(write);
    drop(read);
    writer.direct.reopen(|| true);
    assert!(matches!(writer.direct.write(b"a"), DirectWrite::Queued));
    assert!(!writer.direct.is_open());
    assert_eq!(writer.queued_bytes(), 1);
    assert!(writer.flush_pending().is_err());
}

#[test]
fn a_dropped_writer_closes_the_path_for_good() {
    let (_read, write) = pipe();
    let writer = PtyWriter::new(write);
    let direct = Arc::clone(&writer.direct);
    direct.reopen(|| true);
    drop(writer);
    assert!(!direct.is_open());
    direct.reopen(|| true);
    assert!(matches!(direct.write(b"a"), DirectWrite::Closed));
}

fn sender_with_direct() -> (
    CommandSender,
    Receiver<Command>,
    InputReceiver,
    PtyWriter,
    std::os::fd::OwnedFd,
) {
    let (control, control_rx) = command_channel();
    let (input, input_rx) = input_channel();
    let (read, write) = pipe();
    let writer = PtyWriter::attach(Arc::clone(&input_rx.direct), write);
    let commands = CommandSender {
        queues: Arc::new(CommandQueues {
            control,
            input: Some(input),
            liveness: crossbeam_channel::never(),
            slot: Arc::new(Mutex::new(ControlSlot::default())),
            wake: ActorWake::none(),
        }),
    };
    (commands, control_rx, input_rx, writer, read)
}

#[test]
fn queued_text_closes_the_path_so_a_later_key_follows_it() {
    let (commands, _control, input_rx, writer, read) = sender_with_direct();
    writer.direct.reopen(|| true);
    commands
        .send(Command::Text {
            view: None,
            text: Arc::from("paste"),
        })
        .expect("queue text");
    assert!(!commands.write_direct_key(&key('a', false, KeyAction::Press)));
    assert!(read_all(&read).is_empty());
    assert!(input_rx.commands.try_recv().is_ok());
}

#[test]
fn queued_control_commands_and_slot_changes_close_the_path() {
    let (commands, control_rx, _input, writer, _read) = sender_with_direct();
    writer.direct.reopen(|| true);
    commands.send(Command::ResetScreen).expect("queue reset");
    assert!(!writer.direct.is_open());
    assert!(control_rx.try_recv().is_ok());

    writer.direct.reopen(|| true);
    commands.with_slot(|slot| {
        slot.pending.resize = Some(Geometry::default());
        true
    });
    assert!(!writer.direct.is_open());

    writer.direct.reopen(|| true);
    commands.defer(Command::ResetScreen);
    assert!(!writer.direct.is_open());
}

#[test]
fn a_plain_key_on_an_open_path_reaches_the_pty_without_the_queue() {
    let (commands, control_rx, input_rx, writer, read) = sender_with_direct();
    writer.direct.reopen(|| true);
    assert!(commands.write_direct_key(&key('a', false, KeyAction::Press)));
    assert!(!commands.write_direct_key(&KeyInput {
        modifiers: crate::Modifiers::new(false, true, false, false),
        ..key('c', false, KeyAction::Press)
    }));
    assert_eq!(read_all(&read), b"a");
    assert!(control_rx.try_recv().is_err());
    assert!(input_rx.commands.try_recv().is_err());
}

#[test]
fn a_queued_direct_key_wakes_the_actor_to_flush_it() {
    let (commands, control_rx, _input, writer, read) = sender_with_direct();
    drop(read);
    writer.direct.reopen(|| true);
    assert!(commands.write_direct_key(&key('a', false, KeyAction::Press)));
    assert!(matches!(control_rx.try_recv(), Ok(Command::Wake)));
}

#[test]
fn only_live_views_at_the_bottom_take_direct_keys() {
    let primary = |primary| TerminalViewState {
        screen: Screen::Primary,
        primary,
        alternate: TerminalScreenViewState::default(),
    };
    assert!(view_takes_direct_input(&TerminalViewState::default()));
    assert!(!view_takes_direct_input(&primary(
        TerminalScreenViewState {
            unseen_output: 3,
            ..TerminalScreenViewState::default()
        }
    )));
    assert!(!view_takes_direct_input(&primary(
        TerminalScreenViewState {
            mouse_button_pressed: true,
            ..TerminalScreenViewState::default()
        }
    )));
    assert!(!view_takes_direct_input(&primary(
        TerminalScreenViewState {
            search_origin: Some(PointCoordinate { x: 0, y: 0 }),
            ..TerminalScreenViewState::default()
        }
    )));
}

fn byte_logger(prefix: &str) -> TerminalSession {
    TerminalSession::spawn(
        1000,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            shell: Some("/bin/sh".to_owned()),
            command: Some(vec![format!(
                "stty raw -echo; {prefix}printf READY; while :; do b=$(dd bs=1 count=1 2>/dev/null | od -An -tx1); printf '<%s>' $b; done"
            )]),
            initial_size: Some(TerminalSize::cells(120, 8)),
            ..TerminalSpawn::default()
        },
    )
}

fn wait_for(session: &TerminalSession, what: &str, accept: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let capture = session.capture(CaptureOptions::default()).expect("capture");
        if accept(&capture) {
            return capture;
        }
        assert!(Instant::now() < deadline, "{what}: {capture:?}");
        thread::sleep(Duration::from_millis(5));
    }
}

fn direct_open(session: &TerminalSession) -> bool {
    session
        .commands
        .queues
        .input
        .as_ref()
        .is_some_and(|input| input.direct.is_open())
}

#[test]
fn an_idle_pane_opens_the_path_and_a_key_after_queued_text_follows_it() {
    let session = byte_logger("");
    wait_for(&session, "the logger to start", |text| {
        text.contains("READY")
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !direct_open(&session) {
        assert!(
            Instant::now() < deadline,
            "the idle pane never opened the path"
        );
        thread::sleep(Duration::from_millis(2));
    }
    session.send_text("x");
    session.send_key_for_view(TerminalViewId(1), key('a', false, KeyAction::Press));
    wait_for(&session, "text then key", |text| text.contains("<78><61>"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !direct_open(&session) {
        assert!(Instant::now() < deadline, "the path never reopened");
        thread::sleep(Duration::from_millis(2));
    }
    session.send_key_for_view(TerminalViewId(1), key('b', false, KeyAction::Press));
    session.send_text("y");
    wait_for(&session, "key then text", |text| {
        text.contains("<78><61><62><79>")
    });
}

#[test]
fn kitty_flags_keep_keys_on_the_encoder() {
    let session = byte_logger("printf '\\033[>8u'; ");
    wait_for(&session, "the logger to start", |text| {
        text.contains("READY")
    });
    thread::sleep(Duration::from_millis(100));
    assert!(!direct_open(&session));
    session.send_key_for_view(TerminalViewId(1), key('a', false, KeyAction::Press));
    let text = wait_for(&session, "the encoded key", |text| text.contains("<75>"));
    assert!(text.contains("<1b><5b>"), "{text:?}");
    assert!(!text.contains("<61>"), "{text:?}");
}

fn wait_closed_for_a_while(session: &TerminalSession) {
    let deadline = Instant::now() + Duration::from_millis(150);
    while Instant::now() < deadline {
        assert!(!direct_open(session));
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_open(session: &TerminalSession) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !direct_open(session) {
        assert!(Instant::now() < deadline, "the path never opened");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_selection_or_copy_mode_keeps_keys_on_the_actor() {
    let session = byte_logger("");
    let view = TerminalViewId(7);
    session.attach_view(view);
    wait_for(&session, "the logger to start", |text| {
        text.contains("READY")
    });
    wait_open(&session);
    session.view_action(view, TerminalViewAction::SelectAll);
    wait_closed_for_a_while(&session);
    session.send_key_for_view(view, key('a', false, KeyAction::Press));
    wait_for(&session, "the key", |text| text.contains("<61>"));
    wait_open(&session);
    session.view_action(view, TerminalViewAction::EnterCopyMode);
    wait_closed_for_a_while(&session);
    session.view_action(view, TerminalViewAction::CopyMode(CopyModeAction::Cancel));
    wait_open(&session);
}

struct BareActor {
    actor: PaneActor,
    commands: CommandSender,
    queued: Receiver<QueuedInput>,
    _events: TerminalEvents,
}

fn bare_actor(script: &str) -> BareActor {
    bare_actor_with(script, input_channel())
}

fn bare_actor_with(script: &str, (input, input_rx): (InputSender, InputReceiver)) -> BareActor {
    let (control, control_rx) = command_channel();
    let queued = input_rx.commands.clone();
    let (wake, wake_rx) = actor_wake();
    let slot = Arc::new(Mutex::new(ControlSlot::default()));
    let event_state = Arc::new(EventQueueState::new());
    let (event_tx, events) = terminal_event_channel(&event_state);
    let appearance = TerminalAppearance::default();
    let latest = Arc::new(RwLock::new(PublishedViewports::new(
        TerminalViewport::blank_with_appearance(60, 8, SessionStatus::Starting, &appearance),
    )));
    let publisher = Publisher {
        event_tx,
        latest,
        state: event_state,
    };
    let spawn = TerminalSpawn {
        shell: Some("/bin/sh".to_owned()),
        command: Some(vec![script.to_owned()]),
        initial_size: Some(TerminalSize::cells(60, 8)),
        ..TerminalSpawn::default()
    };
    let actor = PaneActor::spawn(
        control_rx,
        input_rx,
        Arc::clone(&slot),
        publisher,
        1000,
        &appearance,
        &spawn,
        &wake,
        wake_rx,
        false,
        #[cfg(target_os = "linux")]
        None,
    )
    .expect("pane actor");
    BareActor {
        actor,
        commands: CommandSender {
            queues: Arc::new(CommandQueues {
                control,
                input: Some(input),
                liveness: crossbeam_channel::never(),
                slot,
                wake,
            }),
        },
        queued,
        _events: events,
    }
}

#[test]
fn an_actor_turn_closes_the_path_and_an_undrained_reply_keeps_it_closed() {
    let mut bare = bare_actor("stty raw -echo; printf '\\033[6nREADY'; read _");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "the reply query never arrived");
        bare.actor.begin_turn();
        assert!(!bare.actor.writer.direct.is_open());
        let wake = bare.actor.wait_for_wake().expect("wake");
        let readable = matches!(wake, Wake::PtyReadable);
        assert!(bare.actor.on_wake(wake).expect("turn"));
        if readable {
            break;
        }
        bare.actor.on_deadline().expect("deadline");
        bare.actor.end_turn();
    }
    bare.actor.end_turn();
    assert!(!bare.actor.writer.direct.is_open());
    bare.actor.begin_turn();
    bare.actor.on_deadline().expect("drain the reply");
    bare.actor.end_turn();
    assert!(bare.actor.writer.direct.is_open());
    bare.actor.begin_turn();
    assert!(!bare.actor.writer.direct.is_open());
}

#[test]
fn a_key_after_a_bracketed_paste_follows_the_closing_wrapper() {
    let session = byte_logger("printf '\\033[?2004h'; ");
    let view = TerminalViewId(9);
    session.attach_view(view);
    wait_for(&session, "the logger to start", |text| {
        text.contains("READY")
    });
    wait_open(&session);
    session.view_action(view, TerminalViewAction::Paste("p".to_owned()));
    session.send_key_for_view(view, key('a', false, KeyAction::Press));
    let text = wait_for(&session, "the paste and the key", |text| {
        text.contains("<61>")
    });
    let flat = text.replace('\n', "");
    assert!(
        flat.contains("<1b><5b><32><30><30><7e><70><1b><5b><32><30><31><7e><61>"),
        "{flat:?}"
    );
}

fn pump_until_open(bare: &mut BareActor) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        bare.actor.end_turn();
        if bare.actor.writer.direct.is_open() {
            return;
        }
        assert!(Instant::now() < deadline, "the actor never opened the path");
        bare.actor.begin_turn();
        let wake = bare.actor.wait_for_wake().expect("wake");
        assert!(bare.actor.on_wake(wake).expect("turn"));
        bare.actor.on_deadline().expect("deadline");
    }
}

#[test]
fn a_key_typed_while_the_overflow_holds_input_waits_behind_it() {
    let mut bare = bare_actor_with(
        "stty raw -echo; exec cat >/dev/null",
        input_channel_with_limits(1, 1 << 16),
    );
    pump_until_open(&mut bare);
    let text = |text: &str| Command::Text {
        view: None,
        text: Arc::from(text),
    };
    bare.commands.send(text("x")).expect("queue x");
    let in_flight = bare.queued.try_recv().expect("x takes the only slot");
    bare.commands
        .send(text("y"))
        .expect("y waits in the overflow");
    assert!(bare.queued.is_empty());
    bare.actor.end_turn();
    assert!(!bare.actor.writer.direct.is_open());
    let typed = key('a', false, KeyAction::Press);
    assert!(!bare.commands.write_direct_key(&typed));
    bare.commands
        .send(Command::Key {
            view: None,
            input: Box::new(typed),
        })
        .expect("the key queues behind y");
    drop(in_flight);
    let mut order = Vec::new();
    while let Ok(queued) = bare.queued.try_recv() {
        order.push(match queued.command {
            Command::Text { text, .. } => text.to_string(),
            Command::Key { input, .. } => input.text.unwrap_or_default().into_string(),
            other => panic!("unexpected input {}", other.name()),
        });
    }
    assert_eq!(order, ["y", "a"]);
}

#[test]
fn a_direct_key_inside_a_wake_hold_reaches_the_pane() {
    let path = std::env::temp_dir().join(format!("zz-direct-hold-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let shard = shard::ShardHandle::start(112).expect("shard");
    let session = TerminalSession::spawn_with_shard(
        1000,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            shell: Some("/bin/sh".to_owned()),
            command: Some(vec![format!(
                "stty raw -echo; printf READY; exec dd bs=1 of='{}' 2>/dev/null",
                path.display()
            )]),
            initial_size: Some(TerminalSize::cells(60, 8)),
            ..TerminalSpawn::default()
        },
        Ok(Some(shard.clone())),
    );
    wait_for(&session, "the pane to start", |text| text.contains("READY"));
    wait_open(&session);
    let received = |expected: &[u8]| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let seen = std::fs::read(&path).unwrap_or_default();
            if seen == expected {
                return;
            }
            assert!(Instant::now() < deadline, "the pane read {seen:?}");
            thread::sleep(Duration::from_millis(2));
        }
    };
    let view = TerminalViewId(1);
    let hold = hold_actor_wakes();
    session.send_key_for_view(view, key('a', false, KeyAction::Press));
    assert_eq!(session.commands.pending_input().0, 0);
    received(b"a");
    session.send_text("x");
    session.send_key_for_view(view, key('b', false, KeyAction::Press));
    assert_eq!(session.commands.pending_input().0, 2);
    drop(hold);
    received(b"axb");
    let _ = std::fs::remove_file(&path);
}
