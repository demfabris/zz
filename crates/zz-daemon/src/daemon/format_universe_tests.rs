use super::*;
use crate::status::MODE_FORMATS;
use zz_mux::with_eager_universe;
use zz_protocol::Axis;

const LOOPS: &str = "#{S:#{session_name}#{?session_active,*,}:[#{W:#{window_index}#{P:.#{pane_id}}}]}#{FOO}#{E:@fmt}#{N/s:beta}#{Vg:#{environ_name};}#{Og:#{option_name};}";

struct Fixture {
    shared: Arc<Shared>,
    control: ClientId,
    mailbox: Arc<OutboundMailbox>,
    alpha: SessionId,
    alpha_window: WindowId,
    alpha_pane: PaneId,
}

fn fixture() -> Fixture {
    let shared = Arc::new(Shared::new(1));
    let (alpha, alpha_window, alpha_pane) = {
        let mut inner = shared.inner.lock();
        let engine = &mut inner.engine;
        let (alpha, alpha_window, alpha_pane) = engine.state.create_session("alpha").unwrap();
        let split = engine
            .state
            .split_pane(alpha_pane, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let (logs, logs_pane) = engine
            .state
            .create_window(alpha, Some("logs".to_owned()), PaneKind::Terminal)
            .unwrap();
        engine
            .state
            .split_pane(logs_pane, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let (_, _, beta_pane) = engine.state.create_session("beta").unwrap();
        engine
            .state
            .split_pane(beta_pane, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let mut context = ExecutionContext::new(Some(alpha), Some(alpha_window), Some(split));
        for args in [
            vec!["set-environment", "-g", "FOO", "foo"],
            vec!["set-environment", "-t", "beta", "BAR", "bar"],
            vec!["set-option", "-g", "@fmt", "#{W:#{window_name}!}"],
            vec!["set-option", "-w", "-t", &logs.to_string(), "@tag", "l"],
            vec!["select-pane", "-m", "-t", &split.to_string()],
            vec!["resize-pane", "-Z", "-t", &split.to_string()],
            vec!["set-option", "-g", "status", "3"],
            vec!["set-option", "-g", "status-left", "#{S:#{session_name} }"],
            vec!["set-option", "-g", "status-right", "#{FOO}#{T:@fmt}"],
            vec!["set-option", "-g", "status-format[1]", LOOPS],
            vec![
                "set-option",
                "-g",
                "status-format[2]",
                "#{W:#{next_@tag}#{prev_@tag}#{window_name}}",
            ],
            vec!["set-option", "-g", "set-titles", "on"],
            vec![
                "set-option",
                "-g",
                "set-titles-string",
                "#{P:#{pane_index}}#{BAR}",
            ],
            vec![
                "set-option",
                "-g",
                "pane-border-style",
                "fg=#{?#{N/s:beta},red,blue}",
            ],
            vec![
                "set-option",
                "-g",
                "pane-active-border-style",
                "fg=#{?#{==:#{S:x},xx},green,yellow}",
            ],
            vec![
                "set-option",
                "-g",
                "copy-mode-position-format",
                "#{W:#{window_index}}/#{FOO}",
            ],
            vec![
                "set-option",
                "-g",
                "copy-mode-match-style",
                "fg=#{?#{P:x},red,blue}",
            ],
        ] {
            engine
                .execute(
                    &mut context,
                    &CommandInvocation::new(args[0], args[1..].iter().copied()),
                )
                .unwrap_or_else(|error| panic!("{args:?}: {error:?}"));
        }
        (alpha, alpha_window, alpha_pane)
    };
    let mailbox = OutboundMailbox::new();
    let (control, _) = shared.register_subscribed(
        ClientKind::Control,
        Some("watcher".to_owned()),
        None,
        Arc::clone(&mailbox),
    );
    shared.attach(control, alpha).unwrap();
    super::tests::take_reliable_messages(&mailbox);
    Fixture {
        shared,
        control,
        mailbox,
        alpha,
        alpha_window,
        alpha_pane,
    }
}

fn lazy_and_eager<T: PartialEq + std::fmt::Debug>(observe: impl Fn(&Fixture) -> T) {
    let lazy = observe(&fixture());
    let eager = with_eager_universe(|| observe(&fixture()));
    assert_eq!(lazy, eager);
}

#[test]
fn status_requests_render_the_same_line_on_a_detached_universe() {
    lazy_and_eager(|fixture| {
        let request = {
            let inner = fixture.shared.inner.lock();
            status_request(
                &inner,
                fixture.control,
                &inner.engine.state.snapshot(),
                Arc::new(inner.engine.format_option_snapshot()),
                format_hook_facts(&inner),
                true,
                FormatNeeds::NONE,
            )
        };
        let line = StatusRenderer::default().render_forced(&request);
        assert!(line.rows.iter().any(|row| row.contains("alpha")));
        (line.left, line.right, line.title, line.rows)
    });
}

const PARTIAL_ROWS: &[&str] = &[
    "#{S:[#{session_name}:#{P:#{pane_index}}]}",
    "#{S:<#{Ow:#{option_name}=#{option_value}}>}",
    "#{S:<#{Op:#{option_name}=#{option_value}}>}",
    "#{W:<#{Op:#{option_name}=#{option_value}}>}",
    "#{S:#{session_name}}",
    "#{FOO}#{Vg:#{environ_name};}",
    "#{Og:#{option_name};}",
];

fn run_in_engine(fixture: &Fixture, args: &[&str]) {
    let mut context = ExecutionContext::default();
    fixture
        .shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap_or_else(|error| panic!("{args:?}: {error:?}"));
}

fn one_status_row(fixture: &Fixture, row: &str) {
    for args in [
        &["set-option", "-g", "status-left", ""][..],
        &["set-option", "-g", "status-right", ""],
        &["set-option", "-g", "status-format[0]", "#{session_name}"],
        &["set-option", "-g", "status-format[1]", row],
        &["set-option", "-g", "status-format[2]", "x"],
        &["set-option", "-g", "set-titles", "off"],
        &["set-option", "-w", "-t", "beta:0", "@ww", "bw"],
        &["set-option", "-p", "-t", "beta:0.0", "@pp", "b0"],
        &["set-option", "-p", "-t", "beta:0.1", "@pp", "b1"],
        &["set-option", "-p", "-t", "alpha:1.0", "@pp", "l0"],
    ] {
        run_in_engine(fixture, args);
    }
}

fn status_request_for(fixture: &Fixture, job_needs: FormatNeeds) -> StatusRequest {
    let inner = fixture.shared.inner.lock();
    status_request(
        &inner,
        fixture.control,
        &inner.engine.state.snapshot(),
        Arc::new(inner.engine.format_option_snapshot()),
        format_hook_facts(&inner),
        true,
        job_needs,
    )
}

#[test]
fn status_requests_with_partial_needs_render_what_the_eager_universe_renders() {
    for row in PARTIAL_ROWS {
        let fixture = fixture();
        one_status_row(&fixture, row);
        let request = status_request_for(&fixture, FormatNeeds::NONE);
        assert!(
            !request.context.format_universe_covers(FormatNeeds::ALL),
            "{row} detached everything"
        );
        lazy_and_eager(|fixture| {
            one_status_row(fixture, row);
            let line = StatusRenderer::default()
                .render_forced(&status_request_for(fixture, FormatNeeds::NONE));
            line.rows
        });
    }
    let fixture = fixture();
    one_status_row(&fixture, PARTIAL_ROWS[0]);
    let line =
        StatusRenderer::default().render_forced(&status_request_for(&fixture, FormatNeeds::NONE));
    assert!(line.rows[1].contains("[beta:01]"), "{:?}", line.rows);
    one_status_row(&fixture, PARTIAL_ROWS[2]);
    let line =
        StatusRenderer::default().render_forced(&status_request_for(&fixture, FormatNeeds::NONE));
    assert!(line.rows[1].contains("@pp=b"), "{:?}", line.rows);
}

#[test]
fn status_job_output_reads_parts_the_templates_do_not() {
    let fixture = fixture();
    one_status_row(&fixture, "#(echo '##{S:##{session_name}.}')");
    let mut renderer = StatusRenderer::default();
    assert_eq!(
        renderer
            .render_changed(&[status_request_for(&fixture, FormatNeeds::NONE)])
            .len(),
        1
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !renderer.poll_jobs().contains(&fixture.control) {
        assert!(Instant::now() < deadline, "the status job never finished");
        thread::sleep(Duration::from_millis(10));
    }
    let needs = renderer
        .job_needs()
        .lock()
        .get(&fixture.control)
        .copied()
        .unwrap_or_default();
    assert!(needs.contains(FormatNeeds::SESSIONS));
    assert!(
        renderer
            .render_changed(&[status_request_for(&fixture, FormatNeeds::NONE)])
            .is_empty()
    );
    assert!(renderer.poll_jobs().contains(&fixture.control));
    let changed = renderer.render_changed(&[status_request_for(&fixture, needs)]);
    assert_eq!(changed.len(), 1);
    assert!(
        changed[0].1.rows[1] == "alpha.beta.",
        "{:?}",
        changed[0].1.rows
    );
}

#[test]
fn mode_requests_render_the_same_presentation_on_a_detached_universe() {
    lazy_and_eager(|fixture| {
        let inner = fixture.shared.inner.lock();
        let contexts = inner
            .engine
            .format_context_snapshot(FormatClient::Attached(fixture.alpha));
        let needs = inner.engine.format_needs(MODE_FORMATS);
        let request = mode_request(
            &inner,
            &contexts,
            needs,
            false,
            fixture.alpha,
            fixture.alpha_pane,
            false,
            (3, 9),
        )
        .expect("mode request");
        drop(contexts);
        let modes =
            crate::status::expand_modes(&[request], &format_hook_facts(&inner), &inner.engine);
        assert!(modes[0].position.contains("foo"));
        modes
            .into_iter()
            .map(|mode| (mode.position, mode.match_style))
            .collect::<Vec<_>>()
    });
}

#[test]
fn border_presentations_expand_the_same_styles_on_one_shared_universe() {
    lazy_and_eager(|fixture| {
        let inner = fixture.shared.inner.lock();
        border_presentations(
            &inner,
            fixture.control,
            fixture.alpha,
            &format_hook_facts(&inner),
        )
        .iter()
        .map(|border| (border.pane, border.style.clone()))
        .collect::<Vec<_>>()
    });
}

#[test]
fn chooser_rows_expand_the_same_text_on_one_universe_per_session() {
    lazy_and_eager(|fixture| {
        let inner = fixture.shared.inner.lock();
        chooser_presentation::client_chooser_rows(&inner, Some(LOOPS), Some("#{N/s:alpha}"))
            .into_iter()
            .map(|row| (row.text, row.matches))
            .collect::<Vec<_>>()
    });
}

#[test]
fn control_subscriptions_report_the_same_values_on_one_universe_per_client() {
    lazy_and_eager(|fixture| {
        let mut context = ExecutionContext::new(
            Some(fixture.alpha),
            Some(fixture.alpha_window),
            Some(fixture.alpha_pane),
        );
        for value in [
            format!("loops::{LOOPS}"),
            "panes:%*:#{pane_id}#{W:#{window_index}}#{FOO}".to_owned(),
            "windows:@*:#{window_name}#{P:#{pane_id}}".to_owned(),
        ] {
            fixture
                .shared
                .execute(
                    fixture.control,
                    ClientKind::Control,
                    &mut context,
                    &CommandInvocation::new("refresh-client", ["-B", &value]),
                )
                .unwrap();
        }
        fixture.shared.refresh_control_subscriptions();
        let mut values = super::tests::take_reliable_messages(&fixture.mailbox)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::SubscriptionChanged {
                            name,
                            window,
                            pane,
                            value,
                            ..
                        },
                    ..
                }) => Some((name, window, pane, value)),
                _ => None,
            })
            .collect::<Vec<_>>();
        values.sort();
        assert!(!values.is_empty());
        values
    });
}

