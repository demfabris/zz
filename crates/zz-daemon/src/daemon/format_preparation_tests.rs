use super::*;

fn fixture(left: &str) -> (ServerState, ClientId, ExecutionContext) {
    let mut inner = ServerState::default();
    let (session, window, pane) = inner.engine.state.create_session("prepared").unwrap();
    let client = ClientId(3);
    inner.attached.insert(session, BTreeSet::from([client]));
    inner.client_entry(client).focused_window.replace(window);
    inner
        .client_entry(client)
        .kind
        .replace(ClientKind::Interactive);
    inner.client_entry(client).has_terminal = true;
    inner.client_entry(client).size.replace((80, 24));
    inner
        .client_entry(client)
        .environment
        .replace(environment("xterm"));
    inner.engine.set_format_now(1_700_000_000);
    let mut context = ExecutionContext::new(Some(session), Some(window), Some(pane));
    for (name, value) in [
        ("status-left", left),
        ("status-right", ""),
        ("status-left-length", "32767"),
        ("status-format[0]", "#{session_name}"),
        ("pane-border-style", "fg=green"),
        ("pane-active-border-style", "fg=green"),
    ] {
        set_option(&mut inner, &mut context, name, value);
    }
    (inner, client, context)
}

fn environment(term: &str) -> Arc<ClientEnvironmentBlob> {
    Arc::new(ClientEnvironmentBlob::from_map(BTreeMap::from([(
        "TERM".into(),
        term.into(),
    )])))
}

fn set_option(inner: &mut ServerState, context: &mut ExecutionContext, name: &str, value: &str) {
    inner
        .engine
        .execute(
            context,
            &CommandInvocation::new("set-option", ["-g", name, value]),
        )
        .unwrap();
}

fn request(inner: &ServerState, client: ClientId) -> StatusRequest {
    shared_request(inner, client).as_ref().clone()
}

fn shared_request(inner: &ServerState, client: ClientId) -> Arc<StatusRequest> {
    status_request_with_selected_facts(
        inner,
        client,
        inner.engine.cached_format_option_snapshot(),
        true,
        FormatNeeds::NONE,
    )
}

fn reuse_enabled() -> bool {
    zz_mux::borrowed_formats_enabled()
}

#[test]
fn selected_preparation_keeps_scalar_and_string_border_callbacks_fresh() {
    let (mut inner, client, mut context) = fixture("#{session_name}");
    set_option(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?pane_in_mode,red,green}",
    );
    let first = shared_request(&inner, client);
    assert_eq!(first.pane_borders[0].style, "fg=green");
    let same = shared_request(&inner, client);
    assert_eq!(same.pane_borders[0].style, "fg=green");
    inner
        .pane_modes
        .insert(context.pane.unwrap(), vec![PaneModeRequest::Clock]);
    let changed = shared_request(&inner, client);
    assert_eq!(changed.pane_borders[0].style, "fg=red");
    assert_eq!(first.pane_borders[0].style, "fg=green");
    set_option(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?pane_mode,red,green}",
    );
    let with_mode = shared_request(&inner, client);
    assert_eq!(with_mode.pane_borders[0].style, "fg=red");
    inner.pane_modes.remove(&context.pane.unwrap());
    let without_mode = shared_request(&inner, client);
    assert_eq!(without_mode.pane_borders[0].style, "fg=green");
    assert_eq!(with_mode.pane_borders[0].style, "fg=red");
}

fn assert_left(request: &StatusRequest, expected: &str) {
    let rendered = StatusRenderer::default().render_initial(request);
    assert!(
        rendered.left.contains(expected),
        "{:?} lacks {expected:?}",
        rendered.left
    );
}

fn assert_fresh(first: &StatusRequest, next: &StatusRequest) {
    assert!(!Arc::ptr_eq(&first.context, &next.context));
    assert!(!Arc::ptr_eq(&first.facts, &next.facts));
}

#[test]
fn status_preparation_omits_large_unused_config_capture_and_reuses_requests() {
    let (mut inner, client, context) = fixture("#{session_name}:#{client_width}");
    inner.config_files = "a".repeat(STATUS_PREPARATION_MAX_BYTES + 1);
    let first = shared_request(&inner, client);
    assert_left(&first, "prepared:80");
    assert!(!first.references.contains("*"));
    assert!(!first.references.contains("config_files"));
    let selective = reuse_enabled();
    if selective {
        let cache = inner.status_preparation_cache.lock();
        let cached = cache.as_ref().unwrap();
        assert!(!cached.config_files_requested);
        assert!(cached.config_files.is_empty());
        assert!(cached.retained_bytes <= STATUS_PREPARATION_MAX_BYTES);
    } else {
        assert!(inner.status_preparation_cache.lock().is_none());
    }
    let revision = inner.engine.format_cache_revision();
    inner.config_files = "b".repeat(STATUS_PREPARATION_MAX_BYTES + 2);
    let changed = shared_request(&inner, client);
    assert_eq!(inner.engine.format_cache_revision(), revision);
    assert_eq!(Arc::ptr_eq(&first, &changed), selective);
    assert_left(&changed, "prepared:80");
    let candidate = CachedStatusPreparation::new(
        &inner,
        context.session,
        context.window,
        inner.engine.format_cache_revision(),
        Arc::clone(&changed),
    );
    assert_eq!(candidate.config_files_requested, !selective);
    assert_eq!(
        candidate.retained_bytes() <= STATUS_PREPARATION_MAX_BYTES,
        selective
    );
    inner.engine.set_format_now(1_700_000_001);
    let tick = shared_request(&inner, client);
    assert_eq!(tick.context.format_now, Some(1_700_000_001));
    assert_left(&tick, "prepared:80");
    assert_eq!(Arc::ptr_eq(&changed.facts, &tick.facts), selective);
}

