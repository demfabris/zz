use super::tests::{key_table_fixture, output_view_session_fixture};
use super::*;

const CLIENT: ClientId = ClientId(7);

fn run(
    shared: &Arc<Shared>,
    context: &mut ExecutionContext,
    args: &[&str],
) -> Result<Execution, DaemonError> {
    shared.execute(
        CLIENT,
        ClientKind::Command,
        context,
        &CommandInvocation::new(args[0], args[1..].iter().copied()),
    )
}

fn messages(shared: &Shared) -> Vec<String> {
    shared
        .inner
        .lock()
        .message_log
        .iter()
        .map(|message| message.text.clone())
        .collect()
}

fn pane_fixture(name: &str) -> (Arc<Shared>, ExecutionContext) {
    let shared = Arc::new(Shared::new(1));
    let (_, pane, _) = output_view_session_fixture(&shared, name, "fixture text");
    let context =
        ExecutionContext::for_pane(&shared.inner.lock().engine.state, pane).expect("pane context");
    (shared, context)
}

#[test]
fn read_only_commands_still_fire_their_after_hooks() {
    let (shared, mut context) = pane_fixture("after-hooks");
    let commands: [&[&str]; 8] = [
        &["list-keys", "-T", "root"],
        &["list-panes"],
        &["list-windows"],
        &["list-sessions"],
        &["show-options", "-g", "status"],
        &["show-environment", "-g"],
        &["display-message", "-p", "x"],
        &["capture-pane", "-p"],
    ];
    for command in commands {
        assert!(
            !zz_protocol::catalog_command_spec(command[0])
                .expect("catalogued command")
                .mutates(&CommandInvocation::new(command[0], command[1..].iter().copied()).args),
            "{command:?} is read-only"
        );
        let hook = format!("after-{}", command[0]);
        run(
            &shared,
            &mut context,
            &["set-hook", "-g", &hook, "display-message 'fired #{hook}'"],
        )
        .expect("set-hook");
        shared.inner.lock().message_log.clear();
        run(&shared, &mut context, command).expect("read-only command");
        assert_eq!(messages(&shared), [format!("fired {hook}")], "{command:?}");
        run(&shared, &mut context, &["set-hook", "-gu", &hook]).expect("unset hook");
    }
}

#[test]
fn read_only_commands_leave_the_structure_and_its_hooks_alone() {
    let (shared, mut context) = pane_fixture("quiet");
    run(&shared, &mut context, &["split-window", "-d"]).expect("split");
    for hook in [
        "window-layout-changed",
        "window-pane-changed",
        "session-window-changed",
        "pane-focus-in",
        "pane-focus-out",
    ] {
        run(
            &shared,
            &mut context,
            &["set-hook", "-g", hook, "display-message 'fired #{hook}'"],
        )
        .expect("set-hook");
    }
    shared.inner.lock().message_log.clear();
    let generation = shared.inner.lock().engine.state.generation();
    for command in [
        &["list-panes", "-a", "-F", "#{pane_id} #{pane_active}"][..],
        &["display-message", "-p", "#{window_layout}"][..],
        &["show-options", "-gv", "status"][..],
        &["has-session"][..],
    ] {
        run(&shared, &mut context, command).expect("read-only command");
    }
    assert_eq!(shared.inner.lock().engine.state.generation(), generation);
    assert!(messages(&shared).is_empty(), "{:?}", messages(&shared));
    run(&shared, &mut context, &["select-layout", "even-vertical"]).expect("layout");
    assert_eq!(messages(&shared), ["fired window-layout-changed"]);
}

#[test]
fn facts_are_withheld_only_from_commands_that_expand_nothing() {
    let unread = |args: &[&str]| {
        hook_events::expands_no_format(
            args[0],
            &CommandInvocation::new(args[0], args[1..].iter().copied()).args,
        )
    };
    assert!(unread(&["set-option", "-g", "@plugin", "value"]));
    assert!(unread(&["set-option", "-g", "status-left", "left"]));
    assert!(unread(&["set-window-option", "-g", "mode-keys", "vi"]));
    assert!(unread(&[
        "bind-key",
        "-T",
        "table",
        "x",
        "display-message",
        "#{pane_id}"
    ]));
    assert!(unread(&["unbind-key", "-T", "table", "x"]));
    assert!(unread(&["show-options", "-gv", "status"]));
    assert!(unread(&["has-session", "-t", "s"]));
    assert!(unread(&["set-option", "-g", "@plugin", "#{session_name}"]));
    assert!(unread(&["set-option", "-g", "status-left", "#[fg=red]x"]));
    assert!(!unread(&[
        "set-option",
        "-gF",
        "@plugin",
        "#{session_name}"
    ]));
    assert!(!unread(&["set-option", "-g", "@#{session_name}", "value"]));
    assert!(!unread(&["set-option", "-g"]));
    assert!(!unread(&["set-option", "-Z", "@x", "value"]));
    assert!(!unread(&["set-option", "-g", "automatic-rename", "on"]));
    assert!(!unread(&["set-option", "-g", "automatic-ren", "on"]));
    assert!(!unread(&["set-window-option", "automatic-rename"]));
    assert!(!unread(&["show-options", "-g", "#{hook}"]));
    assert!(!unread(&["display-message", "-p", "x"]));
    assert!(!unread(&["list-keys"]));

    let (shared, mut context) = pane_fixture("facts");
    run(
        &shared,
        &mut context,
        &["set-option", "-g", "@plain", "value"],
    )
    .expect("set");
    run(
        &shared,
        &mut context,
        &["set-option", "-g", "@literal", "#{session_name}"],
    )
    .expect("set literal");
    run(
        &shared,
        &mut context,
        &[
            "set-option",
            "-gF",
            "@expanded",
            "#{session_name}:#{@plain}",
        ],
    )
    .expect("set -F");
    run(
        &shared,
        &mut context,
        &[
            "bind-key",
            "-T",
            "hooks-table",
            "x",
            "display-message",
            "#{pane_id}",
        ],
    )
    .expect("bind");
    let shown = run(
        &shared,
        &mut context,
        &[
            "display-message",
            "-p",
            "#{@plain}|#{@expanded}|#{@literal}",
        ],
    )
    .expect("display");
    assert_eq!(shown.output, "value|facts:value|#{session_name}");
    let listed = run(&shared, &mut context, &["list-keys", "-T", "hooks-table"]).expect("keys");
    assert!(listed.output.contains("hooks-table x"), "{}", listed.output);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "facts were withheld")]
