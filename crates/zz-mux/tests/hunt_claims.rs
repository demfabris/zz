use std::path::PathBuf;

use zz_mux::{
    COMMAND_SPECS, DetachRequest, DetachScope, ExecutionContext, MuxEffect, MuxEngine, TmuxSort,
    TmuxSortOrder, parse_config,
};
use zz_protocol::{
    Axis, ChooseTreeKind, CommandInvocation, KeyToken, LayoutNode, PaneId, ServerError,
};
use zz_terminal::{CopyModeAction, CopySelectionMode, TerminalViewAction};

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

fn pane_size(engine: &MuxEngine, pane: PaneId) -> (u16, u16) {
    engine.pane_geometry(pane).expect("pane geometry")
}

#[test]
fn attaching_client_flag_values_reach_the_daemon_effect_without_mux_interpretation() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &command("new-session", &["-d", "-s", "flags"]),
        )
        .unwrap();
    let session = context.session.unwrap();

    let execution = engine
        .execute(
            &mut context,
            &command(
                "attach-session",
                &["-t", "flags", "-f", "ignore-size,!active-pane"],
            ),
        )
        .unwrap();
    assert_eq!(
        execution.effects,
        [MuxEffect::Attach {
            session,
            detach_others: false,
            read_only: false,
            flags: Some("ignore-size,!active-pane".to_owned()),
            update_environment: true,
        }]
    );
}

#[test]
fn catalog_covers_the_options_the_handlers_read() {
    assert_eq!(COMMAND_SPECS.len(), 76);
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
    for flag in ["-d", "-e", "-H", "-M", "-q"] {
        assert!(
            spec("copy-mode")
                .option(flag)
                .is_some_and(|option| !option.unsupported),
            "copy-mode catalog is missing supported {flag}"
        );
    }
    for name in ["choose-tree", "choose-buffer"] {
        assert!(
            spec(name)
                .option("-N")
                .is_some_and(|option| !option.unsupported && option.value.is_none()),
            "{name} catalogs -N as a bare flag so a repeat can be counted"
        );
        assert!(
            spec(name)
                .option("-K")
                .is_some_and(|option| !option.unsupported && option.value.is_some()),
            "{name} catalogs -K so its key format cannot leak into the template"
        );
    }
    assert!(spec("detach-client").option("-a").is_some());
    assert!(spec("detach-client").option("-s").unwrap().value.is_some());
    for flag in ["-D", "-L", "-R", "-U"] {
        let option = spec("resize-pane").option(flag).expect(flag);
        assert!(
            option.optional_value,
            "resize-pane {flag} is optional-valued"
        );
        assert!(
            option.attached_value,
            "resize-pane {flag} accepts attached values"
        );
        assert!(
            option.value.is_none(),
            "resize-pane {flag} is not required-valued"
        );
    }
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
    for flag in ["-F", "-n", "-q", "-t", "-v"] {
        assert!(spec("source-file").option(flag).is_some());
    }
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
fn removing_the_active_window_clears_the_replacement_bell() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let replacement_pane = context.pane.unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "active"]))
        .unwrap();
    assert!(engine.state.set_pane_bell(replacement_pane, true));

    engine
        .execute(&mut context, &command("kill-window", &[]))
        .unwrap();
    let replacement_window = context.window.unwrap();
    assert_eq!(
        engine.state.windows[&replacement_window].active_pane,
        replacement_pane
    );
    assert!(!engine.state.windows[&replacement_window].panes[&replacement_pane].bell);
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
fn bind_key_validates_payloads_before_storing_them() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let original_x = engine.keys.get("prefix", "x").cloned();

    let error = engine
        .execute(&mut context, &command("bind-key", &["x", "not-a-command"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(message)
        if message == "unknown command: not-a-command"));
    assert_eq!(engine.keys.get("prefix", "x"), original_x.as_ref());

    let error = engine
        .execute(
            &mut context,
            &command("bind-key", &["x", "split-window", "-Q"]),
        )
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(message)
        if message == "command split-window: unknown flag -Q"));
    assert_eq!(engine.keys.get("prefix", "x"), original_x.as_ref());

    let error = engine
        .execute(&mut context, &command("bind-key", &["y", "new-pane", "-h"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::UnsupportedCommand(message)
        if message == "bind-key new-pane"));

    engine
        .execute(
            &mut context,
            &command(
                "bind-key",
                &["-T", "copy-mode-vi", "5", "copy-mode-repeat", "5"],
            ),
        )
        .unwrap();
    assert!(engine.keys.get("copy-mode-vi", "5").is_some());

    engine
        .execute(&mut context, &command("bind-key", &["r", "run-shell", "x"]))
        .unwrap();
    assert_eq!(
        engine.keys.get("prefix", "r").unwrap().commands,
        [zz_protocol::CommandInvocation::new("run-shell", ["x"])]
    );

    engine
        .execute(
            &mut context,
            &command("bind-key", &["-n", "Any", "focus-sidebar"]),
        )
        .unwrap();
    assert!(engine.keys.get("root", "Any").is_some());
    engine
        .execute(
            &mut context,
            &command(
                "bind-key",
                &[
                    "-T",
                    "copy-mode-vi",
                    "v",
                    "send-keys",
                    "-X",
                    "begin-selection",
                ],
            ),
        )
        .unwrap();
    assert_eq!(
        engine
            .keys
            .get("copy-mode-vi", "v")
            .expect("copy-mode binding")
            .commands,
        [CommandInvocation::new(
            "send-keys",
            ["-X", "begin-selection"]
        )]
    );
}

