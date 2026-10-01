use super::tests::take_reliable_messages;
use super::*;

struct Target {
    client: ClientId,
    mailbox: Arc<OutboundMailbox>,
    context: ExecutionContext,
    row: String,
}

struct Fixture {
    shared: Arc<Shared>,
    targets: [Target; 3],
    invoking: ClientId,
}

fn engine_command(shared: &Shared, args: &[&str]) -> Execution {
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut ExecutionContext::default(),
            &CommandInvocation::new(args[0], args[1..].iter().copied()),
        )
        .unwrap()
}

fn fixture() -> Fixture {
    let shared = Arc::new(Shared::new(1));
    *shared.startup_ready.lock() = true;
    let (alpha, beta) = {
        let mut inner = shared.inner.lock();
        let (session, window, pane) = inner.engine.state.create_session("alpha").unwrap();
        let alpha = ExecutionContext::new(Some(session), Some(window), Some(pane));
        let (session, window, pane) = inner.engine.state.create_session("beta").unwrap();
        let beta = ExecutionContext::new(Some(session), Some(window), Some(pane));
        (alpha, beta)
    };
    for (name, value) in [
        ("status-left", ""),
        ("status-right", ""),
        ("status-format[0]", "WRONG-GLOBAL-ROW"),
        ("@flavour", "GLOBAL"),
    ] {
        engine_command(&shared, &["set-option", "-g", name, value]);
    }
    for (session, flavour, position) in [("alpha", "A", "top"), ("beta", "B", "bottom")] {
        for (name, value) in [
            ("@flavour", flavour),
            ("status-position", position),
            (
                "status-format[0]",
                "#{session_name}|#{client_name}|#{client_width}|#{@flavour}|#{status-position}",
            ),
        ] {
            engine_command(&shared, &["set-option", "-t", session, name, value]);
        }
    }
    let targets = [
        (
            "alpha-low",
            alpha.clone(),
            80,
            10,
            "alpha|alpha-low|80|A|top",
        ),
        (
            "alpha-high",
            alpha.clone(),
            100,
            30,
            "alpha|alpha-high|100|A|top",
        ),
        (
            "beta-viewer",
            beta.clone(),
            120,
            20,
            "beta|beta-viewer|120|B|bottom",
        ),
    ]
    .map(|(name, context, width, activity, row)| {
        let mailbox = OutboundMailbox::new();
        let (client, _) = shared.register_subscribed(
            ClientKind::Interactive,
            Some(name.to_owned()),
            None,
            Arc::clone(&mailbox),
        );
        let mut inner = shared.inner.lock();
        inner
            .attached
            .entry(context.session.unwrap())
            .or_default()
            .insert(client);
        inner
            .client_entry(client)
            .focused_window
            .replace(context.window.unwrap());
        inner.client_entry(client).size.replace((width, 24));
        inner.client_entry(client).activity.replace(activity);
        inner.client_entry(client).has_terminal = true;
        Target {
            client,
            mailbox,
            context,
            row: row.to_owned(),
        }
    });
    {
        let mut inner = shared.inner.lock();
        inner
            .engine
            .mark_session_active_at(alpha.session.unwrap(), 10);
        inner
            .engine
            .mark_session_active_at(beta.session.unwrap(), 20);
    }
    for target in &targets {
        take_reliable_messages(&target.mailbox);
    }
    Fixture {
        shared,
        targets,
        invoking: ClientId(u64::MAX - 1),
    }
}

fn execute(
    fixture: &Fixture,
    context: &mut ExecutionContext,
    args: &[&str],
) -> Result<Execution, DaemonError> {
    fixture.shared.execute(
        fixture.invoking,
        ClientKind::Command,
        context,
        &CommandInvocation::new(args[0], args[1..].iter().copied()),
    )
}

fn assert_status(fixture: &Fixture, selected: usize, expected: &str) {
    for (index, target) in fixture.targets.iter().enumerate() {
        let statuses = take_reliable_messages(&target.mailbox)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::StatusChanged { status },
                    ..
                }) => Some(status),
                _ => None,
            })
            .collect::<Vec<_>>();
        if index == selected {
            assert_eq!(statuses.len(), 1);
            assert_eq!(statuses[0].rows, [expected]);
        } else {
            assert!(
                statuses.is_empty(),
                "unexpected status for client {:?}",
                target.client
            );
        }
    }
}

