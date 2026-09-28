use super::tests::{
    input_test_key, key_table_events, key_table_fixture, run_test_command, table_active,
    take_reliable_messages, test_key,
};
use super::*;
use zz_terminal::{KeyCode, Modifiers};

fn events(mailbox: &OutboundMailbox) -> Vec<EventPayload> {
    take_reliable_messages(mailbox)
        .into_iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event { payload, .. }) => Some(payload),
            _ => None,
        })
        .collect()
}

fn key_table_publications(events: &[EventPayload]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, EventPayload::KeyTablesChanged { .. }))
        .count()
}

fn snapshots(events: Vec<EventPayload>) -> Vec<MuxSnapshot> {
    events
        .into_iter()
        .filter_map(|event| match event {
            EventPayload::Snapshot(snapshot) => Some(snapshot),
            _ => None,
        })
        .collect()
}

fn attached_fixture(
    name: &str,
) -> (
    Arc<Shared>,
    ClientId,
    ExecutionContext,
    PaneId,
    Arc<OutboundMailbox>,
) {
    let (shared, client, mut context, pane, mailbox) = key_table_fixture(name);
    wait_for_runtime_facts(&shared, pane);
    run_test_command(
        &shared,
        client,
        &mut context,
        &["set-option", "-g", "automatic-rename", "off"],
    );
    take_reliable_messages(&mailbox);
    (shared, client, context, pane, mailbox)
}

fn wait_for_runtime_facts(shared: &Shared, pane: PaneId) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while shared
        .inner
        .lock()
        .engine
        .pane_runtime_facts(pane)
        .is_none_or(|facts| facts.current_command.is_empty() || facts.pid.is_none())
    {
        assert!(
            Instant::now() < deadline,
            "pane runtime facts never settled: {:?}",
            shared.inner.lock().engine.pane_runtime_facts(pane)
        );
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(50));
}

fn set_current_command(shared: &Shared, pane: PaneId, command: &str) -> bool {
    let mut inner = shared.inner.lock();
    let facts = PaneRuntimeFacts {
        current_command: command.to_owned(),
        ..inner
            .engine
            .pane_runtime_facts(pane)
            .cloned()
            .unwrap_or_default()
    };
    inner.engine.set_pane_runtime_facts(pane, facts)
}

#[cfg(unix)]
#[test]
fn key_tables_publish_only_when_the_bindings_change() {
    let (shared, client, mut context, _, mailbox) = key_table_fixture("key-generation");
    let bind = ["bind-key", "-T", "root", "F12", "display-message", "x"];
    run_test_command(&shared, client, &mut context, &bind);
    assert_eq!(key_table_publications(&events(&mailbox)), 1);
    run_test_command(
        &shared,
        client,
        &mut context,
        &["display-message", "-p", "x"],
    );
    assert_eq!(key_table_publications(&events(&mailbox)), 0);
    run_test_command(&shared, client, &mut context, &bind);
    assert_eq!(key_table_publications(&events(&mailbox)), 0);
    run_test_command(
        &shared,
        client,
        &mut context,
        &["unbind-key", "-T", "root", "F12"],
    );
    assert_eq!(key_table_publications(&events(&mailbox)), 1);
}

