use zz_gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement as _, Styled as _, div, linear_color_stop,
    linear_gradient, pattern_slash, px,
};
use zz_ui::{ActiveTheme as _, Colorize as _, h_flex, v_flex};

use crate::knobs::Backdrop;

#[derive(Clone, Copy)]
enum Ink {
    Text,
    Muted,
    Accent,
    Success,
    Warning,
    Danger,
}

const CODE: &[&[(Ink, &str)]] = &[
    &[
        (Ink::Success, "~/dev/zz"),
        (Ink::Muted, " main "),
        (Ink::Accent, "$ "),
        (Ink::Text, "cargo build --release -p zz"),
    ],
    &[
        (Ink::Success, "   Compiling "),
        (
            Ink::Text,
            "zz-gpui v0.1.0 (/Users/fabrico/dev/zz/crates/zz-gpui)",
        ),
    ],
    &[
        (Ink::Success, "   Compiling "),
        (
            Ink::Text,
            "zz-gpui-platform v0.1.0 (crates/zz-gpui-platform)",
        ),
    ],
    &[
        (Ink::Warning, "warning"),
        (Ink::Text, ": unused variable: `frame`"),
        (Ink::Muted, " --> src/glass.rs:212:9"),
    ],
    &[
        (Ink::Success, "   Compiling "),
        (Ink::Text, "zz-ui v0.1.0 (crates/zz-ui)"),
    ],
    &[
        (Ink::Success, "    Finished "),
        (
            Ink::Text,
            "`release` profile [optimized] target(s) in 41.27s",
        ),
    ],
    &[
        (Ink::Accent, "pub fn "),
        (
            Ink::Text,
            "paint_glass(&mut self, bounds: Bounds<Pixels>) {",
        ),
    ],
    &[(
        Ink::Muted,
        "    // one pass break per batch, the last run resumes the scene",
    )],
    &[
        (Ink::Accent, "    let "),
        (Ink::Text, "reach = material."),
        (Ink::Warning, "backdrop_reach"),
        (Ink::Text, "();"),
    ],
    &[
        (Ink::Accent, "    if "),
        (Ink::Text, "!self.supports_backdrop_sampling() { "),
        (Ink::Accent, "return"),
        (Ink::Text, "; }"),
    ],
    &[(Ink::Text, "}")],
    &[
        (Ink::Success, "~/dev/zz"),
        (Ink::Muted, " main "),
        (Ink::Accent, "$ "),
        (Ink::Text, "ls -la crates/"),
    ],
    &[
        (Ink::Accent, "drwxr-xr-x"),
        (Ink::Muted, "  14 fabrico staff  448 "),
        (Ink::Text, "zz"),
    ],
    &[
        (Ink::Accent, "drwxr-xr-x"),
        (Ink::Muted, "  22 fabrico staff  704 "),
        (Ink::Text, "zz-daemon"),
    ],
    &[
        (Ink::Accent, "drwxr-xr-x"),
        (Ink::Muted, "   9 fabrico staff  288 "),
        (Ink::Text, "zz-gpui-kit"),
    ],
    &[
        (Ink::Accent, "drwxr-xr-x"),
        (Ink::Muted, "  31 fabrico staff  992 "),
        (Ink::Text, "zz-ui"),
    ],
    &[
        (Ink::Success, "~/dev/zz"),
        (Ink::Muted, " main "),
        (Ink::Accent, "$ "),
        (Ink::Text, "cargo test -p zz-mux"),
    ],
    &[
        (Ink::Text, "test layout::tests::split_keeps_ratios ... "),
        (Ink::Success, "ok"),
    ],
    &[
        (
            Ink::Text,
            "test session::tests::rename_window_rejects_dup ... ",
        ),
        (Ink::Success, "ok"),
    ],
    &[
        (Ink::Text, "test copy::tests::rectangle_select ... "),
        (Ink::Danger, "FAILED"),
    ],
    &[
        (Ink::Danger, "error"),
        (Ink::Text, ": test failed, to rerun pass `-p zz-mux --lib`"),
    ],
    &[
        (Ink::Success, "~/dev/zz"),
        (Ink::Muted, " main "),
        (Ink::Accent, "$ "),
        (Ink::Text, "git log --oneline -3"),
    ],
    &[
        (Ink::Warning, "37013c871 "),
        (Ink::Text, "Merge remote-tracking branch 'origin/main'"),
    ],
    &[
        (Ink::Warning, "839b1620d "),
        (
            Ink::Text,
            "Ledger: pin.customize-3-8 active on top of the float track",
        ),
    ],
];