#[test]
fn status_preparation_keeps_static_indirect_config_dependencies_fresh() {
    for template in ["#{config_files}", "#{E:status-right}", "#{T:status-right}"] {
        let (mut inner, client, mut context) = fixture(template);
        set_option(&mut inner, &mut context, "status-right", "#{config_files}");
        inner.config_files = "/first.conf".to_owned();
        let first = shared_request(&inner, client);
        assert!(first.references.contains("config_files"), "{template}");
        assert_left(&first, "/first.conf");
        if reuse_enabled() {
            let cache = inner.status_preparation_cache.lock();
            assert!(cache.as_ref().unwrap().config_files_requested);
        }
        let revision = inner.engine.format_cache_revision();
        inner.config_files = "/next.conf".to_owned();
        let next = shared_request(&inner, client);
        assert_eq!(inner.engine.format_cache_revision(), revision);
        assert_fresh(&first, &next);
        assert_left(&next, "/next.conf");
        assert_left(&first, "/first.conf");
    }
}

#[test]
fn status_preparation_keeps_nested_loop_config_dependencies_fresh() {
    let (mut inner, client, context) = fixture("#{W:[#{window_index}:#{P:#{config_files};}]}");
    inner
        .engine
        .state
        .split_pane(
            context.pane.unwrap(),
            zz_protocol::Axis::Horizontal,
            zz_mux::PaneKind::Terminal,
        )
        .unwrap();
    inner.config_files = "/first.conf".to_owned();
    let first = shared_request(&inner, client);
    assert!(first.references.contains("config_files"));
    assert_eq!(
        first.context.variable("config_files").as_deref(),
        Some("/first.conf")
    );
    assert_left(&first, "[0:;;]");
    let revision = inner.engine.format_cache_revision();
    inner.config_files = "/next.conf".to_owned();
    let next = shared_request(&inner, client);
    assert_eq!(inner.engine.format_cache_revision(), revision);
    assert_fresh(&first, &next);
    assert_eq!(
        next.context.variable("config_files").as_deref(),
        Some("/next.conf")
    );
    assert_left(&next, "[0:;;]");
    assert_left(&first, "[0:;;]");
    let snapshot = inner.engine.state.snapshot();
    let owned = status_request(
        &inner,
        client,
        &snapshot,
        inner.engine.cached_format_option_snapshot(),
        format_hook_facts(&inner),
        true,
        FormatNeeds::NONE,
    );
    assert_eq!(
        StatusRenderer::default().render_initial(&next),
        StatusRenderer::default().render_initial(&owned)
    );
}

#[test]
fn status_preparation_keeps_border_only_config_capture_and_guard() {
    let (mut inner, client, mut context) = fixture("#{session_name}");
    set_option(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?config_files,red,green}",
    );
    inner.config_files = "/first.conf".to_owned();
    let first = shared_request(&inner, client);
    assert!(!first.references.contains("config_files"));
    let parameters = status_parameters(&inner, context.session, &first.option_snapshot);
    assert!(parameters.client_references.contains("config_files"));
    assert_eq!(
        first.context.variable("config_files").as_deref(),
        Some("/first.conf")
    );
    if reuse_enabled() {
        let cache = inner.status_preparation_cache.lock();
        assert!(cache.as_ref().unwrap().config_files_requested);
    }
    let revision = inner.engine.format_cache_revision();
    inner.config_files = "/next.conf".to_owned();
    let next = shared_request(&inner, client);
    assert_eq!(inner.engine.format_cache_revision(), revision);
    assert_fresh(&first, &next);
    assert_eq!(
        next.context.variable("config_files").as_deref(),
        Some("/next.conf")
    );
    let snapshot = inner.engine.state.snapshot();
    let owned = status_request(
        &inner,
        client,
        &snapshot,
        inner.engine.cached_format_option_snapshot(),
        format_hook_facts(&inner),
        true,
        FormatNeeds::NONE,
    );
    assert_eq!(first.pane_borders, next.pane_borders);
    assert_eq!(next.pane_borders, owned.pane_borders);
    assert_left(&next, "prepared");
}

