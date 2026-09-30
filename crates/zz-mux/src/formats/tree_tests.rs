use super::*;
use crate::format_universe_tests::{FORMATS, clients, fixtures, targets};

struct Hooks<'a>(&'a MuxEngine);

impl StatusHooks for Hooks<'_> {
    fn strftime(&mut self, literal: &str) -> String {
        literal.to_owned()
    }
    fn shell(&mut self, _command: &str, _tag: &FormatJobTag) -> String {
        String::new()
    }
    fn option_variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
        self.0.format_option_value(context, name)
    }
}

#[test]
fn engine_access_preserves_legacy_resolution_and_ends_at_detach() {
    let mut engine = MuxEngine::default();
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    for borrowed in [true, false] {
        with_borrowed_formats(borrowed, || {
            let mut context = engine.format_status_context(Some(session), Some(window), Some(pane));
            assert!(std::ptr::eq(context.engine().unwrap(), &raw const engine));
            assert_eq!(context.variable("session_name").as_deref(), Some("work"));
            assert_eq!(context.tree.is_some(), borrowed);
            assert_eq!(context.values.get().is_none(), borrowed);
            let child = context.borrowed_child(Some(&engine));
            assert!(std::ptr::eq(child.engine().unwrap(), &raw const engine));
            assert_eq!(child.tree.is_some(), borrowed);
            assert_eq!(child.variable("session_name").as_deref(), Some("work"));
            context.window_name = "owned-window".to_owned();
            assert!(context.tree.is_none());
            assert!(std::ptr::eq(context.engine().unwrap(), &raw const engine));
            assert_eq!(
                context.variable("window_name").as_deref(),
                Some("owned-window")
            );
            let detached = context.detach_with_templates(FormatNeeds::NONE, ["#{window_name}"]);
            assert!(detached.engine().is_none());
            assert_eq!(
                detached.variable("window_name").as_deref(),
                Some("owned-window")
            );
        });
    }
    assert!(
        StatusContext::from(StatusValues::default())
            .engine()
            .is_none()
    );
}

#[test]
fn borrowed_callbacks_match_w1_for_every_pinned_name() {
    for (fixture, engine) in fixtures() {
        for client in clients(&engine) {
            for (session, window, pane) in targets(&engine) {
                let borrowed = with_borrowed_formats(true, || {
                    engine.format_status_context_with_format_client(
                        session, window, pane, session, client,
                    )
                });
                let legacy = with_borrowed_formats(false, || {
                    engine.format_status_context_with_format_client(
                        session, window, pane, session, client,
                    )
                });
                for spec in &FORMAT_VARIABLES {
                    assert_eq!(
                        borrowed.variable(spec.name),
                        legacy.variable(spec.name),
                        "{fixture} {client:?} {session:?} {window:?} {pane:?} {}",
                        spec.name
                    );
                }
                assert!(borrowed.values.get().is_none());
            }
        }
    }
}

#[test]
fn selective_detach_captures_only_referenced_table_values() {
    let mut engine = MuxEngine::default();
    let (session, _, _) = engine.state.create_session("work").unwrap();
    let template = "#{session_name} #{pane_index}";
    let context = with_borrowed_formats(true, || {
        engine
            .format_status_context(Some(session), None, None)
            .detach_with_templates(FormatNeeds::NONE, [template])
    });
    assert_eq!(context.variable("session_name").as_deref(), Some("work"));
    assert_eq!(context.variable("pane_index").as_deref(), Some("0"));
    assert_eq!(context.variables.len(), 2);
    assert!(context.values.get().is_none());
    assert_cache_context_matches_w1(
        &engine,
        FormatContext {
            session: Some(session),
            active_session: Some(session),
            ..FormatContext::default()
        },
        template,
        &context,
    );
    assert_eq!(context.pane_index, 0);
    assert!(std::mem::size_of::<StatusContext>() < 256);
}