#[test]
fn format_monitors_and_hooks_expand_the_same_bodies_on_a_lazy_universe() {
    lazy_and_eager(|fixture| {
        let mut context = ExecutionContext::new(
            Some(fixture.alpha),
            Some(fixture.alpha_window),
            Some(fixture.alpha_pane),
        );
        let run = |context: &mut ExecutionContext, args: &[&str]| {
            fixture
                .shared
                .execute(
                    fixture.control,
                    ClientKind::Control,
                    context,
                    &CommandInvocation::new(args[0], args[1..].iter().copied()),
                )
                .unwrap_or_else(|error| panic!("{args:?}: {error:?}"))
                .output
        };
        run(
            &mut context,
            &[
                "set-hook",
                "-g",
                "after-select-pane",
                "set -gF @after '#{S:#{session_name}}#{W:#{window_index}}#{FOO}'",
            ],
        );
        run(
            &mut context,
            &[
                "set-hook",
                "-g",
                "-B",
                "@watch:@*:#{window_name}#{P:#{pane_id}}#{FOO}",
                "set -gF @fired '#{S:#{session_name}}'",
            ],
        );
        fixture.shared.run_format_monitors();
        run(
            &mut context,
            &[
                "rename-window",
                "-t",
                &fixture.alpha_window.to_string(),
                "renamed",
            ],
        );
        fixture.shared.run_format_monitors();
        run(
            &mut context,
            &["select-pane", "-t", &fixture.alpha_pane.to_string()],
        );
        let after = run(&mut context, &["show-options", "-gqv", "@after"]);
        let fired = run(&mut context, &["show-options", "-gqv", "@fired"]);
        assert_eq!(after.to_string(), "alphabeta01foo");
        assert_eq!(fired.to_string(), "alphabeta");
        (after, fired)
    });
}

