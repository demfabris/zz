use zz_protocol::{Axis, CommandInvocation, PaneId, SessionId, WindowId};

use crate::{
    ExecutionContext, MuxEngine, PaneKind, PaneRuntimeFacts, StatusContext, StatusHooks,
    expand_status,
    formats::{
        FormatClient, FormatContext, FormatJobTag, FormatNeeds, FormatType, expand_format,
        with_eager_universe,
    },
    tmux_options::STATUS_FORMAT_DEFAULTS,
};

const FORMATS: &[&str] = &[
    "#{session_name}:#{window_index}.#{pane_index} #{pane_flags}#{window_flags}",
    "#{S:#{session_name}=#{session_active}#{session_windows}#{session_alerts}|}",
    "#{S/n:#{session_name},[#{session_name}]}",
    "#{S/t:#{session_name}}#{S/r:#{session_id}}",
    "#{W:#{window_index}#{window_name}#{window_flags}|,<#{window_index}>}",
    "#{W/n:#{window_name}}#{W/r:#{window_id}}",
    "#{W:#{next_window_index}/#{prev_window_index}/#{next_window_active}#{prev_@tag}#{next_@tag}#{window_before_active}#{window_after_active};}",
    "#{P:#{pane_id}#{pane_flags}#{pane_dead}#{pane_marked}|,[#{pane_id}]}",
    "#{P/z:#{pane_id}}#{P/r:#{pane_index}}",
    "#{S:[#{W:<#{P:#{pane_id}#{window_visible_layout}#{history_limit}#{pane_synchronized}>}>]}",
    "#{N:logs}#{N/w:logs}#{N/s:beta}#{N/s:nope}#{N/x:alpha}",
    "#{Og:#{option_name}=#{option_value};}",
    "#{Os:#{option_name}}#{Ow:#{option_name}=#{option_value}}#{Op:#{option_name}}#{Ov:#{option_name}}",
    "#{Ogw:#{option_name}}#{Ogs:#{option_name}}",
    "#{Vg:#{environ_name}=#{environ_value}#{environ_removed};}",
    "#{V:#{environ_name}=#{environ_value};}#{Vs:#{environ_hidden}}",
    "#{FOO}|#{SESSION_ONLY}|#{MASKED}|#{@tag}|#{@global}|#{nothing_here}",
    "#{E:status-left}#{T:window-status-format}#{E:@fmt}#{E:@missing}",
    "#{E:window-status-current-format}#{T;=/5:status-right}",
    "#{?pane_marked,M,-}#{?window_zoomed_flag,Z,-}#{?#{==:#{pane_dead},1},D,-}#{?FOO,f,n}",
    "#{window_layout}#{window_manual_width}#{pane_current_command}#{pane_current_path}",
    "#{S:#{E:@fmt}}#{W:#{T:window-status-format}}",
    "#{m/r:^w.*,#{session_name}}#{s/o/0/:session_name}#{R:#{W:x},2}",
    "#{S:[#{session_name}:#{P:#{pane_index}}]}",
    "#{S:<#{Ow:#{option_name}=#{option_value}}>}",
    "#{S:<#{Op:#{option_name}=#{option_value}}>}",
    "#{W:<#{Op:#{option_name}=#{option_value}}>}",
    "#{S:<#{Os:#{option_name}}#{V:#{environ_name}}>}",
    "#{S:#{P:#{Op:#{option_name}}}}#{W:#{P:#{Ow:#{option_value}}}}",
];

struct OptionHooks<'a>(&'a MuxEngine);

impl StatusHooks for OptionHooks<'_> {
    fn strftime(&mut self, literal: &str) -> String {
        literal.to_owned()
    }

    fn shell(&mut self, _command: &str, _tag: &FormatJobTag) -> String {
        String::new()
    }

    fn variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
        self.0.format_option_value(context, name)
    }
}

fn run(engine: &mut MuxEngine, context: &mut ExecutionContext, command: &str, args: &[&str]) {
    engine
        .execute(context, &CommandInvocation::new(command, args.to_vec()))
        .unwrap_or_else(|error| panic!("{command} {args:?}: {error:?}"));
}

