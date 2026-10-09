#![cfg(unix)]

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use crate::session::{
    CaptureBoundary, CaptureOptions, Command, CopyModeAction, CopyModeSearch, DEAD_NOTICE_WAIT,
    SearchDirection, TerminalSession, TerminalSpawn, TerminalViewAction, TerminalViewId,
    ViewStream, input_channel_with_limits,
};
use crate::{SearchCase, SearchMode, SearchQuery, SessionStatus, TerminalAppearance, TerminalSize};

fn whole_history() -> CaptureOptions {
    CaptureOptions {
        start: CaptureBoundary::HistoryStart,
        end: CaptureBoundary::Relative(i64::MAX),
        ..CaptureOptions::default()
    }
}

#[test]
fn an_exited_child_answers_copy_reads_before_the_retention_decision() {
    let session = TerminalSession::spawn(
        1000,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            shell: Some("/bin/sh".to_owned()),
            command: Some(vec![
                "n=1; while [ $n -le 96 ]; do printf 'H%04d row-%04d\\n' $n $n; n=$((n + 1)); done; printf 'CHILD-DONE\\n'; exit 7"
                    .to_owned(),
            ]),
            initial_size: Some(TerminalSize::cells(40, 4)),
            non_login_shell: true,
            ..TerminalSpawn::default()
        },
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while session.completion().is_none() {
        assert!(Instant::now() < deadline, "short child never completed");
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(session.completion().expect("completion").code, 7);

    let started = Instant::now();
    let captured = session
        .capture_frozen_frame(whole_history())
        .expect("full history before a retention decision");
    let source = session
        .capture_copy_source()
        .expect("copy source before a retention decision");
    assert!(
        started.elapsed() < DEAD_NOTICE_WAIT,
        "copy reads waited for the retention deadline"
    );
    let expected = (1..=96)
        .map(|number| format!("H{number:04} row-{number:04}"))
        .chain(std::iter::once("CHILD-DONE".to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(
        captured
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>(),
        expected
    );
    let frozen = source.revision.capture_rows(
        0,
        source.revision.total_rows().saturating_sub(1),
        false,
        false,
    );
    assert_eq!(frozen.trim_end(), captured.trim_end());

    session.write_dead_notice(Some(Arc::from("RETAINED-FOR-COPY")));
    assert!(session.settle());
    let retained = session
        .capture_frozen_frame(whole_history())
        .expect("retained actor capture");
    for line in [
        "H0001 row-0001",
        "H0096 row-0096",
        "CHILD-DONE",
        "RETAINED-FOR-COPY",
    ] {
        assert!(retained.contains(line), "retained capture lost {line}");
    }
    assert!(matches!(
        session.latest_viewport().status,
        SessionStatus::Exited(_)
    ));

    let view = TerminalViewId(9101);
    session.attach_view(view);
    session.set_view_stream(view, ViewStream::Foreground);
    session.view_action(view, TerminalViewAction::EnterCopyMode);
    session.view_action(
        view,
        TerminalViewAction::CopyMode(CopyModeAction::Search(Box::new(CopyModeSearch {
            text: "H0040".to_owned(),
            direction: SearchDirection::Backward,
            regex: false,
            incremental: false,
        }))),
    );
    assert!(session.settle());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let facts = session.copy_mode_facts(view);
        if facts
            .as_ref()
            .is_some_and(|facts| facts.cursor_line.starts_with("H0040 row-0040"))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "retained actor search never published the expected cursor; last facts: {facts:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let expected_geometry = (32, 6);
    session.resize(32, 6, 8, 18);
    assert!(session.settle());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let resized = session.latest_viewport_for(view);
        let facts = session.copy_mode_facts(view);
        if resized
            .as_ref()
            .is_some_and(|frame| (frame.columns, frame.rows) == expected_geometry)
            && facts
                .as_ref()
                .is_some_and(|facts| facts.cursor_line.starts_with("H0040 row-0040"))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "retained actor resize never published {expected_geometry:?} with its cursor; last geometry: {:?}; last facts: {facts:?}",
            resized.as_ref().map(|frame| (frame.columns, frame.rows))
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        source.revision.capture_rows(
            0,
            source.revision.total_rows().saturating_sub(1),
            false,
            false,
        ),
        frozen
    );
}

fn gated_search_child() -> TerminalSession {
    let session = TerminalSession::spawn(
        1000,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            shell: Some("/bin/sh".to_owned()),
            command: Some(vec![
                "printf 'alpha needle\\r\\nbeta needle\\r\\nomega needle\\r\\nSEARCH-READY'; IFS= read -r line; exit 7"
                    .to_owned(),
            ]),
            initial_size: Some(TerminalSize::cells(40, 4)),
            non_login_shell: true,
            ..TerminalSpawn::default()
        },
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let captured = session
            .capture_frozen_frame(whole_history())
            .expect("gated child capture");
        if captured.contains("SEARCH-READY") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "child never reached its input gate"
        );
        thread::sleep(Duration::from_millis(10));
    }
    session
}