#[cfg(unix)]
#[test]
fn source_file_publishes_its_key_tables_once() {
    let (shared, client, mut context, _, mailbox) = key_table_fixture("key-replay");
    let directory = tempfile::tempdir().expect("config directory");
    let path = directory.path().join("keys.conf");
    fs::write(
        &path,
        "bind-key -T zzpub a display-message a\n\
         bind-key -T zzpub b display-message b\n\
         set -g @zzpub on\n\
         bind-key -T zzpub c display-message c\n",
    )
    .expect("write config");
    run_test_command(
        &shared,
        client,
        &mut context,
        &["source-file", path.to_str().expect("utf-8 path")],
    );
    let published = events(&mailbox)
        .into_iter()
        .filter_map(|event| match event {
            EventPayload::KeyTablesChanged { tables } => Some(tables),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(published.len(), 1);
    let zzpub = published[0]
        .iter()
        .find(|table| table.name == "zzpub")
        .expect("replayed table");
    assert_eq!(
        zzpub
            .bindings
            .iter()
            .map(|binding| binding.key.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
}

#[cfg(unix)]
#[test]
fn hello_key_tables_match_a_fresh_snapshot() {
    let (shared, client, mut context, _, _mailbox) = key_table_fixture("key-hello");
    run_test_command(
        &shared,
        client,
        &mut context,
        &["bind-key", "-T", "root", "F11", "splitw"],
    );
    let (_, hello) = shared
        .register(
            ClientKind::Interactive,
            ClientInstanceId::default(),
            None,
            None,
            true,
            false,
        )
        .expect("register");
    assert_eq!(hello.key_tables, shared.inner.lock().engine.keys.snapshot());
    let root = hello
        .key_tables
        .iter()
        .find(|table| table.name == "root")
        .expect("root table");
    let binding = root
        .bindings
        .iter()
        .find(|binding| binding.key == "F11")
        .expect("bound key");
    assert_eq!(binding.commands[0].name, "split-window");
}

#[cfg(unix)]
#[test]
fn an_unchanged_snapshot_is_not_sent_again() {
    let (shared, _, _, _, mailbox) = attached_fixture("snapshot-dedupe");
    shared.publish_mux_snapshots();
    assert!(snapshots(events(&mailbox)).is_empty());
    let (window, previous) = {
        let inner = shared.inner.lock();
        let window = *inner.engine.state.windows.keys().next().expect("window");
        (window, inner.engine.state.generation())
    };
    shared
        .inner
        .lock()
        .engine
        .state
        .rename_window(window, "renamed")
        .expect("rename");
    shared.publish_mux_snapshots();
    let sent = snapshots(events(&mailbox));
    assert_eq!(sent.len(), 1);
    assert!(sent[0].generation > previous);
    shared.publish_mux_snapshots();
    assert!(snapshots(events(&mailbox)).is_empty());
}

#[cfg(unix)]
#[test]
fn runtime_facts_keep_the_tree_generation_and_skip_unreferenced_labels() {
    let (shared, _, _, pane, mailbox) = attached_fixture("runtime-quiet");
    let generation = shared.inner.lock().engine.state.generation();
    let runtime = shared.inner.lock().engine.runtime_facts_generation();
    assert!(set_current_command(&shared, pane, "zzpub-quiet"));
    assert_eq!(shared.inner.lock().engine.state.generation(), generation);
    assert!(shared.inner.lock().engine.runtime_facts_generation() > runtime);
    shared.publish_mux_snapshots();
    assert!(snapshots(events(&mailbox)).is_empty());
}

#[cfg(unix)]
#[test]
fn a_runtime_fact_label_change_is_sent_under_a_new_generation() {
    let (shared, client, mut context, pane, mailbox) = attached_fixture("runtime-label");
    run_test_command(
        &shared,
        client,
        &mut context,
        &[
            "set-option",
            "-g",
            "window-status-current-format",
            "#{pane_current_command}",
        ],
    );
    events(&mailbox);
    let sent_generation = shared.inner.lock().published_snapshots[&client].1;
    assert!(set_current_command(&shared, pane, "zzpub-label"));
    shared.publish_mux_snapshots();
    let sent = snapshots(events(&mailbox));
    assert_eq!(sent.len(), 1);
    assert!(sent[0].generation > sent_generation);
    assert!(
        sent[0].sessions[0]
            .windows
            .iter()
            .any(|window| window.status_label.contains("zzpub-label"))
    );
    shared.publish_mux_snapshots();
    assert!(snapshots(events(&mailbox)).is_empty());
}

#[cfg(unix)]
#[test]
fn pane_events_publish_on_the_leading_edge_and_coalesce_the_rest() {
    let (shared, _, _, _, mailbox) = attached_fixture("publish-coalesce");
    shared.start_timers().expect("start timers");
    let window = *shared
        .inner
        .lock()
        .engine
        .state
        .windows
        .keys()
        .next()
        .expect("window");
    let rename = |name: &str| {
        shared
            .inner
            .lock()
            .engine
            .state
            .rename_window(window, name)
            .expect("rename");
    };
    thread::sleep(timers::PUBLISH_FLUSH_INTERVAL * 2);
    rename("leading");
    shared.request_publish(timers::PublishReason::Tree);
    assert_eq!(snapshots(events(&mailbox)).len(), 1);
    rename("middle");
    shared.request_publish(timers::PublishReason::Tree);
    rename("trailing");
    shared.request_publish(timers::PublishReason::Tree);
    assert!(snapshots(events(&mailbox)).is_empty());
    let deadline = Instant::now() + Duration::from_secs(5);
    let trailing = loop {
        let sent = snapshots(events(&mailbox));
        if !sent.is_empty() {
            break sent;
        }
        assert!(Instant::now() < deadline, "trailing flush never ran");
        thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(trailing.len(), 1);
    assert_eq!(trailing[0].sessions[0].windows[0].name, "trailing");
}

#[cfg(unix)]
#[test]
fn a_blocking_silence_hook_does_not_stall_other_timers() {
    let (shared, client, mut context, pane, mailbox) = key_table_fixture("timer-stall");
    shared.start_timers().expect("start timers");
    run_test_command(
        &shared,
        client,
        &mut context,
        &["new-window", "-d", "-n", "quiet", "exec /bin/cat"],
    );
    run_test_command(
        &shared,
        client,
        &mut context,
        &["set-hook", "-g", "alert-silence", "run-shell 'sleep 2'"],
    );
    run_test_command(
        &shared,
        client,
        &mut context,
        &["set-option", "-g", "prefix-timeout", "1500"],
    );
    run_test_command(
        &shared,
        client,
        &mut context,
        &["set-option", "-w", "-t", ":quiet", "monitor-silence", "1"],
    );
    take_reliable_messages(&mailbox);
    let start = Instant::now();
    input_test_key(
        &shared,
        client,
        &mut context,
        pane,
        test_key(
            KeyCode::Character('b'),
            Modifiers::new(false, true, false, false),
            None,
        ),
    );
    assert_eq!(
        key_table_events(&mailbox),
        vec![
            EventPayload::PrefixArmed { armed: true },
            table_active(Some("prefix"), false),
        ]
    );
    let silenced = |shared: &Shared| {
        shared
            .inner
            .lock()
            .engine
            .state
            .windows
            .values()
            .any(|window| window.name == "quiet" && window.silence_flag)
    };
    let deadline = start + Duration::from_secs(10);
    let mut cleared = Vec::new();
    while cleared
        != vec![
            EventPayload::PrefixArmed { armed: false },
            table_active(None, false),
        ]
    {
        assert!(
            Instant::now() < deadline,
            "prefix never expired: {cleared:?}"
        );
        thread::sleep(Duration::from_millis(10));
        cleared.extend(key_table_events(&mailbox));
    }
    let expired = start.elapsed();
    assert!(silenced(&shared), "the silence alert fired first");
    assert!(
        expired < Duration::from_millis(2600),
        "prefix expiry waited behind the silence hook: {expired:?}"
    );
}

#[test]
fn the_status_sampler_parks_without_clients_and_wakes_for_one() {
    let shared = Arc::new(Shared::new(1));
    shared.start_status_sampler().expect("start sampler");
    let wait_for = |idle: bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while shared.status_sampler_idle.load(Ordering::SeqCst) != idle {
            assert!(
                Instant::now() < deadline,
                "sampler never became idle={idle}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    };
    wait_for(true);
    let mailbox = OutboundMailbox::new();
    shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
    wait_for(false);
    shared.request_shutdown();
}

#[cfg(unix)]
#[test]
fn window_style_reaches_a_detached_pane() {
    let shared = Arc::new(Shared::new(1));
    let directory = tempfile::tempdir().expect("scratch");
    let trigger = directory.path().join("go");
    let reply = directory.path().join("reply");
    let script = format!(
        "while [ ! -e '{}' ]; do sleep 0.02; done; stty raw -echo; printf '\\033]11;?\\033\\\\'; head -c 25 > '{}'; exec cat",
        trigger.display(),
        reply.display()
    );
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(1),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "detached-style", &script]),
        )
        .expect("detached session");
    let terminal = shared.inner.lock().terminals[&context.pane.expect("pane")].clone();
    wait_for_terminal_identity(&terminal);
    shared
        .execute(
            ClientId(1),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(
                "set-option",
                ["-w", "-t", "detached-style:", "window-style", "bg=#ff0000"],
            ),
        )
        .expect("set window-style");
    fs::write(&trigger, "").expect("trigger query");
    let deadline = Instant::now() + Duration::from_secs(10);
    let answer = loop {
        let answer = fs::read(&reply).unwrap_or_default();
        if answer.len() >= 25 {
            break String::from_utf8_lossy(&answer).into_owned();
        }
        assert!(Instant::now() < deadline, "no OSC 11 reply: {answer:?}");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(answer.contains("rgb:ffff/0000/0000"), "{answer:?}");
    shared.request_shutdown();
}

#[cfg(unix)]
#[test]
fn a_busy_tiled_window_sends_few_snapshots_and_status_lines() {
    let (shared, client, mut context, _, mailbox) = key_table_fixture("busy-tiled");
    shared.start_timers().expect("start timers");
    let shell = "exec /bin/bash --noprofile --norc -i";
    run_test_command(
        &shared,
        client,
        &mut context,
        &["respawn-pane", "-k", shell],
    );
    for _ in 0..3 {
        run_test_command(&shared, client, &mut context, &["split-window", shell]);
        run_test_command(&shared, client, &mut context, &["select-layout", "tiled"]);
    }
    let panes = {
        let inner = shared.inner.lock();
        let window = context.window.expect("window");
        inner.engine.state.windows[&window].pane_order().to_vec()
    };
    thread::sleep(Duration::from_millis(500));
    let printer = "for i in $(seq 1 300); do echo line $i padding padding; sleep 0.01; done";
    for pane in &panes {
        run_test_command(
            &shared,
            client,
            &mut context,
            &["send-keys", "-t", &pane.to_string(), printer, "Enter"],
        );
    }
    thread::sleep(Duration::from_millis(500));
    events(&mailbox);
    thread::sleep(Duration::from_secs(3));
    let sent = events(&mailbox);
    let snapshots = sent
        .iter()
        .filter(|event| matches!(event, EventPayload::Snapshot(_)))
        .count();
    let status_lines = sent
        .iter()
        .filter(|event| matches!(event, EventPayload::StatusChanged { .. }))
        .count();
    assert!(snapshots <= 12, "{snapshots} snapshots in 3 s");
    assert!(status_lines <= 10, "{status_lines} status lines in 3 s");
    shared.request_shutdown();
}

#[cfg(unix)]
#[test]
fn a_client_attaching_later_sees_runtime_facts_changed_while_detached() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(1),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "later", "exec /bin/cat"]),
        )
        .expect("detached session");
    let session = context.session.expect("session");
    let pane = context.pane.expect("pane");
    wait_for_runtime_facts(&shared, pane);
    for command in [
        ["set-option", "-g", "automatic-rename", "off"],
        [
            "set-option",
            "-g",
            "window-status-current-format",
            "#{pane_current_command}",
        ],
    ] {
        shared
            .execute(
                ClientId(1),
                ClientKind::Command,
                &mut ExecutionContext::default(),
                &CommandInvocation::new(command[0], command[1..].iter().copied()),
            )
            .expect("option");
    }
    assert!(set_current_command(&shared, pane, "zzpub-detached"));
    shared.publish_mux_snapshots();
    let mailbox = OutboundMailbox::new();
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&mailbox));
    let attached = shared.attach(client, session).expect("attach");
    assert!(
        attached.sessions[0]
            .windows
            .iter()
            .any(|window| window.status_label.contains("zzpub-detached"))
    );
    shared.request_shutdown();
}

