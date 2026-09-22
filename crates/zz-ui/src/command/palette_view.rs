use std::{ops::Range, rc::Rc, sync::Arc, time::Duration};

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Global, IntoElement, KeyDownEvent,
    MouseButton, Render, ScrollStrategy, SharedString, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};
use zz_client::completion::{
    CompletionKind, CompletionSuggestion, PaneKindAvailability, apply_completion, complete_command,
    completion_insertion,
};
use zz_protocol::{
    ChooseTreeAction, ChooseTreeState, ChooseTreeTarget, CommandInvocation, CommandPromptAction,
    CommandPromptKind, CommandPromptMode, CommandPromptState, CommandPromptType, InputMessage,
    MAX_COMMAND_PROMPT_BYTES, MuxSnapshot, PaneId,
};

use super::{
    COMMAND_PALETTE_ROW_HEIGHT, CommandPaletteSurface, PaletteHint, PaletteRow, command_kind_badge,
    command_palette_empty, command_palette_input, command_palette_row, command_palette_section,
    command_palette_tree_entry,
    palette_model::{
        PaletteAction, PaletteEntry, PaletteHostId, PaletteMode, PaletteSettings, PaletteTarget,
        PaletteTree, UnifiedPalette, needs_arguments, target_kind,
    },
    unified_command_palette_input,
};
use crate::{
    WindowExt as _,
    input::{
        Backspace, Escape, IndentInline, InputEvent, InputState, MoveDown, MoveLeft, MoveRight,
        MoveUp,
    },
    notification::Notification,
};

pub trait PaletteBackend {
    fn snapshot(&self, cx: &App) -> Arc<MuxSnapshot>;
    fn host_snapshot<'a>(&self, host: PaletteHostId, cx: &'a App) -> Option<&'a MuxSnapshot>;
    fn tree(&self, cx: &App) -> PaletteTree;
    fn settings(&self, cx: &App) -> PaletteSettings;
    fn availability(&self, cx: &App) -> PaneKindAvailability;
    fn command_shortcut(&self, command: &str, cx: &App) -> Option<SharedString>;
    fn mono_font(&self, cx: &App) -> SharedString;
    fn send_input(&self, input: InputMessage, cx: &mut App);
    fn send_key(&self, pane: PaneId, event: &KeyDownEvent, cx: &mut App);
    fn active_pane(&self, cx: &App) -> Option<PaneId>;
    fn execute(&self, host: PaletteHostId, command: CommandInvocation, cx: &mut App);
    fn connect_host(&self, host: PaletteHostId, cx: &mut App);
    fn activate(&self, host: PaletteHostId, target: PaletteTarget, cx: &mut App) -> bool;
}

const MAX_VISIBLE_ROWS: usize = 8;

#[derive(Default)]
struct RecentPaletteCommands(Vec<String>);

impl Global for RecentPaletteCommands {}

pub enum CommandPaletteEvent {
    ReturnToDefault,
}

impl EventEmitter<CommandPaletteEvent> for CommandPaletteView {}

pub struct CommandPaletteView {
    backend: Rc<dyn PaletteBackend>,
    input: Entity<InputState>,
    prompt: String,
    kind: CommandPromptKind,
    mode: CommandPromptMode,
    history: Vec<String>,
    snapshot: Arc<MuxSnapshot>,
    revision: u64,
    suggestions: Vec<CompletionSuggestion>,
    selected: Option<usize>,
    navigation_engaged: bool,
    scroll_handle: UniformListScrollHandle,
    last_input: String,
    last_cursor: usize,
    finishing: bool,
    availability: PaneKindAvailability,
    unified: Option<UnifiedPalette>,
    window_chooser: Option<ChooseTreeState>,
    chooser_search: ChooserSearch,
    chooser_activation: Option<ChooseTreeTarget>,
    local: bool,
    changing_input: bool,
}

#[derive(Default)]
struct ChooserSearch {
    expanded: Vec<ChooseTreeTarget>,
    pending: Option<(ChooseTreeTarget, bool)>,
    selection: Option<ChooseTreeTarget>,
}

impl ChooserSearch {
    fn advance(&mut self, state: &ChooseTreeState, searching: bool) -> Option<(u32, bool)> {
        if let Some((target, expanded)) = self.pending {
            if state.items.iter().any(|item| {
                item.target == target
                    && item.expanded() != expanded
                    && (!expanded || item.has_children())
            }) {
                return None;
            }
            self.pending = None;
        }
        let next = if searching {
            state.items.iter().enumerate().find_map(|(index, item)| {
                (item.has_children() && !item.expanded()).then_some((index, item.target, true))
            })
        } else {
            self.expanded.iter().rev().find_map(|target| {
                state.items.iter().enumerate().find_map(|(index, item)| {
                    (item.target == *target && item.expanded()).then_some((index, *target, false))
                })
            })
        };
        let Some((index, target, expanded)) = next else {
            if !searching {
                self.expanded.clear();
            }
            return None;
        };
        if expanded && !self.expanded.contains(&target) {
            self.expanded.push(target);
        }
        self.pending = Some((target, expanded));
        Some((index as u32, expanded))
    }
}

