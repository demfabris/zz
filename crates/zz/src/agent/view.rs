use std::{
    collections::BTreeSet,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Datelike as _, Local, NaiveDate, Timelike as _};
use gpui::{
    Anchor, AnyElement, Context, ElementId, Entity, FocusHandle, Focusable, Image, IntoElement,
    KeyDownEvent, ListAlignment, ListState, MouseButton, MouseDownEvent, Render, ScrollStrategy,
    SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*, px, relative,
    uniform_list,
};
use zz_protocol::{AgentDescriptor, AgentProvider, CommandInvocation, PaneId};
#[cfg(all(test, not(target_os = "macos")))]
use zz_ui::agent::DisclosureKind;
use zz_ui::agent::{
    AGENT_CHROME_CONTROL_HEIGHT, AGENT_CONTENT_MAX_WIDTH, AgentEntry, AgentTimeline,
    AgentTimelineStore, AgentToolEntry, AgentToolKind, AgentToolPayload, AgentToolStatus,
    COMPOSER_ATTACHMENT, FoldedTimelineRows, MarkdownSlot, TimelineRow, TimelineStick,
    agent_attachment_thumbnail, agent_jump_to_bottom_button, agent_pane_header,
    append_timeline_row, fold_timeline_rows, timeline_group_kind,
};
use zz_ui::command::palette_shortcut_hint;
use zz_ui::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{IndentInline, Input, InputEvent, InputState, MoveDown, MoveUp},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::Scrollbar,
    spinner::Spinner,
    tag::Tag,
    v_flex,
};

use crate::{
    agent::attachment as agent_attachment,
    agent::controller::{
        AgentCommand, AgentCommandKind, AgentConfigCategory, AgentConfigOption,
        AgentConnectionState, AgentController, AgentControllerEvent, AgentPaneState,
        AgentPermissionKind, AgentPermissionRequest, AgentSessionCapabilities,
        AgentSessionHistoryState, AgentSessionSummary, AgentThreadEntry, AgentToolKindModel,
        AgentToolStatusModel, ToolPayload,
    },
    config::pane_content_radii,
    file_picker::{FilePickerEvent, FilePickerMode, FilePickerView, directory_picker_root},
    mux::client::MuxClient,
    window::corners::{WindowCorners, round_div_radii},
};

const AGENT_KEY_CONTEXT: &str = "Agent";
const COMPLETION_ROW_HEIGHT: f32 = 52.0;
const MAX_VISIBLE_COMPLETION_ROWS: u8 = 6;
const HISTORY_ROW_HEIGHT: f32 = 52.0;
const COMPOSER_MIN_HEIGHT: f32 = 86.0;
const COMPOSER_MAX_WIDTH: f32 = AGENT_CONTENT_MAX_WIDTH + 2.0;
const COMPOSER_OCCLUSION_HEIGHT: f32 = COMPOSER_MIN_HEIGHT / 2.0;
const TIMELINE_COMPOSER_CLEARANCE: f32 = (COMPOSER_MIN_HEIGHT + 12.0) * 1.5;
const STICKY_ROW_HEIGHT: f32 = 32.0;
const CHROME_BUTTON_HEIGHT: f32 = AGENT_CHROME_CONTROL_HEIGHT;

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandCompletion {
    command: AgentCommand,
    replacement: Range<usize>,
    sigil: char,
}

enum TimelineStoreUpdate {
    None,
    Clear,
    Synchronize(Vec<AgentEntry>),
}

