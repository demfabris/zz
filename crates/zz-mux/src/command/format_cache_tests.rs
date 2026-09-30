use super::*;

fn set(engine: &mut MuxEngine, context: &mut ExecutionContext, args: &[&str]) {
    engine
        .execute(
            context,
            &CommandInvocation::new("set-option", args.iter().copied()),
        )
        .expect("set option");
}

#[test]
fn format_cache_identity_distinguishes_equal_revisions_and_survives_updates() {
    let make_engine = |value| {
        let mut engine = MuxEngine::default();
        engine.state.create_session("same").unwrap();
        set(
            &mut engine,
            &mut ExecutionContext::default(),
            &["-g", "status-left", value],
        );
        engine.set_format_now(1_700_000_000);
        engine
    };
    let first = make_engine("old");
    let first_identity = first.format_cache_identity();
    let mut replacement = make_engine("new");
    assert_eq!(
        first.format_cache_revision(),
        replacement.format_cache_revision()
    );
    assert!(first.format_cache_identity_matches(&first_identity));
    assert!(!replacement.format_cache_identity_matches(&first_identity));
    drop(first);
    assert!(first_identity.upgrade().is_none());
    assert!(!replacement.format_cache_identity_matches(&first_identity));
    let replacement_identity = replacement.format_cache_identity();
    let _ = replacement.cached_format_option_snapshot();
    replacement.set_format_now(1_700_000_001);
    set(
        &mut replacement,
        &mut ExecutionContext::default(),
        &["-g", "status-left", "changed"],
    );
    replacement.state.create_session("next").unwrap();
    assert!(replacement.format_cache_identity_matches(&replacement_identity));
    assert!(!replacement.format_cache_identity_matches(&first_identity));
}

#[test]
fn key_table_getter_matches_scoped_inheritance_unset_and_empty_defaults() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "keys"]),
        )
        .unwrap();
    let session = context.session.unwrap();
    let compare = |engine: &MuxEngine, expected: &str| {
        assert_eq!(engine.key_table_for_session(session), expected);
        for target in [session, SessionId(u64::MAX)] {
            let table = engine.session_knobs(target).key_table;
            assert_eq!(
                engine.key_table_for_session(target),
                if table.is_empty() { "root" } else { &table }
            );
        }
    };
    compare(&engine, "root");
    for (args, expected) in [
        (vec!["-g", "key-table", "global"], "global"),
        (vec!["key-table", "local"], "local"),
        (vec!["-g", "key-table", "next-global"], "local"),
        (vec!["-u", "key-table"], "next-global"),
        (vec!["key-table", ""], "root"),
        (vec!["-u", "key-table"], "next-global"),
        (vec!["-gu", "key-table"], "root"),
        (vec!["-g", "key-table", ""], "root"),
    ] {
        set(&mut engine, &mut context, &args);
        compare(&engine, expected);
    }
}

#[test]
fn status_rows_getter_matches_scoped_inheritance_unset_and_missing_sessions() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "work"]),
        )
        .unwrap();
    let session = context.session.unwrap();
    let compare = |engine: &MuxEngine, expected| {
        assert_eq!(engine.status_rows_for_session(Some(session)), expected);
        for target in [None, Some(session), Some(SessionId(u64::MAX))] {
            assert_eq!(
                engine.status_rows_for_session(target),
                engine.status_formats_for_session(target).rows()
            );
        }
    };
    compare(&engine, 1);
    for (args, expected) in [
        (vec!["-g", "status", "off"], 0),
        (vec!["-g", "status", "3"], 3),
        (vec!["status", "2"], 2),
        (vec!["-g", "status", "off"], 2),
        (vec!["status", "off"], 0),
        (vec!["status", "on"], 1),
        (vec!["-u", "status"], 0),
        (vec!["-g", "status", "4"], 4),
        (vec!["-gu", "status"], 1),
        (vec!["status"], 0),
        (vec!["status"], 1),
    ] {
        set(&mut engine, &mut context, &args);
        compare(&engine, expected);
    }
}