#[test]
fn bind_key_accepts_open_quotes_and_empty_endings() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();

    engine
        .execute(
            &mut context,
            &CommandInvocation::new("bind-key", ["x", "{ send-keys 'unterminated }"])
                .with_command_blocks([1]),
        )
        .expect("open quote at block EOF");
    assert_eq!(
        engine
            .keys
            .get("prefix", "x")
            .expect("open quote binding")
            .commands,
        [CommandInvocation::new("send-keys", ["unterminated "])]
    );
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("bind-key", ["x", "{}"]).with_command_blocks([1]),
        )
        .expect("empty block");
    assert!(
        engine
            .keys
            .get("prefix", "x")
            .expect("empty binding")
            .commands
            .is_empty()
    );
    engine
        .execute(
            &mut context,
            &command("bind-key", &["y", "new-window", ";"]),
        )
        .expect("trailing separator");
    assert_eq!(
        engine
            .keys
            .get("prefix", "y")
            .expect("trailing binding")
            .commands,
        [CommandInvocation::new("new-window", [] as [&str; 0])]
    );
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
            .any(|line| line == "bind-key  -T root F2 send-keys \"a ; b\" \\; new-window"),
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
    assert!(matches!(error, ServerError::InvalidCommand(message)
        if message == "create window failed: index 3 in use"));

    engine
        .execute(&mut context, &command("new-window", &["-t", "work"]))
        .unwrap();
    assert_eq!(window_indexes(&engine, "work"), [0, 1, 2, 3]);

    let error = engine
        .execute(&mut context, &command("new-window", &["-t", "work:nope"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::WindowNotFound(target) if target == "nope"));
}

#[test]
fn split_window_dash_l_sizes_the_new_pane_in_cells() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let target = context.pane.unwrap();
    let created = engine
        .execute(&mut context, &command("split-window", &["-h", "-l", "20"]))
        .unwrap();
    assert!(matches!(
        created.effects.first(),
        Some(MuxEffect::PaneCreated { command: None, .. })
    ));
    let created = context.pane.unwrap();
    assert_eq!(pane_size(&engine, target), (59, 24));
    assert_eq!(pane_size(&engine, created), (20, 24));
}

#[test]
fn split_window_dash_l_takes_a_percentage_and_headless_cell_count() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let target = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h", "-l", "25%"]))
        .unwrap();
    let created = context.pane.unwrap();
    assert_eq!(pane_size(&engine, target), (59, 24));
    assert_eq!(pane_size(&engine, created), (20, 24));

    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let target = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h", "-l", "10"]))
        .unwrap();
    let created = context.pane.unwrap();
    assert_eq!(pane_size(&engine, target), (69, 24));
    assert_eq!(pane_size(&engine, created), (10, 24));
}

#[test]
fn split_window_dash_p_gives_the_new_pane_that_share() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let target = context.pane.unwrap();
    let created = engine
        .execute(&mut context, &command("split-window", &["-p", "25"]))
        .unwrap();
    assert!(matches!(
        created.effects.first(),
        Some(MuxEffect::PaneCreated { command: None, .. })
    ));
    let created = context.pane.unwrap();
    assert_eq!(pane_size(&engine, target), (80, 17));
    assert_eq!(pane_size(&engine, created), (80, 6));
}

#[test]
fn split_window_accepts_extreme_percentages_and_reports_no_space_exactly() {
    for (percentage, expected) in [("0", [(78, 24), (1, 24)]), ("100", [(1, 24), (78, 24)])] {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let target = context.pane.unwrap();
        engine
            .execute(
                &mut context,
                &command("split-window", &["-h", "-p", percentage]),
            )
            .unwrap();
        let created = context.pane.unwrap();
        assert_eq!(
            [pane_size(&engine, target), pane_size(&engine, created)],
            expected
        );
    }

    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-l", "1"]))
        .unwrap();
    let narrow = context.pane.unwrap();
    let error = engine
        .execute(&mut context, &command("split-window", &[]))
        .unwrap_err();
    assert_eq!(
        error,
        ServerError::InvalidCommand("no space for a new pane".to_owned())
    );
    assert_eq!(pane_size(&engine, narrow), (80, 1));
    assert_eq!(engine.state.windows.values().next().unwrap().panes.len(), 2);
}