enum TimelineModelUpdate {
    None,
    Rebuild,
    Incremental {
        store_entries: Vec<AgentEntry>,
        remeasure_rows: Vec<usize>,
        splice_start: usize,
        added_rows: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StickyAgentRow {
    Subagent {
        id: u64,
        status: AgentToolStatus,
        label: SharedString,
    },
    Notification {
        id: u64,
        status: SharedString,
        label: SharedString,
    },
}

#[derive(Clone, Default)]
struct TimelineModel {
    rows: Arc<Vec<TimelineRow>>,
    entry_ids: Vec<u64>,
    entry_revisions: Vec<u64>,
    entry_to_row: Vec<usize>,
}

impl TimelineModel {
    fn new(entries: &[AgentThreadEntry], revisions: &[u64]) -> Self {
        debug_assert_eq!(entries.len(), revisions.len());
        let ui_entries = ui_entries(entries);
        let FoldedTimelineRows { rows, entry_to_row } = fold_timeline_rows(&ui_entries);
        Self {
            rows,
            entry_ids: entries.iter().map(AgentThreadEntry::id).collect(),
            entry_revisions: revisions.to_vec(),
            entry_to_row,
        }
    }

    fn clear(&mut self) {
        self.rows = Arc::new(Vec::new());
        self.entry_ids.clear();
        self.entry_revisions.clear();
        self.entry_to_row.clear();
    }

    fn rebuild(&mut self, entries: &[AgentThreadEntry], revisions: &[u64]) {
        *self = Self::new(entries, revisions);
    }

    fn synchronize(
        &mut self,
        entries: &[AgentThreadEntry],
        revisions: &[u64],
    ) -> TimelineModelUpdate {
        debug_assert_eq!(entries.len(), revisions.len());
        if entries.len() != revisions.len() {
            return TimelineModelUpdate::None;
        }

        let old_entry_count = self.entry_ids.len();
        let append_only = old_entry_count <= entries.len()
            && self
                .entry_ids
                .iter()
                .zip(entries)
                .all(|(id, entry)| *id == entry.id());
        if !append_only {
            self.rebuild(entries, revisions);
            return TimelineModelUpdate::Rebuild;
        }

        let changed_existing = (0..old_entry_count)
            .filter(|index| self.entry_revisions[*index] != revisions[*index])
            .collect::<Vec<_>>();
        if changed_existing.is_empty() && old_entry_count == entries.len() {
            return TimelineModelUpdate::None;
        }

        let mut replacements = Vec::with_capacity(changed_existing.len());
        for &entry_index in &changed_existing {
            let id = self.entry_ids[entry_index];
            let Some(&row_index) = self.entry_to_row.get(entry_index) else {
                self.rebuild(entries, revisions);
                return TimelineModelUpdate::Rebuild;
            };
            let next = ui_entry(&entries[entry_index]);
            let Some(previous) = self.rows.get(row_index).and_then(|row| row.entry(id)) else {
                self.rebuild(entries, revisions);
                return TimelineModelUpdate::Rebuild;
            };
            if !replacement_preserves_folding(previous, &next) {
                self.rebuild(entries, revisions);
                return TimelineModelUpdate::Rebuild;
            }
            replacements.push((entry_index, row_index, id, next));
        }

        let old_row_count = self.rows.len();
        let mut store_entries = Vec::with_capacity(replacements.len());
        let mut remeasure_rows = replacements
            .iter()
            .map(|(_, row_index, _, _)| *row_index)
            .collect::<Vec<_>>();
        let rows = Arc::make_mut(&mut self.rows);
        for (entry_index, row_index, id, entry) in replacements {
            store_entries.push(entry.clone());
            debug_assert!(rows[row_index].replace_entry(id, entry));
            self.entry_revisions[entry_index] = revisions[entry_index];
        }

        for entry in entries.iter().skip(old_entry_count) {
            let entry = ui_entry(entry);
            let (row_index, row_added) = append_timeline_row(rows, entry);
            self.entry_to_row.push(row_index);
            if !row_added && row_index < old_row_count {
                remeasure_rows.push(row_index);
            }
        }
        self.entry_ids
            .extend(entries[old_entry_count..].iter().map(AgentThreadEntry::id));
        self.entry_revisions
            .extend_from_slice(&revisions[old_entry_count..]);

        remeasure_rows.sort_unstable();
        remeasure_rows.dedup();
        TimelineModelUpdate::Incremental {
            store_entries,
            remeasure_rows,
            splice_start: old_row_count,
            added_rows: rows.len() - old_row_count,
        }
    }
}

fn replacement_preserves_folding(previous: &AgentEntry, next: &AgentEntry) -> bool {
    timeline_group_kind(previous) == timeline_group_kind(next)
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent visibility, completion, and history UI states"
)]
pub(crate) struct AgentView {
    pane: PaneId,
    mux: Entity<MuxClient>,
    controller: Entity<AgentController>,
    input: Entity<InputState>,
    history_input: Entity<InputState>,
    visible: bool,
    pane_state: AgentPaneState,
    timeline: TimelineModel,
    timeline_store: Entity<AgentTimelineStore>,
    timeline_next_revision: u64,
    timeline_scroll: ListState,
    stick: TimelineStick,
    completion_scroll: UniformListScrollHandle,
    submission_error: Option<Arc<str>>,
    dismissed_notifications: BTreeSet<u64>,
    attachments: Vec<Arc<Image>>,
    completions: Arc<[CommandCompletion]>,
    completion_selected: Option<usize>,
    completion_dismissed: bool,
    last_input: String,
    last_cursor: usize,
    history_open: bool,
    history_all_projects: bool,
    history_results: Arc<[usize]>,
    history_selected: Option<usize>,
    history_scroll: UniformListScrollHandle,
    history_delete_confirmation: Option<Arc<str>>,
    last_history_query: String,
    directory_picker: Option<Entity<FilePickerView>>,
    window_corners: WindowCorners,
    _subscriptions: Vec<Subscription>,
}

impl AgentView {
    pub(crate) fn new(
        pane: PaneId,
        descriptor: &AgentDescriptor,
        controller: Entity<AgentController>,
        mux: Entity<MuxClient>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 8)
                .submit_on_enter(true)
                .placeholder("Ask the agent…")
                .context_menu(true)
        });
        let history_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search by title or project…"));
        let timeline_store = cx.new(|_| AgentTimelineStore::default());
        let timeline_store_observer = cx.observe(&timeline_store, |_, _, cx| cx.notify());
        let input_observer = cx.observe(&input, |view, input, cx| {
            view.synchronize_input(&input, cx);
        });
        let input_subscription = cx.subscribe_in(
            &input,
            window,
            |view, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } => view.enter(window, cx),
                InputEvent::Change if view.submission_error.take().is_some() => cx.notify(),
                InputEvent::PasteImages(images) => view.attach_images(images, cx),
                _ => {}
            },
        );
        let history_input_subscription = cx.subscribe_in(
            &history_input,
            window,
            |view, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => view.synchronize_history_input(input, cx),
                InputEvent::PressEnter { .. } => {
                    view.open_selected_history(window, cx);
                }
                _ => {}
            },
        );
        controller.update(cx, |controller, cx| {
            controller.ensure_pane(pane, descriptor, cx);
        });
        let (pane_state, timeline, timeline_next_revision) = {
            let controller = controller.read(cx);
            let pane_state = controller
                .pane_state(pane)
                .unwrap_or_else(disconnected_pane_state);
            let (entries, revisions, next_revision) =
                controller.pane_entries(pane).unwrap_or((&[], &[], 0));
            (
                pane_state,
                TimelineModel::new(entries, revisions),
                next_revision,
            )
        };
        let timeline_scroll = ListState::new(0, ListAlignment::Top, px(1_200.0));
        timeline_scroll.splice(0..0, timeline.rows.len());
        let stick = TimelineStick::new(&timeline_scroll, cx.reduce_motion());
        let scroll_view = cx.weak_entity();
        timeline_scroll.set_scroll_handler(move |_, _, cx| {
            // The list still holds its own borrow across this call, so reading
            // the scroll state back here panics; run after the effect cycle.
            let view = scroll_view.clone();
            cx.defer(move |cx| {
                view.update(cx, |view: &mut Self, cx| view.on_timeline_scroll(cx))
                    .ok();
            });
        });
        let controller_observer = cx.observe(&controller, |view, controller, cx| {
            if view.visible && view.synchronize_controller(&controller, cx) {
                cx.notify();
            }
        });
        let controller_subscription = cx.subscribe(
            &controller,
            |view, controller, event: &AgentControllerEvent, cx| {
                if view.visible
                    && matches!(event, AgentControllerEvent::Pane { pane } if *pane == view.pane)
                    && view.synchronize_controller(&controller, cx)
                {
                    cx.notify();
                }
            },
        );
        Self {
            pane,
            mux,
            controller,
            input,
            history_input,
            visible: false,
            pane_state,
            timeline,
            timeline_store,
            timeline_next_revision,
            timeline_scroll,
            stick,
            completion_scroll: UniformListScrollHandle::new(),
            submission_error: None,
            dismissed_notifications: BTreeSet::new(),
            attachments: Vec::new(),
            completions: Arc::from([]),
            completion_selected: None,
            completion_dismissed: false,
            last_input: String::new(),
            last_cursor: 0,
            history_open: false,
            history_all_projects: false,
            history_results: Arc::from([]),
            history_selected: None,
            history_scroll: UniformListScrollHandle::new(),
            history_delete_confirmation: None,
            last_history_query: String::new(),
            directory_picker: None,
            window_corners: WindowCorners::NONE,
            _subscriptions: vec![
                input_observer,
                timeline_store_observer,
                input_subscription,
                history_input_subscription,
                controller_observer,
                controller_subscription,
            ],
        }
    }

    pub(crate) fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    pub(crate) fn set_window_corners(&mut self, corners: WindowCorners, cx: &mut Context<Self>) {
        if self.window_corners != corners {
            self.window_corners = corners;
            cx.notify();
        }
    }

    pub(crate) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible {
            let controller = self.controller.clone();
            self.synchronize_controller(&controller, cx);
        }
    }

    fn on_mouse_down(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.mux.read(cx).execute(CommandInvocation::new(
            "select-pane",
            ["-t", &self.pane.to_string()],
        ));
    }

    fn synchronize_input(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let input = input.read(cx);
        let value = input.value().to_string();
        let cursor = input.cursor();
        if value == self.last_input && cursor == self.last_cursor {
            return;
        }
        self.last_input.clone_from(&value);
        self.last_cursor = cursor;
        self.completion_dismissed = false;
        let provider = self.pane_state.provider;
        let commands = self.pane_state.available_commands.clone();
        self.recompute_completions(provider, &commands);
        cx.notify();
    }

    fn synchronize_history_input(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let query = input.read(cx).value().to_string();
        if query == self.last_history_query {
            return;
        }
        self.last_history_query.clone_from(&query);
        self.recompute_history_results(&query);
        self.history_delete_confirmation = None;
        self.submission_error = None;
        cx.notify();
    }

    fn synchronize_controller(
        &mut self,
        controller: &Entity<AgentController>,
        cx: &mut Context<Self>,
    ) -> bool {
        let reduce_motion = cx.reduce_motion();
        let (changed, store_update) = {
            let controller = controller.read(cx);
            self.synchronize_controller_state(controller, reduce_motion)
        };
        let store_changed = self.update_timeline_store(store_update, cx);
        let cwd = self.pane_state.cwd.clone();
        self.timeline_store
            .update(cx, |store, cx| store.set_cwd(Some(cwd), cx));
        changed || store_changed
    }

    fn update_timeline_store(
        &mut self,
        store_update: TimelineStoreUpdate,
        cx: &mut Context<Self>,
    ) -> bool {
        match store_update {
            TimelineStoreUpdate::None => false,
            TimelineStoreUpdate::Clear => self.timeline_store.update(cx, AgentTimelineStore::clear),
            TimelineStoreUpdate::Synchronize(entries) => {
                for entry in &entries {
                    synchronize_entry_store(&self.timeline_store, entry, cx);
                }
                false
            }
        }
    }

    fn synchronize_controller_state(
        &mut self,
        controller: &AgentController,
        reduce_motion: bool,
    ) -> (bool, TimelineStoreUpdate) {
        let next_state = controller
            .pane_state(self.pane)
            .unwrap_or_else(disconnected_pane_state);
        let state_changed = self.pane_state != next_state;
        let provider_changed = self.pane_state.provider != next_state.provider;
        let conversation_changed =
            provider_changed || self.pane_state.session_id != next_state.session_id;
        let history_changed = provider_changed
            || self.pane_state.session_history != next_state.session_history
            || self.pane_state.session_id != next_state.session_id;
        let selected_history_session_id = (history_changed && !provider_changed)
            .then(|| self.selected_history_session_id())
            .flatten();
        let commands_changed = provider_changed
            || self.pane_state.available_commands.as_ref()
                != next_state.available_commands.as_ref();
        if conversation_changed {
            self.dismissed_notifications.clear();
        }
        if state_changed {
            self.pane_state = next_state;
        }
        if history_changed {
            let query = self.last_history_query.clone();
            self.recompute_history_results_preserving(
                &query,
                selected_history_session_id.as_deref(),
            );
        }
        if commands_changed {
            let provider = self.pane_state.provider;
            let commands = self.pane_state.available_commands.clone();
            self.recompute_completions(provider, &commands);
        }

        let Some((entries, revisions, next_revision)) = controller.pane_entries(self.pane) else {
            if !self.timeline.rows.is_empty() {
                self.timeline.clear();
                self.timeline_next_revision = 0;
                self.dismissed_notifications.clear();
                self.timeline_scroll.reset(0);
                self.stick.engage_now(&self.timeline_scroll, reduce_motion);
                return (true, TimelineStoreUpdate::Clear);
            }
            return (state_changed, TimelineStoreUpdate::Clear);
        };
        if self.timeline.entry_ids.len() == entries.len()
            && self.timeline_next_revision == next_revision
        {
            return (state_changed, TimelineStoreUpdate::None);
        }
        self.timeline_next_revision = next_revision;
        let (timeline_changed, store_update) =
            self.synchronize_timeline(entries, revisions, reduce_motion);
        (timeline_changed || state_changed, store_update)
    }

    fn recompute_history_results(&mut self, query: &str) {
        self.recompute_history_results_preserving(query, None);
    }

    fn recompute_history_results_preserving(
        &mut self,
        query: &str,
        preferred_session_id: Option<&str>,
    ) {
        let results = ranked_session_indices(&self.pane_state.session_history.sessions, query);
        let retained_selection = preferred_session_id.and_then(|session_id| {
            history_result_index_for_session(
                &self.pane_state.session_history.sessions,
                &results,
                session_id,
            )
        });
        let selected = retained_selection.or_else(|| (!results.is_empty()).then_some(0));
        self.history_results = results.into();
        self.history_selected = selected;
        if retained_selection.is_none()
            && let Some(selected) = selected
        {
            self.history_scroll
                .scroll_to_item(selected, ScrollStrategy::Top);
        }
    }

    fn selected_history_session_id(&self) -> Option<String> {
        let result_index = self.history_selected?;
        let session_index = *self.history_results.get(result_index)?;
        self.pane_state
            .session_history
            .sessions
            .get(session_index)
            .map(|session| session.session_id.clone())
    }

    fn synchronize_timeline(
        &mut self,
        entries: &[AgentThreadEntry],
        revisions: &[u64],
        reduce_motion: bool,
    ) -> (bool, TimelineStoreUpdate) {
        match self.timeline.synchronize(entries, revisions) {
            TimelineModelUpdate::None => (false, TimelineStoreUpdate::None),
            TimelineModelUpdate::Rebuild => {
                self.timeline_scroll.reset(self.timeline.rows.len());
                self.stick.engage_now(&self.timeline_scroll, reduce_motion);
                (true, TimelineStoreUpdate::Clear)
            }
            TimelineModelUpdate::Incremental {
                store_entries,
                remeasure_rows,
                splice_start,
                added_rows,
            } => {
                for row_index in remeasure_rows {
                    self.timeline_scroll
                        .remeasure_items(row_index..row_index + 1);
                }
                if added_rows > 0 {
                    self.timeline_scroll
                        .splice(splice_start..splice_start, added_rows);
                }
                // The grown rows are still unmeasured, so the distance to the
                // end reads as zero until the next layout: wake the driver
                // outright rather than waiting for a measurement to show up.
                if self.stick.is_pinned() {
                    self.stick.wake();
                }
                let store_update = if store_entries.is_empty() {
                    TimelineStoreUpdate::None
                } else {
                    TimelineStoreUpdate::Synchronize(store_entries)
                };
                (true, store_update)
            }
        }
    }

    fn recompute_completions(&mut self, provider: AgentProvider, commands: &[AgentCommand]) {
        let Some(query) = completion_query(
            &self.last_input,
            self.last_cursor,
            completion_sigil(provider),
        ) else {
            self.completions = Arc::from([]);
            self.completion_selected = None;
            return;
        };
        if self.completion_dismissed {
            self.completions = Arc::from([]);
            self.completion_selected = None;
            return;
        }
        self.completions = ranked_completions(commands, &query).into();
        self.completion_selected = (!self.completions.is_empty()).then_some(
            self.completion_selected
                .unwrap_or_default()
                .min(self.completions.len().saturating_sub(1)),
        );
        if let Some(selected) = self.completion_selected {
            self.completion_scroll
                .scroll_to_item(selected, ScrollStrategy::Nearest);
        }
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_open {
            self.open_selected_history(window, cx);
            return;
        }
        if self.completions.is_empty() {
            self.submit(window, cx);
        } else {
            self.accept_selected_completion(window, cx);
        }
    }

    fn navigate_completion(&mut self, direction: isize, cx: &mut Context<Self>) {
        if self.completions.is_empty() {
            return;
        }
        let count = self.completions.len();
        let current = self.completion_selected.unwrap_or_default();
        let selected = if direction < 0 {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        };
        self.completion_selected = Some(selected);
        self.completion_scroll
            .scroll_to_item(selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pane_state.session_capabilities.list {
            self.submission_error = Some(Arc::from("this agent does not support session history"));
            cx.notify();
            return;
        }
        if self.pane_state.connection.has_active_turn() {
            self.submission_error = Some(Arc::from(
                "finish or cancel the current turn before opening session history",
            ));
            cx.notify();
            return;
        }
        self.history_open = true;
        self.history_all_projects = false;
        self.history_delete_confirmation = None;
        self.last_history_query.clear();
        self.completions = Arc::from([]);
        self.completion_selected = None;
        self.history_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.recompute_history_results("");
        let result = self.controller.update(cx, |controller, cx| {
            controller.list_sessions(self.pane, false, false, cx)
        });
        self.submission_error = result.err();
        self.history_input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    fn close_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_open = false;
        self.history_delete_confirmation = None;
        self.focus(cx).focus(window, cx);
        cx.notify();
    }

    fn refresh_history(&mut self, cx: &mut Context<Self>) {
        let result = self.controller.update(cx, |controller, cx| {
            controller.list_sessions(self.pane, self.history_all_projects, false, cx)
        });
        self.submission_error = result.err();
        cx.notify();
    }

    fn toggle_history_scope(&mut self, cx: &mut Context<Self>) {
        self.history_all_projects = !self.history_all_projects;
        self.history_delete_confirmation = None;
        self.refresh_history(cx);
    }

    fn load_more_history(&mut self, cx: &mut Context<Self>) {
        let result = self.controller.update(cx, |controller, cx| {
            controller.list_sessions(self.pane, self.history_all_projects, true, cx)
        });
        self.submission_error = result.err();
        cx.notify();
    }

    fn navigate_history(&mut self, direction: isize, cx: &mut Context<Self>) {
        if self.history_results.is_empty() {
            return;
        }
        let count = self.history_results.len();
        let current = self.history_selected.unwrap_or_default();
        let selected = if direction < 0 {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        };
        self.history_selected = Some(selected);
        self.history_scroll
            .scroll_to_item(selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn open_history_result(
        &mut self,
        result_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session_index) = self.history_results.get(result_index).copied() else {
            return;
        };
        let Some(session) = self
            .pane_state
            .session_history
            .sessions
            .get(session_index)
            .cloned()
        else {
            return;
        };
        let result = self.controller.update(cx, |controller, cx| {
            controller.switch_session(self.pane, session, cx)
        });
        match result {
            Ok(()) => {
                self.history_open = false;
                self.history_delete_confirmation = None;
                self.submission_error = None;
                self.focus(cx).focus(window, cx);
            }
            Err(error) => self.submission_error = Some(error),
        }
        cx.notify();
    }

    fn open_selected_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.history_open || self.history_delete_confirmation.is_some() {
            return;
        }
        if let Some(index) = self.history_selected {
            self.open_history_result(index, window, cx);
        }
    }

    fn start_new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .controller
            .update(cx, |controller, cx| controller.new_session(self.pane, cx));
        match result {
            Ok(()) => {
                self.history_open = false;
                self.history_delete_confirmation = None;
                self.submission_error = None;
                self.focus(cx).focus(window, cx);
            }
            Err(error) => self.submission_error = Some(error),
        }
        cx.notify();
    }

    fn confirm_delete_history(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let result = self.controller.update(cx, |controller, cx| {
            controller.delete_session(self.pane, session_id, cx)
        });
        self.submission_error = result.err();
        self.history_delete_confirmation = None;
        cx.notify();
    }

    fn accept_completion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(completion) = self.completions.get(index).cloned() else {
            return;
        };
        let name = bare_command_name(&completion.command.name);
        self.completion_dismissed = true;
        self.input.update(cx, |input, cx| {
            input.set_selected_range(completion.replacement, cx);
            input.replace(format!("{}{name} ", completion.sigil), window, cx);
        });
        self.completions = Arc::from([]);
        self.completion_selected = None;
        self.focus(cx).focus(window, cx);
        cx.notify();
    }

    fn accept_selected_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .completion_selected
            .or((!self.completions.is_empty()).then_some(0))
        {
            self.accept_completion(index, window, cx);
        }
    }

    fn complete(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_open {
            cx.stop_propagation();
            return;
        }
        if self.completions.is_empty() {
            return;
        }
        self.accept_selected_completion(window, cx);
        cx.stop_propagation();
    }

    fn move_completion_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.history_open {
            self.navigate_history(-1, cx);
            cx.stop_propagation();
        } else if !self.completions.is_empty() {
            self.navigate_completion(-1, cx);
            cx.stop_propagation();
        }
    }

    fn move_completion_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.history_open {
            self.navigate_history(1, cx);
            cx.stop_propagation();
        } else if !self.completions.is_empty() {
            self.navigate_completion(1, cx);
            cx.stop_propagation();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_open && event.keystroke.key.as_str() == "escape" {
            if self.history_delete_confirmation.take().is_some() {
                cx.notify();
            } else {
                self.close_history(window, cx);
            }
            cx.stop_propagation();
            return;
        }
        if self.completions.is_empty() {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            "up" if !modifiers.platform && !modifiers.alt => {
                self.navigate_completion(-1, cx);
            }
            "down" if !modifiers.platform && !modifiers.alt => {
                self.navigate_completion(1, cx);
            }
            "escape" => {
                self.completion_dismissed = true;
                self.completions = Arc::from([]);
                self.completion_selected = None;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn attach_images(&mut self, images: &[Arc<Image>], cx: &mut Context<Self>) {
        if !self.pane_state.session_capabilities.images {
            self.submission_error = Some(Arc::from("this agent does not accept images"));
            cx.notify();
            return;
        }
        for image in images {
            match agent_attachment::normalize(image) {
                Ok(normalized) => {
                    self.attachments.push(normalized);
                    self.submission_error = None;
                }
                Err(error) => self.submission_error = Some(Arc::from(error.as_ref())),
            }
        }
        cx.notify();
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            cx.notify();
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().to_string();
        if value.trim().is_empty() && self.attachments.is_empty() {
            return;
        }
        let attachments = self.attachments.clone();
        let result = self.controller.update(cx, |controller, cx| {
            controller.prompt(self.pane, &value, attachments, cx)
        });
        match result {
            Ok(()) => {
                self.input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
                self.attachments.clear();
                self.submission_error = None;
                self.completions = Arc::from([]);
                self.completion_selected = None;
                self.stick.engage(&self.timeline_scroll, cx.reduce_motion());
            }
            Err(error) => self.submission_error = Some(error),
        }
        cx.notify();
    }

    fn dismiss_notification(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.dismissed_notifications.insert(id) {
            cx.notify();
        }
    }

    fn render_attachments(&self, view: &Entity<Self>, cx: &gpui::App) -> Option<impl IntoElement> {
        if self.attachments.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .w_full()
                .flex_wrap()
                .gap_2()
                .px_3()
                .pt_2()
                .children(self.attachments.iter().enumerate().map(|(index, image)| {
                    let remove_view = view.clone();
                    div()
                        .relative()
                        .child(agent_attachment_thumbnail(
                            ("agent-composer-attachment", index),
                            Arc::clone(image),
                            COMPOSER_ATTACHMENT,
                            cx,
                        ))
                        .child(
                            div().absolute().top(px(-6.0)).right(px(-6.0)).child(
                                Button::new(format!(
                                    "agent-attachment-remove-{}-{index}",
                                    self.pane.0
                                ))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::Close))
                                .tooltip("Remove this image")
                                .on_click(move |_, _, cx| {
                                    remove_view.update(cx, |view, cx| {
                                        view.remove_attachment(index, cx);
                                    });
                                    cx.stop_propagation();
                                }),
                            ),
                        )
                })),
        )
    }

    fn render_empty_state(state: &AgentPaneState, cx: &gpui::App) -> impl IntoElement {
        let agent = state
            .agent_name
            .as_deref()
            .unwrap_or(state.provider.label());
        let message: SharedString = match state.connection {
            AgentConnectionState::Starting => format!("Starting {agent}…").into(),
            AgentConnectionState::Restoring => "Restoring the previous session…".into(),
            AgentConnectionState::Ready => "Ask the agent to work in this workspace.".into(),
            AgentConnectionState::Running => format!("Waiting for {agent}’s first update…").into(),
            AgentConnectionState::Cancelling => "Cancelling the current turn…".into(),
            AgentConnectionState::Failed => "The agent could not start this session.".into(),
            AgentConnectionState::Disconnected => "The ACP agent is offline.".into(),
        };
        let busy = matches!(
            state.connection,
            AgentConnectionState::Starting
                | AgentConnectionState::Restoring
                | AgentConnectionState::Running
                | AgentConnectionState::Cancelling
        );
        v_flex()
            .w_full()
            .py(px(48.0))
            .items_center()
            .gap_2()
            .text_size(zz_ui::rems_from_px(12.0))
            .text_color(cx.theme().foreground.muted())
            .when(busy, |this| this.child(Spinner::new().small()))
            .child(message)
    }

    fn render_permission(
        &self,
        permission: &AgentPermissionRequest,
        cx: &gpui::App,
    ) -> impl IntoElement {
        let actions = permission
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let controller = self.controller.clone();
                let pane = self.pane;
                let request_id = permission.request_id;
                let option_id = option.id.clone();
                let button = Button::new(format!(
                    "agent-permission-{}-{request_id}-{index}",
                    self.pane.0
                ))
                .small()
                .label(option.name.clone())
                .on_click(move |_, _, cx| {
                    controller.update(cx, |controller, cx| {
                        controller.respond_permission(
                            pane,
                            request_id,
                            Some(option_id.clone()),
                            cx,
                        );
                    });
                });
                match option.kind {
                    AgentPermissionKind::AllowOnce | AgentPermissionKind::AllowAlways => {
                        button.primary()
                    }
                    AgentPermissionKind::RejectOnce | AgentPermissionKind::RejectAlways => {
                        button.danger()
                    }
                }
            })
            .collect::<Vec<_>>();
        let cancel_controller = self.controller.clone();
        let pane = self.pane;
        let request_id = permission.request_id;
        v_flex()
            .w_full()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().warning.outline())
            .bg(cx.theme().warning.fill())
            .p_3()
            .child(
                h_flex()
                    .gap_2()
                    .text_size(zz_ui::rems_from_px(12.0))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .small()
                            .text_color(cx.theme().warning),
                    )
                    .child(permission.title.clone()),
            )
            .child(
                h_flex().flex_wrap().gap_2().children(actions).child(
                    Button::new(format!(
                        "agent-permission-cancel-{}-{request_id}",
                        self.pane.0
                    ))
                    .ghost()
                    .small()
                    .label("Cancel request")
                    .on_click(move |_, _, cx| {
                        cancel_controller.update(cx, |controller, cx| {
                            controller.respond_permission(pane, request_id, None, cx);
                        });
                    }),
                ),
            )
    }

    fn render_error(&self, state: &AgentPaneState, cx: &gpui::App) -> Option<impl IntoElement> {
        let error = state
            .error
            .clone()
            .or_else(|| self.submission_error.clone())?;
        let retry_controller = self.controller.clone();
        let pane = self.pane;
        let auth_buttons = state
            .auth_methods
            .iter()
            .enumerate()
            .map(|(index, method)| {
                let controller = self.controller.clone();
                let method_id = method.id.clone();
                Button::new(format!("agent-auth-{}-{index}", self.pane.0))
                    .secondary()
                    .small()
                    .label(method.name.clone())
                    .tooltip(
                        method
                            .description
                            .clone()
                            .unwrap_or_else(|| "Authenticate with the agent".to_owned()),
                    )
                    .on_click(move |_, _, cx| {
                        controller.update(cx, |controller, cx| {
                            controller.authenticate(pane, method_id.clone(), cx);
                        });
                    })
            })
            .collect::<Vec<_>>();
        Some(
            v_flex()
                .w_full()
                .gap_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().danger.outline())
                .bg(cx.theme().danger.fill())
                .p_3()
                .text_size(zz_ui::rems_from_px(11.0))
                .child(error.to_string())
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new(format!("agent-retry-{}", self.pane.0))
                                .primary()
                                .small()
                                .icon(IconName::Redo2)
                                .label("Try again")
                                .on_click(move |_, _, cx| {
                                    retry_controller.update(cx, |controller, cx| {
                                        controller.retry(pane, cx);
                                    });
                                }),
                        )
                        .children(auth_buttons),
                ),
        )
    }

    fn render_agent_picker(&self, state: &AgentPaneState, view: Entity<Self>) -> impl IntoElement {
        let selected = state.provider;
        let disabled = state.connection.has_active_turn();
        let tooltip = if disabled {
            "Finish or cancel the current turn before switching agents".to_owned()
        } else {
            state.agent_name.as_deref().map_or_else(
                || "Choose an ACP agent".to_owned(),
                |name| format!("{name} · switch agent"),
            )
        };
        agent_chrome_button(("agent-provider-picker", self.pane.0))
            .icon(provider_icon(selected))
            .label(selected.label())
            .dropdown_caret(true)
            .tooltip(tooltip)
            .disabled(disabled)
            .dropdown_menu(move |menu, _, _| {
                AgentProvider::ALL
                    .iter()
                    .fold(menu.min_w(px(190.0)), |menu, provider| {
                        let provider = *provider;
                        let picker_view = view.clone();
                        menu.item(
                            PopupMenuItem::new(provider.label())
                                .icon(provider_icon(provider))
                                .checked(provider == selected)
                                .on_click(move |_, _, cx| {
                                    picker_view.update(cx, |view, cx| {
                                        let result =
                                            view.controller.update(cx, |controller, cx| {
                                                controller.select_provider(view.pane, provider, cx)
                                            });
                                        view.submission_error = result.err();
                                        cx.notify();
                                    });
                                }),
                        )
                    })
            })
            .anchor(Anchor::TopLeft)
    }

    fn render_history_button(
        &self,
        state: &AgentPaneState,
        view: Entity<Self>,
    ) -> impl IntoElement {
        let enabled = state.session_capabilities.list && state.connection.accepts_prompt();
        agent_chrome_button(("agent-session-history", self.pane.0))
            .icon(IconName::GalleryVerticalEnd)
            .label("History")
            .tooltip(if enabled {
                "Browse sessions stored by this agent"
            } else if state.session_capabilities.list {
                "Wait for the current turn to finish"
            } else {
                "This agent does not advertise session history"
            })
            .disabled(!enabled)
            .on_click(move |_, window, cx| {
                view.update(cx, |view, cx| view.open_history(window, cx));
                cx.stop_propagation();
            })
    }

    /// The jump-to-bottom pill, floated over the timeline just above the
    /// composer: an absolute child, so showing it never resizes the scroll
    /// viewport and moves the very tail it is offering to reveal.
    fn render_jump_to_end(&self, view: &Entity<Self>, cx: &gpui::App) -> impl IntoElement {
        let view = view.clone();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(CHROME_GAP))
            .flex()
            .justify_center()
            .child(
                agent_jump_to_bottom_button(("agent-jump-to-end", self.pane.0), cx).on_click(
                    move |_, _, cx| {
                        view.update(cx, AgentView::jump_to_timeline_end);
                        cx.stop_propagation();
                    },
                ),
            )
    }

    fn render_directory_picker(
        &self,
        state: &AgentPaneState,
        view: Entity<Self>,
    ) -> impl IntoElement {
        let busy = state.connection.has_active_turn() || !state.pending_permissions.is_empty();
        let cwd = state.cwd.display().to_string();
        agent_chrome_button(("agent-working-directory", self.pane.0))
            .icon(IconName::FolderOpen)
            .label(session_directory_label(&state.cwd))
            .tooltip(if busy {
                "Finish or cancel the current turn before changing the working directory".to_owned()
            } else {
                format!("{cwd} · choose another workspace (starts a new session)")
            })
            .disabled(busy)
            .on_click(move |_, window, cx| {
                view.update(cx, |view, cx| view.open_directory_picker(window, cx));
                cx.stop_propagation();
            })
    }

    fn open_directory_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.directory_picker.is_some() {
            return;
        }
        let root = directory_picker_root(&self.pane_state.cwd);
        let picker = cx.new(|cx| {
            FilePickerView::new(
                FilePickerMode::Directories,
                root,
                "Choose the agent's working directory",
                window,
                cx,
            )
        });
        cx.subscribe_in(
            &picker,
            window,
            |view, _, event: &FilePickerEvent, window, cx| {
                view.directory_picker = None;
                if let FilePickerEvent::Selected(path) = event {
                    let pane = view.pane;
                    let path = path.clone();
                    let result = view.controller.update(cx, |controller, cx| {
                        controller.set_working_directory(pane, path, cx)
                    });
                    view.submission_error = result.err();
                }
                view.focus(cx).focus(window, cx);
                cx.notify();
            },
        )
        .detach();
        self.completions = Arc::from([]);
        self.completion_selected = None;
        self.directory_picker = Some(picker);
        cx.notify();
    }

    fn render_history_overlay(
        &self,
        state: &AgentPaneState,
        view: &Entity<Self>,
        cx: &gpui::App,
    ) -> impl IntoElement {
        let sessions = state.session_history.sessions.clone();
        let results = self.history_results.clone();
        let selected = self.history_selected;
        let current_session = state.session_id.clone();
        let can_delete = state.session_capabilities.delete;
        let loading = state.session_history.loading;
        let pane = self.pane;
        let today = Local::now().date_naive();
        let rows_view = view.clone();
        let rows = uniform_list(
            ("agent-history-rows", pane.0),
            results.len(),
            move |range, _, cx| {
                range
                    .filter_map(|result_index| {
                        let session_index = *results.get(result_index)?;
                        let session = sessions.get(session_index)?.clone();
                        let title = session_display_title(&session);
                        let directory = session_directory_label(&session.cwd);
                        let updated_at = session
                            .updated_at
                            .as_deref()
                            .map(|value| format_session_timestamp(value, today));
                        let is_selected = selected == Some(result_index);
                        let is_current =
                            current_session.as_deref() == Some(session.session_id.as_str());
                        let pointer_view = rows_view.clone();
                        let click_view = rows_view.clone();
                        let delete_view = rows_view.clone();
                        let delete_id: Arc<str> = Arc::from(session.session_id);
                        Some(
                            h_flex()
                                .id(format!("agent-history-row-{}-{result_index}", pane.0))
                                .w_full()
                                .h(px(HISTORY_ROW_HEIGHT))
                                .items_center()
                                .gap_2()
                                .rounded(cx.theme().radius)
                                .px_2p5()
                                .cursor_pointer()
                                .when(is_selected, |this| this.bg(cx.theme().background.hover()))
                                .when(!is_selected, |this| {
                                    this.hover(|this| this.bg(cx.theme().background.hover()))
                                })
                                .on_mouse_move(move |_, _, cx| {
                                    pointer_view.update(cx, |view, cx| {
                                        if view.history_selected != Some(result_index) {
                                            view.history_selected = Some(result_index);
                                            cx.notify();
                                        }
                                    });
                                })
                                .on_click(move |_, window, cx| {
                                    click_view.update(cx, |view, cx| {
                                        view.open_history_result(result_index, window, cx);
                                    });
                                    cx.stop_propagation();
                                })
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .w_full()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .text_size(zz_ui::rems_from_px(12.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .child(title),
                                        )
                                        .child(
                                            h_flex()
                                                .min_w_0()
                                                .gap_1()
                                                .text_size(zz_ui::rems_from_px(9.0))
                                                .text_color(cx.theme().foreground.muted())
                                                .child(
                                                    Tag::secondary()
                                                        .xsmall()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .text_size(zz_ui::rems_from_px(9.0))
                                                        .text_color(cx.theme().foreground.muted())
                                                        .child(directory),
                                                )
                                                .when(is_current, |this| {
                                                    this.child(
                                                        div()
                                                            .flex_none()
                                                            .rounded(px(999.0))
                                                            .bg(cx.theme().success.fill())
                                                            .px_1()
                                                            .text_size(zz_ui::rems_from_px(8.0))
                                                            .text_color(cx.theme().success)
                                                            .child("CURRENT"),
                                                    )
                                                })
                                                .child(div().flex_1())
                                                .when_some(updated_at, |this, updated_at| {
                                                    this.child(
                                                        div()
                                                            .flex_none()
                                                            .max_w(px(112.0))
                                                            .overflow_hidden()
                                                            .text_ellipsis()
                                                            .whitespace_nowrap()
                                                            .child(updated_at),
                                                    )
                                                }),
                                        ),
                                )
                                .when(can_delete && !is_current, |this| {
                                    this.child(
                                        Button::new(format!(
                                            "agent-history-delete-{}-{result_index}",
                                            pane.0
                                        ))
                                        .ghost()
                                        .xsmall()
                                        .icon(
                                            Icon::new(IconName::Delete)
                                                .text_color(cx.theme().danger),
                                        )
                                        .tooltip("Delete this session")
                                        .disabled(loading)
                                        .on_click(
                                            move |_, _, cx| {
                                                let delete_id = delete_id.clone();
                                                delete_view.update(cx, |view, cx| {
                                                    view.history_delete_confirmation =
                                                        Some(delete_id);
                                                    cx.notify();
                                                });
                                                cx.stop_propagation();
                                            },
                                        ),
                                    )
                                }),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .flex_1()
        .track_scroll(&self.history_scroll);

        let refresh_view = view.clone();
        let scope_view = view.clone();
        let new_view = view.clone();
        let backdrop_view = view.clone();
        let load_more_view = view.clone();
        let scope_label = if self.history_all_projects {
            "All projects"
        } else {
            "This project"
        };
        let scope_icon = if self.history_all_projects {
            IconName::Globe
        } else {
            IconName::Folder
        };
        let delete_confirmation = self.history_delete_confirmation.clone();
        let footer = if let Some(session_id) = delete_confirmation {
            let cancel_view = view.clone();
            let delete_view = view.clone();
            let confirm_id = session_id.clone();
            h_flex()
                .w_full()
                .min_h(px(52.0))
                .flex_none()
                .justify_between()
                .gap(px(CHROME_GAP))
                .border_t_1()
                .border_color(cx.theme().danger.outline())
                .bg(cx.theme().danger.fill())
                .py(px(CHROME_GAP))
                .pl_4()
                .pr(px(CHROME_GAP))
                .text_size(zz_ui::rems_from_px(11.0))
                .child("Permanently delete this session from the agent’s local store?")
                .child(
                    h_flex()
                        .flex_none()
                        .gap(px(CHROME_GAP))
                        .child(
                            agent_chrome_button(("agent-history-delete-cancel", pane.0))
                                .label("Cancel")
                                .on_click(move |_, _, cx| {
                                    cancel_view.update(cx, |view, cx| {
                                        view.history_delete_confirmation = None;
                                        cx.notify();
                                    });
                                    cx.stop_propagation();
                                }),
                        )
                        .child(
                            agent_chrome_button(("agent-history-delete-confirm", pane.0))
                                .danger()
                                .label("Delete")
                                .disabled(loading)
                                .on_click(move |_, _, cx| {
                                    delete_view.update(cx, |view, cx| {
                                        view.confirm_delete_history(&confirm_id, cx);
                                    });
                                    cx.stop_propagation();
                                }),
                        ),
                )
                .into_any_element()
        } else {
            let error = self
                .submission_error
                .clone()
                .or_else(|| state.session_history.error.clone());
            h_flex()
                .w_full()
                .min_h(px(40.0))
                .flex_none()
                .gap(px(CHROME_GAP))
                .border_t_1()
                .border_color(cx.theme().border)
                .py(px(CHROME_GAP))
                .pl_4()
                .pr(px(CHROME_GAP))
                .text_size(zz_ui::rems_from_px(10.0))
                .text_color(cx.theme().foreground.muted())
                .when_some(error, |this, error| {
                    this.child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(cx.theme().danger)
                            .child(error.to_string()),
                    )
                })
                .when(state.session_history.loading, |this| {
                    this.child("Loading sessions…")
                })
                .when(
                    !state.session_history.loading && state.session_history.error.is_none(),
                    |this| {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap(px(CHROME_GAP))
                                .child(palette_shortcut_hint(["up", "down"], "select"))
                                .child(palette_shortcut_hint(["enter"], "open"))
                                .child(palette_shortcut_hint(["escape"], "close")),
                        )
                    },
                )
                .child(div().flex_1())
                .when(state.session_history.next_cursor.is_some(), |this| {
                    this.child(
                        agent_chrome_button(("agent-history-load-more", pane.0))
                            .label("Load more")
                            .disabled(loading)
                            .on_click(move |_, _, cx| {
                                load_more_view.update(cx, |view, cx| {
                                    view.load_more_history(cx);
                                });
                                cx.stop_propagation();
                            }),
                    )
                })
                .into_any_element()
        };

        div()
            .id(("agent-history-overlay", pane.0))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(cx.theme().scrim)
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                backdrop_view.update(cx, |view, cx| view.close_history(window, cx));
                cx.stop_propagation();
            })
            .child(
                v_flex()
                    .id(("agent-history-modal", pane.0))
                    .relative()
                    .w(relative(0.92))
                    .max_w(px(660.0))
                    .h(relative(0.82))
                    .min_h(px(240.0))
                    .max_h(px(720.0))
                    .overflow_hidden()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background.raised(1))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        v_flex()
                            .flex_none()
                            .gap(px(CHROME_GAP))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .p(px(CHROME_GAP))
                            .child(
                                h_flex()
                                    .w_full()
                                    .h(px(32.0))
                                    .rounded(cx.theme().radius)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().background.raised(1))
                                    .px_2p5()
                                    .child(
                                        Icon::new(IconName::Search)
                                            .xsmall()
                                            .text_color(cx.theme().foreground.muted()),
                                    )
                                    .child(
                                        Input::new(&self.history_input)
                                            .small()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(zz_ui::rems_from_px(12.0))
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap(px(CHROME_GAP))
                                    .child(
                                        agent_chrome_button(("agent-history-scope", pane.0))
                                            .secondary()
                                            .icon(scope_icon)
                                            .label(scope_label)
                                            .disabled(loading)
                                            .on_click(move |_, _, cx| {
                                                scope_view.update(cx, |view, cx| {
                                                    view.toggle_history_scope(cx);
                                                });
                                                cx.stop_propagation();
                                            }),
                                    )
                                    .child(
                                        agent_chrome_button(("agent-history-refresh", pane.0))
                                            .icon(IconName::Redo2)
                                            .label("Refresh")
                                            .disabled(loading)
                                            .on_click(move |_, _, cx| {
                                                refresh_view.update(cx, |view, cx| {
                                                    view.refresh_history(cx);
                                                });
                                                cx.stop_propagation();
                                            }),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        agent_chrome_button(("agent-history-new", pane.0))
                                            .icon(IconName::Plus)
                                            .label("New")
                                            .tooltip("Start a new session")
                                            .disabled(loading)
                                            .on_click(move |_, window, cx| {
                                                new_view.update(cx, |view, cx| {
                                                    view.start_new_session(window, cx);
                                                });
                                                cx.stop_propagation();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .p(px(CHROME_GAP))
                            .child(rows)
                            .when(self.history_results.is_empty(), |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(zz_ui::rems_from_px(11.0))
                                        .text_color(cx.theme().foreground.muted())
                                        .child(if state.session_history.loading {
                                            "Loading sessions…"
                                        } else if self.history_input.read(cx).value().is_empty() {
                                            "No sessions found for this scope."
                                        } else {
                                            "No sessions match that search."
                                        }),
                                )
                            }),
                    )
                    .child(footer),
            )
    }

    fn render_config_picker(
        &self,
        option: &AgentConfigOption,
        icon: IconName,
        view: Entity<Self>,
        enabled: bool,
    ) -> gpui::AnyElement {
        let current_label = option
            .choices
            .iter()
            .find(|choice| choice.value == option.current_value)
            .map_or_else(|| option.name.clone(), |choice| choice.name.clone());
        let option_id = option.id.clone();
        let current_value = option.current_value.clone();
        let choices = option.choices.clone();
        let description = option
            .description
            .clone()
            .unwrap_or_else(|| option.name.clone());
        agent_chrome_button(format!("agent-config-picker-{}-{}", self.pane.0, option.id))
            .icon(icon)
            .label(current_label)
            .dropdown_caret(true)
            .tooltip(description)
            .disabled(!enabled || choices.is_empty())
            .dropdown_menu(move |menu, _, _| {
                choices.iter().fold(menu.min_w(px(250.0)), |menu, choice| {
                    let choice_name = choice.name.clone();
                    let choice_description = choice.description.clone();
                    let value = choice.value.clone();
                    let config_id = option_id.clone();
                    let picker_view = view.clone();
                    menu.item(
                        PopupMenuItem::element(move |_, cx| {
                            v_flex()
                                .min_w_0()
                                .ml_1()
                                .py_1()
                                .child(
                                    div()
                                        .text_size(zz_ui::rems_from_px(12.0))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(choice_name.clone()),
                                )
                                .when_some(choice_description.clone(), |this, description| {
                                    this.child(
                                        div()
                                            .max_w(px(300.0))
                                            .text_size(zz_ui::rems_from_px(10.0))
                                            .text_color(cx.theme().foreground.muted())
                                            .child(description),
                                    )
                                })
                        })
                        .checked(choice.value == current_value)
                        .on_click(move |_, _, cx| {
                            picker_view.update(cx, |view, cx| {
                                let result = view.controller.update(cx, |controller, cx| {
                                    controller.set_config_option(view.pane, &config_id, &value, cx)
                                });
                                view.submission_error = result.err();
                                cx.notify();
                            });
                        }),
                    )
                })
            })
            .anchor(Anchor::BottomLeft)
            .into_any_element()
    }

    fn render_legacy_mode_picker(
        &self,
        state: &AgentPaneState,
        view: Entity<Self>,
        enabled: bool,
    ) -> Option<gpui::AnyElement> {
        if !state.config_options.is_empty() || state.modes.is_empty() {
            return None;
        }
        let current = state.mode.clone();
        let current_label = state
            .modes
            .iter()
            .find(|mode| current.as_deref() == Some(mode.id.as_str()))
            .map_or("Permissions", |mode| mode.name.as_str())
            .to_owned();
        let modes = state.modes.clone();
        Some(
            agent_chrome_button(("agent-mode-picker", self.pane.0))
                .icon(IconName::Check)
                .label(current_label)
                .dropdown_caret(true)
                .tooltip("Agent permission mode")
                .disabled(!enabled)
                .dropdown_menu(move |menu, _, _| {
                    modes.iter().fold(menu.min_w(px(250.0)), |menu, mode| {
                        let mode_name = mode.name.clone();
                        let mode_description = mode.description.clone();
                        let mode_id = mode.id.clone();
                        let mode_view = view.clone();
                        menu.item(
                            PopupMenuItem::element(move |_, cx| {
                                v_flex()
                                    .ml_1()
                                    .py_1()
                                    .child(
                                        div()
                                            .text_size(zz_ui::rems_from_px(12.0))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(mode_name.clone()),
                                    )
                                    .when_some(mode_description.clone(), |this, description| {
                                        this.child(
                                            div()
                                                .max_w(px(300.0))
                                                .text_size(zz_ui::rems_from_px(10.0))
                                                .text_color(cx.theme().foreground.muted())
                                                .child(description),
                                        )
                                    })
                            })
                            .checked(current.as_deref() == Some(mode.id.as_str()))
                            .on_click(move |_, _, cx| {
                                mode_view.update(cx, |view, cx| {
                                    let result = view.controller.update(cx, |controller, cx| {
                                        controller.set_mode(view.pane, &mode_id, cx)
                                    });
                                    view.submission_error = result.err();
                                    cx.notify();
                                });
                            }),
                        )
                    })
                })
                .anchor(Anchor::BottomLeft)
                .into_any_element(),
        )
    }

    fn render_completions(&self, view: &Entity<Self>, cx: &gpui::App) -> Option<gpui::AnyElement> {
        if self.completions.is_empty() {
            return None;
        }
        let selected = self.completion_selected;
        let visible_rows = self
            .completions
            .len()
            .min(usize::from(MAX_VISIBLE_COMPLETION_ROWS));
        let visible_rows =
            f32::from(u8::try_from(visible_rows).unwrap_or(MAX_VISIBLE_COMPLETION_ROWS));
        let pane = self.pane;
        let completions = Arc::clone(&self.completions);
        let rows_view = view.clone();
        let rows = uniform_list(
            ("agent-completion-rows", pane.0),
            completions.len(),
            move |range, _, cx| {
                range
                    .filter_map(|index| {
                        let completion = completions.get(index)?.clone();
                        let command = completion.command.clone();
                        let sigil = completion.sigil;
                        let name = bare_command_name(&command.name);
                        let description = meaningful_command_description(&command.description)
                            .map(ToOwned::to_owned);
                        let is_selected = selected == Some(index);
                        let hover_view = rows_view.clone();
                        let click_view = rows_view.clone();
                        Some(
                            h_flex()
                                .id(format!("agent-completion-{}-{index}", pane.0))
                                .w_full()
                                .h(px(COMPLETION_ROW_HEIGHT))
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .rounded(cx.theme().radius)
                                .px_3()
                                .cursor_pointer()
                                .when(is_selected, |this| this.bg(cx.theme().background.hover()))
                                .when(!is_selected, |this| {
                                    this.hover(|this| this.bg(cx.theme().background.hover()))
                                })
                                .on_hover(move |hovered, _, cx| {
                                    if *hovered {
                                        hover_view.update(cx, |view, cx| {
                                            if view.completion_selected != Some(index) {
                                                view.completion_selected = Some(index);
                                                cx.notify();
                                            }
                                        });
                                    }
                                })
                                .on_click(move |_, window, cx| {
                                    click_view.update(cx, |view, cx| {
                                        view.accept_completion(index, window, cx);
                                    });
                                    cx.stop_propagation();
                                })
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .w_full()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .text_size(zz_ui::rems_from_px(12.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .child(format!("{sigil}{name}")),
                                        )
                                        .when_some(description, |this, description| {
                                            this.child(
                                                div()
                                                    .w_full()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .whitespace_nowrap()
                                                    .text_size(zz_ui::rems_from_px(10.0))
                                                    .text_color(cx.theme().foreground.muted())
                                                    .child(description),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .rounded(cx.theme().radius)
                                        .bg(cx.theme().background.raised(2))
                                        .px_2()
                                        .py(px(2.0))
                                        .text_size(zz_ui::rems_from_px(8.0))
                                        .text_color(cx.theme().foreground.muted())
                                        .child(match command.kind {
                                            AgentCommandKind::Skill => "SKILL",
                                            AgentCommandKind::Command => "COMMAND",
                                        }),
                                ),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .w_full()
        .h(px(COMPLETION_ROW_HEIGHT * visible_rows))
        .track_scroll(&self.completion_scroll);
        Some(
            v_flex()
                .w_full()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background.raised(1))
                .p_1()
                .shadow_md()
                .overflow_hidden()
                .child(rows)
                .into_any_element(),
        )
    }

    fn render_sticky_strip(
        rows: &[StickyAgentRow],
        view: &Entity<Self>,
        cx: &gpui::App,
    ) -> Option<AnyElement> {
        if rows.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .w_full()
                .gap_1()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background.raised(1))
                .p_1()
                .children(rows.iter().map(|row| match row {
                    StickyAgentRow::Subagent { id, status, label } => {
                        let spinner_color = if *status == AgentToolStatus::NeedsApproval {
                            cx.theme().warning
                        } else {
                            cx.theme().foreground.muted()
                        };
                        h_flex()
                            .id(("agent-sticky-subagent", *id))
                            .w_full()
                            .h(px(STICKY_ROW_HEIGHT))
                            .items_center()
                            .gap_2()
                            .px_2()
                            .child(
                                div()
                                    .flex_none()
                                    .child(Spinner::new().xsmall().color(spinner_color)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_size(zz_ui::rems_from_px(12.0))
                                    .text_color(cx.theme().foreground.muted())
                                    .child(label.clone()),
                            )
                            .into_any_element()
                    }
                    StickyAgentRow::Notification { id, status, label } => {
                        let id = *id;
                        let dismiss = view.clone();
                        h_flex()
                            .id(("agent-sticky-notification", id))
                            .w_full()
                            .h(px(STICKY_ROW_HEIGHT))
                            .items_center()
                            .gap_2()
                            .px_2()
                            .child(
                                Icon::new(notification_icon(status))
                                    .small()
                                    .flex_none()
                                    .text_color(cx.theme().foreground.muted()),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_size(zz_ui::rems_from_px(12.0))
                                    .text_color(cx.theme().foreground.muted())
                                    .child(label.clone()),
                            )
                            .child(
                                Button::new(("agent-dismiss-notification", id))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip("Dismiss")
                                    .on_click(move |_, _, cx| {
                                        dismiss.update(cx, |view, cx| {
                                            view.dismiss_notification(id, cx);
                                        });
                                    }),
                            )
                            .into_any_element()
                    }
                }))
                .into_any_element(),
        )
    }

    #[allow(clippy::redundant_closure_for_method_calls)]
    fn render_composer(
        &self,
        state: &AgentPaneState,
        sticky_rows: &[StickyAgentRow],
        view: &Entity<Self>,
        cx: &gpui::App,
    ) -> impl IntoElement {
        let active = state.connection.has_active_turn();
        let can_submit = state.connection.accepts_prompt();
        let action = if active {
            let controller = self.controller.clone();
            let pane = self.pane;
            Button::new(format!("agent-cancel-{}", self.pane.0))
                .danger()
                .small()
                .icon(IconName::Close)
                .rounded(px(999.0))
                .tooltip("Cancel the current turn")
                .on_click(move |_, _, cx| {
                    controller.update(cx, |controller, cx| controller.cancel(pane, cx));
                })
                .into_any_element()
        } else {
            let submit_view = view.clone();
            Button::new(format!("agent-submit-{}", self.pane.0))
                .primary()
                .small()
                .icon(IconName::ArrowUp)
                .rounded(px(999.0))
                .tooltip("Send message")
                .disabled(!can_submit)
                .on_click(move |_, window, cx| {
                    submit_view.update(cx, |view, cx| view.submit(window, cx));
                })
                .into_any_element()
        };
        let settings_enabled = state.connection.accepts_prompt() && !state.settings_busy;
        let permission_option = state
            .config_options
            .iter()
            .find(|option| option.category == AgentConfigCategory::Mode);
        let effort_option = state
            .config_options
            .iter()
            .find(|option| option.category == AgentConfigCategory::ThoughtLevel);
        let model_option = state
            .config_options
            .iter()
            .find(|option| option.category == AgentConfigCategory::Model);
        let mut settings = Vec::with_capacity(3);
        if let Some(option) = permission_option {
            settings.push(self.render_config_picker(
                option,
                IconName::Check,
                view.clone(),
                settings_enabled,
            ));
        } else if let Some(mode) =
            self.render_legacy_mode_picker(state, view.clone(), settings_enabled)
        {
            settings.push(mode);
        }
        if let Some(option) = model_option {
            settings.push(self.render_config_picker(
                option,
                IconName::Asterisk,
                view.clone(),
                settings_enabled,
            ));
        }
        if let Some(option) = effort_option {
            settings.push(self.render_config_picker(
                option,
                IconName::Cpu,
                view.clone(),
                settings_enabled,
            ));
        }
        let usage = state.usage.map(|(used, size)| format!("{used} / {size}"));
        let command_hint =
            active_command_hint(&self.last_input, state.provider, &state.available_commands);
        let completions = self.render_completions(view, cx);
        let sticky_strip = Self::render_sticky_strip(sticky_rows, view, cx);

        v_flex()
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .bottom(px(0.0))
            .w_full()
            .p_3()
            .child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .right(px(0.0))
                    .bottom(px(0.0))
                    .h(px(COMPOSER_OCCLUSION_HEIGHT))
                    .bg(cx.theme().background),
            )
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(COMPOSER_MAX_WIDTH))
                    .mx_auto()
                    .gap_2()
                    .when_some(sticky_strip, |this, sticky_strip| this.child(sticky_strip))
                    .children(
                        state
                            .pending_permissions
                            .iter()
                            .map(|permission| self.render_permission(permission, cx)),
                    )
                    .when_some(self.render_error(state, cx), |this, error| {
                        this.child(error)
                    })
                    .when_some(completions, |this, completions| this.child(completions))
                    .child(
                        v_flex()
                            .w_full()
                            .min_h(px(COMPOSER_MIN_HEIGHT))
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background.raised(1))
                            .when(cx.theme().shadow, |this| this.shadow_xs())
                            .children(self.render_attachments(view, cx))
                            .child(
                                Input::new(&self.input)
                                    .w_full()
                                    .min_w_0()
                                    .text_size(zz_ui::rems_from_px(13.0))
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false),
                            )
                            .when_some(command_hint, |this, hint| {
                                this.child(
                                    div()
                                        .px_3()
                                        .pb_1()
                                        .text_size(zz_ui::rems_from_px(10.0))
                                        .text_color(cx.theme().foreground.muted())
                                        .child(hint),
                                )
                            })
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_h(px(40.0))
                                    .items_end()
                                    .justify_between()
                                    .gap(px(CHROME_GAP))
                                    .p(px(CHROME_GAP))
                                    .child(
                                        h_flex()
                                            .min_w_0()
                                            .flex_wrap()
                                            .gap(px(CHROME_GAP))
                                            .children(settings),
                                    )
                                    .child(
                                        h_flex()
                                            .flex_none()
                                            .gap(px(CHROME_GAP))
                                            .when_some(usage, |this, usage| {
                                                this.child(
                                                    div()
                                                        .text_size(zz_ui::rems_from_px(9.0))
                                                        .text_color(cx.theme().foreground.muted())
                                                        .child(usage),
                                                )
                                            })
                                            .child(action),
                                    ),
                            ),
                    ),
            )
    }
}