#[test]
fn status_option_snapshot_reuses_unchanged_options_and_refreshes_scopes() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "cache"]),
        )
        .unwrap();
    let first = engine.cached_format_option_snapshot();
    assert_eq!(
        Arc::ptr_eq(&first, &engine.cached_format_option_snapshot()),
        crate::format_cache_knob()
    );
    set(&mut engine, &mut context, &["status-left", "changed"]);
    let second = engine.cached_format_option_snapshot();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(
        second.lookup(&context.session.unwrap().to_string(), "", "", "status-left"),
        Some("changed".to_owned())
    );
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("split-window", ["-d"]),
        )
        .unwrap();
    let third = engine.cached_format_option_snapshot();
    assert!(!Arc::ptr_eq(&second, &third));
    assert_eq!(third.panes.len(), 2);
}

#[test]
fn format_needs_cache_follows_user_option_indirection() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    set(&mut engine, &mut context, &["-g", "@indirect", "plain"]);
    let templates = ["#{E:@indirect}"];
    let first = engine.cached_format_needs(templates);
    assert_eq!(first, engine.format_needs(templates));
    set(
        &mut engine,
        &mut context,
        &["-g", "@indirect", "#{S:#{session_name}}"],
    );
    let second = engine.cached_format_needs(templates);
    assert_eq!(second, engine.format_needs(templates));
    assert!(second.contains(crate::FormatNeeds::SESSIONS));
    set(&mut engine, &mut context, &["-gu", "@indirect"]);
    assert_eq!(
        engine.cached_format_needs(templates),
        engine.format_needs(templates)
    );
}

#[test]
fn format_needs_cache_follows_status_array_append_and_unset() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    set(
        &mut engine,
        &mut context,
        &["-g", "status-format[0]", "plain"],
    );
    let before = engine.format_options_generation();
    let snapshot = engine.cached_format_option_snapshot();
    set(
        &mut engine,
        &mut context,
        &["-ga", "status-format[0]", "#{W:#{window_name}}"],
    );
    assert_ne!(before, engine.format_options_generation());
    assert!(!Arc::ptr_eq(
        &snapshot,
        &engine.cached_format_option_snapshot()
    ));
    assert_eq!(
        engine.cached_format_needs(["#{E:status-format[0]}"]),
        engine.format_needs(["#{E:status-format[0]}"])
    );
    set(&mut engine, &mut context, &["-gu", "status-format[0]"]);
    assert_eq!(
        engine.cached_format_needs(["#{E:status-format[0]}"]),
        engine.format_needs(["#{E:status-format[0]}"])
    );
}

fn user_hook_mutation_refreshes_format_caches(args: &[&str]) {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "cache"]),
        )
        .unwrap();
    let pane = context.pane.unwrap();
    engine.state.update_pane_title(pane, "cache-title").unwrap();
    set(&mut engine, &mut context, &["-g", "@indirect", "plain"]);
    let templates = ["#{E:@indirect}"];
    let snapshot = engine.cached_format_option_snapshot();
    let before = engine.format_options_generation();
    assert!(
        !engine
            .cached_format_needs(templates)
            .contains(crate::FormatNeeds::SESSIONS)
    );
    let first = crate::with_borrowed_formats(true, || {
        engine
            .format_status_context(context.session, context.window, context.pane)
            .detach_with_templates(engine.cached_format_needs(templates), templates)
    });
    assert_eq!(first.variable("pane_title").as_deref(), Some(""));
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("set-hook", args.iter().copied()),
        )
        .unwrap();
    assert_ne!(before, engine.format_options_generation());
    let refreshed = engine.cached_format_option_snapshot();
    assert!(!Arc::ptr_eq(&snapshot, &refreshed));
    assert_eq!(*refreshed, engine.format_option_snapshot());
    let needs = engine.cached_format_needs(templates);
    assert_eq!(needs, engine.format_needs(templates));
    assert!(needs.contains(crate::FormatNeeds::SESSIONS));
    let detached = crate::with_borrowed_formats(true, || {
        engine
            .format_status_context(context.session, context.window, context.pane)
            .detach_with_templates(needs, templates)
    });
    assert_eq!(
        detached.variable("pane_title").as_deref(),
        Some("cache-title")
    );
}

#[test]
fn user_hook_mutation_invalidates_option_needs_and_reference_caches() {
    user_hook_mutation_refreshes_format_caches(&[
        "-g",
        "@indirect",
        "#{S:#{session_name}}#{pane_title}",
    ]);
}

