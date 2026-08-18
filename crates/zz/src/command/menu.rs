use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Keystroke,
    MouseButton, Render, Window, div, prelude::*, px,
};
use zz_protocol::{InputMessage, MenuAction, MenuState, input_key_name};
use zz_terminal::KeyAction;

use crate::{
    mux::{client::MuxClient, prefix::terminal_key_input},
    terminal::view::TERMINAL_FONT,
    theme::tmux_style_colour,
};
use zz_ui::{ActiveTheme as _, Colorize as _};

enum MenuKeyResult {
    Action(MenuAction),
    Select(Option<usize>),
    Ignore,
}

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
        self.selected = state.selected.and_then(|index| usize::try_from(index).ok());
        self.state = state;
        cx.notify();
    }

    fn send(&self, action: MenuAction, cx: &App) {
        self.mux.read(cx).send_input(InputMessage::Menu { action });
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match menu_key_result(&self.state, self.selected, &event.keystroke) {
            MenuKeyResult::Action(action) => self.send(action, cx),
            MenuKeyResult::Select(selected) => {
                self.selected = selected;
                cx.notify();
            }
            MenuKeyResult::Ignore => {}
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
                        .when_some(item.key.clone(), |row, key| row.child(format!("({key})")))
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

fn menu_key_result(
    state: &MenuState,
    selected: Option<usize>,
    keystroke: &Keystroke,
) -> MenuKeyResult {
    let input = terminal_key_input(keystroke, KeyAction::Press);
    let key = if keystroke.key == "tab" && keystroke.modifiers.shift {
        "BTab".to_owned()
    } else {
        input_key_name(&input).to_string()
    };
    if let Some((index, _)) = state.items.iter().enumerate().find(|(_, item)| {
        item.as_ref()
            .is_some_and(|item| item.enabled && item.key.as_deref() == Some(key.as_str()))
    }) {
        return MenuKeyResult::Action(MenuAction::Choose(u32::try_from(index).unwrap_or(u32::MAX)));
    }
    match key.as_str() {
        "Escape" | "C-[" | "C-c" | "C-g" | "q" => MenuKeyResult::Action(MenuAction::Cancel),
        "Enter" => selected.map_or(MenuKeyResult::Ignore, |index| {
            MenuKeyResult::Action(MenuAction::Choose(u32::try_from(index).unwrap_or(u32::MAX)))
        }),
        "Up" | "k" | "BTab" => MenuKeyResult::Select(menu_step(&state.items, selected, -1, 1)),
        "Down" | "j" => MenuKeyResult::Select(menu_step(&state.items, selected, 1, 1)),
        "Home" | "g" => MenuKeyResult::Select(menu_edge(&state.items, false)),
        "End" | "G" => MenuKeyResult::Select(menu_edge(&state.items, true)),
        "PPage" | "C-b" => MenuKeyResult::Select(menu_step(&state.items, selected, -1, 5)),
        "NPage" => MenuKeyResult::Select(menu_step(&state.items, selected, 1, 5)),
        _ => MenuKeyResult::Ignore,
    }
}

fn menu_edge(items: &[Option<zz_protocol::MenuItem>], reverse: bool) -> Option<usize> {
    if reverse {
        items
            .iter()
            .rposition(|item| item.as_ref().is_some_and(|item| item.enabled))
    } else {
        items
            .iter()
            .position(|item| item.as_ref().is_some_and(|item| item.enabled))
    }
}

fn menu_step(
    items: &[Option<zz_protocol::MenuItem>],
    selected: Option<usize>,
    direction: isize,
    count: usize,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    let Some(mut selected) = selected else {
        return menu_edge(items, direction < 0);
    };
    for _ in 0..count {
        let mut next = selected;
        loop {
            next = if direction < 0 {
                next.checked_sub(1).unwrap_or(items.len().saturating_sub(1))
            } else {
                next.saturating_add(1) % items.len()
            };
            if items[next].as_ref().is_some_and(|item| item.enabled) || next == selected {
                break;
            }
        }
        selected = next;
    }
    Some(selected)
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
                    enabled: true,
                }),
                None,
                Some(MenuItem {
                    name: "Disabled".to_owned(),
                    key: None,
                    enabled: false,
                }),
                Some(MenuItem {
                    name: "Last".to_owned(),
                    key: None,
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
    fn shortcut_claims_q_before_cancel_and_navigation_skips_rows() {
        assert!(matches!(
            menu_key_result(
                &state(),
                Some(0),
                &key("q", Some("q"), Modifiers::default())
            ),
            MenuKeyResult::Action(MenuAction::Choose(0))
        ));
        assert!(matches!(
            menu_key_result(&state(), Some(0), &key("down", None, Modifiers::default())),
            MenuKeyResult::Select(Some(3))
        ));
        assert_eq!(menu_step(&state().items, Some(3), 1, 1), Some(0));
    }

    #[test]
    fn shift_tab_is_backtab_and_page_steps_selectable_rows() {
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert!(matches!(
            menu_key_result(&state(), Some(3), &key("tab", None, shift)),
            MenuKeyResult::Select(Some(0))
        ));
        assert_eq!(menu_step(&state().items, Some(0), 1, 5), Some(3));
    }
}
