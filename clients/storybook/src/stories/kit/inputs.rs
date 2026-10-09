use zpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, Keystroke, ParentElement as _,
    Render, Styled as _, Window, div,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _, Size,
    input::{Input, InputContentType, InputState, NumberInput, TextAlign},
    kbd::Kbd,
};

use super::support::after_first_frame;
use crate::story::{Section, Story, states};

pub const STORY: Story = Story {
    id: "inputs",
    name: "Inputs",
    group: "Kit",
    summary: "Text fields bound to an InputState: single line, auto-growing multi-line and the stepped number field. There is no error state; validation only rejects edits.",
    sections: &[
        Section {
            id: "field-states",
            name: "Field states",
            summary: "One InputState per field. The selection paints without focus; the spinner replaces the suffix while loading.",
            build: |window, cx| cx.new(|cx| FieldStates::new(window, cx)).into(),
        },
        Section {
            id: "field-sizes",
            name: "Field sizes",
            summary: "Sizable on Input: xsmall, small, medium and large set the height, padding and text size together.",
            build: |window, cx| cx.new(|cx| FieldSizes::new(window, cx)).into(),
        },
        Section {
            id: "field-focused",
            name: "Focused field",
            summary: "Focus swaps the border to the accent. The caret only paints while the window is active, so a background tab shows the ring alone.",
            build: |window, cx| Focused::view(window, cx),
        },
        Section {
            id: "multi-line",
            name: "Multi-line",
            summary: "auto_grow(min, max) makes the field multi-line. It grows with its rows, then scrolls past the maximum.",
            build: |window, cx| cx.new(|cx| MultiLine::new(window, cx)).into(),
        },
        Section {
            id: "number",
            name: "Number input",
            summary: "NumberInput wraps a field between two steppers. Step, min and max live on the InputState.",
            build: |window, cx| cx.new(|cx| Numbers::new(window, cx)).into(),
        },
    ],
};

fn field(
    window: &mut Window,
    cx: &mut App,
    build: impl FnOnce(InputState) -> InputState,
) -> Entity<InputState> {
    cx.new(|cx| build(InputState::new(window, cx)))
}

struct FieldStates {
    empty: Entity<InputState>,
    populated: Entity<InputState>,
    disabled_empty: Entity<InputState>,
    disabled_populated: Entity<InputState>,
    selection: Entity<InputState>,
    loading: Entity<InputState>,
    cleanable: Entity<InputState>,
    password: Entity<InputState>,
    affixes: Entity<InputState>,
    bare: Entity<InputState>,
    borderless: Entity<InputState>,
    centered: Entity<InputState>,
    right: Entity<InputState>,
}

impl FieldStates {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let selection = field(window, cx, |s| s.default_value("deploy --target staging"));
        selection.update(cx, |state, cx| state.set_selected_range(9..23, cx));
        let loading = field(window, cx, |s| s.default_value("Resolving host"));
        loading.update(cx, |state, cx| state.set_loading(true, window, cx));
        Self {
            empty: field(window, cx, |s| s.placeholder("Search sessions")),
            populated: field(window, cx, |s| s.default_value("zz attach -t work")),
            disabled_empty: field(window, cx, |s| s.placeholder("Read only")),
            disabled_populated: field(window, cx, |s| s.default_value("~/.config/zz/config")),
            selection,
            loading,
            cleanable: field(window, cx, |s| s.default_value("build logs")),
            password: field(window, cx, |s| s.default_value("correct horse battery")),
            affixes: field(window, cx, |s| s.placeholder("Jump to a pane")),
            bare: field(window, cx, |s| s.default_value("Untitled session")),
            borderless: field(window, cx, |s| s.placeholder("No resting border")),
            centered: field(window, cx, |s| s.default_value("centered")),
            right: field(window, cx, |s| s.default_value("1280")),
        }
    }
}

impl Render for FieldStates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().foreground.muted();
        states()
            .columns(2)
            .state("empty, placeholder", Input::new(&self.empty))
            .state("populated", Input::new(&self.populated))
            .state(
                "disabled, empty",
                Input::new(&self.disabled_empty).disabled(true),
            )
            .state(
                "disabled, populated",
                Input::new(&self.disabled_populated).disabled(true),
            )
            .state("selection, unfocused", Input::new(&self.selection))
            .state("loading", Input::new(&self.loading))
            .state("cleanable", Input::new(&self.cleanable).cleanable(true))
            .state(
                "password",
                Input::new(&self.password).content_type(InputContentType::Password),
            )
            .state(
                "prefix and suffix",
                Input::new(&self.affixes)
                    .prefix(Icon::new(IconName::Search).small().text_color(muted))
                    .suffix(Kbd::new(Keystroke::parse("cmd-k").unwrap_or_default())),
            )
            .state(
                "appearance(false)",
                div()
                    .px_2()
                    .border_b_1()
                    .border_color(cx.theme().border())
                    .child(Input::new(&self.bare).appearance(false)),
            )
            .state(
                "bordered(false)",
                Input::new(&self.borderless).bordered(false),
            )
            .state(
                "text_align center",
                Input::new(&self.centered).text_align(TextAlign::Center),
            )
            .state(
                "text_align right",
                Input::new(&self.right).text_align(TextAlign::Right),
            )
    }
}