#[test]
fn monitor_hook_mutation_invalidates_option_needs_and_reference_caches() {
    user_hook_mutation_refreshes_format_caches(&[
        "-g",
        "-B",
        "@indirect:%*:#{pane_title}",
        "#{S:#{session_name}}#{pane_title}",
    ]);
}

#[test]
fn default_setters_invalidate_existing_option_snapshots() {
    let mut engine = MuxEngine::default();
    for (option, value) in [
        ("mode-keys", "vi"),
        ("status-keys", "vi"),
        ("default-shell", "/bin/bash"),
        ("editor", "nvim"),
    ] {
        let snapshot = engine.cached_format_option_snapshot();
        let before = engine.format_options_generation();
        match option {
            "mode-keys" => engine.set_default_mode_keys(value).unwrap(),
            "status-keys" => engine.set_default_status_keys(value).unwrap(),
            "default-shell" => engine.initialize_default_shell(value),
            "editor" => engine.initialize_default_editor(value),
            _ => unreachable!(),
        }
        assert_ne!(before, engine.format_options_generation(), "{option}");
        let refreshed = engine.cached_format_option_snapshot();
        assert!(!Arc::ptr_eq(&snapshot, &refreshed), "{option}");
        assert_eq!(*refreshed, engine.format_option_snapshot(), "{option}");
        assert_eq!(
            engine.global_tmux_option_value(option),
            Some(value.to_owned()),
            "{option}"
        );
    }
}

struct KeyListingHooks<'a> {
    calls: usize,
    command: Option<&'a str>,
}

impl StatusHooks for KeyListingHooks<'_> {
    fn stable_option_lookups(&self) -> bool {
        true
    }

    fn strftime(&mut self, _: &str) -> String {
        String::new()
    }

    fn shell(&mut self, _: &str, _: &FormatJobTag) -> String {
        String::new()
    }

    fn option_variable(&mut self, name: &str, _: &StatusContext) -> Option<String> {
        self.calls += 1;
        (name == "key_command")
            .then_some(self.command)
            .flatten()
            .map(str::to_owned)
    }
}

fn key_listing_engine() -> MuxEngine {
    let mut engine = MuxEngine {
        keys: KeyTables::empty(),
        ..MuxEngine::default()
    };
    for (table, key, repeat, note) in [
        ("x", "a", false, Some("first")),
        ("x", "C-a", true, None),
        ("prefix", "b", false, Some("prefix")),
        ("root", "c", false, Some("root")),
    ] {
        engine.keys.bind(
            table,
            key,
            Binding {
                commands: vec![CommandInvocation::new("display-message", [key])],
                repeat,
                note: note.map(str::to_owned),
            },
        );
    }
    engine
}

#[test]
fn default_key_listing_reuses_output_and_bypasses_option_hook_overrides() {
    let engine = key_listing_engine();
    let args = [RawText::from("-T"), RawText::from("x")];
    let context = ExecutionContext::default();
    let mut hooks = KeyListingHooks {
        calls: 0,
        command: None,
    };
    let first = engine.list_keys(&context, &args, &mut hooks).unwrap();
    let cold_calls = hooks.calls;
    hooks.calls = 0;
    assert_eq!(
        engine.list_keys(&context, &args, &mut hooks).unwrap(),
        first
    );
    assert_eq!(
        engine.key_listing_cache.lock().is_some(),
        crate::format_cache_knob()
    );
    if crate::format_cache_knob() {
        assert_eq!(hooks.calls, 10);
        assert!(cold_calls > hooks.calls);
    } else {
        assert_eq!(hooks.calls, cold_calls);
    }
    for command in ["override-one", "override-two"] {
        hooks.command = Some(command);
        let output = engine
            .list_keys(&context, &args, &mut hooks)
            .unwrap()
            .output;
        assert!(output.lines().all(|line| line.ends_with(command)));
    }
}