fn assert_attachment_restored(context: &ExecutionContext, before: &ExecutionContext) {
    assert_eq!(
        (context.session, context.window, context.pane),
        (before.session, before.window, before.pane)
    );
    assert_eq!(
        context.attached_client_context(),
        before.attached_client_context()
    );
    assert_eq!(context.format_client(), before.format_client());
    assert_eq!(
        context.target_format_client(),
        before.target_format_client()
    );
    assert_eq!(context.has_client_terminal(), before.has_client_terminal());
    assert_eq!(context.has_no_client(), before.has_no_client());
    assert_eq!(context.replay_client(), before.replay_client());
    assert_eq!(
        context.control_command_target(),
        before.control_command_target()
    );
    assert_eq!(context.format_variables, before.format_variables);
}

#[test]
fn refresh_command_targetless_selection_preserves_origin_and_activity_precedence() {
    let fixture = fixture();
    let mut context = fixture.targets[0].context.clone();
    execute(&fixture, &mut context, &["refresh-client", "-S"]).unwrap();
    assert_status(&fixture, 2, &fixture.targets[2].row);
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.invoking)
        .origin
        .replace(fixture.targets[0].context.pane.unwrap());
    execute(&fixture, &mut context, &["refresh-client", "-S"]).unwrap();
    assert_status(&fixture, 1, &fixture.targets[1].row);
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.targets[0].client)
        .activity
        .replace(40);
    execute(&fixture, &mut context, &["refresh-client", "-S"]).unwrap();
    assert_status(&fixture, 0, &fixture.targets[0].row);
    {
        let mut inner = fixture.shared.inner.lock();
        inner
            .client_mut(fixture.invoking)
            .and_then(|c| c.origin.take());
        inner
            .engine
            .mark_session_active_at(fixture.targets[0].context.session.unwrap(), 30);
    }
    execute(&fixture, &mut context, &["refresh-client", "-S"]).unwrap();
    assert_status(&fixture, 0, &fixture.targets[0].row);
}

#[test]
fn refresh_command_explicit_targets_keep_scoped_options_and_client_facts_fresh() {
    let fixture = fixture();
    let mut context = fixture.targets[2].context.clone();
    for selector in [
        "alpha-high".to_owned(),
        format!("device-{}", fixture.targets[1].client.0),
    ] {
        execute(
            &fixture,
            &mut context,
            &["refresh-client", "-S", "-t", &selector],
        )
        .unwrap();
        assert_status(&fixture, 1, &fixture.targets[1].row);
    }
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.targets[1].client)
        .size
        .replace((121, 24));
    engine_command(
        &fixture.shared,
        &["set-option", "-t", "alpha", "@flavour", "A2"],
    );
    engine_command(
        &fixture.shared,
        &["set-option", "-t", "alpha", "status-position", "bottom"],
    );
    execute(
        &fixture,
        &mut context,
        &["refresh-client", "-S", "-t", "alpha-high"],
    )
    .unwrap();
    assert_status(&fixture, 1, "alpha|alpha-high|121|A2|bottom");
    execute(
        &fixture,
        &mut context,
        &["refresh-client", "-S", "-t", "beta-viewer"],
    )
    .unwrap();
    assert_status(&fixture, 2, &fixture.targets[2].row);
}

#[test]
fn refresh_command_native_and_stored_aliases_preserve_targets_and_group_continuations() {
    let fixture = fixture();
    let mut context = fixture.targets[0].context.clone();
    engine_command(
        &fixture.shared,
        &[
            "set-option",
            "-s",
            "command-alias[90]",
            "refreshfmt=refresh-client",
        ],
    );
    for command in ["refresh", "refreshfmt"] {
        execute(
            &fixture,
            &mut context,
            &[command, "-S", "-t", "beta-viewer"],
        )
        .unwrap();
        assert_status(&fixture, 2, &fixture.targets[2].row);
    }
    engine_command(
        &fixture.shared,
        &[
            "set-option",
            "-s",
            "command-alias[91]",
            "refreshgroup=refresh-client -S -t alpha-high ; display-message -p 'continued:#{session_name}'",
        ],
    );
    let result = execute(&fixture, &mut context, &["refreshgroup"]).unwrap();
    assert_status(&fixture, 1, &fixture.targets[1].row);
    assert_eq!(result.output.trim(), "continued:alpha");
}

