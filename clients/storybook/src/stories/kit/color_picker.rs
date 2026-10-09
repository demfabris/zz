use zpui::{
    AnyView, App, AppContext as _, Context, Entity, Hsla, IntoElement, MouseButton,
    ParentElement as _, Render, Styled as _, Subscription, Window, div, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Sizable as _, Size,
    color_picker::{ColorPicker, ColorPickerState},
    h_flex, parse_hex,
};

use super::support::{Probe, hover, press, when_visible};
use crate::story::{Section, Story, states};

pub const STORY: Story = Story {
    id: "color-picker",
    name: "Color picker",
    group: "Kit",
    summary: "A swatch button for one color override. It opens a hex field and a grid of preset swatches; clearing it falls back to the inherited color.",
    sections: &[
        Section {
            id: "states",
            name: "States",
            summary: "An override, an inherited color (None state), disabled, and the three sizes, each in a settings row.",
            build: |window, cx| Pickers::view(window, cx),
        },
        Section {
            id: "open",
            name: "Open",
            summary: "A press on the swatch opens the popover. It can only open from the trigger; there is no API to open it from code.",
            build: |window, cx| Open::view(window, cx),
        },
    ],
};

fn hex(value: &str, cx: &App) -> Hsla {
    parse_hex(value).unwrap_or(cx.theme().accent)
}

fn setting(label: &'static str, picker: impl IntoElement, cx: &App) -> impl IntoElement {
    h_flex()
        .w(px(320.0))
        .justify_between()
        .text_sm()
        .child(div().text_color(cx.theme().foreground.muted()).child(label))
        .child(picker)
}

struct Pickers {
    set: Entity<ColorPickerState>,
    inherited: Entity<ColorPickerState>,
    disabled: Entity<ColorPickerState>,
    sizes: Vec<(Size, Entity<ColorPickerState>)>,
}

impl Pickers {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let make = |value: Option<&str>, window: &mut Window, cx: &mut App| {
            let color = value.map(|value| hex(value, cx));
            cx.new(|cx| ColorPickerState::new(color, window, cx))
        };
        let set = make(Some("#7aa2f7"), window, cx);
        let inherited = make(None, window, cx);
        let disabled = make(Some("#9ece6a"), window, cx);
        let sizes = [Size::XSmall, Size::Small, Size::Medium]
            .into_iter()
            .map(|size| (size, make(Some("#e0af68"), window, cx)))
            .collect();
        cx.new(|_| Self {
            set,
            inherited,
            disabled,
            sizes,
        })
        .into()
    }
}

impl Render for Pickers {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inherited = hex("#bb9af7", cx);
        let sized = self
            .sizes
            .iter()
            .map(|(size, state)| ColorPicker::new(state, inherited).with_size(*size));
        states()
            .columns(2)
            .state(
                "override",
                setting(
                    "Accent",
                    ColorPicker::new(&self.set, inherited).label("Accent"),
                    cx,
                ),
            )
            .state(
                "inherited",
                setting(
                    "Selection",
                    ColorPicker::new(&self.inherited, inherited).label("Selection"),
                    cx,
                ),
            )
            .state(
                "disabled",
                setting(
                    "Cursor",
                    ColorPicker::new(&self.disabled, inherited).disabled(true),
                    cx,
                ),
            )
            .state("xsmall, small, medium", h_flex().gap_3().children(sized))
    }
}

struct Open {
    state: Entity<ColorPickerState>,
    trigger: Probe,
    _bounds: Subscription,
}

impl Open {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let color = hex("#7dcfff", cx);
        let state = cx.new(|cx| ColorPickerState::new(Some(color), window, cx));
        let trigger = Probe::default();
        let probe = trigger.clone();
        when_visible(&trigger, window, move |window, cx| {
            press(&probe, MouseButton::Left, window, cx);
        });
        cx.new(|cx| Self {
            state,
            trigger,
            _bounds: cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                hover(&this.trigger, window, cx);
            }),
        })
        .into()
    }
}

impl Render for Open {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inherited = hex("#bb9af7", cx);
        div().h(px(300.0)).child(
            h_flex().justify_end().w(px(320.0)).child(
                div()
                    .relative()
                    .child(ColorPicker::new(&self.state, inherited).label("Pane border"))
                    .child(self.trigger.measure()),
            ),
        )
    }
}
