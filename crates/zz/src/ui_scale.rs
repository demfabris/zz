//! Application-wide UI zoom, the browser kind.

use gpui::{App, KeyBinding, Window};
use zz_client::{ChromeAction, UI_TABLE};
use zz_ui::{ROOT_KEY_CONTEXT, UiZoom};

use crate::keymap::ChromeChord;

/// The clamp floor. Below this the chrome stops being legible.
pub const MIN_UI_ZOOM: f32 = 50.0;
pub const MAX_UI_ZOOM: f32 = 300.0;
/// One keystroke, or one press of the Settings field's steppers.
pub const UI_ZOOM_STEP: f32 = 10.0;
const DEFAULT_UI_ZOOM: f32 = 100.0;

gpui::actions!(zz, [IncreaseUiZoom, DecreaseUiZoom, ResetUiZoom]);

pub fn init(cx: &mut App) {
    crate::keymap::bind(cx, UI_TABLE, key_bindings);
    cx.on_action(|_: &IncreaseUiZoom, cx| {
        set_percent(effective_percent(cx) + UI_ZOOM_STEP, cx);
    });
    cx.on_action(|_: &DecreaseUiZoom, cx| {
        set_percent(effective_percent(cx) - UI_ZOOM_STEP, cx);
    });
    cx.on_action(|_: &ResetUiZoom, cx| {
        reset(cx);
    });
}

pub fn reset(cx: &mut App) {
    set_percent(DEFAULT_UI_ZOOM, cx);
}

/// Zoom to `percent`, clamped to the supported range.
pub fn set_percent(percent: f32, cx: &mut App) {
    let next = zoom_for_percent(percent);
    let previous = UiZoom::get(cx);
    if next == previous {
        return;
    }

    cx.set_global(UiZoom(next));
    log::info!(
        target: "zz::diagnostics::appearance",
        "ui zoom previous_percent={} percent={} zoom={}",
        percent_for_zoom(previous),
        percent_for_zoom(next),
        next,
    );
    cx.defer(|cx| {
        let zoom = UiZoom::get(cx);
        for window in cx.windows() {
            window.update(cx, |_, window, _| window.set_zoom(zoom)).ok();
        }
    });
}

/// Multiply the zoom by `factor`, the relative amount a pinch hands over.
#[cfg(target_os = "ios")]
pub fn scale_by(factor: f32, cx: &mut App) {
    set_percent(effective_percent(cx) * factor, cx);
}

/// Start a freshly opened window at the zoom already in effect.
pub fn apply_to_new_window(window: &mut Window, cx: &App) {
    window.set_zoom(UiZoom::get(cx));
}

/// The effective zoom, rounded for display.
pub fn percent(cx: &App) -> u32 {
    percent_for_zoom(UiZoom::get(cx)).round() as u32
}

pub fn is_default(cx: &App) -> bool {
    UiZoom::get(cx) == zoom_for_percent(DEFAULT_UI_ZOOM)
}

/// Whether `percent` resolves to the zoom already in effect, which is not the
/// same as matching [`percent`]: `150.7` displays as 151 but is exact.
pub fn is_effective_percent(percent: f32, cx: &App) -> bool {
    zoom_for_percent(percent) == UiZoom::get(cx)
}

pub(crate) fn key_bindings(chords: &[ChromeChord]) -> Vec<KeyBinding> {
    let context = Some(ROOT_KEY_CONTEXT);
    chords
        .iter()
        .filter_map(|chord| {
            Some(match chord.action() {
                ChromeAction::UiZoomIn => chord.binding(IncreaseUiZoom, context),
                ChromeAction::UiZoomOut => chord.binding(DecreaseUiZoom, context),
                ChromeAction::UiZoomReset => chord.binding(ResetUiZoom, context),
                _ => return None,
            })
        })
        .collect()
}

fn effective_percent(cx: &App) -> f32 {
    percent_for_zoom(UiZoom::get(cx))
}

fn zoom_for_percent(percent: f32) -> f32 {
    percent.clamp(MIN_UI_ZOOM, MAX_UI_ZOOM) / 100.0
}