#[test]
fn default_status_templates_keep_detached_capture_selective() {
    let mut engine = MuxEngine::default();
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    for index in 1..20 {
        engine
            .state
            .create_window(
                session,
                Some(format!("window-{index}")),
                crate::PaneKind::Terminal,
            )
            .unwrap();
    }
    let formats = engine.status_formats_for_session(Some(session));
    let rows = engine.status_format_array_for_session(Some(session));
    let styles = engine.message_styles_for_session(Some(session));
    let mut templates = vec![
        formats.left,
        formats.right,
        formats.style,
        styles.0,
        styles.1,
        "#{socket_path}:#{session_path}:#{pane_current_path}".to_owned(),
        "#{theme}".to_owned(),
    ];
    templates.extend(rows.into_values());
    for half in ["light", "dark"] {
        for suffix in [
            "black",
            "white",
            "light-grey",
            "dark-grey",
            "green",
            "yellow",
            "red",
            "blue",
            "cyan",
            "magenta",
        ] {
            templates.push(format!("#{{E:{half}-theme-{suffix}}}"));
        }
    }
    let mut references = BTreeSet::new();
    for template in &templates {
        let raw = format_references(template);
        let cached = engine.cached_format_references(template);
        assert!(
            !raw.iter().any(|name| name == "*"),
            "raw {template}: {raw:?}"
        );
        assert!(!cached.contains("*"), "cached {template}: {cached:?}");
        references.extend(cached.iter().cloned());
    }
    let table_count = FORMAT_VARIABLES
        .iter()
        .filter(|spec| references.contains(spec.name))
        .count();
    assert!(table_count < FORMAT_VARIABLES.len() / 2);
    with_borrowed_formats(true, || {
        let templates = templates.iter().map(String::as_str).collect::<Vec<_>>();
        let context = engine
            .format_status_context(Some(session), Some(window), Some(pane))
            .detach_with_templates(engine.format_needs(templates.iter().copied()), templates);
        assert_eq!(context.variables.len(), table_count);
        assert!(context.values.get().is_none());
        let parts = &context.format_universe.parts;
        for items in parts
            .windows
            .lock()
            .values()
            .chain(parts.panes.lock().values())
            .flatten()
        {
            for item in items.iter() {
                assert_eq!(item.context.variables.len(), table_count);
                assert!(item.context.values.get().is_none());
            }
        }
    });
}

#[test]
fn detached_nested_loops_keep_whole_status_values_uninitialized() {
    let mut engine = MuxEngine::default();
    let (session, _, pane) = engine.state.create_session("alpha").unwrap();
    engine
        .state
        .split_pane(
            pane,
            zz_protocol::Axis::Horizontal,
            crate::PaneKind::Terminal,
        )
        .unwrap();
    engine
        .state
        .create_window(session, Some("logs".to_owned()), crate::PaneKind::Terminal)
        .unwrap();
    engine.state.create_session("beta").unwrap();
    let template = "#{S:#{session_name}[#{W:#{window_name}(#{P:#{pane_index};})}]}";
    let expected = with_borrowed_formats(false, || {
        compiled::with_enabled(false, || {
            let context = engine.format_status_context(Some(session), None, None);
            expand_status(template, &context, &mut Hooks(&engine))
        })
    });
    with_borrowed_formats(true, || {
        let context = engine
            .format_status_context(Some(session), None, None)
            .detach_with_templates(engine.format_needs([template]), [template]);
        let assert_lazy = || {
            assert!(context.values.get().is_none());
            let assert_items = |items: &LoopItems| {
                assert!(!items.is_empty());
                for item in items.iter() {
                    assert!(item.context.values.get().is_none());
                    assert!(item.context.borrowed_child(None).values.get().is_none());
                }
            };
            let parts = &context.format_universe.parts;
            assert_items(parts.sessions.get().unwrap());
            for items in parts.windows.lock().values().flatten() {
                assert_items(items);
            }
            for items in parts.panes.lock().values().flatten() {
                assert_items(items);
            }
        };
        assert_lazy();
        for compiled in [false, true] {
            assert_eq!(
                compiled::with_enabled(compiled, || {
                    expand_status(template, &context, &mut Hooks(&engine))
                }),
                expected
            );
            assert_lazy();
        }
        with_borrowed_formats(false, || {
            assert!(context.borrowed_child(None).values.get().is_some());
        });
    });
}