fn a_format_read_with_withheld_facts_is_caught() {
    let facts = FormatHookFacts::default();
    let mut hooks = DaemonFormatHooks::command(&facts).withhold_facts(true);
    zz_mux::StatusHooks::tree_entries(&mut hooks);
}

const MUX_HOOKS: [&str; 11] = [
    "session-created",
    "session-renamed",
    "session-closed",
    "session-window-changed",
    "window-linked",
    "window-unlinked",
    "window-renamed",
    "window-pane-changed",
    "window-layout-changed",
    "window-resized",
    "pane-title-changed",
];

fn record_mux_hooks(shared: &Arc<Shared>, context: &mut ExecutionContext) {
    for hook in MUX_HOOKS {
        run(
            shared,
            context,
            &[
                "set-hook",
                "-g",
                hook,
                "display-message '#{hook}:#{hook_window}'",
            ],
        )
        .expect("set-hook");
    }
    shared.inner.lock().message_log.clear();
}

#[test]
fn journal_hooks_match_the_snapshot_diff_across_structural_commands() {
    let (shared, mut context) = pane_fixture("journal");
    record_mux_hooks(&shared, &mut context);
    let commands: &[&[&str]] = &[
        &["new-window", "-d", "-n", "second"],
        &["split-window", "-d"],
        &["select-layout", "tiled"],
        &["rename-window", "renamed"],
        &["rename-session", "journal-renamed"],
        &["select-pane", "-t", ":.1"],
        &["resize-pane", "-Z"],
        &["resize-pane", "-Z"],
        &["select-pane", "-T", "titled"],
        &["swap-pane", "-D"],
        &["rotate-window"],
        &["next-window"],
        &["previous-window"],
        &["break-pane", "-d"],
        &["join-pane", "-d", "-s", ":2", "-t", ":0"],
        &["new-session", "-d", "-s", "other"],
        &[
            "link-window",
            "-d",
            "-s",
            "journal-renamed:1",
            "-t",
            "other:5",
        ],
        &["unlink-window", "-t", "other:5"],
        &[
            "move-window",
            "-d",
            "-s",
            "journal-renamed:1",
            "-t",
            "other:7",
        ],
        &["swap-window", "-d", "-s", "other:7", "-t", "other:0"],
        &["kill-pane", "-t", ":.1"],
        &["kill-window", "-t", "other:7"],
        &["kill-session", "-t", "other"],
    ];
    for command in commands {
        let _ = run(&shared, &mut context, command);
    }
    let fired = messages(&shared);
    for hook in [
        "window-linked",
        "window-unlinked",
        "window-renamed",
        "session-renamed",
        "window-layout-changed",
        "window-pane-changed",
        "session-window-changed",
        "pane-title-changed",
        "session-created",
        "session-closed",
    ] {
        assert!(
            fired.iter().any(|message| message.starts_with(hook)),
            "{hook} never fired: {fired:?}"
        );
    }
}

#[test]
fn an_attached_client_is_detached_by_the_journal_when_its_session_goes() {
    let (shared, client, mut context, _, _) = key_table_fixture("detach-journal");
    run(
        &shared,
        &mut context,
        &[
            "set-hook",
            "-g",
            "client-detached",
            "display-message 'detached #{hook}'",
        ],
    )
    .expect("set-hook");
    run(
        &shared,
        &mut context,
        &["new-session", "-d", "-s", "survivor"],
    )
    .expect("survivor");
    shared.inner.lock().message_log.clear();
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("kill-session", ["-t", "detach-journal"]),
        )
        .expect("kill-session");
    assert!(
        messages(&shared)
            .iter()
            .any(|message| message == "detached client-detached"),
        "{:?}",
        messages(&shared)
    );
}
