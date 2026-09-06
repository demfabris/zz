use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Keystroke,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollWheelEvent, Window, div, prelude::*, px,
};
use zz_client::{MenuBox, MenuKeyResult, MenuPointerKind, resolve_menu_key, resolve_menu_mouse};
use zz_protocol::{InputMessage, MenuAction, MenuState, PopupBorderLines};
use zz_terminal::KeyAction;

use crate::{
    mux::{client::MuxClient, prefix::terminal_key_input},
    terminal::view::TERMINAL_FONT,
    theme::tmux_style_colour,
};
use zz_ui::{ActiveTheme as _, Colorize as _, ElementExt as _};

/// The `m->b` byte `tty-keys.c` reports for a release, and the byte a motion
/// with no button held carries once `MOUSE_MASK_DRAG` is masked off: both read
/// as `MOUSE_RELEASE`.
pub(crate) const RELEASE_BUTTONS: u8 = 3;
/// `MOUSE_WHEEL_UP`. `MOUSE_BUTTONS` leaves it alone, so it is not
/// `MOUSE_BUTTON_1` and a `MENU_NOMOUSE` menu leaves on it.
pub(crate) const WHEEL_BUTTONS: u8 = 64;
/// The hairline `FloatingSurface` draws inside the one cell of border
/// `menu_frame` measures. It sits between the frame and the content box, so
/// the content box's origin is this much past the cell the menu starts on.
const SURFACE_BORDER: Pixels = px(1.0);

/// `MOUSE_BUTTONS(m->b)` for a press of this button: 0, 1 and 2 for the three
/// buttons `tty-keys.c` numbers, and `MOUSE_BUTTON_8` for the navigation
/// buttons no tmux terminal reports as one of the first three.
pub(crate) const fn press_buttons(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => zz_client::MOUSE_BUTTON_1,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Navigate(_) => 128,
    }
}

