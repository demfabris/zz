use std::{f32::consts::PI, rc::Rc};

use gpui::{
    Anchor, AnyElement, App, ElementId, IntoElement, PathBuilder, Role, SharedString, Window,
    canvas, div, point, prelude::*, px,
};

use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _,
    StyledExt as _,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    pane::agent_provider_icon,
    popover::Popover,
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

pub fn agent_header_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    enabled: bool,
    cx: &App,
) -> Button {
    Button::compact_icon(id, icon)
        .disabled(!enabled)
        .when(enabled, |button| {
            button.text_color(cx.theme().foreground.muted())
        })
}

pub fn agent_directory_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    enabled: bool,
    cx: &App,
) -> Button {
    agent_chrome_button(id)
        .min_w_0()
        .flex_shrink_1()
        .max_w(px(140.0))
        .overflow_hidden()
        .disabled(!enabled)
        .when(enabled, |button| {
            button.text_color(cx.theme().foreground.muted())
        })
        .child(Icon::new(IconName::Folder).small().relative().top(px(0.5)))
        .child(
            div()
                .min_w_0()
                .text_ellipsis()
                .when(enabled, |label| label.text_color(cx.theme().foreground))
                .child(label.into()),
        )
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
                    PopupMenuItem::element(move |highlighted, _, cx| {
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
                                        .text_color(if highlighted {
                                            cx.theme().foreground
                                        } else {
                                            cx.theme().foreground.muted()
                                        })
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

#[derive(Clone, Debug, Default)]
pub struct AgentControlSelection {
    pub current_value: String,
    pub choices: Vec<AgentControlChoice>,
}

impl AgentControlSelection {
    fn selected(&self) -> Option<&AgentControlChoice> {
        self.choices
            .iter()
            .find(|choice| choice.value == self.current_value)
    }
}

fn agent_picker_row(
    id: impl Into<ElementId>,
    selected: bool,
    parent: bool,
    enabled: bool,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let focus = window
        .use_keyed_state(format!("{id}:focus"), cx, |_, cx| cx.focus_handle())
        .read(cx)
        .clone();
    let highlight = if parent && selected {
        gpui::StyleRefinement::default()
            .bg(cx.theme().background.washed(2))
            .text_color(cx.theme().foreground)
    } else {
        gpui::StyleRefinement::default().selection_highlight(cx)
    };
    h_flex()
        .id(id)
        .role(Role::Button)
        .aria_selected(selected)
        .flex_none()
        .min_h(px(32.0))
        .px_2()
        .py_1()
        .border(px(0.5))
        .border_color(gpui::transparent_white())
        .menu_item_corners(px(32.0), cx)
        .text_size(crate::rems_from_px(12.0))
        .line_height(px(16.0))
        .text_color(cx.theme().foreground)
        .when(enabled, |this| {
            this.track_focus(&focus.tab_stop(true))
                .when(selected, |this| this.refine_style(&highlight))
                .hover(move |this| this.refine_style(&highlight))
        })
        .when(!enabled, |this| {
            this.text_color(cx.theme().foreground.muted())
        })
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            if !enabled {
                cx.stop_propagation();
                return;
            }
            window.prevent_default();
            crate::text::suppress_text_selection(cx);
        })
        .child(content)
}

#[derive(Clone)]
struct AgentPickerCatalog {
    provider: zz_protocol::AgentProvider,
    model: AgentControlSelection,
    effort: Option<AgentControlSelection>,
}

impl AgentPickerCatalog {
    fn selection(&self) -> zz_client::agent_config::AgentSettingsSelection {
        zz_client::agent_config::AgentSettingsSelection {
            provider: self.provider,
            model: self.model.selected().map(|choice| choice.value.clone()),
            effort: self
                .effort
                .as_ref()
                .and_then(AgentControlSelection::selected)
                .map(|choice| choice.value.clone()),
        }
    }
}

struct AgentPickerDraft {
    initial: zz_client::agent_config::AgentSettingsSelection,
    provider: zz_protocol::AgentProvider,
    catalogs: Vec<AgentPickerCatalog>,
}

#[derive(Default)]
struct AgentPickerState {
    scope: (String, std::path::PathBuf),
    catalogs: Vec<AgentPickerCatalog>,
    draft: Option<AgentPickerDraft>,
}

pub fn agent_model_picker(
    id: impl Into<ElementId>,
    scope: (String, std::path::PathBuf),
    provider: zz_protocol::AgentProvider,
    model: AgentControlSelection,
    effort: Option<AgentControlSelection>,
    enabled: bool,
    models_ready: bool,
    mut catalog_results: Vec<zz_protocol::agent_stream::AgentCatalogResult>,
    on_load: impl Fn(zz_protocol::AgentProvider, &mut App) + 'static,
    on_apply: impl Fn(zz_client::agent_config::AgentSettingsSelection, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = id.into();
    let state = window.use_keyed_state(format!("{id}:catalogs"), cx, |_, _| {
        AgentPickerState::default()
    });
    catalog_results.retain(|result| scope.1.as_os_str().is_empty() || result.cwd == scope.1);
    let live = AgentPickerCatalog {
        provider,
        model,
        effort,
    };
    state.update(cx, |state, _| {
        if state.scope != scope {
            *state = AgentPickerState {
                scope,
                ..Default::default()
            };
        }
        for result in &catalog_results {
            if state
                .catalogs
                .iter()
                .any(|catalog| catalog.provider == result.catalog_provider)
            {
                continue;
            }
            let Some(options) = result
                .config_options
                .clone()
                .and_then(|options| serde_json::from_value(options).ok())
            else {
                continue;
            };
            let options = zz_client::agent_config::config_option_models(options);
            let selection = |category| {
                options
                    .iter()
                    .find(|option| option.category == category)
                    .map(|option| AgentControlSelection {
                        current_value: option.current_value.clone(),
                        choices: option
                            .choices
                            .iter()
                            .map(|choice| AgentControlChoice {
                                value: choice.value.clone(),
                                name: choice.name.clone(),
                                description: None,
                            })
                            .collect(),
                    })
            };
            state.catalogs.push(AgentPickerCatalog {
                provider: result.catalog_provider,
                model: selection(zz_client::agent_config::AgentConfigCategory::Model)
                    .unwrap_or_default(),
                effort: selection(zz_client::agent_config::AgentConfigCategory::ThoughtLevel),
            });
        }
        if let Some(draft) = &mut state.draft {
            for catalog in &state.catalogs {
                if !draft
                    .catalogs
                    .iter()
                    .any(|draft| draft.provider == catalog.provider)
                {
                    draft.catalogs.push(catalog.clone());
                }
            }
        }
        if models_ready {
            if let Some(catalog) = state
                .catalogs
                .iter_mut()
                .find(|catalog| catalog.provider == provider)
            {
                *catalog = live.clone();
            } else {
                state.catalogs.push(live.clone());
            }
        }
    });
    let label = live
        .model
        .selected()
        .map_or_else(|| provider.label().to_owned(), |choice| choice.name.clone());
    let trigger = agent_chrome_button(id.clone())
        .debug_selector(|| "agent-model-trigger".into())
        .icon(agent_provider_icon(provider))
        .label(label)
        .when_some(
            live.effort
                .as_ref()
                .and_then(AgentControlSelection::selected),
            |button, choice| {
                button.child(
                    div()
                        .text_color(cx.theme().foreground.muted())
                        .child(choice.name.clone()),
                )
            },
        )
        .tooltip("Choose vendor, model and effort")
        .disabled(!enabled);
    let dismiss_state = state.clone();
    let on_load = Rc::new(on_load);
    Popover::new(id)
        .p_0()
        .anchor(Anchor::BottomLeft)
        .trigger(trigger)
        .on_dismiss(move |_, cx| {
            let selection = dismiss_state.update(cx, |state, _| {
                let draft = state.draft.take()?;
                let mut selection = draft
                    .catalogs
                    .iter()
                    .find(|catalog| catalog.provider == draft.provider)
                    .map(AgentPickerCatalog::selection)
                    .unwrap_or(zz_client::agent_config::AgentSettingsSelection {
                        provider: draft.provider,
                        model: None,
                        effort: None,
                    });
                if selection == draft.initial {
                    return None;
                }
                if selection.provider == draft.initial.provider
                    && selection.model == draft.initial.model
                {
                    selection.model = None;
                    if selection.effort == draft.initial.effort {
                        selection.effort = None;
                    }
                }
                Some(selection)
            });
            if let Some(selection) = selection {
                on_apply(selection, cx);
            }
        })
        .content(move |_, window, cx| {
            let (selected, catalog, rows, has_effort) = state.update(cx, |state, _| {
                let draft = state.draft.get_or_insert_with(|| AgentPickerDraft {
                    initial: live.selection(),
                    provider,
                    catalogs: state.catalogs.clone(),
                });
                (
                    draft.provider,
                    draft
                        .catalogs
                        .iter()
                        .find(|catalog| catalog.provider == draft.provider)
                        .cloned(),
                    draft
                        .catalogs
                        .iter()
                        .map(|catalog| catalog.model.choices.len())
                        .max()
                        .unwrap_or(0)
                        .max(2),
                    draft.catalogs.iter().any(|catalog| {
                        catalog
                            .effort
                            .as_ref()
                            .is_some_and(|effort| !effort.choices.is_empty())
                    }),
                )
            });
            let width = px(400.0).min((window.viewport_size().width - px(16.0)).max(px(240.0)));
            let height = px(320.0).min((window.viewport_size().height - px(170.0)).max(px(100.0)));
            let panel_height =
                px(rows as f32 * 36.0 + 4.0).min(height) + px(if has_effort { 45.0 } else { 0.0 });
            let muted = cx.theme().foreground.muted();
            let mut vendors = v_flex()
                .w(px(if width < px(400.0) { 112.0 } else { 124.0 }))
                .flex_none()
                .p_1()
                .gap_1();
            for vendor in zz_protocol::AgentProvider::ALL {
                let state = state.clone();
                let on_load = on_load.clone();
                vendors = vendors.child(
                    agent_picker_row(
                        vendor.as_str(),
                        vendor == selected,
                        true,
                        enabled,
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(Icon::new(agent_provider_icon(vendor)).xsmall().flex_none())
                            .child(div().flex_1().min_w_0().child(vendor.label())),
                        window,
                        cx,
                    )
                    .debug_selector(move || vendor.as_str().into())
                    .on_click(move |_, window, cx| {
                        if !enabled {
                            return;
                        }
                        let needs_catalog = state.update(cx, |state, _| {
                            if let Some(draft) = &mut state.draft {
                                draft.provider = vendor;
                            }
                            !state
                                .catalogs
                                .iter()
                                .any(|catalog| catalog.provider == vendor)
                        });
                        if needs_catalog {
                            on_load(vendor, cx);
                        }
                        window.refresh();
                    }),
                );
            }
            let mut models = v_flex()
                .id("model-choices")
                .flex_1()
                .min_w_0()
                .max_h(height)
                .overflow_y_scroll()
                .p_1()
                .gap_1();
            if let Some(catalog) = &catalog {
                for choice in &catalog.model.choices {
                    let value = choice.value.clone();
                    let selector = value.clone();
                    let state = state.clone();
                    let checked = value == catalog.model.current_value;
                    models = models.child(
                        agent_picker_row(
                            value.clone(),
                            checked,
                            false,
                            enabled,
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_2()
                                .child(div().flex_1().min_w_0().child(choice.name.clone()))
                                .child(
                                    Icon::new(IconName::Check)
                                        .xsmall()
                                        .flex_none()
                                        .when(!checked, gpui::Styled::invisible),
                                ),
                            window,
                            cx,
                        )
                        .debug_selector(move || selector.clone())
                        .on_click(move |_, window, cx| {
                            if !enabled {
                                return;
                            }
                            state.update(cx, |state, _| {
                                if let Some(draft) = &mut state.draft
                                    && let Some(catalog) = draft
                                        .catalogs
                                        .iter_mut()
                                        .find(|catalog| catalog.provider == selected)
                                {
                                    catalog.model.current_value.clone_from(&value);
                                }
                            });
                            window.refresh();
                        }),
                    );
                }
            }
            if catalog
                .as_ref()
                .is_none_or(|catalog| catalog.model.choices.is_empty())
            {
                let result = catalog_results
                    .iter()
                    .find(|result| result.catalog_provider == selected);
                let message = if catalog.is_some() {
                    "This agent uses its default model.".to_owned()
                } else if let Some(error) = result.and_then(|result| result.error.as_deref()) {
                    format!("{error} Click the vendor to retry.")
                } else {
                    "Loading models…".to_owned()
                };
                models = models.child(div().p_2().text_xs().text_color(muted).child(message));
            }
            let mut content = v_flex()
                .id("agent-model-menu")
                .debug_selector(|| "agent-model-menu".into())
                .w(width)
                .h(panel_height)
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .items_stretch()
                        .child(vendors)
                        .child(div().w(px(1.0)).flex_none().bg(cx.theme().border()))
                        .child(models),
                );
            if let Some(effort) = catalog
                .and_then(|catalog| catalog.effort)
                .filter(|effort| !effort.choices.is_empty())
            {
                let index = effort
                    .choices
                    .iter()
                    .position(|choice| choice.value == effort.current_value)
                    .unwrap_or(0);
                let labels = effort
                    .choices
                    .iter()
                    .map(|choice| choice.name.clone().into())
                    .collect();
                let state = state.clone();
                content = content.child(
                    v_flex()
                        .w_full()
                        .px_3()
                        .py_2()
                        .gap_1()
                        .border_t_1()
                        .border_color(cx.theme().border())
                        .child(crate::slider::DiscreteSlider::new(
                            "agent-effort",
                            "Effort",
                            labels,
                            index,
                            move |index, window, cx| {
                                if !enabled {
                                    return;
                                }
                                state.update(cx, |state, _| {
                                    if let Some(draft) = &mut state.draft
                                        && let Some(catalog) = draft
                                            .catalogs
                                            .iter_mut()
                                            .find(|catalog| catalog.provider == selected)
                                        && let Some(current) = &mut catalog.effort
                                    {
                                        current
                                            .current_value
                                            .clone_from(&effort.choices[index].value);
                                    }
                                });
                                window.refresh();
                            },
                        )),
                );
            }
            content
        })
        .into_any_element()
}

pub fn agent_provider_label(provider: zz_protocol::AgentProvider, cx: &App) -> impl IntoElement {
    h_flex()
        .flex_none()
        .h(px(24.0))
        .gap(px(6.0))
        .pl_2()
        .text_size(crate::rems_from_px(12.0))
        .line_height(px(16.0))
        .text_color(cx.theme().foreground.muted())
        .child(
            Icon::new(agent_provider_icon(provider))
                .small()
                .relative()
                .top(px(0.5)),
        )
        .child(provider.label())
}

pub fn agent_thread_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() || title == "agent" {
        return "New session".to_owned();
    }
    let mut chars = title.chars();
    let mut label: String = chars.by_ref().take(50).collect();
    if chars.next().is_some() {
        label.pop();
        label.push('…');
    }
    label
}

