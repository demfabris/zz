use zz_mux::{ChangeWindow, JournalChanges, MuxState};
use zz_protocol::PaneBorderStatus;

use super::*;

pub(super) fn command_is_read_only(command: &str, args: &[RawText]) -> bool {
    zz_protocol::catalog_command_spec(command).is_some_and(|spec| !spec.mutates(args))
}

pub(super) fn format_facts_unread(command: &str, args: &[RawText]) -> bool {
    expands_no_format(command, args)
}

pub(super) fn expands_no_format(command: &str, args: &[RawText]) -> bool {
    match command {
        "bind-key" | "unbind-key" | "has-session" => true,
        "display-message" => zz_protocol::catalog_command_spec(command)
            .and_then(|spec| zz_protocol::parse_tmux_options(spec, args).ok())
            .is_some_and(|parsed| {
                if parsed
                    .options
                    .contains(&zz_protocol::TmuxOption::Flag("-a"))
                {
                    return false;
                }
                if parsed
                    .options
                    .contains(&zz_protocol::TmuxOption::Flag("-l"))
                {
                    return true;
                }
                if !parsed.positionals.is_empty() {
                    return parsed
                        .positionals
                        .iter()
                        .all(|value| !value.contains(['#', '%']));
                }
                parsed
                    .options
                    .iter()
                    .rev()
                    .find_map(|option| match option {
                        zz_protocol::TmuxOption::Value("-F", value) => Some(value),
                        _ => None,
                    })
                    .is_some_and(|value| !value.contains(['#', '%']))
            }),
        "list-keys" | "source-file" => zz_protocol::catalog_command_spec(command)
            .and_then(|spec| zz_protocol::parse_tmux_options(spec, args).ok())
            .is_some_and(|parsed| {
                !parsed.options.iter().any(|option| {
                    matches!(
                        option,
                        zz_protocol::TmuxOption::Flag("-F")
                            | zz_protocol::TmuxOption::Value("-F", _)
                    )
                })
            }),
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

pub(super) struct HookDiff<'a> {
    pub(super) before: BeforeView<'a>,
    pub(super) events: Vec<PendingHookEvent>,
}

impl HookDiff<'_> {
    pub(super) fn before_session_context(&self, session: SessionId) -> ExecutionContext {
        session_context(&self.before, session)
    }

    pub(super) fn closed_sessions(&self, state: &MuxState) -> Vec<SessionId> {
        self.before
            .listed_sessions()
            .into_iter()
            .filter(|session| !state.sessions.contains_key(session))
            .collect()
    }
}

impl HookScope {
    pub(super) fn open(engine: &mut MuxEngine) -> Self {
        Self {
            before: cfg!(debug_assertions).then(|| MuxHookSnapshot::capture(engine)),
            window: Some(engine.state.open_change_window()),
        }
    }

    pub(super) fn change_window(&self) -> Option<&ChangeWindow> {
        self.window.as_ref()
    }

    pub(super) fn changes<'a>(&self, engine: &'a MuxEngine) -> Option<JournalChanges<'a>> {
        self.window
            .as_ref()
            .map(|window| engine.state.changes_since(window))
    }

    pub(super) fn finish<'a>(self, engine: &'a MuxEngine, command: &str) -> HookDiff<'a> {
        let border = |window: WindowId| engine.pane_border_status(window);
        let Some(window) = self.window else {
            let before = self
                .before
                .expect("a snapshot scope holds its before state");
            let after = MuxHookSnapshot::capture(engine);
            let events = mux_hook_events_in(&before, &after, command, &border);
            return HookDiff {
                before: BeforeView::Snapshot(before),
                events,
            };
        };
        let before = JournalView {
            engine,
            changes: engine.state.changes_since(&window),
        };
        let after = LiveView {
            engine,
            changes: &before.changes,
        };
        let events = if before.changes.sessions.is_empty() && before.changes.windows.is_empty() {
            Vec::new()
        } else {
            mux_hook_events_in(&before, &after, command, &border)
        };
        if let Some(full_before) = &self.before {
            let expected = mux_hook_events_in(
                full_before,
                &MuxHookSnapshot::capture(engine),
                command,
                &border,
            );
            assert!(
                events == expected,
                "the change journal missed a hook for {command:?}: journal {:?}, snapshots {:?}",
                describe_events(&events),
                describe_events(&expected),
            );
        }
        HookDiff {
            before: BeforeView::Journal(before),
            events,
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

#[derive(Clone, Copy)]
pub(super) struct WindowFacts<'a> {
    session: SessionId,
    index: u32,
    name: &'a str,
    active_pane: PaneId,
    zoomed_pane: Option<PaneId>,
    layout: &'a CellLayout,
    extent: (u16, u16),
}

