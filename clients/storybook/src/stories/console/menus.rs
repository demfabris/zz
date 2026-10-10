use zz_gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement as _, Pixels, SharedString, Styled as _,
    Window, div, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _,
    command::floating::{confirm_prompt, menu_row, menu_separator},
    pane::FloatingSurface,
    tmux_style::tmux_style_colour,
};

use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "floating-menus",
    name: "Floating menus",
    group: "Commands",
    summary: "display-menu and confirm-before, drawn as floating surfaces on the terminal grid: rows are one cell tall and a border takes one cell on each side.",
    sections: &[
        Section {
            id: "display-menu",
            name: "display-menu",
            summary: "A titled menu with separators, key annotations, disabled items and the selected row.",
            build: |_, cx| stateless(display_menu, cx),
        },
        Section {
            id: "menu-styles",
            name: "Menu styles",
            summary: "-b none drops the border and its inset. -s, -S and -H color the menu, the border and the selected row the tmux way.",
            build: |_, cx| stateless(menu_styles, cx),
        },
        Section {
            id: "confirm-before",
            name: "confirm-before",
            summary: "The one-line question confirm-before asks, sized to its prompt.",
            build: |_, cx| stateless(confirm, cx),
        },
    ],
};

const CELL_WIDTH: f32 = 8.0;
const CELL_HEIGHT: f32 = 19.0;
const MENU_COLUMNS: f32 = 26.0;

const PANE_MENU: &[Option<(&str, Option<&str>, bool)>] = &[
    Some(("Horizontal Split", Some("h"), true)),
    Some(("Vertical Split", Some("v"), true)),
    None,
    Some(("Swap Up", Some("u"), true)),
    Some(("Swap Down", Some("d"), true)),
    Some(("Swap Marked", Some("s"), false)),
    None,
    Some(("Kill", Some("X"), true)),
    Some(("Respawn", Some("R"), true)),
    Some(("Mark", Some("m"), true)),
    Some(("Rename", Some("r"), true)),
];

struct MenuColors {
    background: Hsla,
    foreground: Hsla,
    border: Hsla,
    selected_background: Hsla,
    selected_foreground: Hsla,
}

impl MenuColors {
    fn theme(cx: &App) -> Self {
        Self {
            background: cx.theme().background.raised(1).opaque(),
            foreground: cx.theme().foreground,
            border: cx.theme().border(),
            selected_background: cx.theme().background.raised(2).opaque(),
            selected_foreground: cx.theme().foreground,
        }
    }

    fn styled(style: &str, border_style: &str, selected_style: &str, cx: &App) -> Self {
        let theme = Self::theme(cx);
        Self {
            background: tmux_style_colour(style, "bg", theme.background, cx),
            foreground: tmux_style_colour(style, "fg", theme.foreground, cx),
            border: tmux_style_colour(border_style, "fg", theme.border, cx),
            selected_background: tmux_style_colour(
                selected_style,
                "bg",
                theme.selected_background,
                cx,
            ),
            selected_foreground: tmux_style_colour(
                selected_style,
                "fg",
                theme.selected_foreground,
                cx,
            ),
        }
    }
}

fn menu(
    id: &'static str,
    title: &str,
    selected: Option<usize>,
    bordered: bool,
    colors: &MenuColors,
    cx: &App,
) -> AnyElement {
    let row_height = px(CELL_HEIGHT);
    let font = cx.theme().mono_font_family.clone();
    let rows = PANE_MENU
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            Some((name, key, enabled)) => menu_row(
                (id, index),
                *name,
                key.map(SharedString::from),
                *enabled,
                selected == Some(index),
                row_height,
                colors.selected_background,
                colors.selected_foreground,
                font.clone(),
                cx,
            )
            .into_any_element(),
            None => menu_separator((id, index), row_height, cx).into_any_element(),
        });
    let inset: (Pixels, Pixels) = if bordered {
        (px(CELL_WIDTH), px(CELL_HEIGHT))
    } else {
        (px(0.0), px(0.0))
    };
    let lines = PANE_MENU.len() as f32 + if bordered { 2.0 } else { 0.0 };
    let columns = MENU_COLUMNS + if bordered { 2.0 } else { 0.0 };
    div()
        .w(px(columns * CELL_WIDTH))
        .h(px(lines * CELL_HEIGHT))
        .child(
            FloatingSurface::new(
                id,
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .children(rows),
                cx,
            )
            .title(title.to_owned())
            .content_inset(inset.0, inset.1)
            .colors(colors.background, colors.foreground, colors.border)
            .bordered(bordered),
        )
        .into_any_element()
}

fn display_menu(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    let colors = MenuColors::theme(cx);
    states()
        .columns(3)
        .state(
            "nothing selected",
            menu("menu-rest", "Pane %3", None, true, &colors, cx),
        )
        .state(
            "row selected",
            menu("menu-selected", "Pane %3", Some(1), true, &colors, cx),
        )
        .state(
            "disabled row selected",
            menu("menu-disabled", "Pane %3", Some(5), true, &colors, cx),
        )
        .into_any_element()
}

fn menu_styles(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    let theme = MenuColors::theme(cx);
    let tmux = MenuColors::styled(
        "bg=colour236,fg=colour252",
        "fg=colour39",
        "bg=colour39,fg=colour16",
        cx,
    );
    let themed = MenuColors::styled(
        "bg=themedarkgrey,fg=themewhite",
        "fg=themegreen",
        "bg=themeyellow,fg=themeblack",
        cx,
    );
    states()
        .columns(3)
        .state(
            "-b none",
            menu("menu-borderless", "Pane %3", Some(1), false, &theme, cx),
        )
        .state(
            "colour256 styles",
            menu("menu-tmux", "Pane %3", Some(1), true, &tmux, cx),
        )
        .state(
            "theme colours",
            menu("menu-theme", "Pane %3", Some(1), true, &themed, cx),
        )
        .into_any_element()
}

fn confirm_surface(id: &'static str, prompt: &str, cx: &App) -> AnyElement {
    let width = (prompt.chars().count() as f32 * 8.0 + 32.0).max(180.0);
    div()
        .w(px(width))
        .h(px(48.0))
        .child(FloatingSurface::new(
            id,
            confirm_prompt(
                (id, 0_usize),
                prompt.to_owned(),
                cx.theme().mono_font_family.clone(),
            ),
            cx,
        ))
        .into_any_element()
}

fn confirm(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    states()
        .state(
            "short prompt, minimum width",
            confirm_surface("confirm-short", "kill-pane? (y/n)", cx),
        )
        .state(
            "long prompt",
            row().child(confirm_surface(
                "confirm-long",
                "kill-session -t notes? This also closes 3 windows (y/n)",
                cx,
            )),
        )
        .into_any_element()
}
