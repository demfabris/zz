use gpui::{App, ElementId, IntoElement, ParentElement as _, SharedString, Styled as _, div, px};

use super::{
    SettingEntry, SettingsSection, SettingsStack, settings_control_fill, settings_provenance_badge,
};
use crate::{
    ActiveTheme as _, Colorize as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RENDERER: &str = env!("ZZ_UI_GPUI_REVISION");
pub const REPOSITORY_URL: &str = "https://github.com/demfabris/zz";
pub const RELEASES_URL: &str = "https://github.com/demfabris/zz/releases";
pub const ISSUES_URL: &str = "https://github.com/demfabris/zz/issues/new";
pub const ABOUT_LOGO_SIZE: f32 = 88.0;

#[must_use]
pub fn about_hero(logo: impl IntoElement, cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.0))
        .pt(px(20.0))
        .pb(px(10.0))
        .child(logo)
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(5.0))
                .child(
                    div()
                        .text_size(crate::rems_from_px(22.0))
                        .font_medium()
                        .child("zz"),
                )
                .child(
                    div()
                        .text_size(crate::rems_from_px(12.0))
                        .text_color(cx.theme().foreground.muted())
                        .child(SettingsSection::About.description()),
                )
                .child(settings_provenance_badge(concat!(
                    "v",
                    env!("CARGO_PKG_VERSION")
                ))),
        )
}

#[must_use]
pub fn about_value(value: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(crate::rems_from_px(11.0))
        .text_color(cx.theme().foreground.muted())
        .child(value.into())
}

#[must_use]
pub fn about_link_button(
    id: impl Into<ElementId>,
    label: &'static str,
    url: &'static str,
    cx: &App,
) -> Button {
    Button::new(id)
        .small()
        .icon(IconName::ExternalLink)
        .label(label)
        .bg(settings_control_fill(cx))
        .on_click(move |_, _, cx| cx.open_url(url))
}

#[must_use]
pub fn about_copy_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .xsmall()
        .compact()
        .ghost()
        .icon(IconName::Copy)
        .tooltip("Copy build information")
}

#[must_use]
pub fn build_info(platform: &str) -> String {
    format!("zz {VERSION} ({platform}, gpui {RENDERER})")
}

#[must_use]
pub fn about_build_stack(
    platform: impl Into<SharedString>,
    copy: impl IntoElement,
    cx: &App,
) -> SettingsStack {
    SettingsStack::titled("Build")
        .description("What to quote in a bug report.")
        .child(
            SettingEntry::new("Version", "The zz bundle version.")
                .title_actions(copy)
                .control(about_value(VERSION, cx)),
        )
        .child(
            SettingEntry::new(
                "Platform",
                "The operating system and processor architecture this build targets.",
            )
            .control(about_value(platform, cx)),
        )
        .child(
            SettingEntry::new("Renderer", "The GPUI revision this build links.")
                .control(about_value(RENDERER, cx)),
        )
}

#[must_use]
pub fn about_project_stack(cx: &App) -> SettingsStack {
    SettingsStack::titled("Project")
        .child(
            SettingEntry::new(
                "Source code",
                "zz is open source. Read it, build it, or send a patch.",
            )
            .control(about_link_button(
                "settings-about-repository",
                "GitHub",
                REPOSITORY_URL,
                cx,
            )),
        )
        .child(
            SettingEntry::new(
                "Releases",
                "Every tagged build, with notes on what changed.",
            )
            .control(about_link_button(
                "settings-about-releases",
                "Releases",
                RELEASES_URL,
                cx,
            )),
        )
        .child(
            SettingEntry::new(
                "Report an issue",
                "Something broken or missing? Bring the build details above.",
            )
            .control(about_link_button(
                "settings-about-issues",
                "New issue",
                ISSUES_URL,
                cx,
            )),
        )
        .child(
            SettingEntry::new(
                "License",
                "Dual-licensed, at your option. Contributions land under both.",
            )
            .control(about_value("MIT or Apache-2.0", cx)),
        )
}
