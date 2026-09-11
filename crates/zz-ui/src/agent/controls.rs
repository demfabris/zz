use std::{f32::consts::PI, rc::Rc};

use gpui::{
    Anchor, AnyElement, App, ElementId, IntoElement, PathBuilder, Role, SharedString, Window,
    canvas, div, point, prelude::*, px,
};

use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    tooltip::Tooltip,
    v_flex,
};

use super::composer::COMPOSER_FOOTER_HEIGHT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerAction {
    Send,
    Queue,
    Stop,
}

pub const fn composer_action(active_turn: bool, has_content: bool) -> ComposerAction {
    match (active_turn, has_content) {
        (false, _) => ComposerAction::Send,
        (true, true) => ComposerAction::Queue,
        (true, false) => ComposerAction::Stop,
    }
}

pub fn composer_action_button(
    id: impl Into<ElementId>,
    action: ComposerAction,
    enabled: bool,
) -> Button {
    let (icon, tooltip) = match action {
        ComposerAction::Send => (IconName::ArrowUp, "Send message"),
        ComposerAction::Queue => (IconName::Plus, "Queue this as the next turn"),
        ComposerAction::Stop => (IconName::Xmark, "Stop the current turn"),
    };
    Button::compact_icon(id, icon)
        .when(action == ComposerAction::Send, ButtonVariants::accent)
        .when(action == ComposerAction::Queue, ButtonVariants::secondary)
        .rounded_full()
        .tooltip(tooltip)
        .disabled(!enabled)
}

