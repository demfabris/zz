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
    let context = engine
        .format_status_context(Some(session), None, None)
        .detach_with_templates(FormatNeeds::NONE, ["#{session_name} #{pane_index}"]);
    assert_eq!(context.variable("session_name").as_deref(), Some("work"));
    assert_eq!(context.variable("pane_index").as_deref(), Some("0"));
    assert_eq!(context.variables.len(), 2);
    assert!(context.values.get().is_none());
    assert_eq!(context.pane_index, 0);
    assert!(std::mem::size_of::<StatusContext>() < 256);
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
    let (session, _, _) = engine.state.create_session("work").unwrap();
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
    let detached = engine
        .format_status_context(Some(session), None, None)
        .detach_with_templates(
            engine.format_needs(["#{E:status-left}"]),
            ["#{E:status-left}"],
        );
    assert!(detached.variables.contains_key("pane_title"));
    assert!(!detached.variables.contains_key("session_name"));
}
