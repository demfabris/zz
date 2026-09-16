//! Making a titlebar-height strip move the window, the way a title bar does.

use gpui::{App, ElementId, MouseButton, MouseMoveEvent, Window, prelude::FluentBuilder};
use zz_ui::InteractiveElementExt;

struct DragCandidate {
    armed: bool,
    accepts_click: bool,
}

/// Drags `strip` to move the window; double click zooms. `key` must be unique
/// among the strips rendered at once. The caller declares the
/// `WindowControlArea::Drag` hitbox, which is all Windows needs.
pub(crate) fn window_drag_handle<E: InteractiveElementExt + FluentBuilder>(
    key: impl Into<ElementId>,
    strip: E,
    window: &mut Window,
    cx: &mut App,
) -> E {
    if cfg!(target_os = "windows") {
        return strip;
    }
    let state = window.use_keyed_state(key.into(), cx, |_, _| DragCandidate {
        armed: false,
        accepts_click: false,
    });
    strip
        .when(cfg!(target_os = "linux"), |strip| {
            strip.on_double_click(window.listener_for(&state, |state, _, window, _| {
                if state.accepts_click {
                    window.zoom_window();
                }
            }))
        })
        .when(cfg!(target_os = "macos"), |strip| {
            strip.on_double_click(window.listener_for(&state, |state, _, window, _| {
                if state.accepts_click {
                    window.titlebar_double_click();
                }
            }))
        })
        .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| {
            state.armed = false;
            state.accepts_click = false;
        }))
        .on_mouse_down(
            MouseButton::Left,
            window.listener_for(&state, |state, _, window, _| {
                state.accepts_click = !window.default_prevented();
                state.armed = state.accepts_click;
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            window.listener_for(&state, |state, _, _, _| {
                state.armed = false;
            }),
        )
        .on_mouse_move(
            window.listener_for(&state, |state, event: &MouseMoveEvent, window, _| {
                if state.armed {
                    state.armed = false;
                    if event.pressed_button == Some(MouseButton::Left) {
                        window.start_window_move();
                    }
                }
            }),
        )
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Context, Entity, FocusHandle, Render, div, prelude::*, px};
    use zz_ui::{
        Sizable as _,
        button::Button,
        h_flex,
        input::{Input, InputState},
    };

    struct Preview {
        focus: FocusHandle,
        input: Entity<InputState>,
        drag: Option<Entity<DragCandidate>>,
    }

    impl Render for Preview {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let drag = window_drag_handle(
                "settings-drag-test",
                div()
                    .id("settings-drag-test-strip")
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(zz_ui::TITLE_BAR_HEIGHT),
                window,
                cx,
            );
            self.drag =
                Some(window.use_keyed_state("settings-drag-test", cx, |_, _| unreachable!()));
            div()
                .track_focus(&self.focus)
                .relative()
                .w(px(400.0))
                .h(px(200.0))
                .child(drag)
                .child(
                    h_flex()
                        .h(zz_ui::TITLE_BAR_HEIGHT)
                        .child(
                            div()
                                .w(px(80.0))
                                .debug_selector(|| "drag-button".into())
                                .child(Button::new("drag-button").small().label("Reset")),
                        )
                        .child(
                            div()
                                .w(px(120.0))
                                .debug_selector(|| "drag-input".into())
                                .child(Input::new(&self.input).small()),
                        ),
                )
        }
    }

    #[gpui::test]
    fn background_drag_yields_to_buttons_and_text_fields(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        let (preview, cx) = cx.add_window_view(|window, cx| Preview {
            focus: cx.focus_handle(),
            input: cx.new(|cx| InputState::new(window, cx)),
            drag: None,
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let state = preview.read_with(cx, |preview, _| preview.drag.clone().unwrap());
        let background = gpui::point(px(300.0), px(15.0));
        cx.simulate_mouse_down(background, MouseButton::Left, gpui::Modifiers::default());
        assert!(state.read_with(cx, |state: &DragCandidate, _| state.armed));
        cx.simulate_mouse_up(background, MouseButton::Left, gpui::Modifiers::default());
        assert!(!state.read_with(cx, |state, _| state.armed));

        for selector in ["drag-button", "drag-input"] {
            let bounds = cx.debug_bounds(selector).unwrap();
            cx.simulate_mouse_down(
                bounds.center(),
                MouseButton::Left,
                gpui::Modifiers::default(),
            );
            assert!(!state.read_with(cx, |state, _| state.armed), "{selector}");
            assert!(
                !state.read_with(cx, |state, _| state.accepts_click),
                "{selector}"
            );
            cx.simulate_mouse_move(
                bounds.center(),
                Some(MouseButton::Left),
                gpui::Modifiers::default(),
            );
            cx.simulate_mouse_up(
                bounds.center(),
                MouseButton::Left,
                gpui::Modifiers::default(),
            );
        }
    }
}