struct FieldSizes {
    fields: Vec<(&'static str, Size, Entity<InputState>)>,
}

impl FieldSizes {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sizes = [
            ("xsmall", Size::XSmall),
            ("small", Size::Small),
            ("medium", Size::Medium),
            ("large", Size::Large),
        ];
        Self {
            fields: sizes
                .into_iter()
                .map(|(name, size)| {
                    (
                        name,
                        size,
                        field(window, cx, |s| s.default_value("split-window -h")),
                    )
                })
                .collect(),
        }
    }
}

impl Render for FieldSizes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.fields
            .iter()
            .fold(states().columns(2), |states, (name, size, state)| {
                states.state(*name, Input::new(state).with_size(*size))
            })
    }
}

struct Focused {
    focused: Entity<InputState>,
    selected: Entity<InputState>,
}

impl Focused {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let focused = field(window, cx, |s| s.default_value("rename-window build"));
        let selected = field(window, cx, |s| s.default_value("kill-session -t scratch"));
        let target = focused.clone();
        after_first_frame(window, move |window, cx| {
            target.update(cx, |state, cx| state.focus(window, cx));
        });
        cx.new(|_| Self { focused, selected }).into()
    }
}

impl Render for Focused {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        states()
            .columns(2)
            .state("focused", Input::new(&self.focused))
            .state("resting", Input::new(&self.selected))
    }
}

const NOTE: &str = "Panes split along the long edge.\nNew windows open in the current directory.\nThe status bar shows the session name.";
const LONG_NOTE: &str = "line one\nline two\nline three\nline four\nline five\nline six\nline seven\nline eight\nline nine";

struct MultiLine {
    empty: Entity<InputState>,
    filled: Entity<InputState>,
    overflowing: Entity<InputState>,
    disabled: Entity<InputState>,
}

impl MultiLine {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            empty: field(window, cx, |s| {
                s.auto_grow(2, 6).placeholder("Write a commit message")
            }),
            filled: field(window, cx, |s| s.auto_grow(2, 6).default_value(NOTE)),
            overflowing: field(window, cx, |s| s.auto_grow(2, 5).default_value(LONG_NOTE)),
            disabled: field(window, cx, |s| s.auto_grow(2, 6).default_value(NOTE)),
        }
    }
}

impl Render for MultiLine {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        states()
            .columns(2)
            .state("empty, two rows", Input::new(&self.empty))
            .state("three lines", Input::new(&self.filled))
            .state("past max_rows, scrolls", Input::new(&self.overflowing))
            .state("disabled", Input::new(&self.disabled).disabled(true))
    }
}

struct Numbers {
    default: Entity<InputState>,
    bounded: Entity<InputState>,
    decimal: Entity<InputState>,
    disabled: Entity<InputState>,
    small: Entity<InputState>,
    large: Entity<InputState>,
    bare: Entity<InputState>,
}

impl Numbers {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            default: field(window, cx, |s| s.step(1.0).default_value("4")),
            bounded: field(window, cx, |s| {
                s.step(1.0).min(0.0).max(10.0).default_value("0")
            }),
            decimal: field(window, cx, |s| s.step(0.25).default_value("1.25")),
            disabled: field(window, cx, |s| s.step(1.0).default_value("12")),
            small: field(window, cx, |s| s.step(1.0).default_value("8")),
            large: field(window, cx, |s| s.step(1.0).default_value("8")),
            bare: field(window, cx, |s| s.step(1.0).default_value("2")),
        }
    }
}

impl Render for Numbers {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let narrow = |element: NumberInput| div().w(zpui::px(180.0)).child(element);
        states()
            .columns(3)
            .state("step 1", narrow(NumberInput::new(&self.default)))
            .state("at min of 0..10", narrow(NumberInput::new(&self.bounded)))
            .state("step 0.25", narrow(NumberInput::new(&self.decimal)))
            .state(
                "disabled",
                narrow(NumberInput::new(&self.disabled).disabled(true)),
            )
            .state("small", narrow(NumberInput::new(&self.small).small()))
            .state("large", narrow(NumberInput::new(&self.large).large()))
            .state(
                "appearance(false)",
                narrow(NumberInput::new(&self.bare).appearance(false)),
            )
    }
}
