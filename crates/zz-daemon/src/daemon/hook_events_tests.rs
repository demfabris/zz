use super::tests::{key_table_fixture, output_view_session_fixture, take_reliable_messages};
use super::*;
use zz_protocol::KeyTableSnapshot;

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
    assert!(unread(&["display-message", "-p", "x"]));
    assert!(unread(&["display-message", "-p", "-F", "literal"]));
    assert!(unread(&["display-message", "-lp", "#{client_name}"]));
    assert!(!unread(&["display-message", "-p"]));
    assert!(!unread(&["display-message", "-p", "#{client_name}"]));
    assert!(!unread(&["display-message", "-p", "-F", "#{client_name}"]));
    assert!(!unread(&["display-message", "-p", "%c"]));
    assert!(!unread(&[
        "display-message",
        "-p",
        "-F",
        "literal",
        "-F",
        "#{client_name}",
    ]));
    assert!(unread(&[
        "display-message",
        "-p",
        "-F",
        "#{client_name}",
        "-F",
        "literal",
    ]));
    assert!(!unread(&["display-message", "-ap", "literal"]));
    assert!(!unread(&["display-message", "-alp", "literal"]));
    assert!(!unread(&["display-message", "-Z", "literal"]));
    assert!(!unread(&["list-keys"]));

    let (shared, mut context) = pane_fixture("facts");
    assert_eq!(
        run(&shared, &mut context, &["display-message", "-p", "literal"])
            .expect("literal display")
            .output,
        "literal"
    );
    assert_eq!(
        run(
            &shared,
            &mut context,
            &["display-message", "-lp", "#{client_name}"],
        )
        .expect("unexpanded display")
        .output,
        "#{client_name}"
    );
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

fn subscriber(shared: &Arc<Shared>) -> (ClientId, Arc<OutboundMailbox>, Vec<KeyTableSnapshot>) {
    let mailbox = OutboundMailbox::new();
    let (client, hello) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    (client, mailbox, hello.key_tables)
}

fn key_table_payloads(mailbox: &OutboundMailbox) -> Vec<EventPayload> {
    take_reliable_messages(mailbox)
        .into_iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload:
                    payload @ (EventPayload::KeyTablesChanged { .. }
                    | EventPayload::KeyTablesPatched { .. }),
                ..
            }) => Some(payload),
            _ => None,
        })
        .collect()
}

fn fold_key_tables(view: &mut Vec<KeyTableSnapshot>, payloads: Vec<EventPayload>) {
    for payload in payloads {
        match payload {
            EventPayload::KeyTablesChanged { tables } => *view = tables,
            EventPayload::KeyTablesPatched { tables, removed } => {
                view.retain(|table| !removed.contains(&table.name));
                for table in tables {
                    if let Some(current) =
                        view.iter_mut().find(|current| current.name == table.name)
                    {
                        *current = table;
                    } else {
                        let at = view.partition_point(|current| current.name < table.name);
                        view.insert(at, table);
                    }
                }
            }
            _ => {}
        }
    }
}

fn binds(tables: &[KeyTableSnapshot], table: &str, key: &str) -> bool {
    tables
        .iter()
        .filter(|candidate| candidate.name == table)
        .flat_map(|candidate| candidate.bindings.iter())
        .any(|binding| binding.key == key)
}

#[test]
fn a_revert_after_an_unpublished_change_reaches_the_next_subscriber() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    let (early, first, mut first_view) = subscriber(&shared);
    run(&shared, &mut context, &["new-session", "-d", "-s", "keys"]).expect("session");
    let bind = ["bind-key", "-n", "F7", "display-message", "seed"];
    run(&shared, &mut context, &bind).expect("bind");
    fold_key_tables(&mut first_view, key_table_payloads(&first));
    assert!(
        binds(&first_view, "root", "F7"),
        "the first subscriber saw F7"
    );
    shared.unregister(early);
    run(&shared, &mut context, &["unbind-key", "-n", "F7"]).expect("unbind");
    let (_, second, mut view) = subscriber(&shared);
    assert!(!binds(&view, "root", "F7"), "the hello carries the unbind");
    run(&shared, &mut context, &bind).expect("rebind");
    fold_key_tables(&mut view, key_table_payloads(&second));
    assert!(
        binds(&view, "root", "F7"),
        "the subscriber that joined after the unbind never learned F7 is bound again"
    );
    assert_eq!(view, shared.inner.lock().engine.keys.snapshot());
}

