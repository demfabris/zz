use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use zz_terminal::SessionStatus;

use super::*;

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

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

fn terminal(shared: &Shared, pane: PaneId) -> Arc<TerminalSession> {
    Arc::clone(&shared.inner.lock().terminals[&pane])
}

fn pane_of(shared: &Arc<Shared>, context: &mut ExecutionContext, target: &str) -> PaneId {
    let id = run(
        shared,
        context,
        "display-message",
        &["-p", "-t", target, "#{pane_id}"],
    );
    id.trim()
        .parse()
        .unwrap_or_else(|_| panic!("pane id {id:?}"))
}

fn viewport_text(viewport: &TerminalViewport) -> String {
    let mut text = String::new();
    for cell in viewport.cells.iter() {
        viewport.push_glyph(*cell, &mut text);
    }
    text
}

fn printing_session(
    shared: &Arc<Shared>,
    name: &str,
) -> (ClientId, Arc<OutboundMailbox>, PaneId, PaneId) {
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    run(
        shared,
        &mut context,
        "new-session",
        &["-d", "-s", name, "-x", "60", "-y", "10", "read _"],
    );
    run(
        shared,
        &mut context,
        "new-window",
        &[
            "-d",
            "-t",
            &format!("{name}:"),
            "i=0; while :; do i=$((i+1)); printf 'count %d\\n' $i; sleep 0.01; done",
        ],
    );
    let idle = pane_of(shared, &mut context, &format!("{name}:0"));
    let printing = pane_of(shared, &mut context, &format!("{name}:1"));
    let session = {
        let inner = shared.inner.lock();
        inner
            .engine
            .state
            .window_for_pane(idle)
            .map(|window| inner.engine.state.windows[&window].session)
            .expect("session")
    };
    shared.attach(client, session).expect("attach");
    shared.refresh_terminal_visibility();
    (client, mailbox, idle, printing)
}

#[test]
fn a_hidden_pane_streams_nothing_until_its_window_is_shown() {
    let shared = Arc::new(Shared::new(1));
    let (client, mailbox, idle, printing) = printing_session(&shared, "hidden-stream");
    let view = TerminalViewId(client.0);
    {
        let inner = shared.inner.lock();
        let streamed = &inner.streamed_terminals[&client];
        assert!(streamed.contains_key(&idle));
        assert!(!streamed.contains_key(&printing));
    }
    let printer = terminal(&shared, printing);
    wait_until("the printer to run", || {
        viewport_text(&printer.latest_viewport()).contains("count")
    });
    thread::sleep(Duration::from_millis(200));
    assert!(
        printer
            .latest_view_frames()
            .iter()
            .all(|(published, _, _)| *published != view),
        "a hidden pane publishes no frames for the client"
    );
    assert!(
        !mailbox.state.lock().terminals.contains_key(&printing),
        "nothing is queued for the hidden pane"
    );

    let mut context =
        ExecutionContext::for_pane(&shared.inner.lock().engine.state, idle).expect("idle context");
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("select-window", ["-t", ":1"]),
        )
        .expect("show the printing window");
    shared.publish_snapshot();
    wait_until("the shown pane's frame", || {
        printer
            .latest_view_frames()
            .iter()
            .any(|(published, _, epoch)| *published == view && epoch.is_some())
    });
    wait_until("the shown pane in the mailbox", || {
        mailbox.state.lock().terminals.contains_key(&printing)
    });
}