#[test]
fn default_key_listing_proven_tmux_hooks_skip_scoped_option_probes() {
    struct ProvenHooks;

    impl StatusHooks for ProvenHooks {
        fn stable_option_lookups(&self) -> bool {
            true
        }

        fn only_tmux_options(&self) -> bool {
            true
        }

        fn strftime(&mut self, _: &str) -> String {
            String::new()
        }

        fn shell(&mut self, _: &str, _: &FormatJobTag) -> String {
            String::new()
        }

        fn option_variable(&mut self, name: &str, _: &StatusContext) -> Option<String> {
            panic!("cached listing unexpectedly probed {name}")
        }
    }

    assert!(*LIST_KEY_FORMAT_NAMES_ARE_NOT_OPTIONS);
    let engine = key_listing_engine();
    let args = [RawText::from("-T"), RawText::from("x")];
    let context = ExecutionContext::default();
    let first = engine
        .list_keys(&context, &args, &mut CommandHooks::new(0))
        .unwrap();
    if crate::format_cache_knob() {
        assert_eq!(
            engine.list_keys(&context, &args, &mut ProvenHooks).unwrap(),
            first,
        );
        let mut hooks = RowFormatHooks {
            inner: &mut ProvenHooks,
            line: 5,
        };
        assert_eq!(
            engine.list_keys(&context, &args, &mut hooks).unwrap(),
            first
        );
    } else {
        assert!(engine.key_listing_cache.lock().is_none());
    }
}

#[test]
fn default_key_listing_custom_option_hooks_keep_the_resolved_target_context() {
    struct ScopedHooks {
        contexts: Vec<(String, String, String)>,
    }

    impl StatusHooks for ScopedHooks {
        fn stable_option_lookups(&self) -> bool {
            true
        }

        fn strftime(&mut self, _: &str) -> String {
            String::new()
        }

        fn shell(&mut self, _: &str, _: &FormatJobTag) -> String {
            String::new()
        }

        fn option_variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
            if name != "key_command" {
                return None;
            }
            self.contexts.push((
                context.session_id.clone(),
                context.window_id.clone(),
                context.pane_id.clone(),
            ));
            Some(context.pane_id.clone())
        }
    }

    let mut engine = key_listing_engine();
    let first = engine.state.create_session("first").unwrap();
    let second = engine.state.create_session("second").unwrap();
    let args = [RawText::from("-T"), RawText::from("x")];
    engine
        .list_keys(
            &ExecutionContext::new(Some(first.0), Some(first.1), Some(first.2)),
            &args,
            &mut CommandHooks::new(0),
        )
        .unwrap();
    for (session, window, pane) in [first, second] {
        let context = ExecutionContext::new(Some(session), Some(window), Some(pane));
        let mut hooks = ScopedHooks {
            contexts: Vec::new(),
        };
        assert!(!hooks.only_tmux_options());
        let output = engine
            .list_keys(&context, &args, &mut hooks)
            .unwrap()
            .output;
        let expected = (session.to_string(), window.to_string(), pane.to_string());
        assert!(!hooks.contexts.is_empty());
        assert!(hooks.contexts.iter().all(|context| context == &expected));
        assert!(output.lines().all(|line| line.ends_with(&pane.to_string())));
    }
}

#[test]
fn default_key_listing_user_options_do_not_override_ordinary_binding_names() {
    let mut engine = key_listing_engine();
    let mut context = ExecutionContext::default();
    let command = CommandInvocation::new("list-keys", ["-T", "x"]);
    let first = engine.execute(&mut context, &command).unwrap();
    for name in LIST_KEY_BINDING_CONTEXT_FORMATS
        .iter()
        .chain(LIST_KEY_SUMMARY_CONTEXT_FORMATS)
    {
        set(
            &mut engine,
            &mut context,
            &["-g", &format!("@{name}"), "override"],
        );
    }
    assert_eq!(
        engine.format_user_option("", "", "", "@key_command"),
        Some("override"),
    );
    assert_eq!(engine.execute(&mut context, &command).unwrap(), first);
}

