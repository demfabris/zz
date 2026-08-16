use zz_mux::{COMMAND_SPECS, DetachScope, ExecutionContext, MuxEffect, MuxEngine, parse_config};
use zz_protocol::{Axis, CommandInvocation, KeyToken, LayoutNode, ServerError};

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
    assert_eq!(COMMAND_SPECS.len(), 59);
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

    let spec = |name: &str| {
        COMMAND_SPECS
            .iter()
            .find(|spec| spec.name == name)
            .expect("catalogued command")
    };
    assert!(
        spec("send-keys")
            .option("-N")
            .expect("send-keys catalogs -N so the count cannot be typed as a key")
            .value
            .is_some()
    );
    assert!(spec("send-keys").option("-H").is_some());
    assert!(spec("copy-mode").option("-d").is_some());
    assert!(spec("detach-client").option("-a").is_some());
    assert!(spec("detach-client").option("-s").unwrap().value.is_some());
    for flag in ["-n", "-p", "-T"] {
        assert!(
            spec("select-window").option(flag).is_some(),
            "select-window catalog is missing {flag}"
        );
    }
    for name in ["next-window", "previous-window"] {
        assert!(spec(name).option("-a").is_some(), "{name} is missing -a");
        assert!(
            spec(name).option("-t").expect(name).value.is_some(),
            "{name} is missing -t"
        );
    }
    assert!(spec("source-file").option("-q").is_some());
    assert!(
        spec("source-file").variadic.is_some(),
        "source-file takes every path it is given"
    );
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
fn new_window_dash_t_prefers_a_window_index_then_a_session_name() {
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
    assert_eq!(window_indexes(&engine, "work"), [0, 1]);
    assert_eq!(window_count(&engine, "1"), 1);

    engine
        .execute(&mut context, &command("new-window", &["-t", "3"]))
        .unwrap();
    assert_eq!(window_indexes(&engine, "work"), [0, 1, 3]);

    let error = engine
        .execute(&mut context, &command("new-window", &["-t", "3"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(message) if message == "index in use: 3"));

    engine
        .execute(&mut context, &command("new-window", &["-t", "work"]))
        .unwrap();
    assert_eq!(window_indexes(&engine, "work"), [0, 1, 2, 3]);

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
        matches!(error, ServerError::UnsupportedCommand(message) if message == "new-session -t")
    );
    assert!(session_names(&engine).is_empty());
}

#[test]
fn kill_commands_refuse_positional_targets_like_tmux() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "keep"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-session", &["-s", "other"]))
        .unwrap();
    for name in ["kill-session", "kill-window", "kill-pane", "attach-session"] {
        let error = engine
            .execute(&mut context, &command(name, &["other"]))
            .unwrap_err();
        assert!(
            matches!(error, ServerError::InvalidCommand(ref message)
                if message.contains("positional")),
            "{name} accepted a positional target"
        );
    }
    assert_eq!(session_names(&engine), ["keep", "other"]);

    engine
        .execute(&mut context, &command("kill-session", &["-t", "other"]))
        .unwrap();
    assert_eq!(session_names(&engine), ["keep"]);
}

#[test]
fn kill_session_dash_c_clears_alerts_and_kills_nothing() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();
    assert!(engine.state.set_pane_bell(pane, true));

    engine
        .execute(
            &mut context,
            &command("kill-session", &["-C", "-t", "work"]),
        )
        .unwrap();
    assert_eq!(session_names(&engine), ["work"]);
    assert!(
        !engine
            .state
            .windows
            .values()
            .any(|window| window.panes.values().any(|pane| pane.bell))
    );

    let error = engine
        .execute(
            &mut context,
            &command("kill-session", &["-f", "-t", "work"]),
        )
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(ref message)
        if message == "kill-session does not support -f"));
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

fn window_names(engine: &MuxEngine, session_name: &str) -> Vec<String> {
    let session = engine
        .state
        .sessions
        .values()
        .find(|session| session.name == session_name)
        .expect("session exists");
    session
        .windows
        .iter()
        .map(|window| engine.state.windows[window].name.clone())
        .collect()
}

fn active_window_name(engine: &MuxEngine, session_name: &str) -> String {
    let session = engine
        .state
        .sessions
        .values()
        .find(|session| session.name == session_name)
        .expect("session exists");
    engine.state.windows[&session.active_window].name.clone()
}

