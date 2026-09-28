use super::*;

fn engine_with_panes() -> (MuxEngine, WindowId, PaneId, PaneId) {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "runtime"]),
        )
        .expect("session");
    let window = context.window.expect("window");
    let active = context.pane.expect("pane");
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("split-window", ["-d"]),
        )
        .expect("split");
    let other = engine.state.windows[&window]
        .pane_order()
        .iter()
        .copied()
        .find(|pane| *pane != active)
        .expect("second pane");
    (engine, window, active, other)
}

fn run(engine: &mut MuxEngine, pane: PaneId, command: &str) -> bool {
    let facts = PaneRuntimeFacts {
        current_command: command.to_owned(),
        ..engine.pane_runtime_facts(pane).cloned().unwrap_or_default()
    };
    engine.set_pane_runtime_facts(pane, facts)
}

#[test]
fn runtime_facts_move_their_own_counter_not_the_tree() {
    let (mut engine, _, active, _) = engine_with_panes();
    engine
        .execute(
            &mut ExecutionContext::default(),
            &CommandInvocation::new("set-option", ["-g", "automatic-rename", "off"]),
        )
        .expect("disable rename");
    let tree = engine.state.generation();
    let runtime = engine.runtime_facts_generation();
    assert!(run(&mut engine, active, "vim"));
    assert_eq!(engine.state.generation(), tree);
    assert_eq!(engine.runtime_facts_generation(), runtime + 1);
    assert!(!run(&mut engine, active, "vim"));
    assert_eq!(engine.runtime_facts_generation(), runtime + 1);
}

#[test]
fn a_rename_moves_the_tree_generation() {
    let (mut engine, window, active, _) = engine_with_panes();
    let tree = engine.state.generation();
    assert!(run(&mut engine, active, "vim"));
    assert_eq!(engine.state.windows[&window].name, "vim");
    assert!(engine.state.generation() > tree);
}

#[test]
fn renames_inside_the_name_interval_wait_for_the_deadline() {
    let (mut engine, window, active, other) = engine_with_panes();
    engine.set_automatic_rename_throttle(true);
    assert!(run(&mut engine, active, "vim"));
    assert_eq!(engine.state.windows[&window].name, "vim");
    assert_eq!(engine.next_window_rename_deadline(), None);
    assert!(run(&mut engine, other, "top"));
    assert_eq!(engine.next_window_rename_deadline(), None);
    assert!(run(&mut engine, active, "less"));
    assert_eq!(engine.state.windows[&window].name, "vim");
    let deadline = engine
        .next_window_rename_deadline()
        .expect("rename deferred");
    let mut hooks = CommandHooks::new(engine.format_now());
    let early = deadline
        .checked_sub(NAME_INTERVAL / 2)
        .expect("deadline after the interval");
    assert!(!engine.apply_due_window_renames(early, &mut hooks));
    assert_eq!(engine.state.windows[&window].name, "vim");
    assert!(engine.apply_due_window_renames(deadline, &mut hooks));
    assert_eq!(engine.state.windows[&window].name, "less");
    assert_eq!(engine.next_window_rename_deadline(), None);
}

#[test]
fn an_unthrottled_engine_renames_every_change() {
    let (mut engine, window, active, _) = engine_with_panes();
    assert!(run(&mut engine, active, "vim"));
    assert!(run(&mut engine, active, "less"));
    assert_eq!(engine.state.windows[&window].name, "less");
    assert_eq!(engine.next_window_rename_deadline(), None);
}

#[test]
fn format_monitors_are_visible_to_the_status_tick() {
    let (mut engine, _, _, _) = engine_with_panes();
    assert!(!engine.has_format_monitors());
    engine
        .execute(
            &mut ExecutionContext::default(),
            &CommandInvocation::new(
                "set-hook",
                ["-B", "@watch:@*:#{window_name}", "set -g @fired yes"],
            ),
        )
        .expect("monitor");
    assert!(engine.has_format_monitors());
}

#[test]
fn presentation_templates_that_read_runtime_facts_are_detected() {
    let (mut engine, window, active, _) = engine_with_panes();
    assert!(!engine.runtime_facts_reach_presentation());
    let mut set = |args: &[&str], expected: bool| {
        engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new("set-option", args.iter().copied()),
            )
            .expect("option");
        assert_eq!(
            engine.runtime_facts_reach_presentation(),
            expected,
            "{args:?}"
        );
    };
    set(&["-g", "status-right", "#{pane_current_command}"], true);
    set(&["-gu", "status-right"], false);
    let window_target = window.to_string();
    set(
        &[
            "-w",
            "-t",
            &window_target,
            "window-status-format",
            "#{@label}",
        ],
        true,
    );
    set(
        &["-wu", "-t", &window_target, "window-status-format"],
        false,
    );
    let pane_target = active.to_string();
    set(
        &[
            "-p",
            "-t",
            &pane_target,
            "pane-border-format",
            "#{b:pane_current_path}",
        ],
        true,
    );
    set(&["-pu", "-t", &pane_target, "pane-border-format"], false);
    set(&["-g", "status-left", "plain #S"], false);
}

#[test]
fn only_clock_driven_window_labels_need_the_status_tick() {
    let (mut engine, _, _, _) = engine_with_panes();
    assert!(!engine.window_labels_follow_the_clock());
    let mut set = |args: &[&str], expected: bool| {
        engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new("set-option", args.iter().copied()),
            )
            .expect("option");
        assert_eq!(
            engine.window_labels_follow_the_clock(),
            expected,
            "{args:?}"
        );
    };
    set(&["-g", "status-right", "%H:%M:%S"], false);
    set(&["-g", "window-status-format", "#I %H:%M"], true);
    set(&["-gu", "window-status-format"], false);
    set(&["-g", "window-status-current-format", "#(date)"], true);
    set(&["-gu", "window-status-current-format"], false);
    set(&["-g", "pane-border-format", "#{t:pane_start_time}"], true);
}