#[derive(Clone, Copy)]
pub(super) struct PaneFacts<'a> {
    session: SessionId,
    window: WindowId,
    title: &'a str,
}

pub(super) trait HookView {
    fn session(&self, session: SessionId) -> Option<(&str, WindowId)>;

    fn window(&self, window: WindowId) -> Option<WindowFacts<'_>>;

    fn pane(&self, pane: PaneId) -> Option<PaneFacts<'_>>;

    fn listed_sessions(&self) -> Vec<SessionId>;

    fn listed_windows(&self) -> Vec<WindowId>;

    fn listed_panes(&self) -> Vec<(PaneId, PaneFacts<'_>)>;

    fn links(&self) -> BTreeSet<(SessionId, WindowId)>;
}

fn live_window(engine: &MuxEngine, window: WindowId) -> Option<WindowFacts<'_>> {
    let state = engine.state.windows.get(&window)?;
    Some(WindowFacts {
        session: state.session,
        index: state.index,
        name: &state.name,
        active_pane: state.active_pane,
        zoomed_pane: state.zoomed_pane,
        layout: &state.layout,
        extent: (
            engine
                .window_extent(window, zz_protocol::Axis::Horizontal)
                .unwrap_or_default(),
            engine
                .window_extent(window, zz_protocol::Axis::Vertical)
                .unwrap_or_default(),
        ),
    })
}

fn live_pane_in(engine: &MuxEngine, window: WindowId, pane: PaneId) -> Option<PaneFacts<'_>> {
    let state = engine.state.windows.get(&window)?;
    Some(PaneFacts {
        session: state.session,
        window,
        title: &state.panes.get(&pane)?.title,
    })
}

impl HookView for MuxHookSnapshot {
    fn session(&self, session: SessionId) -> Option<(&str, WindowId)> {
        self.sessions
            .get(&session)
            .map(|state| (state.name.as_str(), state.active_window))
    }

    fn window(&self, window: WindowId) -> Option<WindowFacts<'_>> {
        self.windows.get(&window).map(|state| WindowFacts {
            session: state.session,
            index: state.index,
            name: &state.name,
            active_pane: state.active_pane,
            zoomed_pane: state.zoomed_pane,
            layout: &state.layout,
            extent: state.extent,
        })
    }

    fn pane(&self, pane: PaneId) -> Option<PaneFacts<'_>> {
        self.panes.get(&pane).map(|state| PaneFacts {
            session: state.session,
            window: state.window,
            title: &state.title,
        })
    }

    fn listed_sessions(&self) -> Vec<SessionId> {
        self.sessions.keys().copied().collect()
    }

    fn listed_windows(&self) -> Vec<WindowId> {
        self.windows.keys().copied().collect()
    }

    fn listed_panes(&self) -> Vec<(PaneId, PaneFacts<'_>)> {
        self.panes
            .iter()
            .map(|(pane, state)| {
                (
                    *pane,
                    PaneFacts {
                        session: state.session,
                        window: state.window,
                        title: &state.title,
                    },
                )
            })
            .collect()
    }

    fn links(&self) -> BTreeSet<(SessionId, WindowId)> {
        self.links.clone()
    }
}

pub(super) struct JournalView<'a> {
    engine: &'a MuxEngine,
    changes: JournalChanges<'a>,
}

impl JournalView<'_> {
    fn image_pane(&self, pane: PaneId) -> Option<PaneFacts<'_>> {
        self.changes.windows.iter().find_map(|(window, image)| {
            let image = (*image)?;
            image
                .panes
                .iter()
                .find(|candidate| candidate.id == pane)
                .map(|candidate| PaneFacts {
                    session: image.session,
                    window: *window,
                    title: &candidate.title,
                })
        })
    }
}

