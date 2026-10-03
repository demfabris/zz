use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use zz_terminal::SessionStatus;

use super::*;

fn run(shared: &Arc<Shared>, context: &mut ExecutionContext, name: &str, args: &[&str]) -> String {
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            context,
            &CommandInvocation::new(name, args.iter().copied()),
        )
        .unwrap_or_else(|error| panic!("{name} {args:?}: {error}"))
        .output
        .to_string()
}

#[test]
fn a_detached_empty_split_matches_the_pin_and_builds_no_settle_snapshot() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    run(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "empty", "-x", "80", "-y", "24", "read _"],
    );
    let pane = run(
        &shared,
        &mut context,
        "split-window",
        &["-d", "-P", "-F", "#{pane_id}", "-t", "empty:0", ""],
    );
    let pane = pane.trim();
    let id: PaneId = pane.parse().unwrap_or_else(|_| panic!("pane id {pane:?}"));
    let terminal = Arc::clone(&shared.inner.lock().terminals[&id]);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !matches!(terminal.latest_viewport().status, SessionStatus::Running) {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the empty pane"
        );
        thread::sleep(Duration::from_millis(5));
    }

    assert_eq!(
        run(
            &shared,
            &mut context,
            "display-message",
            &[
                "-p",
                "-t",
                pane,
                "#{cursor_flag} #{pane_width}x#{pane_height}"
            ],
        )
        .trim(),
        "0 80x11"
    );
    let captured = run(&shared, &mut context, "capture-pane", &["-p", "-t", pane]);
    assert_eq!(captured, "\n".repeat(11));
    let viewport = terminal.latest_viewport();
    assert_eq!((viewport.columns, viewport.rows), (80, 11));

    thread::sleep(Duration::from_millis(300));
    assert!(
        !terminal.latest_viewport_is_current(),
        "an unwatched empty pane never builds its fallback"
    );
    run(&shared, &mut context, "kill-server", &[]);
}
