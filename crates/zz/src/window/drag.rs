//! Making a titlebar-height strip move the window, the way a title bar does.

use gpui::{App, ElementId, MouseButton, MouseMoveEvent, Window, prelude::FluentBuilder};
use zz_ui::InteractiveElementExt;

struct DragCandidate {
    armed: bool,
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
    let state = window.use_keyed_state(key.into(), cx, |_, _| DragCandidate { armed: false });
    strip
        .when(cfg!(target_os = "linux"), |strip| {
            strip.on_double_click(|_, window, _| window.zoom_window())
        })
        .when(cfg!(target_os = "macos"), |strip| {
            strip.on_double_click(|_, window, _| window.titlebar_double_click())
        })
        .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| {
            state.armed = false;
        }))
        .on_mouse_down(
            MouseButton::Left,
            window.listener_for(&state, |state, _, _, _| {
                state.armed = true;
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
