use super::*;
use crate::status::FormatFactSource;

fn fixture() -> (Arc<Shared>, ClientId, ExecutionContext) {
    let shared = Arc::new(Shared::new(1));
    let client = ClientId(3);
    let context = {
        let mut inner = shared.inner.lock();
        let (session, window, pane) = inner.engine.state.create_session("alpha").unwrap();
        inner.engine.state.create_session("beta").unwrap();
        inner.attached.insert(session, BTreeSet::from([client]));
        inner.client_names.insert(client, "watcher".to_owned());
        inner.client_pids.insert(client, 42);
        inner.client_ttys.insert(client, "/dev/ttys003".to_owned());
        inner.focused_windows.insert(client, window);
        inner.session_last_attached.insert(session, 73);
        inner.client_environments.insert(
            client,
            Arc::new(ClientEnvironmentBlob::from_map(BTreeMap::from([(
                "FROM_CLIENT".into(),
                "yes".into(),
            )]))),
        );
        let mut context = ExecutionContext::new(Some(session), Some(window), Some(pane));
        inner
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "@kept", "original"]),
            )
            .unwrap();
        context
    };
    (shared, client, context)
}

fn borrowed<'a>(
    inner: &'a ServerState,
    client: ClientId,
    context: &ExecutionContext,
) -> BorrowedFormatHookFacts<'a> {
    BorrowedFormatHookFacts {
        client_fields: ClientFormatFields::from_inner(inner),
        paste_buffers: &inner.paste_buffers,
        agent_states: &inner.agent_states,
        terminals: &inner.terminals,
        pane_pipes: &inner.pane_pipes,
        attached: &inner.attached,
        suspended_clients: &inner.suspended_clients,
        client_names: &inner.client_names,
        client_pids: &inner.client_pids,
        client_ttys: &inner.client_ttys,
        session_last_attached: &inner.session_last_attached,
        copy_sessions: &inner.copy_sessions,
        pane_modes: &inner.pane_modes,
        seed: command_format_seed(inner, client, context),
        derived: LazyDerivedFormatFacts::default(),
    }
}

#[test]
fn borrowed_daemon_format_facts_build_only_the_derived_maps_read_by_a_template() {
    let (shared, client, context) = fixture();
    let inner = shared.inner.lock();
    let facts = borrowed(&inner, client, &context);
    let values = inner
        .engine
        .format_status_context(context.session, context.window, context.pane);
    let mut hooks = DaemonFormatHooks::command(&facts);
    assert_eq!(
        expand_format_values("#{session_name}:#{@kept}:#{pane_kind}", &values, &mut hooks),
        "alpha:original:terminal"
    );
    assert!(facts.derived.clients.get().is_none());
    assert!(facts.derived.buffer.get().is_none());
    assert!(facts.derived.pane_pipes.get().is_none());
    assert!(facts.derived.session_attachments.get().is_none());
    assert!(facts.derived.unseen_changes.get().is_none());
    assert!(facts.derived.window_clients.get().is_none());
    assert!(facts.derived.copy_modes.get().is_none());
    assert!(facts.derived.pane_modes.get().is_none());
    assert_eq!(
        expand_format_values("#{session_attached_list}", &values, &mut hooks),
        "/dev/ttys003"
    );
    assert!(facts.derived.session_attachments.get().is_some());
    assert!(facts.derived.window_clients.get().is_none());
}

#[test]
fn borrowed_and_owned_daemon_format_facts_expand_the_same_values() {
    let (shared, client, context) = fixture();
    let inner = shared.inner.lock();
    let borrowed = borrowed(&inner, client, &context);
    let owned = format_hook_facts_for_client(&inner, client, &context);
    let values = inner
        .engine
        .format_status_context(context.session, context.window, context.pane);
    for template in [
        "#{session_attached}:#{session_attached_list}:#{session_many_attached}:#{session_last_attached}",
        "#{window_active_clients}:#{window_active_clients_list}:#{pane_in_mode}:#{pane_mode}:#{pane_unseen_changes}",
        "#{client_name}:#{client_pid}:#{client_prefix}:#{client_session}:#{client_termname}:#{client_width}:#{client_height}",
        "#{pane_kind}:#{browser_url}:#{agent_state}:#{pane_pipe}:#{pane_pipe_pid}:#{history_size}:#{pane_last_command_status}",
        "#{@kept}:#{Vc:#{environ_name}=#{environ_value};}:#{C:#{client_name}:#{client_session};}",
    ] {
        assert_eq!(
            expand_format_values(
                template,
                &values,
                &mut DaemonFormatHooks::command(&borrowed)
            ),
            expand_format_values(template, &values, &mut DaemonFormatHooks::command(&owned)),
            "{template}"
        );
    }
    assert_eq!(borrowed.session_attachments(), owned.session_attachments());
    assert_eq!(
        borrowed.window_clients(&values),
        owned.window_clients(&values)
    );
}