impl Focusable for AgentView {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.focus(cx)
    }
}

impl AgentView {
    fn drain_pending_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane;
        let Some(pending) = self
            .controller
            .update(cx, |controller, _| controller.take_pending_composer(pane))
        else {
            return;
        };
        self.input.update(cx, |input, cx| {
            let current = input.value().to_string();
            let value = if current.trim().is_empty() {
                pending
            } else {
                format!("{}\n{pending}", current.trim_end_matches('\n'))
            };
            input.set_value(value, window, cx);
        });
        self.submission_error = None;
        self.stick.engage(&self.timeline_scroll, cx.reduce_motion());
    }

    fn on_timeline_scroll(&mut self, cx: &mut Context<Self>) {
        let reduce_motion = cx.reduce_motion();
        if self
            .stick
            .on_user_scroll(&self.timeline_scroll, reduce_motion)
        {
            cx.notify();
        }
    }

    fn jump_to_timeline_end(&mut self, cx: &mut Context<Self>) {
        self.stick.engage(&self.timeline_scroll, cx.reduce_motion());
        cx.notify();
    }

    /// Ask for one spring frame, at most one callback in flight. Each step
    /// notifies while it still has travel left, which re-enters `render` and
    /// arms the next frame; the loop stops itself once the spring parks.
    fn drive_stick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.reduce_motion() || !self.stick.wants_frame(&self.timeline_scroll) {
            return;
        }
        self.stick.arm();
        let view = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            view.update(cx, |view: &mut Self, cx| {
                let list = view.timeline_scroll.clone();
                if view.stick.step(&list) {
                    cx.notify();
                }
            })
            .ok();
        });
    }
}

