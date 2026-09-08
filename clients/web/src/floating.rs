use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, Bounds, Context, Hsla, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollWheelEvent, Window, div, point, prelude::*, px, size,
};
use zz_client::{MenuBox, MenuKeyResult, MenuPointerKind, resolve_menu_mouse};
use zz_protocol::{InputMessage, MenuState, PopupBorderLines, TmuxColour, parse_tmux_colour};
use zz_ui::{
    ActiveTheme as _, Colorize as _, ElementExt as _,
    command::floating::{
        RELEASE_BUTTONS, WHEEL_BUTTONS, confirm_prompt, floating_frame, menu_grid_cell,
        menu_press_buttons, menu_row, menu_separator,
    },
    pane::FloatingSurface,
};

use super::WebClient;

impl WebClient {
    pub(super) fn floating_canvas_size(&self, window: &Window) -> gpui::Size<Pixels> {
        let viewport = window.viewport_size();
        size(
            (viewport.width
                - px(if self.sidebar || self.settings.is_some() {
                    zz_ui::navigation::WORKSPACE_SIDEBAR_DEFAULT_WIDTH
                } else {
                    0.0
                }))
            .max(px(0.0)),
            (viewport.height
                - if self.settings.is_none() {
                    zz_ui::TITLE_BAR_HEIGHT
                } else {
                    px(0.0)
                })
            .max(px(0.0)),
        )
    }

