use gpui::{
    App, Context, CursorStyle, ElementId, Entity, IntoElement, Pixels, Point, Render, Window, div,
    prelude::*, px,
};
use zz_protocol::PaneId;

use crate::IconName;

use super::pane_header_icon_button;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneDrag {
    pub pane: PaneId,
    pub requires_prefix: bool,
}

pub struct PaneDragPreview {
    pane: PaneId,
    title: String,
    grab: Point<Pixels>,
}

impl Render for PaneDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.grab.x + px(12.0))
            .pt(self.grab.y + px(12.0))
            .child(super::pane_drag_chip(
                self.pane.to_string(),
                self.title.clone(),
                cx,
            ))
    }
}

pub fn pane_drag_preview(
    pane: PaneId,
    title: String,
    grab: Point<Pixels>,
    cx: &mut App,
) -> Entity<PaneDragPreview> {
    cx.new(|_| PaneDragPreview { pane, title, grab })
}

pub fn pane_drag_button(
    id: impl Into<ElementId>,
    pane: PaneId,
    title: String,
    enabled: bool,
    on_start: impl Fn(&PaneDrag, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .aria_label("Drag pane")
        .debug_selector(|| "pane-drag-handle".into())
        .flex_none()
        .child(
            pane_header_icon_button("pane-drag-control", IconName::GripVertical, enabled, cx)
                .tooltip("Drag pane · hold and move to split or swap")
                .when(enabled, |button| button.cursor(CursorStyle::OpenHand)),
        )
        .when(enabled, |button| {
            button.cursor(CursorStyle::OpenHand).on_drag(
                PaneDrag {
                    pane,
                    requires_prefix: false,
                },
                move |drag, grab, window, cx| {
                    on_start(drag, window, cx);
                    window.defer(cx, |window, cx| {
                        cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                    });
                    pane_drag_preview(drag.pane, title.clone(), grab, cx)
                },
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, MouseButton, TestAppContext, point};

    struct DragTest {
        enabled: bool,
        started: Vec<PaneDrag>,
        dropped: Vec<PaneDrag>,
    }

    impl Render for DragTest {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = cx.entity();
            div()
                .flex()
                .w(px(240.0))
                .h(px(100.0))
                .child(pane_drag_button(
                    "test-handle",
                    PaneId(3),
                    "Session".into(),
                    self.enabled,
                    move |drag, _, cx| view.update(cx, |view, _| view.started.push(*drag)),
                    cx,
                ))
                .child(
                    div().id("test-drop").w(px(200.0)).h(px(100.0)).on_drop(
                        cx.listener(|view, drag: &PaneDrag, _, _| view.dropped.push(*drag)),
                    ),
                )
        }
    }

    #[gpui::test]
    fn handle_starts_without_prefix_and_drops_after_a_single_move(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, _| DragTest {
            enabled: true,
            started: Vec::new(),
            dropped: Vec::new(),
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let start = cx.debug_bounds("pane-drag-handle").unwrap().center();
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        assert!(!cx.update(|_, cx| cx.has_active_drag()));
        let target = point(px(150.0), px(12.0));
        cx.simulate_mouse_move(target, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        assert!(cx.update(|_, cx| cx.has_active_drag()));
        let payload = PaneDrag {
            pane: PaneId(3),
            requires_prefix: false,
        };
        assert_eq!(
            view.read_with(cx, |view, _| view.started.clone()),
            [payload]
        );
        cx.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| cx.has_active_drag()));
        assert_eq!(
            view.read_with(cx, |view, _| view.dropped.clone()),
            [payload]
        );

        view.update(cx, |view, cx| {
            view.enabled = false;
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(target, MouseButton::Left, Modifiers::default());
        assert!(!cx.update(|_, cx| cx.has_active_drag()));
        cx.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
        assert_eq!(
            view.read_with(cx, |view, _| view.started.clone()),
            [payload]
        );
    }
}
