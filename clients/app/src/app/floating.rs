use std::{cell::RefCell, rc::Rc};

use zz_client::{
    MenuBox, MenuKeyResult, MenuPointerKind,
    floating::{FloatCells, float_pixels, float_title},
    resolve_menu_mouse,
};
use zz_gpui::{
    AnchoredPositionMode, AnyElement, App, Bounds, Context, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollWheelEvent, Window, anchored, div, point, prelude::*, px,
    size,
};
use zz_protocol::{
    FloatingPaneSnapshot, InputMessage, MenuState, PaneBorderLines, PopupBorderLines,
};
use zz_protocol::{PaneId, WindowSnapshot};
use zz_ui::{
    ActiveTheme as _,
    command::floating::{
        MenuHits, RELEASE_BUTTONS, WHEEL_BUTTONS, confirm_prompt, display_menu, floating_frame,
        menu_enter, menu_hit_cell, menu_press_buttons,
    },
    pane::{FloatingSurface, PipGrab, moving_pane},
    pip::{PipCard, PipDrag, PipLayout, PipStage, PipSurface, PipThrow},
};

use super::AppShell;
use crate::preferences::Preferences;

#[derive(Clone, Copy, Debug)]
pub(super) struct AppPipDrag {
    pane: PaneId,
    drag: PipDrag,
    started: bool,
    touch: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AppPipLanding {
    pane: PaneId,
    target: FloatCells,
    at: zz_gpui::scheduler::Instant,
}

#[derive(Default)]
pub(super) struct PipState {
    drag: Option<AppPipDrag>,
    throw: PipThrow,
    landing: Option<AppPipLanding>,
    pub(super) stage: PipStage,
}

impl PipState {
    fn thrown(&self, pane: PaneId) -> Option<PipDrag> {
        self.drag
            .filter(|drag| drag.pane == pane && drag.started)
            .map(|drag| drag.drag)
    }

