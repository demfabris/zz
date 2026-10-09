use std::sync::Arc;

use zpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Window, div, prelude::*, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName,
    compact::{
        ArrowPad, KeyRow, PageDot, PopoverKey, PopoverKeyItem, PopoverKeyTap, ToolKeys,
        WhichKeyList, compact_bar, compact_bar_button, compact_bar_pill, compact_hud,
        compact_pane_header, page_dots,
    },
    pane::pane_header_icon_button,
    v_flex,
    which_key::WhichKeyRow,
};

use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "phone",
    name: "Phone",
    group: "Phone",
    summary: "The compact pieces of the phone shell, at phone width: the bottom bar and its pill, page dots, the pane header, the HUD, the key row and the bindings list.",
    sections: &[
        Section {
            id: "bar",
            name: "Bottom bar",
            summary: "compact_bar with its pill and keyboard button. The grabber brightens while the bar is held, and the pill turns amber when the page needs attention.",
            build: |_, cx| stateless(bars, cx),
        },
        Section {
            id: "page-dots",
            name: "Page dots",
            summary: "One group per window, one dot per pane. The current pane is the wide dot; a pane asking for attention is amber.",
            build: |_, cx| stateless(dots, cx),
        },
        Section {
            id: "pane-header",
            name: "Pane header",
            summary: "The header over a terminal page, with and without actions.",
            build: |_, cx| stateless(headers, cx),
        },
        Section {
            id: "hud",
            name: "HUD",
            summary: "The transient message centred over the page.",
            build: |_, cx| stateless(huds, cx),
        },
        Section {
            id: "key-row",
            name: "Key row",
            summary: "The row above the soft keyboard: prefix, ctrl and alt popover keys, plain keys, the arrow pad and hide. It scrolls sideways when it does not fit.",
            build: |_, cx| cx.new(KeyRows::new).into(),
        },
        Section {
            id: "keys",
            name: "Popover keys",
            summary: "A popover key at rest and armed, as when the prefix is waiting for its next key, and the arrow pad at rest.",
            build: |_, cx| cx.new(Keys::new).into(),
        },
        Section {
            id: "bindings",
            name: "Bindings list",
            summary: "WhichKeyList: the prefix table as a touch list, grouped, your own keys in the accent color.",
            build: |_, cx| stateless(bindings, cx),
        },
        Section {
            id: "screen",
            name: "Phone screen",
            summary: "The pieces together: a terminal page under its header, the key row and the bar.",
            build: |_, cx| cx.new(Screen::new).into(),
        },
    ],
};

const PHONE_WIDTH: f32 = 390.0;

fn phone(content: impl IntoElement, cx: &App) -> impl IntoElement {
    div()
        .w(px(PHONE_WIDTH))
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border())
        .rounded(cx.theme().radius)
        .overflow_hidden()
        .child(content)
}

fn dot(active: bool, attention: bool) -> PageDot {
    PageDot { active, attention }
}

fn groups() -> Vec<Vec<PageDot>> {
    vec![
        vec![dot(false, false), dot(true, false)],
        vec![dot(false, false), dot(false, true), dot(false, false)],
        vec![dot(false, false)],
    ]
}

fn bar(
    id: &'static str,
    title: &str,
    meta: &str,
    warning: bool,
    held: bool,
    cx: &App,
) -> impl IntoElement {
    let pill = compact_bar_pill(
        id,
        if warning {
            IconName::TriangleAlert
        } else {
            IconName::SquareTerminal
        },
        title.to_owned(),
        meta.to_owned(),
        warning,
        page_dots(&groups(), cx),
        cx,
    );
    let keys = compact_bar_button((id, 1_usize), IconName::Keyboard);
    phone(
        div()
            .border_t_1()
            .border_color(cx.theme().border())
            .child(compact_bar(pill, keys, held, cx)),
        cx,
    )
}

fn bars(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    states()
        .columns(2)
        .state(
            "rest",
            bar("bar-rest", "cargo watch", "zz · editor", false, false, cx),
        )
        .state(
            "held",
            bar("bar-held", "cargo watch", "zz · editor", false, true, cx),
        )
        .state(
            "attention",
            bar(
                "bar-warning",
                "python train.py",
                "exited with 1 · gpu-box",
                true,
                false,
                cx,
            ),
        )
        .state(
            "long title",
            bar(
                "bar-long",
                "claude · reviewing crates/zz-ui/src/command/palette_view.rs",
                "zz · agents · 2 of 4 panes",
                false,
                false,
                cx,
            ),
        )
        .into_any_element()
}