#[test]
fn a_chooser_keeps_the_panes_it_previews_current() {
    let shared = Arc::new(Shared::new(1));
    let (client, _mailbox, idle, printing) = printing_session(&shared, "chooser-preview");
    let printer = terminal(&shared, printing);
    wait_until("the printer to run", || {
        viewport_text(&printer.latest_viewport()).contains("count")
    });
    let mut unwatched_stale = false;
    for _ in 0..40 {
        unwatched_stale |= !printer.latest_viewport_is_current();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        unwatched_stale,
        "an unwatched printing pane publishes metadata without building cells"
    );

    let mut context =
        ExecutionContext::for_pane(&shared.inner.lock().engine.state, idle).expect("idle context");
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("choose-tree", std::iter::empty::<&str>()),
        )
        .expect("open the chooser");
    shared.publish_snapshot();
    assert!(
        !shared.inner.lock().preview_watched.contains(&printing),
        "only the selected row's preview is watched"
    );
    {
        let mut inner = shared.inner.lock();
        let window = inner
            .engine
            .state
            .window_for_pane(printing)
            .expect("printing window");
        let chooser = inner.choose_trees.get_mut(&client).expect("chooser");
        let row = chooser
            .rendered
            .items
            .iter()
            .position(|item| {
                item.target == ChooseTreeTarget::Window(window)
                    || item.target == ChooseTreeTarget::Pane(printing)
            })
            .expect("the printing window's row");
        chooser.rendered.selected = u32::try_from(row).expect("row index");
    }
    shared.publish_chooser_presentation(client);
    assert!(shared.inner.lock().preview_watched.contains(&printing));
    wait_until("the watched preview", || {
        printer.latest_viewport_is_current()
    });
    thread::sleep(Duration::from_millis(50));
    for _ in 0..40 {
        assert!(
            printer.latest_viewport_is_current(),
            "a previewed pane builds its cells on every publish"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let captured = run(
        &shared,
        &mut context,
        "capture-pane",
        &["-p", "-t", &printing.to_string()],
    );
    let last = captured
        .lines()
        .filter_map(|line| line.strip_prefix("count "))
        .filter_map(|count| count.trim().parse::<u64>().ok())
        .max()
        .expect("a captured count");
    wait_until("the preview to reach the captured count", || {
        viewport_text(&printer.latest_viewport())
            .split("count ")
            .filter_map(|rest| {
                rest.split(|character: char| !character.is_ascii_digit())
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
            .any(|count| count >= last)
    });

    shared.inner.lock().choose_trees.remove(&client);
    shared.publish_snapshot();
    assert!(!shared.inner.lock().preview_watched.contains(&printing));
}

#[test]
fn a_command_client_that_ran_copy_mode_and_capture_leaves_no_view_behind() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    run(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "no-views", "read _"],
    );
    let pane = context.pane.expect("pane");
    let pane_terminal = terminal(&shared, pane);
    let mailbox = OutboundMailbox::new();
    let (command_client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, Arc::clone(&mailbox));
    shared
        .execute(
            command_client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("capture-pane", ["-p", "-t", &pane.to_string()]),
        )
        .expect("capture");
    shared.unregister(command_client);
    assert_eq!(pane_terminal.known_view_count(), 0);

    let interactive_mailbox = OutboundMailbox::new();
    let (interactive, _) = shared.register_subscribed(
        ClientKind::Interactive,
        None,
        None,
        Arc::clone(&interactive_mailbox),
    );
    let session = context.session.expect("session");
    shared.attach(interactive, session).expect("attach");
    assert_eq!(pane_terminal.known_view_count(), 1);
    let (copying_client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, Arc::clone(&mailbox));
    for (name, args) in [
        ("copy-mode", vec!["-t".to_owned(), pane.to_string()]),
        (
            "capture-pane",
            vec!["-p".to_owned(), "-t".to_owned(), pane.to_string()],
        ),
        (
            "send-keys",
            vec![
                "-X".to_owned(),
                "-t".to_owned(),
                pane.to_string(),
                "cancel".to_owned(),
            ],
        ),
    ] {
        shared
            .execute(
                copying_client,
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(name, args.iter().map(String::as_str)),
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
    }
    shared.unregister(copying_client);
    assert_eq!(pane_terminal.known_view_count(), 1);
    shared.unregister(interactive);
    assert_eq!(pane_terminal.known_view_count(), 0);
}

#[test]
fn a_respawned_pane_streams_to_the_clients_that_watched_it() {
    let shared = Arc::new(Shared::new(1));
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    run(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "respawn-stream", "printf first; read _"],
    );
    let pane = context.pane.expect("pane");
    shared
        .attach(client, context.session.expect("session"))
        .expect("attach");
    let view = TerminalViewId(client.0);
    run(
        &shared,
        &mut context,
        "respawn-pane",
        &["-k", "-t", &pane.to_string(), "printf ZZ_RESPAWNED; read _"],
    );
    shared.publish_snapshot();
    let respawned = terminal(&shared, pane);
    wait_until("the respawned pane's streamed frame", || {
        respawned
            .latest_view_frames()
            .iter()
            .any(|(published, viewport, epoch)| {
                *published == view
                    && epoch.is_some()
                    && viewport_text(viewport).contains("ZZ_RESPAWNED")
            })
    });
}

#[test]
fn split_window_true_reports_its_exit_status_once() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    run(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "true-exit", "read _"],
    );
    run(
        &shared,
        &mut context,
        "set-option",
        &["-g", "remain-on-exit", "on"],
    );
    run(
        &shared,
        &mut context,
        "set-hook",
        &["-g", "pane-died", "set -gF @zz-died 'x#{@zz-died}'"],
    );
    let id = run(
        &shared,
        &mut context,
        "split-window",
        &["-d", "-P", "-F", "#{pane_id}", "true"],
    );
    let pane: PaneId = id.trim().parse().expect("split pane id");
    wait_until("the dead pane", || {
        run(
            &shared,
            &mut context,
            "display-message",
            &["-p", "-t", &pane.to_string(), "#{pane_dead}"],
        )
        .trim()
            == "1"
    });
    assert_eq!(
        run(
            &shared,
            &mut context,
            "display-message",
            &["-p", "-t", &pane.to_string(), "#{pane_dead_status}"]
        )
        .trim(),
        "0"
    );
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        run(&shared, &mut context, "show-options", &["-gqv", "@zz-died"]).trim(),
        "x",
        "pane-died fired once"
    );
    assert!(matches!(
        terminal(&shared, pane).latest_viewport().status,
        SessionStatus::Exited(_)
    ));
}

