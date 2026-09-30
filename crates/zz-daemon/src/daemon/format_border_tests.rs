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
) -> Vec<zz_protocol::PaneBorderPresentation> {
    border_presentations_at(inner, client, session, facts, 1_700_000_000)
}

fn expansions() -> usize {
    BORDER_FORMAT_EXPANSIONS.with(Cell::get)
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
fn border_format_cache_invalidates_options_target_clock_and_environment_revision() {
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
    assert_eq!(expansions(), 6);
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