impl CommandPaletteView {
    pub fn new(
        backend: Rc<dyn PaletteBackend>,
        state: &CommandPromptState,
        revision: u64,
        snapshot: Arc<MuxSnapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = match state.kind {
            CommandPromptKind::Command => "Type a zz command…",
            CommandPromptKind::Value => "Enter a value…",
        };
        let initial_cursor =
            byte_index_for_char(&state.input, state.cursor).unwrap_or(state.input.len());
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(state.input.clone())
                .context_menu(true)
                .validate(|value, _| value.len() <= MAX_COMMAND_PROMPT_BYTES)
        });
        input.update(cx, |input, cx| {
            input.set_selected_range(initial_cursor..initial_cursor, cx);
        });

        cx.observe_in(&input, window, |palette, input, window, cx| {
            palette.synchronize_local_input(&input, window, cx);
        })
        .detach();
        cx.subscribe_in(
            &input,
            window,
            |palette, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    palette.enter(window, cx);
                }
            },
        )
        .detach();

        let availability = backend.availability(cx);
        let mut palette = Self {
            backend,
            input,
            prompt: state.prompt.clone(),
            kind: state.kind,
            mode: state.mode,
            history: state.history.clone(),
            snapshot,
            revision,
            suggestions: Vec::new(),
            selected: None,
            navigation_engaged: false,
            scroll_handle: UniformListScrollHandle::new(),
            last_input: state.input.clone(),
            last_cursor: initial_cursor,
            finishing: false,
            availability,
            unified: (state.kind == CommandPromptKind::Command
                && state.mode == CommandPromptMode::Text
                && state.prompt.trim() == ":")
                .then(|| UnifiedPalette::new(Some(PaletteMode::Command))),
            local: false,
            window_chooser: None,
            chooser_search: ChooserSearch::default(),
            chooser_activation: None,
            changing_input: false,
        };
        palette.recompute_suggestions();
        palette.refresh(window, cx);
        palette
    }

    pub fn new_unified(
        backend: Rc<dyn PaletteBackend>,
        mode: Option<PaletteMode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = backend.snapshot(cx);
        let state = CommandPromptState {
            prompt: ":".to_owned(),
            input: String::new(),
            cursor: 0,
            kind: CommandPromptKind::Command,
            mode: CommandPromptMode::Text,
            history: cx
                .try_global::<RecentPaletteCommands>()
                .map_or_else(Vec::new, |history| history.0.clone()),
            prompt_type: CommandPromptType::Command,
            no_freeze: false,
            pane: None,
        };
        let mut palette = Self::new(backend, &state, 0, snapshot, window, cx);
        palette.local = true;
        palette.unified = Some(UnifiedPalette::new(mode));
        palette.selected = None;
        palette.refresh(window, cx);
        palette
    }

    pub const fn is_local(&self) -> bool {
        self.local
    }

    pub const fn is_window_chooser(&self) -> bool {
        self.window_chooser.is_some()
    }

    pub fn return_to_default(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finishing || self.window_chooser.take().is_none() {
            return;
        }
        self.local = true;
        self.chooser_search = ChooserSearch::default();
        self.chooser_activation = None;
        self.unified = Some(UnifiedPalette::new(None));
        self.selected = None;
        self.navigation_engaged = false;
        self.send_tree(ChooseTreeAction::Close, cx);
        self.refresh(window, cx);
        cx.notify();
    }

    pub fn new_window_chooser(
        backend: Rc<dyn PaletteBackend>,
        state: &ChooseTreeState,
        revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut palette = Self::new_unified(backend, Some(PaletteMode::Window), window, cx);
        palette.local = false;
        palette.synchronize_window_chooser(state, revision, window, cx);
        palette
    }

    pub fn synchronize_window_chooser(
        &mut self,
        state: &ChooseTreeState,
        revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.window_chooser.is_some() && self.revision == revision {
            return;
        }
        let target = self.window_chooser.as_ref().and_then(|state| {
            let entry = self.unified.as_ref()?.rows.get(self.selected?)?;
            let Some(PaletteAction::ChooseWindow(index)) = entry.action else {
                return None;
            };
            state.items.get(index as usize).map(|item| item.target)
        });
        let target = target.or_else(|| {
            state
                .items
                .get(state.selected as usize)
                .map(|item| item.target)
        });
        self.window_chooser = Some(state.clone());
        self.revision = revision;
        self.finishing = false;
        self.refresh(window, cx);
        if let Some(target) = target {
            self.selected = self.unified.as_ref().and_then(|unified| {
                unified.rows.iter().position(|entry| {
                    matches!(entry.action, Some(PaletteAction::ChooseWindow(index))
                        if state.items.get(index as usize).is_some_and(|item| item.target == target))
                })
            }).or(self.selected);
        }
        if let Some(selected) = self.selected {
            self.scroll_handle
                .scroll_to_item(selected, ScrollStrategy::Nearest);
        }
        self.advance_chooser_search(cx);
        cx.notify();
    }

    fn advance_chooser_search(&mut self, cx: &mut Context<Self>) {
        let Some(state) = &self.window_chooser else {
            return;
        };
        if self.finishing {
            return;
        }
        let searching = !self.last_input.trim().is_empty();
        if searching && self.chooser_search.selection.is_none() {
            self.chooser_search.selection = state
                .items
                .get(state.selected as usize)
                .map(|item| item.target);
        }
        if self.chooser_activation.is_some() {
            if let Some((target, expanded)) = self.chooser_search.pending
                && state.items.iter().any(|item| {
                    item.target == target
                        && item.expanded() != expanded
                        && (!expanded || item.has_children())
                })
            {
                return;
            }
            if let Some(target) = self.chooser_activation.take()
                && let Some(index) = state.items.iter().position(|item| item.target == target)
            {
                self.finishing = true;
                self.send_tree(ChooseTreeAction::ActivateIndex(index as u32), cx);
                cx.notify();
                return;
            }
        }
        if let Some((index, expand)) = self.chooser_search.advance(state, searching) {
            self.send_tree(ChooseTreeAction::Select(index), cx);
            self.send_tree(
                if expand {
                    ChooseTreeAction::Expand
                } else {
                    ChooseTreeAction::Collapse
                },
                cx,
            );
        } else if !searching
            && self.chooser_search.pending.is_none()
            && let Some(target) = self.chooser_search.selection.take()
        {
            self.selected = self.unified.as_ref().and_then(|unified| {
                unified.rows.iter().position(|entry| {
                    matches!(entry.action, Some(PaletteAction::ChooseWindow(index))
                        if state.items.get(index as usize).is_some_and(|item| item.target == target))
                })
            }).or(self.selected);
            if let Some(selected) = self.selected {
                self.scroll_handle
                    .scroll_to_item(selected, ScrollStrategy::Nearest);
            }
        }
    }

    fn send_tree(&self, action: ChooseTreeAction, cx: &mut App) {
        self.backend
            .send_input(InputMessage::ChooseTree { action }, cx);
    }

    pub const fn is_finished(&self) -> bool {
        self.finishing
    }

    pub fn refresh(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(unified) = &mut self.unified else {
            return;
        };
        let selected_node = self
            .selected
            .and_then(|index| unified.rows.get(index))
            .and_then(|entry| match entry.action {
                Some(PaletteAction::Target { host, target, .. }) => Some((host, Some(target))),
                Some(PaletteAction::Host(host)) => Some((host, None)),
                _ => None,
            });
        unified.apply_settings(self.backend.settings(cx));
        self.availability = self.backend.availability(cx);
        self.snapshot = self.backend.snapshot(cx);
        let mut history = cx
            .try_global::<RecentPaletteCommands>()
            .map_or_else(Vec::new, |history| history.0.clone());
        for entry in &self.history {
            if !history.contains(entry) {
                history.push(entry.clone());
            }
        }
        if let Some(state) = &self.window_chooser {
            unified.rebuild_window_chooser(state, &self.last_input, &self.snapshot);
        } else {
            let backend = Rc::clone(&self.backend);
            unified.rebuild(
                &self.last_input,
                &history,
                self.backend.tree(cx),
                self.availability,
                &|name| backend.command_shortcut(name, cx),
            );
        }
        if unified.mode == Some(PaletteMode::Command)
            && unified.command.is_none()
            && has_command_arguments(&self.last_input)
        {
            let snapshot = unified
                .host
                .and_then(|host| self.backend.host_snapshot(host, cx))
                .unwrap_or(&self.snapshot);
            unified.rows = complete_command(
                &self.last_input,
                self.last_cursor,
                &history,
                snapshot,
                self.availability,
            )
            .into_iter()
            .map(|suggestion| PaletteEntry {
                row: PaletteRow {
                    label: if suggestion.kind == CompletionKind::Value
                        && suggestion.label.starts_with(['$', '@', '%'])
                    {
                        suggestion.detail.clone()
                    } else {
                        suggestion.label.clone()
                    }
                    .into(),
                    detail: if suggestion.kind == CompletionKind::Value
                        && suggestion.label.starts_with(['$', '@', '%'])
                    {
                        String::new()
                    } else {
                        suggestion.detail.clone()
                    }
                    .into(),
                    ..Default::default()
                },
                action: Some(PaletteAction::Completion(suggestion)),
                hint: "",
                score: 0,
            })
            .collect();
        }
        let placeholder = unified.placeholder();
        if let Some(node) = selected_node {
            self.selected = unified.rows.iter().position(|entry| match entry.action {
                Some(PaletteAction::Target { host, target, .. }) => node == (host, Some(target)),
                Some(PaletteAction::Host(host)) => node == (host, None),
                _ => false,
            });
        }
        self.selected = self
            .selected
            .filter(|index| {
                unified
                    .rows
                    .get(*index)
                    .is_some_and(|row| row.action.is_some())
            })
            .or_else(|| {
                if self.last_input.trim().is_empty() {
                    unified.initial_selection()
                } else {
                    unified.rows.iter().position(|row| row.action.is_some())
                }
            });
        if !self.navigation_engaged {
            self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        }
        self.input
            .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
    }

    pub fn focus(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }

    pub fn synchronize(
        &mut self,
        state: &CommandPromptState,
        revision: u64,
        snapshot: &Arc<MuxSnapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot_changed = self.snapshot.generation != snapshot.generation;
        if snapshot_changed {
            self.snapshot = Arc::clone(snapshot);
        }
        if self.revision == revision {
            if snapshot_changed {
                self.recompute_suggestions();
                self.refresh(window, cx);
                cx.notify();
            }
            return;
        }

        self.revision = revision;
        self.prompt.clone_from(&state.prompt);
        self.kind = state.kind;
        self.mode = state.mode;
        self.local = false;
        self.unified = (state.kind == CommandPromptKind::Command
            && state.mode == CommandPromptMode::Text
            && state.prompt.trim() == ":")
            .then(|| UnifiedPalette::new(Some(PaletteMode::Command)));
        self.history.clone_from(&state.history);
        self.finishing = false;
        self.navigation_engaged = false;
        let cursor = byte_index_for_char(&state.input, state.cursor).unwrap_or(state.input.len());
        let current = self.input.read(cx).value().to_string();
        self.input.update(cx, |input, cx| {
            if self.unified.is_none() {
                input.set_placeholder(
                    match state.kind {
                        CommandPromptKind::Command => "Type a zz command…",
                        CommandPromptKind::Value => "Enter a value…",
                    },
                    cx,
                );
            }
            if current != state.input {
                input.set_value(state.input.clone(), window, cx);
            }
            input.set_selected_range(cursor..cursor, cx);
        });
        self.last_input.clone_from(&state.input);
        self.last_cursor = cursor;
        self.recompute_suggestions();
        self.refresh(window, cx);
        cx.notify();
    }

    fn synchronize_local_input(
        &mut self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.changing_input {
            return;
        }
        let (value, cursor) = {
            let input = input.read(cx);
            (input.value().to_string(), input.cursor())
        };
        if value == self.last_input && cursor == self.last_cursor {
            return;
        }
        if self.last_input.trim().is_empty() && !value.trim().is_empty() {
            self.chooser_search.selection = self.window_chooser.as_ref().and_then(|state| {
                let entry = self.unified.as_ref()?.rows.get(self.selected?)?;
                let Some(PaletteAction::ChooseWindow(index)) = entry.action else {
                    return None;
                };
                state.items.get(index as usize).map(|item| item.target)
            });
        }
        if self.last_input.is_empty()
            && let Some(unified) = &mut self.unified
            && unified.mode.is_none()
            && unified.command.is_none()
            && let Some(mode) = PaletteMode::from_prefix(&value, unified.host_prefix)
        {
            unified.mode = Some(mode);
            self.set_query(String::new(), window, cx);
            return;
        }
        self.last_input.clone_from(&value);
        self.last_cursor = cursor;
        self.navigation_engaged = false;
        self.selected = None;
        self.recompute_suggestions();
        self.refresh(window, cx);
        self.advance_chooser_search(cx);
        if !self.finishing && !self.local && self.window_chooser.is_none() {
            let cursor = u32::try_from(value[..cursor].chars().count()).unwrap_or(u32::MAX);
            self.send(
                CommandPromptAction::Update {
                    input: value,
                    cursor,
                },
                cx,
            );
        }
        cx.notify();
    }

    fn recompute_suggestions(&mut self) {
        self.suggestions = if self.kind == CommandPromptKind::Command && self.completes() {
            complete_command(
                &self.last_input,
                self.last_cursor,
                &self.history,
                &self.snapshot,
                self.availability,
            )
        } else {
            Vec::new()
        };
        self.selected = (!self.suggestions.is_empty()).then_some(
            self.selected
                .unwrap_or_default()
                .min(self.suggestions.len().saturating_sub(1)),
        );
    }

    fn set_query(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        self.changing_input = true;
        self.last_input.clone_from(&value);
        self.last_cursor = value.len();
        self.input
            .update(cx, |input, cx| input.set_value(value, window, cx));
        self.changing_input = false;
        self.selected = None;
        self.navigation_engaged = false;
        self.recompute_suggestions();
        self.refresh(window, cx);
        self.focus(cx).focus(window, cx);
        cx.notify();
    }

    fn activate_unified(&mut self, complete: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(unified) = &self.unified else {
            return;
        };
        let raw_command = unified.mode == Some(PaletteMode::Command)
            && unified.command.is_none()
            && has_command_arguments(&self.last_input);
        if raw_command && !complete && !self.navigation_engaged {
            self.run_command(self.last_input.clone(), None, window, cx);
            return;
        }
        let action = self
            .selected
            .and_then(|index| unified.rows.get(index))
            .and_then(|row| row.action.clone());
        let Some(action) = action else {
            if unified.mode == Some(PaletteMode::Command)
                && !self.last_input.trim().is_empty()
                && unified.command.is_none()
            {
                self.run_command(self.last_input.clone(), None, window, cx);
            }
            return;
        };
        match action {
            PaletteAction::ChooseWindow(index) => {
                if self.chooser_search.pending.is_some() {
                    self.chooser_activation = self
                        .window_chooser
                        .as_ref()
                        .and_then(|state| state.items.get(index as usize))
                        .map(|item| item.target);
                    return;
                }
                self.finishing = true;
                self.send_tree(ChooseTreeAction::ActivateIndex(index), cx);
                cx.notify();
            }
            PaletteAction::Command(spec) => {
                if complete {
                    self.set_query(format!("{} ", spec.name), window, cx);
                } else if target_kind(spec).is_some() {
                    if let Some(unified) = &mut self.unified {
                        unified.mode = Some(PaletteMode::Command);
                        unified.command = Some(spec);
                    }
                    self.set_query(String::new(), window, cx);
                } else if needs_arguments(spec) {
                    if let Some(unified) = &mut self.unified {
                        unified.mode = Some(PaletteMode::Command);
                    }
                    self.set_query(format!("{} ", spec.name), window, cx);
                } else {
                    self.run_command(
                        spec.name.to_owned(),
                        Some(CommandInvocation::new(spec.name, [] as [&str; 0])),
                        window,
                        cx,
                    );
                }
            }
            PaletteAction::History(command) => self.run_command(command, None, window, cx),
            PaletteAction::Completion(suggestion) => {
                let (completed, _) = apply_completion(&self.last_input, &suggestion);
                if completed.len() <= MAX_COMMAND_PROMPT_BYTES {
                    self.set_query(completed, window, cx);
                }
            }
            PaletteAction::Host(host) => {
                let connected = self
                    .unified
                    .as_ref()
                    .and_then(|state| state.tree.host(host))
                    .is_some_and(super::PaletteTreeHost::connected);
                if connected {
                    if let Some(unified) = &mut self.unified {
                        unified.host = Some(host);
                        unified.mode = None;
                        unified.command = None;
                    }
                    self.set_query(String::new(), window, cx);
                } else {
                    self.backend.connect_host(host, cx);
                    self.refresh(window, cx);
                    cx.notify();
                }
            }
            PaletteAction::Target {
                host,
                target,
                label,
            } => {
                if let Some(command) = self.unified.as_ref().and_then(|state| state.command) {
                    let target = target.to_string();
                    if let Some(unified) = &mut self.unified {
                        unified.host = Some(host);
                    }
                    self.run_command(
                        format!("{} -t {target}", command.name),
                        Some(CommandInvocation::new(command.name, ["-t", &target])),
                        window,
                        cx,
                    );
                } else if self.backend.activate(host, target, cx) {
                    self.close(cx);
                    Self::notify_action(format!("Switched to {label}"), window, cx);
                }
            }
        }
    }

    fn run_command(
        &mut self,
        input: String,
        command: Option<CommandInvocation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tree = self
            .unified
            .as_ref()
            .map_or_else(|| self.backend.tree(cx), |state| state.tree.clone());
        let host = self
            .unified
            .as_ref()
            .and_then(|state| state.host)
            .unwrap_or(tree.attached);
        if !tree
            .host(host)
            .is_some_and(super::PaletteTreeHost::connected)
        {
            Self::notify_action(
                "Connect to this host before running a command".to_owned(),
                window,
                cx,
            );
            return;
        }
        if !cx.has_global::<RecentPaletteCommands>() {
            cx.set_global(RecentPaletteCommands::default());
        }
        cx.update_global::<RecentPaletteCommands, _>(|history, _| {
            history.0.retain(|entry| entry != &input);
            history.0.insert(0, input.clone());
            history.0.truncate(20);
        });
        let display = self
            .unified
            .as_ref()
            .and_then(|state| state.command)
            .and_then(|command| {
                self.selected
                    .and_then(|index| self.unified.as_ref()?.rows.get(index))
                    .and_then(|row| match &row.action {
                        Some(PaletteAction::Target { label, .. }) => {
                            Some(format!("{} · {label}", command.name))
                        }
                        _ => None,
                    })
            })
            .unwrap_or_else(|| input.split_whitespace().next().unwrap_or(&input).to_owned());
        self.finishing = true;
        if !self.local && self.window_chooser.is_none() && host == tree.attached {
            self.send(CommandPromptAction::Submit { input }, cx);
        } else {
            if self.window_chooser.is_some() {
                self.send_tree(ChooseTreeAction::Close, cx);
            } else if !self.local {
                self.send(CommandPromptAction::Close, cx);
            }
            self.backend.execute(
                host,
                command.unwrap_or_else(|| CommandInvocation::new("if-shell", ["-F", "1", &input])),
                cx,
            );
        }
        Self::notify_action(format!("Ran {display}"), window, cx);
        cx.notify();
    }

    fn notify_action(message: String, window: &mut Window, cx: &mut App) {
        window.push_notification(
            Notification::new()
                .message(message)
                .autohide_after(Duration::from_millis(2600)),
            cx,
        );
    }

    fn render_unified(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(unified) = &self.unified else {
            return div().into_any_element();
        };
        let selected = self.selected;
        let enter_hint = selected
            .and_then(|index| unified.rows.get(index))
            .and_then(|entry| entry.action.as_ref())
            .map_or("choose", |action| match action {
                PaletteAction::Target { target, .. } => match target {
                    PaletteTarget::Session(_) => "switch session",
                    PaletteTarget::Window(_) => "switch window",
                    PaletteTarget::Pane(_) => "focus pane",
                },
                PaletteAction::ChooseWindow(index) => self
                    .window_chooser
                    .as_ref()
                    .and_then(|state| state.items.get(*index as usize))
                    .map_or("switch", |item| match item.target {
                        ChooseTreeTarget::Session(_) => "switch session",
                        ChooseTreeTarget::Window(_) => "switch window",
                        ChooseTreeTarget::Pane(_) => "focus pane",
                        ChooseTreeTarget::Client(_) => "choose",
                    }),
                PaletteAction::Host(_) => "scope host",
                PaletteAction::Command(_) | PaletteAction::History(_) => "run command",
                PaletteAction::Completion(_) => "complete",
            });
        let usage = unified.usage(selected).or_else(|| {
            (unified.mode == Some(PaletteMode::Command))
                .then(|| {
                    self.last_input
                        .split_whitespace()
                        .next()
                        .and_then(zz_protocol::catalog_command_spec)
                        .map(|spec| spec.usage)
                })
                .flatten()
        });
        let entries = Arc::new(unified.rows.clone());
        let palette = cx.entity();
        let rows_palette = palette.clone();
        let available = (f32::from(window.viewport_size().height) * 0.88
            - if usage.is_some() { 175.0 } else { 140.0 })
        .max(40.0);
        let list_height = (entries.len() as f32 * COMMAND_PALETTE_ROW_HEIGHT)
            .min(440.0)
            .min(available);
        let rows = uniform_list(
            "unified-palette-rows",
            entries.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        entries.get(index).map(|entry| {
                            if entry.action.is_none() {
                                return div().h(px(COMMAND_PALETTE_ROW_HEIGHT)).child(
                                    command_palette_section(
                                        entry.row.label.clone(),
                                        entry.hint,
                                        cx,
                                    ),
                                );
                            }
                            let hover_palette = rows_palette.clone();
                            let click_palette = rows_palette.clone();
                            let toggle_palette = rows_palette.clone();
                            let expanded = entry.row.expanded;
                            div().h(px(COMMAND_PALETTE_ROW_HEIGHT)).child(
                                command_palette_tree_entry(
                                    ("unified-palette-row", index),
                                    &entry.row,
                                    selected == Some(index),
                                    move |_, window, cx| {
                                        toggle_palette.update(cx, |palette, cx| {
                                            palette.selected = Some(index);
                                            palette.change_tree_expansion(
                                                !expanded.unwrap_or(false),
                                                window,
                                                cx,
                                            );
                                        });
                                    },
                                    cx,
                                )
                                .on_mouse_enter(move |_, _, cx| {
                                    hover_palette.update(cx, |palette, cx| {
                                        palette.engage_pointer_selection(index, cx);
                                    });
                                })
                                .on_click(move |_, window, cx| {
                                    click_palette.update(cx, |palette, cx| {
                                        palette.selected = Some(index);
                                        palette.navigation_engaged = true;
                                        palette.activate_unified(false, window, cx);
                                    });
                                    cx.stop_propagation();
                                }),
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .h(px(list_height))
        .track_scroll(&self.scroll_handle);
        let mut hints = if unified.command.is_some() {
            vec![
                PaletteHint {
                    key: "up down",
                    label: "navigate",
                },
                PaletteHint {
                    key: "enter",
                    label: "run",
                },
                PaletteHint {
                    key: "backspace",
                    label: "back",
                },
            ]
        } else {
            match unified.mode {
                Some(PaletteMode::Command) => vec![
                    PaletteHint {
                        key: "tab",
                        label: "complete",
                    },
                    PaletteHint {
                        key: "enter",
                        label: "run",
                    },
                    PaletteHint {
                        key: "backspace",
                        label: "leave mode",
                    },
                    PaletteHint {
                        key: "escape",
                        label: "close",
                    },
                ],
                Some(PaletteMode::Window | PaletteMode::Pane) => vec![
                    PaletteHint {
                        key: "up down",
                        label: "navigate",
                    },
                    PaletteHint {
                        key: "left right",
                        label: "expand",
                    },
                    PaletteHint {
                        key: "enter",
                        label: enter_hint,
                    },
                    PaletteHint {
                        key: "backspace",
                        label: "leave mode",
                    },
                    PaletteHint {
                        key: "escape",
                        label: "close",
                    },
                ],
                Some(PaletteMode::Host) => vec![
                    PaletteHint {
                        key: "up down",
                        label: "navigate",
                    },
                    PaletteHint {
                        key: "left right",
                        label: "collapse",
                    },
                    PaletteHint {
                        key: "enter",
                        label: "scope · attach",
                    },
                    PaletteHint {
                        key: "backspace",
                        label: "leave mode",
                    },
                ],
                None => {
                    let mut hints = vec![
                        PaletteHint {
                            key: "left right",
                            label: "expand",
                        },
                        PaletteHint {
                            key: "up down",
                            label: "navigate",
                        },
                        PaletteHint {
                            key: "enter",
                            label: enter_hint,
                        },
                    ];
                    if unified.host.is_some() {
                        hints.push(PaletteHint {
                            key: "backspace",
                            label: "clear host",
                        });
                    }
                    hints.push(PaletteHint {
                        key: "escape",
                        label: "close",
                    });
                    hints
                }
            }
        };
        if !self.last_input.is_empty()
            || !selected.is_some_and(|index| {
                unified
                    .rows
                    .get(index)
                    .is_some_and(|entry| entry.row.expanded.is_some())
                    || unified.parent_index(index).is_some()
            })
        {
            hints.retain(|hint| hint.key != "left right");
        }
        let input = unified_command_palette_input(&self.input, unified.pills(), cx);
        let mut surface = CommandPaletteSurface::new(input, cx.entity_id().as_u64()).hints(hints);
        if let Some(usage) = usage {
            surface = surface.usage(usage);
        }
        surface = if unified.rows.is_empty() {
            surface.rows(command_palette_empty(cx))
        } else {
            surface.rows(rows)
        };
        let focus = self.focus(cx);
        div()
            .id("command-palette-overlay")
            .debug_selector(|| "command-palette-overlay".to_owned())
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_start()
            .justify_center()
            .px(px(16.0))
            .pt(window.viewport_size().height * 0.12)
            .track_focus(&focus)
            .capture_action(cx.listener(Self::complete))
            .capture_action(cx.listener(Self::move_up))
            .capture_action(cx.listener(Self::move_down))
            .capture_action(cx.listener(Self::leave_layer))
            .capture_action(cx.listener(Self::dismiss))
            .capture_action(cx.listener(Self::collapse_host))
            .capture_action(cx.listener(Self::open_host))
            .capture_key_down(cx.listener(Self::on_raw_key))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                palette.update(cx, CommandPaletteView::close);
                cx.stop_propagation();
            })
            .child(surface)
            .into_any_element()
    }

    fn send(&self, action: CommandPromptAction, cx: &mut App) {
        self.backend
            .send_input(InputMessage::CommandPrompt { action }, cx);
    }

    fn navigate(&mut self, direction: isize, cx: &mut Context<Self>) {
        if let Some(unified) = &self.unified {
            let Some(next) = unified.navigate(self.selected, direction) else {
                return;
            };
            self.selected = Some(next);
            self.navigation_engaged = true;
            self.scroll_handle
                .scroll_to_item(next, ScrollStrategy::Nearest);
            cx.notify();
            return;
        }
        if self.suggestions.is_empty() {
            return;
        }
        let count = self.suggestions.len();
        let selected = if self.navigation_engaged {
            let current = self.selected.unwrap_or_default();
            if direction < 0 {
                current.checked_sub(1).unwrap_or(count - 1)
            } else {
                (current + 1) % count
            }
        } else if direction < 0 {
            count - 1
        } else {
            0
        };
        self.navigation_engaged = true;
        self.selected = Some(selected);
        self.scroll_handle
            .scroll_to_item(selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn engage_pointer_selection(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(unified) = &self.unified {
            if unified
                .rows
                .get(index)
                .is_some_and(|row| row.action.is_some())
                && self.selected != Some(index)
            {
                self.selected = Some(index);
                self.navigation_engaged = true;
                cx.notify();
            }
            return;
        }
        if index < self.suggestions.len()
            && (self.selected != Some(index) || !self.navigation_engaged)
        {
            self.selected = Some(index);
            self.navigation_engaged = true;
            cx.notify();
        }
    }

    fn accept(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(suggestion) = self.suggestions.get(index).cloned() else {
            return;
        };
        let input = self.input.read(cx).value().to_string();
        let (completed, cursor) = apply_completion(&input, &suggestion);
        if completed.len() > MAX_COMMAND_PROMPT_BYTES {
            return;
        }
        self.navigation_engaged = false;
        let insertion = completion_insertion(&input, &suggestion);
        self.input.update(cx, |input, cx| {
            input.set_selected_range(suggestion.replacement.clone(), cx);
            input.replace(insertion, window, cx);
            input.set_selected_range(cursor..cursor, cx);
        });
        self.focus(cx).focus(window, cx);
        cx.notify();
    }

    fn accept_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .selected
            .or((!self.suggestions.is_empty()).then_some(0))
        {
            self.accept(index, window, cx);
        }
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finishing {
            return;
        }
        if self.unified.is_some() {
            self.activate_unified(false, window, cx);
            return;
        }
        if self.navigation_engaged && !self.suggestions.is_empty() {
            self.accept_selected(window, cx);
            return;
        }
        self.finishing = true;
        let input = self.input.read(cx).value().to_string();
        self.send(CommandPromptAction::Submit { input }, cx);
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if self.finishing {
            return;
        }
        self.finishing = true;
        if self.window_chooser.is_some() {
            self.send_tree(ChooseTreeAction::Close, cx);
        } else if !self.local {
            self.send(CommandPromptAction::Close, cx);
        }
        cx.notify();
    }

    fn complete(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        if self.unified.is_some() {
            self.activate_unified(true, window, cx);
        } else {
            self.accept_selected(window, cx);
        }
        cx.stop_propagation();
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(-1, cx);
        cx.stop_propagation();
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(1, cx);
        cx.stop_propagation();
    }

    fn leave_layer(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if !self.input.read(cx).value().is_empty() {
            return;
        }
        if self.window_chooser.is_some() {
            cx.emit(CommandPaletteEvent::ReturnToDefault);
            cx.stop_propagation();
            return;
        }
        if let Some(unified) = &mut self.unified {
            unified.pop_layer();
            self.selected = None;
            self.navigation_engaged = false;
            self.refresh(window, cx);
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn dismiss(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        cx.stop_propagation();
    }

    fn collapse_host(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.move_in_tree(false, window, cx);
    }

    fn open_host(&mut self, _: &MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        self.move_in_tree(true, window, cx);
    }

    fn move_in_tree(&mut self, expand: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.last_input.is_empty() {
            return;
        }
        let Some(unified) = &self.unified else {
            return;
        };
        if !unified.is_navigation_tree() && unified.mode != Some(PaletteMode::Host) {
            return;
        }
        let Some(index) = self.selected else {
            return;
        };
        if unified
            .rows
            .get(index)
            .is_some_and(|entry| entry.row.expanded == Some(!expand))
        {
            self.change_tree_expansion(expand, window, cx);
        } else if let Some(next) = if expand {
            unified.child_index(index)
        } else {
            unified.parent_index(index)
        } {
            self.selected = Some(next);
            self.navigation_engaged = true;
            self.scroll_handle
                .scroll_to_item(next, ScrollStrategy::Nearest);
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn change_tree_expansion(&mut self, expand: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.chooser_search.pending.is_some() || !self.last_input.is_empty() {
            return;
        }
        let Some(index) = self.selected else {
            return;
        };
        self.navigation_engaged = true;
        let Some(unified) = &mut self.unified else {
            return;
        };
        if let Some(PaletteAction::ChooseWindow(source)) = unified
            .rows
            .get(index)
            .and_then(|entry| entry.action.as_ref())
        {
            let source = *source;
            self.chooser_search.pending = self
                .window_chooser
                .as_ref()
                .and_then(|state| state.items.get(source as usize))
                .map(|item| (item.target, expand));
            self.send_tree(ChooseTreeAction::Select(source), cx);
            self.send_tree(
                if expand {
                    ChooseTreeAction::Expand
                } else {
                    ChooseTreeAction::Collapse
                },
                cx,
            );
        } else if unified.set_expanded(index, expand) {
            self.navigation_engaged = true;
            self.refresh(window, cx);
            self.scroll_handle
                .scroll_to_item(index, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            "up" if !modifiers.platform && !modifiers.alt => self.navigate(-1, cx),
            "down" if !modifiers.platform && !modifiers.alt => self.navigate(1, cx),
            "escape" => self.close(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// `-1`, `-N` and `-k` are decided key by key inside the daemon, so the
    /// palette stops being a text field and becomes a relay: the keystroke
    /// travels on the pane-targeted key path and never reaches the input
    /// widget. `-e` needs the same interception for exactly one key, because a
    /// backspace on an empty field edits nothing and would otherwise be silent.
    fn on_raw_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.finishing {
            return;
        }
        if self.unified.is_some() {
            return;
        }
        if self.mode == CommandPromptMode::BackspaceExit {
            if event.keystroke.key == "backspace" && self.input.read(cx).value().is_empty() {
                self.close(cx);
                cx.stop_propagation();
            }
            return;
        }
        if !Self::relays_keys(self.mode) {
            return;
        }
        if let Some(pane) = self.backend.active_pane(cx) {
            self.backend.send_key(pane, event, cx);
        }
        cx.stop_propagation();
    }

    const fn relays_keys(mode: CommandPromptMode) -> bool {
        matches!(
            mode,
            CommandPromptMode::Single | CommandPromptMode::Numeric | CommandPromptMode::Key
        )
    }

    /// A prompt that reads keys has no text for the completion engine to work
    /// with, and an incremental prompt runs its template on every edit, so a
    /// tab-completion that rewrites the buffer would fire a command nobody asked
    /// for.
    const fn completes(&self) -> bool {
        matches!(
            self.mode,
            CommandPromptMode::Text | CommandPromptMode::BackspaceExit
        )
    }

    fn kind_label(kind: CompletionKind) -> &'static str {
        match kind {
            CompletionKind::History => "HISTORY",
            CompletionKind::Command => "COMMAND",
            CompletionKind::Option => "OPTION",
            CompletionKind::Value => "VALUE",
        }
    }

    fn row(
        suggestion: CompletionSuggestion,
        index: usize,
        selected: bool,
        palette: Entity<Self>,
        font: SharedString,
        cx: &App,
    ) -> gpui::Div {
        let hover_palette = palette.clone();
        let click_palette = palette;
        let kind = suggestion.kind;
        command_palette_row(
            ("command-palette-suggestion", index),
            suggestion.label,
            suggestion.detail,
            (kind != CompletionKind::Command)
                .then(|| command_kind_badge(Self::kind_label(kind), font).into_any_element()),
            selected,
            cx,
        )
        .on_mouse_enter(move |_, _, cx| {
            hover_palette.update(cx, |palette, cx| {
                palette.engage_pointer_selection(index, cx);
            });
        })
        .on_click(move |event, window, cx| {
            let submit_history = kind == CompletionKind::History && event.click_count() >= 2;
            click_palette.update(cx, |palette, cx| {
                palette.accept(index, window, cx);
                if submit_history {
                    palette.enter(window, cx);
                }
            });
            cx.stop_propagation();
        })
        .map(|row| div().h(px(COMMAND_PALETTE_ROW_HEIGHT)).child(row))
    }
}

impl Render for CommandPaletteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.unified.is_some() {
            return self.render_unified(window, cx);
        }
        let row_count = self.suggestions.len().min(MAX_VISIBLE_ROWS);
        let row_count = f32::from(u8::try_from(row_count).unwrap_or(8));
        let selected = self.selected;
        let selection_visible = self.navigation_engaged;
        let enter_hint = if self.navigation_engaged {
            "accept"
        } else if self.kind == CommandPromptKind::Command {
            "run"
        } else {
            "apply"
        };
        let suggestions: Arc<[CompletionSuggestion]> = self.suggestions.clone().into();
        let font = self.backend.mono_font(cx);
        let palette = cx.entity();
        let rows_palette = palette.clone();
        let rows = uniform_list(
            "command-palette-suggestions",
            suggestions.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        suggestions.get(index).cloned().map(|suggestion| {
                            Self::row(
                                suggestion,
                                index,
                                selection_visible && selected == Some(index),
                                rows_palette.clone(),
                                font.clone(),
                                cx,
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .h(px(COMMAND_PALETTE_ROW_HEIGHT * row_count))
        .track_scroll(&self.scroll_handle);

        let input = command_palette_input(
            &self.input,
            self.prompt.clone(),
            self.backend.mono_font(cx),
            cx,
        );
        let mut hints = Vec::with_capacity(3);
        if Self::relays_keys(self.mode) {
            hints.push(PaletteHint {
                key: match self.mode {
                    CommandPromptMode::Numeric => "digits",
                    _ => "any key",
                },
                label: match self.mode {
                    CommandPromptMode::Numeric => "collect",
                    CommandPromptMode::Key => "name it",
                    _ => "submit",
                },
            });
        } else {
            if self.kind == CommandPromptKind::Command && self.completes() {
                hints.push(PaletteHint {
                    key: "tab",
                    label: "complete",
                });
            }
            hints.push(PaletteHint {
                key: "enter",
                label: enter_hint,
            });
            hints.push(PaletteHint {
                key: "escape",
                label: "close",
            });
        }
        let mut surface = CommandPaletteSurface::new(input, cx.entity_id().as_u64()).hints(hints);
        if !self.suggestions.is_empty() {
            surface = surface.rows(rows);
        }

        let focus = self.focus(cx);
        div()
            .id("command-palette-overlay")
            .debug_selector(|| "command-palette-overlay".to_owned())
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_start()
            .justify_center()
            .px(px(24.0))
            .pt(px(22.0))
            .track_focus(&focus)
            .on_action(cx.listener(Self::complete))
            .capture_key_down(cx.listener(Self::on_raw_key))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                palette.update(cx, CommandPaletteView::close);
                cx.stop_propagation();
            })
            .child(surface)
            .into_any_element()
    }
}

fn byte_index_for_char(value: &str, cursor: u32) -> Option<usize> {
    let cursor = usize::try_from(cursor).ok()?;
    value
        .char_indices()
        .nth(cursor)
        .map(|(index, _)| index)
        .or_else(|| (cursor == value.chars().count()).then_some(value.len()))
}

fn has_command_arguments(input: &str) -> bool {
    input
        .find(char::is_whitespace)
        .is_some_and(|index| zz_protocol::catalog_command_spec(&input[..index]).is_some())
}

#[cfg(test)]
#[path = "palette_view_tests.rs"]
mod tests;
