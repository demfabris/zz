use super::*;

fn session(shard: &ShardHandle, command: &str) -> TerminalSession {
    TerminalSession::spawn_with_shard(
        100,
        Arc::new(TerminalAppearance::default()),
        TerminalSpawn {
            command: Some(vec![command.to_owned()]),
            ..TerminalSpawn::default()
        },
        Ok(Some(shard.clone())),
    )
}

fn wait(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "shard did not service the pane in time"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn captured(session: &TerminalSession, text: &str) -> bool {
    session
        .capture(CaptureOptions::default())
        .is_ok_and(|output| output.contains(text))
}

#[test]
fn a_dead_notice_wait_and_retained_surface_do_not_block_other_panes() {
    let shard = ShardHandle::start(100).expect("shard");
    let dead = session(&shard, "printf '\\033]2;program title\\007finished\\r\\n'");
    wait(|| dead.completion().is_some());
    let title_writes = dead.facts().program_title_writes;
    assert!(title_writes > 0);
    let live = session(&shard, "stty -echo; printf 'ready\\r\\n'; exec cat");
    wait(|| captured(&live, "ready"));
    dead.write_dead_notice(Some(Arc::from("retained")));
    wait(|| captured(&dead, "retained"));
    assert_eq!(dead.facts().program_title_writes, title_writes);
    live.send_text("after retained\n");
    wait(|| captured(&live, "after retained"));
    dead.resize(90, 30, 8, 18);
    assert!(captured(&dead, "finished"));
    assert!(captured(&dead, "retained"));
}

#[test]
fn a_backpressured_flood_does_not_starve_input_on_the_same_shard() {
    let shard = ShardHandle::start(101).expect("shard");
    let floods = (0..4)
        .map(|_| session(&shard, "exec yes flood"))
        .collect::<Vec<_>>();
    let quiet = session(&shard, "stty -echo; printf 'ready\\r\\n'; exec cat");
    wait(|| captured(&quiet, "ready"));
    quiet.send_text("quiet echo\n");
    wait(|| captured(&quiet, "quiet echo"));
    drop(floods);
}

#[test]
fn streamed_echoes_and_busy_panes_both_progress_on_one_shard() {
    let shard = ShardHandle::start(107).expect("shard");
    let floods = (0..2)
        .map(|_| session(&shard, "exec yes flood"))
        .collect::<Vec<_>>();
    for flood in &floods {
        flood.set_preview_watch(true);
        wait(|| flood.latest_viewport().generation > 1);
    }
    let generations = floods
        .iter()
        .map(|flood| flood.latest_viewport().generation)
        .collect::<Vec<_>>();
    let quiet = session(&shard, "stty -echo; printf 'ready\\r\\n'; exec cat");
    let view = TerminalViewId(107);
    quiet.attach_view(view);
    quiet.set_view_stream(view, ViewStream::Foreground);
    let visible = |needle: &str| {
        quiet.latest_view_frames().iter().any(|(id, viewport, _)| {
            let mut text = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut text);
            }
            *id == view && text.contains(needle)
        })
    };
    wait(|| visible("ready"));
    for burst in 0..32 {
        let text = format!("echo {burst:02}");
        quiet.send_text(format!("{text}\n"));
        wait(|| visible(&text));
    }
    for (flood, generation) in floods.iter().zip(generations) {
        wait(|| flood.latest_viewport().generation > generation);
    }
}

#[test]
fn shutdown_of_a_signal_ignoring_child_does_not_block_other_panes() {
    let shard = ShardHandle::start(102).expect("shard");
    let stubborn = session(
        &shard,
        "trap '' HUP TERM; printf 'ready\\r\\n'; while :; do sleep 1; done",
    );
    wait(|| captured(&stubborn, "ready"));
    let events = stubborn.events();
    drop(stubborn);
    let live = session(&shard, "printf 'other pane\\r\\n'; exec cat");
    wait(|| captured(&live, "other pane"));
    wait(|| events.receiver.is_closed());
}

#[cfg(target_os = "linux")]
#[test]
fn shards_start_on_assignment_and_survive_the_last_pane() {
    const CHILD: &str = "ZZ_SHARD_START_TEST";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "session::shard::tests::shards_start_on_assignment_and_survive_the_last_pane",
            ])
            .env(CHILD, "1")
            .env("ZZ_PTY_SHARDS", "4")
            .output()
            .expect("isolated shard test");
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let threads = || {
        std::fs::read_dir("/proc/self/task")
            .expect("process threads")
            .filter_map(Result::ok)
            .filter_map(|entry| std::fs::read_to_string(entry.path().join("comm")).ok())
            .filter(|name| name.starts_with("zz-pty-shard-"))
            .count()
    };
    assert_eq!(threads(), 0);
    let first = choose().expect("choose shard").expect("enabled shards");
    let pane = session(&first, "printf 'ready\\r\\n'; exec cat");
    wait(|| captured(&pane, "ready"));
    assert_eq!(threads(), 1);
    drop(pane);
    for expected in 2..=4 {
        let _ = choose().expect("choose shard").expect("enabled shards");
        wait(|| threads() == expected);
    }
    let _ = choose().expect("reuse shard").expect("enabled shards");
    assert_eq!(threads(), 4);
}