#[test]
fn send_keys_dash_n_repeats_the_keys_instead_of_typing_the_count() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let sent = engine
        .execute(&mut context, &command("send-keys", &["-N", "3", "x"]))
        .unwrap();
    assert!(
        matches!(sent.effects.first(), Some(MuxEffect::SendKeys { keys, .. })
        if keys == &vec![
            KeyToken::Literal("x".to_owned()),
            KeyToken::Literal("x".to_owned()),
            KeyToken::Literal("x".to_owned()),
        ]),
        "{:?}",
        sent.effects
    );

    let error = engine
        .execute(&mut context, &command("send-keys", &["-N", "0", "x"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(message)
            if message == "send-keys -N needs a positive repeat count: 0"));
}

#[test]
fn send_keys_dash_n_repeats_copy_mode_movement_but_never_a_copy() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let moved = engine
        .execute(
            &mut context,
            &command("send-keys", &["-X", "-N", "4", "cursor-up"]),
        )
        .unwrap();
    assert_eq!(moved.effects.len(), 4);
    let copied = engine
        .execute(
            &mut context,
            &command("send-keys", &["-X", "-N", "4", "copy-selection"]),
        )
        .unwrap();
    assert_eq!(copied.effects.len(), 1);
}

#[test]
fn send_keys_dash_h_takes_hexadecimal_character_codes() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let sent = engine
        .execute(&mut context, &command("send-keys", &["-H", "41", "42"]))
        .unwrap();
    assert!(
        matches!(sent.effects.first(), Some(MuxEffect::SendKeys { keys, .. })
        if keys == &vec![
            KeyToken::Literal("A".to_owned()),
            KeyToken::Literal("B".to_owned()),
        ]),
        "{:?}",
        sent.effects
    );

    let error = engine
        .execute(&mut context, &command("send-keys", &["-H", "zz"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(message)
            if message == "send-keys -H needs a character code: zz"));
}

#[test]
fn send_keys_reports_the_flags_it_cannot_honor() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    for flag in ["-R", "-M", "-K", "-F"] {
        let error = engine
            .execute(&mut context, &command("send-keys", &[flag, "x"]))
            .unwrap_err();
        assert!(
            matches!(&error, ServerError::InvalidCommand(message)
                if message == &format!("send-keys does not support {flag}")),
            "{error:?}"
        );
    }
}

#[test]
fn copy_mode_pages_down_with_dash_d_and_reports_dash_e() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let paged = engine
        .execute(&mut context, &command("copy-mode", &["-d"]))
        .unwrap();
    assert_eq!(paged.effects.len(), 2);
    let error = engine
        .execute(&mut context, &command("copy-mode", &["-e"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::InvalidCommand(message)
            if message == "copy-mode does not support -e"),
        "{error:?}"
    );
}

#[test]
fn detach_client_dash_a_leaves_the_caller_attached() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let mine = engine
        .execute(&mut context, &command("detach-client", &[]))
        .unwrap();
    assert_eq!(mine.effects, [MuxEffect::Detach(DetachScope::Client)]);

    let others = engine
        .execute(&mut context, &command("detach-client", &["-a"]))
        .unwrap();
    assert_eq!(others.effects, [MuxEffect::Detach(DetachScope::Others)]);

    let session = engine
        .state
        .sessions
        .values()
        .find(|session| session.name == "work")
        .expect("session exists")
        .id;
    let by_session = engine
        .execute(&mut context, &command("detach-client", &["-s", "work"]))
        .unwrap();
    assert_eq!(
        by_session.effects,
        [MuxEffect::Detach(DetachScope::Session(session))]
    );

    let error = engine
        .execute(&mut context, &command("detach-client", &["-t", "0"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::InvalidCommand(message)
            if message == "detach-client does not support -t"),
        "{error:?}"
    );
}

#[test]
fn select_window_steps_and_repeats_like_tmux() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("rename-window", &["first"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "second"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "third"]))
        .unwrap();
    assert_eq!(window_names(&engine, "work"), ["first", "second", "third"]);
    assert_eq!(active_window_name(&engine, "work"), "third");

    engine
        .execute(&mut context, &command("select-window", &["-n"]))
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "first");
    engine
        .execute(&mut context, &command("select-window", &["-p"]))
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "third");
    engine
        .execute(&mut context, &command("select-window", &["-l"]))
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "first");

    engine
        .execute(
            &mut context,
            &command("select-window", &["-T", "-t", "first"]),
        )
        .unwrap();
    assert_eq!(
        active_window_name(&engine, "work"),
        "third",
        "-T on the current window behaves like last-window"
    );
    engine
        .execute(
            &mut context,
            &command("select-window", &["-T", "-t", "second"]),
        )
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "second");
}

