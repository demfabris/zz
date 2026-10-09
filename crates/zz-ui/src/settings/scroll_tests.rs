use zpui::{
    AppContext as _, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, ScrollDelta, ScrollWheelEvent, Styled as _, TestAppContext, VisualTestContext, Window,
    div, point, px,
};

use crate::Sizable as _;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}

fn scroll(cx: &mut VisualTestContext, x: f32, y: f32, dx: f32, dy: f32) {
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(x), px(y)),
        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
        ..Default::default()
    });
    draw(cx);
}

fn row(selector: &'static str, height: f32) -> Div {
    div()
        .h(px(height))
        .flex_shrink_0()
        .debug_selector(move || selector.to_string())
}

struct WrappedTextTest;

impl Render for WrappedTextTest {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().w(px(300.)).h(px(200.)).child(
            div().flex().flex_1().min_h_0().overflow_hidden().child(
                super::settings_scroll_column("wrapped-text-column")
                    .child(
                        div()
                            .text_size(crate::rems_from_px(10.))
                            .child(zpui::SharedString::from("word ".repeat(60))),
                    )
                    .child(row("wrapped-text-tail", 20.)),
            ),
        )
    }
}

struct SettingsPageReplicaTest {
    pickers: Vec<zpui::Entity<crate::color_picker::ColorPickerState>>,
    numbers: Vec<zpui::Entity<crate::input::InputState>>,
}

impl Render for SettingsPageReplicaTest {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().w(px(760.)).h(px(300.)).child(
            div().flex().flex_1().min_h_0().overflow_hidden().child(
                super::settings_scroll_column("replica-column")
                    .child(
                        div()
                            .text_size(crate::rems_from_px(11.))
                            .child("Changes are written to zz/config."),
                    )
                    .child(
                        super::SettingsStack::titled("Theme")
                            .description(
                                "Recolors the application chrome. Every panel, hover state, \
                                 muted label and focus ring is derived from these six, so \
                                 nothing else needs setting.",
                            )
                            .children(self.pickers.iter().enumerate().map(|(ix, picker)| {
                                super::SettingEntry::new(
                                    "Background",
                                    "The chrome root everything else is derived from, \
                                     wrapping over a couple of lines like the real rows do.",
                                )
                                .title_actions(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(super::settings_reset_button(
                                            ("replica-reset", ix),
                                            "Reset",
                                            false,
                                        ))
                                        .child(super::settings_provenance_badge("default")),
                                )
                                .control(
                                    crate::color_picker::ColorPicker::new(picker, zpui::black()),
                                )
                            })),
                    )
                    .child(
                        super::SettingsStack::titled("Window")
                            .child(
                                super::SettingEntry::new(
                                    "Window background opacity",
                                    "Set below 1 to reveal the desktop or blurred backdrop \
                                     through terminal and app chrome.",
                                )
                                .control(
                                    div().w(px(120.)).flex_none().child(
                                        crate::input::NumberInput::new(&self.numbers[0]).small(),
                                    ),
                                ),
                            )
                            .child(
                                super::SettingEntry::new(
                                    "Window background blur",
                                    "Blur content behind translucent window areas when \
                                     supported by the desktop.",
                                )
                                .control(crate::switch::Switch::new("replica-blur").checked(true)),
                            ),
                    )
                    .child(
                        super::SettingsStack::titled("Interface").child(
                            super::SettingEntry::new(
                                "Widget corner radius",
                                "Rounds every widget (buttons, inputs, tags, menus, dialogs) \
                                 in logical pixels (0-256).",
                            )
                            .control(
                                div().w(px(120.)).flex_none().child(
                                    crate::input::NumberInput::new(&self.numbers[1]).small(),
                                ),
                            ),
                        ),
                    )
                    .child(row("replica-tail", 20.)),
            ),
        )
    }
}

#[zpui::test]
fn settings_page_scroll_range_matches_content(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|window, cx| SettingsPageReplicaTest {
        pickers: (0..6)
            .map(|_| cx.new(|cx| crate::color_picker::ColorPickerState::new(None, window, cx)))
            .collect(),
        numbers: (0..2)
            .map(|_| cx.new(|cx| crate::input::InputState::new(window, cx)))
            .collect(),
    });
    let cx: &mut VisualTestContext = cx;
    draw(cx);

    scroll(cx, 300., 150., 0., -100_000.);
    let tail = cx.debug_bounds("replica-tail").unwrap();
    assert!(
        tail.bottom() >= px(250.) && tail.bottom() <= px(300.),
        "after scrolling to the end, the last row must sit at the viewport bottom, \
         not pages above it (tail bottom: {:?})",
        tail.bottom()
    );
}

#[zpui::test]
fn scroll_range_measures_wrapped_text_at_layout_width(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|_, _| WrappedTextTest);
    let cx: &mut VisualTestContext = cx;
    draw(cx);

    let before = cx.debug_bounds("wrapped-text-tail").unwrap().origin.y;
    scroll(cx, 150., 100., 0., -10_000.);
    let after = cx.debug_bounds("wrapped-text-tail").unwrap().origin.y;
    assert_eq!(
        before, after,
        "content fits the viewport, so the wheel must not move it"
    );
}