impl Render for AgentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_pending_composer(window, cx);
        let state = self.pane_state.clone();
        let rows = self.timeline.rows.clone();
        let sticky_rows = sticky_agent_rows(&rows, &self.dismissed_notifications);
        let timeline_clearance =
            TIMELINE_COMPOSER_CLEARANCE + sticky_strip_clearance(sticky_rows.len());
        self.stick.set_bottom_padding(timeline_clearance);
        self.drive_stick(window, cx);
        let view = cx.entity();
        let header_controls = h_flex()
            .min_w_0()
            .gap(px(CHROME_GAP))
            .child(self.render_agent_picker(&state, view.clone()))
            .child(self.render_history_button(&state, view.clone()));
        let input_focus = self.focus(cx);
        let root = div()
            .id(("agent-pane", self.pane.0))
            .key_context(AGENT_KEY_CONTEXT)
            .track_focus(&input_focus)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .bg(crate::theme::app_pane_background(cx))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .capture_action(cx.listener(Self::move_completion_up))
            .capture_action(cx.listener(Self::move_completion_down))
            .capture_action(cx.listener(Self::complete))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(agent_pane_header(
                header_controls,
                self.render_directory_picker(&state, view.clone()),
                cx,
            ))
            .child(
                div()
                    .id(("agent-thread-scroll", self.pane.0))
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(rows.is_empty(), |this| {
                        this.child(
                            div().w_full().px_3().child(
                                div()
                                    .w_full()
                                    .max_w(px(AGENT_CONTENT_MAX_WIDTH))
                                    .mx_auto()
                                    .child(Self::render_empty_state(&state, cx)),
                            ),
                        )
                    })
                    .when(!rows.is_empty(), |this| {
                        this.child(
                            AgentTimeline::new(
                                rows,
                                self.timeline_scroll.clone(),
                                self.timeline_store.clone(),
                            )
                            .bottom_padding(timeline_clearance),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .bottom(px(COMPOSER_OCCLUSION_HEIGHT))
                                .child(Scrollbar::vertical(&self.timeline_scroll)),
                        )
                        .when(self.stick.shows_jump_button(), |this| {
                            this.child(self.render_jump_to_end(&view, cx))
                        })
                    }),
            )
            .child(self.render_composer(&state, &sticky_rows, &view, cx))
            .when(self.history_open, |this| {
                this.child(self.render_history_overlay(&state, &view, cx))
            })
            .when_some(self.directory_picker.clone(), |this, picker| {
                this.child(picker)
            });
        round_div_radii(root, pane_content_radii(cx, self.window_corners))
    }
}