#[test]
fn split_picker_dash_p_gives_the_new_pane_that_share() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let target = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-picker", &["-h", "-p", "25"]))
        .unwrap();
    let created = context.pane.unwrap();
    assert_eq!(pane_size(&engine, target), (59, 24));
    assert_eq!(pane_size(&engine, created), (20, 24));
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
    let window = engine.state.windows.values().next().expect("window");
    assert_eq!(pane_size(&engine, created), (20, 24));
    assert_eq!(pane_size(&engine, target), (59, 24));
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
    let first = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let second = context.pane.unwrap();
    engine
        .execute(
            &mut context,
            &command("split-window", &["-f", "-v", "-p", "25"]),
        )
        .unwrap();
    let created = context.pane.unwrap();
    assert_eq!(pane_size(&engine, first), (40, 17));
    assert_eq!(pane_size(&engine, second), (39, 17));
    assert_eq!(pane_size(&engine, created), (80, 6));
    let window = engine.state.windows.values().next().unwrap();
    assert_eq!(window.pane_order(), [first, second, created]);
    assert_eq!(
        window.pane_order().iter().position(|pane| *pane == created),
        Some(2)
    );
    assert!(matches!(
        window.layout.project(),
        LayoutNode::Split {
            axis: Axis::Vertical,
            first: top,
            second: bottom,
            ..
        } if top.contains(first)
            && top.contains(second)
            && bottom.as_ref() == &LayoutNode::Pane(created)
    ));
}

#[test]
fn split_window_dash_b_f_puts_the_new_pane_at_index_zero() {
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
        .execute(&mut context, &command("split-window", &["-b", "-f", "-v"]))
        .unwrap();
    let created = context.pane.unwrap();
    let window = engine.state.windows.values().next().unwrap();

    assert_eq!(window.pane_order(), [created, first, second]);
    assert_eq!(
        window.pane_order().iter().position(|pane| *pane == created),
        Some(0)
    );
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
        assert_eq!(
            error,
            ServerError::CommandParse(format!(
                "command {name}: too many arguments (need at most 0)"
            ))
        );
    }
    assert_eq!(session_names(&engine), ["keep", "other"]);

    engine
        .execute(&mut context, &command("kill-session", &["-t", "other"]))
        .unwrap();
    assert_eq!(session_names(&engine), ["keep"]);
}

#[test]
fn select_window_refuses_positional_targets_like_tmux() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let selected = context.window;
    let error = engine
        .execute(&mut context, &command("select-window", &["0"]))
        .unwrap_err();
    assert_eq!(
        error,
        ServerError::CommandParse(
            "command select-window: too many arguments (need at most 0)".to_owned()
        )
    );
    assert_eq!(context.window, selected);
}

