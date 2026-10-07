use std::rc::Rc;

use gpui::{App, Div, SharedString, Window, div, prelude::*, px};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskTrayAction {
    Toggle,
    Stop(String),
    /// Show the tool row that started the task, by its tool call ID.
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

/// The agent's background work: a chip that counts it and, opened, one row
/// per task with its Stop button.
pub fn task_tray(
    id: &str,
    tasks: &[AgentTaskWire],
    expanded: bool,
    enabled: bool,
    on_action: Rc<dyn Fn(TaskTrayAction, &mut Window, &mut App)>,
    cx: &App,
) -> Option<Div> {
    if tasks.is_empty() {
        return None;
    }
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
    Some(
        v_flex()
            .w_full()
            .gap_1()
            .when(expanded, |tray| {
                tray.child(
                    v_flex()
                        .w_full()
                        .p_1()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border())
                        .bg(cx.theme().background.raised(1).opaque())
                        .children(rows),
                )
            })
            .child(
                h_flex().w_full().justify_end().child(
                    agent_chrome_button(SharedString::from(format!("{id}-tasks")))
                        .debug_selector(|| "agent-task-tray".to_owned())
                        .icon(IconName::Loader)
                        .label(task_tray_label(tasks.len()))
                        .tooltip(if expanded {
                            "Hide background tasks"
                        } else {
                            "Show background tasks"
                        })
                        .text_color(cx.theme().foreground.muted())
                        .on_click(move |_, window, cx| {
                            toggle(TaskTrayAction::Toggle, window, cx);
                            cx.stop_propagation();
                        }),
                ),
            ),
    )
}