#[test]
fn refresh_command_present_replay_provenance_restores_attachment_on_success_and_error() {
    let fixture = fixture();
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.invoking)
        .origin
        .replace(fixture.targets[0].context.pane.unwrap());
    let mut context = fixture.targets[2].context.clone();
    let attached = &fixture.targets[0].context;
    context.set_attached_client_context(Some((
        attached.session.unwrap(),
        attached.window.unwrap(),
        attached.pane.unwrap(),
    )));
    context.set_format_client(FormatClient::NoClient);
    context.set_replay_client(Some(fixture.targets[2].client));
    context
        .format_variables
        .insert("kept".to_owned(), "original".to_owned());
    let before = context.clone();
    assert_eq!(
        client_terminal(
            &fixture.shared.inner.lock(),
            fixture.targets[2].client,
            ClientKind::Interactive
        ),
        ClientTerminal::Present
    );
    execute(&fixture, &mut context, &["refresh-client", "-S"]).unwrap();
    assert_status(&fixture, 1, &fixture.targets[1].row);
    assert_attachment_restored(&context, &before);
    assert!(
        execute(
            &fixture,
            &mut context,
            &["refresh-client", "-S", "-t", "missing-viewer"]
        )
        .is_err()
    );
    assert_attachment_restored(&context, &before);
}

#[test]
fn refresh_command_after_and_error_hooks_recompute_their_format_context_and_facts() {
    let fixture = fixture();
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.invoking)
        .origin
        .replace(fixture.targets[0].context.pane.unwrap());
    let mut context = fixture.targets[2].context.clone();
    context.set_format_client(FormatClient::NoClient);
    for (hook, option) in [
        ("after-refresh-client", "@refresh-after"),
        ("command-error", "@refresh-error"),
    ] {
        engine_command(
            &fixture.shared,
            &[
                "set-hook",
                "-g",
                hook,
                &format!(
                    "set-option -gF {option} '#{{hook}}|#{{session_name}}|#{{client_name}}|#{{@flavour}}|#{{status-position}}'"
                ),
            ],
        );
    }
    let before = context.clone();
    execute(
        &fixture,
        &mut context,
        &["refresh-client", "-S", "-t", "alpha-low"],
    )
    .unwrap();
    assert_status(&fixture, 0, &fixture.targets[0].row);
    assert_attachment_restored(&context, &before);
    assert_eq!(
        engine_command(&fixture.shared, &["show-options", "-gqv", "@refresh-after"])
            .output
            .trim(),
        "after-refresh-client|beta|alpha-high|B|bottom"
    );
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.targets[0].client)
        .activity
        .replace(40);
    engine_command(
        &fixture.shared,
        &["set-option", "-t", "beta", "@flavour", "B2"],
    );
    assert!(
        execute(
            &fixture,
            &mut context,
            &["refresh-client", "-S", "-t", "missing-viewer"]
        )
        .is_err()
    );
    assert_attachment_restored(&context, &before);
    assert_eq!(
        engine_command(&fixture.shared, &["show-options", "-gqv", "@refresh-error"])
            .output
            .trim(),
        "command-error|beta|alpha-low|B2|bottom"
    );
}

#[test]
fn literal_command_routing_preserves_scoped_targets_and_stored_format_text() {
    let fixture = fixture();
    let mut context = fixture.targets[2].context.clone();
    context.set_format_client(FormatClient::NoClient);
    let before = context.clone();
    let literal = "literal:#{session_name}|#{client_name}|#{@flavour}";
    execute(
        &fixture,
        &mut context,
        &["set-option", "-t", "alpha", "@route-literal", literal],
    )
    .unwrap();
    assert_attachment_restored(&context, &before);
    assert_eq!(
        engine_command(
            &fixture.shared,
            &["show-options", "-t", "alpha", "-qv", "@route-literal"]
        )
        .output,
        literal
    );
    execute(
        &fixture,
        &mut context,
        &[
            "set-window-option",
            "-t",
            "alpha",
            "window-status-format",
            literal,
        ],
    )
    .unwrap();
    assert_attachment_restored(&context, &before);
    assert_eq!(
        engine_command(
            &fixture.shared,
            &[
                "show-window-options",
                "-t",
                "alpha",
                "-v",
                "window-status-format"
            ]
        )
        .output,
        literal
    );
    execute(
        &fixture,
        &mut context,
        &[
            "bind-key",
            "-T",
            "route-literals",
            "-N",
            literal,
            "x",
            "display-message",
            literal,
        ],
    )
    .unwrap();
    assert_attachment_restored(&context, &before);
    let binding = execute(
        &fixture,
        &mut context,
        &[
            "list-keys",
            "-T",
            "route-literals",
            "-F",
            "#{key_note}|#{key_command}",
        ],
    )
    .unwrap();
    assert!(binding.output.starts_with(literal));
    assert!(binding.output.contains("#{client_name}"));
    assert!(binding.output.contains("display-message"));
}