#[test]
fn split_picker_rejects_positional_arguments() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane_count = engine.state.windows.values().next().unwrap().panes.len();
    let error = engine
        .execute(
            &mut context,
            &command("split-picker", &["printf", "not-a-shell-command"]),
        )
        .unwrap_err();
    assert!(matches!(error, ServerError::CommandParse(message) if message.contains("positional")));
    assert_eq!(
        engine.state.windows.values().next().unwrap().panes.len(),
        pane_count
    );
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
            &command("kill-session", &["-f", "1", "-t", "work"]),
        )
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(ref message)
        if message == "-f only valid with -a"));
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
    let right = context.pane.unwrap();
    engine
        .execute(
            &mut context,
            &command("select-pane", &["-t", &left.to_string()]),
        )
        .unwrap();
    engine.set_pane_geometry(left, 100, 50);
    engine
        .execute(
            &mut context,
            &command("select-pane", &["-t", &right.to_string()]),
        )
        .unwrap();
    engine
        .execute(&mut context, &command("resize-pane", &["-R", "10"]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (110, 50));
    assert_eq!(pane_size(&engine, right), (89, 50));
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
    let right = context.pane.unwrap();
    engine
        .execute(&mut context, &command("select-pane", &["-L"]))
        .unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let middle = context.pane.unwrap();
    engine
        .execute(
            &mut context,
            &command("select-pane", &["-t", &left.to_string()]),
        )
        .unwrap();
    engine.set_pane_geometry(left, 25, 50);
    engine
        .execute(
            &mut context,
            &command("select-pane", &["-t", &middle.to_string()]),
        )
        .unwrap();
    engine
        .execute(&mut context, &command("resize-pane", &["-R", "10"]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (27, 50));
    assert_eq!(pane_size(&engine, middle), (36, 50));
    assert_eq!(pane_size(&engine, right), (35, 50));
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
    let right = context.pane.unwrap();
    engine
        .execute(
            &mut context,
            &command("select-pane", &["-t", &left.to_string()]),
        )
        .unwrap();
    engine.set_pane_geometry(left, 50, 50);
    engine
        .execute(
            &mut context,
            &command("select-pane", &["-t", &right.to_string()]),
        )
        .unwrap();
    engine
        .execute(&mut context, &command("resize-pane", &["-x", "25"]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (74, 50));
    assert_eq!(pane_size(&engine, right), (25, 50));

    engine
        .execute(&mut context, &command("select-pane", &["-L"]))
        .unwrap();
    engine
        .execute(&mut context, &command("resize-pane", &["-x", "25"]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (25, 50));
    assert_eq!(pane_size(&engine, right), (74, 50));
}

#[test]
fn resize_pane_dash_x_uses_headless_cells_and_percentages() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let left = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let right = context.pane.unwrap();
    engine
        .execute(&mut context, &command("resize-pane", &["-x", "30"]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (49, 24));
    assert_eq!(pane_size(&engine, right), (30, 24));

    engine
        .execute(&mut context, &command("resize-pane", &["-x", "25%"]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (59, 24));
    assert_eq!(pane_size(&engine, right), (20, 24));
}

#[test]
fn resize_pane_accepts_percentages_above_one_hundred_and_clamps_layout() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let top = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &[]))
        .unwrap();
    let bottom = context.pane.unwrap();

    engine
        .execute(&mut context, &command("resize-pane", &["-y", "125%"]))
        .expect("oversized percentage");
    assert_eq!(pane_size(&engine, top), (80, 1));
    assert_eq!(pane_size(&engine, bottom), (80, 22));

    let window = context.window.unwrap();
    let layout = engine.state.windows[&window].layout.clone();
    let generation = engine.state.generation();
    let error = engine
        .execute(&mut context, &command("resize-pane", &["-y", "1001%"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(message) if message == "height too large"));
    assert_eq!(engine.state.windows[&window].layout, layout);
    assert_eq!(engine.state.generation(), generation);
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
        matches!(error, ServerError::InvalidCommand(message) if message == "adjustment invalid")
    );
    let window = engine.state.windows.values().next().expect("window");
    let panes = window.pane_order();
    assert_eq!(pane_size(&engine, panes[0]), (40, 24));
    assert_eq!(pane_size(&engine, panes[1]), (39, 24));
}

#[test]
fn resize_pane_validates_every_used_argument_before_mutating() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let left = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let right = context.pane.unwrap();
    let window = context.window.unwrap();

    let generation = engine.state.generation();
    let resized = engine
        .execute(
            &mut context,
            &command("resize-pane", &["-t", "work:0.0", "-x", "20", "bogus"]),
        )
        .unwrap();
    assert_eq!(pane_size(&engine, left), (20, 24));
    assert_eq!(pane_size(&engine, right), (59, 24));
    assert!(engine.state.generation() > generation);
    assert!(resized.effects.contains(&MuxEffect::SnapshotChanged));

    let layout = engine.state.windows[&window].layout.clone();
    let generation = engine.state.generation();
    let error = engine
        .execute(
            &mut context,
            &command(
                "resize-pane",
                &["-t", "work:0.0", "-x", "30", "-R", "bogus"],
            ),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        ServerError::InvalidCommand(message) if message == "adjustment invalid"
    ));
    assert_eq!(engine.state.windows[&window].layout, layout);
    assert_eq!(engine.state.generation(), generation);
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
fn send_keys_dash_n_carries_one_key_list_and_a_repeat_count() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let sent = engine
        .execute(&mut context, &command("send-keys", &["-N", "3", "x"]))
        .unwrap();
    assert!(
        matches!(
            sent.effects.first(),
            Some(MuxEffect::SendKeys {
                keys,
                repeat: 3,
                ..
            }) if keys == &[KeyToken::Literal("x".to_owned())]
        ),
        "{:?}",
        sent.effects
    );

    let error = engine
        .execute(&mut context, &command("send-keys", &["-N", "0", "x"]))
        .unwrap_err();
    assert!(matches!(error, ServerError::InvalidCommand(message)
            if message == "repeat count too small"));
}

#[test]
fn send_keys_dash_n_without_keys_arms_the_copy_mode_repeat() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();
    let armed = engine
        .execute(&mut context, &command("send-keys", &["-N", "5"]))
        .unwrap();
    assert_eq!(
        armed.effects,
        [MuxEffect::CopyModeRepeat { pane, count: 5 }]
    );
}

