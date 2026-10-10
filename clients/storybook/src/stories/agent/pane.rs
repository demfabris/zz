use std::{rc::Rc, sync::Arc};

use zz_gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, ListAlignment, ListState,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div, prelude::*,
    px,
};
use zz_protocol::{AgentProvider, PaneId};
use zz_ui::{
    CHROME_GAP, IconName,
    agent::{
        AgentPaneStatus, AgentTimeline, AgentTimelineStore, AgentToolKind as Kind,
        AgentToolStatus as Status, TimelineRow, agent_pane_header, agent_status_pill,
        composer::COMPOSER_OUTER_PADDING,
        controls::agent_provider_label,
        fold_timeline_rows,
        presentation::spinner_phase,
        tasks::{TaskTrayAction, TrayPanel, task_tray},
        title::agent_thread_title_editor,
    },
    h_flex,
    input::InputState,
    pane::{pane_drag_button, pane_header_icon_button},
    scroll::Scrollbar,
};

use super::{
    composer::{Draft, composer, composer_input},
    fixtures::{
        PLAN, TEST_FAILURE, assistant, done, plan, tab_strip_screenshot, tasks, thought, tool,
        user, user_with_images,
    },
    timeline::pane_frame,
};

const TITLE: &str = "Pinned tabs keep their icon width";

const FIRST_ANSWER: &str = "Pinned tabs now keep a **28px floor**, and only the unpinned tabs \
share what is left of the strip:\n\n\
```rust\n\
let floor = px(28.0) * pinned;\n\
let share = (width - floor).max(px(0.0)) / unpinned.max(1.0);\n\
```\n\n\
The new `thirty_tabs_three_pinned` test covers it.";

pub struct FullPane {
    store: Entity<AgentTimelineStore>,
    rows: Arc<Vec<TimelineRow>>,
    list: ListState,
    input: Entity<InputState>,
    tray: Option<TrayPanel>,
    _observe: Subscription,
}

pub fn full_pane(window: &mut Window, cx: &mut App) -> AnyView {
    let input = composer_input("", window, cx);
    cx.new(|cx| {
        let rows = fold_timeline_rows(&[
            user_with_images(
                1,
                "Pinned tabs shrink to nothing when I open thirty tabs. Can you make them keep their icon width?",
                vec![tab_strip_screenshot()],
            ),
            thought(2, "The tab strip hands every tab the same share of the width, so pinned tabs shrink with the rest."),
            done(3, Kind::Search, "rg -n \"pinned\" crates/zz-ui/src/browser"),
            done(4, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
            done(5, Kind::Edit, "Edit crates/zz-ui/src/browser/tab_strip.rs"),
            tool(6, Kind::Execute, Status::Completed, "cargo test -p zz-ui browser::tab_strip")
                .terminal("running 6 tests\ntest result: ok. 6 passed; 0 failed")
                .entry(),
            assistant(7, FIRST_ANSWER),
            user(8, "Nice. Run the whole browser suite and fix whatever breaks."),
            plan(14, PLAN),
            tool(9, Kind::Execute, Status::Failed, "cargo test -p zz-ui browser")
                .terminal(TEST_FAILURE)
                .exit(101)
                .entry(),
            thought(10, "Two failures, both in drag. The drop preview still measures each tab itself instead of reading the widths the layout pass stored, so it lands one slot off once pinned tabs stop shrinking."),
            done(11, Kind::Read, "Read crates/zz-ui/src/browser/drag.rs"),
            done(12, Kind::Edit, "Edit crates/zz-ui/src/browser/drag.rs"),
            tool(13, Kind::Execute, Status::Running, "cargo test -p zz-ui browser::drag").entry(),
        ])
        .rows;
        let store = cx.new(|_| AgentTimelineStore::default());
        let observe = cx.observe(&store, |_, _, cx| cx.notify());
        FullPane {
            list: ListState::new(rows.len(), ListAlignment::Bottom, px(1200.0)),
            rows,
            store,
            input,
            tray: None,
            _observe: observe,
        }
    })
    .into()
}

impl Render for FullPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity_id();
        let phase = spinner_phase(view, cx);
        let entity = cx.entity();
        let tasks = tasks().into_iter().take(1).collect::<Vec<_>>();
        let tray = task_tray(
            "full-pane-tray",
            Some(PLAN),
            &tasks,
            self.tray,
            true,
            phase,
            move |action, _, cx| {
                if let TaskTrayAction::Toggle(panel) = action {
                    entity.update(cx, |view, cx| {
                        view.tray = (view.tray != Some(panel)).then_some(panel);
                        cx.notify();
                    });
                }
            },
            cx,
        );
        let leading = h_flex()
            .min_w_0()
            .gap(px(CHROME_GAP))
            .child(agent_provider_label(AgentProvider::ClaudeCode, cx))
            .child(agent_thread_title_editor(
                "full-pane-title",
                TITLE,
                true,
                |_, _| {},
                window,
                cx,
            ));
        let trailing = h_flex()
            .gap(px(CHROME_GAP))
            .child(
                pane_header_icon_button("full-pane-split-bottom", IconName::PanelBottom, true, cx)
                    .tooltip("Split bottom"),
            )
            .child(
                pane_header_icon_button("full-pane-split-right", IconName::PanelRight, true, cx)
                    .tooltip("Split right"),
            )
            .child(pane_drag_button(
                "full-pane-drag",
                PaneId(1),
                TITLE.to_owned(),
                true,
                |_, _, _| {},
                cx,
            ))
            .child(
                pane_header_icon_button("full-pane-close", IconName::Xmark, true, cx)
                    .tooltip("Close pane"),
            );
        let status = agent_status_pill(
            "full-pane-status",
            AgentPaneStatus::Running,
            phase,
            None::<Rc<dyn Fn(&mut Window, &mut App)>>,
            cx,
        )
        .into_any_element();
        let timeline = AgentTimeline::new(self.rows.clone(), self.list.clone(), self.store.clone())
            .active_turn(true)
            .bottom_padding(COMPOSER_OUTER_PADDING)
            .open_output(|_, _, _| {});
        let composer = composer(
            Draft {
                id: SharedString::from("full-pane-composer"),
                input: self.input.clone(),
                running: true,
                writable: true,
                images: Vec::new(),
                prefix: tray
                    .map(IntoElement::into_any_element)
                    .into_iter()
                    .collect(),
            },
            window,
            cx,
        );
        pane_frame(cx)
            .h(px(720.0))
            .flex()
            .flex_col()
            .child(agent_pane_header(
                true,
                leading,
                Some(status),
                trailing,
                true,
                cx,
            ))
            .child(
                div()
                    .id("full-pane-timeline")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(timeline)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom(px(COMPOSER_OUTER_PADDING))
                            .child(Scrollbar::vertical(&self.list)),
                    ),
            )
            .child(composer)
    }
}