#[test]
fn a_pending_frozen_search_keeps_its_direction_when_the_child_is_retained() {
    for direction in [SearchDirection::Forward, SearchDirection::Backward] {
        pending_frozen_search_keeps_its_direction_when_retained(direction);
    }
}

fn pending_frozen_search_keeps_its_direction_when_retained(direction: SearchDirection) {
    let (expected_current, expected_row, expected_line) = match direction {
        SearchDirection::Forward => (1, 0, "alpha needle"),
        SearchDirection::Backward => (3, 2, "omega needle"),
    };
    let session = gated_search_child();
    let source = session.capture_copy_source().expect("gated copy source");
    let revision = Arc::clone(&source.revision);
    assert_eq!(revision.total_rows(), 4);
    let view = TerminalViewId(9103);
    session.attach_view(view);
    session.set_view_stream(view, ViewStream::Foreground);
    session.set_pending_copy_source(Some(Box::new(source)));
    session.view_action(view, TerminalViewAction::EnterCopyMode);
    assert!(session.settle());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let facts = session.copy_mode_facts(view);
        if facts
            .as_ref()
            .is_some_and(|facts| facts.cursor_line == "SEARCH-READY")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "gated copy mode never published; last facts: {facts:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let _ = revision.viewport_cells(0);
    let blocked_search = revision.search.search_gate.lock();
    session.view_action(
        view,
        TerminalViewAction::SearchBegin(SearchQuery {
            text: "needle".to_owned(),
            mode: SearchMode::Literal,
            case: SearchCase::Smart,
            direction,
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let pending = session
            .latest_viewport_for(view)
            .and_then(|frame| frame.search)
            .is_some_and(crate::model::SearchStatus::pending);
        if pending {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "blocked search never became pending"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(session.send_raw_input(Arc::from(b"leave\n".as_slice())));
    let deadline = Instant::now() + Duration::from_secs(30);
    while session.completion().is_none() {
        assert!(
            Instant::now() < deadline,
            "child completion waited for an unrelated frozen search"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        session.completion().expect("gated child completion").code,
        7
    );
    assert!(!session.try_send_text("after-exit\n"));
    assert_eq!(session.commands.pending_input(), (0, 0));
    assert_eq!(session.diagnostics().pending_pty_input_bytes, 0);
    session.write_dead_notice(Some(Arc::from("")));
    assert!(session.settle());
    drop(blocked_search);

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let frame = session.latest_viewport_for(view);
        let facts = session.copy_mode_facts(view);
        if frame.as_ref().is_some_and(|frame| {
            matches!(frame.status, SessionStatus::Exited(_))
                && frame.search.as_ref().is_some_and(|search| {
                    !search.pending() && search.total == 3 && search.current() == expected_current
                })
        }) && facts.as_ref().is_some_and(|facts| {
            facts.cursor_line == expected_line
                && facts.cursor_x == 6
                && facts.cursor_y == expected_row
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "retained pending search lost its {direction:?} direction; last search: {:?}; last facts: {facts:?}",
            frame.as_ref().and_then(|frame| frame.search.as_ref())
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(session.pane_search_string(), "needle");
    let captured = session
        .capture_frozen_frame(CaptureOptions {
            mode: true,
            ..whole_history()
        })
        .expect("retained frozen capture after pending search");
    assert_eq!(
        captured.trim_end(),
        "alpha needle\nbeta needle\nomega needle\nSEARCH-READY"
    );
    assert_eq!(session.diagnostics().pending_pty_input_bytes, 0);
}

#[test]
fn a_completed_emacs_forward_search_keeps_its_end_cursor_when_the_child_is_retained() {
    let session = gated_search_child();
    let view = TerminalViewId(9104);
    session.attach_view(view);
    session.set_view_stream(view, ViewStream::Foreground);
    session.view_action(view, TerminalViewAction::EnterCopyMode);
    session.view_action(
        view,
        TerminalViewAction::CopyMode(CopyModeAction::Search(Box::new(CopyModeSearch {
            text: "needle".to_owned(),
            direction: SearchDirection::Forward,
            regex: false,
            incremental: false,
        }))),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let original = loop {
        let facts = session.copy_mode_facts(view);
        if let Some(facts) = facts.as_ref().filter(|facts| {
            facts.cursor_line == "alpha needle"
                && facts.cursor_x == 12
                && facts.search_count == Some((3, false))
        }) {
            break Arc::clone(facts);
        }
        assert!(
            Instant::now() < deadline,
            "emacs forward search never placed its inclusive end cursor; last facts: {facts:?}"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(session.send_raw_input(Arc::from(b"leave\n".as_slice())));
    let deadline = Instant::now() + Duration::from_secs(30);
    while session.completion().is_none() {
        assert!(Instant::now() < deadline, "gated child never exited");
        thread::sleep(Duration::from_millis(10));
    }
    session.write_dead_notice(Some(Arc::from("")));
    assert!(session.settle());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if session
            .latest_viewport_for(view)
            .is_some_and(|frame| matches!(frame.status, SessionStatus::Exited(_)))
        {
            break;
        }
        assert!(Instant::now() < deadline, "retained frame never published");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        session.copy_mode_facts(view).expect("retained copy facts"),
        original
    );
}

#[test]
fn closing_input_releases_queued_payloads_and_admission_while_the_sender_survives() {
    let (sender, receiver) = input_channel_with_limits(4, 64 * 1024);
    let payload: Arc<[u8]> = Arc::from(vec![b'x'; 16 * 1024]);
    let weak = Arc::downgrade(&payload);
    sender
        .try_send(classified_input(payload))
        .expect("queued input");
    assert_eq!(sender.pending().0, 1);
    assert!(sender.pending().1 >= 16 * 1024);
    assert!(weak.upgrade().is_some());
    drop(receiver);
    assert_eq!(sender.pending(), (0, 0));
    assert!(weak.upgrade().is_none());
    assert!(matches!(
        sender.try_send(classified_input(Arc::from(b"later".as_slice()))),
        Err(crossbeam_channel::TrySendError::Disconnected(_))
    ));
    assert_eq!(sender.pending(), (0, 0));
}

fn classified_input(bytes: Arc<[u8]>) -> Command {
    Command::PastePreparedBytes {
        view: None,
        bytes,
        bracketed: false,
    }
}

#[test]
fn racing_input_enqueue_and_shutdown_releases_every_accepted_payload_and_permit() {
    for _ in 0..64 {
        let (sender, receiver) = input_channel_with_limits(4, 64 * 1024);
        let payload: Arc<[u8]> = Arc::from(vec![b'x'; 4096]);
        let first = Arc::downgrade(&payload);
        sender
            .try_send(classified_input(payload))
            .expect("queued input before shutdown race");
        let barrier = Barrier::new(2);
        let accepted = thread::scope(|scope| {
            let producer = scope.spawn(|| {
                let mut accepted = Vec::new();
                barrier.wait();
                for _ in 0..64 {
                    let payload: Arc<[u8]> = Arc::from(vec![b'y'; 4096]);
                    let weak = Arc::downgrade(&payload);
                    match sender.try_send(classified_input(payload)) {
                        Ok(()) => accepted.push(weak),
                        Err(crossbeam_channel::TrySendError::Full(command)) => drop(command),
                        Err(crossbeam_channel::TrySendError::Disconnected(command)) => {
                            drop(command);
                            break;
                        }
                    }
                }
                accepted
            });
            barrier.wait();
            drop(receiver);
            producer.join().expect("input producer")
        });
        assert_eq!(sender.pending(), (0, 0));
        assert!(first.upgrade().is_none());
        assert!(accepted.iter().all(|payload| payload.upgrade().is_none()));
        assert!(matches!(
            sender.try_send(classified_input(Arc::from(b"later".as_slice()))),
            Err(crossbeam_channel::TrySendError::Disconnected(_))
        ));
        assert_eq!(sender.pending(), (0, 0));
    }
}
