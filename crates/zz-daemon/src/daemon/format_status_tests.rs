use super::tests::{counted_value_job, engine_request, execute, request, settled};
use super::*;

#[test]
fn daemon_option_proof_rejects_ordinary_snapshot_rows_and_preserves_scoped_native_values() {
    let mut engine = MuxEngine::default();
    let mut execution = zz_mux::ExecutionContext::default();
    let context = StatusContext::from(zz_mux::StatusValues {
        session_id: "$7".to_owned(),
        ..Default::default()
    });
    let variables = BTreeMap::from([("key_command".to_owned(), "tree-command".to_owned())]);
    let mut options = StatusRowVariables::default();
    options.base.extend([
        ("key_command".to_owned(), "snapshot-collision".to_owned()),
        ("status-left".to_owned(), "global".to_owned()),
    ]);
    for value in ["first", "second"] {
        execute(
            &mut engine,
            &mut execution,
            &["set-option", "-g", "@key_command", value],
        );
        let facts = FormatHookFacts {
            mux: Arc::new(engine.format_facts()),
            ..Default::default()
        };
        options.sessions.insert(
            context.session_id.clone(),
            BTreeMap::from([("status-left".to_owned(), value.to_owned())]),
        );
        let mut hooks =
            DaemonFormatHooks::command_with_optional_variables(&facts, Some(&variables));
        hooks.option_snapshot = Some(&options);
        assert!(hooks.stable_option_lookups());
        assert!(hooks.only_tmux_options());
        assert_eq!(hooks.option_variable("key_command", &context), None);
        assert_eq!(
            hooks.option_variable("status-left", &context),
            Some(value.to_owned()),
        );
        assert_eq!(
            zz_mux::expand_format_values("#{key_command}:#{status-left}", &context, &mut hooks),
            format!("tree-command:{value}"),
        );
        assert_eq!(
            zz_mux::expand_format_values("#{@key_command}:#{key_command}", &context, &mut hooks),
            format!("{value}:tree-command"),
        );
    }
}

fn forget_without_job_lock(mut renderer: StatusRenderer, client: ClientId) -> StatusRenderer {
    let needs = renderer.job_needs();
    let guard = needs.lock();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        renderer.forget(client);
        sender.send(renderer).unwrap();
    });
    let result = receiver.recv_timeout(std::time::Duration::from_secs(1));
    drop(guard);
    worker.join().unwrap();
    result.expect("forget attempted job cleanup for a client the renderer does not own")
}

#[test]
fn status_forget_never_rendered_clients_skips_state_and_job_lock_cleanup() {
    let (_, _, request) = completed_request("#{session_name}");
    let mut renderer = StatusRenderer::default();
    renderer.render_initial(&request);
    let published = renderer.published.clone();
    let completed = renderer.completed.as_ref().map(|entry| entry.client);
    let expansions = renderer.expansions;
    let command_client = ClientId(77);
    assert!(!renderer.owned_clients.contains(&command_client));
    let renderer = forget_without_job_lock(renderer, command_client);
    assert_eq!(renderer.published, published);
    assert_eq!(
        renderer.completed.as_ref().map(|entry| entry.client),
        completed
    );
    assert_eq!(renderer.expansions, expansions);
    assert_eq!(renderer.owned_clients, BTreeSet::from([request.client]));
    assert!(renderer.shell_cache.is_empty());
    assert!(renderer.uncovered_jobs.is_empty());
    assert!(renderer.job_needs.lock().is_empty());
}

#[test]
fn status_forget_high_watermark_preserves_out_of_order_and_reused_lower_ids() {
    let (_, _, mut request) = completed_request("#{session_name}");
    let renderer = forget_without_job_lock(StatusRenderer::default(), ClientId(0));
    assert!(renderer.owned_client_high_watermark.is_none());
    let mut renderer = renderer;
    for client in [900, 7, 800, 3] {
        request.client = ClientId(client);
        renderer.render_forced_at(&request, 1_700_000_000);
        assert_eq!(renderer.owned_client_high_watermark, Some(ClientId(900)));
    }
    for client in [901, 1_000, 0, 899] {
        renderer = forget_without_job_lock(renderer, ClientId(client));
    }
    assert_eq!(renderer.owned_clients.len(), 4);
    for client in [900, 7, 800, 3] {
        renderer.forget(ClientId(client));
        assert!(!renderer.owned_clients.contains(&ClientId(client)));
        assert!(!renderer.published.contains_key(&ClientId(client)));
        assert_eq!(renderer.owned_client_high_watermark, Some(ClientId(900)));
    }
    request.client = ClientId(2);
    renderer.render_forced_at(&request, 1_700_000_001);
    assert!(renderer.owned_clients.contains(&request.client));
    renderer.forget(request.client);
    assert!(renderer.owned_clients.is_empty());
    assert!(renderer.published.is_empty());
    assert!(renderer.completed.is_none());
}