fn sticky_agent_rows(rows: &[TimelineRow], dismissed: &BTreeSet<u64>) -> Vec<StickyAgentRow> {
    let entries: Vec<&AgentEntry> = rows
        .iter()
        .flat_map(|row| match row {
            TimelineRow::Single(entry) => std::slice::from_ref(entry),
            TimelineRow::Group { entries, .. } => entries.as_slice(),
        })
        .collect();
    let last_user = entries
        .iter()
        .rposition(|entry| matches!(entry, AgentEntry::User { .. }));
    let mut sticky = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let acknowledged = last_user.is_some_and(|user| index < user);
        append_sticky_agent_entry(&mut sticky, entry, acknowledged, dismissed);
    }
    sticky
}

fn append_sticky_agent_entry(
    sticky: &mut Vec<StickyAgentRow>,
    entry: &AgentEntry,
    acknowledged: bool,
    dismissed: &BTreeSet<u64>,
) {
    match entry {
        AgentEntry::Tool(tool) => {
            if tool.subagent
                && matches!(
                    tool.status,
                    AgentToolStatus::Pending
                        | AgentToolStatus::Running
                        | AgentToolStatus::NeedsApproval
                )
            {
                sticky.push(StickyAgentRow::Subagent {
                    id: tool.id,
                    status: tool.status,
                    label: tool.label.clone(),
                });
            }
            for child in tool.children.iter() {
                append_sticky_agent_entry(sticky, child, acknowledged, dismissed);
            }
        }
        AgentEntry::Notification {
            id,
            task_id,
            status,
            summary,
            ..
        } if !acknowledged && !dismissed.contains(id) => {
            sticky.push(StickyAgentRow::Notification {
                id: *id,
                status: status.clone(),
                label: if summary.is_empty() {
                    task_id.clone()
                } else {
                    summary.clone()
                },
            });
        }
        _ => {}
    }
}