#[test]
fn status_preparation_retains_config_for_unknown_jobs_full_providers_and_rollback() {
    let (mut inner, client, mut context) = fixture("#{session_name}");
    inner.config_files = "/retained.conf".to_owned();
    let job = status_request_with_selected_facts(
        &inner,
        client,
        inner.engine.cached_format_option_snapshot(),
        true,
        FormatNeeds::PANES,
    );
    assert!(job.references.contains("*"));
    assert_eq!(
        job.context.variable("config_files").as_deref(),
        Some("/retained.conf")
    );
    let snapshot = inner.engine.state.snapshot();
    let owned = status_request(
        &inner,
        client,
        &snapshot,
        inner.engine.cached_format_option_snapshot(),
        format_hook_facts(&inner),
        true,
        FormatNeeds::NONE,
    );
    assert_eq!(
        owned.context.variable("config_files").as_deref(),
        Some("/retained.conf")
    );
    assert_left(&owned, "prepared");
    let legacy = zz_mux::with_borrowed_formats(false, || shared_request(&inner, client));
    assert_eq!(
        legacy.context.variable("config_files").as_deref(),
        Some("/retained.conf")
    );
    assert_left(&legacy, "prepared");
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
    inner
        .terminals_mut()
        .insert(context.pane.unwrap(), terminal);
    enter_copy_session(&mut inner, client, context.pane.unwrap()).unwrap();
    let mode = shared_request(&inner, client);
    assert!(!mode.modes.is_empty());
    assert_eq!(
        mode.context.variable("config_files").as_deref(),
        Some("/retained.conf")
    );
    inner.client_mut(client).and_then(|c| c.copy_session.take());
    inner.client_entry(client).kind.replace(ClientKind::Control);
    let control = shared_request(&inner, client);
    assert_eq!(
        control.context.variable("config_files").as_deref(),
        Some("/retained.conf")
    );
    inner
        .client_entry(client)
        .kind
        .replace(ClientKind::Interactive);
    if !zz_mux::borrowed_formats_enabled() {
        let rollback = shared_request(&inner, client);
        assert_eq!(
            rollback.context.variable("config_files").as_deref(),
            Some("/retained.conf")
        );
        assert_left(&rollback, "prepared");
    }
    set_option(&mut inner, &mut context, "status-left", "#(printf dynamic)");
    let dynamic = shared_request(&inner, client);
    assert!(dynamic.references.contains("*"));
    assert_eq!(
        dynamic.context.variable("config_files").as_deref(),
        Some("/retained.conf")
    );
}

#[test]
fn status_preparation_shares_the_whole_immutable_request() {
    zz_mux::with_borrowed_formats(true, || {
        let (inner, client, _) = fixture("#{session_name}:#{client_width}");
        let first = shared_request(&inner, client);
        let second = shared_request(&inner, client);
        assert_eq!(Arc::ptr_eq(&first, &second), reuse_enabled());
        assert_left(&second, "prepared:80");
        if reuse_enabled() {
            let cache = inner.status_preparation_cache.lock();
            let cached = cache.as_ref().unwrap();
            assert!(Arc::ptr_eq(&cached.request, &first));
            assert!(Arc::ptr_eq(
                &cached.request.pane_borders,
                &first.pane_borders
            ));
        }
        let owned = zz_mux::with_borrowed_formats(false, || shared_request(&inner, client));
        assert!(!Arc::ptr_eq(&first, &owned));
        assert_left(&owned, "prepared:80");
        let startup = status_request_with_selected_facts(
            &inner,
            client,
            inner.engine.cached_format_option_snapshot(),
            false,
            FormatNeeds::NONE,
        );
        assert!(!Arc::ptr_eq(&first, &startup));
        assert!(startup.startup);
        assert!(!first.startup);
    });
}

#[test]
fn status_preparation_replaces_shared_requests_for_border_clock_and_source_changes() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) = fixture("#{session_name}:#{config_files}");
        set_option(
            &mut inner,
            &mut context,
            "pane-active-border-style",
            "fg=#{?pane_in_mode,red,green}",
        );
        let first = shared_request(&inner, client);
        inner
            .pane_modes
            .insert(context.pane.unwrap(), vec![PaneModeRequest::Clock]);
        let changed = shared_request(&inner, client);
        assert!(!Arc::ptr_eq(&first, &changed));
        assert_eq!(first.pane_borders[0].style, "fg=green");
        assert_eq!(changed.pane_borders[0].style, "fg=red");
        assert_eq!(
            Arc::ptr_eq(&changed, &shared_request(&inner, client)),
            reuse_enabled(),
        );
        inner.engine.set_format_now(1_700_000_001);
        inner.pane_modes.remove(&context.pane.unwrap());
        let tick = shared_request(&inner, client);
        assert!(!Arc::ptr_eq(&changed, &tick));
        assert_eq!(changed.context.format_now, Some(1_700_000_000));
        assert_eq!(tick.context.format_now, Some(1_700_000_001));
        assert_eq!(Arc::ptr_eq(&changed.facts, &tick.facts), reuse_enabled());
        assert_eq!(tick.pane_borders[0].style, "fg=green");
        assert_eq!(changed.pane_borders[0].style, "fg=red");
        assert_eq!(
            Arc::ptr_eq(&tick, &shared_request(&inner, client)),
            reuse_enabled()
        );
        inner.config_files = "/new.conf".to_owned();
        let source = shared_request(&inner, client);
        assert!(!Arc::ptr_eq(&tick, &source));
        assert_left(&source, "prepared:/new.conf");
        assert_left(&first, "prepared:");
    });
}

#[test]
fn status_preparation_clock_reuse_refreshes_nested_loop_times_and_preserves_facts() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, context) = fixture(
            "#{client_width}:#{client_colours}:#{W:[#{window_index}:#{P:#{pane_index}=#{t/d:session_created};}]}",
        );
        let session = context.session.unwrap();
        inner.engine.state.session_mut(session).unwrap().created = Some(1_700_000_000);
        inner
            .engine
            .state
            .split_pane(
                context.pane.unwrap(),
                zz_protocol::Axis::Horizontal,
                zz_mux::PaneKind::Terminal,
            )
            .unwrap();
        inner
            .engine
            .state
            .create_window(session, None, zz_mux::PaneKind::Terminal)
            .unwrap();
        let first = shared_request(&inner, client);
        assert_left(&first, "80:8:[0:0=0;1=0;][1:0=0;]");
        inner.engine.set_format_now(1_700_000_001);
        let tick = shared_request(&inner, client);
        assert!(!Arc::ptr_eq(&first, &tick));
        assert!(!Arc::ptr_eq(&first.context, &tick.context));
        assert_eq!(Arc::ptr_eq(&first.facts, &tick.facts), reuse_enabled());
        assert_eq!(
            first.context.same_detached_data(&tick.context),
            zz_mux::borrowed_formats_enabled()
        );
        assert_left(&tick, "80:8:[0:0=1;1=1;][1:0=1;]");
        assert_left(&first, "80:8:[0:0=0;1=0;][1:0=0;]");
        assert_eq!(
            Arc::ptr_eq(&tick, &shared_request(&inner, client)),
            reuse_enabled()
        );
        if reuse_enabled() {
            let cache = inner.status_preparation_cache.lock();
            let cached = cache.as_ref().unwrap();
            assert_eq!(cached.revision, inner.engine.format_cache_revision());
            assert!(cached.retained_bytes() <= STATUS_PREPARATION_MAX_BYTES);
        }
    });
}

