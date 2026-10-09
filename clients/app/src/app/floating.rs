use std::{cell::Cell, rc::Rc};

use zpui::{
    AnyElement, Bounds, Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollWheelEvent, Window, div, point, prelude::*, px, size,
};
use zz_client::{MenuBox, MenuKeyResult, MenuPointerKind, resolve_menu_mouse};
use zz_protocol::{InputMessage, MenuState, PopupBorderLines};
use zz_ui::{
    ActiveTheme as _, Colorize as _, ElementExt as _,
    command::floating::{
        RELEASE_BUTTONS, WHEEL_BUTTONS, confirm_prompt, floating_frame, menu_grid_cell,
        menu_press_buttons, menu_row, menu_separator,
    },
    pane::FloatingSurface,
};

use super::AppShell;

impl AppShell {
    pub(super) fn floating_canvas_size(&self, window: &Window) -> zpui::Size<Pixels> {
        let viewport = window.fully_visible_bounds().size;
        size(
            (viewport.width
                - px(if self.inline_sidebar(window) {
                    self.sidebar_width(window)
                } else {
                    0.0
                }))
            .max(px(0.0)),
            (viewport.height
                - if self.settings.is_none() && !self.inline_sidebar(window) {
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
            let border_color = style_color(&state.border_style, "fg", cx.theme().border(), cx);
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
                                (MenuPointerKind::Press, menu_press_buttons(event.button)),
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
                                (MenuPointerKind::Release, RELEASE_BUTTONS),
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
                                (kind, buttons),
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
                                (MenuPointerKind::Wheel, WHEEL_BUTTONS),
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
        (kind, buttons): (MenuPointerKind, u8),
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

pub(super) use zz_ui::tmux_style::tmux_style_colour as style_color;

impl AppShell {
    pub(super) fn cell_size(
        &self,
        active: &zz_protocol::WindowSnapshot,
        cx: &Context<Self>,
    ) -> (f32, f32) {
        std::iter::once(active.active_pane)
            .chain(self.terminals.keys().copied())
            .find_map(|pane| self.terminals.get(&pane))
            .map_or((8.0, 18.0), |terminal| {
                let (width, height) = terminal.read(cx).cell_size();
                (f32::from(width), f32::from(height))
            })
    }

    pub(super) fn float_layer(
        &self,
        active: &zz_protocol::WindowSnapshot,
        floats: &[zz_protocol::FloatingPaneSnapshot],
        panes: &mut std::collections::HashMap<zz_protocol::PaneId, AnyElement>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let cell = self.cell_size(active, cx);
        let canvas = self.pane_canvas_bounds.get().size;
        let canvas = (f32::from(canvas.width), f32::from(canvas.height));
        let mut layer = Vec::new();
        for float in floats {
            let Some(content) = panes.remove(&float.pane) else {
                continue;
            };
            if let Some(modal) = active.modal.filter(|modal| modal.pane == float.pane) {
                let pane = modal.pane;
                layer.push(
                    div()
                        .id(("web-modal-scrim", pane.0))
                        .absolute()
                        .inset_0()
                        .occlude()
                        .bg(cx.theme().background.opacity(0.4))
                        .on_mouse_down(
                            zpui::MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                if modal.close_on_click {
                                    this.command(
                                        "kill-pane",
                                        vec!["-t".into(), pane.to_string()],
                                        cx,
                                    );
                                }
                                cx.stop_propagation();
                            }),
                        )
                        .into_any_element(),
                );
            }
            let bordered = float.border_lines != zz_protocol::PaneBorderLines::None;
            let Some(placed) = zz_client::floating::float_pixels(
                zz_client::floating::FloatCells::from(float),
                bordered,
                cell,
                canvas,
            ) else {
                continue;
            };
            let title = active
                .panes
                .get(&float.pane)
                .filter(|_| float.border_status.is_on())
                .map(|pane| pane.border_status_text.clone())
                .unwrap_or_default();
            let border_color = if active.active_pane == float.pane {
                cx.theme().accent
            } else {
                cx.theme().border()
            };
            let frame = placed.frame;
            let inset = |content: Option<f32>, frame: f32| {
                px(content.map_or(0.0, |content| content - frame))
            };
            layer.push(
                div()
                    .absolute()
                    .left(px(frame.x))
                    .top(px(frame.y))
                    .w(px(frame.width))
                    .h(px(frame.height))
                    .child(
                        FloatingSurface::new(("web-float", float.pane.0), content, cx)
                            .title(title)
                            .content_inset(
                                inset(placed.content.map(|content| content.x), frame.x),
                                inset(placed.content.map(|content| content.y), frame.y),
                            )
                            .colors(
                                cx.theme().background.raised(1).opaque(),
                                cx.theme().foreground,
                                border_color,
                            )
                            .bordered(bordered),
                    )
                    .into_any_element(),
            );
        }
        layer
    }
}
