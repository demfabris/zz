use super::*;

fn fixture() -> (ServerState, ClientId, ExecutionContext) {
    let mut inner = ServerState::default();
    let (session, window, pane) = inner.engine.state.create_session("borders").unwrap();
    let client = ClientId(3);
    inner.attached.insert(session, BTreeSet::from([client]));
    inner.focused_windows.insert(client, window);
    inner.engine.set_format_now(1_700_000_000);
    BORDER_FORMAT_EXPANSIONS.with(|count| count.set(0));
    (
        inner,
        client,
        ExecutionContext::new(Some(session), Some(window), Some(pane)),
    )
}

fn set_style(inner: &mut ServerState, context: &mut ExecutionContext, name: &str, value: &str) {
    inner
        .engine
        .execute(
            context,
            &CommandInvocation::new("set-option", ["-g", name, value]),
        )
        .unwrap();
}

fn borders(
    inner: &ServerState,
    client: ClientId,
    session: SessionId,
    facts: &dyn crate::status::FormatFactSource,
) -> Arc<Vec<zz_protocol::PaneBorderPresentation>> {
    border_presentations_at(inner, client, session, facts, 1_700_000_000)
}

fn expansions() -> usize {
    BORDER_FORMAT_EXPANSIONS.with(Cell::get)
}

#[test]
fn live_scalar_border_probe_preserves_headers_options_and_fresh_mode_counts() {
    let (mut inner, client, mut context) = fixture();
    set_style(
        &mut inner,
        &mut context,
        "@border-outer",
        "#{E:@border-inner}",
    );
    set_style(
        &mut inner,
        &mut context,
        "@border-inner",
        "fg=#{?pane_in_mode,red,green}",
    );
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "#{E:@border-outer}",
    );
    let session = context.session.unwrap();
    let pane = context.pane.unwrap();
    let second = 1_700_000_000;
    let first = {
        let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
        borders(&inner, client, session, &facts)
    };
    let options = inner.engine.cached_format_option_snapshot();
    let clock_reads = Cell::new(0);
    let hit = cached_live_border_presentations(
        &inner,
        client,
        session,
        context.window,
        inner.engine.format_cache_revision().unwrap_or_default(),
        &options,
        || {
            clock_reads.set(clock_reads.get() + 1);
            second
        },
    );
    assert_eq!(hit.is_some(), zz_mux::format_cache_knob());
    assert_eq!(clock_reads.get(), 0);
    if let Some(hit) = hit {
        assert!(Arc::ptr_eq(&first, &hit));
    }
    let copied_options = Arc::new(inner.engine.format_option_snapshot());
    for (probe_client, probe_session, probe_window, probe_options, probe_second) in [
        (
            ClientId(client.0 + 1),
            session,
            context.window,
            &options,
            second,
        ),
        (
            client,
            SessionId(u64::MAX),
            context.window,
            &options,
            second,
        ),
        (client, session, context.window, &copied_options, second),
        (client, session, None, &options, second),
        (client, session, Some(WindowId(u64::MAX)), &options, second),
    ] {
        assert!(
            cached_live_border_presentations(
                &inner,
                probe_client,
                probe_session,
                probe_window,
                inner.engine.format_cache_revision().unwrap_or_default(),
                probe_options,
                || probe_second,
            )
            .is_none()
        );
    }
    inner.engine.set_format_now(second + 1);
    let later = cached_live_border_presentations(
        &inner,
        client,
        session,
        context.window,
        inner.engine.format_cache_revision().unwrap_or_default(),
        &options,
        || second + 1,
    );
    assert_eq!(later.is_some(), zz_mux::format_cache_knob());
    if let Some(later) = later {
        assert!(Arc::ptr_eq(&first, &later));
    }
    inner.engine.set_format_now(second);
    inner.pane_modes.insert(pane, vec![PaneModeRequest::Clock]);
    assert!(
        cached_live_border_presentations(
            &inner,
            client,
            session,
            context.window,
            inner.engine.format_cache_revision().unwrap_or_default(),
            &options,
            || second,
        )
        .is_none()
    );
    let changed = {
        let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
        borders(&inner, client, session, &facts)
    };
    assert_eq!(first[0].style, "fg=green");
    assert_eq!(changed[0].style, "fg=red");
    assert!(!Arc::ptr_eq(&first, &changed));
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?pane_mode,red,green}",
    );
    let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
    borders(&inner, client, session, &facts);
    let options = inner.engine.cached_format_option_snapshot();
    assert!(
        cached_live_border_presentations(
            &inner,
            client,
            session,
            context.window,
            inner.engine.format_cache_revision().unwrap_or_default(),
            &options,
            || second,
        )
        .is_none()
    );
}