    pub(super) fn floating_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let core = &self.connection.read(cx).core;
        let canvas = self.floating_canvas_size(window);
        if let Some(state) = core.menu().cloned() {
            let bordered = state.border_lines != PopupBorderLines::None;
            let scale = window.scale_factor();
            let frame = floating_frame(
                state.left,
                state.top,
                state.width,
                state.height,
                state.client_columns,
                state.client_rows,
                state.cell_width_px,
                state.cell_height_px,
                bordered,
                point(px(0.0), px(0.0)),
                canvas,
                scale,
            );
            let background = style_color(
                &state.style,
                "bg",
                cx.theme().background.raised(1).opaque(),
                cx,
            );
            let foreground = style_color(&state.style, "fg", cx.theme().foreground, cx);
            let border_color = style_color(&state.border_style, "fg", cx.theme().border, cx);
            let selected_background = style_color(
                &state.selected_style,
                "bg",
                cx.theme().background.raised(2).opaque(),
                cx,
            );
            let selected_foreground =
                style_color(&state.selected_style, "fg", cx.theme().foreground, cx);
            let selected = self.menu_selection;
            let row_height =
                px(f32::from(u16::try_from(state.cell_height_px).unwrap_or(u16::MAX)) / scale);
            let rows = state
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| match item {
                    Some(item) => menu_row(
                        ("web-display-menu-row", index),
                        item.name.clone(),
                        item.annotation.clone().map(Into::into),
                        item.enabled,
                        selected == Some(index),
                        row_height,
                        selected_background,
                        selected_foreground,
                        cx.theme().mono_font_family.clone(),
                        cx,
                    )
                    .into_any_element(),
                    None => menu_separator(("web-display-menu-separator", index), row_height, cx)
                        .into_any_element(),
                });
            let content_bounds = Rc::new(Cell::new(Bounds::default()));
            let measured = content_bounds.clone();
            let content = div()
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .on_prepaint(move |bounds, _, _| measured.set(bounds))
                .children(rows);
            let surface = div()
                .absolute()
                .left(frame.bounds.origin.x)
                .top(frame.bounds.origin.y)
                .w(frame.bounds.size.width)
                .h(frame.bounds.size.height)
                .child(
                    FloatingSurface::new("web-display-menu-surface", content, cx)
                        .title(state.title.clone())
                        .content_inset(frame.inset_x, frame.inset_y)
                        .colors(background, foreground, border_color)
                        .bordered(bordered),
                );
            let state = Rc::new(state);
            let (press_state, press_bounds) = (state.clone(), content_bounds.clone());
            let (release_state, release_bounds) = (state.clone(), content_bounds.clone());
            let (move_state, move_bounds) = (state.clone(), content_bounds.clone());
            return Some(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .capture_any_mouse_down(cx.listener(
                        move |this, event: &MouseDownEvent, window, cx| {
                            this.menu_pointer(
                                &press_state,
                                press_bounds.get(),
                                MenuPointerKind::Press,
                                menu_press_buttons(event.button),
                                event.position,
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        },
                    ))
                    .capture_any_mouse_up(cx.listener(
                        move |this, event: &MouseUpEvent, window, cx| {
                            this.menu_pointer(
                                &release_state,
                                release_bounds.get(),
                                MenuPointerKind::Release,
                                RELEASE_BUTTONS,
                                event.position,
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        },
                    ))
                    .on_mouse_move(
                        cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                            let (kind, buttons) = event
                                .pressed_button
                                .map_or((MenuPointerKind::Motion, RELEASE_BUTTONS), |button| {
                                    (MenuPointerKind::Drag, menu_press_buttons(button))
                                });
                            this.menu_pointer(
                                &move_state,
                                move_bounds.get(),
                                kind,
                                buttons,
                                event.position,
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(
                        move |this, event: &ScrollWheelEvent, window, cx| {
                            this.menu_pointer(
                                &state,
                                content_bounds.get(),
                                MenuPointerKind::Wheel,
                                WHEEL_BUTTONS,
                                event.position,
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        },
                    ))
                    .child(surface)
                    .into_any_element(),
            );
        }
        let state = core.confirm()?;
        let width = px((zz_protocol::display_width(&state.prompt)
            .saturating_mul(8)
            .saturating_add(32)) as f32)
        .max(px(180.0))
        .min((canvas.width - px(24.0)).max(px(180.0)));
        let height = px(48.0);
        Some(
            div()
                .absolute()
                .left(((canvas.width - width) / 2.0).max(px(0.0)))
                .top(((canvas.height - height) / 2.0).max(px(0.0)))
                .w(width)
                .h(height)
                .child(FloatingSurface::new(
                    "web-confirm-before-surface",
                    confirm_prompt(
                        "web-confirm-before-input",
                        state.prompt.clone(),
                        cx.theme().mono_font_family.clone(),
                    ),
                    cx,
                ))
                .into_any_element(),
        )
    }

    fn menu_pointer(
        &mut self,
        state: &MenuState,
        content_bounds: Bounds<Pixels>,
        kind: MenuPointerKind,
        buttons: u8,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let bordered = state.border_lines != PopupBorderLines::None;
        let hairline = px(if bordered { 1.0 } else { 0.0 });
        let inset = u16::from(bordered);
        let column = menu_grid_cell(
            position.x - content_bounds.origin.x + hairline,
            state.cell_width_px,
            window.scale_factor(),
            state.left.saturating_add(inset),
            u16::MAX,
        );
        let row = menu_grid_cell(
            position.y - content_bounds.origin.y + hairline,
            state.cell_height_px,
            window.scale_factor(),
            state.top.saturating_add(inset),
            0,
        );
        match resolve_menu_mouse(
            state,
            self.menu_selection,
            MenuBox {
                left: state.left,
                top: state.top,
                width: state.width,
                items: state.items.len(),
            },
            kind,
            buttons,
            column,
            row,
        ) {
            MenuKeyResult::Action(action) => self.send_input(InputMessage::Menu { action }, cx),
            MenuKeyResult::Select(selected) => {
                if self.menu_selection != selected {
                    self.menu_selection = selected;
                    cx.notify();
                }
            }
            MenuKeyResult::Consumed => {}
        }
    }
}

pub(super) fn style_color(style: &str, key: &str, fallback: Hsla, cx: &App) -> Hsla {
    let Some(value) = style.split(',').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.eq_ignore_ascii_case(key).then_some(value)
    }) else {
        return fallback;
    };
    let Some(color) = parse_tmux_colour(value) else {
        return fallback;
    };
    let packed = match color {
        TmuxColour::Rgb(color) => color,
        TmuxColour::Basic(index) | TmuxColour::Indexed(index) => {
            zz_protocol::indexed_colour_rgb(index)
        }
        TmuxColour::Default | TmuxColour::Terminal => return fallback,
        TmuxColour::Theme(index) => {
            return match index {
                0 => cx.theme().background,
                1 | 7..=9 => cx.theme().foreground,
                2 => cx.theme().border,
                3 => cx.theme().background.raised(1).opaque(),
                4 => cx.theme().success,
                5 => cx.theme().warning,
                6 => cx.theme().danger,
                _ => fallback,
            };
        }
    };
    let channel =
        |shift: u32| f32::from(u8::try_from((packed >> shift) & 0xff).unwrap_or_default()) / 255.0;
    gpui::Rgba {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: 1.0,
    }
    .into()
}