impl HookView for JournalView<'_> {
    fn session(&self, session: SessionId) -> Option<(&str, WindowId)> {
        match self.changes.sessions.get(&session) {
            Some(image) => image.map(|image| (image.name.as_str(), image.active_window)),
            None => self
                .engine
                .state
                .sessions
                .get(&session)
                .map(|state| (state.name.as_str(), state.active_window)),
        }
    }

    fn window(&self, window: WindowId) -> Option<WindowFacts<'_>> {
        match self.changes.windows.get(&window) {
            Some(image) => image.map(|image| WindowFacts {
                session: image.session,
                index: image.index,
                name: &image.name,
                active_pane: image.active_pane,
                zoomed_pane: image.zoomed_pane,
                layout: &image.layout,
                extent: image.extent,
            }),
            None => live_window(self.engine, window),
        }
    }

    fn pane(&self, pane: PaneId) -> Option<PaneFacts<'_>> {
        if let Some(facts) = self.image_pane(pane) {
            return Some(facts);
        }
        let window = self.engine.state.window_for_pane(pane)?;
        if self.changes.windows.contains_key(&window) {
            return None;
        }
        live_pane_in(self.engine, window, pane)
    }

    fn listed_sessions(&self) -> Vec<SessionId> {
        self.changes
            .sessions
            .iter()
            .filter_map(|(session, image)| image.map(|_| *session))
            .collect()
    }

    fn listed_windows(&self) -> Vec<WindowId> {
        self.changes
            .windows
            .iter()
            .filter_map(|(window, image)| image.map(|_| *window))
            .collect()
    }

    fn listed_panes(&self) -> Vec<(PaneId, PaneFacts<'_>)> {
        let mut panes = self
            .changes
            .windows
            .iter()
            .filter_map(|(window, image)| image.map(|image| (*window, image)))
            .flat_map(|(window, image)| {
                image.panes.iter().map(move |pane| {
                    (
                        pane.id,
                        PaneFacts {
                            session: image.session,
                            window,
                            title: &pane.title,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        panes.sort_unstable_by_key(|(pane, _)| *pane);
        panes
    }

    fn links(&self) -> BTreeSet<(SessionId, WindowId)> {
        self.changes
            .sessions
            .iter()
            .filter_map(|(session, image)| image.map(|image| (*session, image)))
            .flat_map(|(session, image)| image.windows.iter().map(move |window| (session, *window)))
            .collect()
    }
}

struct LiveView<'a, 'j> {
    engine: &'a MuxEngine,
    changes: &'j JournalChanges<'a>,
}

impl HookView for LiveView<'_, '_> {
    fn session(&self, session: SessionId) -> Option<(&str, WindowId)> {
        self.engine
            .state
            .sessions
            .get(&session)
            .map(|state| (state.name.as_str(), state.active_window))
    }

    fn window(&self, window: WindowId) -> Option<WindowFacts<'_>> {
        live_window(self.engine, window)
    }

    fn pane(&self, pane: PaneId) -> Option<PaneFacts<'_>> {
        let window = self
            .changes
            .windows
            .keys()
            .copied()
            .find(|window| {
                self.engine
                    .state
                    .windows
                    .get(window)
                    .is_some_and(|state| state.panes.contains_key(&pane))
            })
            .or_else(|| self.engine.state.window_for_pane(pane))?;
        live_pane_in(self.engine, window, pane)
    }

    fn listed_sessions(&self) -> Vec<SessionId> {
        self.changes
            .sessions
            .keys()
            .copied()
            .filter(|session| self.engine.state.sessions.contains_key(session))
            .collect()
    }

    fn listed_windows(&self) -> Vec<WindowId> {
        self.changes
            .windows
            .keys()
            .copied()
            .filter(|window| self.engine.state.windows.contains_key(window))
            .collect()
    }

    fn listed_panes(&self) -> Vec<(PaneId, PaneFacts<'_>)> {
        let mut panes = self
            .changes
            .windows
            .keys()
            .filter_map(|window| {
                self.engine
                    .state
                    .windows
                    .get(window)
                    .map(|state| (*window, state))
            })
            .flat_map(|(window, state)| {
                state.panes.iter().map(move |(pane, pane_state)| {
                    (
                        *pane,
                        PaneFacts {
                            session: state.session,
                            window,
                            title: &pane_state.title,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        panes.sort_unstable_by_key(|(pane, _)| *pane);
        panes
    }

    fn links(&self) -> BTreeSet<(SessionId, WindowId)> {
        self.changes
            .sessions
            .keys()
            .filter_map(|session| {
                self.engine
                    .state
                    .sessions
                    .get(session)
                    .map(|state| (*session, state))
            })
            .flat_map(|(session, state)| state.windows.iter().map(move |window| (session, *window)))
            .collect()
    }
}

pub(super) enum BeforeView<'a> {
    Snapshot(MuxHookSnapshot),
    Journal(JournalView<'a>),
}

impl HookView for BeforeView<'_> {
    fn session(&self, session: SessionId) -> Option<(&str, WindowId)> {
        match self {
            Self::Snapshot(view) => view.session(session),
            Self::Journal(view) => view.session(session),
        }
    }

    fn window(&self, window: WindowId) -> Option<WindowFacts<'_>> {
        match self {
            Self::Snapshot(view) => view.window(window),
            Self::Journal(view) => view.window(window),
        }
    }

    fn pane(&self, pane: PaneId) -> Option<PaneFacts<'_>> {
        match self {
            Self::Snapshot(view) => view.pane(pane),
            Self::Journal(view) => view.pane(pane),
        }
    }

    fn listed_sessions(&self) -> Vec<SessionId> {
        match self {
            Self::Snapshot(view) => view.listed_sessions(),
            Self::Journal(view) => view.listed_sessions(),
        }
    }

    fn listed_windows(&self) -> Vec<WindowId> {
        match self {
            Self::Snapshot(view) => view.listed_windows(),
            Self::Journal(view) => view.listed_windows(),
        }
    }

    fn listed_panes(&self) -> Vec<(PaneId, PaneFacts<'_>)> {
        match self {
            Self::Snapshot(view) => view.listed_panes(),
            Self::Journal(view) => view.listed_panes(),
        }
    }

    fn links(&self) -> BTreeSet<(SessionId, WindowId)> {
        match self {
            Self::Snapshot(view) => view.links(),
            Self::Journal(view) => view.links(),
        }
    }
}

pub(super) fn session_context(view: &impl HookView, session: SessionId) -> ExecutionContext {
    let Some((_, active_window)) = view.session(session) else {
        return ExecutionContext::new(Some(session), None, None);
    };
    let pane = view.window(active_window).map(|window| window.active_pane);
    ExecutionContext::new(Some(session), Some(active_window), pane)
}

pub(super) fn window_context(view: &impl HookView, window: WindowId) -> ExecutionContext {
    view.window(window).map_or_else(
        || ExecutionContext::new(None, Some(window), None),
        |state| ExecutionContext::new(Some(state.session), Some(window), Some(state.active_pane)),
    )
}

pub(super) fn session_event(
    name: &'static str,
    session: SessionId,
    session_name: &str,
    view: &impl HookView,
) -> PendingHookEvent {
    PendingHookEvent {
        name,
        context: session_context(view, session),
        exclude_client: None,
        control_notified: false,
        variables: BTreeMap::from([
            (HOOK_CONTEXT_FORMAT.to_owned(), name.to_owned()),
            (HOOK_SESSION_CONTEXT_FORMAT.to_owned(), session.to_string()),
            (
                HOOK_SESSION_NAME_CONTEXT_FORMAT.to_owned(),
                session_name.to_owned(),
            ),
        ]),
    }
}

pub(super) fn window_event(
    name: &'static str,
    window: WindowId,
    session: SessionId,
    window_name: &str,
    active_pane: PaneId,
    view: &impl HookView,
) -> PendingHookEvent {
    let session_name = view
        .session(session)
        .map(|(name, _)| name.to_owned())
        .unwrap_or_default();
    PendingHookEvent {
        name,
        context: window_context(view, window),
        exclude_client: None,
        control_notified: false,
        variables: BTreeMap::from([
            (HOOK_CONTEXT_FORMAT.to_owned(), name.to_owned()),
            (HOOK_SESSION_CONTEXT_FORMAT.to_owned(), session.to_string()),
            (HOOK_SESSION_NAME_CONTEXT_FORMAT.to_owned(), session_name),
            (HOOK_WINDOW_CONTEXT_FORMAT.to_owned(), window.to_string()),
            (
                HOOK_WINDOW_NAME_CONTEXT_FORMAT.to_owned(),
                window_name.to_owned(),
            ),
            (HOOK_PANE_CONTEXT_FORMAT.to_owned(), active_pane.to_string()),
        ]),
    }
}

pub(super) fn winlink_event(
    name: &'static str,
    session: SessionId,
    session_name: &str,
    window: WindowId,
    window_name: &str,
    view: &impl HookView,
) -> PendingHookEvent {
    PendingHookEvent {
        name,
        context: window_context(view, window),
        exclude_client: None,
        control_notified: false,
        variables: BTreeMap::from([
            (HOOK_CONTEXT_FORMAT.to_owned(), name.to_owned()),
            (HOOK_SESSION_CONTEXT_FORMAT.to_owned(), session.to_string()),
            (
                HOOK_SESSION_NAME_CONTEXT_FORMAT.to_owned(),
                session_name.to_owned(),
            ),
            (HOOK_WINDOW_CONTEXT_FORMAT.to_owned(), window.to_string()),
            (
                HOOK_WINDOW_NAME_CONTEXT_FORMAT.to_owned(),
                window_name.to_owned(),
            ),
        ]),
    }
}

pub(super) fn pane_event(
    name: &'static str,
    pane: PaneId,
    session: SessionId,
    window: WindowId,
    view: &impl HookView,
) -> PendingHookEvent {
    let window_name = view
        .window(window)
        .map(|window| window.name.to_owned())
        .unwrap_or_default();
    let session_name = view
        .session(session)
        .map(|(name, _)| name.to_owned())
        .unwrap_or_default();
    PendingHookEvent::pane_named(name, pane, session, window, session_name, window_name)
}

fn put_session(
    variables: &mut BTreeMap<String, String>,
    key: &str,
    session: SessionId,
    view: &impl HookView,
) {
    variables.insert(format!("hook_{key}"), session.to_string());
    if let Some((name, _)) = view.session(session) {
        variables.insert(format!("hook_{key}_name"), name.to_owned());
    }
}

fn put_window(
    variables: &mut BTreeMap<String, String>,
    key: &str,
    window: WindowId,
    view: &impl HookView,
) {
    variables.insert(format!("hook_{key}"), window.to_string());
    if let Some(state) = view.window(window) {
        variables.insert(format!("hook_{key}_name"), state.name.to_owned());
    }
}

fn payload_event(
    name: &'static str,
    context: ExecutionContext,
    variables: BTreeMap<String, String>,
) -> PendingHookEvent {
    PendingHookEvent {
        name,
        context,
        exclude_client: None,
        control_notified: false,
        variables,
    }
}

fn hook_variables(name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([(HOOK_CONTEXT_FORMAT.to_owned(), name.to_owned())])
}

fn window_payload_event(
    name: &'static str,
    window: WindowId,
    view: &impl HookView,
) -> PendingHookEvent {
    let mut variables = hook_variables(name);
    put_window(&mut variables, "window", window, view);
    payload_event(name, window_context(view, window), variables)
}

fn pane_context(view: &impl HookView, pane: PaneId, window: WindowId) -> ExecutionContext {
    let session = view.window(window).map(|state| state.session);
    ExecutionContext::new(session, Some(window), Some(pane))
}

fn pane_payload(
    name: &'static str,
    pane: PaneId,
    window: WindowId,
    view: &impl HookView,
) -> BTreeMap<String, String> {
    let mut variables = hook_variables(name);
    variables.insert(HOOK_PANE_CONTEXT_FORMAT.to_owned(), pane.to_string());
    put_window(&mut variables, "window", window, view);
    variables
}

fn pane_created_event(pane: PaneId, window: WindowId, view: &impl HookView) -> PendingHookEvent {
    let mut variables = pane_payload("pane-created", pane, window, view);
    if let Some(state) = view.window(window) {
        put_session(&mut variables, "session", state.session, view);
        variables.insert("hook_window_index".to_owned(), state.index.to_string());
    }
    variables.insert("hook_created_empty".to_owned(), "0".to_owned());
    variables.insert("hook_created_respawn".to_owned(), "0".to_owned());
    payload_event("pane-created", pane_context(view, pane, window), variables)
}

fn pane_resized_event(
    pane: PaneId,
    window: WindowId,
    size: (u16, u16),
    old: (u16, u16),
    view: &impl HookView,
) -> PendingHookEvent {
    let mut variables = pane_payload("pane-resized", pane, window, view);
    variables.insert("hook_width".to_owned(), size.0.to_string());
    variables.insert("hook_height".to_owned(), size.1.to_string());
    variables.insert("hook_old_width".to_owned(), old.0.to_string());
    variables.insert("hook_old_height".to_owned(), old.1.to_string());
    payload_event("pane-resized", pane_context(view, pane, window), variables)
}

fn pane_moved_event(
    pane: PaneId,
    old_window: WindowId,
    window: WindowId,
    before: &impl HookView,
    after: &impl HookView,
) -> PendingHookEvent {
    let mut variables = pane_payload("pane-moved", pane, window, after);
    put_window(&mut variables, "new_window", window, after);
    put_window(&mut variables, "old_window", old_window, before);
    if let Some(state) = before.window(old_window) {
        variables.insert("hook_old_window_index".to_owned(), state.index.to_string());
    }
    if let Some(state) = after.window(window) {
        variables.insert("hook_window_index".to_owned(), state.index.to_string());
        variables.insert("hook_new_window_index".to_owned(), state.index.to_string());
    }
    payload_event("pane-moved", pane_context(after, pane, window), variables)
}

fn pane_size(
    view: &impl HookView,
    window: WindowId,
    pane: PaneId,
    border: &dyn Fn(WindowId) -> PaneBorderStatus,
) -> Option<(u16, u16)> {
    let state = view.window(window)?;
    state
        .layout
        .displayed_pane_size(pane, state.zoomed_pane, border(window))
}

#[cfg(test)]
pub(super) fn mux_hook_events(
    before: &impl HookView,
    after: &impl HookView,
    command: &str,
) -> Vec<PendingHookEvent> {
    mux_hook_events_in(before, after, command, &|_| PaneBorderStatus::Off)
}

pub(super) fn mux_hook_events_in(
    before: &impl HookView,
    after: &impl HookView,
    command: &str,
    border: &dyn Fn(WindowId) -> PaneBorderStatus,
) -> Vec<PendingHookEvent> {
    let mut events = Vec::new();
    let before_links = before.links();
    let after_links = after.links();
    let before_panes = before
        .listed_panes()
        .into_iter()
        .map(|(pane, state)| (pane, (state.window, state.session, state.title.to_owned())))
        .collect::<BTreeMap<_, _>>();
    let after_panes = after
        .listed_panes()
        .into_iter()
        .map(|(pane, state)| (pane, (state.window, state.session, state.title.to_owned())))
        .collect::<BTreeMap<_, _>>();
    let mut created_windows = BTreeSet::new();
    for (session, window) in after_links.difference(&before_links) {
        if let (Some((session_name, _)), Some(window_state)) =
            (after.session(*session), after.window(*window))
        {
            if before.window(*window).is_none() && created_windows.insert(*window) {
                for (pane, _) in after_panes
                    .iter()
                    .filter(|(pane, (owner, _, _))| *owner == *window && !before_panes.contains_key(pane))
                {
                    events.push(pane_created_event(*pane, *window, after));
                }
                events.push(window_payload_event("window-created", *window, after));
            }
            events.push(winlink_event(
                "window-linked",
                *session,
                session_name,
                *window,
                window_state.name,
                after,
            ));
        }
    }
    if command == "new-session" {
        for session in after.listed_sessions() {
            if before.session(session).is_none()
                && let Some((name, _)) = after.session(session)
            {
                events.push(session_event("session-created", session, name, after));
            }
        }
    }
    if command == "rename-session" {
        for session in after.listed_sessions() {
            if let Some((name, _)) = after.session(session)
                && let Some((previous, _)) = before.session(session)
                && previous != name
            {
                let mut event = session_event("session-renamed", session, name, after);
                event
                    .variables
                    .insert("hook_old_name".to_owned(), previous.to_owned());
                event
                    .variables
                    .insert("hook_new_name".to_owned(), name.to_owned());
                events.push(event);
            }
        }
    }
    for session in after.listed_sessions() {
        let Some((name, active_window)) = after.session(session) else {
            continue;
        };
        if before
            .session(session)
            .is_some_and(|(_, previous)| previous != active_window)
            && let Some(window) = after.window(active_window)
        {
            events.push(winlink_event(
                "session-window-changed",
                session,
                name,
                active_window,
                window.name,
                after,
            ));
        }
    }
    for window in after.listed_windows() {
        let Some(state) = after.window(window) else {
            continue;
        };
        let previous = before.window(window);
        let geometry_changed = previous.is_none_or(|previous| {
            previous.layout != state.layout
                || previous.zoomed_pane != state.zoomed_pane
                || previous.extent != state.extent
        });
        let mut created = Vec::new();
        let mut moved = Vec::new();
        for (pane, (owner, _, _)) in &after_panes {
            if *owner != window {
                continue;
            }
            let old_size = match before_panes.get(pane) {
                Some((old_window, _, _)) if *old_window == window => {
                    if !geometry_changed {
                        continue;
                    }
                    pane_size(before, window, *pane, border)
                }
                Some((old_window, _, _)) => {
                    moved.push((*pane, *old_window));
                    pane_size(before, *old_window, *pane, border)
                }
                None if previous.is_some() => {
                    created.push(*pane);
                    Some(state.extent)
                }
                None => continue,
            };
            if let (Some(old), Some(size)) = (old_size, pane_size(after, window, *pane, border))
                && old != size
            {
                events.push(pane_resized_event(*pane, window, size, old, after));
            }
        }
        for pane in created {
            events.push(pane_created_event(pane, window, after));
        }
        for (pane, old_window) in moved {
            events.push(pane_moved_event(pane, old_window, window, before, after));
        }
        let Some(previous) = previous else {
            continue;
        };
        if previous.name != state.name {
            let mut event = window_event(
                "window-renamed",
                window,
                state.session,
                state.name,
                state.active_pane,
                after,
            );
            event
                .variables
                .insert("hook_old_name".to_owned(), previous.name.to_owned());
            event
                .variables
                .insert("hook_new_name".to_owned(), state.name.to_owned());
            events.push(event);
        }
        if previous.active_pane != state.active_pane {
            events.push(window_event(
                "window-pane-changed",
                window,
                state.session,
                state.name,
                state.active_pane,
                after,
            ));
        }
        if previous.zoomed_pane != state.zoomed_pane {
            if previous.zoomed_pane.is_some() {
                events.push(window_payload_event("window-unzoomed", window, after));
            }
            if state.zoomed_pane.is_some() {
                events.push(window_payload_event("window-zoomed", window, after));
            }
        }
        if previous.layout != state.layout || previous.zoomed_pane != state.zoomed_pane {
            events.push(window_event(
                "window-layout-changed",
                window,
                state.session,
                state.name,
                state.active_pane,
                after,
            ));
            if previous.extent != state.extent {
                events.push(window_event(
                    "window-resized",
                    window,
                    state.session,
                    state.name,
                    state.active_pane,
                    after,
                ));
            }
        }
    }
    for (pane, (window, session, title)) in &after_panes {
        if before_panes
            .get(pane)
            .is_some_and(|(_, _, previous)| previous != title)
        {
            let mut event = pane_event("pane-title-changed", *pane, *session, *window, after);
            event
                .variables
                .insert("hook_new_title".to_owned(), title.clone());
            events.push(event);
        }
    }
    let removed_links = before_links
        .difference(&after_links)
        .filter_map(|(session, window)| {
            let (session_name, _) = before.session(*session)?;
            let window_state = before.window(*window)?;
            Some(winlink_event(
                "window-unlinked",
                *session,
                session_name,
                *window,
                window_state.name,
                before,
            ))
        })
        .collect::<Vec<_>>();
    let closed_windows = before
        .listed_windows()
        .into_iter()
        .filter(|window| after.window(*window).is_none())
        .map(|window| window_payload_event("window-closed", window, before))
        .collect::<Vec<_>>();
    let closed_sessions = before
        .listed_sessions()
        .into_iter()
        .filter(|session| after.session(*session).is_none())
        .filter_map(|session| {
            let (name, _) = before.session(session)?;
            Some(session_event("session-closed", session, name, before))
        })
        .collect::<Vec<_>>();
    if command == "kill-session" {
        events.extend(closed_sessions);
        events.extend(removed_links);
        events.extend(closed_windows);
    } else {
        events.extend(removed_links);
        events.extend(closed_windows);
        events.extend(closed_sessions);
    }
    events
}

pub(super) fn pane_mode_hook_events(
    engine: &MuxEngine,
    before: &impl HookView,
    before_modes: &BTreeSet<PaneId>,
    after_modes: &BTreeSet<PaneId>,
) -> Vec<PendingHookEvent> {
    before_modes
        .symmetric_difference(after_modes)
        .flat_map(|pane| {
            let entered = after_modes.contains(pane);
            ["pane-mode-entered", "pane-mode-exited", "pane-mode-changed"]
                .into_iter()
                .filter(|name| match *name {
                    "pane-mode-entered" => entered,
                    "pane-mode-exited" => !entered,
                    _ => true,
                })
                .filter_map(|name| {
                    PendingHookEvent::live_pane(name, *pane, engine).or_else(|| {
                        before.pane(*pane).map(|state| {
                            pane_event(name, *pane, state.session, state.window, before)
                        })
                    })
                })
                .map(|mut event| {
                    event.add_copy_mode_payload(entered);
                    event
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn copy_mode_exit_hook_events(pane: PaneId, engine: &MuxEngine) -> Vec<PendingHookEvent> {
    ["pane-mode-exited", "pane-mode-changed"]
        .into_iter()
        .filter_map(|name| PendingHookEvent::live_pane(name, pane, engine))
        .map(|mut event| {
            event.add_copy_mode_payload(false);
            event
        })
        .collect()
}

impl PendingHookEvent {
    fn add_copy_mode_payload(&mut self, entered: bool) {
        let key = if entered {
            "hook_current_mode"
        } else {
            "hook_previous_mode"
        };
        self.variables
            .insert(key.to_owned(), "copy-mode".to_owned());
        self.variables
            .insert("hook_mode_entered".to_owned(), u8::from(entered).to_string());
    }

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
        Self::open_within(inner, None)
    }

    pub(super) fn open_within(inner: &mut ServerState, window: Option<&ChangeWindow>) -> Self {
        Self {
            probe: capture_client_focus_probe(inner),
            full: cfg!(debug_assertions).then(|| capture_pane_focus_probe(inner)),
            window: Some(
                window
                    .cloned()
                    .unwrap_or_else(|| inner.engine.state.open_change_window()),
            ),
        }
    }

    fn materialize(&mut self, inner: &ServerState) {
        let Some(window) = self.window.take() else {
            return;
        };
        let state = &inner.engine.state;
        let changes = state.changes_since(&window);
        let mut window_active_panes = journal_active_panes(&changes);
        window_active_panes.extend(
            state
                .windows
                .iter()
                .filter(|(window, _)| !changes.windows.contains_key(*window))
                .map(|(window, state)| (*window, state.active_pane)),
        );
        let mut session_windows = journal_active_windows(&changes);
        session_windows.extend(
            state
                .sessions
                .iter()
                .filter(|(session, _)| !changes.sessions.contains_key(*session))
                .map(|(session, state)| (*session, state.active_window)),
        );
        self.probe.window_active_panes = window_active_panes;
        self.probe.session_windows = session_windows;
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

pub(super) struct InputFocusScope {
    shared: Arc<Shared>,
    depth: usize,
    active: bool,
}

impl InputFocusScope {
    pub(super) fn open(shared: &Arc<Shared>, inner: &mut ServerState) -> Self {
        let scope = FocusProbeScope::open(inner);
        let mut item = shared.command_item.as_ref().expect("command item").lock();
        let depth = item.input_focus.len();
        item.input_focus.push(scope);
        Self {
            shared: Arc::clone(shared),
            depth,
            active: true,
        }
    }

    pub(super) fn close(mut self, inner: &ServerState) -> Option<PaneFocusProbe> {
        self.active = false;
        let scope = self
            .shared
            .command_item
            .as_ref()
            .expect("command item")
            .lock()
            .input_focus
            .pop();
        scope.map(|scope| scope.close(inner))
    }
}

impl Drop for InputFocusScope {
    fn drop(&mut self) {
        if self.active {
            self.shared
                .command_item
                .as_ref()
                .expect("command item")
                .lock()
                .input_focus
                .truncate(self.depth);
        }
    }
}

pub(super) fn release_input_change_window(shared: &Shared) {
    let Some(item) = &shared.command_item else {
        return;
    };
    if !item
        .lock()
        .input_focus
        .iter()
        .any(|scope| scope.window.is_some())
    {
        return;
    }
    let inner = shared.inner.lock();
    for scope in &mut item.lock().input_focus {
        scope.materialize(&inner);
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

type ActiveBefore = (
    BTreeMap<SessionId, WindowId>,
    BTreeMap<WindowId, PaneId>,
    BTreeSet<PaneId>,
);

pub(super) fn assert_same_active_changes(
    state: &MuxState,
    captured: &ActiveBefore,
    journal: &ActiveBefore,
) {
    let moved = |active: &ActiveBefore| {
        let windows = active
            .0
            .iter()
            .filter(|(session, window)| {
                state
                    .sessions
                    .get(session)
                    .is_some_and(|current| current.active_window != **window)
            })
            .map(|(session, _)| *session)
            .collect::<BTreeSet<_>>();
        let panes = active
            .1
            .iter()
            .filter(|(window, pane)| {
                state
                    .windows
                    .get(window)
                    .is_some_and(|current| current.active_pane != **pane)
            })
            .map(|(window, _)| *window)
            .collect::<BTreeSet<_>>();
        let bells = active
            .2
            .iter()
            .filter(|pane| state.pane(**pane).is_none_or(|current| !current.bell))
            .copied()
            .collect::<BTreeSet<_>>();
        (windows, panes, bells)
    };
    assert!(
        moved(captured) == moved(journal),
        "the change journal missed an active window, active pane or bell change"
    );
}
