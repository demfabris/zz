use std::{rc::Rc, sync::Arc};

use zpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, IntoElement, ListAlignment,
    ListState, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
    prelude::*, px,
};
use zz_protocol::{AgentProvider, PaneId};
use zz_ui::{
    CHROME_GAP, Disableable as _, IconName, Sizable as _,
    agent::{
        AgentPaneStatus, AgentTimeline, AgentTimelineStore, AgentToolKind as Kind, TimelineRow,
        agent_jump_to_bottom_button, agent_pane_header, agent_status_pill,
        composer::COMPOSER_OUTER_PADDING,
        controls::agent_provider_label,
        fold_timeline_rows,
        presentation::{empty_state, error_card, spinner_phase, welcome_state},
        title::agent_thread_title_editor,
    },
    button::{Button, ButtonVariants as _},
    h_flex,
    pane::{pane_drag_button, pane_header_icon_button},
    scroll::Scrollbar,
};

use super::{
    fixtures::{assistant, done, thought, user},
    timeline::pane_frame,
};
use crate::story::{row, states};

struct HeaderCase {
    label: &'static str,
    provider: AgentProvider,
    title: &'static str,
    status: Option<AgentPaneStatus>,
    restart: bool,
    active: bool,
    enabled: bool,
    separator: bool,
    width: Option<f32>,
}

const HEADER: HeaderCase = HeaderCase {
    label: "",
    provider: AgentProvider::Codex,
    title: "Fix the flaky tab drag test",
    status: None,
    restart: false,
    active: true,
    enabled: true,
    separator: true,
    width: None,
};

const HEADERS: [HeaderCase; 9] = [
    HeaderCase {
        label: "active, no status",
        ..HEADER
    },
    HeaderCase {
        label: "running",
        provider: AgentProvider::ClaudeCode,
        status: Some(AgentPaneStatus::Running),
        ..HEADER
    },
    HeaderCase {
        label: "waiting for you",
        status: Some(AgentPaneStatus::Waiting),
        ..HEADER
    },
    HeaderCase {
        label: "exited, with Restart",
        provider: AgentProvider::ClaudeCode,
        status: Some(AgentPaneStatus::Exited),
        restart: true,
        ..HEADER
    },
    HeaderCase {
        label: "offline, read-only",
        status: Some(AgentPaneStatus::Offline),
        enabled: false,
        ..HEADER
    },
    HeaderCase {
        label: "inactive pane, actions show on hover",
        active: false,
        ..HEADER
    },
    HeaderCase {
        label: "new session, no transcript, no rule",
        title: "",
        separator: false,
        ..HEADER
    },
    HeaderCase {
        label: "long title",
        title: "Investigate why the browser pane drops frames on the secondary display at 120 Hz",
        ..HEADER
    },
    HeaderCase {
        label: "narrow pane, running",
        provider: AgentProvider::ClaudeCode,
        title: "Investigate why the browser pane drops frames on the secondary display at 120 Hz",
        status: Some(AgentPaneStatus::Running),
        width: Some(380.0),
        ..HEADER
    },
];

pub struct Headers;

pub fn headers(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|_| Headers).into()
}

impl Render for Headers {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity_id();
        let phase = spinner_phase(view, cx);
        HEADERS
            .iter()
            .enumerate()
            .fold(states(), |states, (index, case)| {
                states.state(case.label, header(index, case, phase, window, cx))
            })
    }
}

fn header(
    index: usize,
    case: &HeaderCase,
    phase: f32,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let leading = h_flex()
        .min_w_0()
        .gap(px(CHROME_GAP))
        .child(agent_provider_label(case.provider, cx))
        .child(agent_thread_title_editor(
            ("header-title", index),
            case.title,
            case.enabled,
            |_, _| {},
            window,
            cx,
        ));
    let trailing = h_flex()
        .gap(px(CHROME_GAP))
        .child(
            pane_header_icon_button(
                ("header-split-bottom", index),
                IconName::PanelBottom,
                case.enabled,
                cx,
            )
            .tooltip("Split bottom"),
        )
        .child(
            pane_header_icon_button(
                ("header-split-right", index),
                IconName::PanelRight,
                case.enabled,
                cx,
            )
            .tooltip("Split right"),
        )
        .child(pane_drag_button(
            ("header-drag", index),
            PaneId(index as u64 + 1),
            case.title.to_owned(),
            case.enabled,
            |_, _, _| {},
            cx,
        ))
        .child(
            pane_header_icon_button(("header-close", index), IconName::Xmark, case.enabled, cx)
                .tooltip("Close pane"),
        );
    let status = case.status.map(|status| {
        let restart = case.restart.then(|| {
            Rc::new(|_: &mut Window, _: &mut App| {}) as Rc<dyn Fn(&mut Window, &mut App)>
        });
        let phase = if status == AgentPaneStatus::Running {
            phase
        } else {
            0.0
        };
        agent_status_pill(("header-status", index), status, phase, restart, cx).into_any_element()
    });
    pane_frame(cx)
        .id(("header-case", index))
        .when_some(case.width, |frame, width| frame.w(px(width)))
        .child(agent_pane_header(
            case.active,
            leading,
            status,
            trailing,
            case.separator,
            cx,
        ))
        .child(div().h(px(20.0)))
        .into_any_element()
}

pub struct StatusPills;

pub fn status_pills(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|_| StatusPills).into()
}

