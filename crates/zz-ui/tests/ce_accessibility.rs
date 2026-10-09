use std::{cell::Cell, rc::Rc};
use zpui::{
    Context, Entity, IntoElement, Modifiers, Render, Role, TestAppContext, VisualTestContext,
    Window, div, prelude::*, px,
};
use zz_ui::{
    Disableable,
    button::Button,
    input::{Input, InputState, NumberInput},
    slider::DiscreteSlider,
};

struct EffortFixture {
    selected: usize,
}

impl Render for EffortFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        div().w(px(400.0)).child(DiscreteSlider::new(
            "ce-effort",
            "Reasoning effort",
            vec!["low".into(), "high".into()],
            self.selected,
            move |selected, _, cx| {
                entity.update(cx, |this, cx| {
                    this.selected = selected;
                    cx.notify();
                });
            },
        ))
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
}

struct DisabledFixture {
    disabled: bool,
    input: Entity<InputState>,
    number: Entity<InputState>,
    clicks: Rc<Cell<usize>>,
}

impl Render for DisabledFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .w(px(400.0))
            .flex()
            .flex_col()
            .child(
                Button::new("ce-disabled")
                    .label("Unavailable")
                    .disabled(self.disabled)
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
            )
            .child(Button::new("ce-enabled").label("Available"))
            .child(Input::new(&self.input).disabled(self.disabled))
            .child(NumberInput::new(&self.number).disabled(self.disabled))
    }
}

#[zpui::test]
fn disabled_zz_controls_expose_and_clear_accessibility_state(cx: &mut TestAppContext) {
    cx.update(zz_ui::init);
    let clicks = Rc::new(Cell::new(0));
    let (fixture, cx) = cx.add_window_view({
        let clicks = clicks.clone();
        move |window, cx| {
            window.set_a11y_forced(true);
            DisabledFixture {
                disabled: true,
                input: cx.new(|cx| InputState::new(window, cx)),
                number: cx.new(|cx| InputState::new(window, cx)),
                clicks,
            }
        }
    });
    draw(cx);
    let button_bounds = cx.update(|window, _| {
        let (id, _) = window
            .a11y_tree()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Unavailable"))
            .unwrap();
        window.a11y_node_bounds(*id).unwrap()
    });
    cx.simulate_click(button_bounds.center(), Modifiers::default());
    assert_eq!(clicks.get(), 0);
    cx.update(|window, _| {
        let tree = window.a11y_tree().unwrap();
        let states = [Role::Button, Role::TextInput, Role::SpinButton].map(|role| {
            let (_, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == role
                        && (role != Role::Button || node.label() == Some("Unavailable"))
                })
                .unwrap();
            (role, node.is_disabled())
        });
        assert!(
            states.iter().all(|(_, disabled)| *disabled),
            "disabled controls lack AccessKit disabled state: {states:?}"
        );
        let (_, enabled) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Available"))
            .unwrap();
        assert!(!enabled.is_disabled());
    });
    fixture.update(cx, |this, cx| {
        this.disabled = false;
        cx.notify();
    });
    draw(cx);
    cx.update(|window, _| {
        for (_, node) in &window.a11y_tree().unwrap().nodes {
            if matches!(
                node.role(),
                Role::Button | Role::TextInput | Role::SpinButton
            ) {
                assert!(!node.is_disabled());
            }
        }
    });
    cx.simulate_click(button_bounds.center(), Modifiers::default());
    assert_eq!(clicks.get(), 1);
}

#[zpui::test]
fn forced_accessibility_inspects_existing_zz_widgets_without_a_screen_reader(
    cx: &mut TestAppContext,
) {
    cx.update(zz_ui::init);
    let (_, cx) = cx.add_window_view(|_, _| EffortFixture { selected: 0 });
    draw(cx);
    cx.update(|window, _| {
        assert!(!window.is_a11y_active());
        assert!(window.debug_a11y_tree_json().is_none());
        window.set_a11y_forced(true);
    });
    draw(cx);
    cx.update(|window, _| {
        assert!(window.is_a11y_active());
        let tree: serde_json::Value =
            serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
        assert!(tree["nodes"].as_object().unwrap().values().any(|node| {
            node["aria"]["role"] == "Slider"
                && node["aria"]["label"] == "Reasoning effort"
                && node["aria"]["value"] == "low"
        }));
        window.set_a11y_forced(false);
    });
    draw(cx);
    cx.update(|window, _| assert!(!window.is_a11y_active()));
}

#[zpui::test]
fn accessibility_snapshot_drives_reasoning_effort_and_tracks_fresh_geometry(
    cx: &mut TestAppContext,
) {
    cx.update(zz_ui::init);
    let (_, cx) = cx.add_window_view(|window, _| {
        window.set_a11y_forced(true);
        EffortFixture { selected: 0 }
    });
    draw(cx);
    let (slider_id, frame, high_bounds) = cx.update(|window, _| {
        let tree = window.a11y_tree().unwrap();
        let (slider_id, slider) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Slider)
            .unwrap();
        assert_eq!(slider.label(), Some("Reasoning effort"));
        assert_eq!(slider.value(), Some("low"));
        let (high_id, _) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Button && node.label() == Some("high"))
            .unwrap();
        let high_bounds = window.a11y_node_bounds(*high_id).unwrap();
        assert!(high_bounds.size.width > px(0.0));
        assert!(high_bounds.size.height > px(0.0));
        (*slider_id, window.a11y_frame_number(), high_bounds)
    });
    cx.simulate_click(high_bounds.center(), Modifiers::default());
    draw(cx);
    cx.update(|window, _| {
        let tree = window.a11y_tree().unwrap();
        let (_, slider) = tree.nodes.iter().find(|(id, _)| *id == slider_id).unwrap();
        assert_eq!(slider.value(), Some("high"));
        assert!(window.a11y_frame_number() > frame);
        assert!(window.a11y_node_bounds(slider_id).is_some());
    });
}
