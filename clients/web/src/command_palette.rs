use std::{ops::Range, sync::Arc};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, Render,
    ScrollStrategy, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use zz_client::completion::{
    CompletionKind, CompletionSuggestion, PaneKindAvailability, apply_completion, complete_command,
    completion_insertion,
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

use crate::connection::Connection;
use zz_ui::Colorize as _;

const MAX_VISIBLE_ROWS: usize = 8;

pub(crate) struct CommandPaletteView {
    connection: Entity<Connection>,
    pane: Option<zz_protocol::PaneId>,
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
        connection: Entity<Connection>,
        pane: Option<zz_protocol::PaneId>,
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
            connection,
            pane,
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
                browser: false,
                agent: true,
                editor: false,
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
        pane: Option<zz_protocol::PaneId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pane = pane;
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

    fn send(&self, action: CommandPromptAction, cx: &mut Context<Self>) {
        self.connection.update(cx, |connection, cx| {
            connection.send(
                zz_protocol::ProtocolMessage::Input(InputMessage::CommandPrompt { action }),
                cx,
            );
        });
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
        if let Some(pane) = self.pane {
            self.connection.update(cx, |connection, cx| {
                connection.send(
                    zz_protocol::ProtocolMessage::Input(InputMessage::Key {
                        pane,
                        input: crate::terminal::key_input(event),
                        text_follows: false,
                    }),
                    cx,
                );
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
        font: gpui::SharedString,
    ) -> impl IntoElement {
        let hover_palette = palette.clone();
        let click_palette = palette;
        let kind = suggestion.kind;
        command_palette_row(
            ("command-palette-suggestion", index),
            suggestion.label,
            suggestion.detail,
            command_kind_badge(Self::kind_label(kind), font.clone()),
            selected,
            selection_background,
            muted,
            font,
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
        let font = cx.theme().mono_font_family.clone();
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
                                font.clone(),
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
            cx.theme().mono_font_family.clone(),
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
    use super::byte_index_for_char;

    #[test]
    fn daemon_scalar_cursor_maps_to_utf8_for_completion() {
        let value = "a🦀日本";
        assert_eq!(byte_index_for_char(value, 0), Some(0));
        assert_eq!(byte_index_for_char(value, 2), Some(5));
        assert_eq!(byte_index_for_char(value, 4), Some(value.len()));
        assert_eq!(byte_index_for_char(value, 5), None);
    }
}
