use gpui::{
    App, Bounds, Div, ElementId, Hsla, Keystroke, Pixels, Point, SharedString, Size, Stateful, div,
    point, prelude::*, px, size,
};

use crate::{ActiveTheme as _, Colorize as _};

pub fn menu_row(
    id: impl Into<ElementId>,
    name: impl Into<SharedString>,
    annotation: Option<SharedString>,
    enabled: bool,
    selected: bool,
    row_height: Pixels,
    selected_background: Hsla,
    selected_foreground: Hsla,
    font_family: impl Into<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(row_height)
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(12.0))
        .font_family(font_family.into())
        .text_size(px(13.0))
        .line_height(px(16.0))
        .when(selected, |row| {
            row.bg(selected_background).text_color(selected_foreground)
        })
        .when(!selected && !enabled, |row| {
            row.text_color(cx.theme().foreground.muted())
        })
        .when(!selected && enabled, |row| {
            row.hover(|row| row.bg(cx.theme().background.raised(1).opaque()))
        })
        .child(name.into())
        .when_some(annotation, |row, key| row.child(format!("({key})")))
}

pub fn menu_separator(id: impl Into<ElementId>, row_height: Pixels, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .h(row_height)
        .flex_none()
        .flex()
        .items_center()
        .px(px(8.0))
        .child(div().h(px(1.0)).w_full().bg(cx.theme().border()))
}

pub fn confirm_prompt(
    id: impl Into<ElementId>,
    prompt: impl Into<SharedString>,
    font_family: impl Into<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .size_full()
        .flex()
        .items_center()
        .px(px(12.0))
        .font_family(font_family.into())
        .text_size(px(13.0))
        .line_height(px(16.0))
        .child(prompt.into())
}

pub fn confirm_accepts(confirm_key: u8, default_yes: bool, keystroke: &Keystroke) -> bool {
    if keystroke.key == "enter" {
        return default_yes && !keystroke.modifiers.platform && !keystroke.modifiers.function;
    }
    if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.function {
        return false;
    }
    keystroke.key_char.as_deref().is_some_and(|value| {
        value.len() == 1 && value.as_bytes().first().copied() == Some(confirm_key)
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatingFrame {
    pub bounds: Bounds<Pixels>,
    pub inset_x: Pixels,
    pub inset_y: Pixels,
}

pub fn floating_frame(
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    client_columns: u16,
    client_rows: u16,
    cell_width_px: u32,
    cell_height_px: u32,
    bordered: bool,
    origin: Point<Pixels>,
    canvas: Size<Pixels>,
    scale: f32,
) -> FloatingFrame {
    let cell_width = px(f32::from(u16::try_from(cell_width_px).unwrap_or(u16::MAX)) / scale);
    let cell_height = px(f32::from(u16::try_from(cell_height_px).unwrap_or(u16::MAX)) / scale);
    let grid_width = cell_width * usize::from(client_columns);
    let grid_height = cell_height * usize::from(client_rows);
    let grid_left = ((canvas.width - grid_width) / 2.0).max(Pixels::ZERO);
    let grid_top = ((canvas.height - grid_height) / 2.0).max(Pixels::ZERO);
    FloatingFrame {
        bounds: Bounds::new(
            point(
                origin.x + grid_left + cell_width * usize::from(left),
                origin.y + grid_top + cell_height * usize::from(top),
            ),
            size(
                cell_width * usize::from(width),
                cell_height * usize::from(height),
            ),
        ),
        inset_x: if bordered { cell_width } else { Pixels::ZERO },
        inset_y: if bordered { cell_height } else { Pixels::ZERO },
    }
}

pub fn menu_grid_cell(offset: Pixels, cell_px: u32, scale: f32, origin: u16, outside: u16) -> u16 {
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

pub const RELEASE_BUTTONS: u8 = 3;
pub const WHEEL_BUTTONS: u8 = 64;

pub const fn menu_press_buttons(button: gpui::MouseButton) -> u8 {
    match button {
        gpui::MouseButton::Left => 0,
        gpui::MouseButton::Middle => 1,
        gpui::MouseButton::Right => 2,
        gpui::MouseButton::Navigate(_) => 128,
    }
}