#[test]
fn a_bind_with_a_subscriber_publishes_only_the_table_it_changed() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    run(&shared, &mut context, &["new-session", "-d", "-s", "delta"]).expect("session");
    let (_, mailbox, mut view) = subscriber(&shared);
    run(
        &shared,
        &mut context,
        &["bind-key", "-T", "zzprime", "x", "display-message", "x"],
    )
    .expect("first bind");
    fold_key_tables(&mut view, key_table_payloads(&mailbox));
    assert_eq!(view, shared.inner.lock().engine.keys.snapshot());
    run(
        &shared,
        &mut context,
        &["bind-key", "-T", "zzdelta", "x", "display-message", "x"],
    )
    .expect("bind");
    let payloads = key_table_payloads(&mailbox);
    if *timers::KEY_TABLE_DELTA {
        assert!(
            matches!(
                payloads.as_slice(),
                [EventPayload::KeyTablesPatched { tables, removed }]
                    if removed.is_empty()
                        && tables.len() == 1
                        && tables[0].name == "zzdelta"
            ),
            "{payloads:?}"
        );
    }
    fold_key_tables(&mut view, payloads);
    run(
        &shared,
        &mut context,
        &["bind-key", "-T", "zzdelta", "x", "display-message", "x"],
    )
    .expect("same binding again");
    assert!(key_table_payloads(&mailbox).is_empty());
    run(&shared, &mut context, &["unbind-key", "-T", "zzdelta", "x"]).expect("unbind");
    let payloads = key_table_payloads(&mailbox);
    if *timers::KEY_TABLE_DELTA {
        assert!(
            matches!(
                payloads.as_slice(),
                [EventPayload::KeyTablesPatched { tables, removed }]
                    if tables.is_empty() && removed == &["zzdelta".to_owned()]
            ),
            "{payloads:?}"
        );
    }
    fold_key_tables(&mut view, payloads);
    for command in [
        &["bind-key", "-r", "C-Up", "resize-pane", "-U"][..],
        &["unbind-key", "-a", "-T", "copy-mode"],
        &["set-option", "-g", "prefix", "C-a"],
        &["bind-key", "-N", "note", "c", "new-window"],
        &["unbind-key", "-a"],
        &[
            "bind-key",
            "-T",
            "copy-mode-vi",
            "v",
            "send-keys",
            "-X",
            "begin-selection",
        ],
    ] {
        run(&shared, &mut context, command).unwrap_or_else(|error| panic!("{command:?}: {error}"));
        fold_key_tables(&mut view, key_table_payloads(&mailbox));
        assert_eq!(
            view,
            shared.inner.lock().engine.keys.snapshot(),
            "after {command:?}"
        );
    }
}

fn focused_panes(shared: &Shared) -> BTreeSet<PaneId> {
    shared.inner.lock().pane_focus.iter().copied().collect()
}

fn session_active_pane_set(shared: &Shared, name: &str) -> BTreeSet<PaneId> {
    let inner = shared.inner.lock();
    let state = &inner.engine.state;
    state
        .sessions
        .values()
        .filter(|session| session.name == name)
        .filter_map(|session| state.windows.get(&session.active_window))
        .map(|window| window.active_pane)
        .collect()
}