fn percent_for_zoom(zoom: f32) -> f32 {
    zoom * 100.0
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use gpui::{AppContext as _, KeyContext, Keymap, Keystroke, TestAppContext};

    use super::*;

    #[test]
    fn zoom_is_a_percentage_of_the_unzoomed_window() {
        assert_eq!(zoom_for_percent(100.0), 1.0);
        assert_eq!(zoom_for_percent(150.0), 1.5);
        assert_eq!(zoom_for_percent(50.0), 0.5);
        for zoom in [1.0, 1.25, 0.5] {
            assert_eq!(zoom_for_percent(percent_for_zoom(zoom)), zoom);
        }
    }

    #[test]
    fn zoom_clamps_to_the_supported_range() {
        assert_eq!(
            zoom_for_percent(MIN_UI_ZOOM - UI_ZOOM_STEP),
            zoom_for_percent(MIN_UI_ZOOM)
        );
        assert_eq!(
            zoom_for_percent(MAX_UI_ZOOM + UI_ZOOM_STEP),
            zoom_for_percent(MAX_UI_ZOOM)
        );
    }

    #[test]
    fn shortcuts_are_scoped_to_the_ui_root() {
        let keymap = Keymap::new(key_bindings(&crate::keymap::test_chords(UI_TABLE)));
        let root = KeyContext::parse(ROOT_KEY_CONTEXT).expect("valid root context");
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let shortcuts = [
            ("cmd-=", TypeId::of::<IncreaseUiZoom>()),
            ("cmd-+", TypeId::of::<IncreaseUiZoom>()),
            ("cmd--", TypeId::of::<DecreaseUiZoom>()),
            ("cmd-0", TypeId::of::<ResetUiZoom>()),
        ];
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let shortcuts = [
            ("ctrl-=", TypeId::of::<IncreaseUiZoom>()),
            ("ctrl-+", TypeId::of::<IncreaseUiZoom>()),
            ("ctrl--", TypeId::of::<DecreaseUiZoom>()),
            ("ctrl-0", TypeId::of::<ResetUiZoom>()),
        ];

        for (source, action_type) in shortcuts {
            let keystroke = Keystroke::parse(source).expect("valid UI scale shortcut");
            let (bindings, pending) = keymap.bindings_for_input(
                std::slice::from_ref(&keystroke),
                std::slice::from_ref(&root),
            );
            assert!(!pending);
            assert_eq!(bindings.len(), 1, "expected one binding for {source}");
            assert_eq!(bindings[0].action().as_any().type_id(), action_type);

            let (bindings, pending) = keymap.bindings_for_input(&[keystroke], &[]);
            assert!(!pending);
            assert!(bindings.is_empty(), "{source} must stay root-scoped");
        }
    }

    #[gpui::test]
    fn actions_step_the_shared_zoom_in_whole_percents(cx: &mut TestAppContext) {
        cx.update(|cx| {
            zz_ui::init(cx);
            init(cx);

            assert_eq!(UiZoom::get(cx), 1.0);
            cx.dispatch_action(&IncreaseUiZoom);
            assert_eq!(percent(cx), 110);
            assert!(!is_default(cx));
            cx.dispatch_action(&DecreaseUiZoom);
            assert_eq!(percent(cx), 100);
            assert!(is_default(cx));

            set_percent(105.0, cx);
            cx.dispatch_action(&IncreaseUiZoom);
            assert_eq!(percent(cx), 115);
            assert!(is_effective_percent(115.0, cx));

            set_percent(MAX_UI_ZOOM * 2.0, cx);
            assert_eq!(UiZoom::get(cx), zoom_for_percent(MAX_UI_ZOOM));
            cx.dispatch_action(&ResetUiZoom);
            assert_eq!(UiZoom::get(cx), 1.0);
        });
    }

    #[gpui::test]
    fn zooming_moves_every_open_window(cx: &mut TestAppContext) {
        cx.update(|cx| {
            zz_ui::init(cx);
            init(cx);
        });

        let first = cx.add_window(|_, _| gpui::Empty);
        let second = cx.add_window(|_, _| gpui::Empty);
        let zoom_of = |cx: &mut TestAppContext, window: gpui::WindowHandle<gpui::Empty>| {
            cx.update_window(window.into(), |_, window, _| window.zoom())
                .expect("window is open")
        };

        cx.update(|cx| set_percent(150.0, cx));
        assert_eq!(zoom_of(cx, first), 1.5);
        assert_eq!(zoom_of(cx, second), 1.5);

        cx.update_window(first.into(), |_, window, cx| {
            window.dispatch_action(Box::new(IncreaseUiZoom), cx);
        })
        .expect("window is open");
        assert_eq!(zoom_of(cx, first), zoom_for_percent(160.0));
        assert_eq!(zoom_of(cx, second), zoom_for_percent(160.0));
        cx.update(|cx| set_percent(150.0, cx));

        let third = cx.add_window(|_, _| gpui::Empty);
        cx.update_window(third.into(), |_, window, cx| {
            apply_to_new_window(window, cx);
        })
        .expect("window is open");
        assert_eq!(zoom_of(cx, third), 1.5);

        cx.update(reset);
        assert_eq!(zoom_of(cx, first), 1.0);
    }
}
