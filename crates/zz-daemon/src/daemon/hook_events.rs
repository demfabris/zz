use std::sync::LazyLock;

use zz_mux::{ChangeWindow, JournalChanges, MuxState};

use super::*;

pub(super) static READONLY_SKIP: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_READONLY_SKIP").is_none_or(|value| value != "0"));

pub(super) static EAGER_FACTS: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_EAGER_FACTS").is_some_and(|value| value == "1"));

pub(super) static HOOK_JOURNAL: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_HOOK_JOURNAL").is_none_or(|value| value != "0"));

pub(super) fn log_knobs() {
    log::info!(
        target: "zz_daemon::perf",
        "hook knobs: ZZ_PERF_READONLY_SKIP={} ZZ_PERF_EAGER_FACTS={} ZZ_PERF_HOOK_JOURNAL={}",
        u8::from(*READONLY_SKIP),
        u8::from(*EAGER_FACTS),
        u8::from(*HOOK_JOURNAL),
    );
}

pub(super) fn command_is_read_only(command: &str, args: &[RawText]) -> bool {
    *READONLY_SKIP
        && zz_protocol::catalog_command_spec(command).is_some_and(|spec| !spec.mutates(args))
}

pub(super) fn format_facts_unread(command: &str, args: &[RawText]) -> bool {
    if *EAGER_FACTS {
        return false;
    }
    match command {
        "bind-key" | "unbind-key" | "has-session" => true,
        "set-option" | "set-window-option" => zz_protocol::catalog_command_spec(command)
            .and_then(|spec| zz_protocol::parse_tmux_options(spec, args).ok())
            .is_some_and(|parsed| {
                let expanded = parsed
                    .options
                    .contains(&zz_protocol::TmuxOption::Flag("-F"));
                let mut positionals = parsed.positionals.iter();
                let name = positionals.next().map_or("", |name| &**name);
                !name.is_empty()
                    && !name.contains('#')
                    && !"automatic-rename".starts_with(name)
                    && (!expanded || !positionals.any(|value| value.contains('#')))
            }),
        "show-options" | "show-window-options" => {
            args.iter().all(|argument| !argument.contains('#'))
        }
        _ => false,
    }
}

pub(super) fn unread_format_facts() -> &'static FormatHookFacts {
    static UNREAD: LazyLock<FormatHookFacts> = LazyLock::new(FormatHookFacts::default);
    &UNREAD
}

pub(super) struct HookScope {
    window: Option<ChangeWindow>,
    before: Option<MuxHookSnapshot>,
}

pub(super) struct HookDiff {
    pub(super) before: MuxHookSnapshot,
    pub(super) events: Vec<PendingHookEvent>,
    partial: Option<BTreeSet<SessionId>>,
}

impl HookDiff {
    pub(super) fn before_session_context(
        &self,
        state: &MuxState,
        session: SessionId,
    ) -> ExecutionContext {
        match &self.partial {
            Some(touched) if !touched.contains(&session) => live_session_context(state, session),
            _ => self.before.session_context(session),
        }
    }
}

impl HookScope {
    pub(super) fn open(engine: &mut MuxEngine) -> Self {
        if *HOOK_JOURNAL {
            Self {
                before: cfg!(debug_assertions).then(|| MuxHookSnapshot::capture(engine)),
                window: Some(engine.state.open_change_window()),
            }
        } else {
            Self {
                window: None,
                before: Some(MuxHookSnapshot::capture(engine)),
            }
        }
    }

    pub(super) fn changes<'a>(&self, engine: &'a MuxEngine) -> Option<JournalChanges<'a>> {
        self.window
            .as_ref()
            .map(|window| engine.state.changes_since(window))
    }

    pub(super) fn finish(self, engine: &MuxEngine, command: &str) -> HookDiff {
        let Some(window) = self.window else {
            let before = self
                .before
                .expect("a snapshot scope holds its before state");
            let after = MuxHookSnapshot::capture(engine);
            let events = mux_hook_events(&before, &after, command);
            return HookDiff {
                before,
                events,
                partial: None,
            };
        };
        let changes = engine.state.changes_since(&window);
        let (before, after) = journal_snapshots(engine, &changes);
        let events = mux_hook_events(&before, &after, command);
        if let Some(full_before) = &self.before {
            let expected = mux_hook_events(full_before, &MuxHookSnapshot::capture(engine), command);
            assert!(
                events == expected,
                "the change journal missed a hook for {command:?}: journal {:?}, snapshots {:?}",
                describe_events(&events),
                describe_events(&expected),
            );
        }
        HookDiff {
            before,
            events,
            partial: Some(changes.sessions.keys().copied().collect()),
        }
    }
}

fn describe_events(events: &[PendingHookEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| format!("{} {:?}", event.name, event.variables))
        .collect()
}