fn dots(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    states()
        .columns(2)
        .state("one pane", page_dots(&[vec![dot(true, false)]], cx))
        .state(
            "one window, four panes",
            page_dots(
                &[vec![
                    dot(false, false),
                    dot(false, false),
                    dot(true, false),
                    dot(false, false),
                ]],
                cx,
            ),
        )
        .state("three windows", page_dots(&groups(), cx))
        .state(
            "attention in every window",
            page_dots(
                &[
                    vec![dot(true, false), dot(false, true)],
                    vec![dot(false, true)],
                    vec![dot(false, false), dot(false, true)],
                ],
                cx,
            ),
        )
        .into_any_element()
}

fn close(id: &'static str, cx: &App) -> AnyElement {
    pane_header_icon_button(id, IconName::Xmark, true, cx).into_any_element()
}

fn headers(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    states()
        .columns(2)
        .state(
            "close action",
            phone(
                compact_pane_header(
                    IconName::SquareTerminal,
                    "cargo watch",
                    [close("header-close", cx)],
                    cx,
                ),
                cx,
            ),
        )
        .state(
            "no actions",
            phone(
                compact_pane_header(IconName::SquareTerminal, "~/dev/zz", [], cx),
                cx,
            ),
        )
        .state(
            "two actions, long title",
            phone(
                compact_pane_header(
                    IconName::SquareTerminal,
                    "ssh gpu-box · tail -f /var/log/train/run-2026-10-09.log",
                    [
                        pane_header_icon_button("header-copy", IconName::Copy, true, cx)
                            .into_any_element(),
                        close("header-close-long", cx),
                    ],
                    cx,
                ),
                cx,
            ),
        )
        .state(
            "disabled action",
            phone(
                compact_pane_header(
                    IconName::SquareTerminal,
                    "read-only attach",
                    [
                        pane_header_icon_button("header-disabled", IconName::Xmark, false, cx)
                            .into_any_element(),
                    ],
                    cx,
                ),
                cx,
            ),
        )
        .into_any_element()
}

fn hud(text: &'static str, cx: &App) -> impl IntoElement {
    phone(
        div()
            .relative()
            .h(px(140.0))
            .p(px(12.0))
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(12.0))
            .text_color(cx.theme().foreground.muted())
            .child("~/dev/zz main* > cargo test")
            .child(compact_hud(text, cx)),
        cx,
    )
}

fn huds(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    states()
        .columns(2)
        .state("short", hud("Copied", cx))
        .state("longer", hud("Pane 3 of 5 · zoomed", cx))
        .into_any_element()
}

fn prefix_items() -> (Vec<PopoverKeyItem>, PopoverKeyItem) {
    (
        vec![
            PopoverKeyItem::new("new-pane", "New pane").icon(IconName::Plus),
            PopoverKeyItem::new("new-window", "New window").icon(IconName::AppWindow),
            PopoverKeyItem::new("rename-pane", "Rename pane").icon(IconName::Pencil),
            PopoverKeyItem::new("last-pane", "Last pane").icon(IconName::History),
            PopoverKeyItem::new("kill-pane", "Kill pane")
                .icon(IconName::Xmark)
                .danger(),
        ],
        PopoverKeyItem::new("all-bindings", "All bindings").icon(IconName::Keyboard),
    )
}

fn tool_keys(cx: &mut App) -> ToolKeys {
    let (items, footer) = prefix_items();
    ToolKeys::new(items, Some(footer), cx)
}

fn key_row(keys: &ToolKeys) -> KeyRow {
    KeyRow::new(keys.row())
        .track_scroll(&keys.scroll)
        .on_key(|_, _, _| {})
        .on_hide(|_, _| {})
}

struct KeyRows {
    rest: ToolKeys,
    armed: ToolKeys,
}

impl KeyRows {
    fn new(cx: &mut Context<Self>) -> Self {
        let rest = tool_keys(cx);
        let armed = tool_keys(cx);
        armed.prefix.update(cx, |key, cx| key.set_armed(true, cx));
        Self { rest, armed }
    }
}

