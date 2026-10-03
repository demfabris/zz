use std::collections::BTreeSet;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::{
    SCROLLBACK_BACKSTOP_CAP, SCROLLBACK_BACKSTOP_FLOOR, TerminalSession, TerminalSpawn,
    TerminalViewId, ViewStream, new_terminal, scrollback_backstop_bytes,
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
fn the_scrollback_byte_backstop_scales_with_width_between_a_floor_and_a_cap() {
    assert_eq!(
        scrollback_backstop_bytes(100, 80),
        SCROLLBACK_BACKSTOP_FLOOR
    );
    assert_eq!(scrollback_backstop_bytes(10_000, 180), 10_000 * 180 * 128);
    assert_eq!(
        scrollback_backstop_bytes(1_000_000, 500),
        SCROLLBACK_BACKSTOP_CAP
    );
}

#[test]
fn combining_marks_fill_the_history_limit_like_plain_text() {
    let retained = |columns: u16, limit: usize, lines: usize| {
        let mut terminal = new_terminal(columns, 50, limit).expect("terminal");
        let mut line = "a\u{301}\u{302}\u{303}\u{304}".repeat(usize::from(columns));
        line.push_str("\r\n");
        let chunk = line.repeat(500);
        for _ in 0..lines / 500 {
            terminal.vt_write(chunk.as_bytes());
        }
        terminal.scrollback_rows().expect("history")
    };
    let wide = retained(180, 2_000, 3_000);
    assert!(wide >= 1_800, "a 180-column pane kept {wide} of 2000 lines");
    let narrow = retained(8, 50_000, 60_000);
    assert!(
        narrow >= 45_000,
        "an 8-column pane kept {narrow} of 50000 lines"
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

#[test]
fn a_cleared_screen_survives_idle_compression() {
    let session = shell_session(
        "read _; i=0; while [ $i -lt 100 ]; do printf 'before clear row %03d\\n' $i; i=$((i+1)); done; printf '\\033[H\\033[2J\\033[3J'; printf 'ZZ_CLEARED\\n'; read _",
    );
    wait_until("the shell to start", || {
        matches!(session.latest_viewport().status, SessionStatus::Running)
    });
    session.send_text("go\n");
    wait_until("the clear", || {
        text(&session.latest_viewport()).contains("ZZ_CLEARED")
    });
    thread::sleep(super::COMPRESS_IDLE + Duration::from_millis(500));
    let capture = session
        .capture(super::CaptureOptions::default())
        .expect("capture the visible screen");
    assert!(
        capture.contains("ZZ_CLEARED"),
        "the cleared screen survived compression: {capture:?}"
    );
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

fn interrupted_by_a_typed_control_c(argv: &[&str], ready: Option<&str>) -> TerminalSession {
    let session = TerminalSession::spawn(
        1000,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            command: Some(argv.iter().map(|argument| (*argument).to_owned()).collect()),
            initial_size: Some(TerminalSize::cells(60, 8)),
            ..TerminalSpawn::default()
        },
    );
    assert!(session.wait_for_identity(Duration::from_secs(10)));
    if let Some(ready) = ready {
        wait_until("the child to start", || {
            text(&session.latest_viewport()).contains(ready)
        });
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.completion().is_none() {
        assert!(
            Instant::now() < deadline,
            "a typed ^C never reached the foreground job"
        );
        assert!(session.send_raw_input(Arc::from(b"\x03".as_slice())));
        thread::sleep(Duration::from_millis(50));
    }
    let completion = session.completion().expect("completion");
    assert!(
        completion.signal == Some(2) || completion.code == 130,
        "the job ended by SIGINT: {completion:?}"
    );
    session
}

#[test]
fn an_interrupt_typed_on_the_pty_reaches_a_program_that_never_opens_its_tty() {
    interrupted_by_a_typed_control_c(&["sleep", "30"], None);
}

#[test]
fn a_zsh_pane_owns_its_controlling_terminal() {
    if !std::path::Path::new("/bin/zsh").exists() {
        return;
    }
    let session = interrupted_by_a_typed_control_c(
        &[
            "/bin/zsh",
            "-fc",
            "if (exec 3</dev/tty) 2>/dev/null; then print ZZ_CTTY; fi; print ZZ_READY; sleep 30",
        ],
        Some("ZZ_READY"),
    );
    assert!(text(&session.latest_viewport()).contains("ZZ_CTTY"));
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

#[test]
fn only_sequences_that_change_published_metadata_hurry_an_unwatched_publish() {
    let mut terminal = new_terminal(40, 5, 100).expect("terminal");
    let mut filter = super::EngineFilter::default();
    let mut hinted = |chunks: &[&[u8]]| {
        filter.metadata_hint = false;
        for chunk in chunks {
            filter.write(
                chunk,
                super::EngineKnobs::default(),
                &mut terminal,
                &mut Vec::new(),
                &mut None,
                &mut None,
            );
        }
        filter.metadata_hint
    };
    assert!(!hinted(&[
        b"plain text\r\n\x1b[1;31mred\x1b[m\x1b[?25l\x1b[?25h"
    ]));
    assert!(!hinted(&[
        b"\x1b]133;A\x07\x1b]8;;https://example.com\x07x\x1b]8;;\x07"
    ]));
    assert!(hinted(&[b"text \x1b]2;title\x07 more"]));
    assert!(hinted(&[b"\x1b]0;ti", b"tle\x1b\\"]));
    assert!(hinted(&[b"\x1b]7;file://host/tmp\x07"]));
    assert!(hinted(&[b"\x1b[?1000;1006h"]));
    assert!(hinted(&[b"\x1b[?10", b"02l"]));
    assert!(hinted(&[b"\x1b[>1u"]));
    assert!(!hinted(&[b"\x1b[?u"]));
    assert!(hinted(&[b"\x1bc"]));
}

#[test]
fn an_unwatched_pane_publishes_a_few_times_a_second_under_steady_output() {
    let session = shell_session(
        "read _; i=0; while [ $i -lt 400 ]; do printf 'row %d\\n' $i; i=$((i+1)); sleep 0.004; done; printf 'ZZ_DONE\\n'; read _",
    );
    wait_until("the shell to start", || {
        matches!(session.latest_viewport().status, SessionStatus::Running)
    });
    session.send_text("go\n");
    let started = Instant::now();
    let mut published: Vec<Arc<TerminalViewport>> = Vec::new();
    wait_until("the output to finish", || {
        let viewport = session.latest_viewport();
        let done = text(&viewport).contains("ZZ_DONE");
        if published
            .last()
            .is_none_or(|last| !Arc::ptr_eq(last, &viewport))
        {
            published.push(viewport);
        }
        done
    });
    let elapsed = started.elapsed().as_secs_f64();
    let per_second = published.len() as f64 / elapsed;
    assert!(
        per_second <= 20.0,
        "{} fallback publishes in {elapsed:.2} s for a pane nobody streams",
        published.len()
    );
}

#[test]
fn slot_changes_made_after_a_queued_command_run_after_it() {
    use super::{
        ActorWake, Command, CommandQueues, CommandSender, CopyModeAction, TerminalViewAction,
        command_channel, take_control_slot,
    };

    let (control, control_rx) = command_channel();
    let slot = Arc::new(parking_lot::Mutex::new(super::ControlSlot::default()));
    let commands = CommandSender {
        queues: Arc::new(CommandQueues {
            control,
            input: None,
            liveness: crossbeam_channel::never(),
            slot: Arc::clone(&slot),
            wake: ActorWake::none(),
        }),
    };
    let view = TerminalViewId(3);
    commands.with_slot(|slot| {
        slot.known_views.insert(view);
        slot.pending.push_view(Command::AttachView(view));
        false
    });
    commands
        .send(Command::ViewAction {
            view,
            action: TerminalViewAction::CopyMode(CopyModeAction::Cancel),
        })
        .expect("queue the cancel");
    commands.with_slot(|slot| {
        slot.pending.push_view(Command::DetachView(view));
        true
    });
    commands.with_slot(|slot| {
        slot.pending.wrap_search = Some(false);
        true
    });
    let woke_by = control_rx.recv().expect("the queued cancel");
    let order = take_control_slot(&slot, Some(woke_by), true)
        .iter()
        .map(Command::name)
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        [
            "attach-view",
            "view-action",
            "detach-view",
            "set-wrap-search"
        ]
    );
    assert_eq!(slot.lock().in_flight, 0);
    commands.with_slot(|slot| {
        slot.pending.wrap_search = Some(true);
        true
    });
    assert!(matches!(control_rx.recv(), Ok(Command::Wake)));
    assert_eq!(
        take_control_slot(&slot, Some(Command::Wake), true)
            .iter()
            .map(Command::name)
            .collect::<Vec<_>>(),
        ["set-wrap-search", "wake"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_pane_gets_transparent_huge_pages_back_when_zz_turned_them_off() {
    let before = std::fs::read_to_string("/proc/self/status").expect("read own status");
    if before.contains("THP_enabled:\t0") {
        return;
    }
    super::unix_pty::disable_transparent_huge_pages();
    let after = std::fs::read_to_string("/proc/self/status").expect("read own status");
    assert!(after.contains("THP_enabled:\t0"), "zz turned them off here");
    let session =
        shell_session("awk '/^THP_enabled/ {print \"THP=\" $2}' /proc/self/status; read _");
    wait_until("the pane's THP state", || {
        text(&session.latest_viewport()).contains("THP=")
    });
    assert!(text(&session.latest_viewport()).contains("THP=1"));
    let spawned = std::fs::read_to_string("/proc/self/status").expect("read own status");
    assert!(
        spawned.contains("THP_enabled:\t0"),
        "spawning the pane left them off here"
    );
}
