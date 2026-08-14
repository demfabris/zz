use zz_mux::{COMMAND_SPECS, ExecutionContext, MuxEffect, MuxEngine, parse_config};
use zz_protocol::{Axis, CommandInvocation, LayoutNode, ServerError};

fn command(name: &str, args: &[&str]) -> CommandInvocation {
    CommandInvocation::new(name, args.iter().copied())
}

fn session_names(engine: &MuxEngine) -> Vec<String> {
    let mut names = engine
        .state
        .sessions
        .values()
        .map(|session| session.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn window_count(engine: &MuxEngine, session_name: &str) -> usize {
    let session = engine
        .state
        .sessions
        .values()
        .find(|session| session.name == session_name)
        .expect("session exists");
    session.windows.len()
}

fn window_indexes(engine: &MuxEngine, session_name: &str) -> Vec<u32> {
    let session = engine
        .state
        .sessions
        .values()
        .find(|session| session.name == session_name)
        .expect("session exists");
    session
        .windows
        .iter()
        .map(|window| engine.state.windows[window].index)
        .collect()
}

fn split_ratio(engine: &MuxEngine) -> f32 {
    let window = engine.state.windows.values().next().expect("window");
    match &window.layout {
        LayoutNode::Split { ratio, .. } => *ratio,
        LayoutNode::Pane(_) => panic!("expected a split"),
    }
}

#[test]
fn catalog_covers_the_options_the_handlers_read() {
    assert_eq!(COMMAND_SPECS.len(), 58);
    for name in ["kill-session", "kill-window", "kill-pane"] {
        let spec = COMMAND_SPECS
            .iter()
            .find(|spec| spec.name == name)
            .expect(name);
        assert!(
            spec.options.iter().any(|option| option.name == "-a"),
            "{name} catalog is missing -a"
        );
    }
    let split = COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == "split-window")
        .expect("split-window");
    assert!(split.options.iter().any(|option| option.name == "-p"));
    assert!(
        split
            .option("-l")
            .expect("split-window catalogs -l so its size cannot leak into the pane command")
            .value
            .is_some()
    );
    let new_session = COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == "new-session")
        .expect("new-session");
    let group = new_session
        .option("-t")
        .expect("new-session catalogs -t so its value cannot leak into the pane command");
    assert!(!group.completable);
    assert!(new_session.options.iter().any(|option| option.name == "-A"));
}

#[test]
fn kill_session_dash_a_keeps_only_the_target() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "keep"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-session", &["-s", "other"]))
        .unwrap();
    engine
        .execute(
            &mut context,
            &command("kill-session", &["-a", "-t", "keep"]),
        )
        .unwrap();
    assert_eq!(session_names(&engine), ["keep"]);
}

#[test]
fn kill_window_dash_a_keeps_only_the_target() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let first = context.window.unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "second"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "third"]))
        .unwrap();
    engine
        .execute(
            &mut context,
            &command("kill-window", &["-a", "-t", &first.to_string()]),
        )
        .unwrap();
    assert_eq!(window_count(&engine, "work"), 1);
    assert_eq!(engine.state.windows.values().next().unwrap().id, first);
}

#[test]
fn kill_pane_dash_a_keeps_only_the_target() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let first = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let second = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-v"]))
        .unwrap();
    let third = context.pane.unwrap();
    let removed = engine
        .execute(
            &mut context,
            &command("kill-pane", &["-a", "-t", &first.to_string()]),
        )
        .unwrap();
    assert!(matches!(
        removed.effects.first(),
        Some(MuxEffect::PanesRemoved(panes)) if panes == &vec![second, third]
    ));
    assert_eq!(
        engine
            .state
            .windows
            .values()
            .flat_map(|window| window.panes.keys().copied())
            .collect::<Vec<_>>(),
        [first]
    );
}

#[test]
fn bind_clustered_nr_flags_bind_a_repeatable_root_table_key() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &command("bind-key", &["-nr", "F2", "split-window", "-h"]),
        )
        .unwrap();
    assert!(engine.keys.get("prefix", "-nr").is_none());
    assert!(engine.keys.get("prefix", "F2").is_none());
    let binding = engine.keys.get("root", "F2").expect("root F2 is bound");
    assert!(binding.repeat);
    assert_eq!(binding.commands.len(), 1);
    assert_eq!(binding.commands[0].name, "split-window");
    assert_eq!(binding.commands[0].args, ["-h"]);
}