#[test]
fn border_format_cache_reuses_default_styles_at_twenty_windows() {
    let (mut inner, client, context) = fixture();
    for index in 1..20 {
        inner
            .engine
            .state
            .create_window(
                context.session.unwrap(),
                Some(format!("window-{index}")),
                zz_mux::PaneKind::Terminal,
            )
            .unwrap();
    }
    let mut facts = FormatHookFacts {
        client: Some(ClientFormatFacts {
            written: "10".to_owned(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let first = borders(&inner, client, context.session.unwrap(), &facts);
    assert_eq!(first.len(), 1);
    let before = expansions();
    facts.client.as_mut().unwrap().written = "99".to_owned();
    let second = borders(&inner, client, context.session.unwrap(), &facts);
    assert_eq!(second, first);
    assert_eq!(Arc::ptr_eq(&first, &second), zz_mux::format_cache_knob());
    assert_eq!(
        expansions(),
        before + usize::from(!zz_mux::format_cache_knob())
    );
    let cache = inner.border_presentations_cache.lock();
    if zz_mux::format_cache_knob() {
        let retained = cache.as_ref().unwrap().retained_bytes();
        eprintln!("twenty-window border cache retained {retained} bytes");
        assert!(retained <= BORDER_FORMAT_CACHE_BYTES);
    } else {
        assert!(cache.is_none());
    }
}

#[test]
fn border_format_cache_reads_fresh_mode_values_for_every_pane() {
    let (mut inner, client, mut context) = fixture();
    let first_pane = context.pane.unwrap();
    inner
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("split-window", ["-h"]),
        )
        .unwrap();
    let second_pane = context.pane.unwrap();
    for name in ["pane-border-style", "pane-active-border-style"] {
        set_style(
            &mut inner,
            &mut context,
            name,
            "fg=#{?pane_in_mode,red,green}",
        );
    }
    let mut facts = FormatHookFacts::default();
    let first = borders(&inner, client, context.session.unwrap(), &facts);
    assert_eq!(first.len(), 2);
    assert!(first.iter().all(|pane| pane.style == "fg=green"));
    let revision = inner.engine.format_cache_revision();
    facts.pane_modes = Arc::new(BTreeMap::from([(first_pane, (1, "choose-tree"))]));
    let second = borders(&inner, client, context.session.unwrap(), &facts);
    assert_eq!(inner.engine.format_cache_revision(), revision);
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(first.iter().all(|pane| pane.style == "fg=green"));
    assert_eq!(
        second
            .iter()
            .find(|pane| pane.pane == first_pane)
            .unwrap()
            .style,
        "fg=red"
    );
    assert_eq!(
        second
            .iter()
            .find(|pane| pane.pane == second_pane)
            .unwrap()
            .style,
        "fg=green"
    );
    facts.pane_modes = Arc::new(BTreeMap::from([(second_pane, (1, "choose-tree"))]));
    let third = borders(&inner, client, context.session.unwrap(), &facts);
    assert_eq!(
        third
            .iter()
            .find(|pane| pane.pane == first_pane)
            .unwrap()
            .style,
        "fg=green"
    );
    assert_eq!(
        third
            .iter()
            .find(|pane| pane.pane == second_pane)
            .unwrap()
            .style,
        "fg=red"
    );
    assert_eq!(expansions(), 6);
}

#[test]
fn border_format_cache_tracks_referenced_client_and_daemon_facts() {
    let (mut inner, client, mut context) = fixture();
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?client_prefix,red,green},bg=#{?window_active_clients,blue,black}",
    );
    let mut facts = FormatHookFacts {
        client: Some(ClientFormatFacts {
            prefix: "0".to_owned(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let session = context.session.unwrap();
    let window = context.window.unwrap();
    let first = borders(&inner, client, session, &facts);
    assert_eq!(first[0].style, "fg=green,bg=black");
    facts.client.as_mut().unwrap().prefix = "1".to_owned();
    facts.window_clients = Arc::new(BTreeMap::from([(window, vec!["other".to_owned()])]));
    let second = borders(&inner, client, session, &facts);
    assert_eq!(second[0].style, "fg=red,bg=blue");
    assert_eq!(expansions(), 2);
}

#[test]
fn border_format_cache_reuses_static_styles_across_clocks_and_invalidates_other_revisions() {
    let (mut inner, client, mut context) = fixture();
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=red",
    );
    let facts = FormatHookFacts::default();
    let session = context.session.unwrap();
    assert_eq!(borders(&inner, client, session, &facts)[0].style, "fg=red");
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=blue",
    );
    assert_eq!(borders(&inner, client, session, &facts)[0].style, "fg=blue");
    border_presentations_at(&inner, client, session, &facts, 1_700_000_001);
    inner.engine.set_format_now(1_700_000_001);
    borders(&inner, client, session, &facts);
    inner
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("set-environment", ["-g", "BORDER_UNUSED", "changed"]),
        )
        .unwrap();
    borders(&inner, client, session, &facts);
    let (window, _) = inner
        .engine
        .state
        .create_window(session, None, zz_mux::PaneKind::Terminal)
        .unwrap();
    inner.focused_windows.insert(client, window);
    let focused = borders(&inner, client, session, &facts);
    assert_ne!(focused[0].pane, context.pane.unwrap());
    assert_eq!(
        expansions(),
        if zz_mux::format_cache_knob() { 4 } else { 6 }
    );
}

#[test]
fn border_format_cache_keeps_clock_guards_for_raw_nested_and_time_modifier_sources() {
    let (mut inner, client, mut context) = fixture();
    let facts = FormatHookFacts::default();
    let session = context.session.unwrap();
    let second = 1_700_000_000;
    inner.engine.state.session_mut(session).unwrap().created = Some(1_700_000_000);
    set_style(&mut inner, &mut context, "@border-colour", "green");
    set_style(
        &mut inner,
        &mut context,
        "@border-time-choice",
        "#{?#{==:#{t/d:session_created},0},green,red}",
    );
    set_style(
        &mut inner,
        &mut context,
        "@border-outer",
        "#{E:@border-inner}",
    );
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "#{E:@border-outer}",
    );
    for source in [
        "fg=colour%S",
        "fg=colour#{t/d:session_created}",
        "fg=#{?#{==:#{t/r:session_created},0s},red,green}",
        "fg=#{T:@border-colour}",
        "fg=#{?pane_active,#{T:@border-colour},red}",
        "fg=#{?pane_active,#{E:@border-time-choice},red}",
    ] {
        inner.engine.set_format_now(second);
        set_style(&mut inner, &mut context, "@border-inner", source);
        let before = expansions();
        let first = borders(&inner, client, session, &facts);
        if zz_mux::format_cache_knob() {
            assert!(
                inner
                    .border_presentations_cache
                    .lock()
                    .as_ref()
                    .unwrap()
                    .clock_dependent
            );
        }
        let options = inner.engine.cached_format_option_snapshot();
        let clock_reads = Cell::new(0);
        assert!(
            cached_live_border_presentations(
                &inner,
                client,
                session,
                context.window,
                inner.engine.format_cache_revision().unwrap_or_default(),
                &options,
                || {
                    clock_reads.set(clock_reads.get() + 1);
                    second + 1
                },
            )
            .is_none(),
            "{source}"
        );
        assert_eq!(
            clock_reads.get(),
            usize::from(zz_mux::format_cache_knob()),
            "{source}"
        );
        let next_second = border_presentations_at(&inner, client, session, &facts, second + 1);
        assert!(!Arc::ptr_eq(&first, &next_second), "{source}");
        inner.engine.set_format_now(second + 1);
        let next_clock = borders(&inner, client, session, &facts);
        assert!(!Arc::ptr_eq(&next_second, &next_clock), "{source}");
        assert_eq!(expansions(), before + 3, "{source}");
        assert_eq!(
            next_clock,
            uncached_border_presentations(&inner, client, session, &facts)
        );
        if source == "fg=colour#{t/d:session_created}" {
            assert_eq!(first[0].style, "fg=colour0");
            assert_eq!(next_clock[0].style, "fg=colour1");
        }
    }
}

#[test]
fn border_format_cache_bypasses_loops_jobs_terminal_and_unknown_inputs() {
    let (mut inner, client, mut context) = fixture();
    let facts = FormatHookFacts::default();
    for source in [
        "#{W:fg=red}",
        "#{P:fg=red}",
        "#{S:fg=red}",
        "fg=#{?pane_marked,#(printf red),green}",
        "fg=colour#{cursor_x}",
        "fg=colour#{scroll_position}",
        "fg=#{UNKNOWN_BORDER_ENV}",
    ] {
        set_style(&mut inner, &mut context, "pane-active-border-style", source);
        let first = borders(&inner, client, context.session.unwrap(), &facts);
        let before = expansions();
        let second = borders(&inner, client, context.session.unwrap(), &facts);
        assert_eq!(second, first, "{source}");
        assert_eq!(expansions(), before + 1, "{source}");
        assert!(
            inner.border_presentations_cache.lock().is_none(),
            "{source}"
        );
    }
}

#[test]
fn border_format_cache_bypasses_oversized_retained_inputs() {
    let (mut inner, client, mut context) = fixture();
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{client_name}",
    );
    let facts = FormatHookFacts {
        client: Some(ClientFormatFacts {
            name: "x".repeat(BORDER_FORMAT_CACHE_BYTES),
            ..Default::default()
        }),
        ..Default::default()
    };
    borders(&inner, client, context.session.unwrap(), &facts);
    let before = expansions();
    borders(&inner, client, context.session.unwrap(), &facts);
    assert_eq!(expansions(), before + 1);
    assert!(inner.border_presentations_cache.lock().is_none());
}