#[test]
fn coalesced_wakes_service_concurrent_view_and_input_bursts() {
    let shard = ShardHandle::start(103).expect("shard");
    let panes = (0..4)
        .map(|_| session(&shard, "stty -echo; printf 'ready\\r\\n'; exec cat"))
        .collect::<Vec<_>>();
    for pane in &panes {
        wait(|| captured(pane, "ready"));
    }
    thread::scope(|scope| {
        for (index, pane) in panes.iter().enumerate() {
            scope.spawn(move || {
                let view = TerminalViewId(index as u64);
                for burst in 0..32 {
                    pane.attach_view(view);
                    pane.set_view_stream(view, ViewStream::Foreground);
                    pane.send_text(format!("burst {burst}\n"));
                    pane.set_view_stream(view, ViewStream::Off);
                    pane.detach_view(view);
                }
                pane.send_text("last burst\n");
                wait(|| captured(pane, "last burst"));
            });
        }
    });
}

#[cfg(target_os = "linux")]
#[test]
fn twenty_empty_and_output_surfaces_only_start_the_configured_shards() {
    const CHILD: &str = "ZZ_SURFACE_SHARD_TEST";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "session::shard::tests::twenty_empty_and_output_surfaces_only_start_the_configured_shards",
            ])
            .env(CHILD, "1")
            .env("ZZ_PTY_SHARDS", "4")
            .output()
            .expect("isolated surface shard test");
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let threads = || {
        std::fs::read_dir("/proc/self/task")
            .expect("process threads")
            .map(|entry| {
                std::fs::read_to_string(entry.expect("thread").path().join("comm"))
                    .expect("thread name")
            })
            .collect::<Vec<_>>()
    };
    let before = threads().len();
    let appearance = Arc::new(TerminalAppearance::default());
    let mut surfaces = Vec::new();
    for index in 0..20 {
        let empty = TerminalSession::spawn_empty_with_appearance(64, Arc::clone(&appearance));
        let output = if index % 2 == 0 {
            TerminalSession::spawn_output_view_with_appearance(
                "output".into(),
                "surface text".into(),
                Arc::clone(&appearance),
            )
        } else {
            TerminalSession::spawn_startup_output_view_with_appearance(
                "startup".into(),
                "surface text".into(),
                Arc::clone(&appearance),
            )
        };
        assert!(captured(&empty, ""));
        assert!(captured(&output, "surface text"));
        assert_eq!(empty.process_id(), None);
        assert_eq!(output.process_id(), None);
        surfaces.extend([empty, output]);
    }
    let names = threads();
    assert_eq!(names.len(), before + 4);
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("zz-pty-shard-"))
            .count(),
        4
    );
    assert!(
        names
            .iter()
            .all(|name| !name.starts_with("zz-empty-pane") && !name.starts_with("zz-output-view"))
    );
    for surface in &surfaces {
        surface.terminate();
        wait(|| {
            matches!(
                surface.commands.queues.liveness.try_recv(),
                Err(crossbeam_channel::TryRecvError::Disconnected)
            )
        });
        assert!(matches!(
            surface.capture(CaptureOptions::default()),
            Err(TerminalCaptureError::ActorStopped)
        ));
    }
    assert_eq!(threads().len(), before + 4);
}

#[cfg(target_os = "linux")]
#[test]
fn twenty_panes_select_direct_or_gather_readers() {
    const CHILD: &str = "ZZ_LINUX_READER_TEST";
    if std::env::var_os(CHILD).is_none() {
        for gather in ["0", "1"] {
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "session::shard::tests::twenty_panes_select_direct_or_gather_readers",
                ])
                .env(CHILD, "1")
                .env("ZZ_PTY_SHARDS", "4")
                .env("ZZ_PTY_GATHER", gather)
                .output()
                .expect("isolated Linux reader test");
            assert!(
                output.status.success(),
                "gather={gather}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    }
    let panes = (0..20)
        .map(|_| {
            TerminalSession::spawn(
                100,
                Arc::new(TerminalAppearance::default()),
                TerminalSpawn {
                    command: Some(vec!["printf 'ready\\r\\n'; exec cat".to_owned()]),
                    ..TerminalSpawn::default()
                },
            )
        })
        .collect::<Vec<_>>();
    for pane in &panes {
        wait(|| captured(pane, "ready"));
    }
    let names = std::fs::read_dir("/proc/self/task")
        .expect("process threads")
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("comm")).ok())
        .collect::<Vec<_>>();
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("zz-pty-shard-"))
            .count(),
        4
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("zz-pty-reader"))
            .count(),
        0
    );
    let gather = std::env::var("ZZ_PTY_GATHER").expect("gather setting") == "1";
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("zz-pty-gather"))
            .count(),
        if gather { 20 } else { 0 }
    );
}

#[test]
fn direct_reads_keep_the_final_batch_before_hangup() {
    let shard = ShardHandle::start(105).expect("shard");
    let pane = session(
        &shard,
        "head -c 262144 /dev/zero | tr '\\0' x; printf '\\r\\nlast batch\\r\\n'",
    );
    wait(|| pane.completion().is_some());
    assert!(captured(&pane, "last batch"));
}

#[test]
fn partial_batches_publish_after_the_producer_stops() {
    let shard = ShardHandle::start(104).expect("shard");
    let pane = session(
        &shard,
        "head -c 4096 /dev/zero | tr '\\0' x; exec sleep 10000",
    );
    wait(|| pane.facts().history_size > 0);
    assert!(pane.completion().is_none());
}

#[test]
fn short_lived_children_complete_without_output_on_a_shared_shard() {
    let shard = ShardHandle::start(106).expect("shard");
    let panes = (0..20)
        .map(|_| session(&shard, "exit 7"))
        .collect::<Vec<_>>();
    for pane in &panes {
        wait(|| pane.completion().is_some());
        assert_eq!(pane.completion().expect("child exit").code, 7);
    }
}