#[test]
fn brace_command_lists_bind_as_a_single_command_sequence() {
    let parsed = parse_config("test.conf", "bind c { new-window ; split-window }");
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.commands.len(), 1);
    assert_eq!(parsed.commands[0].name, "bind");
    assert_eq!(
        parsed.commands[0].args,
        ["c", "{ new-window ; split-window }"]
    );

    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine.execute(&mut context, &parsed.commands[0]).unwrap();
    let binding = engine.keys.get("prefix", "c").expect("bound block").clone();
    assert_eq!(
        binding
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>(),
        ["new-window", "split-window"]
    );
    for bound in &binding.commands {
        engine.execute(&mut context, bound).unwrap();
    }
    assert_eq!(window_count(&engine, "work"), 2);
    let window = context.window.expect("active window");
    assert_eq!(engine.state.windows[&window].panes.len(), 2);

    let flagged = parse_config(
        "test.conf",
        "bind -n F2 {\n  send-keys 'a ; b'\n  new-window\n}\n",
    );
    assert_eq!(flagged.commands.len(), 1);
    engine.execute(&mut context, &flagged.commands[0]).unwrap();
    let binding = engine.keys.get("root", "F2").expect("root block").clone();
    assert_eq!(binding.commands.len(), 2);
    assert_eq!(binding.commands[0].name, "send-keys");
    assert_eq!(binding.commands[0].args, ["a ; b"]);
    assert_eq!(binding.commands[1].name, "new-window");
    let listed = engine
        .execute(&mut context, &command("list-keys", &["-T", "root"]))
        .unwrap()
        .output;
    assert!(
        listed
            .lines()
            .any(|line| line == "bind-key -T root F2 send-keys 'a ; b' \\; new-window"),
        "{listed}"
    );
}

#[test]
fn new_window_dash_t_prefers_a_session_name_then_a_window_index() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-session", &["-s", "1"]))
        .unwrap();
    engine
        .execute(&mut context, &command("attach-session", &["-t", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-t", "1"]))
        .unwrap();
    assert_eq!(window_count(&engine, "work"), 1);
    assert_eq!(window_count(&engine, "1"), 2);

    engine
        .execute(&mut context, &command("attach-session", &["-t", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-t", "2"]))
        .unwrap();
    assert_eq!(window_indexes(&engine, "work"), [0, 2]);

    let error = engine
        .execute(&mut context, &command("new-window", &["-t", "2"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(message) if message == "index in use: 2"));

    let error = engine
        .execute(&mut context, &command("new-window", &["-t", "work:nope"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::MissingTarget(target) if target == "work:nope"));
}

#[test]
fn split_window_dash_l_sizes_the_new_pane_in_cells() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine.set_pane_geometry(context.pane.unwrap(), 80, 24);
    let created = engine
        .execute(&mut context, &command("split-window", &["-h", "-l", "20"]))
        .unwrap();
    assert!(matches!(
        created.effects.first(),
        Some(MuxEffect::PaneCreated { command: None, .. })
    ));
    assert!((split_ratio(&engine) - 0.75).abs() < 1e-5);
}

#[test]
fn split_window_dash_l_takes_a_percentage_and_names_the_missing_geometry() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h", "-l", "25%"]))
        .unwrap();
    assert!((split_ratio(&engine) - 0.75).abs() < 1e-5);

    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let error = engine
        .execute(&mut context, &command("split-window", &["-h", "-l", "20"]))
        .unwrap_err();
    assert!(
        matches!(error, ServerError::InvalidCommand(message) if message.contains("pane geometry"))
    );
    assert_eq!(engine.state.windows.values().next().unwrap().panes.len(), 1);
}

#[test]
fn split_window_dash_p_gives_the_new_pane_that_share() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let created = engine
        .execute(&mut context, &command("split-window", &["-p", "25"]))
        .unwrap();
    assert!(matches!(
        created.effects.first(),
        Some(MuxEffect::PaneCreated { command: None, .. })
    ));
    assert!((split_ratio(&engine) - 0.75).abs() < 1e-5);
}

#[test]
fn new_pane_dash_p_gives_the_new_pane_that_share() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-pane", &["-h", "-p", "25"]))
        .unwrap();
    assert!((split_ratio(&engine) - 0.75).abs() < 1e-5);
}

#[test]
fn split_window_dash_b_lands_the_new_pane_first() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let target = context.pane.unwrap();
    engine
        .execute(
            &mut context,
            &command("split-window", &["-h", "-b", "-p", "25"]),
        )
        .unwrap();
    let created = context.pane.unwrap();
    assert!((split_ratio(&engine) - 0.25).abs() < 1e-5);
    let window = engine.state.windows.values().next().expect("window");
    let LayoutNode::Split { first, second, .. } = &window.layout else {
        panic!("expected a split");
    };
    assert_eq!(**first, LayoutNode::Pane(created));
    assert_eq!(**second, LayoutNode::Pane(target));
    assert_eq!(window.pane_order(), [created, target]);
}

#[test]
fn split_window_dash_d_keeps_the_focus_on_the_current_pane() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let target = context.pane.unwrap();
    let created = engine
        .execute(&mut context, &command("split-window", &["-d"]))
        .unwrap();
    let Some(MuxEffect::PaneCreated { pane, .. }) = created.effects.first() else {
        panic!("expected a created pane");
    };
    assert_ne!(*pane, target);
    assert_eq!(context.pane, Some(target));
    assert_eq!(
        engine
            .state
            .windows
            .values()
            .next()
            .expect("window")
            .active_pane,
        target
    );
}

#[test]
fn split_window_dash_f_spans_the_whole_window() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    engine
        .execute(
            &mut context,
            &command("split-window", &["-f", "-v", "-p", "25"]),
        )
        .unwrap();
    let created = context.pane.unwrap();
    let window = engine.state.windows.values().next().expect("window");
    let LayoutNode::Split {
        axis,
        ratio,
        second,
        ..
    } = &window.layout
    else {
        panic!("expected a split");
    };
    assert_eq!(*axis, Axis::Vertical);
    assert!((*ratio - 0.75).abs() < 1e-5);
    assert_eq!(**second, LayoutNode::Pane(created));
}