#[test]
fn next_and_previous_window_target_a_session_and_follow_alerts() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("rename-window", &["first"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "second"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "third"]))
        .unwrap();
    let belled = context.pane.unwrap();
    engine
        .execute(&mut context, &command("new-session", &["-s", "other"]))
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "third");

    engine
        .execute(&mut context, &command("next-window", &["-t", "work"]))
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "first");
    engine
        .execute(&mut context, &command("previous-window", &["-t", "work"]))
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "third");

    engine
        .execute(
            &mut context,
            &command("select-window", &["-t", "work:first"]),
        )
        .unwrap();
    let error = engine
        .execute(&mut context, &command("next-window", &["-a"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::InvalidCommand(message)
            if message == "no next window"),
        "{error:?}"
    );
    assert!(engine.state.set_pane_bell(belled, true));
    engine
        .execute(&mut context, &command("next-window", &["-a"]))
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "third");
}

#[test]
fn attach_session_reports_the_client_flags_it_cannot_honor() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    for flag in ["-r", "-x", "-E"] {
        let error = engine
            .execute(
                &mut context,
                &command("attach-session", &[flag, "-t", "work"]),
            )
            .unwrap_err();
        assert!(
            matches!(&error, ServerError::InvalidCommand(message)
                if message == &format!("attach-session does not support {flag}")),
            "{error:?}"
        );
    }
    engine
        .execute(
            &mut context,
            &command("attach-session", &["-d", "-t", "work"]),
        )
        .unwrap();
}

#[test]
fn list_keys_rejects_the_selectors_it_does_not_implement() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    for flag in ["-n", "-a", "-N", "-1"] {
        let error = engine
            .execute(&mut context, &command("list-keys", &[flag]))
            .unwrap_err();
        assert!(
            matches!(&error, ServerError::InvalidCommand(message)
                if message == &format!("list-keys does not support {flag}")),
            "{error:?}"
        );
    }
    let error = engine
        .execute(&mut context, &command("list-keys", &["c"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::UnsupportedCommand(message)
            if message == "list-keys c (key filter)"),
        "{error:?}"
    );
    let error = engine
        .execute(&mut context, &command("list-keys", &["-T", "bogus"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::InvalidCommand(message)
            if message == "table bogus doesn't exist"),
        "{error:?}"
    );
    let listed = engine
        .execute(&mut context, &command("list-keys", &["-T", "root"]))
        .unwrap();
    assert!(listed.output.lines().all(|line| line.contains("-T root")));
}

#[test]
fn source_file_keeps_every_path_in_order() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let sourced = engine
        .execute(&mut context, &command("source-file", &["first", "second"]))
        .unwrap();
    assert_eq!(
        sourced.effects,
        [
            MuxEffect::SourceFile {
                path: "first".to_owned(),
                quiet: false,
            },
            MuxEffect::SourceFile {
                path: "second".to_owned(),
                quiet: false,
            },
        ]
    );
    let quiet = engine
        .execute(&mut context, &command("source-file", &["-q", "maybe"]))
        .unwrap();
    assert_eq!(
        quiet.effects,
        [MuxEffect::SourceFile {
            path: "maybe".to_owned(),
            quiet: true,
        }]
    );
    let error = engine
        .execute(&mut context, &command("source-file", &["-v", "loud"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::InvalidCommand(message)
            if message == "source-file does not support -v"),
        "{error:?}"
    );
}

#[test]
fn session_targets_accept_a_unique_prefix() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-session", &["-s", "workshop"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-t", "works"]))
        .unwrap();
    assert_eq!(window_count(&engine, "workshop"), 2);
    assert_eq!(window_count(&engine, "work"), 1);

    engine
        .execute(&mut context, &command("new-window", &["-t", "work"]))
        .unwrap();
    assert_eq!(
        window_count(&engine, "work"),
        2,
        "an exact name still resolves exactly"
    );

    let error = engine
        .execute(&mut context, &command("new-window", &["-t", "wor"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::AmbiguousTarget(message)
            if message == "wor matches work, workshop"),
        "{error:?}"
    );
}

#[test]
fn set_dash_o_keeps_an_already_set_option() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(
            &mut context,
            &command("set-option", &["-g", "prefix", "C-a"]),
        )
        .unwrap();
    let error = engine
        .execute(
            &mut context,
            &command("set-option", &["-go", "prefix", "C-x"]),
        )
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::InvalidCommand(message)
            if message == "option is already set: prefix"),
        "{error:?}"
    );
    engine
        .execute(
            &mut context,
            &command("set-option", &["-goq", "prefix", "C-x"]),
        )
        .unwrap();
    assert_eq!(engine.keys.prefix(), "C-a");
}

