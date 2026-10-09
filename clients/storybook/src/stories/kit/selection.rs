use zpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Window, div, px,
};
use zz_ui::{
    Disableable as _, IndexPath, Sizable as _, h_flex,
    select::{Select, SelectState},
    slider::DiscreteSlider,
    switch::Switch,
};

use super::support::{Probe, after_first_frame, click, dispatch, when_visible};
use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "choices",
    name: "Choices",
    group: "Kit",
    summary: "Controls that pick a value: the dropdown Select, the Switch and the DiscreteSlider.",
    sections: &[
        Section {
            id: "select",
            name: "Select",
            summary: "The trigger is a Button with a dropdown caret. One SelectState per Select, since the element writes its options into the state.",
            build: |window, cx| cx.new(|cx| Selects::new(window, cx)).into(),
        },
        Section {
            id: "select-open",
            name: "Select, open",
            summary: "Focus plus the Confirm action opens the menu: a scrollable PopupMenu as wide as the trigger, with the picked row checked.",
            build: |window, cx| OpenSelect::view(window, cx),
        },
        Section {
            id: "switch",
            name: "Switch",
            summary: "Off, on, compact and disabled. The thumb slides between states; it holds still with motion off.",
            build: |_, cx| stateless(switches, cx),
        },
        Section {
            id: "slider",
            name: "Discrete slider",
            summary: "A row of pills up to the picked step, with the label on the left and the value on the right.",
            build: |_, cx| stateless(sliders, cx),
        },
        Section {
            id: "slider-focused",
            name: "Discrete slider, focused",
            summary: "A press on the slider focuses it; the picked pill then carries a foreground border and the arrow keys step it.",
            build: |window, cx| FocusedSlider::view(window, cx),
        },
    ],
};

const LANGUAGES: [&str; 5] = ["Rust", "Go", "Zig", "Swift", "OCaml"];

fn select(
    items: Vec<&'static str>,
    picked: Option<usize>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<SelectState<Vec<&'static str>>> {
    cx.new(|cx| SelectState::new(items, picked.map(IndexPath::new), window, cx))
}

struct Selects {
    placeholder: Entity<SelectState<Vec<&'static str>>>,
    picked: Entity<SelectState<Vec<&'static str>>>,
    disabled: Entity<SelectState<Vec<&'static str>>>,
    small: Entity<SelectState<Vec<&'static str>>>,
    wide: Entity<SelectState<Vec<&'static str>>>,
    empty: Entity<SelectState<Vec<&'static str>>>,
}

impl Selects {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            placeholder: select(LANGUAGES.to_vec(), None, window, cx),
            picked: select(LANGUAGES.to_vec(), Some(0), window, cx),
            disabled: select(LANGUAGES.to_vec(), Some(2), window, cx),
            small: select(LANGUAGES.to_vec(), Some(1), window, cx),
            wide: select(LANGUAGES.to_vec(), Some(4), window, cx),
            empty: select(Vec::new(), None, window, cx),
        }
    }
}

impl Render for Selects {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        states()
            .columns(3)
            .state(
                "placeholder",
                Select::new(&self.placeholder).placeholder("Language"),
            )
            .state("picked", Select::new(&self.picked))
            .state("disabled", Select::new(&self.disabled).disabled(true))
            .state("small", Select::new(&self.small).small())
            .state("fixed width", Select::new(&self.wide).w(px(200.0)))
            .state(
                "empty delegate",
                Select::new(&self.empty).placeholder("No languages"),
            )
    }
}

struct OpenSelect {
    state: Entity<SelectState<Vec<&'static str>>>,
}

impl OpenSelect {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let state = select(LANGUAGES.to_vec(), Some(0), window, cx);
        let target = state.clone();
        after_first_frame(window, move |window, cx| {
            let handle = zpui::Focusable::focus_handle(target.read(cx), cx);
            handle.focus(window, cx);
            dispatch("zz_select::Confirm", window, cx);
        });
        cx.new(|_| Self { state }).into()
    }
}

impl Render for OpenSelect {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().h(px(300.0)).child(states().state(
            "open",
            h_flex().child(Select::new(&self.state).w(px(200.0))),
        ))
    }
}

fn switches(_: &mut Window, _: &mut App) -> AnyElement {
    states()
        .columns(3)
        .state(
            "off, on",
            row()
                .child(Switch::new("switch-off"))
                .child(Switch::new("switch-on").checked(true)),
        )
        .state(
            "small",
            row()
                .child(Switch::new("switch-small-off").small())
                .child(Switch::new("switch-small-on").small().checked(true)),
        )
        .state(
            "disabled",
            row()
                .child(Switch::new("switch-disabled-off").disabled(true))
                .child(
                    Switch::new("switch-disabled-on")
                        .checked(true)
                        .disabled(true),
                ),
        )
        .into_any_element()
}

fn labels(names: &[&'static str]) -> Vec<SharedString> {
    names.iter().copied().map(SharedString::from).collect()
}

const SCROLLBACK: [&str; 5] = ["1k", "5k", "10k", "50k", "100k"];
const OPACITY: [&str; 11] = [
    "0%", "10%", "20%", "30%", "40%", "50%", "60%", "70%", "80%", "90%", "100%",
];

fn sliders(_: &mut Window, _: &mut App) -> AnyElement {
    states()
        .columns(2)
        .state(
            "first step",
            DiscreteSlider::new(
                "slider-first",
                "Scrollback",
                labels(&SCROLLBACK),
                0,
                |_, _, _| {},
            ),
        )
        .state(
            "middle step",
            DiscreteSlider::new(
                "slider-middle",
                "Scrollback",
                labels(&SCROLLBACK),
                2,
                |_, _, _| {},
            ),
        )
        .state(
            "last step",
            DiscreteSlider::new(
                "slider-last",
                "Scrollback",
                labels(&SCROLLBACK),
                4,
                |_, _, _| {},
            ),
        )
        .state(
            "eleven steps",
            DiscreteSlider::new("slider-many", "Opacity", labels(&OPACITY), 7, |_, _, _| {}),
        )
        .into_any_element()
}

struct FocusedSlider {
    label: Probe,
    _bounds: Subscription,
}

impl FocusedSlider {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let label = Probe::default();
        let probe = label.clone();
        when_visible(&label, window, move |window, cx| click(&probe, window, cx));
        cx.new(|cx| Self {
            label,
            _bounds: cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                click(&this.label, window, cx);
            }),
        })
        .into()
    }
}

impl Render for FocusedSlider {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        states().state(
            "focused",
            div()
                .relative()
                .w(px(360.0))
                .child(DiscreteSlider::new(
                    "slider-focused",
                    "Scrollback",
                    labels(&SCROLLBACK),
                    1,
                    |_, _, _| {},
                ))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(24.0))
                        .h_full()
                        .child(self.label.measure()),
                ),
        )
    }
}