#[test]
fn borrowed_command_facts_expand_without_building_an_owned_snapshot() {
    if !*BORROWED_FORMAT_FACTS {
        return;
    }
    let (shared, client, mut context) = fixture();
    let before = OWNED_FORMAT_FACT_BUILDS.with(Cell::get);
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("set-option", ["-gF", "@created", "#{session_name}"]),
        )
        .unwrap();
    let execution = shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new(
                "display-message",
                ["-p", "#{session_name}:#{pane_kind}:#{@created}"],
            ),
        )
        .unwrap();
    assert_eq!(execution.output.to_string(), "alpha:terminal:alpha");
    let mut inner = shared.inner.lock();
    assert_eq!(
        expand_buffer_path(
            &mut inner,
            Some(client),
            &context,
            None,
            "save-buffer",
            "#{session_name}:#{pane_kind}:#{command}",
        ),
        "alpha:terminal:save-buffer"
    );
    let rows = chooser_presentation::client_chooser_rows(
        &inner,
        Some("#{client_name}:#{client_session}:#{session_name}"),
        None,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "/dev/ttys003:alpha:alpha");
    assert_eq!(OWNED_FORMAT_FACT_BUILDS.with(Cell::get), before);
}

#[test]
fn format_tree_entries_leave_table_callbacks_ahead_of_command_values() {
    let (shared, client, context) = fixture();
    let inner = shared.inner.lock();
    let facts = borrowed(&inner, client, &context);
    let values = inner
        .engine
        .format_status_context(context.session, context.window, context.pane);
    let variables = BTreeMap::from([
        ("session_name".to_owned(), "spoofed".to_owned()),
        ("pane_id".to_owned(), "spoofed".to_owned()),
        ("local".to_owned(), "kept".to_owned()),
    ]);
    let mut hooks = DaemonFormatHooks::command_with_variables(&facts, &variables)
        .with_command_item("display-message");
    assert_eq!(
        expand_format_values(
            "#{session_name}:#{pane_id}:#{local}:#{command}",
            &values,
            &mut hooks
        ),
        format!("alpha:{}:kept:display-message", context.pane.unwrap())
    );
}

#[test]
fn daemon_config_files_and_mouse_callbacks_read_their_fact_variables() {
    let (shared, client, context) = fixture();
    let inner = shared.inner.lock();
    let facts = borrowed(&inner, client, &context);
    let mut values =
        inner
            .engine
            .format_status_context(context.session, context.window, context.pane);
    values.set_format_value("config_files", "/context.conf");
    let variables = BTreeMap::from([
        (
            "config_files".to_owned(),
            "/first.conf,/second.conf".to_owned(),
        ),
        ("mouse_pane".to_owned(), "%7".to_owned()),
        ("mouse_x".to_owned(), "9".to_owned()),
        ("mouse_y".to_owned(), "12".to_owned()),
        ("mouse_word".to_owned(), "word".to_owned()),
        ("mouse_line".to_owned(), "a line".to_owned()),
        (
            "mouse_hyperlink".to_owned(),
            "https://example.test".to_owned(),
        ),
        ("session_name".to_owned(), "spoofed".to_owned()),
    ]);
    let mut hooks = DaemonFormatHooks::command_with_variables(&facts, &variables);
    assert_eq!(
        expand_format_values(
            "#{config_files}|#{mouse_pane}|#{mouse_x}|#{mouse_y}|#{mouse_word}|#{mouse_line}|#{mouse_hyperlink}|#{session_name}",
            &values,
            &mut hooks,
        ),
        "/first.conf,/second.conf|%7|9|12|word|a line|https://example.test|alpha"
    );
    assert_eq!(
        expand_format_values(
            "#{config_files}|#{mouse_word}",
            &values,
            &mut DaemonFormatHooks::command(&facts),
        ),
        "/context.conf|"
    );
}