fn sticky_strip_clearance(row_count: usize) -> f32 {
    if row_count == 0 {
        0.0
    } else {
        let row_count = u16::try_from(row_count).unwrap_or(u16::MAX);
        let row_gaps = f32::from(row_count.saturating_sub(1)) * 4.0;
        f32::from(row_count) * STICKY_ROW_HEIGHT + row_gaps + 16.0
    }
}

fn notification_icon(status: &str) -> IconName {
    match status.trim().to_ascii_lowercase().as_str() {
        "completed" | "succeeded" | "success" => IconName::Check,
        "failed" | "error" | "cancelled" | "canceled" => IconName::Close,
        _ => IconName::Asterisk,
    }
}

fn disconnected_pane_state() -> AgentPaneState {
    AgentPaneState {
        provider: AgentProvider::Codex,
        connection: AgentConnectionState::Disconnected,
        pending_permissions: Arc::from([]),
        auth_methods: Arc::from([]),
        error: Some(Arc::from("agent pane is not registered")),
        agent_name: None,
        cwd: PathBuf::from("/"),
        session_id: None,
        session_capabilities: AgentSessionCapabilities::default(),
        session_history: AgentSessionHistoryState::default(),
        settings_busy: false,
        mode: None,
        modes: Arc::from([]),
        config_options: Arc::from([]),
        available_commands: Arc::from([]),
        usage: None,
        pending_composer: None,
        queued_prompts: 0,
    }
}

fn ui_entries(entries: &[AgentThreadEntry]) -> Arc<[AgentEntry]> {
    entries.iter().map(ui_entry).collect::<Vec<_>>().into()
}

fn ui_entry(entry: &AgentThreadEntry) -> AgentEntry {
    match entry {
        AgentThreadEntry::User {
            id,
            markdown,
            images,
        } => AgentEntry::User {
            id: *id,
            markdown: SharedString::from(markdown.clone()),
            images: images.clone().into(),
        },
        AgentThreadEntry::Assistant { id, markdown, .. } => AgentEntry::Assistant {
            id: *id,
            markdown: SharedString::from(markdown.clone()),
        },
        AgentThreadEntry::Reasoning {
            id,
            label,
            markdown,
            default_expanded,
        } => AgentEntry::Reasoning {
            id: *id,
            label: SharedString::from(label.clone()),
            markdown: SharedString::from(markdown.clone()),
            default_expanded: *default_expanded,
        },
        AgentThreadEntry::Tool {
            id,
            kind,
            status,
            label,
            location,
            input,
            output,
            default_expanded,
            subagent,
            children,
            ..
        } => AgentEntry::Tool(AgentToolEntry {
            id: *id,
            kind: match kind {
                AgentToolKindModel::Read => AgentToolKind::Read,
                AgentToolKindModel::Search => AgentToolKind::Search,
                AgentToolKindModel::Edit
                | AgentToolKindModel::Delete
                | AgentToolKindModel::Move => AgentToolKind::Edit,
                AgentToolKindModel::Execute => AgentToolKind::Execute,
                AgentToolKindModel::Fetch => AgentToolKind::Fetch,
                AgentToolKindModel::Think => AgentToolKind::Think,
                AgentToolKindModel::SwitchMode | AgentToolKindModel::Other => AgentToolKind::Other,
            },
            status: match status {
                AgentToolStatusModel::Pending => AgentToolStatus::Pending,
                AgentToolStatusModel::Running => AgentToolStatus::Running,
                AgentToolStatusModel::NeedsApproval => AgentToolStatus::NeedsApproval,
                AgentToolStatusModel::Completed => AgentToolStatus::Completed,
                AgentToolStatusModel::Failed => AgentToolStatus::Failed,
                AgentToolStatusModel::Canceled => AgentToolStatus::Canceled,
            },
            label: SharedString::from(label.clone()),
            location: location.clone().map(SharedString::from),
            input: input.as_ref().map(ui_tool_payload),
            output: output
                .iter()
                .map(ui_tool_payload)
                .collect::<Vec<_>>()
                .into(),
            default_expanded: *default_expanded,
            subagent: *subagent,
            children: children.iter().map(ui_entry).collect::<Vec<_>>().into(),
        }),
        AgentThreadEntry::Plan { id, markdown } => AgentEntry::Plan {
            id: *id,
            markdown: SharedString::from(markdown.clone()),
        },
        AgentThreadEntry::Notification {
            id,
            task_id,
            status,
            summary,
            result_markdown,
            ..
        } => AgentEntry::Notification {
            id: *id,
            task_id: SharedString::from(task_id.clone()),
            status: SharedString::from(status.clone()),
            summary: SharedString::from(summary.clone()),
            result_markdown: SharedString::from(result_markdown.clone()),
        },
    }
}

fn synchronize_entry_store(
    store: &Entity<AgentTimelineStore>,
    entry: &AgentEntry,
    cx: &mut Context<AgentView>,
) {
    match entry {
        AgentEntry::User { id, markdown, .. }
        | AgentEntry::Assistant { id, markdown }
        | AgentEntry::Reasoning { id, markdown, .. }
        | AgentEntry::Plan { id, markdown }
        | AgentEntry::Notification {
            id,
            result_markdown: markdown,
            ..
        } => {
            store.update(cx, |store, cx| {
                store.synchronize_markdown(*id, MarkdownSlot::Body, markdown.clone(), cx);
            });
        }
        AgentEntry::Tool(tool) => {
            store.update(cx, |store, cx| {
                store.synchronize_tool_content(
                    tool.id,
                    tool.location.clone(),
                    tool.input.clone(),
                    tool.output.clone(),
                    cx,
                );
            });
            for child in tool.children.iter() {
                synchronize_entry_store(store, child, cx);
            }
        }
    }
}

fn ui_tool_payload(payload: &ToolPayload) -> AgentToolPayload {
    match payload {
        ToolPayload::Diff { path, old, new } => AgentToolPayload::Diff {
            path: path.clone().into(),
            old: old.clone().map(SharedString::from),
            new: new.clone().into(),
        },
        ToolPayload::Text(text) => AgentToolPayload::Text(text.clone().into()),
        ToolPayload::Json(json) => AgentToolPayload::Json(json.clone().into()),
        ToolPayload::Terminal(terminal) => AgentToolPayload::Terminal(terminal.clone().into()),
    }
}

fn agent_chrome_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .h(px(CHROME_BUTTON_HEIGHT))
        .px_2p5()
}

fn session_directory_label(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| cwd.display().to_string(), ToOwned::to_owned)
}

fn session_display_title(session: &AgentSessionSummary) -> String {
    session
        .title
        .clone()
        .unwrap_or_else(|| session_directory_label(&session.cwd))
}

fn format_session_timestamp(value: &str, today: NaiveDate) -> String {
    let Ok(timestamp) = DateTime::parse_from_rfc3339(value) else {
        return value.to_owned();
    };
    let local = timestamp.with_timezone(&Local);
    session_timestamp_label(local.date_naive(), local.hour(), local.minute(), today)
}

fn session_timestamp_label(date: NaiveDate, hour: u32, minute: u32, today: NaiveDate) -> String {
    let time = format!("{hour:02}:{minute:02}");
    if date == today {
        return format!("Today, {time}");
    }
    if today.pred_opt() == Some(date) {
        return format!("Yesterday, {time}");
    }
    let month = date.format("%b");
    if date.year() == today.year() {
        format!("{month} {}, {time}", date.day())
    } else {
        format!("{month} {}, {}, {time}", date.day(), date.year())
    }
}