#[test]
fn status_preparation_lazy_options_reject_equal_revision_engine_replacement() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, _) = fixture("old:#{session_name}");
        let first =
            status_request_with_selected_options(&inner, client, None, true, FormatNeeds::NONE);
        assert_left(&first, "old:prepared");
        let same =
            status_request_with_selected_options(&inner, client, None, true, FormatNeeds::NONE);
        assert_eq!(Arc::ptr_eq(&first, &same), reuse_enabled());
        let (replacement, _, _) = fixture("new:#{session_name}");
        assert_eq!(
            inner.engine.format_cache_revision(),
            replacement.engine.format_cache_revision()
        );
        inner.engine = replacement.engine;
        let changed =
            status_request_with_selected_options(&inner, client, None, true, FormatNeeds::NONE);
        assert_fresh(&first, &changed);
        assert!(!Arc::ptr_eq(
            &first.option_snapshot,
            &changed.option_snapshot
        ));
        assert_left(&changed, "new:prepared");
        assert_left(&first, "old:prepared");
    });
}

#[test]
fn status_preparation_reuses_retained_size_only_for_unchanged_clock_storage() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) = fixture("#{session_name}:#{t/d:session_created}");
        inner
            .engine
            .state
            .session_mut(context.session.unwrap())
            .unwrap()
            .created = Some(1_700_000_000);
        set_option(
            &mut inner,
            &mut context,
            "pane-active-border-style",
            "fg=#{?pane_in_mode,red,green}",
        );
        let first = shared_request(&inner, client);
        assert_left(&first, "prepared:0");
        inner.engine.set_format_now(1_700_000_001);
        let tick = shared_request(&inner, client);
        assert_left(&tick, "prepared:1");
        if reuse_enabled() {
            let mut cache = inner.status_preparation_cache.lock();
            let cached = cache.as_mut().unwrap();
            assert_eq!(cached.retained_bytes, cached.retained_bytes());
            cached.retained_bytes += 128;
        }
        inner.engine.set_format_now(1_700_000_002);
        let same_storage = shared_request(&inner, client);
        assert_left(&same_storage, "prepared:2");
        if reuse_enabled() {
            let cache = inner.status_preparation_cache.lock();
            let cached = cache.as_ref().unwrap();
            assert_eq!(cached.retained_bytes, cached.retained_bytes() + 128);
            assert!(Arc::ptr_eq(&tick.pane_borders, &same_storage.pane_borders));
        }
        inner
            .pane_modes
            .insert(context.pane.unwrap(), vec![PaneModeRequest::Clock]);
        let borders = shared_request(&inner, client);
        assert_eq!(borders.pane_borders[0].style, "fg=red");
        assert_eq!(same_storage.pane_borders[0].style, "fg=green");
        if reuse_enabled() {
            let cache = inner.status_preparation_cache.lock();
            let cached = cache.as_ref().unwrap();
            assert_eq!(cached.retained_bytes, cached.retained_bytes());
            assert!(!Arc::ptr_eq(
                &same_storage.pane_borders,
                &borders.pane_borders
            ));
        }
        assert_left(&first, "prepared:0");
    });
}

#[test]
fn status_preparation_clock_reuse_falls_back_after_engine_capture_eviction() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, context) =
            fixture("#{session_name}:#{client_width}:#{t/d:session_created}");
        inner
            .engine
            .state
            .session_mut(context.session.unwrap())
            .unwrap()
            .created = Some(1_700_000_000);
        let first = shared_request(&inner, client);
        assert_left(&first, "prepared:80:0");
        let unrelated = inner
            .engine
            .format_status_context(context.session, context.window, context.pane)
            .detach_with_references(
                FormatNeeds::NONE,
                &BTreeSet::from(["session_name".to_owned()]),
            );
        assert_eq!(
            unrelated.variable("session_name").as_deref(),
            Some("prepared")
        );
        let parameters = status_parameters(&inner, context.session, &first.option_snapshot);
        assert!(
            cached_live_status_context(
                &inner,
                context.session,
                context.window,
                parameters.needs,
                &first.references,
                true,
            )
            .is_none()
        );
        inner.engine.set_format_now(1_700_000_001);
        let missed = shared_request(&inner, client);
        assert_fresh(&first, &missed);
        assert_left(&missed, "prepared:80:1");
        assert_left(&first, "prepared:80:0");
        inner.engine.set_format_now(1_700_000_002);
        let warm = shared_request(&inner, client);
        assert!(!Arc::ptr_eq(&missed.context, &warm.context));
        assert_eq!(Arc::ptr_eq(&missed.facts, &warm.facts), reuse_enabled());
        assert_left(&warm, "prepared:80:2");
        assert_eq!(
            Arc::ptr_eq(&warm, &shared_request(&inner, client)),
            reuse_enabled()
        );
    });
}