pub(super) fn journal_active_windows(changes: &JournalChanges) -> BTreeMap<SessionId, WindowId> {
    changes
        .sessions
        .iter()
        .filter_map(|(session, image)| image.map(|image| (*session, image.active_window)))
        .collect()
}

pub(super) fn journal_active_panes(changes: &JournalChanges) -> BTreeMap<WindowId, PaneId> {
    changes
        .windows
        .iter()
        .filter_map(|(window, image)| image.map(|image| (*window, image.active_pane)))
        .collect()
}

pub(super) fn journal_belled_panes(changes: &JournalChanges) -> BTreeSet<PaneId> {
    changes
        .windows
        .values()
        .flatten()
        .flat_map(|image| image.panes.iter())
        .filter(|pane| pane.bell)
        .map(|pane| pane.id)
        .collect()
}

fn journal_snapshots(
    engine: &MuxEngine,
    changes: &JournalChanges,
) -> (MuxHookSnapshot, MuxHookSnapshot) {
    let state = &engine.state;
    let mut windows = changes.windows.keys().copied().collect::<BTreeSet<_>>();
    for (session, image) in &changes.sessions {
        if let Some(image) = image {
            windows.extend(image.windows.iter().copied());
            windows.insert(image.active_window);
        }
        if let Some(current) = state.sessions.get(session) {
            windows.extend(current.windows.iter().copied());
        }
    }
    let mut sessions = changes.sessions.keys().copied().collect::<BTreeSet<_>>();
    for window in &windows {
        if let Some(Some(image)) = changes.windows.get(window) {
            sessions.insert(image.session);
        }
        if let Some(current) = state.windows.get(window) {
            sessions.insert(current.session);
        }
    }
    let mut before = MuxHookSnapshot::default();
    let mut after = MuxHookSnapshot::default();
    for session in &sessions {
        let current = state.sessions.get(session);
        if let Some(current) = current {
            after.add_session(
                *session,
                &current.name,
                current.active_window,
                &current.windows,
            );
        }
        match changes.sessions.get(session) {
            Some(Some(image)) => {
                before.add_session(*session, &image.name, image.active_window, &image.windows);
            }
            Some(None) => {}
            None => {
                if let Some(current) = current {
                    before.add_session(
                        *session,
                        &current.name,
                        current.active_window,
                        &current.windows,
                    );
                }
            }
        }
    }
    for window in &windows {
        let current = state.windows.get(window);
        if let Some(current) = current {
            after.add_window(engine, *window, current);
        }
        match changes.windows.get(window) {
            Some(Some(image)) => before.add_window_image(*window, image),
            Some(None) => {}
            None => {
                if let Some(current) = current {
                    before.add_window(engine, *window, current);
                }
            }
        }
    }
    (before, after)
}

impl PendingHookEvent {
    pub(super) fn live_pane(name: &'static str, pane: PaneId, engine: &MuxEngine) -> Option<Self> {
        let window = engine.state.window_for_pane(pane)?;
        let window_state = engine.state.windows.get(&window)?;
        let session_name = engine
            .state
            .sessions
            .get(&window_state.session)
            .map(|session| session.name.clone())
            .unwrap_or_default();
        Some(Self::pane_named(
            name,
            pane,
            window_state.session,
            window,
            session_name,
            window_state.name.clone(),
        ))
    }
}

pub(super) struct FocusProbeScope {
    probe: PaneFocusProbe,
    window: Option<ChangeWindow>,
    full: Option<PaneFocusProbe>,
}

impl FocusProbeScope {
    pub(super) fn open(inner: &mut ServerState) -> Self {
        if *HOOK_JOURNAL {
            Self {
                probe: capture_client_focus_probe(inner),
                full: cfg!(debug_assertions).then(|| capture_pane_focus_probe(inner)),
                window: Some(inner.engine.state.open_change_window()),
            }
        } else {
            Self {
                probe: capture_pane_focus_probe(inner),
                window: None,
                full: None,
            }
        }
    }

    pub(super) fn close(self, inner: &ServerState) -> PaneFocusProbe {
        let mut probe = self.probe;
        if let Some(window) = &self.window {
            let changes = inner.engine.state.changes_since(window);
            probe.window_active_panes = journal_active_panes(&changes);
            probe.session_windows = journal_active_windows(&changes);
        }
        if let Some(full) = &self.full {
            let journal = pane_focus_candidates(inner, &probe);
            let expected = pane_focus_candidates(inner, full);
            assert!(
                journal == expected,
                "the change journal missed a focus change: journal {journal:?}, snapshots {expected:?}"
            );
        }
        probe
    }
}

pub(super) fn live_session_context(state: &MuxState, session: SessionId) -> ExecutionContext {
    let Some(active_window) = state
        .sessions
        .get(&session)
        .map(|state| state.active_window)
    else {
        return ExecutionContext::new(Some(session), None, None);
    };
    let pane = state
        .windows
        .get(&active_window)
        .map(|window| window.active_pane);
    ExecutionContext::new(Some(session), Some(active_window), pane)
}
