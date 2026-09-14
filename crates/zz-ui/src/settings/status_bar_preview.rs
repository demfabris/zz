use std::rc::Rc;

use gpui::{App, IntoElement, div, prelude::*, px};
pub use zz_client::StatusBarSettings;

use crate::{
    ActiveTheme as _, Colorize as _, IconName,
    navigation::{
        WorkspaceStatusWindowState,
        status::{
            StatusAgentEntry, StatusPaneEntry, StatusSessionEntry, StatusWindowActions,
            status_agents, status_session, status_window,
        },
        workspace_status_item,
    },
    shell::{WorkspaceStatusSlots, workspace_status_bar},
};

use super::{
    SETTINGS_PAGE_PADDING, SettingsSection, settings_group_header, settings_page_content,
    settings_page_description, settings_scroll_column,
};

pub fn status_bar_page(
    settings: StatusBarSettings,
    gaps: bool,
    controls: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .child(
            div().flex_none().p(px(SETTINGS_PAGE_PADDING)).pb_0().child(
                settings_page_content()
                    .gap(px(12.0))
                    .child(settings_page_description(SettingsSection::StatusBar, cx))
                    .child(settings_group_header(
                        "Preview".into(),
                        Some("Sample windows, a remote host, and an available update.".into()),
                        cx,
                    ))
                    .child(status_bar_preview(settings, gaps, cx)),
            ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(settings_scroll_column("settings-status-bar").child(controls)),
        )
}

fn status_bar_preview(settings: StatusBarSettings, gaps: bool, cx: &App) -> gpui::Div {
    let session = settings.show_session.then(|| {
        status_session(
            "settings-preview-session",
            "dev".into(),
            vec![StatusSessionEntry {
                label: "dev · 2 windows".into(),
                active: true,
                select: Rc::new(|_, _| {}),
            }],
            true,
            cx,
        )
    });
    let windows = ["workspace", "logs"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            status_window(
                ("settings-preview-window", index),
                index.to_string().into(),
                name.into(),
                WorkspaceStatusWindowState {
                    connected: true,
                    active: index == 0,
                    bell: settings.badges && index == 1,
                    activity: settings.badges && index == 1,
                },
                if index == 0 {
                    [
                        (IconName::SquareTerminal, "cargo test"),
                        (IconName::Globe, "GPUI docs"),
                        (
                            crate::pane::agent_provider_icon(zz_protocol::AgentProvider::Codex),
                            "Rework status bar",
                        ),
                        (IconName::Claude, "Review pane titles"),
                        (IconName::File, "status_bar.rs"),
                        (IconName::SquareTerminal, "dev server"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (icon, label))| StatusPaneEntry {
                        label: label.into(),
                        detail: "Sample pane".into(),
                        icon,
                        favicon: None,
                        active: index == 1,
                        select: Rc::new(|_, _| {}),
                    })
                    .collect()
                } else {
                    vec![StatusPaneEntry {
                        label: "daemon logs".into(),
                        detail: "Terminal".into(),
                        icon: IconName::SquareTerminal,
                        favicon: None,
                        active: true,
                        select: Rc::new(|_, _| {}),
                    }]
                },
                StatusWindowActions {
                    select: Rc::new(|_, _| {}),
                    close: None,
                    rename: None,
                },
                cx,
            )
        })
        .collect();
    let mut right = Vec::new();
    if settings.show_agents {
        right.push(status_agents(
            "settings-preview-agents",
            [
                (
                    crate::pane::agent_provider_icon(zz_protocol::AgentProvider::Codex),
                    "Rework status bar",
                ),
                (IconName::Claude, "Review pane titles"),
            ]
            .into_iter()
            .map(|(icon, label)| StatusAgentEntry {
                label: label.into(),
                window_name: "workspace".into(),
                icon,
                status: Some(zz_client::AgentAttentionStatus::Working),
                select: Rc::new(|_, _| {}),
            })
            .collect(),
            true,
            cx,
        ));
    }
    if settings.show_host {
        right.push(
            workspace_status_item(
                "settings-preview-host",
                Some(IconName::Globe),
                "devbox".into(),
                cx,
            )
            .into_any_element(),
        );
    }
    if settings.show_update {
        right.push(
            workspace_status_item("settings-preview-update", None, "v0.9.0".into(), cx)
                .flex_none()
                .px(px(6.0))
                .rounded(cx.theme().radius)
                .when(cx.theme().shadow, |item| {
                    item.border(px(0.5)).border_color(gpui::transparent_white())
                })
                .child(
                    div()
                        .flex_none()
                        .size(px(5.0))
                        .rounded_full()
                        .bg(cx.theme().success),
                )
                .into_any_element(),
        );
    }
    div()
        .relative()
        .w_full()
        .flex_none()
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .bg(cx.theme().background.opaque())
        .child(
            workspace_status_bar(
                gaps,
                px(0.0),
                WorkspaceStatusSlots {
                    session,
                    windows,
                    right,
                    ..Default::default()
                },
                cx,
            )
            .id("settings-status-bar-preview"),
        )
}