#[test]
fn split_window_with_an_empty_command_prints_its_pane_without_waiting() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    run(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "empty-split", "read _"],
    );
    let started = Instant::now();
    let id = run(
        &shared,
        &mut context,
        "split-window",
        &["-d", "-P", "-F", "#{pane_id}", ""],
    );
    assert!(id.trim().starts_with('%'), "{id:?}");
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "an empty pane has no child identity to wait for: {:?}",
        started.elapsed()
    );
}

#[test]
fn styled_wide_lines_fill_the_history_limit_like_plain_ones() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    run(
        &shared,
        &mut context,
        "set-option",
        &["-g", "history-limit", "2000"],
    );
    let history = |name: &str, style: &str| {
        let script = format!(
            "awk 'BEGIN {{ for (l = 0; l < 3000; l++) {{ s = \"\"; for (c = 0; c < 180; c++) s = s sprintf(\"{style}%c\", (l + c) % 216 + 16, 97 + c % 26); print s \"\\033[m\" }} print \"ZZ_FILLED\" }}'; read _"
        );
        let mut context = ExecutionContext::default();
        run(
            &shared,
            &mut context,
            "new-session",
            &["-d", "-s", name, "-x", "180", "-y", "50", &script],
        );
        let pane = context.pane.expect("pane").to_string();
        wait_until("the fill", || {
            run(&shared, &mut context, "capture-pane", &["-p", "-t", &pane]).contains("ZZ_FILLED")
        });
        run(
            &shared,
            &mut context,
            "display-message",
            &["-p", "-t", &pane, "#{history_size}"],
        )
        .trim()
        .parse::<usize>()
        .expect("history size")
    };
    let plain = history("plain-history", "%.0s");
    let styled = history("styled-history", "\\033[1;38;5;%d;48;5;17m");
    assert!((1_800..=2_000).contains(&plain), "plain kept {plain}");
    assert_eq!(
        styled, plain,
        "styled lines keep as much history as plain ones"
    );
}

#[test]
fn an_interactive_zsh_pane_runs_jobs_in_the_foreground_of_its_tty() {
    if !std::path::Path::new("/bin/zsh").exists() {
        return;
    }
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    run(
        &shared,
        &mut context,
        "set-option",
        &["-g", "default-shell", "/bin/zsh"],
    );
    run(
        &shared,
        &mut context,
        "new-session",
        &[
            "-d",
            "-s",
            "zsh-tty",
            "-x",
            "80",
            "-y",
            "10",
            "exec /bin/zsh -fi",
        ],
    );
    let pane = context.pane.expect("pane").to_string();
    let screen = |context: &mut ExecutionContext| {
        run(&shared, context, "capture-pane", &["-p", "-t", &pane])
    };
    run(
        &shared,
        &mut context,
        "send-keys",
        &[
            "-t",
            &pane,
            "[[ -o monitor ]] && print ZZ_MONITOR_ON",
            "Enter",
        ],
    );
    wait_until("job control", || {
        screen(&mut context).contains("\nZZ_MONITOR_ON")
    });
    run(
        &shared,
        &mut context,
        "send-keys",
        &["-t", &pane, "sleep 30", "Enter"],
    );
    wait_until("sleep in the foreground", || {
        run(
            &shared,
            &mut context,
            "display-message",
            &["-p", "-t", &pane, "#{pane_current_command}"],
        )
        .trim()
            == "sleep"
    });
    let interrupted = Instant::now();
    run(&shared, &mut context, "send-keys", &["-t", &pane, "C-c"]);
    run(
        &shared,
        &mut context,
        "send-keys",
        &["-t", &pane, "print ZZ_BACK", "Enter"],
    );
    wait_until("the prompt after C-c", || {
        screen(&mut context).contains("\nZZ_BACK")
    });
    assert!(interrupted.elapsed() < Duration::from_secs(20));
}
