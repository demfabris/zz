use super::*;

fn set(engine: &mut MuxEngine, context: &mut ExecutionContext, args: &[&str]) {
    engine
        .execute(
            context,
            &CommandInvocation::new("set-option", args.iter().copied()),
        )
        .expect("set option");
}

#[test]
fn status_option_snapshot_reuses_unchanged_options_and_refreshes_scopes() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "cache"]),
        )
        .unwrap();
    let first = engine.cached_format_option_snapshot();
    assert_eq!(
        Arc::ptr_eq(&first, &engine.cached_format_option_snapshot()),
        crate::format_cache_knob()
    );
    set(&mut engine, &mut context, &["status-left", "changed"]);
    let second = engine.cached_format_option_snapshot();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(
        second.lookup(&context.session.unwrap().to_string(), "", "", "status-left"),
        Some("changed".to_owned())
    );
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("split-window", ["-d"]),
        )
        .unwrap();
    let third = engine.cached_format_option_snapshot();
    assert!(!Arc::ptr_eq(&second, &third));
    assert_eq!(third.panes.len(), 2);
}

#[test]
fn format_needs_cache_follows_user_option_indirection() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    set(&mut engine, &mut context, &["-g", "@indirect", "plain"]);
    let templates = ["#{E:@indirect}"];
    let first = engine.cached_format_needs(templates);
    assert_eq!(first, engine.format_needs(templates));
    set(
        &mut engine,
        &mut context,
        &["-g", "@indirect", "#{S:#{session_name}}"],
    );
    let second = engine.cached_format_needs(templates);
    assert_eq!(second, engine.format_needs(templates));
    assert!(second.contains(crate::FormatNeeds::SESSIONS));
    set(&mut engine, &mut context, &["-gu", "@indirect"]);
    assert_eq!(
        engine.cached_format_needs(templates),
        engine.format_needs(templates)
    );
}

#[test]
fn format_needs_cache_follows_status_array_append_and_unset() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    set(
        &mut engine,
        &mut context,
        &["-g", "status-format[0]", "plain"],
    );
    let before = engine.format_options_generation();
    let snapshot = engine.cached_format_option_snapshot();
    set(
        &mut engine,
        &mut context,
        &["-ga", "status-format[0]", "#{W:#{window_name}}"],
    );
    assert_ne!(before, engine.format_options_generation());
    assert!(!Arc::ptr_eq(
        &snapshot,
        &engine.cached_format_option_snapshot()
    ));
    assert_eq!(
        engine.cached_format_needs(["#{E:status-format[0]}"]),
        engine.format_needs(["#{E:status-format[0]}"])
    );
    set(&mut engine, &mut context, &["-gu", "status-format[0]"]);
    assert_eq!(
        engine.cached_format_needs(["#{E:status-format[0]}"]),
        engine.format_needs(["#{E:status-format[0]}"])
    );
}

fn user_hook_mutation_refreshes_format_caches(args: &[&str]) {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "cache"]),
        )
        .unwrap();
    let pane = context.pane.unwrap();
    engine.state.update_pane_title(pane, "cache-title").unwrap();
    set(&mut engine, &mut context, &["-g", "@indirect", "plain"]);
    let templates = ["#{E:@indirect}"];
    let snapshot = engine.cached_format_option_snapshot();
    let before = engine.format_options_generation();
    assert!(
        !engine
            .cached_format_needs(templates)
            .contains(crate::FormatNeeds::SESSIONS)
    );
    let first = crate::with_borrowed_formats(true, || {
        engine
            .format_status_context(context.session, context.window, context.pane)
            .detach_with_templates(engine.cached_format_needs(templates), templates)
    });
    assert_eq!(first.variable("pane_title").as_deref(), Some(""));
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("set-hook", args.iter().copied()),
        )
        .unwrap();
    assert_ne!(before, engine.format_options_generation());
    let refreshed = engine.cached_format_option_snapshot();
    assert!(!Arc::ptr_eq(&snapshot, &refreshed));
    assert_eq!(*refreshed, engine.format_option_snapshot());
    let needs = engine.cached_format_needs(templates);
    assert_eq!(needs, engine.format_needs(templates));
    assert!(needs.contains(crate::FormatNeeds::SESSIONS));
    let detached = crate::with_borrowed_formats(true, || {
        engine
            .format_status_context(context.session, context.window, context.pane)
            .detach_with_templates(needs, templates)
    });
    assert_eq!(
        detached.variable("pane_title").as_deref(),
        Some("cache-title")
    );
}

#[test]
fn user_hook_mutation_invalidates_option_needs_and_reference_caches() {
    user_hook_mutation_refreshes_format_caches(&[
        "-g",
        "@indirect",
        "#{S:#{session_name}}#{pane_title}",
    ]);
}

#[test]
fn monitor_hook_mutation_invalidates_option_needs_and_reference_caches() {
    user_hook_mutation_refreshes_format_caches(&[
        "-g",
        "-B",
        "@indirect:%*:#{pane_title}",
        "#{S:#{session_name}}#{pane_title}",
    ]);
}

#[test]
fn default_setters_invalidate_existing_option_snapshots() {
    let mut engine = MuxEngine::default();
    for (option, value) in [
        ("mode-keys", "vi"),
        ("status-keys", "vi"),
        ("default-shell", "/bin/bash"),
        ("editor", "nvim"),
    ] {
        let snapshot = engine.cached_format_option_snapshot();
        let before = engine.format_options_generation();
        match option {
            "mode-keys" => engine.set_default_mode_keys(value).unwrap(),
            "status-keys" => engine.set_default_status_keys(value).unwrap(),
            "default-shell" => engine.initialize_default_shell(value),
            "editor" => engine.initialize_default_editor(value),
            _ => unreachable!(),
        }
        assert_ne!(before, engine.format_options_generation(), "{option}");
        let refreshed = engine.cached_format_option_snapshot();
        assert!(!Arc::ptr_eq(&snapshot, &refreshed), "{option}");
        assert_eq!(*refreshed, engine.format_option_snapshot(), "{option}");
        assert_eq!(
            engine.global_tmux_option_value(option),
            Some(value.to_owned()),
            "{option}"
        );
    }
}