#[test]
fn literal_command_routing_recomputes_immediate_after_and_error_hook_facts() {
    let fixture = fixture();
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.invoking)
        .origin
        .replace(fixture.targets[0].context.pane.unwrap());
    for (hook, option) in [
        ("@option-changed", "@route-immediate"),
        ("after-set-option", "@route-after"),
        ("command-error", "@route-error"),
    ] {
        engine_command(
            &fixture.shared,
            &[
                "set-hook",
                "-g",
                hook,
                &format!(
                    "set-option -gF {option} '#{{hook}}|#{{session_name}}|#{{client_name}}|#{{@flavour}}|#{{status-position}}|#{{hook_option}}'"
                ),
            ],
        );
    }
    let mut context = fixture.targets[2].context.clone();
    context.set_format_client(FormatClient::NoClient);
    let before = context.clone();
    execute(
        &fixture,
        &mut context,
        &["set-option", "-g", "@route-value", "stored:#{client_name}"],
    )
    .unwrap();
    assert_attachment_restored(&context, &before);
    for (option, expected) in [
        (
            "@route-immediate",
            "@option-changed|beta|alpha-high|B|bottom|@route-value",
        ),
        ("@route-after", "after-set-option|beta|alpha-high|B|bottom|"),
    ] {
        assert_eq!(
            engine_command(&fixture.shared, &["show-options", "-gqv", option])
                .output
                .trim(),
            expected
        );
    }
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.targets[0].client)
        .activity
        .replace(40);
    for args in [
        vec!["set-option", "definitely-invalid-format-option", "value"],
        vec!["set-option", "-Z", "@route-invalid", "value"],
    ] {
        assert!(execute(&fixture, &mut context, &args).is_err());
        assert_attachment_restored(&context, &before);
        assert_eq!(
            engine_command(&fixture.shared, &["show-options", "-gqv", "@route-error"])
                .output
                .trim(),
            "command-error|beta|alpha-low|B|bottom|"
        );
    }
    context.set_replay_client(Some(fixture.targets[2].client));
    let before_present = context.clone();
    execute(
        &fixture,
        &mut context,
        &[
            "set-option",
            "-g",
            "@route-present",
            "stored:#{client_name}",
        ],
    )
    .unwrap();
    assert_attachment_restored(&context, &before_present);
    assert_eq!(
        engine_command(
            &fixture.shared,
            &["show-options", "-gqv", "@route-immediate"]
        )
        .output
        .trim(),
        "@option-changed|beta|beta-viewer|B|bottom|@route-present"
    );
}

#[test]
fn literal_routing_keeps_formatted_setters_sources_and_listings_on_client_fact_path() {
    let fixture = fixture();
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.invoking)
        .origin
        .replace(fixture.targets[0].context.pane.unwrap());
    let mut context = fixture.targets[2].context.clone();
    context.set_format_client(FormatClient::NoClient);
    let before = context.clone();
    let fields = "#{session_name}|#{client_name}|#{client_width}|#{@flavour}|#{status-position}";
    execute(
        &fixture,
        &mut context,
        &["set-option", "-gF", "@route-formatted", fields],
    )
    .unwrap();
    assert_attachment_restored(&context, &before);
    assert_eq!(
        engine_command(
            &fixture.shared,
            &["show-options", "-gqv", "@route-formatted"]
        )
        .output,
        "beta|alpha-high|100|B|bottom"
    );
    engine_command(
        &fixture.shared,
        &[
            "bind-key",
            "-T",
            "route-formatted",
            "x",
            "display-message",
            "value",
        ],
    );
    assert_eq!(
        execute(
            &fixture,
            &mut context,
            &["list-keys", "-T", "route-formatted", "-F", fields]
        )
        .unwrap()
        .output,
        "beta|alpha-high|100|B|bottom"
    );
    assert_attachment_restored(&context, &before);
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("beta-alpha-high.conf");
    fs::write(
        &source,
        "set-option -gF @route-source '#{session_name}|#{client_name}|#{current_file}'\n",
    )
    .unwrap();
    let source_format = directory
        .path()
        .join("#{session_name}-#{client_name}.conf")
        .display()
        .to_string();
    execute(
        &fixture,
        &mut context,
        &["source-file", "-F", "-t", "beta:0", &source_format],
    )
    .unwrap();
    assert_attachment_restored(&context, &before);
    assert_eq!(
        engine_command(&fixture.shared, &["show-options", "-gqv", "@route-source"]).output,
        format!("beta|alpha-high|{}", source.display())
    );
}