#[test]
fn send_keys_dash_n_carries_movement_count_but_runs_copy_once() {
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
    assert!(matches!(
        moved.effects.as_slice(),
        [MuxEffect::TerminalView {
            action: zz_terminal::TerminalViewAction::CopyModeCounted {
                action: zz_terminal::CopyModeAction::Up,
                count: 4,
            },
            ..
        }]
    ));
    let copied = engine
        .execute(
            &mut context,
            &command("send-keys", &["-X", "-N", "4", "copy-selection"]),
        )
        .unwrap();
    assert!(matches!(
        copied.effects.as_slice(),
        [MuxEffect::TerminalView {
            action: zz_terminal::TerminalViewAction::CopyMode(
                zz_terminal::CopyModeAction::CopySelection(_)
            ),
            ..
        }]
    ));
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
    let plain = engine
        .execute(&mut context, &command("send-keys", &["x"]))
        .unwrap();
    let formatted = engine
        .execute(&mut context, &command("send-keys", &["-F", "x"]))
        .unwrap();
    assert_eq!(formatted, plain);

    for flag in ["-R", "-M", "-K"] {
        let error = engine
            .execute(&mut context, &command("send-keys", &[flag, "x"]))
            .unwrap_err();
        assert!(
            matches!(&error, ServerError::UnsupportedCommand(message)
                if message == &format!("send-keys {flag}")),
            "{error:?}"
        );
    }
}

#[test]
fn copy_mode_stock_flags_preserve_tmux_branch_order_and_scroll_exit() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();
    let paged = engine
        .execute(&mut context, &command("copy-mode", &["-d"]))
        .unwrap();
    assert_eq!(
        paged.effects,
        [
            MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::EnterCopyMode,
            },
            MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(CopyModeAction::PageDown),
            },
        ]
    );
    let scroll_exit = engine
        .execute(&mut context, &command("copy-mode", &["-e"]))
        .unwrap();
    assert_eq!(
        scroll_exit.effects,
        [MuxEffect::TerminalView {
            pane,
            action: TerminalViewAction::EnterCopyModeScrollExit,
        }]
    );
    let immediate_scroll_exit = engine
        .execute(&mut context, &command("copy-mode", &["-ed"]))
        .unwrap();
    assert_eq!(
        immediate_scroll_exit.effects,
        [
            MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::EnterCopyModeScrollExit,
            },
            MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(CopyModeAction::PageDownScrollExit),
            },
        ]
    );

    let quit = engine
        .execute(&mut context, &command("copy-mode", &["-q"]))
        .unwrap();
    let quit_with_dead_flag = engine
        .execute(&mut context, &command("copy-mode", &["-qu"]))
        .unwrap();
    let cancel = [MuxEffect::TerminalView {
        pane,
        action: TerminalViewAction::CopyMode(CopyModeAction::Cancel),
    }];
    assert_eq!(quit.effects, cancel);
    assert_eq!(quit_with_dead_flag.effects, cancel);

    let mouse = engine
        .execute(&mut context, &command("copy-mode", &["-M"]))
        .unwrap();
    assert!(mouse.effects.is_empty());
    assert!(mouse.output.is_empty());
    let quit_mouse = engine
        .execute(&mut context, &command("copy-mode", &["-qM"]))
        .unwrap();
    assert_eq!(
        quit_mouse.effects,
        [MuxEffect::TerminalView {
            pane,
            action: TerminalViewAction::CopyMode(CopyModeAction::Cancel),
        }]
    );
}

#[test]
fn copy_mode_composes_hide_position_with_scroll_exit() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();
    let enter = |engine: &mut MuxEngine, context: &mut ExecutionContext, flags: &[&str]| {
        engine
            .execute(context, &command("copy-mode", flags))
            .unwrap()
            .effects
    };

    assert_eq!(
        enter(&mut engine, &mut context, &[]),
        [MuxEffect::TerminalView {
            pane,
            action: TerminalViewAction::EnterCopyMode,
        }]
    );
    assert_eq!(
        enter(&mut engine, &mut context, &["-e"]),
        [MuxEffect::TerminalView {
            pane,
            action: TerminalViewAction::EnterCopyModeScrollExit,
        }]
    );
    assert_eq!(
        enter(&mut engine, &mut context, &["-H"]),
        [MuxEffect::TerminalView {
            pane,
            action: TerminalViewAction::EnterCopyModeWith {
                scroll_exit: false,
                hide_position: true,
            },
        }]
    );
    for flags in [&["-eH"][..], &["-He"][..], &["-e", "-H"][..]] {
        assert_eq!(
            enter(&mut engine, &mut context, flags),
            [MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::EnterCopyModeWith {
                    scroll_exit: true,
                    hide_position: true,
                },
            }],
            "copy-mode {flags:?} composes both flags"
        );
    }

    assert_eq!(
        enter(&mut engine, &mut context, &["-Hd"]),
        [
            MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::EnterCopyModeWith {
                    scroll_exit: false,
                    hide_position: true,
                },
            },
            MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(CopyModeAction::PageDown),
            },
        ]
    );
    assert_eq!(
        enter(&mut engine, &mut context, &["-qH"]),
        [MuxEffect::TerminalView {
            pane,
            action: TerminalViewAction::CopyMode(CopyModeAction::Cancel),
        }]
    );
}