#[test]
fn status_preparation_bounds_the_retained_shared_border_payload() {
    zz_mux::with_borrowed_formats(true, || {
        let (inner, client, context) = fixture("#{session_name}");
        let mut request = shared_request(&inner, client);
        Arc::make_mut(&mut Arc::make_mut(&mut request).pane_borders)[0].style =
            "x".repeat(STATUS_PREPARATION_MAX_BYTES);
        let cached = CachedStatusPreparation::new(
            &inner,
            context.session,
            context.window,
            inner.engine.format_cache_revision(),
            request,
        );
        assert!(cached.retained_bytes() > STATUS_PREPARATION_MAX_BYTES);
    });
}

#[test]
fn status_preparation_reuses_safe_engine_width_and_colour_dependencies() {
    zz_mux::with_borrowed_formats(true, || {
        let (inner, client, _) = fixture("#{session_name}:#{client_width}:#{client_colours}");
        let first = request(&inner, client);
        let second = request(&inner, client);
        assert_left(&first, "prepared:80:8");
        assert_eq!(
            StatusRenderer::default().render_initial(&first),
            StatusRenderer::default().render_initial(&second)
        );
        assert_eq!(
            Arc::ptr_eq(&first.context, &second.context),
            reuse_enabled()
        );
        assert_eq!(Arc::ptr_eq(&first.facts, &second.facts), reuse_enabled());
        let cache = inner.status_preparation_cache.lock();
        if reuse_enabled() {
            assert!(cache.as_ref().unwrap().retained_bytes() <= STATUS_PREPARATION_MAX_BYTES);
        } else {
            assert!(cache.is_none());
        }
    });
}

#[test]
fn status_preparation_tracks_fresh_width_features_and_client_environment() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, _) = fixture("#{client_width}:#{client_colours}");
        let first = request(&inner, client);
        let revision = inner.engine.format_cache_revision();
        inner.client_entry(client).size.replace((100, 30));
        let wider = request(&inner, client);
        assert_fresh(&first, &wider);
        assert_left(&wider, "100:8");
        inner
            .client_entry(client)
            .features
            .replace(client_features_fact(&["client-features-v1:RGB".to_owned()]));
        let rgb = request(&inner, client);
        assert_fresh(&wider, &rgb);
        assert_left(&rgb, "100:16777216");
        inner.client_mut(client).and_then(|c| c.features.take());
        inner
            .client_entry(client)
            .environment
            .replace(environment("xterm-256color"));
        let colours = request(&inner, client);
        assert_fresh(&rgb, &colours);
        assert_left(&colours, "100:256");
        inner
            .client_mut(client)
            .map(|client| std::mem::take(&mut client.has_terminal));
        let no_terminal = request(&inner, client);
        assert_fresh(&colours, &no_terminal);
        assert_eq!(no_terminal.facts.client.as_ref().unwrap().colours, "");
        assert_eq!(inner.engine.format_cache_revision(), revision);
        assert_left(&first, "80:8");
    });
}

#[test]
fn status_preparation_invalidates_config_state_options_data_environment_and_clock() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) =
            fixture("#{pane_title}:#{pid}:#{PREPARATION_ENV}:#{config_files}");
        let pane = context.pane.unwrap();
        inner.engine.state.update_pane_title(pane, "first").unwrap();
        inner.engine.set_format_server_identity(11, "22", "user");
        inner.config_files = "/first.conf".to_owned();
        let first = request(&inner, client);
        inner.config_files = "/second.conf".to_owned();
        let config = request(&inner, client);
        assert_fresh(&first, &config);
        assert_eq!(
            config.context.variable("config_files").as_deref(),
            Some("/second.conf")
        );
        inner
            .engine
            .state
            .update_pane_title(pane, "second")
            .unwrap();
        let state = request(&inner, client);
        assert_fresh(&config, &state);
        assert_eq!(
            state.context.variable("pane_title").as_deref(),
            Some("second")
        );
        set_option(
            &mut inner,
            &mut context,
            "status-left",
            "updated:#{pane_title}:#{pid}:#{PREPARATION_ENV}:#{config_files}",
        );
        let options = request(&inner, client);
        assert_fresh(&state, &options);
        assert!(options.formats.left.starts_with("updated:"));
        inner.engine.set_format_server_identity(33, "44", "other");
        let data = request(&inner, client);
        assert_fresh(&options, &data);
        assert_eq!(data.context.variable("pid").as_deref(), Some("33"));
        inner
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-environment", ["-g", "PREPARATION_ENV", "fresh"]),
            )
            .unwrap();
        let environment = request(&inner, client);
        assert_fresh(&data, &environment);
        assert_left(&environment, "updated:second:33:fresh:/second.conf");
        inner.engine.set_format_now(1_700_000_001);
        let clock = request(&inner, client);
        assert!(!Arc::ptr_eq(&environment.context, &clock.context));
        assert_eq!(
            Arc::ptr_eq(&environment.facts, &clock.facts),
            reuse_enabled()
        );
        assert_eq!(clock.context.format_now, Some(1_700_000_001));
        assert_left(&clock, "updated:second:33:fresh:/second.conf");
        let same_second = request(&inner, client);
        assert_eq!(
            Arc::ptr_eq(&clock.context, &same_second.context),
            reuse_enabled()
        );
        assert_eq!(
            Arc::ptr_eq(&clock.facts, &same_second.facts),
            reuse_enabled()
        );
        assert_eq!(
            first.context.variable("pane_title").as_deref(),
            Some("first")
        );
        assert_eq!(
            first.context.variable("config_files").as_deref(),
            Some("/first.conf")
        );
    });
}

