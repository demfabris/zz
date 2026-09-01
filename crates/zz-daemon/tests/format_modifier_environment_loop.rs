//! The `V` format modifier over the three environment stores.
//!
//! format.c `format_loop_environ` reads the flag word as one exact spelling:
//! nothing or `s` is the session's store, `g` the global one, `c` the selected
//! client's, and anything else leaves no store and therefore no rows. Whatever
//! store it lands on it walks whole, in the name order environ.c keeps, hidden
//! and removed entries included.

#![cfg(all(unix, feature = "daemon"))]

mod clients;

use clients::Clients;
use zz_protocol::CommandInvocation;

const NAMES: &str = "#{V:<#{environ_name}>}";

fn wrap(lines: &[String]) -> String {
    lines.iter().fold(String::new(), |mut joined, line| {
        joined.push('<');
        joined.push_str(line);
        joined.push('>');
        joined
    })
}

/// `show-environment` prints `NAME=value` for a live entry and `-NAME` for a
/// removed one, in the same store order the loop walks.
fn stored(clients: &mut Clients, args: &[&str]) -> Vec<String> {
    clients
        .commands
        .execute(CommandInvocation::new("show-environment", args.to_vec()))
        .expect("show the environment")
        .lines()
        .map(|line| {
            line.strip_prefix('-')
                .map_or_else(
                    || line.split_once('=').expect("a live entry").0,
                    |removed| removed,
                )
                .to_owned()
        })
        .collect()
}

#[test]
fn the_default_and_the_session_flag_walk_the_sessions_own_store() {
    let mut clients = Clients::start("environ-loop-session", &["base"]);
    clients
        .commands
        .execute(CommandInvocation::new(
            "set-environment",
            ["-t", "base", "ZZ_LOOP_ONE", "one"],
        ))
        .expect("set one");
    clients
        .commands
        .execute(CommandInvocation::new(
            "set-environment",
            ["-t", "base", "ZZ_LOOP_TWO", ""],
        ))
        .expect("set an empty value");

    let names = stored(&mut clients, &["-t", "base"]);
    assert!(names.len() > 2, "the session store is seeded: {names:?}");
    assert_eq!(clients.format("base", NAMES), wrap(&names));
    assert_eq!(
        clients.format("base", "#{Vs:<#{environ_name}>}"),
        wrap(&names)
    );

    // Store order is name order, and an empty value is a live entry rather than
    // a removed one.
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert_eq!(
        clients.format(
            "base",
            "#{V:#{?#{==:#{environ_name},ZZ_LOOP_TWO},<#{environ_value}|#{environ_removed}>,}}"
        ),
        "<|0>"
    );
}

#[test]
fn hidden_and_removed_entries_keep_their_rows() {
    let mut clients = Clients::start("environ-loop-flags", &["base"]);
    for args in [
        vec!["-t", "base", "-h", "ZZ_LOOP_HIDDEN", "secret"],
        vec!["-t", "base", "-r", "ZZ_LOOP_REMOVED"],
        vec!["-t", "base", "ZZ_LOOP_PLAIN", "kept"],
    ] {
        clients
            .commands
            .execute(CommandInvocation::new("set-environment", args))
            .expect("set the entry");
    }

    let row = "#{V:#{?#{m:ZZ_LOOP_*,#{environ_name}},\
        <#{environ_name}=#{environ_value}:#{environ_hidden}#{environ_removed}>,}}";
    assert_eq!(
        clients.format("base", row),
        "<ZZ_LOOP_HIDDEN=secret:10><ZZ_LOOP_PLAIN=kept:00><ZZ_LOOP_REMOVED=:01>"
    );
    // show-environment hides the hidden entry; the loop still walks it.
    assert!(!stored(&mut clients, &["-t", "base"]).contains(&"ZZ_LOOP_HIDDEN".to_owned()));
}

#[test]
fn each_flag_word_reads_its_own_store_when_the_names_collide() {
    let mut clients = Clients::start("environ-loop-collide", &["base"]);
    clients.attach_interactive("base");
    for args in [
        vec!["-g", "ZZ_LOOP_COLLIDE", "global"],
        vec!["-t", "base", "ZZ_LOOP_COLLIDE", "session"],
    ] {
        clients
            .commands
            .execute(CommandInvocation::new("set-environment", args))
            .expect("set the entry");
    }

    let pick = |flags: &str| {
        format!(
            "#{{V{flags}:#{{?#{{==:#{{environ_name}},ZZ_LOOP_COLLIDE}},<#{{environ_value}}>,}}}}"
        )
    };
    assert_eq!(clients.format("base", &pick("")), "<session>");
    assert_eq!(clients.format("base", &pick("s")), "<session>");
    assert_eq!(clients.format("base", &pick("g")), "<global>");
    // The client store is the attached client's own process environment, which
    // never learned the name.
    assert_eq!(clients.format("base", &pick("c")), "");

    let path = std::env::var("PATH").expect("the test process has a PATH");
    assert_eq!(
        clients.format(
            "base",
            "#{Vc:#{?#{==:#{environ_name},PATH},<#{environ_value}>,}}"
        ),
        format!("<{path}>")
    );
}