#[test]
fn status_forget_cleans_rendered_clients_and_preserves_other_clients_and_jobs() {
    #[cfg(unix)]
    let job = "#(printf '\\043{P:first}\\n')";
    #[cfg(windows)]
    let job = "#(echo ##{P:first})";
    let (engine, context, mut first) = completed_request(job);
    first.context = Arc::new(
        engine
            .format_status_context(context.session, context.window, context.pane)
            .detach(FormatNeeds::ENVIRONMENT),
    );
    first.row_formats = Arc::new(BTreeMap::new());
    let mut second = first.clone();
    second.client = ClientId(2);
    let mut renderer = StatusRenderer::default();
    let _jobs = crate::daemon::status_jobs::tests::Driver::new(renderer.job_client());
    settled(&mut renderer, &first);
    settled(&mut renderer, &second);
    renderer.render_initial(&first);
    renderer.render_initial(&second);
    assert_eq!(renderer.shell_cache.len(), 2);
    assert_eq!(
        renderer.job_needs.lock().get(&first.client),
        Some(&FormatNeeds::PANES)
    );
    assert_eq!(
        renderer.job_needs.lock().get(&second.client),
        Some(&FormatNeeds::PANES)
    );
    let uncovered = !second.context.format_universe_covers(FormatNeeds::PANES);
    assert_eq!(renderer.uncovered_jobs.contains(&first.client), uncovered);
    assert_eq!(renderer.uncovered_jobs.contains(&second.client), uncovered);
    let (_, _, mut plain) = completed_request("#{session_name}");
    plain.client = second.client;
    renderer.render_initial(&plain);
    let published = renderer.published.get(&second.client).cloned();
    let completed = renderer.completed.as_ref().map(|entry| entry.client);
    let second_jobs = renderer
        .shell_cache
        .iter()
        .filter(|((client, _, _), _)| *client == second.client)
        .map(|(key, entry)| {
            (
                key.clone(),
                entry.expanded.clone(),
                entry.output.clone(),
                entry.output_needs,
            )
        })
        .collect::<Vec<_>>();
    renderer.forget(first.client);
    assert!(!renderer.owned_clients.contains(&first.client));
    assert!(renderer.owned_clients.contains(&second.client));
    assert!(!renderer.published.contains_key(&first.client));
    assert_eq!(renderer.published.get(&second.client).cloned(), published);
    assert_eq!(
        renderer.completed.as_ref().map(|entry| entry.client),
        completed
    );
    assert!(!renderer.job_needs.lock().contains_key(&first.client));
    assert_eq!(
        renderer.job_needs.lock().get(&second.client),
        Some(&FormatNeeds::PANES)
    );
    assert!(!renderer.uncovered_jobs.contains(&first.client));
    assert_eq!(renderer.uncovered_jobs.contains(&second.client), uncovered);
    let remaining_jobs = renderer
        .shell_cache
        .iter()
        .map(|(key, entry)| {
            (
                key.clone(),
                entry.expanded.clone(),
                entry.output.clone(),
                entry.output_needs,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(remaining_jobs, second_jobs);
    let renderer = forget_without_job_lock(renderer, first.client);
    assert_eq!(renderer.published.get(&second.client).cloned(), published);
    assert_eq!(renderer.shell_cache.len(), 1);
    assert_eq!(
        renderer.completed.as_ref().map(|entry| entry.client),
        completed
    );
}

#[test]
fn status_forget_cleans_rendered_clients_before_publication() {
    let request = request(9, "#(printf ready)", "");
    let mut renderer = StatusRenderer::default();
    let mut touched = BTreeSet::new();
    let _ = renderer.render_request(&request, &mut touched, false, 1_700_000_000, None);
    assert!(renderer.owned_clients.contains(&request.client));
    assert!(!renderer.published.contains_key(&request.client));
    assert_eq!(renderer.shell_cache.len(), 1);
    renderer.forget(request.client);
    assert!(renderer.owned_clients.is_empty());
    assert!(renderer.shell_cache.is_empty());
    assert!(renderer.published.is_empty());
    assert!(renderer.completed.is_none());
    assert!(renderer.job_needs.lock().is_empty());
    assert!(renderer.uncovered_jobs.is_empty());
}

#[test]
fn live_status_context_matches_model_snapshot_for_focused_missing_and_linked_targets() {
    let mut engine = MuxEngine::default();
    let mut execution = zz_mux::ExecutionContext::default();
    execute(&mut engine, &mut execution, &["new-session", "-s", "alpha"]);
    let alpha = execution.session.unwrap();
    let first = execution.window.unwrap();
    execute(
        &mut engine,
        &mut execution,
        &["new-window", "-n", "focused"],
    );
    let focused = execution.window.unwrap();
    execute(&mut engine, &mut execution, &["new-session", "-s", "beta"]);
    let beta = execution.session.unwrap();
    let other = execution.window.unwrap();
    let linked = engine.state.session_mut(beta).unwrap();
    linked.windows.push(first);
    linked.active_window = first;
    assert!(engine.state.sessions[&beta].windows.contains(&first));
    engine.set_format_now(1_700_000_000);
    let snapshot = engine.state.snapshot();
    let references = BTreeSet::from(["*".to_owned()]);
    for borrowed in [true, false] {
        zz_mux::with_borrowed_formats(borrowed, || {
            for (attached, window) in [
                (None, None),
                (None, Some(first)),
                (Some(alpha), None),
                (Some(alpha), Some(focused)),
                (Some(alpha), Some(other)),
                (Some(beta), None),
                (Some(beta), Some(first)),
                (Some(SessionId(999)), None),
                (Some(alpha), Some(WindowId(999))),
                (Some(SessionId(999)), Some(WindowId(999))),
            ] {
                let live = live_status_context(&engine, attached, window)
                    .detach_with_references(FormatNeeds::ALL, &references);
                let snapshotted = status_context(&snapshot, &engine, attached, window)
                    .detach_with_references(FormatNeeds::ALL, &references);
                assert!(
                    live.same_detached(&snapshotted),
                    "borrowed={borrowed} attached={attached:?} window={window:?}"
                );
            }
        });
    }
}

#[test]
fn snapshot_status_context_preserves_viewers_for_existing_snapshot_callers() {
    let mut engine = MuxEngine::default();
    let (session, window, _) = engine.state.create_session("viewers").unwrap();
    let (other, _) = engine
        .state
        .create_window(
            session,
            Some("other".to_owned()),
            zz_mux::PaneKind::Terminal,
        )
        .unwrap();
    let mut snapshot = engine.state.snapshot();
    snapshot.sessions[0].viewers = vec![
        zz_protocol::SessionViewer {
            name: "focused".to_owned(),
            window,
            is_self: true,
        },
        zz_protocol::SessionViewer {
            name: "other".to_owned(),
            window: other,
            is_self: false,
        },
    ];
    for borrowed in [true, false] {
        zz_mux::with_borrowed_formats(borrowed, || {
            let context = status_context(&snapshot, &engine, Some(session), Some(window));
            for (name, expected) in [
                ("session_attached", "2"),
                ("session_many_attached", "1"),
                ("session_attached_list", "focused,other"),
                ("window_active_clients", "1"),
                ("window_active_clients_list", "focused"),
            ] {
                assert_eq!(context.variable(name).unwrap(), expected, "{name}");
            }
        });
    }
}

#[test]
fn completed_status_reads_fresh_scoped_daemon_overrides_with_the_same_detached_context() {
    for (name, first, second) in [
        ("session_attached", "1", "2"),
        ("session_attached_list", "one", "two"),
        ("session_many_attached", "0", "1"),
        ("window_active_clients", "1", "2"),
        ("window_active_clients_list", "one", "two,other"),
    ] {
        for template in [format!("#{{{name}}}"), format!("#{{W:#{{{name}}}}}")] {
            let (_, _, mut request) = completed_request(&template);
            let session = request.context.session_id.parse().unwrap();
            let window = request.context.window_id.parse().unwrap();
            Arc::make_mut(&mut request.facts).session_attachments =
                Arc::new(BTreeMap::from([(session, (1, "one".to_owned()))]));
            Arc::make_mut(&mut request.facts).window_clients =
                Arc::new(BTreeMap::from([(window, vec!["one".to_owned()])]));
            let mut renderer = StatusRenderer::default();
            assert_eq!(
                renderer.render_forced_at(&request, 1_700_000_000).left,
                first
            );
            let retained = request.context.clone();
            Arc::make_mut(&mut request.facts).session_attachments =
                Arc::new(BTreeMap::from([(session, (2, "two".to_owned()))]));
            Arc::make_mut(&mut request.facts).window_clients = Arc::new(BTreeMap::from([(
                window,
                vec!["two".to_owned(), "other".to_owned()],
            )]));
            assert!(request.context.same_detached(&retained));
            assert_eq!(
                renderer.render_forced_at(&request, 1_700_000_000).left,
                second
            );
            assert_eq!(renderer.expansions, 2);
            assert!(renderer.completed.is_none(), "{template}");
        }
    }
}

#[test]
fn completed_status_twenty_windows_fit_the_bound_and_reuse_fresh_requests() {
    let cache_enabled = zz_mux::borrowed_formats_enabled();
    let mut engine = MuxEngine::default();
    let mut context = zz_mux::ExecutionContext::default();
    execute(&mut engine, &mut context, &["new-session", "-s", "twenty"]);
    for _ in 1..20 {
        execute(&mut engine, &mut context, &["new-window"]);
    }
    let mut request = engine_request(1, &engine, context.session);
    Arc::make_mut(&mut request.facts)
        .client
        .as_mut()
        .unwrap()
        .viewport = Some(ClientViewportFacts {
        columns: 180,
        rows: 50,
        window_width: 180,
        window_height: 49,
        cursor: None,
    });
    let mut renderer = StatusRenderer::default();
    let first = renderer.render_forced_at(&request, 1_700_000_000);
    if cache_enabled {
        let entry = renderer
            .completed
            .as_ref()
            .expect("twenty-window admission");
        let bytes = completed_status_bytes(
            &request,
            &entry.callback_names,
            &entry.callbacks,
            &entry.status,
        );
        println!("twenty-window completed status admission: {bytes} bytes");
        assert!(bytes <= COMPLETED_STATUS_MAX_BYTES);
    }
    let mut fresh = engine_request(1, &engine, context.session);
    Arc::make_mut(&mut fresh.facts)
        .client
        .as_mut()
        .unwrap()
        .viewport = Arc::make_mut(&mut request.facts)
        .client
        .as_ref()
        .unwrap()
        .viewport;
    Arc::make_mut(&mut fresh.facts)
        .client
        .as_mut()
        .unwrap()
        .written = "456".to_owned();
    assert_eq!(renderer.render_forced_at(&fresh, 1_700_000_000), first);
    assert_eq!(renderer.expansions, if cache_enabled { 1 } else { 2 });
}

#[test]
fn completed_status_does_not_retain_oversized_sources_callbacks_or_borders() {
    for kind in ["source", "callback", "border", "environment", "terminal"] {
        let (_, _, mut request) = completed_request("#{session_name}:#{client_prefix}");
        let large = "x".repeat(COMPLETED_STATUS_MAX_BYTES);
        match kind {
            "source" => *Arc::make_mut(&mut request.title_format) = Some(large),
            "callback" => {
                Arc::make_mut(&mut request.facts)
                    .client
                    .as_mut()
                    .unwrap()
                    .prefix = large;
            }
            "environment" => {
                request.environment = Arc::new(vec![("LARGE".into(), Some(large.into()))]);
            }
            "terminal" => request.default_terminal = Arc::new(large),
            _ => {
                Arc::make_mut(&mut request.pane_borders).push(
                    zz_protocol::PaneBorderPresentation {
                        pane: request.context.pane_id.parse().unwrap(),
                        style: large,
                    },
                );
            }
        }
        let mut renderer = StatusRenderer::default();
        let first = renderer.render_forced_at(&request, 1_700_000_000);
        assert!(renderer.completed.is_none(), "{kind}");
        assert_eq!(renderer.render_forced_at(&request, 1_700_000_000), first);
        assert!(renderer.completed.is_none(), "{kind}");
        assert_eq!(renderer.expansions, 2, "{kind}");
    }
}

fn completed_request(left: &str) -> (MuxEngine, zz_mux::ExecutionContext, StatusRequest) {
    let mut engine = MuxEngine::default();
    let mut context = zz_mux::ExecutionContext::default();
    execute(&mut engine, &mut context, &["new-session", "-s", "cached"]);
    execute(
        &mut engine,
        &mut context,
        &["set-option", "-g", "status-left", left],
    );
    execute(
        &mut engine,
        &mut context,
        &["set-option", "-g", "status-right", ""],
    );
    execute(
        &mut engine,
        &mut context,
        &["set-option", "-g", "status-left-length", "32767"],
    );
    let mut request = engine_request(1, &engine, context.session);
    Arc::make_mut(&mut request.formats).style.clear();
    Arc::make_mut(&mut request.formats).left_style.clear();
    Arc::make_mut(&mut request.formats).right_style.clear();
    Arc::make_mut(&mut request.formats).foreground = "default".to_owned();
    Arc::make_mut(&mut request.formats).background = "default".to_owned();
    (engine, context, request)
}

fn fresh_clock_request(engine: &MuxEngine, previous: &StatusRequest) -> StatusRequest {
    let mut request = engine_request(
        previous.client.0,
        engine,
        previous.context.session_id.parse().ok(),
    );
    if request.environment == previous.environment {
        request.environment = Arc::clone(&previous.environment);
    }
    if request.default_terminal == previous.default_terminal {
        request.default_terminal = Arc::clone(&previous.default_terminal);
    }
    request
}

fn whole_status(request: &StatusRequest, now: i64) -> StatusLine {
    render(
        &mut BTreeMap::new(),
        &mut BTreeSet::new(),
        request,
        false,
        now,
        None,
        None,
        None,
        &crate::daemon::status_jobs::StatusClient::default(),
        None,
    )
}

#[test]
fn completed_status_parts_reuse_twenty_window_loop_across_fresh_clock_requests() {
    let mut engine = MuxEngine::default();
    let mut execution = zz_mux::ExecutionContext::default();
    engine.set_format_now(1_700_000_000);
    execute(&mut engine, &mut execution, &["new-session", "-s", "parts"]);
    for _ in 1..20 {
        execute(&mut engine, &mut execution, &["new-window"]);
    }
    let mut request = engine_request(1, &engine, execution.session);
    let mut renderer = StatusRenderer::default();
    let first = renderer.render_forced_at(&request, 1_700_000_000);
    assert_eq!(first, whole_status(&request, 1_700_000_000));
    let parts = renderer
        .completed
        .as_ref()
        .and_then(|entry| entry.parts.clone());
    assert_eq!(parts.is_some(), status_parts_enabled());
    if let Some(parts) = &parts {
        assert!(parts.rows[&0].iter().any(|part| {
            part.value
                .as_ref()
                .and_then(OnceLock::get)
                .is_some_and(|value| {
                    value.contains("range=window|") && value.matches("range=window|").count() == 20
                })
        }));
    }
    for now in [1_700_000_060, 1_700_000_120] {
        engine.set_format_now(now);
        request = fresh_clock_request(&engine, &request);
        let next = renderer.render_forced_at(&request, i64::try_from(now).unwrap());
        assert_eq!(next.left, first.left);
        assert_ne!(next.right, first.right);
        assert_eq!(next, whole_status(&request, i64::try_from(now).unwrap()));
        if let Some(parts) = &parts {
            assert!(Arc::ptr_eq(
                parts,
                renderer.completed.as_ref().unwrap().parts.as_ref().unwrap()
            ));
        }
    }
}

#[test]
fn completed_status_parts_reuse_message_styles_and_recheck_facts_and_sources() {
    let (mut engine, mut execution, _) = completed_request("#{session_name}");
    for (name, value) in [
        ("message-style", "fg=#{?client_colours,green,red}"),
        ("message-command-style", "bg=#{?client_prefix,yellow,blue}"),
    ] {
        execute(
            &mut engine,
            &mut execution,
            &["set-option", "-g", name, value],
        );
    }
    engine.set_format_now(1_700_000_000);
    let first = engine_request(1, &engine, execution.session);
    let mut renderer = StatusRenderer::default();
    let original = renderer.render_forced_at(&first, 1_700_000_000);
    assert_eq!(original, whole_status(&first, 1_700_000_000));
    let parts = renderer
        .completed
        .as_ref()
        .and_then(|entry| entry.parts.clone());
    assert_eq!(parts.is_some(), status_parts_enabled());
    if let Some(parts) = &parts {
        assert_eq!(
            parts.message_styles.as_ref().and_then(OnceLock::get),
            Some(&(
                original.message_style.clone(),
                original.message_command_style.clone(),
            )),
        );
    }
    engine.set_format_now(1_700_000_001);
    let fresh = fresh_clock_request(&engine, &first);
    let next = renderer.render_forced_at(&fresh, 1_700_000_001);
    assert_eq!(next, whole_status(&fresh, 1_700_000_001));
    if let Some(parts) = &parts {
        assert!(Arc::ptr_eq(
            parts,
            renderer.completed.as_ref().unwrap().parts.as_ref().unwrap(),
        ));
    }
    for change in ["colours", "prefix", "source"] {
        let mut changed = fresh.clone();
        match change {
            "colours" => {
                Arc::make_mut(&mut changed.facts)
                    .client
                    .as_mut()
                    .unwrap()
                    .colours = "256".to_owned();
            }
            "prefix" => {
                Arc::make_mut(&mut changed.facts)
                    .client
                    .as_mut()
                    .unwrap()
                    .prefix = "1".to_owned();
            }
            _ => Arc::make_mut(&mut changed.message_styles).0 = "fg=blue".to_owned(),
        }
        let actual = renderer.render_forced_at(&changed, 1_700_000_001);
        assert_eq!(actual, whole_status(&changed, 1_700_000_001), "{change}");
        assert_ne!(
            (&actual.message_style, &actual.message_command_style),
            (&original.message_style, &original.message_command_style),
            "{change}",
        );
        if let Some(parts) = &parts {
            assert!(
                renderer
                    .completed
                    .as_ref()
                    .and_then(|entry| entry.parts.as_ref())
                    .is_none_or(|current| !Arc::ptr_eq(parts, current))
            );
        }
    }
}

#[test]
fn completed_status_parts_keep_timed_native_and_user_message_styles_fresh() {
    let (mut engine, mut execution, _) = completed_request("#{session_name}");
    execute(
        &mut engine,
        &mut execution,
        &["set-option", "-g", "@clock_style", "fg=colour%S"],
    );
    engine.set_format_now(1_700_000_000);
    let initial = engine_request(1, &engine, execution.session);
    for source in [
        "fg=colour%S",
        "#{E:window-status-style}",
        "#{T:window-status-style}",
        "#{E:@clock_style}",
        "#{T:@clock_style}",
    ] {
        let mut request = initial.clone();
        Arc::make_mut(&mut request.facts).mux = Arc::new(engine.format_facts());
        *Arc::make_mut(&mut request.message_styles) = (source.to_owned(), "bg=blue".to_owned());
        let snapshot = Arc::make_mut(&mut request.option_snapshot);
        snapshot
            .base
            .insert("window-status-style".to_owned(), "fg=colour%S".to_owned());
        for values in snapshot.windows.values_mut() {
            values.insert("window-status-style".to_owned(), "fg=colour%S".to_owned());
        }
        request.references = engine.cached_format_references_for_templates(status_line_templates(
            &request.formats,
            &request.row_formats,
            request.title_format.as_deref(),
            &request.message_styles,
        ));
        let mut renderer = StatusRenderer::default();
        let first = renderer.render_forced_at(&request, 1_700_000_000);
        assert_eq!(first, whole_status(&request, 1_700_000_000), "{source}");
        if let Some(parts) = renderer
            .completed
            .as_ref()
            .and_then(|entry| entry.parts.as_ref())
        {
            assert!(parts.message_styles.is_none(), "{source}");
        }
        let next = renderer.render_forced_at(&request, 1_700_000_001);
        assert_eq!(next, whole_status(&request, 1_700_000_001), "{source}");
        assert_ne!(next.message_style, first.message_style, "{source}");
    }
}

#[test]
fn completed_status_parts_reuse_static_theme_and_recheck_palette_inputs() {
    let (mut engine, execution, _) = completed_request("#{session_name}");
    engine.set_format_now(1_700_000_000);
    let first = engine_request(1, &engine, execution.session);
    let mut renderer = StatusRenderer::default();
    let original = renderer.render_forced_at(&first, 1_700_000_000);
    let parts = renderer
        .completed
        .as_ref()
        .and_then(|entry| entry.parts.clone());
    if let Some(parts) = &parts {
        assert_eq!(
            parts.theme.as_ref().and_then(OnceLock::get),
            Some(&original.theme)
        );
    }
    engine.set_format_now(1_700_000_001);
    let fresh = fresh_clock_request(&engine, &first);
    let next = renderer.render_forced_at(&fresh, 1_700_000_001);
    assert_eq!(next, whole_status(&fresh, 1_700_000_001));
    if let Some(parts) = &parts {
        assert!(Arc::ptr_eq(
            parts,
            renderer.completed.as_ref().unwrap().parts.as_ref().unwrap()
        ));
    }
    for change in ["colours", "scheme", "option", "context"] {
        let mut renderer = StatusRenderer::default();
        renderer.render_forced_at(&first, 1_700_000_000);
        let old = renderer
            .completed
            .as_ref()
            .and_then(|entry| entry.parts.clone());
        let mut changed = fresh.clone();
        match change {
            "colours" => {
                Arc::make_mut(&mut changed.facts)
                    .client
                    .as_mut()
                    .unwrap()
                    .colours = "256".to_owned();
            }
            "scheme" => changed.client_scheme = Some(TerminalColorScheme::Light),
            "option" => {
                Arc::make_mut(&mut changed.option_snapshot)
                    .base
                    .insert("dark-theme-green".to_owned(), "colour124".to_owned());
            }
            _ => Arc::make_mut(&mut changed.context).set_format_value("session_name", "changed"),
        }
        let actual = renderer.render_forced_at(&changed, 1_700_000_001);
        assert_eq!(actual, whole_status(&changed, 1_700_000_001), "{change}");
        if change == "option" {
            assert_eq!(actual.theme.slot(4), Some(TmuxColour::Indexed(124)));
        }
        if let Some(old) = old {
            assert!(
                renderer
                    .completed
                    .as_ref()
                    .and_then(|entry| entry.parts.as_ref())
                    .is_none_or(|parts| !Arc::ptr_eq(&old, parts)),
                "{change}"
            );
        }
    }
}

#[test]
fn completed_status_parts_keep_timed_and_user_theme_sources_fresh() {
    let (mut engine, mut execution, _) = completed_request("#{session_name}");
    execute(
        &mut engine,
        &mut execution,
        &["set-option", "-g", "@clock_colour", "colour%S"],
    );
    engine.set_format_now(1_700_000_000);
    let initial = engine_request(1, &engine, execution.session);
    for source in ["colour%S", "#{T:@clock_colour}", "#{E:@clock_colour}"] {
        let mut first = initial.clone();
        Arc::make_mut(&mut first.facts).mux = Arc::new(engine.format_facts());
        Arc::make_mut(&mut first.option_snapshot)
            .base
            .insert("dark-theme-green".to_owned(), source.to_owned());
        let mut renderer = StatusRenderer::default();
        let first_status = renderer.render_forced_at(&first, 1_700_000_000);
        assert_eq!(
            first_status,
            whole_status(&first, 1_700_000_000),
            "{source}"
        );
        if let Some(parts) = renderer
            .completed
            .as_ref()
            .and_then(|entry| entry.parts.as_ref())
        {
            assert!(parts.theme.is_none(), "{source}");
        }
        let next = renderer.render_forced_at(&first, 1_700_000_001);
        assert_eq!(next, whole_status(&first, 1_700_000_001), "{source}");
        assert_ne!(next.theme, first_status.theme, "{source}");
    }
}

#[test]
fn completed_status_parts_keep_nested_timed_options_and_data_percent_text_fresh() {
    for value in ["fg=colour%S", "#{t/d:session_created}", "#{E:@timed}"] {
        let (mut engine, mut execution, _) = completed_request("#{session_name}");
        engine.set_format_now(1_700_000_000);
        engine
            .state
            .session_mut(execution.session.unwrap())
            .unwrap()
            .created = Some(1_700_000_000);
        execute(
            &mut engine,
            &mut execution,
            &["set-option", "-g", "@timed", "%S"],
        );
        execute(
            &mut engine,
            &mut execution,
            &["set-option", "-g", "window-status-format", value],
        );
        execute(
            &mut engine,
            &mut execution,
            &["set-option", "-g", "window-status-current-format", value],
        );
        let mut request = engine_request(1, &engine, execution.session);
        let mut renderer = StatusRenderer::default();
        renderer.render_forced_at(&request, 1_700_000_000);
        engine.set_format_now(1_700_000_001);
        request = fresh_clock_request(&engine, &request);
        let next = renderer.render_forced_at(&request, 1_700_000_001);
        assert_eq!(next, whole_status(&request, 1_700_000_001), "{value}");
        if let Some(parts) = renderer
            .completed
            .as_ref()
            .and_then(|entry| entry.parts.as_ref())
        {
            assert!(
                parts.rows[&0]
                    .iter()
                    .filter(|part| { request.row_formats[&0][part.range.clone()].contains("#{W:") })
                    .all(|part| part.value.is_none()),
                "{value}"
            );
        }
    }
    let (mut engine, mut execution, _) = completed_request("#{session_name}");
    execute(&mut engine, &mut execution, &["rename-session", "%S"]);
    engine.set_format_now(1_700_000_000);
    let first = engine_request(1, &engine, execution.session);
    let mut renderer = StatusRenderer::default();
    assert!(
        renderer
            .render_forced_at(&first, 1_700_000_000)
            .left
            .contains("%S")
    );
    engine.set_format_now(1_700_000_001);
    let next = fresh_clock_request(&engine, &first);
    assert!(
        renderer
            .render_forced_at(&next, 1_700_000_001)
            .left
            .contains("%S")
    );
}

#[test]
fn completed_status_option_size_memo_requires_retained_arc_identity() {
    let (mut engine, _, request) = completed_request("#{session_name}");
    engine.set_format_now(1_700_000_000);
    let mut renderer = StatusRenderer::default();
    renderer.render_forced_at(&request, 1_700_000_000);
    let memo = renderer.completed.as_mut().map(|entry| {
        entry.option_bytes = entry.option_bytes.saturating_add(128);
        entry.option_bytes
    });
    renderer.render_forced_at(&request, 1_700_000_001);
    if let Some(memo) = memo {
        assert_eq!(renderer.completed.as_ref().unwrap().option_bytes, memo);
    }
    let mut changed = request.clone();
    Arc::make_mut(&mut changed.option_snapshot).base.insert(
        "@giant-unused".to_owned(),
        "x".repeat(COMPLETED_STATUS_MAX_BYTES),
    );
    assert!(!Arc::ptr_eq(
        &request.option_snapshot,
        &changed.option_snapshot
    ));
    let next = renderer.render_forced_at(&changed, 1_700_000_002);
    assert!(renderer.completed.is_none());
    let mut oracle = StatusRenderer::default();
    assert_eq!(next, oracle.render_forced_at(&changed, 1_700_000_002));
}

#[test]
fn completed_status_context_size_memo_reuses_clock_capture_and_recounts_mutations() {
    let cache_enabled = zz_mux::borrowed_formats_enabled();
    let (mut engine, mut execution, _) = completed_request("#{session_name}");
    execute(
        &mut engine,
        &mut execution,
        &["set-option", "-g", "status-right", "%S"],
    );
    engine.set_format_now(1_700_000_000);
    let first = engine_request(1, &engine, execution.session);
    let capacities = |context: &StatusContext| {
        [
            context.session_id.capacity(),
            context.window_id.capacity(),
            context.pane_id.capacity(),
        ]
    };
    let first_bytes = first.context.retained_bytes();
    let mut renderer = StatusRenderer::default();
    let first_status = renderer.render_forced_at(&first, 1_700_000_000);
    let memo = renderer.completed.as_mut().map(|entry| {
        entry.context_bytes = entry.context_bytes.saturating_add(128);
        entry.context_bytes
    });
    engine.set_format_now(1_700_000_001);
    let fresh = fresh_clock_request(&engine, &first);
    let next = renderer.render_forced_at(&fresh, 1_700_000_001);
    assert_eq!(next, whole_status(&fresh, 1_700_000_001));
    assert_ne!(next.right, first_status.right);
    if cache_enabled {
        assert!(!Arc::ptr_eq(&first.context, &fresh.context));
        assert!(first.context.same_detached_data(&fresh.context));
        assert_eq!(
            fresh.context.retained_bytes() + capacities(&first.context).into_iter().sum::<usize>(),
            first_bytes + capacities(&fresh.context).into_iter().sum::<usize>(),
        );
        assert_eq!(
            renderer.completed.as_ref().unwrap().context_bytes,
            if capacities(&first.context) == capacities(&fresh.context) {
                memo.unwrap()
            } else {
                fresh.context.retained_bytes()
            },
        );
    }
    let memo = renderer.completed.as_mut().map(|entry| {
        entry.context_bytes = entry.context_bytes.saturating_add(128);
        entry.context_bytes
    });
    engine.set_format_now(1_700_000_002);
    let stable = fresh_clock_request(&engine, &fresh);
    let next = renderer.render_forced_at(&stable, 1_700_000_002);
    assert_eq!(next, whole_status(&stable, 1_700_000_002));
    if cache_enabled {
        assert!(fresh.context.same_detached_data(&stable.context));
        assert_eq!(capacities(&fresh.context), capacities(&stable.context));
        assert_eq!(
            renderer.completed.as_ref().unwrap().context_bytes,
            memo.unwrap()
        );
    }
    for change in ["session", "window", "pane", "value", "legacy"] {
        let mut renderer = StatusRenderer::default();
        renderer.render_forced_at(&fresh, 1_700_000_001);
        assert_eq!(renderer.completed.is_some(), cache_enabled);
        let mut changed = fresh.clone();
        match change {
            "session" => Arc::make_mut(&mut changed.context)
                .session_id
                .reserve_exact(COMPLETED_STATUS_MAX_BYTES),
            "window" => Arc::make_mut(&mut changed.context)
                .window_id
                .reserve_exact(COMPLETED_STATUS_MAX_BYTES),
            "pane" => Arc::make_mut(&mut changed.context)
                .pane_id
                .reserve_exact(COMPLETED_STATUS_MAX_BYTES),
            "value" => Arc::make_mut(&mut changed.context)
                .set_format_value("unused_extra", "x".repeat(COMPLETED_STATUS_MAX_BYTES)),
            _ => {
                let _: &zz_mux::StatusValues = &changed.context;
            }
        }
        if cache_enabled {
            assert_eq!(
                fresh.context.same_detached_data(&changed.context),
                matches!(change, "session" | "window" | "pane"),
                "{change}",
            );
        }
        let actual = renderer.render_forced_at(&changed, 1_700_000_002);
        assert_eq!(actual, whole_status(&changed, 1_700_000_002), "{change}");
        assert!(renderer.completed.is_none(), "{change}");
    }
}

#[test]
fn status_parts_preserve_malformed_escape_style_and_whole_source_strftime_order() {
    for source in [
        "start#{==:broken}tail",
        "start#{?session_name}tail",
        "before#{R:broken}after",
        "before#{W:#{session_name}}after",
        "##{session_name}#{session_name}",
        "#[fg=#{?session_name,red,green}]#{session_name}",
        "#{t/f/%S:session_created}suffix",
    ] {
        let (_, _, request) = completed_request(source);
        let parts = StatusParts::new(&request);
        let mut hooks = DaemonFormatHooks::command(request.facts.as_ref());
        hooks.option_snapshot = Some(&request.option_snapshot);
        hooks.now = 1_700_000_000;
        let original = expand_status(source, &request.context, &mut hooks);
        let split = expand_status_parts(source, &request.context, &mut hooks, Some(&parts.left));
        assert_eq!(split, original, "{source}");
    }
}

#[test]
fn status_parts_keep_missing_loop_context_and_adjacent_raw_environment_whole() {
    for source in [
        "before#{W:#{session_name}}after",
        "before#{P:#{pane_id}}after",
    ] {
        let mut request = request(1, source, "");
        request.context = Arc::new(StatusContext::default());
        let parts = StatusParts::new(&request);
        assert_eq!(parts.left.len(), 1);
        assert!(parts.left[0].value.is_none());
        let mut hooks = DaemonFormatHooks::command(request.facts.as_ref());
        let expected = expand_status(source, &request.context, &mut hooks);
        assert_eq!(
            expand_status_parts(source, &request.context, &mut hooks, Some(&parts.left)),
            expected,
        );
    }
    let (mut engine, execution, _) = completed_request("x#{FIRST}#{SECOND}|#[#{FIRST}#{SECOND}]");
    engine.seed_global_environment([
        ("FIRST", RawText::from_bytes(vec![0xc3])),
        ("SECOND", RawText::from_bytes(vec![0xa9])),
    ]);
    let raw_request = engine_request(1, &engine, execution.session);
    let mut renderer = StatusRenderer::default();
    let actual = renderer.render_forced_at(&raw_request, 1_700_000_000);
    assert_eq!(actual, whole_status(&raw_request, 1_700_000_000));
    assert!(actual.left.contains("xé"));
    let large_source = format!("{}#{{session_name}}", "x".repeat(8192));
    let request = request(1, &large_source, "");
    let parts = StatusParts::new(&request);
    assert_eq!(parts.left.len(), 1);
    assert!(parts.left[0].value.is_none());
}

#[test]
fn completed_status_parts_recheck_facts_layout_targets_modes_and_sources() {
    let (mut engine, execution, _) =
        completed_request("#{session_name}:#{client_prefix}:#{client_width}");
    engine.set_format_now(1_700_000_000);
    let initial = engine_request(1, &engine, execution.session);
    engine.set_format_now(1_700_000_001);
    let fresh = fresh_clock_request(&engine, &initial);
    for change in [
        "facts",
        "length",
        "target",
        "mode",
        "environment",
        "terminal",
        "template",
        "message",
        "client",
        "border",
    ] {
        let mut renderer = StatusRenderer::default();
        renderer.render_forced_at(&initial, 1_700_000_000);
        let previous = renderer
            .completed
            .as_ref()
            .and_then(|entry| entry.parts.clone());
        let mut next = fresh.clone();
        match change {
            "facts" => {
                Arc::make_mut(&mut next.facts)
                    .client
                    .as_mut()
                    .unwrap()
                    .prefix = "1".to_owned();
            }
            "length" => Arc::make_mut(&mut next.formats).left_length = 2,
            "target" => {
                Arc::make_mut(&mut next.context).set_format_value("session_name", "changed");
            }
            "mode" => next.modes.push(ModeRequest {
                pane: next.context.pane_id.parse().unwrap(),
                view: false,
                context: next.context.as_ref().clone(),
                position: 0,
                limit: 0,
                vi_keys: false,
                line_numbers: 0,
                hide_position: false,
                rows: 0,
            }),
            "environment" => next.environment = Arc::new(next.environment.as_ref().clone()),
            "terminal" => next.default_terminal = Arc::new(next.default_terminal.as_ref().clone()),
            "template" => Arc::make_mut(&mut next.formats).left.push('!'),
            "message" => next.message_line = next.message_line.saturating_add(1),
            "client" => next.client = ClientId(2),
            _ => Arc::make_mut(&mut next.pane_borders).push(zz_protocol::PaneBorderPresentation {
                pane: next.context.pane_id.parse().unwrap(),
                style: "fg=red".to_owned(),
            }),
        }
        let actual = renderer.render_forced_at(&next, 1_700_000_001);
        assert_eq!(actual, whole_status(&next, 1_700_000_001), "{change}");
        if let Some(previous) = previous {
            assert!(
                renderer
                    .completed
                    .as_ref()
                    .and_then(|entry| entry.parts.as_ref())
                    .is_none_or(|parts| !Arc::ptr_eq(&previous, parts)),
                "{change}"
            );
        }
    }
}

#[test]
fn completed_status_shared_request_identity_keeps_clock_and_mutations_fresh() {
    let cache_enabled = zz_mux::borrowed_formats_enabled();
    let (_, _, request) = completed_request("#{session_name}:#{client_prefix}:%S");
    let mut request = Arc::new(request);
    let mut renderer = StatusRenderer::default();
    let mut wire = renderer.render_forced_shared_at(&request, 1_700_000_000);
    let first = wire.clone();
    wire.left = "wire-only".to_owned();
    assert_eq!(
        renderer.render_forced_shared_at(&request, 1_700_000_000),
        first
    );
    assert_eq!(renderer.expansions, if cache_enabled { 1 } else { 2 });
    request = Arc::new(request.as_ref().clone());
    assert_eq!(
        renderer.render_forced_shared_at(&request, 1_700_000_000),
        first
    );
    assert_eq!(renderer.expansions, if cache_enabled { 1 } else { 3 });
    if cache_enabled {
        assert_eq!(
            renderer
                .completed
                .as_ref()
                .unwrap()
                .request_identity
                .as_ptr(),
            Arc::as_ptr(&request),
        );
    }
    let tick = renderer.render_forced_shared_at(&request, 1_700_000_001);
    assert_ne!(tick.left, first.left);
    let old_request = Arc::clone(&request);
    let old_published = Arc::clone(renderer.published.get(&request.client).unwrap());
    Arc::make_mut(&mut Arc::make_mut(&mut request).facts)
        .client
        .as_mut()
        .unwrap()
        .prefix = "1".to_owned();
    assert!(!Arc::ptr_eq(&old_request, &request));
    let changed = renderer.render_forced_shared_at(&request, 1_700_000_001);
    assert!(changed.left.starts_with("cached:1:"));
    assert_eq!(old_request.facts.client.as_ref().unwrap().prefix, "");
    assert_eq!(old_published.left, tick.left);
    Arc::make_mut(&mut Arc::make_mut(&mut request).context)
        .set_format_value("session_name", "fresh");
    assert!(
        renderer
            .render_forced_shared_at(&request, 1_700_000_001)
            .left
            .starts_with("fresh:1:"),
    );
    let pane = request.context.pane_id.parse().unwrap();
    let context = request.context.as_ref().clone();
    Arc::make_mut(&mut request).modes.push(ModeRequest {
        pane,
        view: false,
        context,
        position: 0,
        limit: 0,
        vi_keys: false,
        line_numbers: 0,
        hide_position: false,
        rows: 0,
    });
    assert_eq!(
        renderer
            .render_forced_shared_at(&request, 1_700_000_001)
            .modes
            .len(),
        1,
    );
    assert!(renderer.completed.is_none());
    let expansions = renderer.expansions;
    renderer.render_forced_shared_at(&request, 1_700_000_001);
    assert_eq!(renderer.expansions, expansions + 1);
}

#[test]
fn completed_status_shared_request_make_mut_disassociates_weak_identity() {
    let cache_enabled = zz_mux::borrowed_formats_enabled();
    let (_, _, request) = completed_request("#{session_name}");
    let mut request = Arc::new(request);
    let mut renderer = StatusRenderer::default();
    assert_eq!(
        renderer
            .render_forced_shared_at(&request, 1_700_000_000)
            .left,
        "cached"
    );
    let prior_identity = renderer
        .completed
        .as_ref()
        .map(|completed| completed.request_identity.clone());
    assert_eq!(Arc::strong_count(&request), 1);
    Arc::make_mut(&mut Arc::make_mut(&mut request).formats).left = "changed".to_owned();
    if cache_enabled {
        assert_ne!(prior_identity.unwrap().as_ptr(), Arc::as_ptr(&request));
    }
    assert_eq!(
        renderer
            .render_forced_shared_at(&request, 1_700_000_000)
            .left,
        "changed"
    );
    assert_eq!(renderer.expansions, 2);
}

#[test]
fn completed_status_shared_request_bypasses_unsafe_and_forced_job_formats() {
    for template in ["#{cursor_x}", "#{copy_cursor_line}", "#{agent_state}"] {
        let (_, _, request) = completed_request(template);
        let request = Arc::new(request);
        let mut renderer = StatusRenderer::default();
        renderer.render_forced_shared_at(&request, 1_700_000_000);
        renderer.render_forced_shared_at(&request, 1_700_000_000);
        assert_eq!(renderer.expansions, 2, "{template}");
        assert!(renderer.completed.is_none(), "{template}");
    }
    let directory = tempfile::tempdir().expect("shared forced job fixture");
    let count = directory.path().join("count");
    let format = counted_value_job(&count);
    let request = Arc::new(request(1, &format, ""));
    let mut renderer = StatusRenderer::default();
    let _jobs = crate::daemon::status_jobs::tests::Driver::new(renderer.job_client());
    assert_eq!(settled(&mut renderer, &request).left, "value");
    let runs = std::fs::read_to_string(&count).unwrap().lines().count();
    renderer.render_forced_shared(&request);
    assert_eq!(settled(&mut renderer, &request).left, "value");
    assert!(std::fs::read_to_string(&count).unwrap().lines().count() > runs);
    assert!(renderer.completed.is_none());
}

#[test]
fn completed_status_reuses_forced_output_and_ignores_unreferenced_client_fields() {
    let cache_enabled = zz_mux::borrowed_formats_enabled();
    let (_, _, mut request) = completed_request("#{session_name}:#{client_prefix}");
    let mut renderer = StatusRenderer::default();
    let first = renderer.render_forced_at(&request, 1_700_000_000);
    assert_eq!(first.left, "cached:");
    assert_eq!(
        renderer.completed.is_some(),
        cache_enabled,
        "{:?}",
        request.references
    );
    renderer.published.clear();
    Arc::make_mut(&mut request.facts)
        .client
        .as_mut()
        .unwrap()
        .written = "999".to_owned();
    Arc::make_mut(&mut request.facts)
        .client
        .as_mut()
        .unwrap()
        .activity = "123".to_owned();
    assert_eq!(renderer.render_forced_at(&request, 1_700_000_000), first);
    assert_eq!(
        renderer.published.get(&request.client).map(Arc::as_ref),
        Some(&first),
    );
    assert_eq!(renderer.expansions, if cache_enabled { 1 } else { 2 });
    renderer.forget(request.client);
    assert!(renderer.completed.is_none());
    assert_eq!(renderer.render_forced_at(&request, 1_700_000_000), first);
    assert_eq!(renderer.expansions, if cache_enabled { 2 } else { 3 });
}

#[test]
fn completed_status_shares_internal_output_and_keeps_wire_and_mode_updates_independent() {
    let cache_enabled = zz_mux::borrowed_formats_enabled();
    let (_, _, request) = completed_request("#{session_name}");
    let mut renderer = StatusRenderer::default();
    let mut first = renderer.render_forced_at(&request, 1_700_000_000);
    let retained = Arc::clone(renderer.published.get(&request.client).unwrap());
    if cache_enabled {
        assert!(Arc::ptr_eq(
            &retained,
            &renderer.completed.as_ref().unwrap().status,
        ));
    }
    first.left = "wire-only change".to_owned();
    assert_eq!(
        renderer.render_forced_at(&request, 1_700_000_000).left,
        "cached",
    );
    assert_eq!(
        Arc::ptr_eq(&retained, renderer.published.get(&request.client).unwrap()),
        cache_enabled,
    );
    assert_eq!(renderer.expansions, if cache_enabled { 1 } else { 2 });
    let modes = vec![zz_protocol::ModePresentation {
        pane: request.context.pane_id.parse().unwrap(),
        view: false,
        position: "[1/2]".to_owned(),
        position_style: "fg=red".to_owned(),
        selection_style: String::new(),
        vi_keys: false,
        match_style: String::new(),
        current_match_style: String::new(),
        line_numbers: 0,
        line_number_style: String::new(),
        current_line_number_style: String::new(),
    }];
    let updated = renderer
        .republish_modes(request.client, modes.clone())
        .unwrap();
    assert_eq!(updated.modes, modes);
    assert!(retained.modes.is_empty());
    assert_eq!(retained.left, "cached");
    assert!(renderer.completed.is_none());
    assert_eq!(
        renderer.published.get(&request.client).unwrap().modes,
        modes,
    );
    assert!(renderer.republish_modes(request.client, modes).is_none());
    assert!(
        renderer
            .render_forced_at(&request, 1_700_000_000)
            .modes
            .is_empty(),
    );
}

#[test]
fn completed_status_invalidates_referenced_client_context_option_and_time_inputs() {
    let (mut engine, mut context, _) =
        completed_request("#{status-interval}:#{session_name}:#{client_prefix}:%S");
    execute(
        &mut engine,
        &mut context,
        &["set-option", "-g", "status-interval", "20"],
    );
    let mut request = engine_request(1, &engine, context.session);
    Arc::make_mut(&mut request.formats).style.clear();
    Arc::make_mut(&mut request.formats).left_style.clear();
    Arc::make_mut(&mut request.formats).right_style.clear();
    Arc::make_mut(&mut request.formats).foreground = "default".to_owned();
    Arc::make_mut(&mut request.formats).background = "default".to_owned();
    let mut renderer = StatusRenderer::default();
    assert!(
        renderer
            .render_forced_at(&request, 1_700_000_000)
            .left
            .starts_with("20:cached::")
    );
    let initial = renderer.expansions;
    Arc::make_mut(&mut request.facts)
        .client
        .as_mut()
        .unwrap()
        .prefix = "1".to_owned();
    assert!(
        renderer
            .render_forced_at(&request, 1_700_000_000)
            .left
            .starts_with("20:cached:1:")
    );
    assert_eq!(renderer.expansions, initial + 1);
    Arc::make_mut(&mut request.context).set_format_value("session_name", "changed");
    assert!(
        renderer
            .render_forced_at(&request, 1_700_000_000)
            .left
            .starts_with("20:changed:1:")
    );
    assert_eq!(renderer.expansions, initial + 2);
    execute(
        &mut engine,
        &mut context,
        &["set-option", "-g", "status-interval", "30"],
    );
    request.option_snapshot = engine.cached_format_option_snapshot();
    let before_clock = renderer.render_forced_at(&request, 1_700_000_000);
    assert!(before_clock.left.starts_with("30:changed:1:"));
    assert_eq!(renderer.expansions, initial + 3);
    assert_ne!(
        renderer.render_forced_at(&request, 1_700_000_001).left,
        before_clock.left
    );
    assert_eq!(renderer.expansions, initial + 4);
}

#[test]
fn completed_status_rejects_terminal_mode_and_unknown_dependencies() {
    for template in [
        "#{cursor_x}",
        "#{copy_cursor_line}",
        "#{agent_state}",
        "#{message_text}",
    ] {
        let (_, _, request) = completed_request(template);
        let mut renderer = StatusRenderer::default();
        renderer.render_forced_at(&request, 1_700_000_000);
        renderer.render_forced_at(&request, 1_700_000_000);
        assert_eq!(renderer.expansions, 2, "{template}");
        assert!(renderer.completed.is_none(), "{template}");
    }
    let mut legacy = request(1, "#{MISSING_ENV}", "");
    legacy.option_snapshot = MuxEngine::default().cached_format_option_snapshot();
    legacy.references =
        MuxEngine::default().cached_format_references_for_templates(status_line_templates(
            &legacy.formats,
            &legacy.row_formats,
            legacy.title_format.as_deref(),
            &legacy.message_styles,
        ));
    let mut renderer = StatusRenderer::default();
    renderer.render_forced_at(&legacy, 1_700_000_000);
    renderer.render_forced_at(&legacy, 1_700_000_000);
    assert_eq!(renderer.expansions, 2);
    assert!(renderer.completed.is_none());
    let (_, context, mut request) = completed_request("#{cursor_x}");
    let terminal = Arc::new(TerminalSession::spawn_empty_with_appearance(
        64,
        Arc::new(zz_terminal::TerminalAppearance::default()),
    ));
    Arc::make_mut(&mut request.facts).terminals = Arc::new(BTreeMap::from([(
        context.pane.unwrap(),
        Arc::clone(&terminal),
    )]));
    let mut renderer = StatusRenderer::default();
    assert_eq!(renderer.render_forced_at(&request, 1_700_000_000).left, "0");
    assert!(terminal.feed(Arc::from(b"abc".as_slice())));
    terminal
        .capture(zz_terminal::CaptureOptions::default())
        .unwrap();
    assert_eq!(renderer.render_forced_at(&request, 1_700_000_000).left, "3");
    assert_eq!(renderer.expansions, 2);
    let (_, _, mut request) = completed_request("#{session_name}");
    request.modes.push(ModeRequest {
        pane: request.context.pane_id.parse().unwrap(),
        view: false,
        context: request.context.as_ref().clone(),
        position: 0,
        limit: 0,
        vi_keys: false,
        line_numbers: 0,
        hide_position: false,
        rows: 0,
    });
    let mut renderer = StatusRenderer::default();
    assert_eq!(
        renderer
            .render_forced_at(&request, 1_700_000_000)
            .modes
            .len(),
        1
    );
    renderer.render_forced_at(&request, 1_700_000_000);
    assert_eq!(renderer.expansions, 2);
    assert!(renderer.completed.is_none());
}

#[test]
fn completed_status_cache_keeps_forced_shell_jobs_running() {
    let directory = tempfile::tempdir().expect("forced job fixture");
    let count = directory.path().join("count");
    let format = counted_value_job(&count);
    let mut renderer = StatusRenderer::default();
    let _jobs = crate::daemon::status_jobs::tests::Driver::new(renderer.job_client());
    let request = request(1, &format, "");
    assert_eq!(settled(&mut renderer, &request).left, "value");
    let runs = std::fs::read_to_string(&count).unwrap().lines().count();
    renderer.render_forced(&request);
    assert_eq!(settled(&mut renderer, &request).left, "value");
    assert!(std::fs::read_to_string(&count).unwrap().lines().count() > runs);
    assert!(renderer.completed.is_none());
}

#[test]
fn completed_status_invalidates_captured_global_and_session_environment_values() {
    let cache_enabled = zz_mux::borrowed_formats_enabled();
    let (mut engine, mut context, _) = completed_request("#{CACHE_ENV}");
    let mut renderer = StatusRenderer::default();
    for (args, expected) in [
        (
            vec!["set-environment", "-g", "CACHE_ENV", "global"],
            "global",
        ),
        (vec!["set-environment", "CACHE_ENV", "session"], "session"),
        (vec!["set-environment", "-u", "CACHE_ENV"], "global"),
        (
            vec!["set-environment", "-g", "CACHE_ENV", "changed"],
            "changed",
        ),
    ] {
        execute(&mut engine, &mut context, &args);
        let mut request = engine_request(1, &engine, context.session);
        Arc::make_mut(&mut request.formats).style.clear();
        Arc::make_mut(&mut request.formats).left_style.clear();
        Arc::make_mut(&mut request.formats).right_style.clear();
        Arc::make_mut(&mut request.formats).foreground = "default".to_owned();
        Arc::make_mut(&mut request.formats).background = "default".to_owned();
        assert_eq!(
            renderer.render_forced_at(&request, 1_700_000_000).left,
            expected
        );
        let expansions = renderer.expansions;
        assert_eq!(
            renderer.render_forced_at(&request, 1_700_000_000).left,
            expected
        );
        assert_eq!(
            renderer.expansions,
            expansions + usize::from(!cache_enabled)
        );
    }
}

#[test]
fn completed_status_rejects_stale_references_after_unsafe_template_changes() {
    let (_, _, mut request) = completed_request("#{session_name}");
    let mut renderer = StatusRenderer::default();
    let _jobs = crate::daemon::status_jobs::tests::Driver::new(renderer.job_client());
    assert_eq!(
        renderer.render_forced_at(&request, 1_700_000_000).left,
        "cached"
    );
    Arc::make_mut(&mut request.formats).left = "#{C:#{client_name}}".to_owned();
    renderer.render_forced_at(&request, 1_700_000_000);
    assert!(renderer.completed.is_none());
    let directory = tempfile::tempdir().expect("stale references job fixture");
    let count = directory.path().join("count");
    Arc::make_mut(&mut request.formats).left = counted_value_job(&count);
    assert_eq!(settled(&mut renderer, &request).left, "value");
    let runs = std::fs::read_to_string(&count).unwrap().lines().count();
    renderer.render_forced(&request);
    assert_eq!(settled(&mut renderer, &request).left, "value");
    assert!(std::fs::read_to_string(&count).unwrap().lines().count() > runs);
    assert!(renderer.completed.is_none());
}

#[test]
fn status_dependencies_capture_only_rows_that_are_rendered() {
    let mut engine = MuxEngine::default();
    let mut execution = zz_mux::ExecutionContext::default();
    execute(
        &mut engine,
        &mut execution,
        &["new-session", "-s", "work", "-n", "main"],
    );
    for (index, template) in [
        ("status-format[1]", "#{P:#{pane_height}}"),
        ("status-format[2]", "#{S:#{session_stack}}"),
    ] {
        execute(
            &mut engine,
            &mut execution,
            &["set-option", "-g", index, template],
        );
    }
    let session = execution.session;
    let title = "title #{host}";
    let mut generation = engine.format_options_generation();
    for (status, panes, sessions) in [
        ("1", false, false),
        ("2", true, false),
        ("3", true, true),
        ("off", false, false),
    ] {
        execute(
            &mut engine,
            &mut execution,
            &["set-option", "-g", "status", status],
        );
        assert_ne!(engine.format_options_generation(), generation);
        generation = engine.format_options_generation();
        let formats = engine.status_formats_for_session(session);
        let rows = engine.status_format_array_for_session(session);
        let styles = engine.message_styles_for_session(session);
        let templates =
            status_line_templates(&formats, &rows, Some(title), &styles).collect::<Vec<_>>();
        assert!(templates.contains(&title));
        assert!(templates.contains(&styles.0.as_str()));
        assert!(templates.contains(&styles.1.as_str()));
        assert!(templates.contains(&"#{theme}"));
        assert!(templates.contains(&"#{socket_path}:#{session_path}:#{pane_current_path}"));
        let needs = status_line_needs(&engine, &formats, &rows, Some(title), &styles);
        assert_eq!(needs, engine.format_needs(templates.iter().copied()));
        assert_eq!(needs.contains(FormatNeeds::PANES), panes, "{status}");
        assert_eq!(needs.contains(FormatNeeds::SESSIONS), sessions, "{status}");
        let mut references = BTreeSet::new();
        for template in &templates {
            references.extend(engine.cached_format_references(template).iter().cloned());
        }
        assert_eq!(references.contains("pane_height"), panes, "{status}");
        assert_eq!(references.contains("session_stack"), sessions, "{status}");
        let context = zz_mux::with_borrowed_formats(true, || {
            engine
                .format_status_context(session, execution.window, execution.pane)
                .detach_with_templates(needs, templates)
        });
        assert_eq!(
            context.format_universe_covers(FormatNeeds::PANES),
            panes,
            "{status}"
        );
        assert_eq!(
            context.format_universe_covers(FormatNeeds::SESSIONS),
            sessions,
            "{status}"
        );
    }
}
