use std::collections::{BTreeMap, BTreeSet};

use zz_mux::{COMMAND_SPECS, ExecutionContext, MuxEffect, MuxEngine};
use zz_protocol::{CommandInvocation, DAEMON_COMMAND_SPECS, ServerError};

fn command(name: &str, args: &[&str]) -> CommandInvocation {
    CommandInvocation::new(name, args.iter().copied())
}

/// Catalogued command names with no entry in the pin's `cmd_table` (cmd.c), so
/// their flag markers are zz surface area and never tmux compatibility debt.
/// Derived against `compat/.cache/tmux-src`, not guessed: the union of both
/// spec tables minus that table is exactly these fourteen. Only `split-picker`
/// carries unsupported markers today; the rest are named so that "exclude the
/// zz-native commands" is a written rule instead of folklore.
const ZZ_NATIVE_COMMANDS: &[&str] = &[
    "copy-mode-search-prompt",
    "focus-sidebar",
    "new-browser",
    "reload-config",
    "restart-agent-pane",
    "select-pane-kind",
    "set-agent-provider",
    "set-agent-session",
    "set-browser-profile",
    "set-browser-tabs",
    "set-browser-url",
    "set-editor-path",
    "split-browser",
    "split-picker",
];

/// Every tmux flag zz catalogues as unsupported, command by command — the
/// roster `knowledge/tmux/divergences.md` publishes as "122 pairs across 29
/// commands".
///
/// The counts alone cannot police this: a wave that implements one flag while
/// another regresses keeps the total. So the literal is the contract and the
/// test cross-checks it both ways against the catalog. What the PIN accepts is
/// not in this tree, so the roster stays hand-maintained; the drift this
/// catches is roster-versus-catalog, which is the drift a wave actually causes.
const UNSUPPORTED_FLAG_LEDGER: &[(&str, &[&str])] = &[
    ("attach-session", &["-E", "-c", "-f", "-x"]),
    ("break-pane", &["-W", "-X", "-Y", "-a", "-b", "-x", "-y"]),
    ("capture-pane", &["-C", "-F", "-H", "-L", "-P", "-R"]),
    ("choose-buffer", &["-F", "-k", "-y"]),
    ("choose-tree", &["-F", "-G", "-h", "-k", "-y"]),
    ("clear-history", &["-H"]),
    (
        "command-prompt",
        &[
            "-1", "-C", "-F", "-N", "-P", "-T", "-e", "-i", "-k", "-l", "-t",
        ],
    ),
    ("copy-mode", &["-S", "-k", "-s"]),
    ("detach-client", &["-E", "-P", "-t"]),
    (
        "display-message",
        &["-C", "-I", "-N", "-a", "-c", "-d", "-l", "-v"],
    ),
    ("display-panes", &["-N", "-t"]),
    ("join-pane", &["-l"]),
    ("kill-pane", &["-f"]),
    ("kill-session", &["-f", "-g"]),
    ("kill-window", &["-f"]),
    ("last-pane", &["-d", "-e"]),
    ("list-keys", &["-1", "-N", "-O", "-P", "-a", "-r"]),
    ("load-buffer", &["-t", "-w"]),
    (
        "move-pane",
        &["-D", "-L", "-M", "-P", "-R", "-U", "-X", "-Y", "-l", "-z"],
    ),
    ("new-session", &["-E", "-X", "-e", "-f", "-t"]),
    ("new-window", &["-E", "-b", "-e"]),
    ("resize-pane", &["-M", "-T"]),
    ("select-pane", &["-M", "-P", "-d", "-e", "-g", "-m"]),
    ("send-keys", &["-F", "-K", "-M", "-R", "-c"]),
    ("set-buffer", &["-n", "-t", "-w"]),
    ("show-messages", &["-J", "-T", "-t"]),
    ("source-file", &["-F", "-n", "-t", "-v"]),
    (
        "split-window",
        &[
            "-E", "-I", "-R", "-S", "-T", "-W", "-Z", "-e", "-k", "-m", "-s",
        ],
    ),
    ("unbind-key", &["-a", "-q"]),
];

const LEDGER_PAIRS: usize = 122;
const LEDGER_COMMANDS: usize = 29;

fn catalogued_specs() -> BTreeMap<&'static str, &'static zz_protocol::CommandSpec> {
    COMMAND_SPECS
        .iter()
        .chain(DAEMON_COMMAND_SPECS.iter())
        .map(|spec| (spec.name, spec))
        .collect()
}

