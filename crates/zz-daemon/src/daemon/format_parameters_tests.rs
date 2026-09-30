use super::*;

fn parameters(inner: &ServerState, session: Option<SessionId>) -> Arc<StatusParameters> {
    status_parameters(
        inner,
        session,
        &inner.engine.cached_format_option_snapshot(),
    )
}

fn execute(inner: &mut ServerState, args: &[&str]) {
    inner
        .engine
        .execute(
            &mut ExecutionContext::default(),
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap();
}

#[test]
fn status_parameters_observe_options_arrays_titles_environment_and_default_terminal() {
    let mut inner = ServerState::default();
    let (session, _, _) = inner.engine.state.create_session("work").unwrap();
    let first = parameters(&inner, Some(session));
    inner.engine.set_format_now(1234);
    let second = parameters(&inner, Some(session));
    if inner.engine.format_cache_revision().is_some() {
        assert!(Arc::ptr_eq(&first, &second));
    }
    for args in [
        vec!["set-option", "-g", "status-left", "#{session_name}:new"],
        vec!["set-option", "-g", "status-format[0]", "#{client_width}"],
        vec!["set-option", "-g", "set-titles", "on"],
        vec![
            "set-option",
            "-g",
            "set-titles-string",
            "title:#{session_name}",
        ],
        vec!["set-option", "-g", "message-style", "fg=blue"],
        vec!["set-option", "-g", "message-line", "1"],
        vec!["set-option", "-g", "default-terminal", "xterm"],
        vec!["set-environment", "-g", "FORMAT_PARAMETER_TEST", "changed"],
    ] {
        execute(&mut inner, &args);
    }
    let changed = parameters(&inner, Some(session));
    assert_eq!(changed.formats.left, "#{session_name}:new");
    assert_eq!(changed.row_formats.get(&0).unwrap(), "#{client_width}");
    assert_eq!(
        changed.title_format.as_deref(),
        Some("title:#{session_name}")
    );
    assert_eq!(changed.message_styles.0, "fg=blue");
    assert_eq!(changed.message_line, 1);
    assert_eq!(changed.default_terminal.as_str(), "xterm");
    assert!(changed.references.contains("client_width"));
    assert!(changed.environment.iter().any(|(name, value)| {
        name.as_bytes() == b"FORMAT_PARAMETER_TEST"
            && value
                .as_ref()
                .is_some_and(|value| value.as_bytes() == b"changed")
    }));
    assert_eq!(first.formats.left, second.formats.left);
}

#[test]
fn status_parameters_retarget_sessions_and_reject_a_replaced_engine_with_equal_revisions() {
    let mut inner = ServerState::default();
    let (work, _, _) = inner.engine.state.create_session("work").unwrap();
    let (other, _, _) = inner.engine.state.create_session("other").unwrap();
    execute(
        &mut inner,
        &["set-option", "-t", "work", "status-left", "work-left"],
    );
    execute(
        &mut inner,
        &["set-option", "-t", "other", "status-left", "other-left"],
    );
    assert_eq!(parameters(&inner, Some(work)).formats.left, "work-left");
    assert_eq!(parameters(&inner, Some(other)).formats.left, "other-left");
    assert_eq!(parameters(&inner, Some(work)).formats.left, "work-left");

    let make_engine = |left: &str| {
        let mut engine = MuxEngine::default();
        engine.state.create_session("equal").unwrap();
        engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new("set-option", ["-g", "status-left", left]),
            )
            .unwrap();
        engine
    };
    inner.engine = make_engine("old");
    let revision = inner.engine.format_cache_revision();
    let old = parameters(&inner, None);
    inner.engine = make_engine("new");
    assert_eq!(inner.engine.format_cache_revision(), revision);
    assert_eq!(parameters(&inner, None).formats.left, "new");
    assert_eq!(old.formats.left, "old");
}

#[test]
fn status_parameters_bound_includes_environment_and_disables_retention_on_rollback() {
    let mut inner = ServerState::default();
    parameters(&inner, None);
    assert_eq!(
        inner.status_parameters_cache.lock().is_some(),
        inner.engine.format_cache_revision().is_some()
    );
    let large = "v".repeat(600_000);
    execute(
        &mut inner,
        &["set-environment", "-g", "FORMAT_PARAMETER_TEST", &large],
    );
    let oversized = parameters(&inner, None);
    assert!(oversized.retained_bytes() > 1024 * 1024);
    assert!(inner.status_parameters_cache.lock().is_none());
    assert!(oversized.environment.iter().any(|(name, value)| {
        name.as_bytes() == b"FORMAT_PARAMETER_TEST"
            && value
                .as_ref()
                .is_some_and(|value| value.as_bytes() == large.as_bytes())
    }));
}

