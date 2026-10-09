use zpui::{
    AnyElement, AnyView, App, AppContext as _, Context, IntoElement, Keystroke, ParentElement as _,
    Render, Styled as _, Subscription, Window, div, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _, Size, h_flex,
    kbd::Kbd, list::ListItem, separator::Separator, spinner::Spinner, tag::Tag, v_flex,
};

use super::support::{Probe, hover, when_visible};
use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "display",
    name: "Display",
    group: "Kit",
    summary: "Small read-only pieces: tags, key caps, spinners, separators and list rows.",
    sections: &[
        Section {
            id: "tag",
            name: "Tag",
            summary: "Three color roles, filled or outlined. Small and xsmall share one compact form.",
            build: |_, cx| stateless(tags, cx),
        },
        Section {
            id: "kbd",
            name: "Kbd",
            summary: "One Keystroke as a pill. The web build spells modifiers out (Ctrl, Alt, Shift, Win); macOS draws glyphs.",
            build: |_, cx| stateless(kbds, cx),
        },
        Section {
            id: "spinner",
            name: "Spinner",
            summary: "The Loader glyph on a 800ms turn, at every size and in a few tints. It stops with motion off.",
            build: |_, cx| stateless(spinners, cx),
        },
        Section {
            id: "separator",
            name: "Separator",
            summary: "A one pixel rule that takes no room along its axis. A vertical rule needs a parent with a definite height.",
            build: |_, cx| stateless(separators, cx),
        },
        Section {
            id: "list-item",
            name: "List item",
            summary: "Resting, hovered, selected and disabled rows. The pointer rests on the hovered row.",
            build: |window, cx| ListRows::view(window, cx),
        },
    ],
};

fn tags(_: &mut Window, _: &mut App) -> AnyElement {
    let set = |outline: bool, size: Size| {
        row()
            .child(styled_tag(Tag::primary(), outline, size).child("primary"))
            .child(styled_tag(Tag::secondary(), outline, size).child("secondary"))
            .child(styled_tag(Tag::success(), outline, size).child("success"))
    };
    states()
        .columns(2)
        .state("filled", set(false, Size::Medium))
        .state("outline", set(true, Size::Medium))
        .state("filled, small", set(false, Size::Small))
        .state("outline, small", set(true, Size::Small))
        .state(
            "with an icon",
            row()
                .child(
                    Tag::secondary().child(
                        h_flex()
                            .gap_1()
                            .child(Icon::new(IconName::GitBranch).xsmall())
                            .child("main"),
                    ),
                )
                .child(
                    Tag::success().outline().child(
                        h_flex()
                            .gap_1()
                            .child(Icon::new(IconName::Check).xsmall())
                            .child("passing"),
                    ),
                ),
        )
        .into_any_element()
}

fn styled_tag(tag: Tag, outline: bool, size: Size) -> Tag {
    let tag = if outline { tag.outline() } else { tag };
    tag.with_size(size)
}

fn key(source: &str) -> Kbd {
    Kbd::new(Keystroke::parse(source).unwrap_or_default())
}

fn kbds(_: &mut Window, _: &mut App) -> AnyElement {
    states()
        .columns(2)
        .state(
            "single keys",
            row()
                .child(key("a"))
                .child(key("escape"))
                .child(key("enter"))
                .child(key("tab"))
                .child(key("space"))
                .child(key("backspace")),
        )
        .state(
            "arrows and function keys",
            row()
                .child(key("up"))
                .child(key("down"))
                .child(key("left"))
                .child(key("right"))
                .child(key("f5"))
                .child(key("pageup")),
        )
        .state(
            "chords",
            row()
                .child(key("cmd-k"))
                .child(key("ctrl-b"))
                .child(key("cmd-shift-p"))
                .child(key("ctrl-alt-shift-cmd-z")),
        )
        .state(
            "lowercase",
            row()
                .child(key("cmd-shift-p").lowercase())
                .child(key("ctrl-a").lowercase()),
        )
        .into_any_element()
}

fn spinners(_: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    states()
        .columns(2)
        .state(
            "sizes",
            row()
                .child(Spinner::new().xsmall())
                .child(Spinner::new().small())
                .child(Spinner::new())
                .child(Spinner::new().large())
                .child(Spinner::new().with_size(px(32.0))),
        )
        .state(
            "colors",
            row()
                .child(Spinner::new().color(theme.foreground.muted()))
                .child(Spinner::new().color(theme.accent))
                .child(Spinner::new().color(theme.success))
                .child(Spinner::new().color(theme.warning))
                .child(Spinner::new().color(theme.danger)),
        )
        .state(
            "inline with text",
            h_flex()
                .gap_2()
                .text_sm()
                .child(Spinner::new().small().color(theme.foreground.muted()))
                .child("Connecting to devbox"),
        )
        .into_any_element()
}

fn separators(_: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.foreground.muted();
    states()
        .columns(3)
        .state(
            "horizontal",
            v_flex()
                .gap_2()
                .text_sm()
                .child("Sessions")
                .child(Separator::horizontal())
                .child(div().text_color(muted).child("work, scratch, logs")),
        )
        .state(
            "vertical, 20px parent",
            h_flex()
                .h(px(20.0))
                .gap_3()
                .text_sm()
                .child("zz")
                .child(Separator::vertical())
                .child("work")
                .child(Separator::vertical())
                .child(div().text_color(muted).child("3 panes")),
        )
        .state(
            "custom color",
            v_flex()
                .gap_2()
                .text_sm()
                .child("Danger zone")
                .child(Separator::horizontal().color(theme.danger))
                .child(div().text_color(muted).child("Kill every session")),
        )
        .into_any_element()
}

struct ListRows {
    hovered: Probe,
    _bounds: Subscription,
}

impl ListRows {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let hovered = Probe::default();
        let probe = hovered.clone();
        when_visible(&hovered, window, move |window, cx| {
            hover(&probe, window, cx);
        });
        cx.new(|cx| Self {
            hovered,
            _bounds: cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                hover(&this.hovered, window, cx);
            }),
        })
        .into()
    }
}

fn list_row(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    detail: &'static str,
    cx: &App,
) -> ListItem {
    let muted = cx.theme().foreground.muted();
    ListItem::new(id).child(
        h_flex()
            .w_full()
            .gap_2()
            .text_sm()
            .child(Icon::new(icon).small().text_color(muted))
            .child(div().flex_1().child(label))
            .child(div().text_xs().text_color(muted).child(detail)),
    )
}

impl Render for ListRows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = v_flex()
            .w(px(360.0))
            .gap_0p5()
            .child(list_row(
                "row-rest",
                IconName::SquareTerminal,
                "work",
                "3 panes",
                cx,
            ))
            .child(
                div()
                    .relative()
                    .child(list_row(
                        "row-hover",
                        IconName::SquareTerminal,
                        "scratch",
                        "1 pane",
                        cx,
                    ))
                    .child(self.hovered.measure()),
            )
            .child(
                list_row(
                    "row-selected",
                    IconName::SquareTerminal,
                    "logs",
                    "2 panes",
                    cx,
                )
                .selected(true),
            )
            .child(
                list_row(
                    "row-disabled",
                    IconName::SquareTerminal,
                    "detached",
                    "gone",
                    cx,
                )
                .disabled(true),
            )
            .child(
                list_row(
                    "row-disabled-selected",
                    IconName::SquareTerminal,
                    "stale",
                    "gone",
                    cx,
                )
                .selected(true)
                .disabled(true),
            );
        states().state(
            "resting, hovered, selected, disabled, disabled and selected",
            list,
        )
    }
}