#[test]
fn status_preparation_retargets_focus_session_and_replaced_engine() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, context) = fixture("#{session_name}:#{pane_title}");
        let session = context.session.unwrap();
        let (other_window, other_pane) = inner
            .engine
            .state
            .create_window(session, None, zz_mux::PaneKind::Terminal)
            .unwrap();
        let (other_session, third_window, third_pane) =
            inner.engine.state.create_session("other").unwrap();
        for (pane, title) in [
            (context.pane.unwrap(), "first"),
            (other_pane, "second"),
            (third_pane, "third"),
        ] {
            inner.engine.state.update_pane_title(pane, title).unwrap();
        }
        let first = request(&inner, client);
        let revision = inner.engine.format_cache_revision();
        inner
            .client_entry(client)
            .focused_window
            .replace(other_window);
        let focused = request(&inner, client);
        assert_fresh(&first, &focused);
        assert_left(&focused, "prepared:second");
        inner.attached.get_mut(&session).unwrap().remove(&client);
        inner
            .attached
            .insert(other_session, BTreeSet::from([client]));
        inner
            .client_entry(client)
            .focused_window
            .replace(third_window);
        let attached = request(&inner, client);
        assert_fresh(&focused, &attached);
        assert_left(&attached, "other:third");
        assert_eq!(inner.engine.format_cache_revision(), revision);

        let (mut replacement, replacement_client, _) = fixture("replacement");
        let old = request(&replacement, replacement_client);
        let revision = replacement.engine.format_cache_revision();
        let (fresh, _, _) = fixture("changed");
        replacement.engine = fresh.engine;
        assert_eq!(replacement.engine.format_cache_revision(), revision);
        let changed = request(&replacement, replacement_client);
        assert_fresh(&old, &changed);
        assert_left(&changed, "changed");
    });
}

#[test]
fn status_preparation_invalidates_scheme_startup_and_client_identity() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, context) = fixture("#{session_name}:#{client_width}");
        let first = request(&inner, client);
        let revision = inner.engine.format_cache_revision();
        inner
            .client_entry(client)
            .color_scheme
            .replace(TerminalColorScheme::Light);
        let light = request(&inner, client);
        assert_fresh(&first, &light);
        assert_eq!(light.client_scheme, Some(TerminalColorScheme::Light));
        let startup = status_request_with_selected_facts(
            &inner,
            client,
            inner.engine.cached_format_option_snapshot(),
            false,
            FormatNeeds::NONE,
        );
        assert_fresh(&light, &startup);
        assert!(startup.startup);
        let other_client = ClientId(4);
        inner
            .attached
            .get_mut(&context.session.unwrap())
            .unwrap()
            .insert(other_client);
        inner
            .client_entry(other_client)
            .focused_window
            .replace(context.window.unwrap());
        inner.client_entry(other_client).size.replace((55, 24));
        inner
            .client_entry(other_client)
            .kind
            .replace(ClientKind::Interactive);
        let other = request(&inner, other_client);
        assert_fresh(&startup, &other);
        assert_eq!(other.client, other_client);
        assert_left(&other, "prepared:55");
        assert_eq!(inner.engine.format_cache_revision(), revision);
    });
}

#[test]
fn status_preparation_bypasses_live_callbacks_jobs_modes_control_and_rollback() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) = fixture("#{session_name}");
        for template in [
            "#{session_attached}",
            "#{window_active_clients}",
            "#{cursor_x}",
            "#{pane_in_mode}",
            "#{client_prefix}",
            "#(printf dynamic)",
            "#{@preparation}",
        ] {
            set_option(&mut inner, &mut context, "status-left", template);
            let first = request(&inner, client);
            let second = request(&inner, client);
            assert_fresh(&first, &second);
            assert!(
                inner.status_preparation_cache.lock().is_none(),
                "{template}"
            );
        }
        set_option(&mut inner, &mut context, "status-left", "#{session_name}");
        let first = request(&inner, client);
        let job = status_request_with_selected_facts(
            &inner,
            client,
            inner.engine.cached_format_option_snapshot(),
            true,
            FormatNeeds::PANES,
        );
        assert_fresh(&first, &job);
        assert!(job.references.contains("*"));
        enter_copy_session(&mut inner, client, context.pane.unwrap()).unwrap();
        let mode = request(&inner, client);
        assert_fresh(&first, &mode);
        inner.client_mut(client).and_then(|c| c.copy_session.take());
        inner.client_entry(client).kind.replace(ClientKind::Control);
        let control = request(&inner, client);
        assert_fresh(&first, &control);
        inner
            .client_entry(client)
            .kind
            .replace(ClientKind::Interactive);
        let owned = zz_mux::with_borrowed_formats(false, || request(&inner, client));
        assert_fresh(&first, &owned);
        assert_left(&owned, "prepared");
    });
}

#[test]
fn status_preparation_reuse_keeps_border_mode_callbacks_fresh() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) = fixture("#{session_name}:#{client_width}");
        set_option(
            &mut inner,
            &mut context,
            "pane-active-border-style",
            "fg=#{?pane_in_mode,red,green}",
        );
        let first = request(&inner, client);
        assert_eq!(first.pane_borders[0].style, "fg=green");
        let revision = inner.engine.format_cache_revision();
        inner
            .pane_modes
            .insert(context.pane.unwrap(), vec![PaneModeRequest::Clock]);
        let second = request(&inner, client);
        assert_eq!(inner.engine.format_cache_revision(), revision);
        assert_eq!(
            Arc::ptr_eq(&first.context, &second.context),
            reuse_enabled()
        );
        assert_eq!(second.pane_borders[0].style, "fg=red");
        assert_eq!(first.pane_borders[0].style, "fg=green");
    });
}