#[test]
fn border_format_cache_reuses_arbitrary_captured_option_styles() {
    let (mut inner, client, mut context) = fixture();
    set_style(&mut inner, &mut context, "@border-choice", "0");
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?@border-choice,red,green}",
    );
    let facts = FormatHookFacts::default();
    let session = context.session.unwrap();
    assert_eq!(
        borders(&inner, client, session, &facts)[0].style,
        "fg=green"
    );
    borders(&inner, client, session, &facts);
    assert_eq!(expansions(), 1 + usize::from(!zz_mux::format_cache_knob()));
    set_style(&mut inner, &mut context, "@border-choice", "1");
    assert_eq!(borders(&inner, client, session, &facts)[0].style, "fg=red");
    assert_eq!(expansions(), 2 + usize::from(!zz_mux::format_cache_knob()));
}

#[test]
fn border_format_cache_tracks_linked_window_owner_attachments_in_the_same_second() {
    let (mut inner, owner_client, mut context) = fixture();
    let window = context.window.unwrap();
    let (viewer, _, _) = inner.engine.state.create_session("viewer").unwrap();
    let viewer_client = ClientId(4);
    inner
        .engine
        .state
        .session_mut(viewer)
        .unwrap()
        .windows
        .push(window);
    inner
        .attached
        .insert(viewer, BTreeSet::from([viewer_client]));
    inner.focused_windows.insert(viewer_client, window);
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?session_attached,green,red}",
    );
    let revision = inner.engine.format_cache_revision();
    let first = {
        let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
        borders(&inner, viewer_client, viewer, &facts)
    };
    assert_eq!(first[0].style, "fg=green");
    inner.suspended_clients.insert(owner_client);
    assert_eq!(inner.engine.format_cache_revision(), revision);
    let second = {
        let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
        borders(&inner, viewer_client, viewer, &facts)
    };
    assert_eq!(second[0].style, "fg=red");
    assert_eq!(expansions(), 2);
}