#[test]
fn default_key_listing_routes_aliases_and_keeps_hook_facts_and_attachment_fresh() {
    let fixture = fixture();
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.invoking)
        .origin
        .replace(fixture.targets[0].context.pane.unwrap());
    engine_command(
        &fixture.shared,
        &[
            "bind-key",
            "-T",
            "route-default",
            "-N",
            "listed-note",
            "x",
            "display-message",
            "listed-command",
        ],
    );
    engine_command(
        &fixture.shared,
        &[
            "set-option",
            "-s",
            "command-alias[92]",
            "defaultkeys=list-keys",
        ],
    );
    for (hook, option) in [
        ("after-list-keys", "@listing-after"),
        ("command-error", "@listing-error"),
    ] {
        engine_command(
            &fixture.shared,
            &[
                "set-hook",
                "-g",
                hook,
                &format!(
                    "set-option -gF {option} '#{{hook}}|#{{session_name}}|#{{client_name}}|#{{client_width}}|#{{@flavour}}|#{{status-position}}'"
                ),
            ],
        );
    }
    let mut context = fixture.targets[2].context.clone();
    context.set_format_client(FormatClient::NoClient);
    let before = context.clone();
    for name in ["list-keys", "lsk", "defaultkeys"] {
        let listed = execute(&fixture, &mut context, &[name, "-T", "route-default"]).unwrap();
        assert!(listed.output.contains("route-default"), "{}", listed.output);
        assert!(
            listed.output.contains("listed-command"),
            "{}",
            listed.output
        );
        assert_attachment_restored(&context, &before);
        assert_eq!(
            engine_command(&fixture.shared, &["show-options", "-gqv", "@listing-after"])
                .output
                .trim(),
            "after-list-keys|beta|alpha-high|100|B|bottom"
        );
    }
    let notes = execute(
        &fixture,
        &mut context,
        &["lsk", "-N", "-P", "seen:", "-T", "route-default"],
    )
    .unwrap();
    assert_eq!(notes.output, "seen: x listed-note");
    assert_attachment_restored(&context, &before);
    let single = execute(
        &fixture,
        &mut context,
        &["defaultkeys", "-1", "-T", "route-default", "x"],
    )
    .unwrap();
    assert!(single.output.is_empty());
    assert!(
        fixture
            .shared
            .inner
            .lock()
            .message_log
            .iter()
            .any(|message| message.text.contains("route-default"))
    );
    assert_attachment_restored(&context, &before);
    fixture
        .shared
        .inner
        .lock()
        .client_entry(fixture.targets[0].client)
        .activity
        .replace(40);
    engine_command(
        &fixture.shared,
        &["set-option", "-t", "beta", "@flavour", "B2"],
    );
    let fields = "#{session_name}|#{client_name}|#{client_width}|#{@flavour}|#{status-position}";
    assert_eq!(
        execute(
            &fixture,
            &mut context,
            &["lsk", "-T", "route-default", "-F", fields]
        )
        .unwrap()
        .output,
        "beta|alpha-low|80|B2|bottom"
    );
    assert_attachment_restored(&context, &before);
    for args in [
        &["list-keys", "-Z"][..],
        &["lsk", "-F"][..],
        &["defaultkeys", "-T", "missing-route-table"][..],
    ] {
        assert!(execute(&fixture, &mut context, args).is_err());
        assert_attachment_restored(&context, &before);
        assert_eq!(
            engine_command(&fixture.shared, &["show-options", "-gqv", "@listing-error"])
                .output
                .trim(),
            "command-error|beta|alpha-low|80|B2|bottom"
        );
    }
    context.set_replay_client(Some(fixture.targets[2].client));
    let before_present = context.clone();
    execute(&fixture, &mut context, &["lsk", "-T", "route-default"]).unwrap();
    assert_attachment_restored(&context, &before_present);
    assert_eq!(
        engine_command(&fixture.shared, &["show-options", "-gqv", "@listing-after"])
            .output
            .trim(),
        "after-list-keys|beta|beta-viewer|120|B2|bottom"
    );
}