#[test]
fn the_unsupported_flag_ledger_matches_the_catalog() {
    let specs = catalogued_specs();

    for name in ZZ_NATIVE_COMMANDS {
        assert!(
            specs.contains_key(name),
            "the zz-native exclusion list names {name}, which no longer exists"
        );
    }
    let native = ZZ_NATIVE_COMMANDS.iter().copied().collect::<BTreeSet<_>>();

    let mut rostered = BTreeSet::new();
    let mut previous = "";
    for (command, flags) in UNSUPPORTED_FLAG_LEDGER {
        assert!(
            *command > previous,
            "{command} breaks the ledger's sorted order after {previous}"
        );
        previous = command;
        assert!(
            !native.contains(command),
            "{command} is zz-native and must not count against tmux compatibility"
        );
        let spec = specs
            .get(command)
            .unwrap_or_else(|| panic!("the ledger names {command}, which is not catalogued"));

        let mut seen = "";
        for flag in *flags {
            assert!(
                *flag > seen,
                "{command} lists {flag} out of order or twice after {seen}"
            );
            seen = flag;
            let option = spec.option(flag).unwrap_or_else(|| {
                panic!("the ledger says {command} rejects {flag}, but the catalog has no such flag")
            });
            assert!(
                option.unsupported,
                "{command} {flag} is implemented now; drop it from the ledger, \
                 knowledge/tmux/divergences.md, and the campaign plan"
            );
            rostered.insert((*command, *flag));
        }
    }

    let mut catalogued = BTreeSet::new();
    for (name, spec) in &specs {
        if native.contains(name) {
            continue;
        }
        for option in spec.options.iter().filter(|option| option.unsupported) {
            catalogued.insert((*name, option.name));
        }
    }

    let missing = catalogued.difference(&rostered).collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "the catalog rejects flags the ledger never declared: {missing:?}"
    );
    assert_eq!(
        rostered, catalogued,
        "the ledger and the catalog disagree about which flags zz rejects"
    );

    assert_eq!(rostered.len(), LEDGER_PAIRS);
    assert_eq!(UNSUPPORTED_FLAG_LEDGER.len(), LEDGER_COMMANDS);
    assert_eq!(
        UNSUPPORTED_FLAG_LEDGER
            .iter()
            .map(|(_, flags)| flags.len())
            .sum::<usize>(),
        LEDGER_PAIRS
    );
}

fn assert_unknown_flag(command: &str, flag: &str, error: &ServerError) {
    assert_eq!(
        error,
        &ServerError::InvalidCommand(format!("{command} does not support {flag}"))
    );
}

#[test]
fn every_catalog_command_rejects_a_mechanically_absent_flag() {
    assert_eq!(COMMAND_SPECS.len(), 75);
    for spec in COMMAND_SPECS {
        let candidate = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .find(|candidate| {
                let flag = format!("-{candidate}");
                spec.option(&flag).is_none()
            })
            .expect("every command leaves at least one short flag unused");
        let flag = format!("-{candidate}");
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        let error = engine
            .execute(
                &mut context,
                &CommandInvocation::new(spec.name, [flag.clone()]),
            )
            .unwrap_err();
        assert_unknown_flag(spec.name, &flag, &error);
    }
}

#[test]
fn kill_server_rejects_unknown_flag_before_shutdown() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let error = engine
        .execute(&mut context, &command("kill-server", &["-q"]))
        .unwrap_err();
    assert_unknown_flag("kill-server", "-q", &error);
}

#[test]
fn last_window_rejects_unknown_flag_before_switching() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "second"]))
        .unwrap();
    let current = context.clone();

    let error = engine
        .execute(&mut context, &command("last-window", &["-q"]))
        .unwrap_err();

    assert_unknown_flag("last-window", "-q", &error);
    assert_eq!(context, current);
}

#[test]
fn send_prefix_rejects_unknown_flag_before_emitting_input() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();

    let error = engine
        .execute(&mut context, &command("send-prefix", &["-q"]))
        .unwrap_err();

    assert_unknown_flag("send-prefix", "-q", &error);
}

#[test]
fn catalogued_unsupported_value_keeps_the_unsupported_error_shape() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let error = engine
        .execute(&mut context, &command("new-session", &["-e", "FOO=bar"]))
        .unwrap_err();

    assert_eq!(
        error,
        ServerError::UnsupportedCommand("new-session -e".to_owned())
    );
}

#[test]
fn clustered_flags_reject_an_unknown_member() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let error = engine
        .execute(&mut context, &command("new-session", &["-dq"]))
        .unwrap_err();

    assert_unknown_flag("new-session", "-q", &error);
}

#[test]
fn double_dash_allows_a_positional_starting_with_dash() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();

    engine
        .execute(&mut context, &command("rename-session", &["--", "-weird"]))
        .unwrap();

    assert!(
        engine
            .state
            .sessions
            .values()
            .any(|session| session.name == "-weird")
    );
}

#[test]
fn option_value_starting_with_dash_is_not_validated_as_a_flag() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();

    let sent = engine
        .execute(&mut context, &command("send-keys", &["-t", "-1"]))
        .expect("previous pane target");

    assert!(matches!(
        sent.effects.as_slice(),
        [MuxEffect::SendKeys { .. }]
    ));
}