#[test]
fn selective_detach_keeps_table_values_introduced_by_shell_output() {
    struct OutputHooks<'a> {
        context: &'a StatusContext<'static>,
        engine: &'a MuxEngine,
    }

    impl StatusHooks for OutputHooks<'_> {
        fn strftime(&mut self, literal: &str) -> String {
            literal.to_owned()
        }

        fn shell(&mut self, _command: &str, _tag: &FormatJobTag) -> String {
            expand_format_values("#{pane_title}", self.context, &mut Hooks(self.engine))
        }

        fn option_variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
            self.engine.format_option_value(context, name)
        }
    }

    let mut engine = MuxEngine::default();
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    engine
        .state
        .update_pane_title(pane, "captured-title")
        .unwrap();
    engine
        .execute(
            &mut crate::ExecutionContext::default(),
            &zz_protocol::CommandInvocation::new(
                "set-option",
                vec!["-g", "status-left", "#(later-output)"],
            ),
        )
        .unwrap();
    for (template, expected) in [
        ("#(later-output)", "captured-title"),
        ("#{E:status-left}", "captured-title"),
        ("#[fg=#(later-output)]", "#[fg=captured-title]"),
    ] {
        engine
            .state
            .update_pane_title(pane, "captured-title")
            .unwrap();
        let needs = engine.format_needs([template]);
        assert_eq!(needs, FormatNeeds::NONE);
        let context = engine
            .format_status_context(Some(session), Some(window), Some(pane))
            .detach_with_templates(needs, [template]);
        engine
            .state
            .update_pane_title(pane, "updated-title")
            .unwrap();
        assert_eq!(
            context.variable("pane_title").as_deref(),
            Some("captured-title")
        );
        let mut hooks = OutputHooks {
            context: &context,
            engine: &engine,
        };
        assert_eq!(expand_status(template, &context, &mut hooks), expected);
    }
}

