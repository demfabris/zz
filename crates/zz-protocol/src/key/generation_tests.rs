use super::*;

fn binding(command: &str) -> Binding {
    Binding {
        commands: vec![CommandInvocation::new(command, [] as [&str; 0])],
        repeat: false,
        note: None,
    }
}

#[test]
fn every_mutation_moves_the_generation_and_no_op_calls_keep_it() {
    let mut tables = KeyTables::empty();
    let mut last = tables.generation();
    let mut moved = |tables: &KeyTables, expected: bool| {
        let current = tables.generation();
        assert_eq!(current != last, expected);
        last = current;
    };
    tables.bind("root", "F1", binding("new-window"));
    moved(&tables, true);
    tables.ensure_table("root");
    moved(&tables, false);
    tables.ensure_table("other");
    moved(&tables, true);
    tables.update_binding_metadata("root", "F1", Some("note".to_owned()), false);
    moved(&tables, true);
    tables.update_binding_metadata("root", "F1", Some("note".to_owned()), false);
    moved(&tables, false);
    tables.update_binding_metadata("root", "F1", None, true);
    moved(&tables, true);
    assert!(!tables.unbind("root", "F2"));
    moved(&tables, false);
    assert!(tables.unbind("root", "F1"));
    moved(&tables, true);
    assert!(tables.remove_table("other"));
    moved(&tables, true);
    assert!(!tables.remove_table("other"));
    moved(&tables, false);
    tables.set_prefix("C-a");
    moved(&tables, true);
    tables.set_prefix("C-a");
    moved(&tables, false);
    tables.set_prefix2(Some("C-x"));
    moved(&tables, true);
    tables.set_prefix2(Some("C-x"));
    moved(&tables, false);
}

#[test]
fn replaced_tables_never_reuse_a_generation() {
    let first = KeyTables::default();
    let copy = first.clone();
    assert_eq!(copy.generation(), first.generation());
    let replacement = KeyTables::default();
    assert_ne!(replacement.generation(), first.generation());
}

#[test]
fn snapshots_carry_canonical_names_resolved_once() {
    let mut tables = KeyTables::empty();
    let mut sourced = CommandInvocation::new("splitw", ["-h"]);
    sourced.source = Some(crate::message::SourceSpan {
        source: "keys.conf".to_owned(),
        line: 1,
        column: 1,
    });
    tables.bind(
        "root",
        "F1",
        Binding {
            commands: vec![sourced, CommandInvocation::new("zz-unknown", ["x"])],
            repeat: true,
            note: Some("split".to_owned()),
        },
    );
    let first = tables.snapshot();
    assert_eq!(first, tables.snapshot());
    let binding = &first[0].bindings[0];
    assert_eq!(binding.commands[0].name, "split-window");
    assert_eq!(binding.commands[0].source, None);
    assert_eq!(binding.commands[1].name, "zz-unknown");
    assert!(binding.repeat);
    assert_eq!(binding.note.as_deref(), Some("split"));
    assert_eq!(
        tables.get("root", "F1").expect("bound").commands[0].name,
        "splitw"
    );
}

#[test]
fn table_generations_move_with_their_own_table_only() {
    let mut tables = KeyTables::empty();
    let of = |tables: &KeyTables| {
        tables
            .table_generations()
            .map(|(name, generation)| (name.to_owned(), generation))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    tables.bind("root", "F1", binding("new-window"));
    tables.bind("prefix", "c", binding("new-window"));
    let first = of(&tables);
    assert_eq!(first.keys().collect::<Vec<_>>(), ["prefix", "root"]);
    let global = tables.generation();
    tables.bind("root", "F1", binding("new-window"));
    assert_eq!(tables.generation(), global);
    assert_eq!(of(&tables), first);
    tables.bind("root", "F2", binding("kill-pane"));
    let second = of(&tables);
    assert_ne!(second["root"], first["root"]);
    assert_eq!(second["prefix"], first["prefix"]);
    assert_eq!(second["root"], tables.generation());
    tables.update_binding_metadata("prefix", "c", Some("window".to_owned()), false);
    let third = of(&tables);
    assert_eq!(third["root"], second["root"]);
    assert_ne!(third["prefix"], second["prefix"]);
    tables.ensure_table("resize");
    assert!(of(&tables).contains_key("resize"));
    assert!(tables.unbind("prefix", "c"));
    assert!(!of(&tables).contains_key("prefix"));
    assert!(tables.remove_table("resize"));
    assert_eq!(of(&tables).keys().collect::<Vec<_>>(), ["root"]);
    let before_prefix2 = of(&tables);
    tables.set_prefix2(Some("C-x"));
    assert_eq!(of(&tables), before_prefix2);
    assert_eq!(
        tables.snapshot_table("root"),
        tables
            .snapshot()
            .into_iter()
            .find(|table| table.name == "root")
    );
    assert_eq!(tables.snapshot_table("prefix"), None);
}

#[test]
fn every_table_of_the_stock_set_has_a_generation() {
    let tables = KeyTables::default();
    assert_eq!(
        tables
            .table_generations()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        tables.table_names().collect::<Vec<_>>()
    );
}
