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
        inner
            .clients
            .entry(client)
            .or_default()
            .name
            .replace("watcher".to_owned());
        inner.clients.entry(client).or_default().pid.replace(42);
        inner
            .clients
            .entry(client)
            .or_default()
            .tty
            .replace("/dev/ttys003".to_owned());
        inner
            .clients
            .entry(client)
            .or_default()
            .focused_window
            .replace(window);
        inner.session_last_attached.insert(session, 73);
        inner
            .clients
            .entry(client)
            .or_default()
            .environment
            .replace(Arc::new(ClientEnvironmentBlob::from_map(BTreeMap::from([
                ("FROM_CLIENT".into(), "yes".into()),
            ]))));
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
        clients: &inner.clients,
        session_last_attached: &inner.session_last_attached,
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
fn borrowed_facts_keep_engine_access_with_legacy_variables_and_nested_loops() {
    let (shared, client, context) = fixture();
    let inner = shared.inner.lock();
    for borrowed_variables in [true, false] {
        zz_mux::with_borrowed_formats(borrowed_variables, || {
            let borrowed = borrowed(&inner, client, &context);
            let owned = format_hook_facts_for_client(&inner, client, &context);
            let values =
                inner
                    .engine
                    .format_status_context(context.session, context.window, context.pane);
            for template in [
                "#{@kept}:#{pane_kind}:#{window_active_clients}:#{window_active_clients_list}",
                "#{W:#{@kept}:#{pane_kind}:#{window_active_clients}:#{window_active_clients_list};}",
                "#{S:#{session_name}[#{W:#{@kept}:#{P:#{pane_kind}}:#{window_active_clients};}]}",
                "#{L:#{client_name}:#{client_session};}",
            ] {
                assert_eq!(
                    expand_format_values(
                        template,
                        &values,
                        &mut DaemonFormatHooks::command(&borrowed)
                    ),
                    expand_format_values(
                        template,
                        &values,
                        &mut DaemonFormatHooks::command(&owned)
                    ),
                    "borrowed_variables={borrowed_variables} {template}"
                );
            }
        });
    }
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

fn selected_request(inner: &ServerState, client: ClientId) -> StatusRequest {
    status_request_with_selected_facts(
        inner,
        client,
        inner.engine.cached_format_option_snapshot(),
        true,
        FormatNeeds::NONE,
    )
    .as_ref()
    .clone()
}

fn complete_request(inner: &ServerState, client: ClientId) -> StatusRequest {
    status_request(
        inner,
        client,
        &inner.engine.state.snapshot(),
        inner.engine.cached_format_option_snapshot(),
        format_hook_facts(inner),
        true,
        FormatNeeds::NONE,
    )
}

#[test]
fn selected_status_default_omits_unused_fact_maps_and_matches_complete_capture() {
    let (shared, client, _) = fixture();
    let inner = shared.inner.lock();
    let before = OWNED_FORMAT_FACT_BUILDS.with(Cell::get);
    let selected = selected_request(&inner, client);
    if *BORROWED_FORMAT_FACTS {
        assert_eq!(OWNED_FORMAT_FACT_BUILDS.with(Cell::get), before);
        assert!(selected.facts.clients.is_empty());
        assert!(selected.facts.session_attachments.is_empty());
        assert!(selected.facts.session_last_attached.is_empty());
        assert!(selected.facts.window_clients.is_empty());
        assert!(selected.facts.copy_modes.is_empty());
        assert!(selected.facts.pane_modes.is_empty());
        assert!(selected.facts.terminals.is_empty());
        assert!(selected.facts.buffer.is_none());
        assert!(selected.facts.client_environment.is_none());
        let client_facts = selected.facts.client.as_ref().unwrap();
        assert_eq!(client_facts.width, "80");
        assert!(client_facts.name.is_empty());
        assert!(client_facts.pid.is_empty());
        assert!(client_facts.tty.is_empty());
        assert!(client_facts.session.is_empty());
        assert!(client_facts.written.is_empty());
        assert!(client_facts.discarded.is_empty());
        assert!(client_facts.environment.is_none());
        assert!(client_facts.terminal.is_none());
    }
    let complete = complete_request(&inner, client);
    assert_eq!(complete.facts.clients.len(), 1);
    assert_eq!(
        StatusRenderer::default().render_initial(&selected),
        StatusRenderer::default().render_initial(&complete)
    );
}

#[test]
fn selected_status_client_callbacks_match_complete_capture_for_every_field() {
    let (shared, client, mut target) = fixture();
    let mut inner = shared.inner.lock();
    inner.clients.entry(client).or_default().has_terminal = true;
    inner
        .clients
        .entry(client)
        .or_default()
        .size
        .replace((42, 13));
    inner
        .clients
        .entry(client)
        .or_default()
        .created_time
        .replace(111);
    inner
        .clients
        .entry(client)
        .or_default()
        .activity_time
        .replace(222);
    inner
        .clients
        .entry(client)
        .or_default()
        .terminal_type
        .replace("VT420".to_owned());
    inner
        .clients
        .entry(client)
        .or_default()
        .color_scheme
        .replace(TerminalColorScheme::Light);
    let last_session = inner
        .engine
        .state
        .sessions
        .values()
        .find(|session| session.name == "beta")
        .unwrap()
        .id;
    inner
        .clients
        .entry(client)
        .or_default()
        .last_session
        .replace(last_session);
    inner.client_flags.apply(client, "read-only,active-pane");
    inner
        .clients
        .entry(client)
        .or_default()
        .key_engine
        .get_or_insert_default()
        .switch_table(Some("copy-mode".to_owned()));
    inner
        .clients
        .entry(client)
        .or_default()
        .features
        .replace(client_features_fact(&["client-features-v1:RGB".to_owned()]));
    inner
        .clients
        .entry(client)
        .or_default()
        .environment
        .replace(Arc::new(ClientEnvironmentBlob::from_map(BTreeMap::from([
            ("TERM".into(), "xterm-256color".into()),
            ("COLORTERM".into(), "truecolor".into()),
            ("LANG".into(), "en_US.UTF-8".into()),
        ]))));
    let mailbox = OutboundMailbox::new();
    {
        let mut state = mailbox.state.lock();
        state.written_bytes = 1234;
        state.discarded_bytes = 56;
    }
    inner.subscribers.insert(client, mailbox);
    inner
        .terminal_geometries
        .entry(target.pane.unwrap())
        .or_default()
        .insert(
            client,
            TerminalGeometry {
                columns: 120,
                rows: 40,
                cell_width_px: 9,
                cell_height_px: 19,
            },
        );
    inner
        .engine
        .execute(
            &mut target,
            &CommandInvocation::new(
                "set-option",
                ["-s", "terminal-features", "xterm*:RGB:extkeys"],
            ),
        )
        .unwrap();
    let fields = [
        "client_activity",
        "client_cell_height",
        "client_cell_width",
        "client_colours",
        "client_control_mode",
        "client_created",
        "client_discarded",
        "client_flags",
        "client_height",
        "client_key_table",
        "client_last_session",
        "client_name",
        "client_pid",
        "client_prefix",
        "client_readonly",
        "client_session",
        "client_termfeatures",
        "client_termname",
        "client_termtype",
        "client_theme",
        "client_tty",
        "client_uid",
        "client_user",
        "client_utf8",
        "client_width",
        "client_written",
        "list_clients_line",
        "window_cell_height",
        "window_cell_width",
        "window_bigger",
        "window_offset_x",
        "window_offset_y",
    ];
    for kind in [
        ClientKind::Interactive,
        ClientKind::Control,
        ClientKind::Command,
    ] {
        inner.clients.entry(client).or_default().kind.replace(kind);
        let values = inner
            .engine
            .format_status_context(target.session, target.window, target.pane);
        let complete = FormatHookFacts {
            client: Some(client_format_facts(&inner, client, target.session.unwrap())),
            ..FormatHookFacts::default()
        };
        if kind == ClientKind::Interactive {
            let client_facts = complete.client.as_ref().unwrap();
            assert_eq!(client_facts.cell_height, "19");
            assert_eq!(client_facts.cell_width, "9");
            assert!(client_facts.viewport.is_some());
            assert!(
                client_facts
                    .termfeatures
                    .split(',')
                    .any(|feature| feature == "RGB")
            );
        }
        for name in fields {
            let references = BTreeSet::from([name.to_owned()]);
            let selected = FormatHookFacts {
                client: Some(selected_client_format_facts(
                    &inner,
                    client,
                    target.session.unwrap(),
                    &references,
                )),
                ..FormatHookFacts::default()
            };
            let template = format!("#{{{name}}}");
            assert_eq!(
                expand_format_values(
                    &template,
                    &values,
                    &mut DaemonFormatHooks::command(&selected)
                ),
                expand_format_values(
                    &template,
                    &values,
                    &mut DaemonFormatHooks::command(&complete)
                ),
                "{kind:?}: {name}"
            );
        }
    }
}

#[test]
fn selected_status_client_dynamic_references_keep_terminal_and_environment_facts() {
    let (shared, client, target) = fixture();
    let mut inner = shared.inner.lock();
    inner
        .clients
        .entry(client)
        .or_default()
        .kind
        .replace(ClientKind::Interactive);
    inner.clients.entry(client).or_default().has_terminal = true;
    inner
        .clients
        .entry(client)
        .or_default()
        .environment
        .replace(Arc::new(ClientEnvironmentBlob::from_map(BTreeMap::from([
            ("TERM".into(), "xterm-256color".into()),
            ("FROM_CLIENT".into(), "yes".into()),
        ]))));
    for reference in ["*", "mode_unknown"] {
        let selected = selected_client_format_facts(
            &inner,
            client,
            target.session.unwrap(),
            &BTreeSet::from([reference.to_owned()]),
        );
        assert_eq!(selected.pid, "42");
        assert_eq!(selected.tty, "/dev/ttys003");
        assert!(selected.environment.is_some());
        assert!(selected.terminal.is_some());
    }
}

#[test]
fn selected_status_explicit_fact_groups_match_complete_capture() {
    let (shared, client, mut target) = fixture();
    let mut inner = shared.inner.lock();
    inner.paste_buffers.push(PasteBuffer {
        name: "kept-buffer".to_owned(),
        data: Arc::from(b"buffer data".as_slice()),
        created: UNIX_EPOCH + Duration::from_secs(17),
        automatic: true,
        utf8: true,
    });
    inner
        .pane_modes
        .insert(target.pane.unwrap(), vec![PaneModeRequest::Clock]);
    let template = "#{session_attached}:#{session_attached_list}:#{session_many_attached}:#{session_last_attached}:#{window_active_clients}:#{window_active_clients_list}:#{pane_in_mode}:#{pane_mode}:#{pane_unseen_changes}:#{buffer_created}:#{buffer_full}:#{buffer_name}:#{buffer_sample}:#{buffer_size}:#{pane_kind}:#{browser_url}:#{agent_state}:#{agent_pending_permission}:#{pane_pipe}:#{pane_pipe_pid}:#{history_size}:#{cursor_x}:#{cursor_y}:#{alternate_on}:#{mouse_any_flag}:#{pane_last_command_status}:#{pane_search_string}:#{pane_pb_progress}:#{pane_pb_state}:#{@kept}:#{client_name}:#{client_width}";
    inner
        .engine
        .execute(
            &mut target,
            &CommandInvocation::new("set-option", ["-g", "status-left", template]),
        )
        .unwrap();
    let selected = selected_request(&inner, client);
    let complete = complete_request(&inner, client);
    assert_eq!(
        expand_format_values(
            template,
            &selected.context,
            &mut DaemonFormatHooks::command(selected.facts.as_ref())
        ),
        expand_format_values(
            template,
            &complete.context,
            &mut DaemonFormatHooks::command(complete.facts.as_ref())
        )
    );
    assert_eq!(selected.facts.buffer.as_ref().unwrap().name, "kept-buffer");
    assert_eq!(selected.facts.session_attachments.len(), 1);
    assert_eq!(selected.facts.pane_modes.len(), 1);
}

#[test]
fn selected_status_dynamic_facts_and_control_clients_keep_complete_capture() {
    let (shared, client, _) = fixture();
    let mut inner = shared.inner.lock();
    let context = inner.engine.format_status_context(None, None, None);
    for reference in ["*", "mode_unknown"] {
        let selected =
            selected_status_format_facts(&inner, &context, &BTreeSet::from([reference.to_owned()]));
        assert_eq!(selected.clients.len(), 1);
        assert_eq!(selected.session_attachments.len(), 1);
    }
    drop(context);
    inner
        .clients
        .entry(client)
        .or_default()
        .kind
        .replace(ClientKind::Control);
    let request = selected_request(&inner, client);
    assert_eq!(request.facts.clients.len(), 1);
    assert!(request.facts.client_environment.is_some());
}

#[test]
fn selected_status_borders_borrow_facts_without_adding_them_to_the_request() {
    let (shared, client, mut target) = fixture();
    let mut inner = shared.inner.lock();
    inner
        .engine
        .execute(
            &mut target,
            &CommandInvocation::new(
                "set-option",
                [
                    "-g",
                    "pane-active-border-style",
                    "fg=#{?#{&&:#{session_attached_list},#{client_pid}},red,green}",
                ],
            ),
        )
        .unwrap();
    let selected = selected_request(&inner, client);
    let complete = complete_request(&inner, client);
    assert_eq!(selected.pane_borders, complete.pane_borders);
    assert!(!selected.pane_borders.is_empty());
    if *BORROWED_FORMAT_FACTS {
        assert!(selected.facts.session_attachments.is_empty());
    }
}

#[test]
fn selected_status_modes_capture_their_own_detached_fact_dependencies() {
    let (shared, client, mut target) = fixture();
    let terminal = Arc::new(TerminalSession::spawn_empty_with_appearance(
        64,
        Arc::new(TerminalAppearance::default()),
    ));
    let view = TerminalViewId(client.0);
    terminal.attach_view(view);
    terminal.view_action(view, zz_terminal::TerminalViewAction::EnterCopyMode);
    let deadline = Instant::now() + Duration::from_secs(3);
    while terminal.copy_mode_facts(view).is_none()
        || terminal.latest_viewport_for(view).is_none_or(|viewport| {
            !matches!(
                viewport.mode,
                TerminalMode::Copy {
                    hide_position: false,
                    ..
                }
            )
        })
    {
        assert!(Instant::now() < deadline, "copy facts did not become ready");
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut inner = shared.inner.lock();
    inner.terminals_mut().insert(target.pane.unwrap(), terminal);
    enter_copy_session(&mut inner, client, target.pane.unwrap()).unwrap();
    inner.paste_buffers.push(PasteBuffer {
        name: "mode-buffer".to_owned(),
        data: Arc::from(b"mode data".as_slice()),
        created: UNIX_EPOCH,
        automatic: true,
        utf8: true,
    });
    inner
        .engine
        .execute(
            &mut target,
            &CommandInvocation::new(
                "set-option",
                [
                    "-g",
                    "copy-mode-position-format",
                    "#{buffer_name}:#{pane_mode}:#{cursor_x}:#{copy_position}",
                ],
            ),
        )
        .unwrap();
    let selected = selected_request(&inner, client);
    let complete = complete_request(&inner, client);
    assert_eq!(selected.modes.len(), 1);
    assert_eq!(selected.facts.client.as_ref().unwrap().pid, "42");
    assert!(
        selected
            .facts
            .client
            .as_ref()
            .unwrap()
            .environment
            .is_some()
    );
    assert_eq!(selected.facts.buffer.as_ref().unwrap().name, "mode-buffer");
    assert_eq!(
        StatusRenderer::default().render_initial(&selected),
        StatusRenderer::default().render_initial(&complete)
    );
}
