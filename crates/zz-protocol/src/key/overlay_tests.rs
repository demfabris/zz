use super::*;

fn overlay(engine: &mut KeyEngine, tables: &KeyTables, key: &str) -> KeyDecision {
    engine
        .handle_overlay_with_repeat_metadata(
            tables,
            key,
            Instant::now(),
            Duration::from_millis(500),
            Duration::ZERO,
            Duration::ZERO,
            "root",
        )
        .0
}

fn copy_mode_engine() -> KeyEngine {
    let mut engine = KeyEngine::default();
    engine.switch_table(Some("copy-mode-vi".to_owned()));
    engine
}

#[test]
fn an_overlay_over_copy_mode_passes_mode_keys_and_still_takes_the_prefix() {
    let tables = KeyTables::default();
    let mut engine = copy_mode_engine();
    assert!(matches!(
        engine.clone().handle(&tables, "j"),
        KeyDecision::Commands(_)
    ));
    assert_eq!(overlay(&mut engine, &tables, "j"), KeyDecision::Pass);
    assert_eq!(overlay(&mut engine, &tables, "1"), KeyDecision::Pass);
    assert_eq!(engine.active_table(), Some("copy-mode-vi"));

    assert_eq!(overlay(&mut engine, &tables, "C-b"), KeyDecision::Prefix);
    assert_eq!(engine.active_table(), Some("prefix"));
    assert_eq!(
        overlay(&mut engine, &tables, "d"),
        KeyDecision::Commands(vec![CommandInvocation::new(
            "detach-client",
            [] as [&str; 0]
        )])
    );
    assert_eq!(engine.active_table(), Some("copy-mode-vi"));
    assert_eq!(overlay(&mut engine, &tables, "j"), KeyDecision::Pass);
}

#[test]
fn an_overlay_over_copy_mode_still_runs_root_bindings() {
    let mut tables = KeyTables::default();
    tables.bind(
        "root",
        "M-x",
        Binding {
            commands: vec![CommandInvocation::new("next-window", [] as [&str; 0])],
            repeat: false,
            note: None,
        },
    );
    let mut engine = copy_mode_engine();
    assert_eq!(
        overlay(&mut engine, &tables, "M-x"),
        KeyDecision::Commands(vec![CommandInvocation::new("next-window", [] as [&str; 0])])
    );
    assert_eq!(engine.active_table(), Some("copy-mode-vi"));
}

#[test]
fn an_overlay_without_a_mode_resolves_like_the_root_table() {
    let tables = KeyTables::default();
    let mut engine = KeyEngine::default();
    assert_eq!(overlay(&mut engine, &tables, "j"), KeyDecision::Pass);
    assert_eq!(overlay(&mut engine, &tables, "C-b"), KeyDecision::Prefix);
    assert_eq!(overlay(&mut engine, &tables, "Z"), KeyDecision::Ignore);
    assert_eq!(engine.active_table(), None);
    assert_eq!(overlay(&mut engine, &tables, "C-b"), KeyDecision::Prefix);
    assert!(matches!(
        overlay(&mut engine, &tables, "n"),
        KeyDecision::Commands(_)
    ));
    assert_eq!(engine.active_table(), None);
}