#[test]
fn stateful_option_hooks_keep_uncached_listing_output_and_lookup_order() {
    #[derive(Default)]
    struct StatefulHooks {
        names: Vec<String>,
        commands: usize,
    }

    impl StatusHooks for StatefulHooks {
        fn strftime(&mut self, _: &str) -> String {
            String::new()
        }

        fn shell(&mut self, _: &str, _: &FormatJobTag) -> String {
            String::new()
        }

        fn option_variable(&mut self, name: &str, _: &StatusContext) -> Option<String> {
            self.names.push(name.to_owned());
            if name != "key_command" {
                return None;
            }
            self.commands += 1;
            (self.commands > 1).then(|| format!("override-{}", self.commands))
        }
    }

    let engine = key_listing_engine();
    let context = ExecutionContext::default();
    let args = [RawText::from("-T"), RawText::from("x")];
    engine
        .list_keys(&context, &args, &mut CommandHooks::new(0))
        .unwrap();
    let mut hooks = StatefulHooks::default();
    let actual = engine.list_keys(&context, &args, &mut hooks).unwrap();
    let uncached_args = [
        RawText::from("-T"),
        RawText::from("x"),
        RawText::from("-F"),
        RawText::from(format!("{DEFAULT_LIST_KEYS_FORMAT}#{{l:}}")),
    ];
    let mut uncached_hooks = StatefulHooks::default();
    let expected = engine
        .list_keys(&context, &uncached_args, &mut uncached_hooks)
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(hooks.names, uncached_hooks.names);
    assert_eq!(hooks.commands, 2);
    assert!(actual.output.contains("override-2"));
}

#[test]
fn default_key_listing_variants_match_the_existing_formatter() {
    let engine = key_listing_engine();
    let context = ExecutionContext::default();
    let custom = format!("{DEFAULT_LIST_KEYS_FORMAT}#{{l:}}");
    for args in [
        vec![],
        vec!["-T", "x"],
        vec!["-N"],
        vec!["-N", "-a"],
        vec!["-N", "-a", "-P", "ZZ"],
        vec!["-1", "-T", "x"],
        vec!["-O", "key", "-r"],
        vec!["-T", "x", "C-a"],
        vec!["-T", "x", "--", "a"],
        vec!["-a"],
        vec!["-r"],
        vec!["-F", DEFAULT_LIST_KEYS_FORMAT],
    ] {
        let mut hooks = CommandHooks::new(0);
        let args = args.into_iter().map(RawText::from).collect::<Vec<_>>();
        let first = engine.list_keys(&context, &args, &mut hooks).unwrap();
        assert_eq!(
            engine.list_keys(&context, &args, &mut hooks).unwrap(),
            first
        );
        let mut uncached = vec![RawText::from("-F"), RawText::from(custom.as_str())];
        uncached.extend(args.iter().cloned());
        if args.first().is_some_and(|argument| argument == "-F") {
            uncached.truncate(2);
        }
        assert_eq!(
            engine.list_keys(&context, &uncached, &mut hooks).unwrap(),
            first,
            "{args:?}"
        );
    }
}

#[test]
fn default_key_listing_refreshes_after_binding_metadata_and_prefix_changes() {
    let mut engine = key_listing_engine();
    let mut context = ExecutionContext::default();
    let list = |engine: &mut MuxEngine, context: &mut ExecutionContext, args: &[&str]| {
        engine
            .execute(
                context,
                &CommandInvocation::new("list-keys", args.iter().copied()),
            )
            .unwrap()
            .output
    };
    let args = ["-N", "-a", "-T", "x"];
    let before = list(&mut engine, &mut context, &args);
    assert_eq!(list(&mut engine, &mut context, &args), before);
    engine
        .keys
        .update_binding_metadata("x", "a", Some("changed".to_owned()), true);
    assert!(list(&mut engine, &mut context, &args).contains("changed"));
    engine.keys.set_prefix("C-z");
    assert!(
        list(&mut engine, &mut context, &args)
            .lines()
            .all(|line| line.starts_with("C-z "))
    );
    engine.keys.bind(
        "x",
        "C-a",
        Binding {
            commands: vec![CommandInvocation::new("display-message", ["replacement"])],
            repeat: false,
            note: None,
        },
    );
    assert!(list(&mut engine, &mut context, &args).contains("replacement"));
    engine.keys.unbind("x", "a");
    assert!(!list(&mut engine, &mut context, &args).contains("changed"));
    engine.keys.remove_table("x");
    assert!(
        engine
            .execute(&mut context, &CommandInvocation::new("list-keys", args))
            .is_err()
    );
}