#[test]
fn resize_pane_takes_attached_adjustments_and_rejects_unknown_flags() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();

    let before = split_ratio(&engine);
    engine
        .execute(&mut context, &command("resize-pane", &["-L20"]))
        .unwrap();
    assert!(split_ratio(&engine) < before);

    let held = split_ratio(&engine);
    engine
        .execute(&mut context, &command("resize-pane", &[]))
        .unwrap();
    assert_eq!(split_ratio(&engine), held);
    engine
        .execute(&mut context, &command("resize-pane", &["7"]))
        .unwrap();
    assert_eq!(split_ratio(&engine), held);

    let error = engine
        .execute(&mut context, &command("resize-pane", &["-M"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::InvalidCommand(message)
        if message == "resize-pane does not support -M"));
    let error = engine
        .execute(&mut context, &command("resize-pane", &["-R", "10.5"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::InvalidCommand(message)
        if message == "invalid resize adjustment: 10.5"));
}

#[test]
fn send_keys_dash_h_is_bytewise_ascii_like_tmux() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();

    let sent = engine
        .execute(&mut context, &command("send-keys", &["-H", "0x41", "a"]))
        .unwrap();
    assert_eq!(
        sent.effects,
        [MuxEffect::SendKeys {
            pane,
            keys: vec![
                KeyToken::Literal("A".to_owned()),
                KeyToken::Literal("\n".to_owned()),
            ],
        }]
    );

    let error = engine
        .execute(&mut context, &command("send-keys", &["-H", "e9"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::UnsupportedCommand(message)
        if message == "send-keys -H e9 (raw bytes above 7f)"));
    let error = engine
        .execute(&mut context, &command("send-keys", &["-H", "1F600"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::InvalidCommand(message)
        if message == "send-keys -H needs a character code: 1F600"));
}

#[test]
fn send_keys_dash_l_concatenates_arguments_without_separators() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();
    let sent = engine
        .execute(&mut context, &command("send-keys", &["-l", "foo", "bar"]))
        .unwrap();
    assert_eq!(
        sent.effects,
        [MuxEffect::SendKeys {
            pane,
            keys: vec![KeyToken::Literal("foobar".to_owned())],
        }]
    );
}

#[test]
fn copy_mode_combines_dash_u_and_dash_d_like_tmux() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let entered = engine
        .execute(&mut context, &command("copy-mode", &["-du"]))
        .unwrap();
    assert_eq!(entered.effects.len(), 3);
}

#[test]
fn window_steps_error_instead_of_landing_in_place() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    for name in ["next-window", "previous-window"] {
        let error = engine
            .execute(&mut context, &command(name, &[]))
            .unwrap_err();
        let direction = if name == "next-window" {
            "next"
        } else {
            "previous"
        };
        assert!(
            matches!(&error, ServerError::InvalidCommand(message)
                if message == &format!("no {direction} window")),
            "{error:?}"
        );
    }
    let error = engine
        .execute(&mut context, &command("select-window", &["-n"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::InvalidCommand(message)
        if message == "no next window"));
}

#[test]
fn creation_commands_refuse_the_valued_options_they_cannot_honor() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    for (name, args) in [
        ("new-session", &["-x", "80"]),
        ("new-session", &["-e", "FOO=bar"]),
        ("new-window", &["-e", "FOO=bar"]),
        ("new-window", &["-F", "#{window_id}"]),
        ("split-window", &["-e", "FOO=bar"]),
    ] {
        let error = engine
            .execute(&mut context, &command(name, args))
            .unwrap_err();
        assert!(
            matches!(&error, ServerError::UnsupportedCommand(message)
                if message.starts_with(name)),
            "{name} {args:?} produced {error:?}"
        );
    }
    let error = engine
        .execute(&mut context, &command("split-window", &["-Z"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::InvalidCommand(message)
        if message == "split-window does not support -Z"));
    assert_eq!(window_count(&engine, "work"), 1);
}

#[test]
fn set_option_accepts_tmux_boolean_case_and_empty_toggle() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(
            &mut context,
            &command("set-option", &["synchronize-panes", "ON"]),
        )
        .unwrap();
    engine
        .execute(
            &mut context,
            &command("set-option", &["synchronize-panes", "Off"]),
        )
        .unwrap();
    engine
        .execute(
            &mut context,
            &command("set-option", &["synchronize-panes", ""]),
        )
        .unwrap();
}