#[test]
fn border_format_cache_preserves_borrowed_window_client_callbacks() {
    let (mut inner, client, mut context) = fixture();
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?window_active_clients,green,red}",
    );
    let session = context.session.unwrap();
    let first = {
        let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
        borders(&inner, client, session, &facts)
    };
    assert_eq!(first[0].style, "fg=green");
    inner.attached.get_mut(&session).unwrap().clear();
    let second = {
        let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
        borders(&inner, client, session, &facts)
    };
    assert_eq!(second[0].style, "fg=red");
    assert_eq!(expansions(), 2);
    assert!(inner.border_presentations_cache.lock().is_none());
}

#[test]
fn border_format_cache_checks_borrowed_mode_counts_without_materializing_maps() {
    let (mut inner, client, mut context) = fixture();
    let first_pane = context.pane.unwrap();
    inner
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("split-window", ["-h"]),
        )
        .unwrap();
    let second_pane = context.pane.unwrap();
    for name in ["pane-border-style", "pane-active-border-style"] {
        set_style(
            &mut inner,
            &mut context,
            name,
            "fg=#{?#{==:#{pane_in_mode},2},red,green}",
        );
    }
    let session = context.session.unwrap();
    let first = {
        let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
        let first = borders(&inner, client, session, &facts);
        let second = borders(&inner, client, session, &facts);
        assert_eq!(first, second);
        assert_eq!(Arc::ptr_eq(&first, &second), zz_mux::format_cache_knob());
        assert!(facts.derived.pane_modes.get().is_none());
        assert!(facts.derived.copy_modes.get().is_none());
        first
    };
    assert!(first.iter().all(|pane| pane.style == "fg=green"));
    if zz_mux::format_cache_knob() {
        let cache = inner.border_presentations_cache.lock();
        let cache = cache.as_ref().unwrap();
        assert!(cache.callbacks.is_empty());
        assert!(
            cache
                .panes
                .iter()
                .all(|pane| { pane.pane_in_mode == Some(0) && pane.callback_values.is_empty() })
        );
    }
    let revision = inner.engine.format_cache_revision();
    for (active, inactive) in [(first_pane, second_pane), (second_pane, first_pane)] {
        inner.pane_modes.insert(inactive, Vec::new());
        inner
            .pane_modes
            .insert(active, vec![PaneModeRequest::Clock, PaneModeRequest::Clock]);
        let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
        let actual = borders(&inner, client, session, &facts);
        assert_eq!(inner.engine.format_cache_revision(), revision);
        assert_eq!(
            actual
                .iter()
                .find(|pane| pane.pane == active)
                .unwrap()
                .style,
            "fg=red"
        );
        assert_eq!(
            actual
                .iter()
                .find(|pane| pane.pane == inactive)
                .unwrap()
                .style,
            "fg=green"
        );
        assert!(facts.derived.pane_modes.get().is_none());
        assert!(facts.derived.copy_modes.get().is_none());
    }
}