#[test]
fn choosers_take_a_key_format_and_refuse_the_large_preview() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();

    let tree = engine
        .execute(
            &mut context,
            &command("choose-tree", &["-N", "-K", "#{line}"]),
        )
        .unwrap();
    assert_eq!(
        tree.effects,
        [MuxEffect::ChooseTree {
            pane,
            kind: ChooseTreeKind::Panes,
            sessions_only: false,
            filter: None,
            sort: TmuxSort::parse(None, false, Some(TmuxSortOrder::Index)).unwrap(),
            key_format: Some("#{line}".to_owned()),
            template: None,
        }]
    );

    let buffer = engine
        .execute(&mut context, &command("choose-buffer", &["-N", "-K", "x"]))
        .unwrap();
    assert_eq!(
        buffer.effects,
        [MuxEffect::ChooseBuffer {
            pane,
            filter: None,
            sort: TmuxSort::parse(None, false, Some(TmuxSortOrder::Creation)).unwrap(),
            key_format: Some("x".to_owned()),
            template: None,
        }]
    );

    for (name, flags) in [
        ("choose-tree", &["-NN"][..]),
        ("choose-tree", &["-N", "-N"][..]),
        ("choose-buffer", &["-NN"][..]),
        ("choose-buffer", &["-N", "-N"][..]),
    ] {
        assert_eq!(
            engine
                .execute(&mut context, &command(name, flags))
                .unwrap_err(),
            ServerError::UnsupportedCommand(format!("{name} -NN")),
            "{name} {flags:?} must ledger the pin's large preview"
        );
    }
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
    assert_eq!(
        mine.effects,
        [MuxEffect::Detach(DetachRequest {
            target_client: None,
            scope: DetachScope::Client,
        })]
    );

    let others = engine
        .execute(&mut context, &command("detach-client", &["-a"]))
        .unwrap();
    assert_eq!(
        others.effects,
        [MuxEffect::Detach(DetachRequest {
            target_client: None,
            scope: DetachScope::Others,
        })]
    );

    let by_session = engine
        .execute(
            &mut context,
            &command("detach-client", &["-a", "-s", "work", "-t", "target:"]),
        )
        .unwrap();
    assert_eq!(
        by_session.effects,
        [MuxEffect::Detach(DetachRequest {
            target_client: Some("target:".to_owned()),
            scope: DetachScope::Session("work".to_owned()),
        })]
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
    let first_belled = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let second_belled = context.pane.unwrap();
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
    assert!(engine.state.set_pane_bell(first_belled, true));
    assert!(engine.state.set_pane_bell(second_belled, true));
    engine
        .execute(
            &mut context,
            &command("select-window", &["-t", "work:third"]),
        )
        .unwrap();
    assert!(
        [first_belled, second_belled]
            .into_iter()
            .all(|pane| !engine.state.windows[&context.window.unwrap()].panes[&pane].bell)
    );

    engine
        .execute(
            &mut context,
            &command("select-window", &["-t", "work:first"]),
        )
        .unwrap();
    assert!(engine.state.set_pane_bell(first_belled, true));
    assert!(engine.state.set_pane_bell(second_belled, true));
    engine
        .execute(&mut context, &command("next-window", &["-a"]))
        .unwrap();
    assert_eq!(active_window_name(&engine, "work"), "third");
    assert!(
        [first_belled, second_belled]
            .into_iter()
            .all(|pane| !engine.state.windows[&context.window.unwrap()].panes[&pane].bell)
    );
    let error = engine
        .execute(&mut context, &command("next-window", &["-a"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::InvalidCommand(message)
        if message == "no next window"));
}

#[test]
fn attach_session_reports_the_remaining_client_flag_it_cannot_honor() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let error = engine
        .execute(
            &mut context,
            &command("attach-session", &["-x", "-t", "work"]),
        )
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::UnsupportedCommand(message)
            if message == "attach-session -x"),
        "{error:?}"
    );
    engine
        .execute(
            &mut context,
            &command("attach-session", &["-dE", "-t", "work"]),
        )
        .unwrap();
}

