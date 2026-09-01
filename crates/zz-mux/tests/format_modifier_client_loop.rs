//! The `L` modifier's row order, straight against a roster the test controls.
//!
//! sort.c `sort_client_cmp` only ever compares two things: activity, newest
//! first, and the client name, which both breaks every tie and stands in for
//! the orders the client comparator does not implement. `r` negates the
//! finished comparison, so it reverses the tie-break too. A daemon carrying two
//! GUI clients on one host cannot hand those cases distinct names, so the
//! roster here is synthetic.

use std::collections::BTreeMap;

use zz_mux::{FormatClientRow, StatusContext, StatusHooks, expand_status};

struct Roster {
    rows: Vec<FormatClientRow>,
}

impl Roster {
    /// `(name, activity, session)` triples, deliberately handed to the engine
    /// out of order so nothing passes by accident.
    fn new(rows: &[(&str, u64, &str)]) -> Self {
        Self {
            rows: rows
                .iter()
                .map(|(name, activity, session)| FormatClientRow {
                    name: (*name).to_owned(),
                    activity: *activity,
                    variables: BTreeMap::from([
                        ("client_name".to_owned(), (*name).to_owned()),
                        ("client_session".to_owned(), (*session).to_owned()),
                        ("client_activity".to_owned(), activity.to_string()),
                    ]),
                })
                .collect(),
        }
    }
}

impl StatusHooks for Roster {
    fn strftime(&mut self, literal: &str) -> String {
        literal.to_owned()
    }

    fn shell(&mut self, _command: &str) -> String {
        String::new()
    }

    fn variable(&mut self, name: &str, _context: &StatusContext) -> Option<String> {
        // Outside a row the client formats answer for one outer client, the way
        // a daemon expansion does.
        (name == "client_name").then(|| "outer".to_owned())
    }

    fn client_loop_rows(&mut self) -> Vec<FormatClientRow> {
        self.rows.clone()
    }
}

fn expand(rows: &[(&str, u64, &str)], format: &str) -> String {
    let mut hooks = Roster::new(rows);
    expand_status(format, &StatusContext::default(), &mut hooks)
}

const THREE: &[(&str, u64, &str)] = &[("beta", 2, "s2"), ("alpha", 9, "s1"), ("gamma", 5, "s3")];

#[test]
fn the_default_and_the_index_and_name_flags_all_order_by_client_name() {
    for form in ["#{L:", "#{Li:", "#{Ln:"] {
        assert_eq!(
            expand(THREE, &format!("{form}<#{{client_name}}>}}")),
            "<alpha><beta><gamma>",
            "{form}"
        );
    }
}

#[test]
fn the_time_flag_orders_by_activity_newest_first() {
    assert_eq!(
        expand(THREE, "#{Lt:<#{client_name}>}"),
        "<alpha><gamma><beta>"
    );
}

#[test]
fn reversal_negates_the_finished_comparison_including_the_name_tie_break() {
    assert_eq!(
        expand(THREE, "#{Lr:<#{client_name}>}"),
        "<gamma><beta><alpha>"
    );
    assert_eq!(
        expand(THREE, "#{Lnr:<#{client_name}>}"),
        "<gamma><beta><alpha>"
    );
    assert_eq!(
        expand(THREE, "#{Ltr:<#{client_name}>}"),
        "<beta><gamma><alpha>"
    );

    // Equal activity leaves only the name comparison, and r negates that too.
    let tied: &[(&str, u64, &str)] = &[("b", 4, "s"), ("a", 4, "s"), ("c", 4, "s")];
    assert_eq!(expand(tied, "#{Lt:#{client_name}}"), "abc");
    assert_eq!(expand(tied, "#{Ltr:#{client_name}}"), "cba");
}

#[test]
fn an_unknown_order_letter_falls_back_to_the_name_order_without_reversing() {
    for form in ["#{Lz:", "#{L:", "#{Lqq:"] {
        assert_eq!(
            expand(THREE, &format!("{form}#{{client_name}}}}")),
            "alphabetagamma",
            "{form}"
        );
    }
    // r is read from the same argument as the order letter, so it still applies
    // when the order letter itself is unknown.
    assert_eq!(expand(THREE, "#{Lzr:#{client_name}}"), "gammabetaalpha");
}

#[test]
fn a_row_answers_only_for_its_own_client_and_leaks_nothing_outside_the_loop() {
    assert_eq!(
        expand(THREE, "#{L:<#{client_name}:#{client_session}>}"),
        "<alpha:s1><beta:s2><gamma:s3>"
    );
    // A client format the row does not carry is empty inside the loop rather
    // than the outer client's value.
    assert_eq!(expand(THREE, "#{L:<#{client_tty}>}"), "<><><>");
    // And the outer client is untouched on either side of the loop.
    assert_eq!(
        expand(THREE, "[#{client_name}]#{L:x}[#{client_name}]"),
        "[outer]xxx[outer]"
    );
}

#[test]
fn an_empty_roster_expands_the_loop_to_nothing() {
    assert_eq!(expand(&[], "[#{L:<#{client_name}>}]"), "[]");
    assert_eq!(expand(&[], "#{L:}"), "");
}

#[test]
fn client_loops_nest_and_each_level_keeps_its_own_row() {
    assert_eq!(
        expand(THREE, "#{L:[#{client_name}#{L:(#{client_name})}]}"),
        concat!(
            "[alpha(alpha)(beta)(gamma)]",
            "[beta(alpha)(beta)(gamma)]",
            "[gamma(alpha)(beta)(gamma)]"
        )
    );
}