#[test]
fn border_format_cache_checks_live_copy_sessions_without_materializing_maps() {
    let (mut inner, client, mut context) = fixture();
    let pane = context.pane.unwrap();
    let copy_client = ClientId(4);
    let terminal = Arc::new(TerminalSession::spawn_empty_with_appearance(
        64,
        Arc::new(TerminalAppearance::default()),
    ));
    let view = TerminalViewId(copy_client.0);
    terminal.attach_view(view);
    terminal.view_action(view, zz_terminal::TerminalViewAction::EnterCopyMode);
    let deadline = Instant::now() + Duration::from_secs(3);
    while terminal.copy_mode_facts(view).is_none() {
        assert!(Instant::now() < deadline, "copy facts did not become ready");
        std::thread::sleep(Duration::from_millis(5));
    }
    inner.terminals_mut().insert(pane, terminal);
    enter_copy_session(&mut inner, copy_client, pane).unwrap();
    inner
        .pane_modes
        .insert(pane, vec![PaneModeRequest::Clock, PaneModeRequest::Clock]);
    set_style(
        &mut inner,
        &mut context,
        "pane-active-border-style",
        "fg=#{?#{==:#{pane_in_mode},3},red,green}",
    );
    let session = context.session.unwrap();
    let revision = inner.engine.format_cache_revision();
    for (exiting, expected) in [(false, "fg=red"), (true, "fg=green"), (false, "fg=red")] {
        inner.copy_sessions.get_mut(&copy_client).unwrap().exiting = exiting;
        let owned = format_hook_facts(&inner);
        let facts = readonly_borrowed_format_hook_facts(
            &inner,
            CommandFormatSeed {
                client: Some(ClientFormatFacts {
                    name: "another-client".to_owned(),
                    ..Default::default()
                }),
                invoking: Some(client),
            },
        );
        assert_eq!(
            crate::status::FormatFactSource::pane_in_mode_count(&facts, pane),
            crate::status::FormatFactSource::pane_in_mode_count(&owned, pane)
        );
        let actual = borders(&inner, client, session, &facts);
        assert_eq!(actual[0].style, expected);
        assert_eq!(
            actual,
            uncached_border_presentations(&inner, client, session, &owned)
        );
        assert_eq!(inner.engine.format_cache_revision(), revision);
        assert!(facts.derived.pane_modes.get().is_none());
        assert!(facts.derived.copy_modes.get().is_none());
    }
    assert!(inner.terminals_mut().remove(&pane).is_some());
    let owned = format_hook_facts(&inner);
    let facts = readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default());
    let actual = borders(&inner, client, session, &facts);
    assert_eq!(actual[0].style, "fg=green");
    assert_eq!(
        actual,
        uncached_border_presentations(&inner, client, session, &owned)
    );
    assert!(facts.derived.pane_modes.get().is_none());
    assert!(facts.derived.copy_modes.get().is_none());
}