#[test]
fn cached_key_listing_rebuilds_single_effects_and_custom_formats_stay_live() {
    let mut engine = key_listing_engine();
    let (session, window, pane) = engine.state.create_session("live").unwrap();
    let mut context = ExecutionContext::new(Some(session), Some(window), Some(pane));
    let single = CommandInvocation::new("list-keys", ["-1", "-T", "x"]);
    engine.execute(&mut context, &single).unwrap();
    set(&mut engine, &mut context, &["display-time", "321"]);
    assert!(matches!(
        engine.execute(&mut context, &single).unwrap().effects.as_slice(),
        [MuxEffect::PrintOrMessage { pane: target, duration_ms: 321, .. }] if *target == Some(pane)
    ));
    let (_, _, second_pane) = engine.state.create_session("second").unwrap();
    context.pane = Some(second_pane);
    assert!(matches!(
        engine.execute(&mut context, &single).unwrap().effects.as_slice(),
        [MuxEffect::PrintOrMessage { pane: target, .. }] if *target == Some(second_pane)
    ));
    context.pane = Some(pane);
    let custom = CommandInvocation::new("list-keys", ["-T", "x", "-F", "#{pane_title}"]);
    for title in ["first-title", "second-title"] {
        engine.state.update_pane_title(pane, title).unwrap();
        assert!(
            engine
                .execute(&mut context, &custom)
                .unwrap()
                .output
                .lines()
                .all(|line| line == title)
        );
    }
}

#[test]
fn default_key_listing_does_not_cache_oversized_output() {
    let mut engine = key_listing_engine();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("list-keys", [] as [&str; 0]),
        )
        .unwrap();
    engine
        .keys
        .update_binding_metadata("x", "a", Some("x".repeat(1024 * 1024)), false);
    let output = engine
        .execute(
            &mut context,
            &CommandInvocation::new("list-keys", ["-N", "-T", "x"]),
        )
        .unwrap()
        .output;
    assert!(output.len() > 1024 * 1024);
    assert!(engine.key_listing_cache.lock().is_none());
}

#[test]
fn message_scalar_getters_follow_scoped_inheritance_and_unset() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "message"]),
        )
        .unwrap();
    let session = context.session;
    let defaults = (
        MESSAGE_STYLE_DEFAULT.to_owned(),
        MESSAGE_COMMAND_STYLE_DEFAULT.to_owned(),
    );
    assert_eq!(engine.message_styles_for_session(session), defaults);
    assert_eq!(engine.message_line_for_session(session), 0);
    for args in [
        ["-g", "message-style", "fg=blue"],
        ["-g", "message-command-style", "bg=yellow"],
        ["-g", "message-line", "4"],
    ] {
        set(&mut engine, &mut context, &args);
    }
    assert_eq!(
        engine.message_styles_for_session(session),
        ("fg=blue".to_owned(), "bg=yellow".to_owned())
    );
    assert_eq!(engine.message_line_for_session(session), 4);
    set(&mut engine, &mut context, &["message-style", "fg=red"]);
    set(&mut engine, &mut context, &["-a", "message-style", "bold"]);
    set(
        &mut engine,
        &mut context,
        &["message-command-style", "bg=green"],
    );
    set(&mut engine, &mut context, &["message-line", "2"]);
    assert_eq!(
        engine.message_styles_for_session(session),
        ("fg=red,bold".to_owned(), "bg=green".to_owned())
    );
    assert_eq!(engine.message_line_for_session(session), 2);
    assert_eq!(
        engine.message_styles_for_session(None),
        ("fg=blue".to_owned(), "bg=yellow".to_owned())
    );
    assert_eq!(engine.message_line_for_session(None), 4);
    for name in ["message-style", "message-command-style", "message-line"] {
        set(&mut engine, &mut context, &["-u", name]);
    }
    assert_eq!(
        engine.message_styles_for_session(session),
        engine.message_styles_for_session(None)
    );
    assert_eq!(engine.message_line_for_session(session), 4);
    for name in ["message-style", "message-command-style", "message-line"] {
        set(&mut engine, &mut context, &["-gu", name]);
    }
    assert_eq!(engine.message_styles_for_session(session), defaults);
    assert_eq!(engine.message_line_for_session(session), 0);
}
