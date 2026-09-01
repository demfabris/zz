//! The six formats a `V` row carries.
//!
//! format.c `format_loop_environ` adds `environ_name`, `environ_value`,
//! `environ_hidden`, `environ_removed`, `loop_last_flag`, and `loop_index` to
//! each row before it fills the rest from the outer context. A removed entry
//! keeps its name with an empty value, a hidden one keeps its value, and the
//! position counts the store from zero however many entries it holds.

#![cfg(all(unix, feature = "daemon"))]

mod clients;

use clients::Clients;
use zz_protocol::CommandInvocation;

const ROW: &str = "#{V:<#{environ_name}|#{environ_value}|#{environ_hidden}\
                   |#{environ_removed}|#{loop_index}|#{loop_last_flag}>}";

fn start(name: &str) -> Clients {
    let mut clients = Clients::start(name, &["base"]);
    // Start from a store this test owns outright.
    for stored in clients
        .commands
        .execute(CommandInvocation::new("show-environment", ["-t", "base"]))
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
        .collect::<Vec<_>>()
    {
        clients
            .commands
            .execute(CommandInvocation::new(
                "set-environment",
                ["-t", "base", "-u", stored.as_str()],
            ))
            .expect("empty the store");
    }
    clients
}

fn set(clients: &mut Clients, args: &[&str]) {
    clients
        .commands
        .execute(CommandInvocation::new("set-environment", args.to_vec()))
        .expect("set the entry");
}

#[test]
fn each_row_reports_its_own_entry_and_its_position() {
    let mut clients = start("environ-context-rows");
    set(&mut clients, &["-t", "base", "BETA", "two"]);
    set(&mut clients, &["-t", "base", "ALPHA", "one"]);
    set(&mut clients, &["-t", "base", "GAMMA", ""]);

    assert_eq!(
        clients.format("base", ROW),
        "<ALPHA|one|0|0|0|0><BETA|two|0|0|1|0><GAMMA||0|0|2|1>"
    );
}

#[test]
fn hidden_and_removed_entries_report_their_own_flags() {
    let mut clients = start("environ-context-flags");
    set(&mut clients, &["-t", "base", "-h", "HIDDEN", "seen"]);
    set(&mut clients, &["-t", "base", "-r", "REMOVED"]);
    set(&mut clients, &["-t", "base", "EMPTY", ""]);

    // A removed entry keeps its name and loses its value; an empty value is not
    // removed; a hidden entry keeps its value in the loop.
    assert_eq!(
        clients.format("base", ROW),
        "<EMPTY||0|0|0|0><HIDDEN|seen|1|0|1|0><REMOVED||0|1|2|1>"
    );
}

#[test]
fn the_global_and_client_stores_carry_the_same_six_formats() {
    let mut clients = start("environ-context-stores");
    set(&mut clients, &["-g", "ZZ_CTX_GLOBAL", "g"]);
    set(&mut clients, &["-t", "base", "ZZ_CTX_GLOBAL", "s"]);

    let pick = |flags: &str, name: &str| {
        format!(
            "#{{V{flags}:#{{?#{{==:#{{environ_name}},{name}}},\
             <#{{environ_name}}=#{{environ_value}}:#{{environ_hidden}}#{{environ_removed}}>,}}}}"
        )
    };
    assert_eq!(
        clients.format("base", &pick("", "ZZ_CTX_GLOBAL")),
        "<ZZ_CTX_GLOBAL=s:00>"
    );
    assert_eq!(
        clients.format("base", &pick("g", "ZZ_CTX_GLOBAL")),
        "<ZZ_CTX_GLOBAL=g:00>"
    );
    let path = std::env::var("PATH").expect("the test process has a PATH");
    assert_eq!(
        clients.format("base", &pick("c", "PATH")),
        format!("<PATH={path}:00>")
    );

    // The session store here holds one entry, so its position is the only one.
    assert_eq!(
        clients.format("base", "#{V:<#{loop_index}#{loop_last_flag}>}"),
        "<01>"
    );
    // The global store holds more, so its last flag lands elsewhere.
    assert!(
        clients
            .format("base", "#{Vg:#{loop_last_flag}}")
            .ends_with('1')
    );
    assert!(clients.format("base", "#{Vg:#{loop_last_flag}}").len() > 1);
}

#[test]
fn a_nested_loop_owns_the_row_formats_only_while_its_rows_run() {
    let mut clients = start("environ-context-nested");
    set(&mut clients, &["-t", "base", "OUTER", "o"]);
    set(&mut clients, &["-t", "base", "PLAIN", "p"]);

    // The inner loop replaces all six names and the outer row's values come back
    // after it.
    assert_eq!(
        clients.format(
            "base",
            "#{V:[#{environ_name}#{loop_index}#{V:(#{environ_name}#{loop_index})}#{environ_name}#{loop_index}]}"
        ),
        "[OUTER0(OUTER0)(PLAIN1)OUTER0][PLAIN1(OUTER0)(PLAIN1)PLAIN1]"
    );
    // A window loop inside an environment row keeps the outer session's windows
    // and takes over only the position.
    assert_eq!(
        clients.format(
            "base",
            "#{V:[#{environ_name}#{W:<#{loop_index}#{loop_last_flag}>}#{environ_name}]}"
        ),
        "[OUTER<01>OUTER][PLAIN<01>PLAIN]"
    );
    // And an environment loop inside a client row still walks the session store,
    // which attaching re-seeds from the update-environment list.
    clients.attach_interactive("base");
    let names = clients.format("base", "#{V:#{environ_name}}");
    assert!(names.contains("OUTERPLAIN"), "{names}");
    assert_eq!(
        clients.format("base", "#{L:[#{V:#{environ_name}}]}"),
        format!("[{names}]")
    );
}

#[test]
fn none_of_the_six_formats_answer_outside_a_loop() {
    let mut clients = start("environ-context-leak");
    set(&mut clients, &["-t", "base", "ONLY", "o"]);

    assert_eq!(
        clients.format(
            "base",
            "[#{environ_name}|#{environ_value}|#{environ_hidden}|#{environ_removed}\
             |#{loop_index}|#{loop_last_flag}]"
        ),
        "[|||||]"
    );
    assert_eq!(
        clients.format(
            "base",
            "#{V:x}[#{environ_name}#{environ_removed}#{loop_index}]"
        ),
        "x[]"
    );
    // A conditional reads them the same way the plain replacement does.
    assert_eq!(
        clients.format(
            "base",
            "#{?environ_name,set,unset}#{V:#{?environ_name,set,unset}}"
        ),
        "unsetset"
    );
}
