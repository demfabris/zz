use super::*;

fn fixture(
    count: u64,
) -> (
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
            TerminalViewport::blank(8, 3, SessionStatus::Running),
        ))),
        state,
    };
    let mut active = ActiveTerminalViews::new();
    let mut frames = Frames::new(&TerminalAppearance::default()).expect("frames");
    for id in 1..=count {
        let id = TerminalViewId(id);
        active.insert(id, Box::new(TerminalViewState::default()));
        frames.set_stream(id, ViewStream::Foreground);
    }
    (publisher, events, active, frames)
}

fn publish(
    terminal: &mut Terminal<'static, 'static>,
    publisher: &Publisher,
    frames: &mut Frames<'static>,
    active: &mut ActiveTerminalViews,
    change: SnapshotChange,
) -> usize {
    let before = frames.snapshot_builds;
    publish_active_views(
        terminal,
        publisher,
        frames,
        change,
        active,
        &WordSeparators::default(),
        SessionStatus::Running,
    )
    .expect("publish views");
    frames.snapshot_builds - before
}

#[test]
fn two_and_four_live_views_build_one_snapshot_per_publish() {
    for count in [2, 4] {
        let mut terminal = new_terminal(8, 3, 32).expect("terminal");
        let (publisher, events, mut active, mut frames) = fixture(count);
        frames.set_stream(TerminalViewId(2), ViewStream::Preview);
        let epochs = (1..=count)
            .map(|id| (TerminalViewId(id), frames.epoch(TerminalViewId(id))))
            .collect::<HashMap<_, _>>();
        let mut previous = None;
        for change in [
            SnapshotChange::Content,
            SnapshotChange::View,
            SnapshotChange::Overlay,
        ] {
            terminal.vt_write(b"x");
            assert_eq!(
                publish(&mut terminal, &publisher, &mut frames, &mut active, change),
                1
            );
            let latest = publisher.latest.read();
            let first = &latest.by_view[&TerminalViewId(1)];
            assert!(Arc::ptr_eq(first, &latest.fallback));
            assert_eq!(latest.epochs, epochs);
            assert_eq!(frames.published.len(), count as usize);
            for viewport in latest.by_view.values() {
                assert!(Arc::ptr_eq(first, viewport));
            }
            if let Some(previous) = previous {
                assert!(!Arc::ptr_eq(first, &previous));
            }
            previous = Some(Arc::clone(first));
            assert!(matches!(
                events.try_recv(),
                Ok(TerminalEvent::ViewportReady { .. })
            ));
            assert!(events.try_recv().is_err());
        }
        terminal.resize(12, 5, 8, 18).expect("resize");
        assert_eq!(
            publish(
                &mut terminal,
                &publisher,
                &mut frames,
                &mut active,
                SnapshotChange::Content
            ),
            1
        );
        let latest = publisher.latest.read();
        for viewport in latest.by_view.values() {
            assert_eq!((viewport.columns, viewport.rows), (12, 5));
            assert!(!Arc::ptr_eq(
                viewport,
                previous.as_ref().expect("previous frame")
            ));
        }
    }
}

