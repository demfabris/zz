use zpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
    px,
};
use zz_ui::{ActiveTheme as _, Colorize as _, h_flex, to_hex, v_flex};

use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "colors",
    name: "Colors",
    group: "Foundation",
    summary: "Six roots and a scrim. Every other color is derived from them at paint time, so a palette swap or a contrast change moves the whole UI at once.",
    sections: &[
        Section {
            id: "roots",
            name: "Roots",
            summary: "The only colors the UI names. Read them off cx.theme().",
            build: |_, cx| stateless(roots, cx),
        },
        Section {
            id: "derived",
            name: "Derived",
            summary: "Colorize moves a color away from its own lightness, so one rule covers light mode, dark mode and colored controls.",
            build: |_, cx| stateless(derived, cx),
        },
    ],
};

fn roots(_: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .flex_wrap()
        .gap_3()
        .child(swatch("background", theme.background, cx))
        .child(swatch("foreground", theme.foreground, cx))
        .child(swatch("accent", theme.accent, cx))
        .child(swatch("success", theme.success, cx))
        .child(swatch("warning", theme.warning, cx))
        .child(swatch("danger", theme.danger, cx))
        .child(swatch("scrim", theme.scrim, cx))
        .child(swatch("border()", theme.border(), cx))
        .into_any_element()
}

fn derived(_: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let background = theme.background;
    let foreground = theme.foreground;
    let accent = theme.accent;
    states()
        .state(
            "background.raised(0..=4)",
            h_flex().gap_3().children(
                (0..=4)
                    .map(|level| swatch(format!("raised({level})"), background.raised(level), cx)),
            ),
        )
        .state(
            "background.washed(1..=4)",
            h_flex().gap_3().children(
                (1..=4)
                    .map(|level| swatch(format!("washed({level})"), background.washed(level), cx)),
            ),
        )
        .state(
            "foreground",
            h_flex()
                .gap_3()
                .child(swatch("muted()", foreground.muted(), cx))
                .child(swatch("subtle()", foreground.subtle(), cx))
                .child(swatch("wash()", foreground.wash(), cx))
                .child(swatch("glow()", foreground.glow(), cx)),
        )
        .state(
            "accent",
            h_flex()
                .gap_3()
                .child(swatch("fill()", accent.fill(), cx))
                .child(swatch("outline()", accent.outline(), cx))
                .child(swatch("on()", accent.on(), cx))
                .child(swatch("selection", theme.selection_background(), cx)),
        )
        .into_any_element()
}

fn swatch(name: impl Into<SharedString>, color: Hsla, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .w(px(104.0))
        .gap_1p5()
        .child(
            div()
                .h(px(44.0))
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border())
                .bg(color),
        )
        .child(
            div()
                .text_size(px(12.0))
                .line_height(px(16.0))
                .child(name.into()),
        )
        .child(
            div()
                .text_size(px(11.0))
                .line_height(px(14.0))
                .font_family(theme.mono_font_family.clone())
                .text_color(theme.foreground.muted())
                .child(to_hex(color)),
        )
}
