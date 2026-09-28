use std::collections::BTreeSet;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::{
    SCROLLBACK_BACKSTOP_CAP, TerminalSession, TerminalSpawn, TerminalViewId, ViewStream,
    new_terminal, scrollback_backstop_bytes,
};
use crate::{SessionStatus, TerminalAppearance, TerminalSize, TerminalViewport};

fn text(viewport: &TerminalViewport) -> String {
    let mut contents = String::new();
    for cell in viewport.cells.iter() {
        viewport.push_glyph(*cell, &mut contents);
    }
    contents
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(2));
    }
}

fn shell_session(script: &str) -> TerminalSession {
    TerminalSession::spawn(
        1000,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            shell: Some("/bin/sh".to_owned()),
            command: Some(vec![script.to_owned()]),
            initial_size: Some(TerminalSize::cells(60, 8)),
            ..TerminalSpawn::default()
        },
    )
}

#[test]
fn an_unwatched_pane_keeps_its_metadata_current_without_rebuilding_cells_per_burst() {
    let session = shell_session(
        "read _; i=0; while [ $i -lt 60 ]; do printf '\\033]0;burst %d\\007line %d\\n' $i $i; i=$((i+1)); sleep 0.005; done; printf 'ZZ_DONE\\n'; read _",
    );
    wait_until("the shell to start", || {
        matches!(session.latest_viewport().status, SessionStatus::Running)
    });
    session.send_text("go\n");
    let mut generations = BTreeSet::new();
    let mut titles = BTreeSet::new();
    wait_until("the burst to settle", || {
        let viewport = session.latest_viewport();
        generations.insert(viewport.generation);
        titles.insert(viewport.title().to_owned());
        text(&viewport).contains("ZZ_DONE")
    });
    assert!(
        titles.len() > 3,
        "the title followed the burst through metadata: {titles:?}"
    );
    assert!(
        generations.len() <= 4,
        "cells were rebuilt per burst while nothing watched: {} generations",
        generations.len()
    );
    assert!(session.latest_viewport_for(TerminalViewId(1)).is_none());
}

#[test]
fn turning_a_stream_on_publishes_a_whole_frame_under_a_new_epoch() {
    let session = shell_session(
        "printf 'ZZ_READY\\n'; while read -r line; do printf '%s\\n' \"$line\"; done",
    );
    let view = TerminalViewId(7);
    session.attach_view(view);
    wait_until("the unwatched pane to settle", || {
        text(&session.latest_viewport()).contains("ZZ_READY")
    });
    assert!(
        session.latest_view_frames().is_empty(),
        "an attached view that does not stream publishes nothing"
    );

    session.set_view_stream(view, ViewStream::Foreground);
    wait_until("the streamed frame", || {
        session
            .latest_view_frames()
            .iter()
            .any(|(published, viewport, epoch)| {
                *published == view && epoch.is_some() && text(viewport).contains("ZZ_READY")
            })
    });
    let first = session.latest_view_frames()[0].2;

    session.set_view_stream(view, ViewStream::Off);
    session.send_text("ZZ_WHILE_OFF\n");
    wait_until("the stopped view to leave the published frames", || {
        text(&session.latest_viewport()).contains("ZZ_WHILE_OFF")
            && session.latest_view_frames().is_empty()
    });

    session.set_view_stream(view, ViewStream::Foreground);
    wait_until("the restarted frame", || {
        session
            .latest_view_frames()
            .iter()
            .any(|(published, viewport, epoch)| {
                *published == view && *epoch != first && text(viewport).contains("ZZ_WHILE_OFF")
            })
    });
}

#[test]
fn a_preview_watch_keeps_the_fallback_current_and_reports_it_once() {
    let session = shell_session("read _; printf 'ZZ_PREVIEWED\\n'; read _");
    wait_until("the shell to start", || {
        matches!(session.latest_viewport().status, SessionStatus::Running)
    });
    session.set_preview_watch(true);
    wait_until("the watched fallback", || session.take_preview_ready());
    assert!(!session.take_preview_ready(), "readiness is reported once");
    session.send_text("go\n");
    wait_until("the watched output", || {
        session.latest_viewport_is_current()
            && text(&session.latest_viewport()).contains("ZZ_PREVIEWED")
    });
}

#[test]
fn a_child_that_exits_at_once_reports_its_status_once() {
    let session = TerminalSession::spawn(
        100,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            command: Some(vec!["true".to_owned()]),
            ..TerminalSpawn::default()
        },
    );
    wait_until("the exit", || session.completion().is_some());
    let completion = session.completion().expect("completion");
    assert_eq!((completion.code, completion.signal), (0, None));
    assert!(session.wait_for_identity(Duration::ZERO));
    session.write_dead_notice(None);
    wait_until("the exited status", || {
        matches!(session.latest_viewport().status, SessionStatus::Exited(_))
    });
}

#[test]
fn a_pty_free_surface_resolves_its_identity_at_once() {
    let session =
        TerminalSession::spawn_empty_with_appearance(100, Arc::new(TerminalAppearance::default()));
    let started = Instant::now();
    assert!(session.wait_for_identity(Duration::from_secs(2)));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(session.process_id().is_none());
}

#[test]
fn a_spawned_pane_reports_its_pid_and_tty_through_the_identity_wait() {
    let session = shell_session("read _");
    assert!(session.wait_for_identity(Duration::from_secs(10)));
    assert!(session.process_id().is_some());
    let tty = session.tty().expect("pane tty");
    assert!(tty.to_string_lossy().starts_with("/dev/"), "{tty:?}");
}