    fn landing(&self, pane: PaneId) -> Option<AppPipLanding> {
        self.landing.filter(|landing| landing.pane == pane)
    }
}

impl AppShell {
    pub(super) fn floating_canvas_size(&self, window: &Window, cx: &App) -> zz_gpui::Size<Pixels> {
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
                    cx.theme().title_bar_height()
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
        let canvas = self.floating_canvas_size(window, cx);
        if let Some(state) = core.menu().cloned() {
            let frame = floating_frame(
                state.left,
                state.top,
                state.width,
                state.height,
                state.client_columns,
                state.client_rows,
                state.cell_width_px,
                state.cell_height_px,
                state.border_lines != PopupBorderLines::None,
                point(px(0.0), px(0.0)),
                canvas,
                window.scale_factor(),
            );
            let hits = Rc::new(RefCell::new(MenuHits::default()));
            let surface = menu_enter(display_menu(
                &state,
                self.menu_selection,
                window.viewport_size(),
                &hits,
                cx,
            ));
            let state = Rc::new(state);
            let (press_state, press_hits) = (state.clone(), hits.clone());
            let (release_state, release_hits) = (state.clone(), hits.clone());
            let (move_state, move_hits) = (state.clone(), hits.clone());
            return Some(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .capture_any_mouse_down(cx.listener(
                        move |this, event: &MouseDownEvent, _, cx| {
                            this.menu_pointer(
                                &press_state,
                                &press_hits.borrow(),
                                (MenuPointerKind::Press, menu_press_buttons(event.button)),
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        },
                    ))
                    .capture_any_mouse_up(cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                        this.menu_pointer(
                            &release_state,
                            &release_hits.borrow(),
                            (MenuPointerKind::Release, RELEASE_BUTTONS),
                            event.position,
                            cx,
                        );
                        cx.stop_propagation();
                    }))
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        let (kind, buttons) = event
                            .pressed_button
                            .map_or((MenuPointerKind::Motion, RELEASE_BUTTONS), |button| {
                                (MenuPointerKind::Drag, menu_press_buttons(button))
                            });
                        this.menu_pointer(
                            &move_state,
                            &move_hits.borrow(),
                            (kind, buttons),
                            event.position,
                            cx,
                        );
                        cx.stop_propagation();
                    }))
                    .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                        this.menu_pointer(
                            &state,
                            &hits.borrow(),
                            (MenuPointerKind::Wheel, WHEEL_BUTTONS),
                            event.position,
                            cx,
                        );
                        cx.stop_propagation();
                    }))
                    .child(
                        anchored()
                            .position_mode(AnchoredPositionMode::Local)
                            .position(frame.bounds.origin)
                            .snap_to_window_with_margin(px(8.0))
                            .child(surface),
                    )
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
        hits: &MenuHits,
        (kind, buttons): (MenuPointerKind, u8),
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !state.mouse_keys && kind == MenuPointerKind::Motion {
            return;
        }
        let (column, row) = menu_hit_cell(state, hits, position);
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

    fn begin_pip(
        &mut self,
        float: FloatingPaneSnapshot,
        press: Point<Pixels>,
        touch: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_window(cx) else {
            return;
        };
        let cell = self.cell_size(&active, cx);
        let now = cx.background_executor().now();
        let start = self
            .pip
            .landing
            .take_if(|landing| landing.pane == float.pane)
            .map_or_else(|| FloatCells::from(&float), |landing| landing.target);
        let layout = PipLayout {
            window: (active.sx, active.sy),
            cell: size(px(cell.0), px(cell.1)),
            canvas: self.float_canvas_bounds.get().size,
            lines: float.border_lines,
            header: cx.theme().pane_header_height(),
        };
        let shown = self
            .pane_motion
            .drawn(float.pane)
            .map_or_else(Point::default, |drawn| {
                drawn.origin - self.float_canvas_bounds.get().origin - layout.frame(start).origin
            });
        let drag = PipDrag::new(start, layout, press, shown);
        self.pip.throw = PipThrow::default();
        self.pip.throw.track(now, press);
        self.pip.drag = Some(AppPipDrag {
            pane: float.pane,
            drag,
            started: false,
            touch,
        });
        self.command("select-pane", vec!["-t".into(), float.pane.to_string()], cx);
    }

    fn move_pip(&mut self, pointer: Point<Pixels>, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let Some(drag) = self.pip.drag.as_mut() else {
            return;
        };
        drag.started = true;
        drag.drag.moved(pointer);
        self.pip.throw.track(now, pointer);
        cx.notify();
    }

    fn release_pip(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.pip.drag.take() else {
            return;
        };
        cx.notify();
        if !drag.started {
            return;
        }
        let now = cx.background_executor().now();
        let velocity = self.pip.throw.velocity(now);
        let landing = drag.drag.release(velocity);
        if landing.stash.is_some() {
            self.pip.stage.stash(drag.pane);
        }
        self.pane_motion
            .fling(drag.pane, velocity, landing.stash.is_none());
        let command = landing.place.command(drag.pane);
        self.invoke(command, cx);
        self.pip.landing = Some(AppPipLanding {
            pane: drag.pane,
            target: landing.target,
            at: now,
        });
    }

    pub(super) fn tick_pip(
        &mut self,
        active: &WindowSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .pip
            .drag
            .is_some_and(|drag| drag.started && !drag.touch)
            && !cx.has_active_drag()
        {
            self.release_pip(cx);
        }
        let Some(landing) = self.pip.landing else {
            return;
        };
        let now = cx.background_executor().now();
        let landed = active
            .floating
            .iter()
            .any(|float| float.pane == landing.pane && FloatCells::from(float) == landing.target);
        if landed || now.saturating_duration_since(landing.at) > std::time::Duration::from_secs(1) {
            self.pip.landing = None;
        } else {
            window.request_animation_frame();
        }
    }

    pub(super) fn touch_pip(
        &mut self,
        float: FloatingPaneSnapshot,
        event: &zz_gpui::TouchDragEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.phase {
            zz_gpui::TouchPhase::Started => {
                self.begin_pip(float, event.start_position, true, cx);
                self.move_pip(event.position, cx);
                window.prevent_default();
            }
            zz_gpui::TouchPhase::Moved => self.move_pip(event.position, cx),
            zz_gpui::TouchPhase::Ended => self.release_pip(cx),
            zz_gpui::TouchPhase::Cancelled => {
                self.pip.drag = None;
                cx.notify();
            }
        }
        cx.stop_propagation();
    }

    pub(super) fn grab_pip(
        &mut self,
        pane: PaneId,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.pip.drag.is_none_or(|drag| drag.pane != pane) {
            let Some(float) = self.active_window(cx).and_then(|active| {
                active
                    .floating
                    .iter()
                    .find(|float| float.pane == pane)
                    .copied()
            }) else {
                return;
            };
            self.begin_pip(float, position, false, cx);
        }
        if self.pip.drag.is_some_and(|drag| !drag.touch) {
            self.move_pip(position, cx);
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a PiP needs its window, float, content, stash, title and inset"
    )]
    fn pip_surface(
        &self,
        active: &WindowSnapshot,
        float: FloatingPaneSnapshot,
        content: AnyElement,
        stashed: bool,
        title: String,
        inset: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> PipSurface {
        let pane = float.pane;
        let snapshot = active.panes.get(&pane);
        let fill = self.terminals.get(&pane).map_or_else(
            || cx.theme().pane_color(cx.theme().background),
            |terminal| terminal.read(cx).fill(cx),
        );
        let surface = PipSurface::new(("web-float", pane.0), content)
            .bordered(float.border_lines != PaneBorderLines::None)
            .fill(fill)
            .stashed(
                stashed,
                cx.listener(move |this, _: &zz_gpui::ClickEvent, _, cx| {
                    this.pip.stage.restore(pane);
                    this.command("select-pane", vec!["-t".into(), pane.to_string()], cx);
                }),
            );
        if snapshot.is_some_and(|pane| draws_own_header(&pane.kind)) {
            return surface;
        }
        let label = if title.is_empty() {
            snapshot
                .map(zz_client::navigation::pane_label)
                .unwrap_or_default()
        } else {
            title
        };
        let icon = snapshot.map_or(zz_ui::IconName::SquareTerminal, |pane| {
            zz_ui::pane::pane_kind_icon(&pane.kind)
        });
        let focused = active.active_pane == pane;
        let writable = !self.connection.read(cx).core.attached_read_only();
        let header = self.pane_header(pane, icon, label, focused, writable, true, cx);
        surface.content_inset(inset.x, inset.y).header(
            div()
                .opacity(if focused {
                    1.0
                } else {
                    self.preferences.pane_inactive_opacity
                })
                .child(header),
        )
    }

    pub(super) fn float_layer(
        &self,
        active: &zz_protocol::WindowSnapshot,
        floats: &[zz_protocol::FloatingPaneSnapshot],
        panes: &mut std::collections::HashMap<zz_protocol::PaneId, AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let cell = self.cell_size(active, cx);
        let canvas = self.float_canvas_bounds.get().size;
        let header = cx.theme().pane_header_height();
        let laid = floats
            .iter()
            .filter_map(|float| {
                let modal = active.modal.is_some_and(|modal| modal.pane == float.pane);
                let thrown = self.pip.thrown(float.pane);
                let landing = self.pip.landing(float.pane);
                let placed_cells = thrown
                    .map(|drag| drag.start)
                    .or(landing.map(|landing| landing.target));
                let placed_float = placed_cells.map_or(*float, |cells| FloatingPaneSnapshot {
                    xoff: cells.xoff,
                    yoff: cells.yoff,
                    ..*float
                });
                let placed = float_placement(&placed_float, cell, canvas)?;
                let mut frame = if modal {
                    placed.frame
                } else {
                    PipLayout {
                        window: (active.sx, active.sy),
                        cell: size(px(cell.0), px(cell.1)),
                        canvas,
                        lines: float.border_lines,
                        header,
                    }
                    .frame(FloatCells::from(&placed_float))
                };
                if let Some(drag) = thrown {
                    frame.origin = drag.drawn_origin();
                }
                Some((*float, modal, placed, frame, thrown.is_some()))
            })
            .collect::<Vec<_>>();
        let cards = laid
            .iter()
            .rev()
            .filter(|(_, modal, ..)| !modal)
            .map(|(float, _, _, frame, held)| PipCard {
                pane: float.pane,
                frame: *frame,
                held: *held,
            })
            .collect::<Vec<_>>();
        let arranged = self
            .pip
            .stage
            .arrange(&cards, canvas, header)
            .into_iter()
            .map(|placed| (placed.pane, placed))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut layer = Vec::new();
        for (float, modal, placed, mut frame, held) in laid {
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
                        .bg(cx.theme().scrim)
                        .on_mouse_down(
                            zz_gpui::MouseButton::Left,
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
            let title = active
                .panes
                .get(&float.pane)
                .filter(|_| float.border_status.is_on())
                .map(|pane| float_title(&pane.border_status_text).left)
                .unwrap_or_default();
            let inset = placed.content.origin - placed.frame.origin;
            let bordered = float.border_lines != PaneBorderLines::None;
            let surface = if modal {
                let border_color =
                    zz_ui::pane::pane_border_color(active.active_pane == float.pane, cx);
                FloatingSurface::new(("web-float", float.pane.0), content, cx)
                    .title(title)
                    .content_inset(inset.x, inset.y)
                    .edge_color(border_color)
                    .bordered(bordered)
                    .into_any_element()
            } else {
                let arranged = arranged.get(&float.pane).copied();
                if let Some(arranged) = arranged {
                    frame = arranged.frame;
                }
                let stashed = arranged.is_some_and(|arranged| arranged.stashed.is_some());
                let surface = self.pip_surface(active, float, content, stashed, title, inset, cx);
                moving_pane(float.pane, &self.pane_motion, surface)
                    .held(held)
                    .into_any_element()
            };
            layer.push(
                div()
                    .absolute()
                    .left(frame.origin.x)
                    .top(frame.origin.y)
                    .w(frame.size.width)
                    .h(frame.size.height)
                    .child(surface)
                    .into_any_element(),
            );
        }
        (!layer.is_empty()).then(|| {
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .on_drag_move::<PipGrab>(cx.listener(
                    |this, event: &zz_gpui::DragMoveEvent<PipGrab>, _, cx| {
                        let pane = event.drag(cx).pane;
                        this.grab_pip(pane, event.event.position, cx);
                    },
                ))
                .children(layer)
                .into_any_element()
        })
    }
}

pub(super) fn draws_own_header(kind: &zz_protocol::PaneKindSnapshot) -> bool {
    const NATIVE_BROWSER: bool = cfg!(any(target_os = "ios", target_os = "android"));
    match kind {
        zz_protocol::PaneKindSnapshot::Agent(_) => true,
        zz_protocol::PaneKindSnapshot::Browser(_) => NATIVE_BROWSER,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PaneFrame {
    pub header: bool,
    pub framed: bool,
    pub radius: f32,
}

pub(super) fn pane_frame(
    terminal: bool,
    floating: bool,
    preferences: &Preferences,
    pane_radius: f32,
) -> PaneFrame {
    let gaps = preferences.gaps && !floating;
    PaneFrame {
        header: terminal && !floating,
        framed: gaps,
        radius: if gaps { pane_radius } else { 0.0 },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FloatPlacement {
    pub frame: Bounds<Pixels>,
    pub content: Bounds<Pixels>,
}

#[allow(
    clippy::cast_precision_loss,
    reason = "window cell offsets are far below f32's exact integer range"
)]
pub(super) fn float_placement(
    float: &FloatingPaneSnapshot,
    cell: (f32, f32),
    canvas: zz_gpui::Size<Pixels>,
) -> Option<FloatPlacement> {
    let bordered = float.border_lines != PaneBorderLines::None;
    if canvas.width > Pixels::ZERO
        && canvas.height > Pixels::ZERO
        && float_pixels(
            FloatCells::from(float),
            bordered,
            cell,
            (f32::from(canvas.width), f32::from(canvas.height)),
        )
        .is_none()
    {
        return None;
    }
    let pad = i32::from(bordered);
    let rect = |x: i32, y: i32, width: i32, height: i32| {
        Bounds::new(
            point(px(x as f32 * cell.0), px(y as f32 * cell.1)),
            size(px(width as f32 * cell.0), px(height as f32 * cell.1)),
        )
    };
    Some(FloatPlacement {
        frame: rect(
            float.xoff - pad,
            float.yoff - pad,
            i32::from(float.sx) + 2 * pad,
            i32::from(float.sy) + 2 * pad,
        ),
        content: rect(
            float.xoff,
            float.yoff,
            i32::from(float.sx),
            i32::from(float.sy),
        ),
    })
}

#[cfg(any(target_os = "ios", target_os = "android", test))]
pub(super) fn native_panes_shown(
    tiled: impl IntoIterator<Item = (zz_protocol::PaneId, Option<Bounds<Pixels>>)>,
    floats: &[(zz_protocol::PaneId, FloatPlacement)],
    modal: Option<zz_protocol::PaneId>,
) -> std::collections::BTreeSet<zz_protocol::PaneId> {
    let covered = |bounds: Bounds<Pixels>, front: &[(zz_protocol::PaneId, FloatPlacement)]| {
        front
            .iter()
            .any(|(_, placed)| placed.frame.intersects(&bounds))
    };
    tiled
        .into_iter()
        .filter(|(_, bounds)| bounds.map_or(floats.is_empty(), |bounds| !covered(bounds, floats)))
        .map(|(pane, _)| pane)
        .chain(
            floats
                .iter()
                .enumerate()
                .filter(|(index, (_, placed))| !covered(placed.content, &floats[..*index]))
                .map(|(_, (pane, _))| *pane),
        )
        .filter(|pane| modal.is_none_or(|modal| modal == *pane))
        .collect()
}

#[cfg(test)]
mod tests {
    use zz_gpui::{Bounds, point, px, size};
    use zz_protocol::{FloatingPaneSnapshot, PaneBorderLines, PaneBorderStatus, PaneId};

    use super::{FloatPlacement, float_placement, native_panes_shown, pane_frame};
    use crate::preferences::Preferences;

    fn float(pane: u64, xoff: i32, yoff: i32, sx: u16, sy: u16) -> FloatingPaneSnapshot {
        FloatingPaneSnapshot {
            pane: PaneId(pane),
            xoff,
            yoff,
            sx,
            sy,
            visible: true,
            border_lines: PaneBorderLines::Single,
            border_status: PaneBorderStatus::Off,
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<zz_gpui::Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn a_floating_terminal_keeps_its_whole_box_for_the_grid() {
        let preferences = Preferences {
            gaps: true,
            ..Preferences::default()
        };
        assert_eq!(
            pane_frame(true, true, &preferences, 10.0),
            super::PaneFrame {
                header: false,
                framed: false,
                radius: 0.0,
            }
        );
        let tiled = pane_frame(true, false, &preferences, 10.0);
        assert!(tiled.header);
        assert!(tiled.framed);
        assert_eq!(tiled.radius, 10.0);
        assert!(!pane_frame(false, false, &preferences, 10.0).header);
        let gapless = Preferences {
            gaps: false,
            ..preferences
        };
        assert_eq!(pane_frame(true, false, &gapless, 10.0).radius, 0.0);
    }

    #[test]
    fn a_float_past_the_left_edge_is_cropped_not_reshaped() {
        let canvas = size(px(400.0), px(560.0));
        let placed = float_placement(&float(1, -3, 4, 10, 6), (8.0, 16.0), canvas).unwrap();
        assert_eq!(
            placed,
            FloatPlacement {
                frame: rect(-32.0, 48.0, 96.0, 128.0),
                content: rect(-24.0, 64.0, 80.0, 96.0),
            }
        );
        let inset = placed.content.origin - placed.frame.origin;
        assert_eq!((inset.x, inset.y), (px(8.0), px(16.0)));
        assert_eq!(
            placed.frame.size.width - placed.content.size.width,
            inset.x * 2.0
        );
        assert_eq!(
            float_placement(&float(1, 60, 4, 10, 6), (8.0, 16.0), canvas),
            None
        );
        assert!(
            float_placement(&float(1, 60, 4, 10, 6), (8.0, 16.0), size(px(0.0), px(0.0))).is_some()
        );
    }

    #[test]
    fn a_float_hides_only_the_native_browsers_it_covers() {
        let cell = (8.0, 16.0);
        let canvas = size(px(800.0), px(480.0));
        let left = (PaneId(1), Some(rect(0.0, 0.0, 400.0, 480.0)));
        let right = (PaneId(2), Some(rect(400.0, 0.0, 400.0, 480.0)));
        let front = (
            PaneId(3),
            float_placement(&float(3, 5, 5, 20, 10), cell, canvas).unwrap(),
        );
        let behind_front = (
            PaneId(4),
            float_placement(&float(4, 10, 8, 20, 10), cell, canvas).unwrap(),
        );
        let apart = (
            PaneId(5),
            float_placement(&float(5, 70, 20, 20, 5), cell, canvas).unwrap(),
        );
        assert_eq!(
            native_panes_shown([left, right], &[], None),
            [PaneId(1), PaneId(2)].into()
        );
        assert_eq!(
            native_panes_shown([left, right], &[front], None),
            [PaneId(2), PaneId(3)].into()
        );
        assert_eq!(
            native_panes_shown([left, right], &[front, behind_front, apart], None),
            [PaneId(3), PaneId(5)].into()
        );
        let border_over_left = (
            PaneId(6),
            float_placement(&float(6, 50, 5, 10, 5), cell, canvas).unwrap(),
        );
        assert!(!border_over_left.1.content.intersects(&left.1.unwrap()));
        assert_eq!(
            native_panes_shown([left, right], &[border_over_left], None),
            [PaneId(6)].into()
        );
        assert_eq!(
            native_panes_shown([(PaneId(1), None)], &[front], None),
            [PaneId(3)].into()
        );
    }

    #[test]
    fn a_modal_hides_every_native_browser_but_its_own() {
        let cell = (8.0, 16.0);
        let canvas = size(px(800.0), px(480.0));
        let left = (PaneId(1), Some(rect(0.0, 0.0, 400.0, 480.0)));
        let right = (PaneId(2), Some(rect(400.0, 0.0, 400.0, 480.0)));
        let modal = (
            PaneId(3),
            float_placement(&float(3, 5, 5, 20, 10), cell, canvas).unwrap(),
        );
        let behind = (
            PaneId(4),
            float_placement(&float(4, 60, 2, 10, 4), cell, canvas).unwrap(),
        );
        assert!(!modal.1.frame.intersects(&right.1.unwrap()));
        assert!(!modal.1.frame.intersects(&behind.1.frame));
        assert_eq!(
            native_panes_shown([left, right], &[modal], None),
            [PaneId(2), PaneId(3)].into()
        );
        assert_eq!(
            native_panes_shown([left, right], &[modal], Some(PaneId(3))),
            [PaneId(3)].into()
        );
        assert_eq!(
            native_panes_shown([left], &[modal, behind], None),
            [PaneId(3), PaneId(4)].into()
        );
        assert_eq!(
            native_panes_shown([left], &[modal, behind], Some(PaneId(3))),
            [PaneId(3)].into()
        );
        assert_eq!(
            native_panes_shown([left, right], &[behind], Some(PaneId(3))),
            [].into()
        );
    }
}