#[test]
fn status_preparation_bypasses_border_prefix_and_cell_dependencies() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) = fixture("#{session_name}");
        set_option(
            &mut inner,
            &mut context,
            "pane-active-border-style",
            "fg=#{?client_prefix,red,green}",
        );
        let first = request(&inner, client);
        assert_eq!(first.pane_borders[0].style, "fg=green");
        let revision = inner.engine.format_cache_revision();
        inner
            .client_entry(client)
            .key_engine
            .get_or_insert_default()
            .switch_table(Some("copy-mode".to_owned()));
        let prefix = request(&inner, client);
        assert_fresh(&first, &prefix);
        assert_eq!(prefix.pane_borders[0].style, "fg=red");
        assert!(inner.status_preparation_cache.lock().is_none());
        assert_eq!(inner.engine.format_cache_revision(), revision);
        set_option(
            &mut inner,
            &mut context,
            "pane-active-border-style",
            "fg=#{?#{==:#{window_cell_width},9},red,green},bg=#{?#{==:#{window_cell_height},19},blue,black}",
        );
        let before = request(&inner, client);
        assert_eq!(before.pane_borders[0].style, "fg=green,bg=black");
        let revision = inner.engine.format_cache_revision();
        inner
            .terminal_geometries
            .entry(context.pane.unwrap())
            .or_default()
            .insert(
                client,
                TerminalGeometry {
                    columns: 80,
                    rows: 24,
                    cell_width_px: 9,
                    cell_height_px: 19,
                },
            );
        let geometry = request(&inner, client);
        assert_fresh(&before, &geometry);
        assert_eq!(geometry.pane_borders[0].style, "fg=red,bg=blue");
        assert!(inner.status_preparation_cache.lock().is_none());
        assert_eq!(inner.engine.format_cache_revision(), revision);
    });
}

#[test]
fn status_preparation_rejects_oversized_context_and_environment_capture() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) = fixture("#{session_name}:#{config_files}");
        let small = request(&inner, client);
        if reuse_enabled() {
            assert!(inner.status_preparation_cache.lock().is_some());
        }
        inner.config_files = "c".repeat(400_000);
        let large_environment = "e".repeat(400_000);
        inner
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new(
                    "set-environment",
                    ["-g", "PREPARATION_LARGE", large_environment.as_str()],
                ),
            )
            .unwrap();
        let large = request(&inner, client);
        assert_fresh(&small, &large);
        assert_eq!(
            large.context.variable("config_files").unwrap().len(),
            400_000
        );
        assert!(large.environment.iter().any(|(name, value)| {
            name.as_bytes() == b"PREPARATION_LARGE"
                && value
                    .as_ref()
                    .is_some_and(|value| value.as_bytes().len() == 400_000)
        }));
        let candidate = CachedStatusPreparation::new(
            &inner,
            context.session,
            context.window,
            inner.engine.format_cache_revision(),
            Arc::new(large),
        );
        assert!(candidate.retained_bytes() > STATUS_PREPARATION_MAX_BYTES);
        assert!(inner.status_preparation_cache.lock().is_none());
    });
}

#[test]
fn raw_text_bound_rejects_large_prepared_job_environment_and_reuses_small_values() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) = fixture("#{session_name}");
        for name in ["message-style", "message-command-style"] {
            set_option(&mut inner, &mut context, name, "fg=blue");
        }
        for raw in [b"a\xffb".to_vec(), vec![0xff; 300_000]] {
            let value = RawText::from_bytes(raw);
            inner
                .engine
                .execute(
                    &mut context,
                    &CommandInvocation::new(
                        "set-environment",
                        ["-g".into(), "RAW_TEXT_BOUND".into(), value.clone()],
                    ),
                )
                .unwrap();
            let first = request(&inner, client);
            let same = request(&inner, client);
            let stored = first
                .environment
                .iter()
                .find(|(name, _)| name.as_bytes() == b"RAW_TEXT_BOUND")
                .and_then(|(_, value)| value.as_ref())
                .unwrap();
            assert_eq!(stored.as_bytes(), value.as_bytes());
            assert_eq!(stored.as_str(), value.as_str());
            assert_eq!(
                StatusRenderer::default().render_initial(&first),
                StatusRenderer::default().render_initial(&same)
            );
            let candidate = CachedStatusPreparation::new(
                &inner,
                context.session,
                context.window,
                inner.engine.format_cache_revision(),
                Arc::new(first.clone()),
            );
            if value.as_bytes().len() == 300_000 {
                let mut without_job_environment = first;
                without_job_environment.environment = Arc::default();
                let empty = CachedStatusPreparation::new(
                    &inner,
                    context.session,
                    context.window,
                    inner.engine.format_cache_revision(),
                    Arc::new(without_job_environment),
                );
                assert!(candidate.retained_bytes() >= empty.retained_bytes() + 2_400_000);
                assert!(candidate.retained_bytes() > STATUS_PREPARATION_MAX_BYTES);
                assert!(inner.status_preparation_cache.lock().is_none());
            } else {
                assert!(candidate.retained_bytes() < STATUS_PREPARATION_MAX_BYTES);
                assert_eq!(Arc::ptr_eq(&first.context, &same.context), reuse_enabled());
                assert_eq!(Arc::ptr_eq(&first.facts, &same.facts), reuse_enabled());
            }
        }
    });
}