const FLOATING: &[&str] = &[
    "menus/popup-menu",
    "menus/highlighted-row",
    "menus/submenu-open",
    "menus/dropdown-open",
    "menus/context-menu-open",
    "notifications",
    "dialogs",
    "sheet",
    "choices/select-open",
    "floating-menus",
    "feedback-dialogs",
    "which-key",
    "command-palette/surface",
    "command-palette/workspace",
    "command-palette/workspace-flat",
    "command-palette/search",
    "command-palette/navigate-mode",
    "command-palette/pane-mode",
    "command-palette/host-mode",
    "command-palette/offline",
    "command-palette/command-mode",
    "command-palette/command-query",
    "command-palette/no-matches",
    "daemon-prompts",
    "chooser",
    "path-picker",
    "phone/hud",
    "phone/keys",
    "panes/floating",
    "browser/omnibox",
    "browser/action-menu",
    "browser/site-menu",
];

pub fn floats(story: &str, section: &str) -> bool {
    if story == crate::stories::STYLE_CREATOR {
        return crate::stories::style_creator_source(section)
            .is_some_and(|source| floats(source, section));
    }
    FLOATING.iter().any(|entry| match entry.split_once('/') {
        Some((entry_story, entry_section)) => entry_story == story && entry_section == section,
        None => *entry == story,
    })
}

pub fn backdrop(kind: Backdrop, cx: &App) -> Option<AnyElement> {
    match kind {
        Backdrop::Plain => None,
        Backdrop::Code => Some(code(cx)),
        Backdrop::Color => Some(color(cx)),
    }
}

fn code(cx: &App) -> AnyElement {
    let theme = cx.theme();
    let ink = |ink: Ink| -> Hsla {
        match ink {
            Ink::Text => theme.foreground,
            Ink::Muted => theme.foreground.muted(),
            Ink::Accent => theme.accent,
            Ink::Success => theme.success,
            Ink::Warning => theme.warning,
            Ink::Danger => theme.danger,
        }
    };
    v_flex()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .p(px(12.0))
        .font_family(theme.mono_font_family.clone())
        .text_size(px(13.0))
        .line_height(px(18.0))
        .children(CODE.iter().cycle().take(CODE.len() * 3).map(|line| {
            h_flex().flex_none().whitespace_nowrap().children(
                line.iter()
                    .map(|(color, text)| div().text_color(ink(*color)).child(*text)),
            )
        }))
        .into_any_element()
}

fn color(cx: &App) -> AnyElement {
    let theme = cx.theme();
    let blob = |left: f32, top: f32, size: f32, color: Hsla| {
        div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .size(px(size))
            .rounded_full()
            .bg(color)
    };
    div()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .bg(linear_gradient(
            120.0,
            linear_color_stop(theme.accent, 0.0),
            linear_color_stop(theme.danger, 1.0),
        ))
        .child(blob(40.0, 30.0, 220.0, theme.warning))
        .child(blob(360.0, 120.0, 180.0, theme.success))
        .child(blob(620.0, -40.0, 260.0, theme.background))
        .child(blob(180.0, 260.0, 160.0, theme.foreground))
        .child(div().absolute().inset_0().bg(pattern_slash(
            theme.background.alpha(0.35),
            4.0,
            16.0,
        )))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::STORIES;

    #[test]
    fn every_floating_entry_names_a_section() {
        for entry in FLOATING {
            let (story, section) = entry.split_once('/').unwrap_or((entry, ""));
            let story = STORIES
                .iter()
                .find(|candidate| candidate.id == story)
                .unwrap_or_else(|| panic!("no story {entry}"));
            assert!(
                section.is_empty() || story.sections.iter().any(|s| s.id == section),
                "no section {entry}"
            );
        }
    }
}