#[test]
fn list_keys_remaining_selectors_share_the_catalog_and_runtime_contract() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let error = engine
        .execute(&mut context, &command("list-keys", &["-n"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::CommandParse(message)
            if message == "command list-keys: unknown flag -n"),
        "{error:?}"
    );
    engine
        .execute(
            &mut context,
            &command("bind-key", &["-T", "zzlk", "a", "display-message", "a"]),
        )
        .unwrap();
    engine
        .execute(
            &mut context,
            &command(
                "bind-key",
                &["-r", "-T", "zzlk", "x", "display-message", "x"],
            ),
        )
        .unwrap();
    let base = engine
        .execute(
            &mut context,
            &command(
                "list-keys",
                &["-T", "zzlk", "-F", "#{key_string}:#{key_repeat}"],
            ),
        )
        .unwrap()
        .output;
    assert_eq!(base, "a:0\nx:1");
    assert_eq!(
        engine
            .execute(
                &mut context,
                &command(
                    "list-keys",
                    &["-r", "-T", "zzlk", "-F", "#{key_string}:#{key_repeat}"],
                ),
            )
            .unwrap()
            .output,
        base
    );
    assert_eq!(
        engine
            .execute(
                &mut context,
                &command(
                    "list-keys",
                    &["-O", "key", "-r", "-T", "zzlk", "-F", "#{key_string}",],
                ),
            )
            .unwrap()
            .output,
        "x\na"
    );
    assert_eq!(
        engine
            .execute(
                &mut context,
                &command("list-keys", &["-T", "zzlk", "-F", "#{key_string}", "a"]),
            )
            .unwrap()
            .output,
        "a"
    );
    assert!(matches!(
        engine
            .execute(
                &mut context,
                &command("list-keys", &["-1", "-T", "zzlk", "-F", "#{key_string}"]),
            )
            .unwrap()
            .effects
            .as_slice(),
        [MuxEffect::PrintOrMessage { text, freeze: true, .. }] if text == "a"
    ));
    let error = engine
        .execute(&mut context, &command("list-keys", &["-T", "bogus"]))
        .unwrap_err();
    assert!(
        matches!(&error, ServerError::InvalidCommand(message)
            if message == "table bogus doesn't exist"),
        "{error:?}"
    );
}