pub(crate) struct MenuView {
    focus_handle: FocusHandle,
    mux: Entity<MuxClient>,
    state: MenuState,
    selected: Option<usize>,
    content_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl MenuView {
    pub(crate) fn new(mux: Entity<MuxClient>, state: MenuState, cx: &mut Context<Self>) -> Self {
        let selected = state.selected.and_then(|index| usize::try_from(index).ok());
        Self {
            focus_handle: cx.focus_handle(),
            mux,
            state,
            selected,
            content_bounds: Rc::new(Cell::new(Bounds::default())),
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

    /// One pointer report, answered by `menu_key_cb`'s whole mouse arm rather
    /// than by the desktop. A `MENU_NOMOUSE` menu swallows a button-1 press and
    /// leaves on anything else, wherever the pointer is. A menu that took the
    /// mouse moves its highlight under a press or a motion, chooses the
    /// highlighted row when a release lands inside the box, closes when a
    /// release lands outside it, and, when it is stay-open, closes instead on
    /// any report that is neither a release, a wheel nor a drag.
    pub(crate) fn pointer(
        &mut self,
        kind: MenuPointerKind,
        buttons: u8,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let (column, row) = self.cell_at(position, window.scale_factor());
        let frame = MenuBox {
            left: self.state.left,
            top: self.state.top,
            width: self.state.width,
            items: self.state.items.len(),
        };
        match resolve_menu_mouse(
            &self.state,
            self.selected,
            frame,
            kind,
            buttons,
            column,
            row,
        ) {
            MenuKeyResult::Action(action) => self.send(action, cx),
            MenuKeyResult::Select(selected) => {
                if self.selected != selected {
                    self.selected = selected;
                    cx.notify();
                }
            }
            MenuKeyResult::Consumed => {}
        }
    }

    /// The client-grid cell the pointer sits on, in the coordinates
    /// `MenuState`'s `left` and `top` use. This view's own box is the menu's
    /// content, which `FloatingSurface::content_inset` insets by the one cell
    /// of border `menu_frame` measures, so its top-left corner is the cell one
    /// right and one down from the menu's origin. A pointer left of or above
    /// that box is reported at a cell outside the menu, never clamped onto its
    /// edge.
    fn cell_at(&self, position: Point<Pixels>, scale: f32) -> (u16, u16) {
        let bounds = self.content_bounds.get();
        let bordered = self.state.border_lines != PopupBorderLines::None;
        let hairline = if bordered {
            SURFACE_BORDER
        } else {
            Pixels::ZERO
        };
        let inset = u16::from(bordered);
        let column = grid_cell(
            position.x - bounds.origin.x + hairline,
            self.state.cell_width_px,
            scale,
            self.state.left.saturating_add(inset),
            u16::MAX,
        );
        let row = grid_cell(
            position.y - bounds.origin.y + hairline,
            self.state.cell_height_px,
            scale,
            self.state.top.saturating_add(inset),
            0,
        );
        (column, row)
    }

    /// One drawn row is one cell of the box `menu_frame` measures, because
    /// `menu.c` sizes a menu as `count + 2` rows and hands the client that
    /// height. Letting the rows take their text height instead overflows the
    /// box whenever the terminal cell is shorter than the row's line height,
    /// and `FloatingSurface` clips what overflows.
    fn row_height(&self, scale: f32) -> Pixels {
        px(f32::from(u16::try_from(self.state.cell_height_px).unwrap_or(u16::MAX)) / scale)
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row_height = self.row_height(window.scale_factor());
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
                    .h(row_height)
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .child(div().h(px(1.0)).w_full().bg(cx.theme().border))
                    .into_any_element(),
                Some(item) => {
                    let selected = self.selected == Some(index);
                    let enabled = item.enabled;
                    div()
                        .id(("display-menu-row", index))
                        .debug_selector(move || format!("display-menu-row-{index}"))
                        .h(row_height)
                        .flex_none()
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
                        .child(item.name.clone())
                        .when_some(item.annotation.clone(), |row, key| {
                            row.child(format!("({key})"))
                        })
                        .into_any_element()
                }
            });
        let measured = Rc::clone(&self.content_bounds);
        div()
            .id("display-menu-input")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(|_, _, cx| cx.stop_propagation())
            .on_prepaint(move |bounds, _, _| measured.set(bounds))
            .capture_any_mouse_down(cx.listener(|menu, event: &MouseDownEvent, window, cx| {
                menu.pointer(
                    MenuPointerKind::Press,
                    press_buttons(event.button),
                    event.position,
                    window,
                    cx,
                );
                cx.stop_propagation();
            }))
            .capture_any_mouse_up(cx.listener(|menu, event: &MouseUpEvent, window, cx| {
                menu.pointer(
                    MenuPointerKind::Release,
                    RELEASE_BUTTONS,
                    event.position,
                    window,
                    cx,
                );
                cx.stop_propagation();
            }))
            .on_mouse_move(cx.listener(|menu, event: &MouseMoveEvent, window, cx| {
                let (kind, buttons) = event
                    .pressed_button
                    .map_or((MenuPointerKind::Motion, RELEASE_BUTTONS), |button| {
                        (MenuPointerKind::Drag, press_buttons(button))
                    });
                menu.pointer(kind, buttons, event.position, window, cx);
                cx.stop_propagation();
            }))
            .on_scroll_wheel(cx.listener(|menu, event: &ScrollWheelEvent, window, cx| {
                menu.pointer(
                    MenuPointerKind::Wheel,
                    WHEEL_BUTTONS,
                    event.position,
                    window,
                    cx,
                );
                cx.stop_propagation();
            }))
            .children(rows)
    }
}

/// One axis of `cell_at`. `menu_frame` sizes a cell as the daemon's device
/// pixels over the window's scale factor, so the same division maps a pointer
/// back onto the grid. A negative offset is the pointer before the box, which
/// answers `outside` rather than the box's own first cell.
fn grid_cell(offset: Pixels, cell_px: u32, scale: f32, origin: u16, outside: u16) -> u16 {
    let cell = f32::from(u16::try_from(cell_px).unwrap_or(u16::MAX)) / scale;
    let offset = f32::from(offset);
    if cell <= 0.0 || offset < 0.0 {
        return outside;
    }
    let steps = (offset / cell).floor();
    if steps < 0.0 || steps > f32::from(u16::MAX) {
        return outside;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    origin.saturating_add(steps as u16)
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
            mouse_keys: false,
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