#[test]
fn new_session_dash_t_is_rejected_instead_of_leaking_into_the_pane_command() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let error = engine
        .execute(&mut context, &command("new-session", &["-t", "name"]))
        .unwrap_err();
    assert!(
        matches!(error, ServerError::UnsupportedCommand(message) if message == "new-session -t (session groups)")
    );
    assert!(session_names(&engine).is_empty());
}

#[test]
fn kill_session_positional_name_is_the_target() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "keep"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-session", &["-s", "other"]))
        .unwrap();
    engine
        .execute(&mut context, &command("attach-session", &["-t", "keep"]))
        .unwrap();
    engine
        .execute(&mut context, &command("kill-session", &["other"]))
        .unwrap();
    assert_eq!(session_names(&engine), ["keep"]);
}

#[test]
fn resize_pane_dash_r_on_the_right_pane_grows_the_left_share() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let left = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    engine.set_pane_geometry(left, 100, 50);
    engine
        .execute(&mut context, &command("resize-pane", &["-R", "10"]))
        .unwrap();
    let ratio = split_ratio(&engine);
    assert!(
        (ratio - 0.55).abs() < 1e-5,
        "tmux moves the boundary right, and the right pane has no boundary of its own: {ratio}"
    );
}

#[test]
fn resize_pane_dash_r_grows_a_nested_pane_toward_its_right_neighbor() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let left = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    engine
        .execute(&mut context, &command("select-pane", &["-L"]))
        .unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    engine.set_pane_geometry(left, 25, 50);
    engine
        .execute(&mut context, &command("resize-pane", &["-R", "10"]))
        .unwrap();
    let window = engine.state.windows.values().next().expect("window");
    let LayoutNode::Split { ratio, first, .. } = &window.layout else {
        panic!("expected a split");
    };
    assert!(
        (*ratio - 0.6).abs() < 1e-5,
        "the boundary right of the nested pane moved: {ratio}"
    );
    let LayoutNode::Split { ratio, .. } = &**first else {
        panic!("expected a nested split");
    };
    assert!(
        (*ratio - 0.5).abs() < f32::EPSILON,
        "the boundary left of the nested pane stayed put: {ratio}"
    );
}

#[test]
fn resize_pane_dash_x_sets_an_absolute_width_from_either_side() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let left = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    engine.set_pane_geometry(left, 50, 50);
    engine
        .execute(&mut context, &command("resize-pane", &["-x", "25"]))
        .unwrap();
    assert!((split_ratio(&engine) - 0.75).abs() < 1e-5);

    engine.set_pane_geometry(left, 75, 50);
    engine
        .execute(&mut context, &command("select-pane", &["-L"]))
        .unwrap();
    engine
        .execute(&mut context, &command("resize-pane", &["-x", "25"]))
        .unwrap();
    assert!((split_ratio(&engine) - 0.25).abs() < 1e-5);
}

#[test]
fn resize_pane_dash_x_takes_a_percentage_and_names_the_missing_geometry() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let error = engine
        .execute(&mut context, &command("resize-pane", &["-x", "25"]))
        .unwrap_err();
    assert!(
        matches!(error, ServerError::InvalidCommand(message) if message.contains("pane geometry"))
    );
    assert!((split_ratio(&engine) - 0.5).abs() < f32::EPSILON);

    engine
        .execute(&mut context, &command("resize-pane", &["-x", "25%"]))
        .unwrap();
    assert!((split_ratio(&engine) - 0.75).abs() < 1e-5);
}

#[test]
fn resize_pane_rejects_an_unparseable_adjustment() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let error = engine
        .execute(&mut context, &command("resize-pane", &["-R", "wide"]))
        .unwrap_err();
    assert!(
        matches!(error, ServerError::InvalidCommand(message) if message == "invalid resize adjustment: wide")
    );
    assert!((split_ratio(&engine) - 0.5).abs() < f32::EPSILON);
}
