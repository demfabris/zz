use super::tests::{engine_request, execute, request, settled};
use super::*;

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
    let cache_enabled = zz_mux::format_cache_knob() && zz_mux::borrowed_formats_enabled();
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
    for kind in ["source", "callback", "border"] {
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
            _ => {
                request
                    .pane_borders
                    .push(zz_protocol::PaneBorderPresentation {
                        pane: request.context.pane_id.parse().unwrap(),
                        style: large,
                    });
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

#[test]
fn completed_status_reuses_forced_output_and_ignores_unreferenced_client_fields() {
    let cache_enabled = zz_mux::format_cache_knob() && zz_mux::borrowed_formats_enabled();
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
    assert_eq!(renderer.published.get(&request.client), Some(&first));
    assert_eq!(renderer.expansions, if cache_enabled { 1 } else { 2 });
    renderer.forget(request.client);
    assert!(renderer.completed.is_none());
    assert_eq!(renderer.render_forced_at(&request, 1_700_000_000), first);
    assert_eq!(renderer.expansions, if cache_enabled { 2 } else { 3 });
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
    let format = format!("#(echo run >> '{}'; echo value)", count.display());
    let mut renderer = StatusRenderer::default();
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
    let cache_enabled = zz_mux::format_cache_knob() && zz_mux::borrowed_formats_enabled();
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
    assert_eq!(
        renderer.render_forced_at(&request, 1_700_000_000).left,
        "cached"
    );
    Arc::make_mut(&mut request.formats).left = "#{C:#{client_name}}".to_owned();
    renderer.render_forced_at(&request, 1_700_000_000);
    assert!(renderer.completed.is_none());
    let directory = tempfile::tempdir().expect("stale references job fixture");
    let count = directory.path().join("count");
    Arc::make_mut(&mut request.formats).left =
        format!("#(echo run >> '{}'; echo value)", count.display());
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
