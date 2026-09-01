//! The `L` format modifier over the daemon's real client roster.
//!
//! The pin loops `sort_get_clients`: every attached client, none of the ones
//! that never attached, each row built from that client while the outer
//! session, window, and pane stay where they were (format.c
//! `format_loop_clients`, sort.c `sort_client_cmp`). Command clients are
//! connections, not attachments, so they never own a row.

#![cfg(all(unix, feature = "daemon"))]

mod clients;

use clients::Clients;

const ROW: &str = "#{L:<#{client_name}>}";

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

fn wrap(names: &[String]) -> String {
    surround(names, '<', '>')
}

fn surround(names: &[String], open: char, close: char) -> String {
    names.iter().fold(String::new(), |mut joined, name| {
        joined.push(open);
        joined.push_str(name);
        joined.push(close);
        joined
    })
}

#[test]
fn a_server_with_no_attached_client_expands_the_loop_to_nothing() {
    let mut clients = Clients::start("client-loop-empty", &["base"]);

    assert_eq!(clients.format("base", ROW), "");
    assert_eq!(clients.format("base", "[#{L:row}]"), "[]");
    assert_eq!(clients.format("base", "#{L:#{loop_index}}"), "");
}

#[test]
fn every_attached_client_owns_one_row_and_the_command_client_owns_none() {
    let mut clients = Clients::start("client-loop-roster", &["base"]);
    clients.attach_interactive("base");
    clients.attach_control("base");

    let names = clients.client_names();
    assert_eq!(names.len(), 2, "two clients attached: {names:?}");
    assert_eq!(clients.format("base", ROW), wrap(&sorted(names)));
    // The connection running display-message is a command client, so the row
    // count stays at the number of attachments.
    assert_eq!(clients.format("base", "#{L:x}").len(), 2);
}

#[test]
fn a_row_answers_the_client_formats_for_its_own_client() {
    let mut clients = Clients::start("client-loop-identity", &["left", "right"]);
    clients.attach_interactive("left");
    clients.attach_control("right");

    let listed = clients.listed("#{client_name}\u{1}#{client_session}\u{1}#{client_control_mode}");
    let mut expected = listed
        .iter()
        .map(|line| {
            let fields = line.split('\u{1}').collect::<Vec<_>>();
            (
                fields[0].to_owned(),
                format!("<{}|{}|{}>", fields[0], fields[1], fields[2]),
            )
        })
        .collect::<Vec<_>>();
    expected.sort();
    let expected = expected.into_iter().map(|(_, row)| row).collect::<String>();
    assert_eq!(
        clients.format(
            "left",
            "#{L:<#{client_name}|#{client_session}|#{client_control_mode}>}"
        ),
        expected
    );

    // The outer session, window, and pane survive the row's client swap: both
    // rows report the target of the display-message, not their own session.
    assert_eq!(
        clients.format("right", "#{L:<#{session_name}:#{window_index}>}"),
        "<right:0><right:0>"
    );
    // A client format the row does not carry stays empty rather than falling
    // back to the client the format was expanded for.
    assert_eq!(clients.format("left", "#{L:<#{client_termtype}>}"), "<><>");
}

#[test]
fn the_order_flags_and_reversal_follow_the_pinned_comparator() {
    let mut clients = Clients::start("client-loop-order", &["base"]);
    let first = clients.attach_interactive("base");
    clients.attach_control("base");

    let by_name = wrap(&sorted(clients.client_names()));
    let mut reversed_names = sorted(clients.client_names());
    reversed_names.reverse();
    let by_name_reversed = wrap(&reversed_names);

    // Default, i, and n all land on SORT_ORDER or SORT_NAME, which the pin
    // resolves through the same strcmp on the client name.
    for form in [
        "#{L:<#{client_name}>}",
        "#{Li:<#{client_name}>}",
        "#{Ln:<#{client_name}>}",
    ] {
        assert_eq!(clients.format("base", form), by_name, "{form}");
    }
    for form in [
        "#{Lr:<#{client_name}>}",
        "#{Lir:<#{client_name}>}",
        "#{Lnr:<#{client_name}>}",
    ] {
        assert_eq!(clients.format("base", form), by_name_reversed, "{form}");
    }

    // t is activity, newest first: the control client attached last, so it
    // leads until the interactive client sends input.
    let control_first = clients.format("base", "#{Lt:<#{client_name}>}");
    assert_eq!(
        clients.format("base", "#{Ltr:<#{client_name}>}"),
        control_first
            .strip_prefix('<')
            .and_then(|rest| rest.split_once('>'))
            .map(|(head, tail)| format!("{tail}<{head}>"))
            .expect("two rows"),
        "tr negates the finished activity comparison"
    );
    clients.note_activity(first, "base");
    let interactive_first = clients.format("base", "#{Lt:<#{client_name}>}");
    assert_ne!(
        interactive_first, control_first,
        "input moved the interactive client to the front of the activity order"
    );
    assert_eq!(
        interactive_first,
        control_first
            .strip_prefix('<')
            .and_then(|rest| rest.split_once('>'))
            .map(|(head, tail)| format!("{tail}<{head}>"))
            .expect("two rows")
    );
}

#[test]
fn attaching_and_detaching_change_the_roster_the_loop_walks() {
    let mut clients = Clients::start("client-loop-refresh", &["base"]);
    assert_eq!(clients.format("base", "#{L:x}"), "");

    let first = clients.attach_interactive("base");
    assert_eq!(clients.format("base", "#{L:x}"), "x");

    clients.attach_control("base");
    assert_eq!(clients.format("base", "#{L:x}"), "xx");

    clients.detach(first);
    let names = clients.client_names();
    assert_eq!(names.len(), 1);
    assert_eq!(clients.format("base", ROW), wrap(&names));

    clients.attach_interactive("base");
    assert_eq!(clients.format("base", "#{L:x}"), "xx");
}

#[test]
fn nested_and_malformed_client_loops_keep_the_pinned_fallback() {
    let mut clients = Clients::start("client-loop-nesting", &["base"]);
    clients.attach_interactive("base");
    clients.attach_control("base");
    let by_name = wrap(&sorted(clients.client_names()));

    // An order letter the pin does not know falls back to SORT_ORDER without
    // reversing, so it reads exactly like the bare modifier.
    for form in ["#{Lz:<#{client_name}>}", "#{Lqq:<#{client_name}>}"] {
        assert_eq!(clients.format("base", form), by_name, "{form}");
    }

    // A window loop inside a client row keeps the row's client and picks up the
    // outer session's windows.
    assert_eq!(
        clients.format("base", "#{L:#{W:[#{window_index}]}}"),
        "[0][0]"
    );
    let names = sorted(clients.client_names());
    assert_eq!(
        clients.format("base", "#{L:#{W:(#{client_name})}}"),
        surround(&names, '(', ')')
    );

    // Client loops nest: the inner loop walks the same roster for every row of
    // the outer one.
    assert_eq!(clients.format("base", "#{L:[#{L:x}]}"), "[xx][xx]");

    // An empty body still runs the rows and contributes nothing.
    assert_eq!(clients.format("base", "#{L:}"), "");

    // Outside a loop the client formats keep answering for the one client the
    // format was expanded for, never for every row of the roster.
    let outer = clients.format("base", "[#{client_name}]");
    assert!(
        names.iter().any(|name| outer == format!("[{name}]")),
        "{outer} is not one of {names:?}"
    );
    assert_eq!(
        clients.format("base", "#{L:x}[#{client_name}]"),
        format!("xx{outer}")
    );
}
