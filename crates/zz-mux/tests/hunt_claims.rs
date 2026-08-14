use zz_mux::{COMMAND_SPECS, ExecutionContext, MuxEffect, MuxEngine, parse_config};
use zz_protocol::{CommandInvocation, LayoutNode, ServerError};

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

fn split_ratio(engine: &MuxEngine) -> f32 {
    let window = engine.state.windows.values().next().expect("window");
    match &window.layout {
        LayoutNode::Split { ratio, .. } => *ratio,
        LayoutNode::Pane(_) => panic!("expected a split"),
    }
}

#[test]
fn catalog_has_fifty_eight_commands_and_kill_has_no_dash_a() {
    assert_eq!(COMMAND_SPECS.len(), 58);
    for name in ["kill-session", "kill-window", "kill-pane"] {
        let spec = COMMAND_SPECS
            .iter()
            .find(|spec| spec.name == name)
            .expect(name);
        assert!(
            spec.options.iter().all(|option| option.name != "-a"),
            "{name} catalog grew a -a option"
        );
    }
    let split = COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == "split-window")
        .expect("split-window");
    assert!(split.options.iter().any(|option| option.name == "-p"));
    assert!(split.options.iter().all(|option| option.name != "-l"));
    let new_session = COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == "new-session")
        .expect("new-session");
    assert!(new_session.options.iter().all(|option| option.name != "-t"));
}

#[test]
fn kill_session_dash_a_kills_the_named_target_not_everyone_else() {
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
    assert_eq!(session_names(&engine), ["other"]);
}

#[test]
fn kill_window_dash_a_kills_only_the_target() {
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
        .execute(
            &mut context,
            &command("kill-window", &["-a", "-t", &first.to_string()]),
        )
        .unwrap();
    assert_eq!(window_count(&engine, "work"), 1);
    assert_eq!(engine.state.windows.values().next().unwrap().name, "second");
}

#[test]
fn kill_pane_dash_a_kills_only_the_target() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let first = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let removed = engine
        .execute(
            &mut context,
            &command("kill-pane", &["-a", "-t", &first.to_string()]),
        )
        .unwrap();
    assert!(matches!(
        removed.effects.first(),
        Some(MuxEffect::PanesRemoved(panes)) if panes == &vec![first]
    ));
    assert_eq!(
        engine
            .state
            .windows
            .values()
            .map(|window| window.panes.len())
            .sum::<usize>(),
        1
    );
}

#[test]
fn bind_clustered_nr_flags_are_treated_as_the_key() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &command("bind-key", &["-nr", "F2", "split-window", "-h"]),
        )
        .unwrap();
    assert!(engine.keys.get("prefix", "-nr").is_some());
    assert!(engine.keys.get("root", "F2").is_none());
    assert!(engine.keys.get("prefix", "F2").is_none());
}

#[test]
fn brace_command_lists_split_on_semicolon() {
    let parsed = parse_config("test.conf", "bind c { new-window ; split-window }");
    assert_eq!(parsed.commands.len(), 2);
    assert_eq!(parsed.commands[0].name, "bind");
    assert_eq!(parsed.commands[0].args, ["c", "{", "new-window"]);
    assert_eq!(parsed.commands[1].name, "split-window");
    assert_eq!(parsed.commands[1].args, ["}"]);
}

#[test]
fn new_window_dash_t_resolves_a_session_name_not_a_window_index() {
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

    let error = engine
        .execute(&mut context, &command("new-window", &["-t", "2"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::MissingTarget(target) if target == "2"));
}

#[test]
fn split_window_dash_l_becomes_the_pane_command() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let created = engine
        .execute(&mut context, &command("split-window", &["-l", "40"]))
        .unwrap();
    assert!(matches!(
        created.effects.first(),
        Some(MuxEffect::PaneCreated {
            command: Some(command),
            ..
        }) if command == "40"
    ));
}

#[test]
fn split_window_dash_p_is_ignored() {
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
    assert!((split_ratio(&engine) - 0.5).abs() < f32::EPSILON);
}

#[test]
fn new_session_dash_t_becomes_the_pane_command() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let created = engine
        .execute(&mut context, &command("new-session", &["-t", "name"]))
        .unwrap();
    assert_eq!(session_names(&engine), ["0"]);
    assert!(matches!(
        created.effects.first(),
        Some(MuxEffect::PaneCreated {
            command: Some(command),
            ..
        }) if command == "name"
    ));
}

#[test]
fn kill_session_positional_name_is_ignored() {
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
    assert_eq!(session_names(&engine), ["other"]);
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
        ratio > 0.5,
        "right-pane -R grew the first child's share: {ratio}"
    );
}
