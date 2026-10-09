use std::{cell::Cell, rc::Rc};

use zpui::{
    App, Bounds, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, Styled as _, Window, canvas,
};

#[derive(Clone, Default)]
pub struct Probe(Rc<Cell<Option<Bounds<Pixels>>>>);

impl Probe {
    pub fn measure(&self) -> impl IntoElement {
        let cell = self.0.clone();
        canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, (), _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
    }

    fn center(&self, window: &Window) -> Option<Point<Pixels>> {
        self.0.get().map(|bounds| bounds.center() * window.zoom())
    }
}

pub fn after_first_frame(window: &Window, f: impl FnOnce(&mut Window, &mut App) + 'static) {
    window.on_next_frame(move |window, _| window.on_next_frame(f));
}

pub fn when_visible(
    probe: &Probe,
    window: &Window,
    f: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    retry(probe.clone(), None, 0, 240, Box::new(f), window);
}

fn retry(
    probe: Probe,
    last: Option<Bounds<Pixels>>,
    steady: u32,
    tries: u32,
    f: Box<dyn FnOnce(&mut Window, &mut App)>,
    window: &Window,
) {
    window.on_next_frame(move |window, cx| {
        let current = probe.0.get();
        let visible =
            current.is_some_and(|bounds| bounds.bottom() <= window.viewport_size().height);
        let steady = if visible && current == last {
            steady + 1
        } else {
            0
        };
        if steady >= 3 || tries == 0 {
            f(window, cx);
        } else {
            retry(probe, current, steady, tries - 1, f, window);
        }
    });
}

pub fn hover(probe: &Probe, window: &mut Window, cx: &mut App) {
    if let Some(position) = probe.center(window) {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
    }
}

pub fn nudge(frames: u32, window: &Window) {
    if frames > 0 {
        window.on_next_frame(move |window, _| {
            window.refresh();
            nudge(frames - 1, window);
        });
    }
}

pub fn press(probe: &Probe, button: MouseButton, window: &mut Window, cx: &mut App) {
    hover(probe, window, cx);
    if let Some(position) = probe.center(window) {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    }
    nudge(8, window);
}

pub fn click(probe: &Probe, window: &mut Window, cx: &mut App) {
    press(probe, MouseButton::Left, window, cx);
    if let Some(position) = probe.center(window) {
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            cx,
        );
    }
}

pub fn dispatch(name: &str, window: &mut Window, cx: &mut App) {
    if let Ok(action) = cx.build_action(name, None) {
        window.dispatch_action(action, cx);
    }
}
