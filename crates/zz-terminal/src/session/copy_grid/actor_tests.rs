#![cfg(unix)]

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::session::{
    CaptureBoundary, CaptureOptions, CopyModeAction, CopyModeSearch, DEAD_NOTICE_WAIT,
    SearchDirection, TerminalSession, TerminalSpawn, TerminalViewAction, TerminalViewId,
    ViewStream,
};
use crate::{SessionStatus, TerminalAppearance, TerminalSize};

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
    session.resize(32, 6, 8, 18);
    assert!(session.settle());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let resized = session.latest_viewport_for(view);
        let facts = session.copy_mode_facts(view);
        if resized
            .as_ref()
            .is_some_and(|frame| (frame.columns, frame.rows) == (32, 6))
            && facts
                .as_ref()
                .is_some_and(|facts| facts.cursor_line.starts_with("H0040 row-0040"))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "retained actor resize never published 32x6 with its cursor; last geometry: {:?}; last facts: {facts:?}",
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
            false,
        ),
        frozen
    );
}