fn configure(engine: &mut MuxEngine) {
    let mut context = ExecutionContext::default();
    for (command, args) in [
        ("set-environment", vec!["-g", "FOO", "global-foo"]),
        ("set-environment", vec!["-g", "MASKED", "global"]),
        ("set-environment", vec!["-gr", "REMOVED"]),
        ("set-option", vec!["-g", "@global", "g"]),
        ("set-option", vec!["-g", "@fmt", "#{S:#{session_name}.}"]),
        (
            "set-option",
            vec!["-g", "status-left", "[#{W:#{window_index}}]"],
        ),
        (
            "set-option",
            vec!["-gw", "window-status-format", "#{window_name}#{P:.}"],
        ),
    ] {
        run(engine, &mut context, command, &args);
    }
}

fn one_pane() -> MuxEngine {
    let mut engine = MuxEngine::default();
    engine.state.create_session("solo").unwrap();
    configure(&mut engine);
    engine
}

fn twenty_panes() -> MuxEngine {
    let mut engine = MuxEngine::default();
    let (session, _, first) = engine.state.create_session("work").unwrap();
    let mut anchors = vec![first];
    for name in ["logs", "build", "edit", "test"] {
        let (_, pane) = engine
            .state
            .create_window(session, Some(name.to_owned()), PaneKind::Terminal)
            .unwrap();
        anchors.push(pane);
    }
    for anchor in anchors {
        let mut pane = anchor;
        for axis in [Axis::Horizontal, Axis::Vertical, Axis::Horizontal] {
            pane = engine
                .state
                .split_pane(pane, axis, PaneKind::Terminal)
                .unwrap();
        }
    }
    configure(&mut engine);
    engine
}

fn several_sessions() -> MuxEngine {
    let mut engine = MuxEngine::default();
    let (alpha, alpha_window, alpha_pane) = engine.state.create_session("alpha").unwrap();
    let split = engine
        .state
        .split_pane(alpha_pane, Axis::Horizontal, PaneKind::Terminal)
        .unwrap();
    let (logs, logs_pane) = engine
        .state
        .create_window(alpha, Some("logs".to_owned()), PaneKind::Terminal)
        .unwrap();
    let dead = engine
        .state
        .split_pane(logs_pane, Axis::Vertical, PaneKind::Terminal)
        .unwrap();
    let (beta, beta_window, beta_pane) = engine.state.create_session("beta").unwrap();
    let beta_split = engine
        .state
        .split_pane(beta_pane, Axis::Vertical, PaneKind::Terminal)
        .unwrap();
    let (_, gamma_window, gamma_pane) = engine.state.create_session("gamma").unwrap();
    configure(&mut engine);
    let mut context = ExecutionContext::new(Some(alpha), Some(alpha_window), Some(split));
    let alpha_target = format!("{alpha_window}");
    let logs_target = format!("{logs}");
    let gamma_target = format!("{gamma_window}");
    let beta_target = format!("{beta}");
    let split_target = format!("{split}");
    let beta_window_target = format!("{beta_window}");
    let beta_pane_target = format!("{beta_pane}");
    let beta_split_target = format!("{beta_split}");
    let logs_pane_target = format!("{logs_pane}");
    let gamma_pane_target = format!("{gamma_pane}");
    for (command, args) in [
        (
            "set-option",
            vec!["-w", "-t", beta_window_target.as_str(), "@ww", "bw"],
        ),
        (
            "set-option",
            vec!["-p", "-t", beta_pane_target.as_str(), "@pp", "b0"],
        ),
        (
            "set-option",
            vec!["-p", "-t", beta_split_target.as_str(), "@pp", "b1"],
        ),
        (
            "set-option",
            vec!["-p", "-t", logs_pane_target.as_str(), "@pp", "l0"],
        ),
        (
            "set-option",
            vec!["-p", "-t", gamma_pane_target.as_str(), "@pp", "g0"],
        ),
        (
            "set-option",
            vec!["-p", "-t", split_target.as_str(), "@pp", "a1"],
        ),
        ("select-pane", vec!["-m", "-t", split_target.as_str()]),
        ("resize-pane", vec!["-Z", "-t", split_target.as_str()]),
        (
            "set-option",
            vec!["-w", "-t", alpha_target.as_str(), "@tag", "a"],
        ),
        (
            "set-option",
            vec!["-w", "-t", logs_target.as_str(), "@tag", "l"],
        ),
        (
            "set-option",
            vec![
                "-w",
                "-t",
                logs_target.as_str(),
                "window-status-format",
                "#{E:@fmt}!",
            ],
        ),
        ("set-option", vec!["-t", beta_target.as_str(), "@tag", "b"]),
        (
            "set-option",
            vec!["-w", "-t", gamma_target.as_str(), "window-size", "manual"],
        ),
        (
            "set-environment",
            vec!["-t", beta_target.as_str(), "SESSION_ONLY", "beta"],
        ),
        (
            "set-environment",
            vec!["-t", beta_target.as_str(), "-u", "MASKED"],
        ),
        (
            "set-environment",
            vec!["-t", beta_target.as_str(), "-h", "HIDDEN", "h"],
        ),
    ] {
        run(&mut engine, &mut context, command, &args);
    }
    engine.mark_pane_dead(dead, Some(3), None).unwrap();
    engine.set_pane_runtime_facts(
        gamma_pane,
        PaneRuntimeFacts {
            current_command: "vim".to_owned(),
            current_path: "/tmp/gamma".to_owned(),
            ..PaneRuntimeFacts::default()
        },
    );
    engine
}