#[test]
fn frozen_views_keep_separate_frames_and_captured_sizes() {
    let mut terminal = new_terminal(8, 3, 32).expect("terminal");
    terminal.vt_write(b"frozen");
    let (publisher, _events, mut active, mut frames) = fixture(4);
    for id in [2, 3] {
        let state = active
            .get_mut(&TerminalViewId(id))
            .expect("view")
            .active_mut();
        enter_copy_mode(
            &mut terminal,
            &mut state.selection,
            &mut state.copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
    }
    assert_eq!(
        publish(
            &mut terminal,
            &publisher,
            &mut frames,
            &mut active,
            SnapshotChange::Content
        ),
        3
    );
    {
        let latest = publisher.latest.read();
        assert!(Arc::ptr_eq(
            &latest.by_view[&TerminalViewId(1)],
            &latest.by_view[&TerminalViewId(4)]
        ));
        assert!(!Arc::ptr_eq(
            &latest.by_view[&TerminalViewId(2)],
            &latest.by_view[&TerminalViewId(3)]
        ));
        assert_eq!(latest.copy_facts.len(), 2);
    }
    terminal.resize(12, 5, 8, 18).expect("resize");
    terminal.vt_write(b" live");
    assert_eq!(
        publish(
            &mut terminal,
            &publisher,
            &mut frames,
            &mut active,
            SnapshotChange::Content
        ),
        3
    );
    let latest = publisher.latest.read();
    for id in [1, 4] {
        let viewport = &latest.by_view[&TerminalViewId(id)];
        assert_eq!((viewport.columns, viewport.rows), (12, 5));
        assert_eq!(viewport.mode, TerminalMode::Live);
    }
    for id in [2, 3] {
        let viewport = &latest.by_view[&TerminalViewId(id)];
        assert_eq!((viewport.columns, viewport.rows), (8, 3));
        assert!(matches!(viewport.mode, TerminalMode::Copy { .. }));
    }
}

#[test]
fn pinned_views_build_their_own_frames() {
    let mut terminal = new_terminal(8, 3, 32).expect("terminal");
    terminal.vt_write(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive");
    let (publisher, _events, mut active, mut frames) = fixture(4);
    terminal.scroll_viewport(ScrollViewport::Top);
    for id in [2, 3] {
        let view = active.get_mut(&TerminalViewId(id)).expect("view");
        sync_viewport_anchor(&terminal, view).expect("pin");
        assert!(matches!(view.viewport, ViewportAnchor::Pinned(_)));
    }
    assert_eq!(
        publish(
            &mut terminal,
            &publisher,
            &mut frames,
            &mut active,
            SnapshotChange::Content
        ),
        3
    );
    let latest = publisher.latest.read();
    assert!(Arc::ptr_eq(
        &latest.by_view[&TerminalViewId(1)],
        &latest.by_view[&TerminalViewId(4)]
    ));
    assert!(!Arc::ptr_eq(
        &latest.by_view[&TerminalViewId(2)],
        &latest.by_view[&TerminalViewId(3)]
    ));
    assert_eq!(latest.by_view[&TerminalViewId(2)].scrollbar.offset, 0);
    assert!(latest.by_view[&TerminalViewId(1)].scrollbar.offset > 0);
}

#[test]
fn live_views_share_only_matching_frame_options() {
    let mut terminal = new_terminal(8, 3, 32).expect("terminal");
    terminal.vt_write(b"link");
    let (publisher, _events, mut active, mut frames) = fixture(4);
    for id in [2, 3] {
        let view = active.get_mut(&TerminalViewId(id)).expect("view");
        view.hover_link = Some(HoverLink {
            row: 0,
            start: 0,
            end: 4,
            uri: "https://example.com".to_owned(),
        });
        view.search = Some(Box::new(SearchState {
            matches: vec![SearchMatch {
                row: 0,
                end_row: 0,
                start: 0,
                end: 4,
            }],
            current: Some(0),
            ..SearchState::default()
        }));
    }
    active
        .get_mut(&TerminalViewId(4))
        .expect("view")
        .unseen_output = 7;
    assert_eq!(
        publish(
            &mut terminal,
            &publisher,
            &mut frames,
            &mut active,
            SnapshotChange::Content
        ),
        3
    );
    let latest = publisher.latest.read();
    assert!(Arc::ptr_eq(
        &latest.by_view[&TerminalViewId(2)],
        &latest.by_view[&TerminalViewId(3)]
    ));
    assert!(!Arc::ptr_eq(
        &latest.by_view[&TerminalViewId(1)],
        &latest.by_view[&TerminalViewId(2)]
    ));
    assert!(!Arc::ptr_eq(
        &latest.by_view[&TerminalViewId(1)],
        &latest.by_view[&TerminalViewId(4)]
    ));
    assert!(latest.by_view[&TerminalViewId(1)].overlays.is_empty());
    assert_eq!(latest.by_view[&TerminalViewId(2)].overlays.len(), 2);
    assert_eq!(latest.by_view[&TerminalViewId(4)].unseen_output, 7);
}

#[test]
fn leaving_copy_mode_preserves_stream_admission_and_first_streamed_fallback() {
    let mut terminal = new_terminal(8, 3, 32).expect("terminal");
    let (publisher, _events, mut active, mut frames) = fixture(4);
    frames.set_stream(TerminalViewId(1), ViewStream::Off);
    frames.set_stream(TerminalViewId(3), ViewStream::Off);
    frames.mode_views.insert(TerminalViewId(1));
    assert_eq!(
        publish(
            &mut terminal,
            &publisher,
            &mut frames,
            &mut active,
            SnapshotChange::View
        ),
        1
    );
    let latest = publisher.latest.read();
    assert_eq!(latest.by_view.len(), 3);
    assert!(!latest.epochs.contains_key(&TerminalViewId(1)));
    assert!(!latest.by_view.contains_key(&TerminalViewId(3)));
    assert!(Arc::ptr_eq(
        &latest.fallback,
        &latest.by_view[&TerminalViewId(2)]
    ));
    assert!(frames.mode_views.is_empty());
}

#[test]
fn selected_live_views_keep_their_own_overlays() {
    let mut terminal = new_terminal(8, 3, 32).expect("terminal");
    terminal.vt_write(b"selected");
    let (publisher, _events, mut active, mut frames) = fixture(4);
    for (id, end) in [(2, 2), (3, 4)] {
        let view = active.get_mut(&TerminalViewId(id)).expect("view");
        view.selection = Some(SelectionState {
            anchor: terminal
                .track_grid_ref(Point::Active(PointCoordinate { x: 0, y: 0 }))
                .expect("anchor"),
            focus: terminal
                .track_grid_ref(Point::Active(PointCoordinate { x: end, y: 0 }))
                .expect("focus"),
            mode: SelectionMode::Cell,
            rectangle: false,
        });
    }
    assert_eq!(
        publish(
            &mut terminal,
            &publisher,
            &mut frames,
            &mut active,
            SnapshotChange::Content
        ),
        3
    );
    let latest = publisher.latest.read();
    assert!(Arc::ptr_eq(
        &latest.by_view[&TerminalViewId(1)],
        &latest.by_view[&TerminalViewId(4)]
    ));
    assert!(latest.by_view[&TerminalViewId(1)].overlays.is_empty());
    assert_ne!(
        latest.by_view[&TerminalViewId(2)].overlays,
        latest.by_view[&TerminalViewId(3)].overlays
    );
}
