use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Keystroke,
    MouseButton, Render, Window, div, prelude::*, px,
};
use zz_client::{MenuKeyResult, resolve_menu_key};
use zz_protocol::{InputMessage, MenuAction, MenuState};
use zz_terminal::KeyAction;

use crate::{
    mux::{client::MuxClient, prefix::terminal_key_input},
    terminal::view::TERMINAL_FONT,
    theme::tmux_style_colour,
};
use zz_ui::{ActiveTheme as _, Colorize as _};

pub(crate) struct MenuView {
    focus_handle: FocusHandle,
    mux: Entity<MuxClient>,
    state: MenuState,
    selected: Option<usize>,
}

impl MenuView {
    pub(crate) fn new(mux: Entity<MuxClient>, state: MenuState, cx: &mut Context<Self>) -> Self {
        let selected = state.selected.and_then(|index| usize::try_from(index).ok());
        Self {
            focus_handle: cx.focus_handle(),
            mux,
            state,
            selected,
        }
    }

    pub(crate) fn focus(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(crate) fn state(&self) -> &MenuState {
        &self.state
    }

    pub(crate) fn synchronize(&mut self, state: MenuState, cx: &mut Context<Self>) {
        self.selected = self
            .selected
            .filter(|_| !state.items.is_empty())
            .map(|selected| selected.min(state.items.len() - 1));
        self.state = state;
        cx.notify();
    }

    fn send(&self, action: MenuAction, cx: &App) {
        self.mux.read(cx).send_input(InputMessage::Menu { action });
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match resolve_keystroke(&self.state, self.selected, &event.keystroke) {
            MenuKeyResult::Action(action) => self.send(action, cx),
            MenuKeyResult::Select(selected) => {
                self.selected = selected;
                cx.notify();
            }
            MenuKeyResult::Consumed => {}
        }
        cx.stop_propagation();
    }
}

impl Focusable for MenuView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MenuView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_background = tmux_style_colour(
            &self.state.selected_style,
            "bg",
            cx.theme().background.raised(2).opaque(),
            cx,
        );
        let selected_foreground =
            tmux_style_colour(&self.state.selected_style, "fg", cx.theme().foreground, cx);
        let muted = cx.theme().foreground.muted();
        let rows = self
            .state
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| match item {
                None => div()
                    .id(("display-menu-separator", index))
                    .flex_1()
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .child(div().h(px(1.0)).w_full().bg(cx.theme().border))
                    .into_any_element(),
                Some(item) => {
                    let selected = self.selected == Some(index);
                    let enabled = item.enabled;
                    let mux = self.mux.clone();
                    div()
                        .id(("display-menu-row", index))
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(12.0))
                        .font_family(TERMINAL_FONT)
                        .text_size(px(13.0))
                        .line_height(px(16.0))
                        .when(selected, |row| {
                            row.bg(selected_background).text_color(selected_foreground)
                        })
                        .when(!selected && !enabled, |row| row.text_color(muted))
                        .when(!selected && enabled, |row| {
                            row.hover(|row| row.bg(cx.theme().background.raised(1).opaque()))
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                            mux.read(cx).send_input(InputMessage::Menu {
                                action: MenuAction::Choose(
                                    u32::try_from(index).unwrap_or(u32::MAX),
                                ),
                            });
                            cx.stop_propagation();
                        })
                        .child(item.name.clone())
                        .when_some(item.annotation.clone(), |row, key| {
                            row.child(format!("({key})"))
                        })
                        .into_any_element()
                }
            });
        div()
            .id("display-menu-input")
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(|_, _, cx| cx.stop_propagation())
            .children(rows)
    }
}

fn resolve_keystroke(
    state: &MenuState,
    selected: Option<usize>,
    keystroke: &Keystroke,
) -> MenuKeyResult {
    let input = terminal_key_input(keystroke, KeyAction::Press);
    resolve_menu_key(state, selected, &input)
}

#[cfg(test)]
mod tests {
    use gpui::Modifiers;
    use zz_protocol::{MenuItem, PopupBorderLines};

    use super::*;

    fn state() -> MenuState {
        MenuState {
            left: 0,
            top: 0,
            width: 20,
            height: 6,
            client_columns: 80,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 18,
            title: String::new(),
            style: "default".to_owned(),
            selected_style: "default".to_owned(),
            border_style: "default".to_owned(),
            border_lines: PopupBorderLines::Single,
            items: vec![
                Some(MenuItem {
                    name: "Quit item".to_owned(),
                    key: Some("q".to_owned()),
                    annotation: Some("q".to_owned()),
                    enabled: true,
                }),
                None,
                Some(MenuItem {
                    name: "Disabled".to_owned(),
                    key: None,
                    annotation: None,
                    enabled: false,
                }),
                Some(MenuItem {
                    name: "Last".to_owned(),
                    key: None,
                    annotation: None,
                    enabled: true,
                }),
            ],
            selected: Some(0),
            stay_open: false,
        }
    }

    fn key(value: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            key: value.to_owned(),
            key_char: key_char.map(str::to_owned),
            modifiers,
        }
    }

    #[test]
    fn gpui_shift_tab_reaches_the_shared_backtab_path() {
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            resolve_keystroke(&state(), Some(3), &key("tab", None, shift)),
            MenuKeyResult::Select(Some(0))
        );
    }
}