#[test]
fn selective_detached_loops_and_indirection_match_w1() {
    for (fixture, engine) in fixtures() {
        for client in clients(&engine) {
            for (session, window, pane) in targets(&engine) {
                for format in FORMATS {
                    let legacy = with_borrowed_formats(false, || {
                        compiled::with_enabled(false, || {
                            let context = engine.format_status_context_with_format_client(
                                session, window, pane, session, client,
                            );
                            expand_status(format, &context, &mut Hooks(&engine))
                        })
                    });
                    for borrowed in [false, true] {
                        let detached = with_borrowed_formats(borrowed, || {
                            engine
                                .format_status_context_with_format_client(
                                    session, window, pane, session, client,
                                )
                                .detach_with_templates(engine.format_needs([*format]), [*format])
                        });
                        let actual = compiled::with_enabled(true, || {
                            expand_status(format, &detached, &mut Hooks(&engine))
                        });
                        assert_eq!(
                            actual, legacy,
                            "{fixture} {client:?} {session:?} {window:?} {pane:?} borrowed={borrowed}: {format}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn indirect_references_follow_option_generations_and_scopes() {
    let mut engine = MuxEngine::default();
    let (session, _, pane) = engine.state.create_session("work").unwrap();
    engine
        .state
        .update_pane_title(pane, "captured-title")
        .unwrap();
    let mut context = crate::ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &zz_protocol::CommandInvocation::new(
                "set-option",
                vec!["-g", "status-left", "#{session_name}"],
            ),
        )
        .unwrap();
    let first = engine.cached_format_references("#{E:status-left}");
    assert!(first.contains("session_name"));
    assert!(!first.contains("*"));
    assert_eq!(
        Arc::ptr_eq(&first, &engine.cached_format_references("#{E:status-left}")),
        format_cache_knob()
    );
    engine
        .execute(
            &mut context,
            &zz_protocol::CommandInvocation::new(
                "set-option",
                vec!["-g", "status-left", "#{pane_title}"],
            ),
        )
        .unwrap();
    let second = engine.cached_format_references("#{E:status-left}");
    assert!(second.contains("pane_title"));
    assert!(!second.contains("session_name"));
    let detached = with_borrowed_formats(true, || {
        engine
            .format_status_context(Some(session), None, None)
            .detach_with_templates(
                engine.format_needs(["#{E:status-left}"]),
                ["#{E:status-left}"],
            )
    });
    assert!(detached.variables.contains_key("pane_title"));
    assert!(!detached.variables.contains_key("session_name"));
    assert_eq!(
        expand_status("#{E:status-left}", &detached, &mut Hooks(&engine)),
        "captured-title"
    );
    assert_cache_context_matches_w1(
        &engine,
        FormatContext {
            session: Some(session),
            active_session: Some(session),
            ..FormatContext::default()
        },
        "#{E:status-left}",
        &detached,
    );
}

fn cache_context(
    engine: &MuxEngine,
    target: FormatContext,
    template: &str,
) -> StatusContext<'static> {
    with_borrowed_formats(true, || {
        engine
            .format_status_context_with_format_client(
                target.session,
                target.window,
                target.pane,
                target.active_session,
                target.format_client,
            )
            .detach_with_templates(engine.format_needs([template]), [template])
    })
}

fn assert_cache_context_matches_w1(
    engine: &MuxEngine,
    target: FormatContext,
    template: &str,
    captured: &StatusContext<'static>,
) {
    let expected = with_borrowed_formats(false, || {
        let legacy = engine
            .format_status_context_with_format_client(
                target.session,
                target.window,
                target.pane,
                target.active_session,
                target.format_client,
            )
            .detach(engine.format_needs([template]));
        expand_status(template, &legacy, &mut Hooks(engine))
    });
    assert_eq!(
        expand_status(template, captured, &mut Hooks(engine)),
        expected
    );
    assert!(captured.values.get().is_none());
}

#[test]
fn early_capture_lookup_preserves_exact_inputs_and_resolved_targets() {
    let mut engine = MuxEngine::default();
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    let (linked, _, _) = engine.state.create_session("linked").unwrap();
    engine
        .state
        .session_mut(linked)
        .unwrap()
        .windows
        .push(window);
    let client = FormatClient::Attached(linked);
    let template = "#{config_files}:#{window_active}:#{session_name}:#{W:#{window_name}}";
    let references = engine.cached_format_references_for_templates([template]);
    let needs = engine.format_needs([template]);
    let overrides = [("config_files", "first.conf"), ("window_active", "1")];
    with_borrowed_formats(true, || {
        let mut live = engine.format_status_context_with_format_client(
            Some(linked),
            Some(window),
            Some(pane),
            Some(linked),
            client,
        );
        for (name, value) in overrides {
            live.set_format_value(name, value);
        }
        let captured = live.detach_with_references(needs, &references);
        for target in [
            (Some(session), Some(window), Some(pane)),
            (Some(linked), Some(window), None),
            (Some(SessionId(u64::MAX)), None, Some(pane)),
        ] {
            let hit = engine.cached_detached_format_context(
                target,
                client,
                needs,
                &references,
                overrides,
            );
            assert_eq!(hit.is_some(), format_cache_knob());
            if let Some(hit) = hit {
                assert!(captured.same_detached(&hit));
                assert!(Arc::ptr_eq(&captured.variables, &hit.variables));
                assert!(hit.values.get().is_none());
                assert_eq!(
                    expand_status(template, &hit, &mut Hooks(&engine)),
                    expand_status(template, &captured, &mut Hooks(&engine))
                );
            }
        }
        let target = (Some(session), Some(window), Some(pane));
        for changed in [
            vec![("config_files", "second.conf"), ("window_active", "1")],
            vec![("config_files", "first.conf")],
            vec![
                ("config_files", "first.conf"),
                ("extra", "value"),
                ("window_active", "1"),
            ],
            vec![("window_active", "1"), ("config_files", "first.conf")],
        ] {
            assert!(
                engine
                    .cached_detached_format_context(target, client, needs, &references, changed)
                    .is_none()
            );
        }
        for (changed_target, changed_client, changed_needs, changed_references) in [
            ((None, None, None), client, needs, references.as_ref()),
            (target, FormatClient::Unattached, needs, references.as_ref()),
            (target, client, FormatNeeds::NONE, references.as_ref()),
            (target, client, needs, &BTreeSet::new()),
        ] {
            assert!(
                engine
                    .cached_detached_format_context(
                        changed_target,
                        changed_client,
                        changed_needs,
                        changed_references,
                        overrides,
                    )
                    .is_none()
            );
        }
        with_borrowed_formats(false, || {
            assert!(
                engine
                    .cached_detached_format_context(target, client, needs, &references, overrides)
                    .is_none()
            );
        });
    });
}

#[test]
fn early_capture_lookup_rejects_changed_compact_ids_and_source_revisions() {
    let mut engine = MuxEngine::default();
    engine.set_format_now(10);
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    let target = (Some(session), Some(window), Some(pane));
    let references = engine.cached_format_references("#{session_name}");
    with_borrowed_formats(true, || {
        let mut live = engine.format_status_context(target.0, target.1, target.2);
        live.session_id = "$00".to_owned();
        let _ = live.detach_with_references(FormatNeeds::NONE, &references);
        assert!(
            engine
                .cached_detached_format_context(
                    target,
                    FormatClient::NoClient,
                    FormatNeeds::NONE,
                    &references,
                    [],
                )
                .is_none()
        );
    });
    let mut previous = engine.format_cache_revision();
    for change in 0..4 {
        let _ = cache_context(
            &engine,
            FormatContext {
                session: Some(session),
                window: Some(window),
                pane: Some(pane),
                ..FormatContext::default()
            },
            "#{session_name}",
        );
        match change {
            0 => {
                engine
                    .state
                    .create_window(session, Some("fresh".to_owned()), crate::PaneKind::Terminal)
                    .unwrap();
            }
            1 => {
                engine
                    .execute(
                        &mut crate::ExecutionContext::default(),
                        &zz_protocol::CommandInvocation::new(
                            "set-option",
                            ["-g", "default-terminal", "fresh-terminal"],
                        ),
                    )
                    .unwrap();
            }
            2 => engine.set_format_server_identity(21, "22", "fresh-user"),
            3 => engine.set_format_now(20),
            _ => unreachable!(),
        }
        let next = engine.format_cache_revision();
        assert_eq!(next.is_some(), format_cache_knob());
        if let (Some(previous), Some(next)) = (previous, next) {
            assert_ne!(previous, next);
            match change {
                0 => assert_ne!(previous.0, next.0),
                1 => assert_ne!(previous.1, next.1),
                2 => assert_ne!(previous.2, next.2),
                3 => assert_ne!(previous.3, next.3),
                _ => unreachable!(),
            }
        }
        with_borrowed_formats(true, || {
            assert_eq!(
                engine
                    .cached_detached_format_context(
                        target,
                        FormatClient::NoClient,
                        FormatNeeds::NONE,
                        &references,
                        [],
                    )
                    .is_some(),
                change == 3 && format_cache_knob()
            );
        });
        previous = next;
    }
}

#[test]
fn detached_capture_cache_reuses_selected_maps_and_preserves_exact_inputs() {
    let mut engine = MuxEngine::default();
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    let target = FormatContext {
        session: Some(session),
        window: Some(window),
        pane: Some(pane),
        active_session: Some(session),
        format_client: FormatClient::Attached(session),
        format_type: FormatType::Pane,
    };
    let template = "#{S:#{session_name}[#{W:#{window_name}(#{P:#{pane_id};})}]}";
    let first = cache_context(&engine, target, template);
    let second = cache_context(&engine, target, template);
    assert_eq!(
        Arc::ptr_eq(&first.variables, &second.variables),
        format_cache_knob()
    );
    assert_eq!(
        Arc::ptr_eq(&first.format_universe.parts, &second.format_universe.parts),
        format_cache_knob()
    );
    assert!(first.same_detached(&second));
    assert_cache_context_matches_w1(&engine, target, template, &second);
    let borrowed = with_borrowed_formats(true, || {
        engine.format_status_context(Some(session), Some(window), Some(pane))
    });
    assert!(!borrowed.same_detached(&first));
    assert!(borrowed.values.get().is_none());
    let capture_override = |value: &str| {
        with_borrowed_formats(true, || {
            let mut context = engine.format_status_context(Some(session), Some(window), Some(pane));
            context.set_format_value("config_files", value);
            context.detach_with_templates(FormatNeeds::NONE, ["#{config_files}"])
        })
    };
    let override_first = capture_override("first.conf");
    let override_same = capture_override("first.conf");
    let override_second = capture_override("second.conf");
    assert!(override_first.same_detached(&override_same));
    assert!(!override_first.same_detached(&override_second));
    assert_eq!(
        override_second.variable("config_files").as_deref(),
        Some("second.conf")
    );
    let mut changed = override_same.clone();
    changed.set_format_value("config_files", "changed.conf");
    assert_eq!(
        override_same.variable("config_files").as_deref(),
        Some("first.conf")
    );
    assert!(!changed.same_detached(&override_same));
    let other_client = cache_context(
        &engine,
        FormatContext {
            format_client: FormatClient::Unattached,
            ..target
        },
        template,
    );
    assert!(!first.same_detached(&other_client));
    let legacy = with_borrowed_formats(false, || {
        engine
            .format_status_context(Some(session), Some(window), Some(pane))
            .detach_with_templates(engine.format_needs([template]), [template])
    });
    assert!(legacy.values.get().is_some());
    assert!(!Arc::ptr_eq(&legacy.variables, &first.variables));
}

#[test]
fn detached_capture_cache_invalidates_runtime_environment_identity_and_clocks() {
    let mut engine = MuxEngine::default();
    engine.set_format_server_context("host-one", "one", "/tmp/one", 10);
    engine.set_format_server_identity(11, "12", "first-user");
    engine.seed_global_environment([("CACHE_ENV", "first-env")]);
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    engine.set_pane_runtime_facts(
        pane,
        crate::PaneRuntimeFacts {
            current_path: "/first".to_owned(),
            ..crate::PaneRuntimeFacts::default()
        },
    );
    engine
        .set_pane_start_command(pane, vec!["first-command".to_owned()])
        .unwrap();
    let target = FormatContext {
        session: Some(session),
        window: Some(window),
        pane: Some(pane),
        active_session: Some(session),
        format_client: FormatClient::Attached(session),
        format_type: FormatType::Pane,
    };
    let template = "#{P:#{pane_id}|#{pane_current_path}|#{pane_start_command}|#{pid}|#{uid}|#{user}|#{host}|#{socket_path}|#{CACHE_ENV}|#{t/r:session_created}|#{session_activity}|#{window_activity};}";
    let mut previous = cache_context(&engine, target, template);
    let state_generation = engine.state.generation();
    engine.set_pane_runtime_facts(
        pane,
        crate::PaneRuntimeFacts {
            current_path: "/second".to_owned(),
            ..crate::PaneRuntimeFacts::default()
        },
    );
    assert_eq!(engine.state.generation(), state_generation);
    let next = cache_context(&engine, target, template);
    assert!(!previous.same_detached(&next));
    assert_cache_context_matches_w1(&engine, target, template, &next);
    previous = next;
    engine.set_format_server_identity(21, "22", "second-user");
    let next = cache_context(&engine, target, template);
    assert!(!previous.same_detached(&next));
    assert_cache_context_matches_w1(&engine, target, template, &next);
    previous = next;
    engine.set_format_server_context("host-two", "two", "/tmp/two", 10);
    let next = cache_context(&engine, target, template);
    assert!(!previous.same_detached(&next));
    assert_cache_context_matches_w1(&engine, target, template, &next);
    previous = next;
    engine.set_config_environment("CACHE_ENV".to_owned(), "second-env".to_owned(), false);
    let next = cache_context(&engine, target, template);
    assert!(!previous.same_detached(&next));
    assert_cache_context_matches_w1(&engine, target, template, &next);
    previous = next;
    let mut execution = crate::ExecutionContext::default();
    execution.session = Some(session);
    execution.window = Some(window);
    execution.pane = Some(pane);
    engine
        .execute(
            &mut execution,
            &zz_protocol::CommandInvocation::new("set-environment", ["CACHE_ENV", "session-env"]),
        )
        .unwrap();
    let next = cache_context(&engine, target, template);
    assert!(!previous.same_detached(&next));
    assert_cache_context_matches_w1(&engine, target, template, &next);
    previous = next;
    engine
        .execute(
            &mut execution,
            &zz_protocol::CommandInvocation::new("respawn-pane", ["-k", "second-command"]),
        )
        .unwrap();
    let next = cache_context(&engine, target, template);
    assert!(!previous.same_detached(&next));
    assert_cache_context_matches_w1(&engine, target, template, &next);
    previous = next;
    engine.mark_session_active_at(session, 11);
    let next = cache_context(&engine, target, template);
    assert!(!previous.same_detached(&next));
    assert_cache_context_matches_w1(&engine, target, template, &next);
    previous = next;
    engine.touch_window_activity_for_pane(pane);
    let next = cache_context(&engine, target, template);
    assert!(!previous.same_detached(&next));
    assert_cache_context_matches_w1(&engine, target, template, &next);
    previous = next;
    engine.set_format_now(20);
    let next = cache_context(&engine, target, template);
    assert_eq!(previous.format_now, Some(10));
    assert_eq!(next.format_now, Some(20));
    assert!(!previous.same_detached(&next));
    assert_eq!(
        Arc::ptr_eq(&previous.variables, &next.variables),
        format_cache_knob()
    );
    assert_eq!(
        Arc::ptr_eq(&previous.format_universe.parts, &next.format_universe.parts),
        format_cache_knob()
    );
    assert_cache_context_matches_w1(&engine, target, template, &next);
}

#[test]
fn detached_clock_overlay_reuses_capture_and_updates_every_nested_hook_and_modifier() {
    struct ClockHooks(Vec<Option<i64>>);

    impl StatusHooks for ClockHooks {
        fn strftime(&mut self, literal: &str) -> String {
            literal.to_owned()
        }

        fn shell(&mut self, _: &str, _: &FormatJobTag) -> String {
            String::new()
        }

        fn variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
            (name == "clock_probe").then(|| {
                self.0.push(context.format_now);
                context.format_now.unwrap().to_string()
            })
        }
    }

    for borrowed in [false, true] {
        let mut engine = MuxEngine::default();
        engine.set_format_now(10);
        let (session, window, pane) = engine.state.create_session("work").unwrap();
        engine.state.session_mut(session).unwrap().created = Some(10);
        let target = (Some(session), Some(window), Some(pane));
        let template = "#{clock_probe}/#{t/r:session_created}[#{S:#{clock_probe}/#{t/r:session_created}[#{W:#{clock_probe}/#{t/r:session_created}[#{P:#{clock_probe}/#{t/r:session_created}}]}]}]";
        let references = engine.cached_format_references_for_templates([template]);
        let needs = engine.format_needs([template]);
        let capture = |engine: &MuxEngine| {
            with_borrowed_formats(borrowed, || {
                engine
                    .format_status_context(target.0, target.1, target.2)
                    .detach_with_references(needs, &references)
            })
        };
        let first = capture(&engine);
        engine.set_format_now(20);
        let second = capture(&engine);
        let materialized = StatusContext::from(StatusValues {
            format_now: Some(10),
            ..StatusValues::default()
        });
        let overlaid =
            materialized.with_capture_clock(FormatCaptureRevision::new(&engine), Some(20));
        assert_eq!(overlaid.format_now, Some(20));
        assert_eq!(overlaid.values.get().unwrap().format_now, Some(20));
        assert_eq!(materialized.values.get().unwrap().format_now, Some(10));
        let shared = borrowed && format_cache_knob();
        assert_eq!(Arc::ptr_eq(&first.variables, &second.variables), shared);
        assert_eq!(
            Arc::ptr_eq(&first.format_universe.parts, &second.format_universe.parts),
            shared
        );
        assert_eq!(first.format_now, Some(10));
        assert_eq!(second.format_now, Some(20));
        assert!(!first.same_detached(&second));
        assert_eq!(
            second.capture_revision.as_deref().map(|value| value.now),
            borrowed.then_some(20)
        );
        let early = with_borrowed_formats(borrowed, || {
            engine.cached_detached_format_context(
                target,
                FormatClient::NoClient,
                needs,
                &references,
                [],
            )
        });
        assert_eq!(early.is_some(), shared);
        if let Some(early) = &early {
            assert!(second.same_detached(early));
            assert!(Arc::ptr_eq(&first.variables, &early.variables));
            assert!(Arc::ptr_eq(
                &first.format_universe.parts,
                &early.format_universe.parts
            ));
        }
        for compiled in [false, true] {
            with_borrowed_formats(borrowed, || {
                compiled::with_enabled(compiled, || {
                    for (context, now, expected) in [
                        (&first, 10, "10/0s[10/0s[10/0s[10/0s]]]"),
                        (&second, 20, "20/10s[20/10s[20/10s[20/10s]]]"),
                    ] {
                        let mut hooks = ClockHooks(Vec::new());
                        assert_eq!(expand_status(template, context, &mut hooks), expected);
                        assert_eq!(hooks.0, vec![Some(now); 4]);
                    }
                });
            });
        }
    }
}

#[test]
fn capture_and_reference_union_caches_are_bounded_and_follow_rollback() {
    let mut engine = MuxEngine::default();
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    let sources = ["#{pane_title}", "#{S:#{session_name}}"];
    let first = engine.cached_format_references_for_templates(sources);
    let same = engine.cached_format_references_for_templates(sources);
    assert_eq!(Arc::ptr_eq(&first, &same), format_cache_knob());
    assert_eq!(
        first.as_ref(),
        &BTreeSet::from(["pane_title".to_owned(), "session_name".to_owned()])
    );
    let changed = engine.cached_format_references_for_templates(["#{pane_id}"]);
    assert!(!Arc::ptr_eq(&first, &changed));
    assert_eq!(changed.as_ref(), &BTreeSet::from(["pane_id".to_owned()]));
    let oversized = "x".repeat(FORMAT_CAPTURE_CACHE_BYTES);
    let _ = engine.cached_format_references_for_templates([oversized.as_str()]);
    if format_cache_knob() {
        assert!(engine.format_reference_union_cache.lock().is_none());
    }
    engine.state.update_pane_title(pane, oversized).unwrap();
    let target = FormatContext {
        session: Some(session),
        window: Some(window),
        pane: Some(pane),
        ..FormatContext::default()
    };
    let template = "#{P:#{pane_title}}";
    let first = cache_context(&engine, target, template);
    let same = cache_context(&engine, target, template);
    assert!(!Arc::ptr_eq(&first.variables, &same.variables));
    assert!(engine.format_context_cache.lock().is_none());
    assert!(first.values.get().is_none());
    assert!(same.values.get().is_none());
}

#[test]
fn detached_source_revisions_preserve_environment_and_legacy_equality_boundaries() {
    let mut engine = MuxEngine::default();
    let (session, window, pane) = engine.state.create_session("work").unwrap();
    let target = FormatContext {
        session: Some(session),
        window: Some(window),
        pane: Some(pane),
        ..FormatContext::default()
    };
    let first = cache_context(&engine, target, "#{pane_title}");
    assert!(!first.has_captured_environment());
    engine.set_config_environment("prompt_flags".to_owned(), "fresh".to_owned(), false);
    let second = cache_context(&engine, target, "#{pane_title}");
    assert_eq!(first.variables, second.variables);
    assert!(
        first
            .format_universe
            .parts
            .same_owned(&second.format_universe.parts)
    );
    assert!(!first.same_detached(&second));
    let captured = cache_context(&engine, target, "#{prompt_flags}");
    assert!(captured.has_captured_environment());
    assert_eq!(
        expand_status("#{prompt_flags}", &captured, &mut Hooks(&engine)),
        "fresh"
    );
    assert!(!format_variable_is_known("prompt_flags"));
    assert!(format_variable_is_known("pane_title"));
    assert!(format_variable_is_captured("pane_title"));
    assert!(format_variable_is_known("cursor_x"));
    assert!(!format_variable_is_captured("cursor_x"));
    assert!(format_variable_is_known("window_bigger"));
    assert!(!format_variable_is_captured("window_bigger"));
    let legacy = StatusContext::from(StatusValues::default());
    let mut changed = legacy.clone();
    assert!(legacy.same_detached(&changed));
    assert!(!legacy.has_captured_environment());
    changed.host = "different-host".to_owned();
    assert!(!legacy.same_detached(&changed));
    assert!(captured.values.get().is_none());
    assert!(first.values.get().is_none());
    assert!(second.values.get().is_none());
}