#[test]
fn terminal_features_and_overrides_reach_a_connected_client_on_the_next_read() {
    let fixture = fixture();
    let mailbox = OutboundMailbox::new();
    let (client, _) = fixture.shared.register_subscribed(
        ClientKind::Interactive,
        Some("tty".to_owned()),
        None,
        Arc::clone(&mailbox),
    );
    {
        let mut inner = fixture.shared.inner.lock();
        inner.clients.entry(client).or_default().has_terminal = true;
        inner
            .clients
            .entry(client)
            .or_default()
            .environment
            .replace(Arc::new(ClientEnvironmentBlob::from_map(BTreeMap::from([
                ("TERM".into(), "xterm-256color".into()),
            ]))));
    }
    let read = || {
        let inner = fixture.shared.inner.lock();
        let terminal = client_format_facts(&inner, client, fixture.alpha)
            .terminal
            .expect("xterm-256color terminfo");
        (
            client_feature_mask(&inner, client),
            terminal.has_capability("smcup"),
        )
    };
    let run = |args: &[&str]| {
        let mut context = ExecutionContext::default();
        fixture
            .shared
            .inner
            .lock()
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new(args[0], args[1..].iter().copied()),
            )
            .unwrap();
    };
    let (before, before_override) = read();
    assert!(before_override);
    run(&[
        "set-option",
        "-as",
        "terminal-features",
        "xterm-256color:sync",
    ]);
    let (after, _) = read();
    assert_eq!(
        after & !before,
        crate::terminal_features::terminal_feature_mask(["sync"])
    );
    run(&[
        "set-option",
        "-as",
        "terminal-overrides",
        "xterm-256color:smcup@",
    ]);
    assert!(!read().1);
}