#[test]
fn pane_in_mode_typed_counts_preserve_owned_rows_and_provider_selection() {
    let (inner, _, context) = fixture();
    let pane = context.pane.unwrap();
    let owned = FormatHookFacts {
        pane_modes: Arc::new(BTreeMap::from([(pane, (2, "clock-mode"))])),
        copy_modes: Arc::new(BTreeMap::from([(pane, Vec::new())])),
        ..Default::default()
    };
    let facts = FormatHookFactsView {
        borrowed: readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default()),
        owned: Some(owned),
    };
    assert_eq!(
        crate::status::FormatFactSource::pane_in_mode_count(&facts, pane),
        3
    );
    let context = inner
        .engine
        .format_status_context(context.session, context.window, context.pane);
    assert_eq!(
        DaemonFormatHooks::command(&facts).variable("pane_in_mode", &context),
        Some("3".to_owned())
    );
    assert!(facts.borrowed.derived.pane_modes.get().is_none());
    assert!(facts.borrowed.derived.copy_modes.get().is_none());
    let facts = FormatHookFactsView {
        borrowed: readonly_borrowed_format_hook_facts(&inner, CommandFormatSeed::default()),
        owned: None,
    };
    assert_eq!(
        DaemonFormatHooks::command(&facts).variable("pane_in_mode", &context),
        Some("0".to_owned())
    );
    assert!(facts.borrowed.derived.pane_modes.get().is_none());
    assert!(facts.borrowed.derived.copy_modes.get().is_none());
}

#[derive(Default)]
struct ChangingModeCount {
    facts: FormatHookFacts,
    count: Cell<usize>,
    reads: Cell<usize>,
    change_after_read: Cell<bool>,
}

impl crate::status::FormatFactSource for ChangingModeCount {
    fn agent_states(&self) -> &BTreeMap<PaneId, zz_protocol::AgentPaneWire> {
        &self.facts.agent_states
    }

    fn terminals(&self) -> &BTreeMap<PaneId, Arc<TerminalSession>> {
        &self.facts.terminals
    }