#[test]
fn focus_follows_active_pane_and_window_moves_under_an_attached_client() {
    let (shared, _, mut context, _, _) = key_table_fixture("focus-oracle");
    let mut focus = focused_panes(&shared);
    for (command, gated) in [
        (&["set-option", "-g", "focus-events", "on"][..], false),
        (&["split-window", "-t", "focus-oracle"], false),
        (&["select-pane", "-t", "focus-oracle:0.0"], false),
        (&["new-window", "-t", "focus-oracle"], false),
        (&["select-window", "-t", "focus-oracle:0"], false),
        (&["kill-pane", "-t", "focus-oracle:0.0"], false),
        (&["set-option", "-g", "focus-events", "off"], false),
        (&["split-window", "-t", "focus-oracle"], true),
        (&["kill-pane", "-t", "focus-oracle:0.1"], false),
    ] {
        run(&shared, &mut context, command).unwrap_or_else(|error| panic!("{command:?}: {error}"));
        let expected = if gated {
            focus.clone()
        } else {
            session_active_pane_set(&shared, "focus-oracle")
        };
        focus = focused_panes(&shared);
        assert_eq!(focus, expected, "after {command:?}");
    }
}

#[test]
fn a_blocking_key_binding_holds_no_change_window() {
    let (shared, client, mut context, pane, _) = key_table_fixture("sleepy");
    run(
        &shared,
        &mut context,
        &["split-window", "-d", "-t", "sleepy"],
    )
    .expect("split");
    run(
        &shared,
        &mut context,
        &["bind-key", "-n", "z", "run-shell", "sleep 1"],
    )
    .expect("bind");
    let pressing = {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            shared
                .input(
                    client,
                    ClientKind::Interactive,
                    &mut ExecutionContext::default(),
                    InputMessage::Key {
                        pane,
                        input: super::tests::test_key(
                            zz_terminal::KeyCode::Character('z'),
                            zz_terminal::Modifiers::default(),
                            Some("z"),
                        ),
                        text_follows: false,
                    },
                )
                .expect("input");
        })
    };
    thread::sleep(Duration::from_millis(200));
    let mut peak = 0;
    let mut commands = 0;
    while !pressing.is_finished() {
        run(
            &shared,
            &mut context,
            &["select-pane", "-t", &format!("sleepy:0.{}", commands % 2)],
        )
        .expect("select-pane");
        commands += 1;
        peak = peak.max(shared.inner.lock().engine.state.journal_len());
    }
    pressing.join().expect("press");
    assert!(
        commands > 10,
        "only {commands} commands ran beside the binding"
    );
    assert!(
        peak <= 4,
        "the journal held {peak} entries across {commands} commands"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_run_shell_job_sleeps_until_its_output_or_its_exit() {
    fn switches() -> u64 {
        fs::read_to_string("/proc/thread-self/status")
            .expect("thread status")
            .lines()
            .find_map(|line| line.strip_prefix("voluntary_ctxt_switches:"))
            .and_then(|value| value.trim().parse().ok())
            .expect("voluntary switches")
    }
    let job = |command: &str| {
        let process = Mutex::new(None);
        let stopping = AtomicBool::new(false);
        let before = switches();
        let started = Instant::now();
        let result = run_shell_job(
            command,
            &std::env::temp_dir(),
            "tmux",
            &[],
            "screen",
            Path::new("/tmp/zz-hooks-no-socket"),
            None,
            None,
            None,
            false,
            &process,
            &stopping,
            false,
            None,
        )
        .expect("shell job");
        (result, started.elapsed(), switches() - before)
    };
    let (result, elapsed, woke) = job("sleep 0.6; echo done");
    assert_eq!(result.output, b"done\n");
    assert!(result.status.success());
    assert!(elapsed >= Duration::from_millis(600), "{elapsed:?}");
    assert!(woke < 10, "the job thread woke {woke} times in {elapsed:?}");
    let (result, elapsed, woke) = job("sleep 3 & sleep 0.4; echo left");
    assert_eq!(result.output, b"left\n");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    assert!(woke < 10, "the job thread woke {woke} times in {elapsed:?}");
}