fn ranked_session_indices(sessions: &[AgentSessionSummary], query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    let mut ranked = sessions
        .iter()
        .enumerate()
        .filter_map(|(index, session)| {
            if needle.is_empty() {
                return Some((3, index));
            }
            [
                session.title.as_deref().unwrap_or_default().to_lowercase(),
                session.cwd.to_string_lossy().to_lowercase(),
                session.session_id.to_lowercase(),
            ]
            .iter()
            .filter_map(|candidate| completion_score(candidate, &needle))
            .min()
            .map(|score| (score, index))
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(score, index)| (*score, *index));
    ranked.into_iter().map(|(_, index)| index).collect()
}

fn history_result_index_for_session(
    sessions: &[AgentSessionSummary],
    results: &[usize],
    session_id: &str,
) -> Option<usize> {
    results.iter().position(|session_index| {
        sessions
            .get(*session_index)
            .is_some_and(|session| session.session_id == session_id)
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompletionQuery {
    needle: String,
    replacement: Range<usize>,
    sigil: char,
}

const fn completion_sigil(provider: AgentProvider) -> char {
    match provider {
        AgentProvider::Codex => '$',
        AgentProvider::ClaudeCode => '/',
    }
}

const fn provider_icon(provider: AgentProvider) -> IconName {
    match provider {
        AgentProvider::Codex => IconName::Openai,
        AgentProvider::ClaudeCode => IconName::Claude,
    }
}

fn completion_query(value: &str, cursor: usize, sigil: char) -> Option<CompletionQuery> {
    if cursor > value.len() || !value.is_char_boundary(cursor) {
        return None;
    }
    let before_cursor = &value[..cursor];
    let line_start = before_cursor.rfind('\n').map_or(0, |index| index + 1);
    let sigil_index = before_cursor[line_start..].rfind(sigil)? + line_start;
    if sigil_index > line_start
        && !value[..sigil_index]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        return None;
    }
    let tail = &value[sigil_index + sigil.len_utf8()..cursor];
    if tail.chars().any(char::is_whitespace) {
        return None;
    }
    Some(CompletionQuery {
        needle: tail.to_owned(),
        replacement: sigil_index..cursor,
        sigil,
    })
}

fn bare_command_name(name: &str) -> &str {
    name.trim_start_matches(['/', '$'])
}

fn completion_score(candidate: &str, needle: &str) -> Option<u8> {
    if needle.is_empty() {
        return Some(3);
    }
    if candidate == needle {
        return Some(0);
    }
    if candidate.starts_with(needle) {
        return Some(1);
    }
    if candidate.contains(needle) {
        return Some(2);
    }
    let mut characters = candidate.chars();
    needle
        .chars()
        .all(|needle| characters.by_ref().any(|candidate| candidate == needle))
        .then_some(3)
}

fn ranked_completions(
    commands: &[AgentCommand],
    query: &CompletionQuery,
) -> Vec<CommandCompletion> {
    let needle = query.needle.to_ascii_lowercase();
    let mut ranked = commands
        .iter()
        .filter_map(|command| {
            let searchable = bare_command_name(&command.name).to_ascii_lowercase();
            completion_score(&searchable, &needle).map(|score| {
                (
                    score,
                    matches!(command.kind, AgentCommandKind::Command),
                    command.name.to_ascii_lowercase(),
                    CommandCompletion {
                        command: command.clone(),
                        replacement: query.replacement.clone(),
                        sigil: query.sigil,
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| (left.1, left.0, &left.2).cmp(&(right.1, right.0, &right.2)));
    ranked
        .into_iter()
        .map(|(_, _, _, completion)| completion)
        .collect()
}

fn meaningful_command_description(description: &str) -> Option<&str> {
    let description = description.trim();
    (!description.is_empty()
        && description
            .chars()
            .any(|character| character != '.' && character != '…'))
    .then_some(description)
}

fn active_command_hint(
    value: &str,
    provider: AgentProvider,
    commands: &[AgentCommand],
) -> Option<String> {
    let command = value
        .trim_start()
        .strip_prefix(completion_sigil(provider))?;
    let (name, arguments) = command
        .split_once(char::is_whitespace)
        .map_or((command, ""), |(name, arguments)| (name, arguments));
    if !arguments.trim().is_empty() {
        return None;
    }
    commands
        .iter()
        .find(|command| bare_command_name(&command.name).eq_ignore_ascii_case(name))
        .and_then(|command| command.input_hint.as_deref())
        .map(|hint| format!("Argument · {hint}"))
}

#[cfg(test)]
mod completion_tests {
    #[cfg(not(target_os = "macos"))]
    use std::{cell::RefCell, rc::Rc};

    #[cfg(not(target_os = "macos"))]
    use gpui::{TestAppContext, VisualTestContext};
    #[cfg(not(target_os = "macos"))]
    use zz_daemon::DaemonError;
    #[cfg(not(target_os = "macos"))]
    use zz_ui::Root;

    use super::*;

    fn command(name: &str) -> AgentCommand {
        AgentCommand {
            name: name.to_owned(),
            description: format!("Run {name}"),
            input_hint: None,
            kind: AgentCommandKind::Command,
        }
    }

    fn session(id: &str, cwd: &str, title: Option<&str>) -> AgentSessionSummary {
        AgentSessionSummary {
            session_id: id.to_owned(),
            cwd: PathBuf::from(cwd),
            additional_directories: Vec::new(),
            title: title.map(ToOwned::to_owned),
            updated_at: None,
        }
    }

    #[test]
    fn session_history_searches_title_path_and_opaque_id_without_losing_source_order() {
        let sessions = vec![
            session("sess-alpha", "/work/one", Some("Release planning")),
            session("sess-beta", "/work/payments", Some("Fix retries")),
            session("opaque-needle-id", "/work/three", None),
        ];

        assert_eq!(ranked_session_indices(&sessions, ""), vec![0, 1, 2]);
        assert_eq!(ranked_session_indices(&sessions, "release"), vec![0]);
        assert_eq!(ranked_session_indices(&sessions, "payments"), vec![1]);
        assert_eq!(ranked_session_indices(&sessions, "needle"), vec![2]);
    }

    #[test]
    fn session_history_retains_selection_by_identity_after_catalog_updates() {
        let sessions = vec![
            session("sess-gamma", "/work/three", Some("Gamma")),
            session("sess-alpha", "/work/one", Some("Alpha")),
            session("sess-beta", "/work/two", Some("Beta")),
        ];
        let results = ranked_session_indices(&sessions, "");

        assert_eq!(
            history_result_index_for_session(&sessions, &results, "sess-beta"),
            Some(2)
        );
        assert_eq!(
            history_result_index_for_session(&sessions, &results, "missing"),
            None
        );
    }

    #[test]
    fn session_directories_are_clipped_to_their_last_component() {
        assert_eq!(
            session_directory_label(Path::new("/work/payments")),
            "payments"
        );
        assert_eq!(
            session_directory_label(Path::new("/work/payments/")),
            "payments"
        );
        assert_eq!(session_directory_label(Path::new("/")), "/");
    }

    #[test]
    fn session_timestamps_use_compact_calendar_labels() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid fixture date");

        assert_eq!(session_timestamp_label(today, 18, 1, today), "Today, 18:01");
        assert_eq!(
            session_timestamp_label(
                NaiveDate::from_ymd_opt(2026, 7, 19).expect("valid fixture date"),
                9,
                43,
                today,
            ),
            "Yesterday, 09:43"
        );
        assert_eq!(
            session_timestamp_label(
                NaiveDate::from_ymd_opt(2026, 6, 4).expect("valid fixture date"),
                7,
                5,
                today,
            ),
            "Jun 4, 07:05"
        );
        assert_eq!(
            session_timestamp_label(
                NaiveDate::from_ymd_opt(2025, 12, 31).expect("valid fixture date"),
                23,
                59,
                today,
            ),
            "Dec 31, 2025, 23:59"
        );
    }

    #[test]
    fn completion_uses_the_provider_specific_sigil() {
        assert_eq!(
            completion_query("$rev", 4, completion_sigil(AgentProvider::Codex)),
            Some(CompletionQuery {
                needle: "rev".to_owned(),
                replacement: 0..4,
                sigil: '$',
            })
        );
        assert_eq!(
            completion_query(
                "please /rev",
                11,
                completion_sigil(AgentProvider::ClaudeCode),
            ),
            Some(CompletionQuery {
                needle: "rev".to_owned(),
                replacement: 7..11,
                sigil: '/',
            })
        );
        assert!(completion_query("/rev", 4, completion_sigil(AgentProvider::Codex)).is_none());
        assert!(
            completion_query(
                "https://zed.dev",
                15,
                completion_sigil(AgentProvider::ClaudeCode)
            )
            .is_none()
        );
        assert!(
            completion_query(
                "/review branch",
                14,
                completion_sigil(AgentProvider::ClaudeCode),
            )
            .is_none()
        );
    }

    #[test]
    fn completion_matching_supports_skill_names_without_the_dollar_prefix() {
        assert_eq!(completion_score("brainstorm", "brain"), Some(1));
        assert_eq!(completion_score("gh-address-comments", "gac"), Some(3));
        assert_eq!(completion_score("review", "xyz"), None);
    }

    #[test]
    fn completion_results_keep_every_available_command() {
        let commands = (0..16)
            .map(|index| command(&format!("command-{index:02}")))
            .collect::<Vec<_>>();
        let query = completion_query("$", 1, '$').expect("command completion query");

        let completions = ranked_completions(&commands, &query);

        assert_eq!(completions.len(), commands.len());
    }

    #[test]
    fn command_hints_follow_the_provider_sigil() {
        let command = AgentCommand {
            input_hint: Some("optional context".to_owned()),
            ..command("review")
        };

        assert_eq!(
            active_command_hint(
                "$review ",
                AgentProvider::Codex,
                std::slice::from_ref(&command)
            ),
            Some("Argument · optional context".to_owned())
        );
        assert_eq!(
            active_command_hint("/review ", AgentProvider::Codex, &[command]),
            None
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn non_append_timeline_rebuild_clears_retained_store(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx));
            let history_input = cx.new(|cx| InputState::new(window, cx));
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let controller =
                cx.new(|_| AgentController::new(crate::config::AgentConfig::default()));
            let timeline_store = cx.new(|_| AgentTimelineStore::default());
            timeline_store.update(cx, |store, cx| {
                assert!(!store.expanded(1, DisclosureKind::Tool, false));
                assert!(!store.expanded(1, DisclosureKind::Group, false));
                store.toggle_expanded(1, DisclosureKind::Tool, false, cx);
                store.toggle_expanded(1, DisclosureKind::Group, false, cx);
                store.markdown(1, MarkdownSlot::Body, "old session".into(), cx);
            });
            let timeline_entries = [
                AgentThreadEntry::User {
                    id: 1,
                    markdown: "old session".to_owned(),
                    images: Vec::new(),
                },
                AgentThreadEntry::Assistant {
                    id: 2,
                    markdown: "old response".to_owned(),
                    memory_citations: Vec::new(),
                },
            ];

            let timeline_scroll = ListState::new(2, ListAlignment::Top, px(1_200.0));
            AgentView {
                pane: PaneId(7),
                mux,
                controller,
                input,
                history_input,
                visible: false,
                pane_state: disconnected_pane_state(),
                timeline: TimelineModel::new(&timeline_entries, &[1, 2]),
                timeline_store,
                timeline_next_revision: 2,
                stick: TimelineStick::new(&timeline_scroll, false),
                timeline_scroll,
                completion_scroll: UniformListScrollHandle::new(),
                submission_error: None,
                dismissed_notifications: BTreeSet::new(),
                attachments: Vec::new(),
                completions: Arc::from([]),
                completion_selected: None,
                completion_dismissed: false,
                last_input: String::new(),
                last_cursor: 0,
                history_open: false,
                history_all_projects: false,
                history_results: Arc::from([]),
                history_selected: None,
                history_scroll: UniformListScrollHandle::new(),
                history_delete_confirmation: None,
                last_history_query: String::new(),
                directory_picker: None,
                window_corners: WindowCorners::NONE,
                _subscriptions: Vec::new(),
            }
        });
        let replacement = [AgentThreadEntry::User {
            id: 1,
            markdown: "new session".to_owned(),
            images: Vec::new(),
        }];
        let (changed, cleared) = cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                let (changed, store_update) = view.synchronize_timeline(&replacement, &[3], false);
                let cleared = view.update_timeline_store(store_update, cx);
                (changed, cleared)
            })
        });

        assert!(changed);
        assert!(cleared);
        let timeline_store = cx.update(|_, cx| view.read(cx).timeline_store.clone());
        assert!(!cx.update(|_, cx| {
            timeline_store.update(cx, |store, _| {
                store.expanded(1, DisclosureKind::Tool, false)
            })
        }));
        assert!(!cx.update(|_, cx| {
            timeline_store.update(cx, |store, _| {
                store.expanded(1, DisclosureKind::Group, false)
            })
        }));
    }

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn arrow_actions_navigate_completions_before_the_multiline_input(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let view_slot = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&view_slot);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .auto_grow(2, 8)
                    .default_value("$")
            });
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let controller =
                cx.new(|_| AgentController::new(crate::config::AgentConfig::default()));
            let history_input = cx.new(|cx| InputState::new(window, cx));
            let timeline_scroll = ListState::new(0, ListAlignment::Top, px(1_200.0));
            let view = cx.new(|view_cx| AgentView {
                pane: PaneId(7),
                mux,
                controller,
                input,
                history_input,
                visible: false,
                pane_state: disconnected_pane_state(),
                timeline: TimelineModel::default(),
                timeline_store: view_cx.new(|_| AgentTimelineStore::default()),
                timeline_next_revision: 0,
                stick: TimelineStick::new(&timeline_scroll, false),
                timeline_scroll,
                completion_scroll: UniformListScrollHandle::new(),
                submission_error: None,
                dismissed_notifications: BTreeSet::new(),
                attachments: Vec::new(),
                completions: vec![
                    CommandCompletion {
                        command: command("first"),
                        replacement: 0..1,
                        sigil: '$',
                    },
                    CommandCompletion {
                        command: command("second"),
                        replacement: 0..1,
                        sigil: '$',
                    },
                ]
                .into(),
                completion_selected: Some(0),
                completion_dismissed: false,
                last_input: "$".to_owned(),
                last_cursor: 1,
                history_open: false,
                history_all_projects: false,
                history_results: Arc::from([]),
                history_selected: None,
                history_scroll: UniformListScrollHandle::new(),
                history_delete_confirmation: None,
                last_history_query: String::new(),
                directory_picker: None,
                window_corners: WindowCorners::NONE,
                _subscriptions: Vec::new(),
            });
            view.read(cx).focus(cx).focus(window, cx);
            captured.replace(Some(view.clone()));
            Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        let view = view_slot.borrow().clone().expect("captured agent view");

        cx.simulate_keystrokes("down");
        assert_eq!(
            cx.update(|_, cx| view.read(cx).completion_selected),
            Some(1)
        );

        cx.simulate_keystrokes("up");
        assert_eq!(
            cx.update(|_, cx| view.read(cx).completion_selected),
            Some(0)
        );

        cx.simulate_keystrokes("tab");
        assert_eq!(
            cx.update(|_, cx| view.read(cx).input.read(cx).value().to_string()),
            "$first "
        );
        assert!(cx.update(|_, cx| view.read(cx).completions.is_empty()));
    }

    #[test]
    fn placeholder_only_command_descriptions_are_hidden() {
        assert_eq!(meaningful_command_description("..."), None);
        assert_eq!(meaningful_command_description(" … "), None);
        assert_eq!(
            meaningful_command_description(" Review the current diff "),
            Some("Review the current diff")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread_tool(id: u64, label: &str, status: AgentToolStatusModel) -> AgentThreadEntry {
        AgentThreadEntry::Tool {
            id,
            protocol_id: format!("tool-{id}"),
            kind: AgentToolKindModel::Edit,
            status,
            label: label.to_owned(),
            location: None,
            input: None,
            output: Vec::new(),
            default_expanded: false,
            subagent: false,
            children: Vec::new(),
        }
    }

    fn thread_reasoning(id: u64, markdown: &str) -> AgentThreadEntry {
        AgentThreadEntry::Reasoning {
            id,
            label: "Reasoning".to_owned(),
            markdown: markdown.to_owned(),
            default_expanded: false,
        }
    }

    fn thread_notification(id: u64, summary: &str) -> AgentThreadEntry {
        AgentThreadEntry::Notification {
            id,
            task_id: format!("task-{id}"),
            tool_use_id: format!("toolu_{id}"),
            status: "completed".to_owned(),
            summary: summary.to_owned(),
            result_markdown: "details".to_owned(),
        }
    }

    #[test]
    fn sticky_notifications_follow_prompt_position_and_per_row_dismissal() {
        let entries = [
            thread_notification(1, "first completed"),
            AgentThreadEntry::Assistant {
                id: 2,
                markdown: "answer".to_owned(),
                memory_citations: Vec::new(),
            },
            thread_notification(3, "second completed"),
        ];
        let timeline = TimelineModel::new(&entries, &[1, 1, 1]);
        let mut dismissed = BTreeSet::new();

        assert_eq!(sticky_agent_rows(&timeline.rows, &dismissed).len(), 2);
        dismissed.insert(1);
        assert!(matches!(
            sticky_agent_rows(&timeline.rows, &dismissed).as_slice(),
            [StickyAgentRow::Notification { id: 3, .. }]
        ));

        let with_prompt = [
            thread_notification(1, "first completed"),
            AgentThreadEntry::User {
                id: 2,
                markdown: "next prompt".to_owned(),
                images: Vec::new(),
            },
            thread_notification(3, "second completed"),
        ];
        let timeline = TimelineModel::new(&with_prompt, &[1, 1, 1]);
        assert!(matches!(
            sticky_agent_rows(&timeline.rows, &BTreeSet::new()).as_slice(),
            [StickyAgentRow::Notification { id: 3, .. }]
        ));
        assert!(sticky_strip_clearance(0).abs() < f32::EPSILON);
        assert!(sticky_strip_clearance(2) > sticky_strip_clearance(1));
    }

    #[test]
    fn sticky_subagent_rows_follow_the_root_tool_lifecycle() {
        let mut task = thread_tool(1, "Research", AgentToolStatusModel::Running);
        let AgentThreadEntry::Tool { subagent, .. } = &mut task else {
            unreachable!();
        };
        *subagent = true;
        let running = TimelineModel::new(&[task.clone()], &[1]);
        assert!(matches!(
            sticky_agent_rows(&running.rows, &BTreeSet::new()).as_slice(),
            [StickyAgentRow::Subagent {
                id: 1,
                status: AgentToolStatus::Running,
                ..
            }]
        ));

        let AgentThreadEntry::Tool { status, .. } = &mut task else {
            unreachable!();
        };
        *status = AgentToolStatusModel::Completed;
        let completed = TimelineModel::new(&[task], &[2]);
        assert!(sticky_agent_rows(&completed.rows, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn reasoning_runs_fold_into_one_row_without_absorbing_other_kinds() {
        let mut entries = vec![
            thread_reasoning(1, "first"),
            thread_reasoning(2, "second"),
            thread_tool(3, "Editing files", AgentToolStatusModel::Completed),
        ];
        let mut revisions = vec![1, 1, 1];
        let mut timeline = TimelineModel::new(&entries, &revisions);
        assert_eq!(timeline.entry_to_row, [0, 0, 1]);
        assert!(matches!(
            &timeline.rows[0],
            TimelineRow::Group { entries, .. } if entries.len() == 2
        ));
        assert!(matches!(&timeline.rows[1], TimelineRow::Single(_)));

        entries.push(thread_reasoning(4, "third"));
        entries.push(thread_reasoning(5, "fourth"));
        revisions.extend([1, 1]);
        let TimelineModelUpdate::Incremental { splice_start, .. } =
            timeline.synchronize(&entries, &revisions)
        else {
            panic!("appending reasoning after a tool should splice a row");
        };
        assert_eq!(splice_start, 2);
        assert_eq!(timeline.entry_to_row, [0, 0, 1, 2, 2]);

        entries[3] = thread_reasoning(4, "third, continued");
        revisions[3] = 2;
        let TimelineModelUpdate::Incremental {
            remeasure_rows,
            added_rows,
            ..
        } = timeline.synchronize(&entries, &revisions)
        else {
            panic!("a member revision should update its owning group row");
        };
        assert_eq!(remeasure_rows, [2]);
        assert_eq!(added_rows, 0);
    }

    #[test]
    fn timeline_model_updates_trailing_groups_and_maps_member_revisions() {
        let mut entries = vec![
            thread_tool(1, "Editing files", AgentToolStatusModel::Completed),
            thread_tool(2, "Editing files", AgentToolStatusModel::Running),
            thread_tool(3, "Editing files", AgentToolStatusModel::Completed),
        ];
        let mut revisions = vec![1, 1, 1];
        let mut timeline = TimelineModel::new(&entries, &revisions);
        assert_eq!(timeline.rows.len(), 1);
        assert_eq!(timeline.entry_to_row, [0, 0, 0]);

        entries.push(thread_tool(
            4,
            "Editing files",
            AgentToolStatusModel::Pending,
        ));
        revisions.push(1);
        let TimelineModelUpdate::Incremental {
            store_entries,
            remeasure_rows,
            splice_start,
            added_rows,
        } = timeline.synchronize(&entries, &revisions)
        else {
            panic!("tool append should update the trailing group");
        };
        assert!(store_entries.is_empty());
        assert_eq!(remeasure_rows, [0]);
        assert_eq!(splice_start, 1);
        assert_eq!(added_rows, 0);
        assert_eq!(timeline.rows.len(), 1);
        assert_eq!(timeline.entry_to_row, [0, 0, 0, 0]);
        assert!(matches!(
            &timeline.rows[0],
            TimelineRow::Group { entries, .. } if entries.len() == 4
        ));

        entries.push(thread_tool(
            5,
            "Running command",
            AgentToolStatusModel::Completed,
        ));
        revisions.push(1);
        let TimelineModelUpdate::Incremental {
            remeasure_rows,
            splice_start,
            added_rows,
            ..
        } = timeline.synchronize(&entries, &revisions)
        else {
            panic!("different-label tool append should update the trailing group");
        };
        assert_eq!(remeasure_rows, [0]);
        assert_eq!(splice_start, 1);
        assert_eq!(added_rows, 0);
        assert_eq!(timeline.rows.len(), 1);
        assert_eq!(timeline.entry_to_row, [0, 0, 0, 0, 0]);

        entries[1] = thread_tool(2, "Editing files", AgentToolStatusModel::Failed);
        revisions[1] = 2;
        let TimelineModelUpdate::Incremental {
            store_entries,
            remeasure_rows,
            added_rows,
            ..
        } = timeline.synchronize(&entries, &revisions)
        else {
            panic!("member revision should update its owning row");
        };
        assert_eq!(remeasure_rows, [0]);
        assert_eq!(added_rows, 0);
        assert_eq!(store_entries.len(), 1);
        assert_eq!(store_entries[0].id(), 2);
        assert!(matches!(
            timeline.rows[0].entry(2),
            Some(AgentEntry::Tool(AgentToolEntry {
                status: AgentToolStatus::Failed,
                ..
            }))
        ));

        entries[1] = thread_tool(2, "Renamed tool", AgentToolStatusModel::Failed);
        revisions[1] = 3;
        let TimelineModelUpdate::Incremental {
            store_entries,
            remeasure_rows,
            added_rows,
            ..
        } = timeline.synchronize(&entries, &revisions)
        else {
            panic!("tool label changes should preserve contiguous grouping");
        };
        assert_eq!(remeasure_rows, [0]);
        assert_eq!(added_rows, 0);
        assert_eq!(store_entries.len(), 1);
        assert_eq!(timeline.entry_to_row, [0, 0, 0, 0, 0]);
    }

    #[test]
    fn tool_entry_adapter_preserves_typed_payloads() {
        let entry = ui_entry(&AgentThreadEntry::Tool {
            id: 1,
            protocol_id: "tool".to_owned(),
            kind: AgentToolKindModel::Execute,
            status: AgentToolStatusModel::Completed,
            label: "run".to_owned(),
            location: Some("/workspace/src/lib.rs:9".to_owned()),
            input: Some(ToolPayload::Json("{\n  \"check\": true\n}".to_owned())),
            output: vec![
                ToolPayload::Diff {
                    path: "/workspace/src/lib.rs".to_owned(),
                    old: Some("old".to_owned()),
                    new: "new".to_owned(),
                },
                ToolPayload::Text("done".to_owned()),
                ToolPayload::Terminal("$ cargo check\nok\n[exit status: 0]".to_owned()),
            ],
            default_expanded: false,
            subagent: false,
            children: Vec::new(),
        });
        let AgentEntry::Tool(AgentToolEntry {
            location,
            input,
            output,
            ..
        }) = entry
        else {
            panic!("tool output should be present");
        };

        assert_eq!(location.as_deref(), Some("/workspace/src/lib.rs:9"));
        assert!(matches!(input, Some(AgentToolPayload::Json(json)) if json.contains("check")));
        assert!(matches!(
            output.as_ref(),
            [
                AgentToolPayload::Diff { path, old: Some(old), new },
                AgentToolPayload::Text(text),
                AgentToolPayload::Terminal(terminal),
            ]
                if path == "/workspace/src/lib.rs"
                    && old == "old"
                    && new == "new"
                    && text == "done"
                    && terminal.contains("exit status")
        ));
    }

    #[test]
    fn live_entry_adapter_covers_each_core_shape() {
        let entries = ui_entries(&[
            AgentThreadEntry::User {
                id: 1,
                markdown: "user".to_owned(),
                images: Vec::new(),
            },
            AgentThreadEntry::Assistant {
                id: 2,
                markdown: "assistant".to_owned(),
                memory_citations: Vec::new(),
            },
            AgentThreadEntry::Reasoning {
                id: 3,
                label: "reasoning".to_owned(),
                markdown: "thought".to_owned(),
                default_expanded: false,
            },
            AgentThreadEntry::Tool {
                id: 4,
                protocol_id: "tool".to_owned(),
                kind: AgentToolKindModel::Execute,
                status: AgentToolStatusModel::Running,
                label: "run".to_owned(),
                location: None,
                input: None,
                output: Vec::new(),
                default_expanded: false,
                subagent: false,
                children: Vec::new(),
            },
            AgentThreadEntry::Plan {
                id: 5,
                markdown: "- [ ] plan".to_owned(),
            },
            AgentThreadEntry::Notification {
                id: 6,
                task_id: "task".to_owned(),
                tool_use_id: "toolu_6".to_owned(),
                status: "completed".to_owned(),
                summary: "background task finished".to_owned(),
                result_markdown: "done".to_owned(),
            },
        ]);

        assert_eq!(entries.len(), 6);
        assert!(matches!(entries[0], AgentEntry::User { .. }));
        assert!(matches!(entries[1], AgentEntry::Assistant { .. }));
        assert!(matches!(entries[2], AgentEntry::Reasoning { .. }));
        assert!(matches!(entries[3], AgentEntry::Tool(_)));
        assert!(matches!(entries[4], AgentEntry::Plan { .. }));
        assert!(matches!(entries[5], AgentEntry::Notification { .. }));
    }

    #[test]
    fn user_attachments_reach_the_timeline_without_a_second_copy() {
        let image = Arc::new(Image::from_bytes(gpui::ImageFormat::Png, vec![0x89, 0x50]));
        let entries = ui_entries(&[AgentThreadEntry::User {
            id: 1,
            markdown: String::new(),
            images: vec![Arc::clone(&image)],
        }]);

        let AgentEntry::User { images, .. } = &entries[0] else {
            panic!("the adapter should keep this a user entry");
        };
        assert_eq!(images.len(), 1);
        assert!(
            Arc::ptr_eq(&images[0], &image),
            "the bytes should travel by handle, not by clone, so gpui decodes them once"
        );
    }
}