#[test]
fn styled_wide_lines_fill_the_history_limit_like_plain_ones() {
    let retained = |styled: bool| {
        let mut terminal = new_terminal(180, 50, 10_000).expect("terminal");
        let mut output = Vec::new();
        for line in 0..12_000_u32 {
            for column in 0..180_u32 {
                if styled {
                    let colour = (line + column) % 216 + 16;
                    output.extend_from_slice(
                        format!("\x1b[1;38;5;{colour};48;5;{}m", 231 - colour + 16).as_bytes(),
                    );
                }
                output.push(b'a' + u8::try_from(column % 26).expect("letter"));
            }
            output.extend_from_slice(b"\x1b[m\r\n");
        }
        terminal.vt_write(&output);
        terminal.scrollback_rows().expect("history")
    };
    let plain = retained(false);
    let styled = retained(true);
    assert!((9_000..=10_000).contains(&plain), "plain kept {plain}");
    assert!((9_000..=10_000).contains(&styled), "styled kept {styled}");
}

#[test]
fn the_scrollback_byte_backstop_scales_with_width_and_is_capped() {
    assert_eq!(scrollback_backstop_bytes(10_000, 80), 10_000 * 80 * 40);
    assert_eq!(scrollback_backstop_bytes(10_000, 180), 10_000 * 180 * 40);
    assert_eq!(
        scrollback_backstop_bytes(1_000_000, 500),
        SCROLLBACK_BACKSTOP_CAP
    );
}

#[test]
fn compressed_history_reads_back_whole() {
    let session = TerminalSession::spawn(
        5000,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            shell: Some("/bin/sh".to_owned()),
            command: Some(vec![
                "read _; i=0; while [ $i -lt 3000 ]; do printf 'compressed history row %05d\\n' $i; i=$((i+1)); done; printf 'ZZ_FILLED\\n'; read _"
                    .to_owned(),
            ]),
            initial_size: Some(TerminalSize::cells(60, 8)),
            ..TerminalSpawn::default()
        },
    );
    wait_until("the shell to start", || {
        matches!(session.latest_viewport().status, SessionStatus::Running)
    });
    session.send_text("go\n");
    wait_until("the fill", || {
        text(&session.latest_viewport()).contains("ZZ_FILLED")
    });
    thread::sleep(super::COMPRESS_IDLE + Duration::from_millis(500));
    let capture = session
        .capture(super::CaptureOptions {
            start: super::CaptureBoundary::HistoryStart,
            end: super::CaptureBoundary::Relative(i64::MAX),
            ..super::CaptureOptions::default()
        })
        .expect("capture the whole history");
    for row in [0, 1500, 2999] {
        assert!(
            capture.contains(&format!("compressed history row {row:05}")),
            "row {row} survived compression"
        );
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
#[test]
#[allow(
    clippy::zombie_processes,
    reason = "the exit watch under test is the child's reaper"
)]
fn the_exit_watch_reaps_a_child_that_exited_before_it_was_registered() {
    let child = std::process::Command::new("/usr/bin/true")
        .spawn()
        .expect("spawn true");
    let pid = child.id();
    thread::sleep(Duration::from_millis(100));
    let mut watch = super::ChildExitWatch::new(Some(pid)).expect("watch");
    let status = watch
        .take_ready()
        .or_else(|| watch.wait_timeout(Duration::from_secs(5)))
        .expect("the exit")
        .expect("the exit status");
    assert_eq!(status.exit_code(), 0);
    assert!(
        watch.wait_timeout(Duration::ZERO).is_none(),
        "one owner reaps once"
    );
}

#[test]
fn an_interrupt_typed_on_the_pty_reaches_the_foreground_job() {
    let session = shell_session(
        "if (exec 3</dev/tty) 2>/dev/null; then echo ZZ_CTTY; fi; printf 'ZZ_READY\\n'; sleep 30",
    );
    wait_until("the controlling terminal", || {
        text(&session.latest_viewport()).contains("ZZ_READY")
    });
    assert!(text(&session.latest_viewport()).contains("ZZ_CTTY"));
    let started = Instant::now();
    assert!(session.send_raw_input(Arc::from(b"\x03".as_slice())));
    wait_until("the interrupted job", || session.completion().is_some());
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn an_unwatched_pane_wakes_its_watcher_a_few_times_a_second_under_steady_output() {
    let session = shell_session(
        "read _; i=0; while [ $i -lt 300 ]; do printf 'row %d\\n' $i; i=$((i+1)); sleep 0.005; done; printf 'ZZ_DONE\\n'; read _",
    );
    let events = session.events();
    wait_until("the shell to start", || {
        matches!(session.latest_viewport().status, SessionStatus::Running)
    });
    while events.try_recv().is_ok() {}
    session.send_text("go\n");
    let started = Instant::now();
    let mut ready = 0_u32;
    loop {
        match events.try_recv() {
            Ok(super::TerminalEvent::ViewportReady { .. }) => ready += 1,
            Ok(_) => {}
            Err(_) => {
                if text(&session.latest_viewport()).contains("ZZ_DONE") {
                    break;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(30),
                    "the output never finished"
                );
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let per_second = f64::from(ready) / elapsed;
    assert!(
        per_second <= 20.0,
        "{ready} watcher wakes in {elapsed:.2} s for a pane nobody streams"
    );
}