#[test]
fn status_preparation_reuses_engine_capture_and_keeps_client_and_config_values_fresh() {
    let mut inner = ServerState::default();
    let (session, window, _) = inner.engine.state.create_session("work").unwrap();
    let client = ClientId(3);
    inner.attached.insert(session, BTreeSet::from([client]));
    inner.focused_windows.insert(client, window);
    inner.client_kinds.insert(client, ClientKind::Interactive);
    inner.client_sizes.insert(client, (80, 24));
    inner.engine.set_format_now(1234);
    for args in [
        vec![
            "set-option",
            "-g",
            "status-left",
            "#{client_width}:#{config_files}:#{pane_active}",
        ],
        vec!["set-option", "-g", "status-left-length", "100"],
        vec!["set-option", "-g", "status-right", ""],
    ] {
        execute(&mut inner, &args);
    }
    let request = |inner: &ServerState| {
        status_request_with_selected_facts(
            inner,
            client,
            inner.engine.cached_format_option_snapshot(),
            true,
            FormatNeeds::NONE,
        )
    };
    let first = zz_mux::with_borrowed_formats(true, || request(&inner));
    if zz_mux::format_cache_knob() {
        zz_mux::with_borrowed_formats(true, || {
            let cached = cached_live_status_context(
                &inner,
                Some(session),
                Some(window),
                parameters(&inner, Some(session)).needs,
                &first.references,
            )
            .unwrap();
            assert!(first.context.same_detached(&cached));
        });
    }
    let mut renderer = StatusRenderer::default();
    let first_left = renderer.render_initial(&first).left;
    let prefix = first_left.strip_suffix("80::1").unwrap().to_owned();
    let revision = inner.engine.format_cache_revision();
    inner.client_sizes.insert(client, (120, 24));
    let resized = request(&inner);
    assert_eq!(inner.engine.format_cache_revision(), revision);
    assert_eq!(
        renderer.render_initial(&resized).left,
        format!("{prefix}120::1")
    );
    inner.config_files = "new.conf".to_owned();
    let configured = request(&inner);
    assert_eq!(
        renderer.render_initial(&configured).left,
        format!("{prefix}120:new.conf:1")
    );
    assert_eq!(renderer.render_initial(&first).left, first_left);
}

#[test]
fn status_fact_selection_plans_reuse_dependencies_and_keep_referenced_facts_fresh() {
    let mut inner = ServerState::default();
    let (session, window, pane) = inner.engine.state.create_session("work").unwrap();
    let client = ClientId(3);
    inner.attached.insert(session, BTreeSet::from([client]));
    inner.focused_windows.insert(client, window);
    inner.client_kinds.insert(client, ClientKind::Interactive);
    inner.client_names.insert(client, "first".to_owned());
    inner.client_ttys.insert(client, "/dev/first".to_owned());
    inner.client_sizes.insert(client, (80, 24));
    let template = "#{client_name}:#{client_width}:#{session_attached_list}:#{pane_in_mode}";
    execute(&mut inner, &["set-option", "-g", "status-left", template]);
    let first = parameters(&inner, Some(session));
    let builds = FORMAT_FACT_SELECTION_BUILDS.with(Cell::get);
    let facts = |inner: &ServerState, selection: &StatusParameters| {
        let context = inner
            .engine
            .format_status_context(Some(session), Some(window), Some(pane));
        let mut facts =
            selected_status_format_facts_with_selection(inner, &context, &selection.fact_selection);
        facts.client = Some(selected_client_format_facts_with_selection(
            inner,
            client,
            session,
            &selection.client_fact_selection,
        ));
        facts
    };
    let expand = |inner: &ServerState, facts: &FormatHookFacts| {
        let context = inner
            .engine
            .format_status_context(Some(session), Some(window), Some(pane));
        expand_format_values(template, &context, &mut DaemonFormatHooks::command(facts))
    };
    let original = facts(&inner, &first);
    assert_eq!(expand(&inner, &original), "/dev/first:80:/dev/first:0");
    let revision = inner.engine.format_cache_revision();
    inner.client_names.insert(client, "changed".to_owned());
    inner.client_ttys.insert(client, "/dev/changed".to_owned());
    inner.client_sizes.insert(client, (120, 24));
    inner.pane_modes.insert(pane, vec![PaneModeRequest::Clock]);
    assert_eq!(inner.engine.format_cache_revision(), revision);
    for _ in 0..3 {
        let current = parameters(&inner, Some(session));
        if zz_mux::format_cache_knob() {
            assert!(Arc::ptr_eq(&first, &current));
            assert_eq!(FORMAT_FACT_SELECTION_BUILDS.with(Cell::get), builds);
        }
        let selected = facts(&inner, &current);
        assert_eq!(expand(&inner, &selected), "/dev/changed:120:/dev/changed:1");
        let request = status_request_with_selected_facts(
            &inner,
            client,
            inner.engine.cached_format_option_snapshot(),
            true,
            FormatNeeds::NONE,
        );
        assert_eq!(
            expand_format_values(
                template,
                &request.context,
                &mut DaemonFormatHooks::command(request.facts.as_ref()),
            ),
            "/dev/changed:120:/dev/changed:1"
        );
        let mut complete = format_hook_facts(&inner);
        complete.client = Some(client_format_facts(&inner, client, session));
        assert_eq!(expand(&inner, &selected), expand(&inner, &complete));
    }
    assert_eq!(expand(&inner, &original), "/dev/first:80:/dev/first:0");
    if zz_mux::format_cache_knob() {
        assert_eq!(FORMAT_FACT_SELECTION_BUILDS.with(Cell::get), builds);
    }
}
