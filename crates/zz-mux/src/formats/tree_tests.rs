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