#[test]
fn source_file_keeps_every_path_in_order() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let sourced = engine
        .execute(&mut context, &command("source-file", &["first", "second"]))
        .unwrap();
    assert_eq!(
        sourced.effects,
        [
            MuxEffect::SourceFile {
                path: "first".to_owned(),
                quiet: false,
                parse_only: false,
                verbose: false,
                context: context.clone(),
            },
            MuxEffect::SourceFile {
                path: "second".to_owned(),
                quiet: false,
                parse_only: false,
                verbose: false,
                context: context.clone(),
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
            parse_only: false,
            verbose: false,
            context: context.clone(),
        }]
    );
    let formatted = engine
        .execute(
            &mut context,
            &command(
                "source-file",
                &[
                    "-F",
                    "#{session_name}-#{window_index}-#{pane_index}-first.conf",
                    "#{session_name}-#{window_index}-#{pane_index}-second.conf",
                ],
            ),
        )
        .unwrap();
    assert_eq!(
        formatted.effects,
        [
            MuxEffect::SourceFile {
                path: "work-0-0-first.conf".to_owned(),
                quiet: false,
                parse_only: false,
                verbose: false,
                context: context.clone(),
            },
            MuxEffect::SourceFile {
                path: "work-0-0-second.conf".to_owned(),
                quiet: false,
                parse_only: false,
                verbose: false,
                context: context.clone(),
            },
        ]
    );
    let stdin = engine
        .execute(&mut context, &command("source-file", &["-"]))
        .unwrap();
    assert_eq!(
        stdin.effects,
        [MuxEffect::SourceFile {
            path: "-".to_owned(),
            quiet: false,
            parse_only: false,
            verbose: false,
            context: context.clone(),
        }]
    );
    let flags = engine
        .execute(
            &mut context,
            &command("source-file", &["-nqv", "flags.conf"]),
        )
        .unwrap();
    assert_eq!(
        flags.effects,
        [MuxEffect::SourceFile {
            path: "flags.conf".to_owned(),
            quiet: true,
            parse_only: true,
            verbose: true,
            context: context.clone(),
        }]
    );

    let work_pane = context.pane.unwrap();
    engine
        .execute(
            &mut context,
            &command("new-session", &["-d", "-s", "other"]),
        )
        .unwrap();
    context.set_client_working_directory(Some(PathBuf::from("/tmp/source caller")));
    let targeted = engine
        .execute(
            &mut context,
            &command(
                "source-file",
                &[
                    "-Fv",
                    "-t",
                    "work:0",
                    "#{session_name}-#{window_index}-#{pane_index}.conf",
                ],
            ),
        )
        .unwrap();
    let mut target_context = context.clone();
    target_context.retarget(&ExecutionContext::for_pane(&engine.state, work_pane).unwrap());
    assert_eq!(
        targeted.effects,
        [MuxEffect::SourceFile {
            path: "work-0-0.conf".to_owned(),
            quiet: false,
            parse_only: false,
            verbose: true,
            context: target_context,
        }]
    );

    let targetless = engine
        .execute(
            &mut context,
            &command("source-file", &["-t", "missing:0", "still-load.conf"]),
        )
        .unwrap();
    let [
        MuxEffect::SourceFile {
            path,
            context: targetless_context,
            ..
        },
    ] = targetless.effects.as_slice()
    else {
        panic!("expected one source-file effect");
    };
    assert_eq!(path, "still-load.conf");
    assert_eq!(
        (
            targetless_context.session,
            targetless_context.window,
            targetless_context.pane,
        ),
        (None, None, None)
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
        matches!(&error, ServerError::WindowNotFound(target) if target == "wor"),
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
            if message == "already set: prefix"),
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
    let left = context.pane.unwrap();
    engine
        .execute(&mut context, &command("split-window", &["-h"]))
        .unwrap();
    let right = context.pane.unwrap();

    engine
        .execute(&mut context, &command("resize-pane", &["-L20"]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (20, 24));
    assert_eq!(pane_size(&engine, right), (59, 24));

    engine
        .execute(&mut context, &command("resize-pane", &[]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (20, 24));
    assert_eq!(pane_size(&engine, right), (59, 24));
    engine
        .execute(&mut context, &command("resize-pane", &["7"]))
        .unwrap();
    assert_eq!(pane_size(&engine, left), (20, 24));
    assert_eq!(pane_size(&engine, right), (59, 24));

    let error = engine
        .execute(&mut context, &command("resize-pane", &["-M"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::UnsupportedCommand(message)
        if message == "resize-pane -M"));
    let error = engine
        .execute(&mut context, &command("resize-pane", &["-R", "10.5"]))
        .unwrap_err();
    assert!(matches!(&error, ServerError::InvalidCommand(message)
        if message == "adjustment invalid"));
}

#[test]
fn resize_pane_direction_flags_accept_bare_attached_and_separated_amounts() {
    let run = |flag: &str, amount: Option<&str>, attached: bool| {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let split = if matches!(flag, "-L" | "-R") {
            ["-h"].as_slice()
        } else {
            [].as_slice()
        };
        engine
            .execute(&mut context, &command("split-window", split))
            .unwrap();
        let window = context.window.unwrap();
        let panes = engine.state.windows[&window].pane_order().to_vec();
        let before = panes
            .iter()
            .map(|pane| pane_size(&engine, *pane))
            .collect::<Vec<_>>();
        let args = match (amount, attached) {
            (None, _) => vec![flag.to_owned()],
            (Some(amount), true) => vec![format!("{flag}{amount}")],
            (Some(amount), false) => vec![flag.to_owned(), amount.to_owned()],
        };
        engine
            .execute(&mut context, &CommandInvocation::new("resize-pane", args))
            .unwrap();
        let after = panes
            .iter()
            .map(|pane| pane_size(&engine, *pane))
            .collect::<Vec<_>>();
        assert_ne!(after, before, "resize-pane {flag} must change the layout");
        after
    };

    for flag in ["-D", "-L", "-R", "-U"] {
        assert_eq!(run(flag, None, false), run(flag, Some("1"), true));
        assert_eq!(run(flag, Some("2"), true), run(flag, Some("2"), false));
    }
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
            repeat: 1,
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
            repeat: 1,
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

#[test]
fn selection_mode_takes_the_pins_names_abbreviations_and_silent_no_op() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();
    for (argument, unit) in [
        (None, CopySelectionMode::Char),
        (Some("char"), CopySelectionMode::Char),
        (Some("C"), CopySelectionMode::Char),
        (Some("word"), CopySelectionMode::Word),
        (Some("W"), CopySelectionMode::Word),
        (Some("LINE"), CopySelectionMode::Line),
        (Some("l"), CopySelectionMode::Line),
    ] {
        let mut arguments = vec!["-X", "selection-mode"];
        arguments.extend(argument);
        let execution = engine
            .execute(&mut context, &command("send-keys", &arguments))
            .unwrap();
        assert_eq!(
            execution.effects,
            [MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(CopyModeAction::SelectionMode(unit)),
            }],
            "{argument:?}"
        );
    }
    for arguments in [
        &["-X", "selection-mode", "sentence"][..],
        &["-X", "selection-mode", "word", "line"][..],
    ] {
        let execution = engine
            .execute(&mut context, &command("send-keys", arguments))
            .unwrap();
        assert_eq!(
            execution.effects,
            [MuxEffect::CopyModeRepeat { pane, count: 1 }],
            "{arguments:?}"
        );
    }
}

#[test]
fn stop_selection_is_typed_apart_from_clear_selection() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();
    for (name, action) in [
        ("clear-selection", CopyModeAction::ClearSelection),
        ("stop-selection", CopyModeAction::StopSelection),
    ] {
        let execution = engine
            .execute(&mut context, &command("send-keys", &["-X", name]))
            .unwrap();
        assert_eq!(
            execution.effects,
            [MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(action),
            }],
            "{name}"
        );
    }
}