fn fixtures() -> [(&'static str, MuxEngine); 3] {
    [
        ("one pane", one_pane()),
        ("twenty panes", twenty_panes()),
        ("several sessions", several_sessions()),
    ]
}

type Target = (Option<SessionId>, Option<WindowId>, Option<PaneId>);

fn targets(engine: &MuxEngine) -> Vec<Target> {
    let mut targets = vec![(None, None, None)];
    for session in engine.state.sessions.values() {
        targets.push((Some(session.id), None, None));
        for window in &session.windows {
            targets.push((Some(session.id), Some(*window), None));
            for pane in engine.state.windows[window].panes.keys() {
                targets.push((Some(session.id), Some(*window), Some(*pane)));
            }
        }
    }
    targets
}

fn clients(engine: &MuxEngine) -> Vec<FormatClient> {
    let mut clients = vec![FormatClient::NoClient, FormatClient::Unattached];
    clients.extend(
        engine
            .state
            .sessions
            .keys()
            .copied()
            .map(FormatClient::Attached),
    );
    clients
}

#[test]
fn lazy_and_eager_universes_expand_every_template_alike() {
    for (name, engine) in fixtures() {
        for client in clients(&engine) {
            for (session, window, pane) in targets(&engine) {
                let context = FormatContext {
                    session,
                    window,
                    pane,
                    active_session: session,
                    format_client: client,
                    format_type: FormatType::None,
                };
                for format in FORMATS {
                    let lazy = expand_format(format, &engine, context);
                    let eager = with_eager_universe(|| expand_format(format, &engine, context));
                    assert_eq!(
                        lazy, eager,
                        "{name} {client:?} {session:?} {window:?} {pane:?}: {format}"
                    );
                }
            }
        }
    }
}

#[test]
fn detached_contexts_answer_what_the_eager_universe_answers() {
    for (name, engine) in fixtures() {
        for client in clients(&engine) {
            for (session, window, pane) in targets(&engine) {
                for format in FORMATS
                    .iter()
                    .copied()
                    .chain(STATUS_FORMAT_DEFAULTS.iter().copied())
                {
                    let eager = with_eager_universe(|| {
                        let context = engine.format_status_context_with_format_client(
                            session, window, pane, session, client,
                        );
                        expand_status(format, &context, &mut OptionHooks(&engine))
                    });
                    let detached = engine
                        .format_status_context_with_format_client(
                            session, window, pane, session, client,
                        )
                        .detach(engine.format_needs([format]));
                    let lazy = expand_status(format, &detached, &mut OptionHooks(&engine));
                    assert_eq!(
                        lazy, eager,
                        "{name} {client:?} {session:?} {window:?} {pane:?}: {format}"
                    );
                }
            }
        }
    }
}

#[test]
fn list_commands_give_the_same_rows_on_a_lazy_universe() {
    for (name, mut engine) in fixtures() {
        let session = *engine.state.sessions.keys().next().unwrap();
        let loops = "#{session_name}:#{window_index}.#{pane_index}:#{W:#{window_name}}:#{P:#{pane_id}}:#{S:#{session_name}}:#{FOO}:#{E:@fmt}";
        for (command, args) in [
            ("list-panes", vec!["-a", "-F", loops]),
            (
                "list-panes",
                vec!["-s", "-F", loops, "-f", "#{pane_active}"],
            ),
            ("list-panes", vec!["-a", "--json"]),
            ("list-windows", vec!["-a", "-F", loops]),
            ("list-windows", vec!["-a", "--json"]),
            ("list-sessions", vec!["-F", loops]),
            ("list-sessions", vec!["-f", "#{N/s:beta}", "-F", loops]),
            ("list-keys", vec![]),
            (
                "list-keys",
                vec!["-F", "#{key_table} #{W:#{window_index}} #{FOO}"],
            ),
            ("list-commands", vec!["-F", "#{command_list_name}#{S:.}"]),
            ("display-message", vec!["-p", loops]),
            ("display-message", vec!["-pa"]),
        ] {
            let invocation = CommandInvocation::new(command, args.clone());
            let mut context = ExecutionContext::new(Some(session), None, None);
            let lazy = engine.execute(&mut context, &invocation);
            let mut context = ExecutionContext::new(Some(session), None, None);
            let eager = with_eager_universe(|| engine.execute(&mut context, &invocation));
            assert_eq!(lazy, eager, "{name}: {command} {args:?}");
        }
    }
}

#[test]
fn the_template_scan_names_only_the_parts_a_template_reads() {
    let engine = several_sessions();
    let needs = |templates: &[&str]| engine.format_needs(templates.iter().copied());
    assert_eq!(needs(&["#{session_name} #{pane_id}"]), FormatNeeds::NONE);
    assert_eq!(needs(&["#{W:#{window_name}}"]), FormatNeeds::WINDOWS);
    assert_eq!(
        needs(&["#{S:#{W:#{P:x}}}"]),
        FormatNeeds::SESSIONS | FormatNeeds::WINDOWS | FormatNeeds::PANES
    );
    assert_eq!(needs(&["#{FOO}"]), FormatNeeds::ENVIRONMENT);
    assert_eq!(needs(&["#{Og:x}"]), FormatNeeds::OPTIONS);
    assert_eq!(needs(&["#{Vg:x}"]), FormatNeeds::ENVIRONMENT);
    assert_eq!(needs(&["#{N/s:beta}"]), FormatNeeds::SESSIONS);
    assert_eq!(needs(&["#{l:#{S:x}}"]), FormatNeeds::NONE);
    assert_eq!(needs(&["#{E:status-left}"]), FormatNeeds::WINDOWS);
    assert_eq!(
        needs(&["#{T:window-status-format}"]),
        FormatNeeds::PANES | FormatNeeds::SESSIONS | FormatNeeds::ENVIRONMENT
    );
    assert_eq!(needs(&["#{@global}"]), FormatNeeds::ENVIRONMENT);
    assert_eq!(needs(&["#{E:#{FOO}}"]), FormatNeeds::ALL);
    assert_eq!(needs(&["#{E:pane_title}"]), FormatNeeds::ALL);
    assert_eq!(needs(&["#{E:@missing}"]), FormatNeeds::ALL);
    assert_eq!(
        needs(&[STATUS_FORMAT_DEFAULTS[0]]),
        FormatNeeds::WINDOWS
            | FormatNeeds::ENVIRONMENT
            | FormatNeeds::PANES
            | FormatNeeds::SESSIONS
    );
    assert_eq!(
        crate::formats::format_needs_without_engine(["#{E:status-left}"]),
        FormatNeeds::ALL
    );
}
