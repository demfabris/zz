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
    assert!(Arc::ptr_eq(&first, &engine.cached_format_option_snapshot()));
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
