use super::*;

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_empty_pane_starts_at_its_size_with_its_modes_and_builds_no_settle_snapshot() {
    let session = TerminalSession::spawn_empty_pane(
        64,
        Arc::new(TerminalAppearance::default()),
        Some(TerminalSize::cells(80, 11)),
    );
    wait_until("the empty pane to run", || {
        matches!(session.latest_viewport().status, SessionStatus::Running)
    });
    let viewport = session.latest_viewport();
    assert_eq!((viewport.columns, viewport.rows), (80, 11));
    assert!(session.facts().cursor_hidden);
    let capture = session
        .capture(CaptureOptions::default())
        .expect("capture the empty pane");
    assert_eq!(capture.split('\n').count(), 11, "{capture:?}");
    thread::sleep(UNWATCHED_SETTLE_QUIET * 3);
    assert!(
        !session.latest_viewport_is_current(),
        "nothing was written, so the unwatched fallback is never built"
    );

    assert!(session.feed(Arc::from(&b"a\nb"[..])));
    wait_until("the settle after real output", || {
        session.latest_viewport_is_current()
    });
    let capture = session
        .capture(CaptureOptions::default())
        .expect("capture after output");
    assert!(capture.starts_with("a\nb\n"), "{capture:?}");
}