    fn pane_pipes(&self) -> &BTreeMap<PaneId, u32> {
        &self.facts.pane_pipes
    }

    fn session_attachments(&self) -> &BTreeMap<SessionId, (usize, String)> {
        &self.facts.session_attachments
    }

    fn session_last_attached(&self) -> &BTreeMap<SessionId, u64> {
        &self.facts.session_last_attached
    }

    fn unseen_changes(&self) -> &BTreeSet<PaneId> {
        &self.facts.unseen_changes
    }

    fn window_clients(&self, _context: &zz_mux::StatusContext) -> &BTreeMap<WindowId, Vec<String>> {
        &self.facts.window_clients
    }

    fn buffer(&self) -> Option<&BufferFormatFacts> {
        self.facts.buffer.as_ref()
    }

    fn client(&self) -> Option<&ClientFormatFacts> {
        self.facts.client.as_ref()
    }

    fn clients(&self, _context: &zz_mux::StatusContext) -> &[zz_mux::FormatClientRow] {
        &self.facts.clients
    }

    fn client_environment(&self) -> Option<&Arc<ClientEnvironmentBlob>> {
        self.facts.client_environment.as_ref()
    }

    fn message(&self) -> Option<&MessageFormatFacts> {
        self.facts.message.as_ref()
    }

    fn mux(&self) -> &zz_mux::FormatFacts {
        &self.facts.mux
    }

    fn copy_modes(&self) -> &BTreeMap<PaneId, Vec<(String, Arc<zz_terminal::CopyModeFacts>)>> {
        &self.facts.copy_modes
    }

    fn pane_modes(&self) -> &BTreeMap<PaneId, (usize, &'static str)> {
        &self.facts.pane_modes
    }

    fn pane_in_mode_count(&self, _pane: PaneId) -> usize {
        self.reads.set(self.reads.get() + 1);
        let count = self.count.get();
        if self.change_after_read.get() {
            self.count.set(count ^ 1);
        }
        count
    }
}

#[test]
fn border_format_cache_pins_one_mode_count_for_style_and_expected_callbacks() {
    let (mut inner, client, mut context) = fixture();
    let source = "fg=#{?pane_in_mode,red,green},bg=#{?pane_in_mode,red,green}";
    set_style(&mut inner, &mut context, "pane-active-border-style", source);
    let facts = ChangingModeCount {
        change_after_read: Cell::new(true),
        ..Default::default()
    };
    let pane = context.pane.unwrap();
    let format_context =
        inner
            .engine
            .format_status_context(context.session, context.window, context.pane);
    let mut hooks = DaemonFormatHooks::command(&facts);
    let count = crate::status::FormatFactSource::pane_in_mode_count(&facts, pane);
    hooks.set_pane_in_mode_count(pane, count);
    assert_eq!(
        crate::status::expand_style(source, &format_context, &mut hooks),
        "fg=green,bg=green"
    );
    assert_eq!(facts.reads.get(), 1);
    if zz_mux::format_cache_knob() {
        facts.reads.set(0);
        facts.count.set(0);
        let session = context.session.unwrap();
        let first = borders(&inner, client, session, &facts);
        assert_eq!(first[0].style, "fg=green,bg=green");
        assert_eq!(facts.reads.get(), 1);
        assert_eq!(
            inner
                .border_presentations_cache
                .lock()
                .as_ref()
                .unwrap()
                .panes[0]
                .pane_in_mode,
            Some(0)
        );
        facts.change_after_read.set(false);
        facts.count.set(0);
        let second = borders(&inner, client, session, &facts);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(facts.reads.get(), 2);
        facts.count.set(1);
        let third = borders(&inner, client, session, &facts);
        assert_eq!(third[0].style, "fg=red,bg=red");
        assert!(!Arc::ptr_eq(&second, &third));
        assert_eq!(facts.reads.get(), 4);
        assert_eq!(
            inner
                .border_presentations_cache
                .lock()
                .as_ref()
                .unwrap()
                .panes[0]
                .pane_in_mode,
            Some(1)
        );
    }
}
