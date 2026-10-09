use std::rc::Rc;

use zpui::{App, Div, SharedString, Window, div, prelude::*, px};
use zz_protocol::AgentTaskWire;

use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

use super::controls::agent_chrome_button;

const TASK_ROW_HEIGHT: f32 = 28.0;
const TASK_ROW_FONT_SIZE: f32 = 12.0;
const TASK_ROW_LINE_HEIGHT: f32 = 16.0;
const TASK_ICON_SIZE: f32 = 13.0;
const TASK_ICON_OPTICAL_DROP: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayPanel {
    Plan,
    Tasks,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskTrayAction {
    Toggle(TrayPanel),
    Stop(String),
    Reveal(String),
}

pub fn task_kind_icon(kind: &str) -> IconName {
    match kind {
        "shell" => IconName::Terminal,
        "agent" => IconName::Bot,
        "monitor" => IconName::Inspector,
        _ => IconName::Cpu,
    }
}

pub fn task_tray_label(count: usize) -> String {
    format!("{count} running")
}

pub fn plan_chip_label(source: &str) -> Option<(String, Option<String>)> {
    let (done, total, current) = super::plan_progress(source);
    (total > 0).then(|| (format!("Plan {done}/{total}"), current.map(str::to_owned)))
}

#[allow(clippy::too_many_arguments)]
pub fn task_tray(
    id: &str,
    plan: Option<&str>,
    tasks: &[AgentTaskWire],
    open: Option<TrayPanel>,
    enabled: bool,
    phase: f32,
    on_action: impl Fn(TaskTrayAction, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Option<Div> {
    let plan = plan.and_then(|source| plan_chip_label(source).map(|label| (source, label)));
    if tasks.is_empty() && plan.is_none() {
        return None;
    }
    let on_action: Rc<dyn Fn(TaskTrayAction, &mut Window, &mut App)> = Rc::new(on_action);
    let toggle = Rc::clone(&on_action);
    let foreground = cx.theme().foreground;
    let rows = tasks.iter().enumerate().map(|(index, task)| {
        let stop = Rc::clone(&on_action);
        let reveal = Rc::clone(&on_action);
        let task_id = task.id.clone();
        let tool_call = task.tool_call_id.clone();
        h_flex()
            .id((SharedString::from(format!("{id}-task")), index))
            .w_full()
            .h(px(TASK_ROW_HEIGHT))
            .flex_none()
            .gap_2()
            .px_2()
            .rounded(cx.theme().menu_radius())
            .overflow_hidden()
            .text_size(crate::rems_from_px(TASK_ROW_FONT_SIZE))
            .line_height(px(TASK_ROW_LINE_HEIGHT))
            .text_color(cx.theme().foreground.muted())
            .when_some(tool_call, |row, tool_call| {
                row.cursor_pointer()
                    .hover(move |row| row.text_color(foreground))
                    .on_click(move |_, window, cx| {
                        reveal(TaskTrayAction::Reveal(tool_call.clone()), window, cx);
                        cx.stop_propagation();
                    })
            })
            .child(
                div()
                    .flex_none()
                    .relative()
                    .top(px(TASK_ICON_OPTICAL_DROP))
                    .child(Icon::new(task_kind_icon(&task.kind)).size(px(TASK_ICON_SIZE))),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(task.description.clone()),
            )
            .child(
                Button::new((SharedString::from(format!("{id}-task-stop")), index))
                    .debug_selector(move || format!("agent-task-stop-{index}"))
                    .ghost()
                    .xsmall()
                    .label("Stop")
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        stop(TaskTrayAction::Stop(task_id.clone()), window, cx);
                        cx.stop_propagation();
                    }),
            )
    });
    let plan_toggle = Rc::clone(&on_action);
    let panel = match open {
        Some(TrayPanel::Tasks) if !tasks.is_empty() => Some(v_flex().children(rows)),
        Some(TrayPanel::Plan) => plan.as_ref().map(|(source, _)| {
            v_flex()
                .px_2()
                .py_1()
                .child(super::render_plan_items(source, cx))
        }),
        _ => None,
    };
    let plan_chip = plan.map(|(_, (label, current))| {
        let open = open == Some(TrayPanel::Plan);
        h_flex()
            .id(SharedString::from(format!("{id}-plan")))
            .debug_selector(|| "agent-plan-chip".to_owned())
            .min_w_0()
            .h(px(24.0))
            .px_2()
            .gap_1p5()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .text_size(crate::rems_from_px(TASK_ROW_FONT_SIZE))
            .line_height(px(TASK_ROW_LINE_HEIGHT))
            .text_color(cx.theme().foreground)
            .hover(|chip| chip.bg(cx.theme().background.washed(2)))
            .when(open, |chip| chip.bg(cx.theme().background.washed(2)))
            .child(
                div()
                    .flex_none()
                    .relative()
                    .top(px(TASK_ICON_OPTICAL_DROP))
                    .child(Icon::new(IconName::CircleCheck).size(px(TASK_ROW_FONT_SIZE))),
            )
            .child(div().flex_none().child(label))
            .when_some(current, |chip, current| {
                chip.child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_color(cx.theme().foreground.muted())
                        .child(current),
                )
            })
            .child(
                div()
                    .flex_none()
                    .relative()
                    .top(px(TASK_ICON_OPTICAL_DROP))
                    .text_color(cx.theme().foreground.muted())
                    .child(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronUp
                        })
                        .size(px(TASK_ROW_FONT_SIZE)),
                    ),
            )
            .on_click(move |_, window, cx| {
                plan_toggle(TaskTrayAction::Toggle(TrayPanel::Plan), window, cx);
                cx.stop_propagation();
            })
    });
    let tasks_open = open == Some(TrayPanel::Tasks);
    let tasks_chip = (!tasks.is_empty()).then(|| {
        agent_chrome_button(SharedString::from(format!("{id}-tasks")))
            .debug_selector(|| "agent-task-tray".to_owned())
            .flex_none()
            .icon(super::presentation::spinner(phase))
            .label(task_tray_label(tasks.len()))
            .tooltip(if tasks_open {
                "Hide background tasks"
            } else {
                "Show background tasks"
            })
            .text_color(cx.theme().foreground.muted())
            .on_click(move |_, window, cx| {
                toggle(TaskTrayAction::Toggle(TrayPanel::Tasks), window, cx);
                cx.stop_propagation();
            })
    });
    Some(
        v_flex()
            .w_full()
            .gap_1()
            .when_some(panel, |tray, panel| {
                tray.child(
                    panel
                        .w_full()
                        .p_1()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border())
                        .bg(cx.theme().background.raised(1).opaque()),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_between()
                    .child(div().min_w_0().children(plan_chip))
                    .children(tasks_chip),
            ),
    )
}