impl Render for KeyRows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        states()
            .columns(2)
            .state(
                "rest",
                phone(div().id("key-row-rest").child(key_row(&self.rest)), cx),
            )
            .state(
                "prefix armed",
                phone(div().id("key-row-armed").child(key_row(&self.armed)), cx),
            )
    }
}

struct Keys {
    rest: Entity<PopoverKey>,
    armed: Entity<PopoverKey>,
    plain: Entity<PopoverKey>,
    arrows: Entity<ArrowPad>,
}

impl Keys {
    fn new(cx: &mut Context<Self>) -> Self {
        let popover = |cx: &mut Context<Self>, armed: bool| {
            let key = cx.new(|cx| {
                let (items, footer) = prefix_items();
                PopoverKey::new("prefix", PopoverKeyTap::Open, cx)
                    .items(items)
                    .footer(footer)
            });
            key.update(cx, |key, cx| key.set_armed(armed, cx));
            key
        };
        Self {
            rest: popover(cx, false),
            armed: popover(cx, true),
            plain: cx.new(|cx| PopoverKey::new("menu", PopoverKeyTap::Open, cx)),
            arrows: cx.new(ArrowPad::new),
        }
    }
}

impl Render for Keys {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let slot = |id: &'static str, child: AnyElement| {
            div()
                .id(id)
                .p(px(8.0))
                .rounded(cx.theme().radius)
                .bg(cx.theme().background.raised(1))
                .child(child)
        };
        states()
            .columns(4)
            .state(
                "prefix, rest",
                row().child(slot("key-rest", self.rest.clone().into_any_element())),
            )
            .state(
                "prefix, armed",
                row().child(slot("key-armed", self.armed.clone().into_any_element())),
            )
            .state(
                "no items",
                row().child(slot("key-plain", self.plain.clone().into_any_element())),
            )
            .state(
                "arrow pad",
                row().child(slot("key-arrows", self.arrows.clone().into_any_element())),
            )
    }
}

fn binding_rows() -> Arc<[WhichKeyRow]> {
    super::which_key::prefix_rows()
        .into_iter()
        .filter(|row| {
            row.group
                .as_ref()
                .is_some_and(|group| matches!(group.as_ref(), "Windows" | "Panes" | "Yours"))
        })
        .collect()
}

fn bindings(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    phone(
        v_flex()
            .id("bindings-list")
            .h(px(560.0))
            .overflow_y_scroll()
            .child(WhichKeyList::new(binding_rows())),
        cx,
    )
    .into_any_element()
}

struct Screen {
    keys: ToolKeys,
}

impl Screen {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            keys: tool_keys(cx),
        }
    }
}

const SCREEN_LINES: &[&str] = &[
    "~/dev/zz main* > cargo test -p zz-ui",
    "   Compiling zz-ui v0.15.0",
    "    Finished `test` profile in 41.87s",
    "     Running unittests src/lib.rs",
    "",
    "running 214 tests",
    "test result: ok. 214 passed; 0 failed",
    "",
    "~/dev/zz main* > ",
];

impl Render for Screen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let pill = compact_bar_pill(
            "screen-pill",
            IconName::SquareTerminal,
            "cargo test",
            "zz · editor",
            false,
            page_dots(&groups(), cx),
            cx,
        );
        let keyboard = compact_bar_button("screen-keyboard", IconName::Keyboard);
        phone(
            v_flex()
                .h(px(640.0))
                .child(compact_pane_header(
                    IconName::SquareTerminal,
                    "cargo test",
                    [close("screen-close", cx)],
                    cx,
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .px(px(12.0))
                        .py(px(8.0))
                        .font_family(theme.mono_font_family.clone())
                        .text_size(px(12.0))
                        .line_height(px(17.0))
                        .text_color(theme.foreground)
                        .children(
                            SCREEN_LINES
                                .iter()
                                .map(|line| div().h(px(17.0)).child(*line)),
                        ),
                )
                .child(div().id("screen-key-row").child(key_row(&self.keys)))
                .child(
                    div()
                        .border_t_1()
                        .border_color(theme.border())
                        .child(compact_bar(pill, keyboard, false, cx)),
                ),
            cx,
        )
    }
}
