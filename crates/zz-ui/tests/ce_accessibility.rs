use gpui::{
    Context, IntoElement, Modifiers, Render, Role, TestAppContext, VisualTestContext,
    Window, div, prelude::*, px,
};
use zz_ui::slider::DiscreteSlider;

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

#[gpui::test]
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

#[gpui::test]
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
