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