#[test]
fn the_client_store_is_the_invoking_clients_own_environment() {
    let mut clients = Clients::start("environ-loop-client", &["base"]);
    // cmd-display-message.c creates the tree with cmdq_get_client, so the store
    // is the connection that ran the command, not whoever is attached.
    let detached = clients.format("base", "#{Vc:<#{environ_name}>}");
    assert!(!detached.is_empty(), "the command client has an environment");

    let attached = clients.attach_interactive("base");
    assert_eq!(clients.format("base", "#{Vc:<#{environ_name}>}"), detached);
    clients.detach(attached);
    assert_eq!(clients.format("base", "#{Vc:<#{environ_name}>}"), detached);

    // The store is that client's process environment, in name order.
    let path = std::env::var("PATH").expect("the test process has a PATH");
    assert_eq!(
        clients.format(
            "base",
            "#{Vc:#{?#{==:#{environ_name},PATH},<#{environ_value}>,}}"
        ),
        format!("<{path}>")
    );
    let names = detached
        .trim_matches(['<', '>'])
        .split("><")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    // No hidden or removed entries: a client sends only what it has.
    assert_eq!(
        clients.format("base", "#{Vc:#{environ_hidden}#{environ_removed}}"),
        "00".repeat(names.len())
    );
}

#[test]
fn an_unspelled_flag_word_leaves_no_store_and_no_rows() {
    let mut clients = Clients::start("environ-loop-flagword", &["base"]);
    clients.attach_interactive("base");
    clients
        .commands
        .execute(CommandInvocation::new(
            "set-environment",
            ["-g", "ZZ_LOOP_GLOBAL", "g"],
        ))
        .expect("set a global entry");

    // The pin compares the whole word, so no combination and no unknown letter
    // selects a store.
    for flags in ["z", "gs", "sg", "sc", "gc", "S", "G", "C", "gg", "cs", " s"] {
        assert_eq!(
            clients.format("base", &format!("[#{{V{flags}:x}}]")),
            "[]",
            "V{flags}"
        );
    }
    // The three the pin does spell keep their rows.
    for flags in ["", "s", "g", "c"] {
        assert_ne!(
            clients.format("base", &format!("[#{{V{flags}:x}}]")),
            "[]",
            "V{flags}"
        );
    }
}

#[test]
fn an_emptied_session_store_produces_no_rows_while_the_global_one_keeps_its_own() {
    let mut clients = Clients::start("environ-loop-context", &["base"]);
    for name in stored(&mut clients, &["-t", "base"]) {
        clients
            .commands
            .execute(CommandInvocation::new(
                "set-environment",
                ["-t", "base", "-u", name.as_str()],
            ))
            .expect("unset the entry");
    }

    assert_eq!(clients.format("base", "[#{V:x}]"), "[]");
    assert_eq!(clients.format("base", "[#{Vs:x}]"), "[]");
    // The session store is looked up, never fallen back on: the global store
    // still has rows of its own.
    assert_ne!(clients.format("base", "[#{Vg:x}]"), "[]");
}

#[test]
fn nested_and_malformed_environment_loops_keep_the_pinned_fallback() {
    let mut clients = Clients::start("environ-loop-nesting", &["base"]);
    clients
        .commands
        .execute(CommandInvocation::new(
            "set-environment",
            ["-t", "base", "ZZ_LOOP_NEST", "n"],
        ))
        .expect("set the entry");
    let rows = clients.format("base", "#{n:#{V:x}}");

    // A window loop inside an environment row still sees the outer session.
    assert_eq!(
        clients.format("base", "#{n:#{V:#{W:x}}}"),
        rows,
        "one window per row"
    );
    // Environment loops nest, and the inner rows do not disturb the outer ones.
    assert_eq!(
        clients.format("base", "#{n:#{V:#{V:x}}}"),
        (rows.parse::<usize>().expect("a row count").pow(2)).to_string()
    );
    // The inner row's name wins inside the inner loop and the outer name comes
    // back afterwards.
    assert_eq!(
        clients.format(
            "base",
            "#{V:#{?#{==:#{environ_name},ZZ_LOOP_NEST},\
             [#{V:#{?#{==:#{environ_name},ZZ_LOOP_NEST},in,}}#{environ_name}],}}"
        ),
        "[inZZ_LOOP_NEST]"
    );
    // An empty body runs the rows and contributes nothing.
    assert_eq!(clients.format("base", "#{V:}"), "");
}

#[test]
fn the_row_formats_stay_empty_outside_a_loop() {
    let mut clients = Clients::start("environ-loop-leak", &["base"]);
    clients.attach_interactive("base");

    assert_eq!(
        clients.format(
            "base",
            "[#{environ_name}|#{environ_value}|#{environ_hidden}|#{environ_removed}]"
        ),
        "[|||]"
    );
    // And a loop leaves nothing behind for the text after it.
    assert_eq!(
        clients.format("base", "#{V:x}[#{environ_name}#{environ_hidden}]"),
        format!(
            "{}[]",
            "x".repeat(
                clients
                    .format("base", "#{n:#{V:x}}")
                    .parse::<usize>()
                    .expect("a row count")
            )
        )
    );
}
