use zz_mux::{
    CommandPromptTemplate, CommandSpec, ExecutionContext, MuxEffect, MuxEngine, parse_config,
};
use zz_protocol::ServerError;

#[test]
fn existing_tmux_config_applies_supported_subset_and_skips_the_rest() {
    let input = include_str!("fixtures/tmux.conf");
    let parsed = parse_config("fixtures/tmux.conf", input);
    assert!(parsed.diagnostics.is_empty());

    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let mut unsupported = Vec::new();
    for command in parsed.commands {
        if CommandSpec::DAEMON_COMMAND_NAMES.contains(&command.name.as_str()) {
            continue;
        }
        match engine.execute(&mut context, &command) {
            Ok(_) => {}
            Err(ServerError::UnsupportedCommand(command)) => unsupported.push(command),
            Err(error) => panic!("unexpected config error: {error}"),
        }
    }

    assert_eq!(engine.keys.prefix(), "C-a");
    assert!(engine.keys.get("root", "F2").is_some());
    assert!(engine.keys.get("prefix", "c").is_some());
    assert_eq!(
        engine.keys.get("prefix", "R").unwrap().commands,
        [zz_protocol::CommandInvocation::new(
            "command-prompt",
            ["-I", "#S", "rename-session -- '%%'"],
        )]
    );
    assert_eq!(
        engine.keys.get("prefix", "W").unwrap().commands,
        [zz_protocol::CommandInvocation::new(
            "command-prompt",
            ["-I", "#W", "rename-window -- '%%'"],
        )]
    );
    assert!(unsupported.is_empty());
    let status = engine.status_formats();
    assert!(!status.enabled);
    assert_eq!(status.interval, std::time::Duration::from_secs(5));
    assert_eq!(status.left, "[#S] ");
    assert_eq!(status.right, "#(battery) %H:%M");

    engine
        .execute(
            &mut context,
            &zz_protocol::CommandInvocation::new("new-session", [] as [&str; 0]),
        )
        .expect("session with configured options");
    for (key, input, template) in [
        ("R", "0", "rename-session -- '%%'"),
        ("W", "0", "rename-window -- '%%'"),
    ] {
        let command = engine.keys.get("prefix", key).unwrap().commands[0].clone();
        let execution = engine
            .execute(&mut context, &command)
            .expect("configured rename prompt");
        assert_eq!(
            execution.effects,
            [MuxEffect::CommandPrompt {
                prompt: ":".to_owned(),
                input: input.to_owned(),
                template: Some(CommandPromptTemplate::String(template.to_owned())),
                prompt_type: zz_protocol::CommandPromptType::Command,
                mode: zz_protocol::CommandPromptMode::Text,
                no_freeze: false,
            }]
        );
    }
    assert_eq!(
        engine
            .history_limit_for_pane(context.pane.expect("configured pane"))
            .expect("live pane"),
        1234
    );
    assert_eq!(
        engine
            .word_separators_for_pane(context.pane.expect("configured pane"))
            .expect("live pane"),
        ""
    );
    assert_eq!(
        engine
            .copy_mode_table_for_pane(context.pane.expect("configured pane"))
            .expect("live pane"),
        "copy-mode-vi"
    );
}
