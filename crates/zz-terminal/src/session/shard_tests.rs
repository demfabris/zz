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