pub fn agent_thread_button(id: impl Into<ElementId>, title: &str, cx: &App) -> Button {
    agent_chrome_button(id)
        .text()
        .flat()
        .text_color(cx.theme().foreground.muted().opacity(0.45))
        .flex_shrink_1()
        .min_w_0()
        .overflow_hidden()
        .tooltip(format!("{title} · rename session"))
        .child(
            div()
                .min_w_0()
                .text_ellipsis()
                .child(agent_thread_title(title)),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Modifiers, Render, TestAppContext, VisualTestContext};
    use zz_protocol::AgentProvider;

    #[test]
    fn thread_titles_fit_fifty_characters_without_cutting_utf8() {
        assert_eq!(agent_thread_title(""), "New session");
        assert_eq!(agent_thread_title("agent"), "New session");
        assert_eq!(
            agent_thread_title("  Fix the composer  "),
            "Fix the composer"
        );
        assert_eq!(agent_thread_title(&"界".repeat(50)), "界".repeat(50));
        assert_eq!(
            agent_thread_title(&"界".repeat(51)),
            format!("{}…", "界".repeat(49))
        );
    }

    struct ModelPickerTest {
        provider: AgentProvider,
        applied: Vec<zz_client::agent_config::AgentSettingsSelection>,
        catalogs: Vec<zz_protocol::agent_stream::AgentCatalogResult>,
        loads: Vec<zz_protocol::AgentProvider>,
        scope: String,
    }

    impl Render for ModelPickerTest {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = cx.entity();
            let catalog_view = view.clone();
            let values = if self.provider == AgentProvider::Codex {
                ["model-a", "model-b"]
            } else {
                ["claude-a", "claude-b"]
            };
            div().pt(px(400.0)).child(agent_model_picker(
                "test-model-picker",
                (self.scope.clone(), std::path::PathBuf::default()),
                self.provider,
                AgentControlSelection {
                    current_value: values[0].into(),
                    choices: values
                        .into_iter()
                        .map(|value| AgentControlChoice {
                            value: value.into(),
                            name: value.into(),
                            description: Some("Hidden description".into()),
                        })
                        .collect(),
                },
                Some(AgentControlSelection {
                    current_value: "low".into(),
                    choices: ["low", "high"]
                        .into_iter()
                        .map(|value| AgentControlChoice {
                            value: value.into(),
                            name: value.into(),
                            description: None,
                        })
                        .collect(),
                }),
                true,
                true,
                self.catalogs.clone(),
                move |provider, cx| {
                    catalog_view.update(cx, |view, cx| {
                        view.loads.push(provider);
                        cx.notify();
                    });
                },
                move |selection, cx| {
                    view.update(cx, |view, cx| {
                        view.provider = selection.provider;
                        view.applied.push(selection);
                        cx.notify();
                    });
                },
                window,
                cx,
            ))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        draw(cx);
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing {selector}"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        draw(cx);
    }

    #[gpui::test]
    fn model_highlights_share_menu_corners_and_keep_the_provider_neutral(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
            crate::Theme::global_mut(cx).radius = px(25.0);
        });
        let (_, cx) = cx.add_window_view(|window, _| {
            window.set_adaptive_corner_fraction(Some(0.45));
            window.set_default_corner_smoothing(4.0);
            ModelPickerTest {
                provider: AgentProvider::Codex,
                applied: Vec::new(),
                catalogs: Vec::new(),
                loads: Vec::new(),
                scope: "local:/project".into(),
            }
        });
        click(cx, "agent-model-trigger");
        let provider = cx.debug_bounds("codex").unwrap();
        let model = cx.debug_bounds("model-a").unwrap();
        let hovered = cx.debug_bounds("model-b").unwrap();
        cx.simulate_mouse_move(hovered.center(), None, Modifiers::default());
        draw(cx);
        cx.update(|window, cx| {
            let scale = window.scale_factor();
            let quads = window.painted_quads();
            for (bounds, color) in [
                (provider, cx.theme().background.washed(2)),
                (model, cx.theme().selection_background()),
                (hovered, cx.theme().selection_background()),
            ] {
                let quad = quads
                    .iter()
                    .find(|quad| {
                        quad.background == gpui::solid_background(color)
                            && (quad.bounds.origin.x.0 / scale - f32::from(bounds.origin.x)).abs()
                                < 1.0
                            && (quad.bounds.origin.y.0 / scale - f32::from(bounds.origin.y)).abs()
                                < 1.0
                    })
                    .expect("picker row highlight");
                assert_eq!(quad.corner_smoothing, 2.5);
                assert!((quad.corner_radii.top_left.0 / scale - 12.8).abs() < 0.001);
            }
        });
    }

    #[gpui::test]
    fn picker_stages_model_and_effort_pills_until_dismissed_once(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, _| ModelPickerTest {
            provider: AgentProvider::Codex,
            applied: Vec::new(),
            catalogs: Vec::new(),
            loads: Vec::new(),
            scope: "local:/project".into(),
        });
        click(cx, "agent-model-trigger");
        click(cx, "model-b");
        click(cx, "agent-effort:pill-1");
        assert!(view.read_with(cx, |view, _| view.applied.is_empty()));
        assert!(cx.debug_bounds("agent-model-menu").is_some());
        cx.simulate_keystrokes("escape");
        draw(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.applied.len(), 1);
            assert_eq!(view.applied[0].model.as_deref(), Some("model-b"));
            assert_eq!(view.applied[0].effort.as_deref(), Some("high"));
        });
        assert!(cx.debug_bounds("agent-model-menu").is_none());
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(view.read_with(cx, |view, _| view.applied.len()), 1);
        click(cx, "agent-model-trigger");
        click(cx, "agent-effort:pill-1");
        cx.simulate_keystrokes("left");
        draw(cx);
        click(cx, "agent-model-trigger");
        assert_eq!(view.read_with(cx, |view, _| view.applied.len()), 1);
    }

    #[gpui::test]
    fn catalog_arrives_in_the_open_picker_without_switching_provider(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, _| ModelPickerTest {
            provider: AgentProvider::Codex,
            applied: Vec::new(),
            catalogs: Vec::new(),
            loads: Vec::new(),
            scope: "local:/project".into(),
        });
        click(cx, "agent-model-trigger");
        click(cx, "claude-code");
        assert_eq!(
            view.read_with(cx, |view, _| view.loads.clone()),
            vec![AgentProvider::ClaudeCode]
        );
        assert!(view.read_with(cx, |view, _| view.applied.is_empty()));
        view.update(cx, |view, cx| {
            view.catalogs.push(zz_protocol::agent_stream::AgentCatalogResult {
                catalog_provider: AgentProvider::ClaudeCode, cwd: std::path::PathBuf::default(), request_id: 1, error: None,
                config_options: Some(serde_json::json!([{
                    "id": "model", "name": "Model", "type": "select", "category": "model", "currentValue": "claude-a",
                    "options": [{"value": "claude-a", "name": "Claude A"}, {"value": "claude-b", "name": "Claude B"}]
                }])),
            });
            cx.notify();
        });
        click(cx, "claude-b");
        assert!(view.read_with(cx, |view, _| view.applied.is_empty()));
        assert_eq!(
            view.read_with(cx, |view, _| view.provider),
            AgentProvider::Codex
        );
        click(cx, "codex");
        click(cx, "claude-code");
        assert_eq!(view.read_with(cx, |view, _| view.loads.len()), 1);
        cx.simulate_keystrokes("escape");
        draw(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.applied.len(), 1);
            assert_eq!(view.applied[0].provider, AgentProvider::ClaudeCode);
            assert_eq!(view.applied[0].model.as_deref(), Some("claude-b"));
        });
    }

    #[gpui::test]
    fn vendor_tabs_use_cached_choices_without_applying_and_clear_on_scope_change(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, _| ModelPickerTest {
            provider: AgentProvider::Codex,
            applied: Vec::new(),
            catalogs: Vec::new(),
            loads: Vec::new(),
            scope: "local:/project".into(),
        });
        click(cx, "agent-model-trigger");
        let bounds = cx.debug_bounds("agent-model-menu").unwrap();
        click(cx, "claude-code");
        assert_eq!(cx.debug_bounds("agent-model-menu").unwrap(), bounds);
        assert!(cx.debug_bounds("model-a").is_none());
        assert!(view.read_with(cx, |view, _| view.applied.is_empty()));
        click(cx, "codex");
        assert!(cx.debug_bounds("model-a").is_some());
        click(cx, "agent-model-trigger");
        assert!(view.read_with(cx, |view, _| view.applied.is_empty()));
        click(cx, "agent-model-trigger");
        click(cx, "claude-code");
        click(cx, "agent-model-trigger");
        view.read_with(cx, |view, _| {
            assert_eq!(view.applied.len(), 1);
            assert_eq!(view.provider, AgentProvider::ClaudeCode);
            assert_eq!(view.applied[0].model, None);
        });
        click(cx, "agent-model-trigger");
        assert!(cx.debug_bounds("claude-a").is_some());
        click(cx, "codex");
        assert!(cx.debug_bounds("model-a").is_some());
        assert!(cx.debug_bounds("claude-a").is_none());
        assert_eq!(view.read_with(cx, |view, _| view.applied.len()), 1);
        click(cx, "model-b");
        cx.simulate_click(point(px(700.0), px(450.0)), Modifiers::default());
        draw(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.applied.len(), 2);
            assert_eq!(view.applied[1].model.as_deref(), Some("model-b"));
        });
        view.update(cx, |view, cx| {
            view.scope = "remote:/elsewhere".into();
            cx.notify();
        });
        click(cx, "agent-model-trigger");
        click(cx, "claude-code");
        assert!(cx.debug_bounds("claude-a").is_none());
    }
}
