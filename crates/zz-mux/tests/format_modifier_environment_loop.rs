//! The `V` modifier's store selection, against stores the test controls.
//!
//! Two cases a daemon cannot stage: an expansion with no session at all, and
//! two attached clients whose environments differ, which one host's GUI clients
//! never have. format.c `format_loop_environ` reads `ft->s->environ`,
//! `global_environ`, or `ft->client->environ`, and returns an empty string when
//! the flag word names none of them.

use std::collections::BTreeMap;

use zz_mux::{FormatClientRow, FormatEnvironRow, StatusContext, StatusHooks, expand_status};

fn row(name: &str, value: &str) -> FormatEnvironRow {
    FormatEnvironRow {
        name: name.to_owned(),
        value: value.to_owned(),
        hidden: false,
        removed: false,
    }
}

struct Stores {
    clients: Vec<FormatClientRow>,
    selected: Vec<FormatEnvironRow>,
}

impl StatusHooks for Stores {
    fn strftime(&mut self, literal: &str) -> String {
        literal.to_owned()
    }

    fn shell(&mut self, _command: &str) -> String {
        String::new()
    }

    fn client_loop_rows(&mut self) -> Vec<FormatClientRow> {
        self.clients.clone()
    }

    fn client_environment_rows(&mut self) -> Vec<FormatEnvironRow> {
        self.selected.clone()
    }
}

fn client(name: &str, environment: Vec<FormatEnvironRow>) -> FormatClientRow {
    FormatClientRow {
        name: name.to_owned(),
        activity: 0,
        variables: BTreeMap::from([("client_name".to_owned(), name.to_owned())]),
        environment,
    }
}

fn expand(stores: &mut Stores, format: &str) -> String {
    expand_status(format, &StatusContext::default(), stores)
}

#[test]
fn an_expansion_with_no_session_has_no_session_store_to_walk() {
    let mut stores = Stores {
        clients: Vec::new(),
        selected: vec![row("PATH", "/bin")],
    };
    // The session store is a lookup, not a fallback: without a session there is
    // nothing to walk, while the selected client still answers.
    assert_eq!(expand(&mut stores, "[#{V:x}]"), "[]");
    assert_eq!(expand(&mut stores, "[#{Vs:x}]"), "[]");
    assert_eq!(expand(&mut stores, "[#{Vc:<#{environ_name}>}]"), "[<PATH>]");
}

#[test]
fn a_client_row_selects_that_rows_environment_for_the_client_store() {
    let mut stores = Stores {
        clients: vec![
            client("one", vec![row("WHO", "first"), row("ONLY_ONE", "1")]),
            client("two", vec![row("WHO", "second")]),
        ],
        selected: vec![row("WHO", "outer")],
    };

    assert_eq!(
        expand(
            &mut stores,
            "#{L:<#{client_name}:#{Vc:#{?#{==:#{environ_name},WHO},#{environ_value},}}>}"
        ),
        "<one:first><two:second>"
    );
    // Row cardinality follows the row's own store, and the outer selection comes
    // back after the loop.
    assert_eq!(expand(&mut stores, "#{L:[#{n:#{Vc:x}}]}"), "[2][1]");
    assert_eq!(expand(&mut stores, "#{Vc:<#{environ_value}>}"), "<outer>");
}

#[test]
fn the_row_formats_answer_only_inside_the_loop() {
    let mut stores = Stores {
        clients: Vec::new(),
        selected: vec![
            FormatEnvironRow {
                name: "GONE".to_owned(),
                value: String::new(),
                hidden: false,
                removed: true,
            },
            FormatEnvironRow {
                name: "SECRET".to_owned(),
                value: "s".to_owned(),
                hidden: true,
                removed: false,
            },
        ],
    };

    assert_eq!(
        expand(
            &mut stores,
            "#{Vc:<#{environ_name}=#{environ_value}:#{environ_hidden}#{environ_removed}\
             :#{loop_index}#{loop_last_flag}>}"
        ),
        "<GONE=:01:00><SECRET=s:10:11>"
    );
    assert_eq!(
        expand(
            &mut stores,
            "[#{environ_name}#{environ_value}#{environ_hidden}#{environ_removed}]"
        ),
        "[]"
    );
}