#[cfg(unix)]
#[test]
fn a_runtime_fact_flush_is_silent_unless_a_template_reads_runtime_facts() {
    let (shared, client, mut context, pane, mailbox) = attached_fixture("runtime-flush");
    shared.inner.lock().client_sizes.insert(client, (80, 24));
    shared.start_timers().expect("start timers");
    let flush = |shared: &Arc<Shared>| {
        shared.request_publish(timers::PublishReason::RuntimeFacts);
        thread::sleep(timers::PUBLISH_FLUSH_INTERVAL * 3);
    };
    let counts = |events: &[EventPayload]| {
        (
            events
                .iter()
                .filter(|event| matches!(event, EventPayload::Snapshot(_)))
                .count(),
            events
                .iter()
                .filter(|event| matches!(event, EventPayload::StatusChanged { .. }))
                .count(),
        )
    };
    assert!(set_current_command(&shared, pane, "zzpub-silent"));
    flush(&shared);
    assert_eq!(counts(&events(&mailbox)), (0, 0));
    run_test_command(
        &shared,
        client,
        &mut context,
        &[
            "set-option",
            "-g",
            "status-right",
            "#{pane_current_command}",
        ],
    );
    events(&mailbox);
    assert!(set_current_command(&shared, pane, "zzpub-loud"));
    flush(&shared);
    let (_, status_lines) = counts(&events(&mailbox));
    assert_eq!(status_lines, 1);
}
