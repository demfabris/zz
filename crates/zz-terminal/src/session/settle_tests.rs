use super::*;

fn unwatched(
    columns: u16,
    rows: u16,
) -> (
    Terminal<'static, 'static>,
    Publisher,
    TerminalEvents,
    ActiveTerminalViews,
    Frames<'static>,
) {
    let state = Arc::new(EventQueueState::new());
    let (event_tx, events) = terminal_event_channel(&state);
    let publisher = Publisher {
        event_tx,
        latest: Arc::new(RwLock::new(PublishedViewports::new(
            TerminalViewport::blank(columns, rows, SessionStatus::Starting),
        ))),
        state,
    };
    let terminal = new_terminal(columns, rows, 32).expect("terminal");
    let frames = Frames::new(&TerminalAppearance::default()).expect("frames");
    (
        terminal,
        publisher,
        events,
        ActiveTerminalViews::new(),
        frames,
    )
}

fn feed(
    terminal: &mut Terminal<'static, 'static>,
    publisher: &Publisher,
    frames: &mut Frames<'static>,
    active: &mut ActiveTerminalViews,
    bytes: &[u8],
) {
    terminal.vt_write(bytes);
    frames.mode_only_write = writes_only_modes(bytes);
    publish_active_views(
        terminal,
        publisher,
        frames,
        SnapshotChange::Content,
        active,
        &WordSeparators::default(),
        SessionStatus::Running,
    )
    .expect("publish");
}

#[test]
fn only_cell_neutral_mode_sets_count_as_mode_writes() {
    for bytes in [
        &b"\x1b[20h\x1b[?25l"[..],
        b"\x1b[?1000;1006h",
        b"\x1b[4l",
        b"\x1b[?2004h\x1b[?1004l",
    ] {
        assert!(writes_only_modes(bytes), "{bytes:?}");
    }
    for bytes in [
        &b""[..],
        b"x",
        b"\x1b[?25l\r\n",
        b"\x1b[?1049h",
        b"\x1b[?3h",
        b"\x1b[?20h",
        b"\x1b[25l",
        b"\x1b[2J",
        b"\x1b[?25",
        b"\x1b[?h",
        b"\x1b[?25lx",
        b"\x1b[?25;3l",
    ] {
        assert!(!writes_only_modes(bytes), "{bytes:?}");
    }
}

#[test]
fn a_blank_pane_fed_only_modes_builds_no_settle_snapshot() {
    let (mut terminal, publisher, _events, mut active, mut frames) = unwatched(80, 24);
    feed(
        &mut terminal,
        &publisher,
        &mut frames,
        &mut active,
        b"\x1b[20h\x1b[?25l",
    );
    assert!(frames.rebuild_due().is_none());
    assert!(matches!(
        publisher.latest_fallback().status,
        SessionStatus::Running
    ));
    std::thread::sleep(UNWATCHED_SETTLE_QUIET);
    settle_unwatched(
        &mut terminal,
        &publisher,
        &mut frames,
        &mut active,
        &WordSeparators::default(),
        SessionStatus::Running,
    )
    .expect("settle");
    assert_eq!(frames.snapshot_builds, 0);

    feed(
        &mut terminal,
        &publisher,
        &mut frames,
        &mut active,
        b"hello",
    );
    assert!(frames.rebuild_due().is_some());
    feed(
        &mut terminal,
        &publisher,
        &mut frames,
        &mut active,
        b"\x1b[?2004h",
    );
    assert!(
        frames.rebuild_due().is_some(),
        "a later mode write keeps the settle owed"
    );
}

#[test]
fn a_mode_write_on_a_pane_whose_cursor_shows_still_settles() {
    let (mut terminal, publisher, _events, mut active, mut frames) = unwatched(80, 24);
    feed(
        &mut terminal,
        &publisher,
        &mut frames,
        &mut active,
        b"\x1b[?2004h",
    );
    assert!(
        frames.rebuild_due().is_some(),
        "the fallback has no cursor but the terminal shows one"
    );
}