impl Render for StatusPills {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity_id();
        let phase = spinner_phase(view, cx);
        let restart =
            || Some(Rc::new(|_: &mut Window, _: &mut App| {}) as Rc<dyn Fn(&mut Window, &mut App)>);
        states()
            .columns(3)
            .state(
                "running",
                row().child(agent_status_pill(
                    "pill-running",
                    AgentPaneStatus::Running,
                    phase,
                    None,
                    cx,
                )),
            )
            .state(
                "stopping",
                row().child(agent_status_pill(
                    "pill-stopping",
                    AgentPaneStatus::Stopping,
                    phase,
                    None,
                    cx,
                )),
            )
            .state(
                "waiting for you",
                row().child(agent_status_pill(
                    "pill-waiting",
                    AgentPaneStatus::Waiting,
                    0.0,
                    None,
                    cx,
                )),
            )
            .state(
                "exited",
                row().child(agent_status_pill(
                    "pill-exited",
                    AgentPaneStatus::Exited,
                    0.0,
                    None,
                    cx,
                )),
            )
            .state(
                "exited, with Restart",
                row().child(agent_status_pill(
                    "pill-restart",
                    AgentPaneStatus::Exited,
                    0.0,
                    restart(),
                    cx,
                )),
            )
            .state(
                "offline",
                row().child(agent_status_pill(
                    "pill-offline",
                    AgentPaneStatus::Offline,
                    0.0,
                    None,
                    cx,
                )),
            )
    }
}

pub struct Jump {
    store: Entity<AgentTimelineStore>,
    rows: Arc<Vec<TimelineRow>>,
    list: ListState,
    _observe: Subscription,
}

pub fn jump(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        let rows = fold_timeline_rows(&[
            user(1, "Why do pinned tabs shrink when I open thirty tabs?"),
            thought(2, "The tab strip divides the width evenly."),
            done(3, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
            assistant(4, "Every tab gets the same share of the width, pinned or not. With thirty tabs that share drops below the icon, so pinned tabs collapse first because they have no title to keep them wide.\n\nThe fix is a floor at the icon width for pinned tabs, with the rest of the width divided among the unpinned ones."),
            user(5, "Do it, and keep the drop preview in sync."),
            done(6, Kind::Edit, "Edit crates/zz-ui/src/browser/tab_strip.rs"),
            done(7, Kind::Edit, "Edit crates/zz-ui/src/browser/drag.rs"),
            done(8, Kind::Execute, "cargo test -p zz-ui browser"),
            assistant(9, "Done. Pinned tabs keep a 28px floor and the drop preview reads the same widths."),
        ])
        .rows;
        let store = cx.new(|_| AgentTimelineStore::default());
        let observe = cx.observe(&store, |_, _, cx| cx.notify());
        Jump {
            list: ListState::new(rows.len(), ListAlignment::Top, px(1200.0)),
            rows,
            store,
            _observe: observe,
        }
    })
    .into()
}

impl Render for Jump {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let timeline = AgentTimeline::new(self.rows.clone(), self.list.clone(), self.store.clone())
            .bottom_padding(COMPOSER_OUTER_PADDING);
        states()
            .state(
                "button",
                row().child(agent_jump_to_bottom_button("jump-alone", cx)),
            )
            .state(
                "over a timeline scrolled up",
                pane_frame(cx)
                    .h(px(300.0))
                    .child(timeline)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom(px(COMPOSER_OUTER_PADDING))
                            .child(Scrollbar::vertical(&self.list)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(px(2.0 * COMPOSER_OUTER_PADDING))
                            .flex()
                            .justify_center()
                            .child(agent_jump_to_bottom_button("jump-over-timeline", cx)),
                    ),
            )
    }
}

pub struct EmptyStates;

pub fn empty_states(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|_| EmptyStates).into()
}

impl Render for EmptyStates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity_id();
        let empty = |message: &str, busy: bool, cx: &mut App| -> AnyElement {
            pane_frame(cx)
                .child(empty_state(
                    SharedString::from(message.to_owned()),
                    busy,
                    view,
                    cx,
                ))
                .into_any_element()
        };
        let messages = [
            ("starting", "Starting Codex…", true),
            ("restoring", "Restoring the previous session…", true),
            (
                "running, nothing streamed yet",
                "Waiting for Claude Code’s first update…",
                true,
            ),
            ("cancelling", "Cancelling the current turn…", true),
            (
                "failed to start",
                "The agent could not start this session.",
                false,
            ),
            ("offline", "The agent is offline.", false),
        ];
        let grid = messages
            .iter()
            .fold(states().columns(2), |states, (label, message, busy)| {
                states.state(*label, empty(message, *busy, cx))
            });
        states()
            .state(
                "welcome, ready with no transcript",
                pane_frame(cx).h(px(340.0)).child(welcome_state(cx)),
            )
            .state("waiting and failure messages", grid)
            .state(
                "error card, failed to start, with Try again and sign-in",
                error_card(
                    "Claude Code exited before the session started: authentication required (401). Sign in and try again.",
                    cx,
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("error-retry")
                                .primary()
                                .small()
                                .icon(IconName::Redo2)
                                .label("Try again"),
                        )
                        .child(
                            Button::new("error-auth")
                                .secondary()
                                .small()
                                .label("Log in with Claude")
                                .tooltip("Open the browser to sign in"),
                        ),
                ),
            )
            .state(
                "error card, restarting",
                error_card("Codex exited with status 1.", cx).child(
                    h_flex().flex_wrap().gap_2().child(
                        Button::new("error-restarting")
                            .primary()
                            .small()
                            .icon(IconName::Redo2)
                            .label("Restarting…")
                            .disabled(true),
                    ),
                ),
            )
            .state(
                "error card, draft error",
                error_card("Timed out applying agent settings.", cx),
            )
            .state(
                "error card, long error",
                error_card(&"thread 'tokio-runtime-worker' panicked at crates/zz-daemon/src/agent/host.rs:2873:13: the adapter closed stdout before answering session/new\nstack backtrace:\n   0: rust_begin_unwind\n   1: core::panicking::panic_fmt\n".repeat(3), cx),
            )
    }
}