#[test]
fn raw_text_bound_client_blob_guard_reuses_without_retaining_parsed_payloads() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, context) = fixture("#{session_name}:#{client_colours}");
        for lazy in [false, true] {
            for size in [3, 15_000] {
                let mut values = BTreeMap::from([("TERM".into(), "xterm".into())]);
                for index in 0..16 {
                    values.insert(
                        format!("BAD_{index}").into(),
                        RawText::from_bytes(vec![0xff; size]),
                    );
                }
                let blob = ClientEnvironmentBlob::from_map(values);
                let blob = if lazy {
                    ClientEnvironmentBlob::from_bytes(blob.as_bytes().to_vec())
                } else {
                    blob
                };
                assert!(blob.is_valid());
                assert_eq!(blob.map().get("BAD_0").unwrap().as_bytes().len(), size);
                let minimum_payload = blob.as_bytes().len() + 16 * size * 4;
                inner
                    .client_entry(client)
                    .environment
                    .replace(Arc::new(blob));
                let identity = Arc::downgrade(
                    inner
                        .client(client)
                        .and_then(|c| c.environment.as_ref())
                        .unwrap(),
                );
                let first = request(&inner, client);
                let same = request(&inner, client);
                assert_left(&first, "prepared:8");
                assert_eq!(
                    StatusRenderer::default().render_initial(&first),
                    StatusRenderer::default().render_initial(&same)
                );
                let candidate = CachedStatusPreparation::new(
                    &inner,
                    context.session,
                    context.window,
                    inner.engine.format_cache_revision(),
                    Arc::new(first.clone()),
                );
                assert!(candidate.retained_bytes() < STATUS_PREPARATION_MAX_BYTES);
                if size == 15_000 {
                    assert!(minimum_payload > STATUS_PREPARATION_MAX_BYTES);
                }
                assert_eq!(Arc::ptr_eq(&first.context, &same.context), reuse_enabled());
                assert_eq!(Arc::ptr_eq(&first.facts, &same.facts), reuse_enabled());
                assert_eq!(
                    candidate.environment.as_ref().unwrap().as_ptr(),
                    identity.as_ptr()
                );
                drop(candidate);
                drop(first);
                drop(same);
                assert!(
                    inner
                        .client_mut(client)
                        .and_then(|c| c.environment.take())
                        .is_some()
                );
                assert!(identity.upgrade().is_none());
            }
        }
    });
}

#[test]
fn status_preparation_reuses_the_default_status_templates() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) = fixture("#{session_name}");
        for name in [
            "status-left",
            "status-right",
            "status-left-length",
            "status-format[0]",
            "pane-border-style",
            "pane-active-border-style",
        ] {
            inner
                .engine
                .execute(
                    &mut context,
                    &CommandInvocation::new("set-option", ["-gu", name]),
                )
                .unwrap();
        }
        let first = request(&inner, client);
        let second = request(&inner, client);
        assert!(first.references.contains("window_bigger"));
        assert!(first.references.contains("window_offset_x"));
        assert!(first.references.contains("window_offset_y"));
        assert_eq!(
            Arc::ptr_eq(&first.context, &second.context),
            reuse_enabled()
        );
        assert_eq!(Arc::ptr_eq(&first.facts, &second.facts), reuse_enabled());
        assert_left(&second, "prepared");
    });
}

#[test]
fn status_preparation_tracks_viewport_changes_without_engine_revisions() {
    zz_mux::with_borrowed_formats(true, || {
        let (mut inner, client, mut context) =
            fixture("#{window_bigger}:#{window_offset_x}:#{window_offset_y}");
        set_option(
            &mut inner,
            &mut context,
            "pane-active-border-style",
            "fg=#{?window_bigger,red,green}",
        );
        inner.client_entry(client).size.replace((100, 100));
        let fitting = request(&inner, client);
        let same_fitting = request(&inner, client);
        assert_eq!(
            Arc::ptr_eq(&fitting.context, &same_fitting.context),
            reuse_enabled()
        );
        assert_left(&same_fitting, "0::");
        inner.client_entry(client).size.replace((40, 12));
        let narrowed = request(&inner, client);
        assert_fresh(&fitting, &narrowed);
        assert_left(&narrowed, "1:0:0");
        assert_left(&fitting, "0::");
        inner.client_mut(client).and_then(|c| c.size.take());
        inner
            .terminal_geometries
            .entry(context.pane.unwrap())
            .or_default()
            .insert(
                client,
                TerminalGeometry {
                    columns: 100,
                    rows: 100,
                    cell_width_px: 9,
                    cell_height_px: 19,
                },
            );
        let first = request(&inner, client);
        let unchanged = request(&inner, client);
        assert_eq!(
            Arc::ptr_eq(&first.context, &unchanged.context),
            reuse_enabled()
        );
        assert_left(&first, "0::");
        assert_eq!(first.pane_borders[0].style, "fg=green");
        let revision = inner.engine.format_cache_revision();
        inner
            .terminal_geometries
            .get_mut(&context.pane.unwrap())
            .unwrap()
            .insert(
                client,
                TerminalGeometry {
                    columns: 40,
                    rows: 12,
                    cell_width_px: 9,
                    cell_height_px: 19,
                },
            );
        let smaller = request(&inner, client);
        assert_fresh(&first, &smaller);
        assert_left(&smaller, "1:0:0");
        assert_eq!(smaller.pane_borders[0].style, "fg=red");
        assert_eq!(inner.engine.format_cache_revision(), revision);
        assert_left(&first, "0::");
    });
}
