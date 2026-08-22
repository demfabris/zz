use std::{ops::Range, sync::Arc};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, Render,
    ScrollStrategy, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use zz_protocol::{
    CommandPromptAction, CommandPromptKind, CommandPromptMode, CommandPromptState, InputMessage,
    MAX_COMMAND_PROMPT_BYTES, MuxSnapshot,
};
use zz_ui::command::{
    COMMAND_PALETTE_ROW_HEIGHT, CommandPaletteSurface, PaletteHint, command_kind_badge,
    command_palette_input, command_palette_row,
};
use zz_ui::{
    ActiveTheme as _,
    input::{IndentInline, InputEvent, InputState},
};

use crate::{
    command::completion::{
        CompletionKind, CompletionSuggestion, PaneKindAvailability, apply_completion,
        complete_command, completion_insertion,
    },
    mux::{client::MuxClient, prefix::terminal_key_input},
    terminal::view::TERMINAL_FONT,
};
use zz_ui::Colorize as _;

const MAX_VISIBLE_ROWS: usize = 8;

pub(crate) struct CommandPaletteView {
    mux: Entity<MuxClient>,
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
}

impl CommandPaletteView {
    pub(crate) fn new(
        mux: Entity<MuxClient>,
        state: &CommandPromptState,
        revision: u64,
        snapshot: Arc<MuxSnapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = match state.kind {
            CommandPromptKind::Command => "Type a tmux command…",
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

        cx.observe(&input, |palette, input, cx| {
            palette.synchronize_local_input(&input, cx);
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

        let mut palette = Self {
            mux,
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
            availability: PaneKindAvailability {
                browser: crate::browser::controller::is_available(cx),
                agent: crate::config::agent_pane_enabled(cx),
                editor: crate::config::editor_pane_enabled(cx),
            },
        };
        palette.recompute_suggestions();
        palette
    }

    pub(crate) fn focus(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }

    pub(crate) fn synchronize(
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
                cx.notify();
            }
            return;
        }

        self.revision = revision;
        self.prompt.clone_from(&state.prompt);
        self.kind = state.kind;
        self.mode = state.mode;
        self.history.clone_from(&state.history);
        self.finishing = false;
        self.navigation_engaged = false;
        let cursor = byte_index_for_char(&state.input, state.cursor).unwrap_or(state.input.len());
        let current = self.input.read(cx).value().to_string();
        self.input.update(cx, |input, cx| {
            if current != state.input {
                input.set_value(state.input.clone(), window, cx);
            }
            input.set_selected_range(cursor..cursor, cx);
        });
        self.last_input.clone_from(&state.input);
        self.last_cursor = cursor;
        self.recompute_suggestions();
        cx.notify();
    }

    fn synchronize_local_input(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let (value, cursor) = {
            let input = input.read(cx);
            (input.value().to_string(), input.cursor())
        };
        if value == self.last_input && cursor == self.last_cursor {
            return;
        }
        self.last_input.clone_from(&value);
        self.last_cursor = cursor;
        self.navigation_engaged = false;
        self.recompute_suggestions();
        if !self.finishing {
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

    fn send(&self, action: CommandPromptAction, cx: &App) {
        self.mux
            .read(cx)
            .send_input(InputMessage::CommandPrompt { action });
    }

    fn navigate(&mut self, direction: isize, cx: &mut Context<Self>) {
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
            .scroll_to_item(selected, ScrollStrategy::Center);
        cx.notify();
    }

    fn engage_pointer_selection(&mut self, index: usize, cx: &mut Context<Self>) {
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
        self.send(CommandPromptAction::Close, cx);
    }

    fn complete(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        self.accept_selected(window, cx);
        cx.stop_propagation();
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
        let mux = self.mux.read(cx);
        if let Some(pane) = mux.active_pane() {
            mux.send_input(InputMessage::Key {
                pane,
                input: terminal_key_input(&event.keystroke, zz_terminal::KeyAction::Press),
                text_follows: false,
            });
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
        muted: gpui::Hsla,
        selection_background: gpui::Hsla,
        palette: Entity<Self>,
    ) -> impl IntoElement {
        let hover_palette = palette.clone();
        let click_palette = palette;
        let kind = suggestion.kind;
        command_palette_row(
            ("command-palette-suggestion", index),
            suggestion.label,
            suggestion.detail,
            command_kind_badge(Self::kind_label(kind), TERMINAL_FONT),
            selected,
            selection_background,
            muted,
            TERMINAL_FONT,
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
    }
}

impl Render for CommandPaletteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let muted = cx.theme().foreground.muted();
        let selection_background = cx.theme().background.hover();
        let suggestions: Arc<[CompletionSuggestion]> = self.suggestions.clone().into();
        let palette = cx.entity();
        let rows_palette = palette.clone();
        let rows = uniform_list(
            "command-palette-suggestions",
            suggestions.len(),
            cx.processor(move |_, range: Range<usize>, _, _| {
                range
                    .filter_map(|index| {
                        suggestions.get(index).cloned().map(|suggestion| {
                            Self::row(
                                suggestion,
                                index,
                                selection_visible && selected == Some(index),
                                muted,
                                selection_background,
                                rows_palette.clone(),
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .h(px(COMMAND_PALETTE_ROW_HEIGHT * row_count))
        .track_scroll(&self.scroll_handle);

        let input = command_palette_input(&self.input, self.prompt.clone(), TERMINAL_FONT, cx);
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
        let mut surface = CommandPaletteSurface::new(input, self.revision).hints(hints);
        if !self.suggestions.is_empty() {
            surface = surface.rows(rows);
        }

        let focus = self.focus(cx);
        div()
            .id("command-palette-overlay")
            .absolute()
            .inset_0()
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

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "macos"))]
    use std::{cell::RefCell, rc::Rc};

    #[cfg(not(target_os = "macos"))]
    use gpui::{TestAppContext, VisualTestContext};
    #[cfg(not(target_os = "macos"))]
    use zz_daemon::DaemonError;
    #[cfg(not(target_os = "macos"))]
    use zz_protocol::{CommandPromptMode, CommandPromptType};
    #[cfg(not(target_os = "macos"))]
    use zz_ui::Root;

    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn palette_keeps_local_edits_for_the_same_revision_and_disables_value_completions(
        cx: &mut TestAppContext,
    ) {
        cx.update(zz_ui::init);
        let palette_slot = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&palette_slot);
        let initial = CommandPromptState {
            prompt: ":".to_owned(),
            input: String::new(),
            cursor: 0,
            kind: CommandPromptKind::Command,
            history: vec!["list-panes".to_owned()],
            prompt_type: CommandPromptType::Command,
            mode: CommandPromptMode::Text,
            no_freeze: false,
        };
        let stale = initial.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let palette = cx.new(|cx| {
                CommandPaletteView::new(
                    mux,
                    &initial,
                    1,
                    Arc::new(MuxSnapshot::default()),
                    window,
                    cx,
                )
            });
            palette.read(cx).focus(cx).focus(window, cx);
            captured.replace(Some(palette.clone()));
            Root::new(palette, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        let palette = palette_slot.borrow().clone().expect("captured palette");

        cx.update(|window, cx| {
            assert!(palette.read(cx).focus(cx).is_focused(window));
            let input = palette.read(cx).input.clone();
            input.update(cx, |input, cx| input.insert("ren", window, cx));
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| palette.read(cx).last_input.clone()),
            "ren"
        );
        assert!(cx.update(|_, cx| !palette.read(cx).suggestions.is_empty()));

        cx.update(|window, cx| {
            palette.update(cx, |palette, cx| {
                palette.synchronize(&stale, 1, &Arc::new(MuxSnapshot::default()), window, cx);
            });
        });
        assert_eq!(
            cx.update(|_, cx| palette.read(cx).input.read(cx).value().to_string()),
            "ren"
        );

        let value = CommandPromptState {
            prompt: "rename-window: ".to_owned(),
            input: "notes".to_owned(),
            cursor: 5,
            kind: CommandPromptKind::Value,
            history: Vec::new(),
            prompt_type: CommandPromptType::Command,
            mode: CommandPromptMode::Text,
            no_freeze: false,
        };
        cx.update(|window, cx| {
            palette.update(cx, |palette, cx| {
                palette.synchronize(&value, 2, &Arc::new(MuxSnapshot::default()), window, cx);
            });
        });
        assert!(cx.update(|_, cx| palette.read(cx).suggestions.is_empty()));
        assert_eq!(
            cx.update(|_, cx| palette.read(cx).input.read(cx).value().to_string()),
            "notes"
        );
    }

    /// The daemon owns `-1`, `-N` and `-k` key by key, so the palette relays
    /// their presses instead of editing, and it offers no completion for a
    /// prompt whose buffer it is not allowed to rewrite.
    #[test]
    fn key_reading_prompts_relay_instead_of_editing() {
        for mode in [
            zz_protocol::CommandPromptMode::Single,
            zz_protocol::CommandPromptMode::Numeric,
            zz_protocol::CommandPromptMode::Key,
        ] {
            assert!(CommandPaletteView::relays_keys(mode), "{mode:?}");
        }
        for mode in [
            zz_protocol::CommandPromptMode::Text,
            zz_protocol::CommandPromptMode::Incremental,
            zz_protocol::CommandPromptMode::BackspaceExit,
        ] {
            assert!(!CommandPaletteView::relays_keys(mode), "{mode:?}");
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn a_key_reading_prompt_drops_the_completion_list(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let palette_slot = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&palette_slot);
        let initial = CommandPromptState {
            prompt: ":".to_owned(),
            input: "ren".to_owned(),
            cursor: 3,
            kind: CommandPromptKind::Command,
            history: Vec::new(),
            prompt_type: CommandPromptType::Command,
            mode: CommandPromptMode::Text,
            no_freeze: false,
        };
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let palette = cx.new(|cx| {
                CommandPaletteView::new(
                    mux,
                    &initial,
                    1,
                    Arc::new(MuxSnapshot::default()),
                    window,
                    cx,
                )
            });
            captured.replace(Some(palette.clone()));
            Root::new(palette, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        let palette = palette_slot.borrow().clone().expect("captured palette");
        assert!(cx.update(|_, cx| !palette.read(cx).suggestions.is_empty()));

        for (revision, mode) in [
            (2, CommandPromptMode::Key),
            (3, CommandPromptMode::Numeric),
            (4, CommandPromptMode::Incremental),
        ] {
            let state = CommandPromptState {
                prompt: ":".to_owned(),
                input: "ren".to_owned(),
                cursor: 3,
                kind: CommandPromptKind::Command,
                history: Vec::new(),
                prompt_type: CommandPromptType::Command,
                mode,
                no_freeze: false,
            };
            cx.update(|window, cx| {
                palette.update(cx, |palette, cx| {
                    palette.synchronize(
                        &state,
                        revision,
                        &Arc::new(MuxSnapshot::default()),
                        window,
                        cx,
                    );
                });
            });
            assert_eq!(cx.update(|_, cx| palette.read(cx).mode), mode);
            assert!(
                cx.update(|_, cx| palette.read(cx).suggestions.is_empty()),
                "{mode:?}"
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn tab_accepts_completion_without_leaving_the_palette(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let palette_slot = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&palette_slot);
        let initial = CommandPromptState {
            prompt: ":".to_owned(),
            input: "new-w".to_owned(),
            cursor: 5,
            kind: CommandPromptKind::Command,
            history: Vec::new(),
            prompt_type: CommandPromptType::Command,
            mode: CommandPromptMode::Text,
            no_freeze: false,
        };
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let palette = cx.new(|cx| {
                CommandPaletteView::new(
                    mux,
                    &initial,
                    1,
                    Arc::new(MuxSnapshot::default()),
                    window,
                    cx,
                )
            });
            palette.read(cx).focus(cx).focus(window, cx);
            captured.replace(Some(palette.clone()));
            Root::new(palette, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        let palette = palette_slot.borrow().clone().expect("captured palette");

        cx.simulate_keystrokes("tab");

        assert_eq!(
            cx.update(|_, cx| palette.read(cx).input.read(cx).value().to_string()),
            "new-window "
        );
        assert!(cx.update(|window, cx| palette.read(cx).focus(cx).is_focused(window)));
    }

    #[test]
    fn unicode_scalar_cursor_conversion_is_boundary_safe() {
        assert_eq!(byte_index_for_char("aα界", 0), Some(0));
        assert_eq!(byte_index_for_char("aα界", 2), Some(3));
        assert_eq!(byte_index_for_char("aα界", 3), Some(6));
        assert_eq!(byte_index_for_char("aα界", 4), None);
    }
}
