use zz_gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement as _, Styled as _, Window, div, px,
};
use zz_protocol::parse_styled_segments;
use zz_ui::{
    ActiveTheme as _, Colorize as _, h_flex,
    tmux_style::{split_tmux_alignment, tmux_style_colour, tmux_styled_segments_text},
    v_flex,
};

use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "tmux-styles",
    name: "tmux styles",
    group: "Commands",
    summary: "#[...] style runs from formats and options, turned into styled text: colours, attributes, theme colours and alignment.",
    sections: &[
        Section {
            id: "attributes",
            name: "Colours and attributes",
            summary: "One line per feature, each drawn from its style string by tmux_styled_segments_text.",
            build: |_, cx| stateless(attributes, cx),
        },
        Section {
            id: "theme-colours",
            name: "Theme colours",
            summary: "The ten theme colour names resolve to chrome colours, so a style can follow the zz palette.",
            build: |_, cx| stateless(theme_colours, cx),
        },
        Section {
            id: "alignment",
            name: "Alignment",
            summary: "split_tmux_alignment buckets runs by align=left, centre and right, the way a status line or pane border lays them out.",
            build: |_, cx| stateless(alignment, cx),
        },
    ],
};

const ATTRIBUTES: &[(&str, &str)] = &[
    (
        "named, 256 and rgb colours",
        "#[fg=red]red #[fg=green]green #[fg=colour208]colour208 #[fg=#7aa2f7]#7aa2f7 #[fg=brightmagenta]brightmagenta",
    ),
    (
        "backgrounds",
        "#[bg=blue,fg=white] blue #[default] #[bg=colour236] colour236 #[default] #[bg=#e0af68,fg=black] #e0af68 ",
    ),
    (
        "reverse",
        "#[reverse] reversed #[noreverse] plain #[fg=yellow,reverse] yellow reversed #[default]",
    ),
    (
        "bold and italics",
        "#[bold]bold#[nobold] #[italics]italics#[noitalics] #[bold,italics]bold italics",
    ),
    (
        "underlines",
        "#[underscore]underscore#[nounderscore] #[double-underscore]double#[nodouble-underscore] #[curly-underscore,us=red]curly red#[nocurly-underscore] #[dotted-underscore]dotted#[nodotted-underscore] #[dashed-underscore]dashed",
    ),
    (
        "strikethrough and dim",
        "#[strikethrough]strikethrough#[nostrikethrough] #[dim]dim#[nodim] #[dim=25]dim=25#[default] normal",
    ),
    (
        "status line window list",
        "#[fg=colour244] 1:zsh #[fg=black,bg=green,bold] 2:editor* #[default]#[fg=colour244] 3:agents- #[fg=yellow,italics] 4:logs#",
    ),
];

const THEME_COLOURS: [&str; 10] = [
    "themeblack",
    "themewhite",
    "themelightgrey",
    "themedarkgrey",
    "themegreen",
    "themeyellow",
    "themered",
    "themeblue",
    "themecyan",
    "thememagenta",
];

fn styled(source: &str, foreground: Hsla, background: Hsla, cx: &App) -> AnyElement {
    tmux_styled_segments_text(&parse_styled_segments(source), foreground, background, cx)
        .into_styled_text()
        .into_any_element()
}

fn line(source: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(13.0))
                .line_height(px(20.0))
                .text_color(theme.foreground)
                .child(styled(source, theme.foreground, theme.background, cx)),
        )
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(10.0))
                .text_color(theme.foreground.muted())
                .child(source.to_owned()),
        )
}

fn attributes(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    ATTRIBUTES
        .iter()
        .fold(states(), |states, (caption, source)| {
            states.state(*caption, line(source, cx))
        })
        .into_any_element()
}

fn theme_colours(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    let foregrounds = THEME_COLOURS
        .iter()
        .flat_map(|name| ["#[fg=", name, "]", name, " "])
        .collect::<String>();
    let backgrounds = THEME_COLOURS
        .iter()
        .flat_map(|name| ["#[bg=", name, "] ", name, " #[default] "])
        .collect::<String>();
    states()
        .state("as fg", line(&foregrounds, cx))
        .state("as bg", line(&backgrounds, cx))
        .into_any_element()
}

fn bar(source: &str, foreground: Hsla, background: Hsla, cx: &App) -> impl IntoElement {
    let [left, centre, right] = split_tmux_alignment(source);
    let text = |segments: &[zz_protocol::StyledSegment]| {
        tmux_styled_segments_text(segments, foreground, background, cx).into_styled_text()
    };
    v_flex()
        .w_full()
        .gap_1()
        .child(
            h_flex()
                .w_full()
                .h(px(24.0))
                .px(px(8.0))
                .rounded(cx.theme().radius)
                .bg(background)
                .text_color(foreground)
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(12.0))
                .child(div().flex_1().min_w_0().child(text(&left)))
                .child(div().flex_none().child(text(&centre)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .justify_end()
                        .child(text(&right)),
                ),
        )
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(10.0))
                .text_color(cx.theme().foreground.muted())
                .child(source.to_owned()),
        )
}

fn alignment(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    let theme = cx.theme();
    let raised = theme.background.raised(1).opaque();
    states()
        .state(
            "status line on the chrome background",
            bar(
                "#[fg=green,bold] zz #[default] 1:editor* 2:agents- 3:logs#[align=centre]#[italics]fabrico@macbook#[align=right]#[fg=colour244]load 1.42 #[fg=default,bold]14:32",
                theme.foreground,
                raised,
                cx,
            ),
        )
        .state(
            "pane border format",
            bar(
                "#[fg=themegreen]%3 #[default]cargo watch#[align=right]#[dim]~/dev/zz",
                theme.foreground.muted(),
                theme.background,
                cx,
            ),
        )
        .state(
            "classic tmux green",
            bar(
                "[zz] 0:zsh  1:nvim* 2:htop-#[align=right]\"macbook\" 14:32 09-Oct-26",
                tmux_style_colour("fg=black", "fg", theme.foreground, cx),
                tmux_style_colour("bg=green", "bg", raised, cx),
                cx,
            ),
        )
        .state(
            "right only",
            bar(
                "#[align=right]#[fg=red,bold]80x24",
                theme.foreground,
                raised,
                cx,
            ),
        )
        .into_any_element()
}