pub fn agent_chrome_button(id: impl Into<ElementId>) -> Button {
    Button::new(id).ghost().xsmall().h(px(24.0)).px_2()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentControlChoice {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

pub fn agent_config_picker(
    id: impl Into<ElementId>,
    icon: IconName,
    current_value: &str,
    fallback_label: &str,
    description: &str,
    choices: Vec<AgentControlChoice>,
    enabled: bool,
    on_select: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let current_label = choices
        .iter()
        .find(|choice| choice.value == current_value)
        .map_or_else(|| fallback_label.to_owned(), |choice| choice.name.clone());
    let current_value = current_value.to_owned();
    let on_select = Rc::new(on_select);
    agent_chrome_button(id)
        .icon(icon)
        .label(current_label)
        .dropdown_caret(true)
        .tooltip(description.to_owned())
        .disabled(!enabled || choices.is_empty())
        .dropdown_menu(move |menu, _, _| {
            choices.iter().fold(menu.min_w(px(250.0)), |menu, choice| {
                let name = choice.name.clone();
                let description = choice.description.clone();
                let value = choice.value.clone();
                let on_select = on_select.clone();
                menu.item(
                    PopupMenuItem::element(move |_, cx| {
                        v_flex()
                            .min_w_0()
                            .ml_1()
                            .py_1()
                            .child(
                                div()
                                    .text_size(crate::rems_from_px(12.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(name.clone()),
                            )
                            .when_some(description.clone(), |this, description| {
                                this.child(
                                    div()
                                        .max_w(px(300.0))
                                        .text_size(crate::rems_from_px(10.0))
                                        .text_color(cx.theme().foreground.muted())
                                        .child(description),
                                )
                            })
                    })
                    .checked(choice.value == current_value)
                    .on_click(move |_, window, cx| on_select(&value, window, cx)),
                )
            })
        })
        .anchor(Anchor::BottomLeft)
        .into_any_element()
}

pub fn git_file_count_label(count: u32) -> String {
    if count == 1 {
        "1 file".to_owned()
    } else {
        format!("{count} files")
    }
}

pub fn git_summary_footer(
    id: impl Into<ElementId>,
    branch: Option<SharedString>,
    changed_files: u32,
    additions: u32,
    deletions: u32,
    cx: &App,
) -> AnyElement {
    let files = git_file_count_label(changed_files);
    let tooltip = format!(
        "{}: {files}, +{additions} additions, -{deletions} deletions",
        branch.as_deref().unwrap_or("Detached HEAD"),
    );
    h_flex()
        .id(id)
        .min_w_0()
        .h(px(COMPOSER_FOOTER_HEIGHT))
        .items_center()
        .gap_2()
        .text_size(crate::rems_from_px(11.0))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(
            Icon::new(IconName::GitBranch)
                .xsmall()
                .flex_none()
                .text_color(cx.theme().foreground.muted()),
        )
        .when_some(branch, |this, branch| {
            this.child(
                div()
                    .min_w_0()
                    .max_w(px(220.0))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(cx.theme().foreground.muted())
                    .child(branch),
            )
        })
        .child(
            div()
                .flex_none()
                .text_color(cx.theme().foreground.muted())
                .child(files),
        )
        .child(
            div()
                .flex_none()
                .text_color(cx.theme().success)
                .child(format!("+{additions}")),
        )
        .child(
            div()
                .flex_none()
                .text_color(cx.theme().danger)
                .child(format!("-{deletions}")),
        )
        .into_any_element()
}

const CONTEXT_USAGE_RING_SIZE: f32 = 16.0;
const CONTEXT_USAGE_STROKE_WIDTH: f32 = 2.0;

#[allow(clippy::cast_precision_loss)]
pub fn context_usage_fraction(used: u64, size: u64) -> f64 {
    if size == 0 {
        0.0
    } else {
        (used.min(size) as f64 / size as f64).clamp(0.0, 1.0)
    }
}

pub fn context_usage_tooltip(used: u64, size: u64) -> String {
    if size == 0 {
        "Context window usage unavailable".to_owned()
    } else {
        format!(
            "{used} of {size} context tokens used ({:.0}%)",
            context_usage_fraction(used, size) * 100.0
        )
    }
}

#[allow(clippy::cast_possible_truncation)]
pub fn context_usage_meter(
    id: impl Into<ElementId>,
    used: u64,
    size: u64,
    cx: &gpui::App,
) -> AnyElement {
    let progress = context_usage_fraction(used, size) as f32;
    let tooltip = context_usage_tooltip(used, size);
    let track_color = cx.theme().foreground.muted().opacity(0.22);
    let progress_color = cx.theme().foreground.muted();
    let ring = canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let stroke = px(CONTEXT_USAGE_STROKE_WIDTH);
            let radius = px((CONTEXT_USAGE_RING_SIZE - CONTEXT_USAGE_STROKE_WIDTH) / 2.0);
            let center_x = bounds.origin.x + bounds.size.width / 2.0;
            let center_y = bounds.origin.y + bounds.size.height / 2.0;

            let mut track = PathBuilder::stroke(stroke);
            track.move_to(point(center_x + radius, center_y));
            track.arc_to(
                point(radius, radius),
                px(0.0),
                false,
                true,
                point(center_x - radius, center_y),
            );
            track.arc_to(
                point(radius, radius),
                px(0.0),
                false,
                true,
                point(center_x + radius, center_y),
            );
            track.close();
            if let Ok(path) = track.build() {
                window.paint_path(path, track_color);
            }

            if progress <= 0.0 {
                return;
            }
            let mut fill = PathBuilder::stroke(stroke);
            if progress >= 0.999 {
                fill.move_to(point(center_x + radius, center_y));
                fill.arc_to(
                    point(radius, radius),
                    px(0.0),
                    false,
                    true,
                    point(center_x - radius, center_y),
                );
                fill.arc_to(
                    point(radius, radius),
                    px(0.0),
                    false,
                    true,
                    point(center_x + radius, center_y),
                );
                fill.close();
            } else {
                fill.move_to(point(center_x, center_y - radius));
                let angle = -PI / 2.0 + progress * 2.0 * PI;
                fill.arc_to(
                    point(radius, radius),
                    px(0.0),
                    progress > 0.5,
                    true,
                    point(
                        center_x + radius * angle.cos(),
                        center_y + radius * angle.sin(),
                    ),
                );
            }
            if let Ok(path) = fill.build() {
                window.paint_path(path, progress_color);
            }
        },
    )
    .size(px(CONTEXT_USAGE_RING_SIZE));
    let aria_value = format!("{:.0}%", f64::from(progress) * 100.0);
    let hover_tooltip = tooltip.clone();

    div()
        .id(id)
        .role(Role::ProgressIndicator)
        .aria_label(tooltip)
        .aria_value(aria_value)
        .flex()
        .flex_none()
        .size(px(28.0))
        .items_center()
        .justify_center()
        .tooltip(move |window, cx| Tooltip::new(hover_tooltip.clone()).build(window, cx))
        .child(ring)
        .into_any_element()
}
