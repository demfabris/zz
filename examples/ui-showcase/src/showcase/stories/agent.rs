//! The agent-pane pieces: the pane header and the timeline rows.

use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, ParentElement as _, SharedString, Styled as _, div, prelude::*, px,
};
use zz_ui::agent::{
    AgentEntry, AgentMarkdown, AgentTimeline, AgentToolEntry, AgentToolKind, AgentToolPayload,
    AgentToolStatus, agent_pane_header,
};
use zz_ui::{ActiveTheme as _, Icon, IconName, Sizable as _};

use super::{Showcase, gallery, specimen_block, specimens, story_stack};

const TIMELINE_HEIGHT: f32 = 420.0;

pub(super) fn render(showcase: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Pane header",
                "The 40px header strip: two app-supplied slots pinned to the ends, nothing in between. The strip decides only its height, its inset, and the rule beneath it.",
                cx,
            )
            .child(specimens().w_full().child(specimen_block(
                "leading · trailing",
                header("codex · zz", "zz", cx),
                cx,
            ))),
        )
        .child(
            gallery(
                "Thread entries",
                "A turn as the timeline lays it out: the user prompt, the assistant's markdown reply, a collapsed reasoning disclosure, and the plan. Click a disclosure to expand it.",
                cx,
            )
            .child(specimens().w_full().child(specimen_block(
                "user · assistant · reasoning · plan",
                timeline(showcase, ThreadFixture::Conversation, cx),
                cx,
            ))),
        )
        .child(
            gallery(
                "Tool calls",
                "One row per tool call, with the kind glyph and a status the row derives its treatment from. Consecutive tool calls fold into a single group row that reports the aggregate status.",
                cx,
            )
            .child(specimens().w_full().child(specimen_block(
                "running · needs approval · failed · folded group",
                timeline(showcase, ThreadFixture::Tools, cx),
                cx,
            ))),
        )
        .child(
            gallery(
                "Tool payloads",
                "The three payload shapes a completed tool can carry. Output past ten rows scrolls inside the row rather than growing it.",
                cx,
            )
            .child(specimens().w_full().child(specimen_block(
                "diff · text · json",
                timeline(showcase, ThreadFixture::Payloads, cx),
                cx,
            ))),
        )
        .child(
            gallery(
                "Subagents & task notifications",
                "A tool call that spawned its own agent expands into a nested timeline of that agent's entries, ruled off against the parent thread. Background tasks report back as notification rows, which read as a status and a line until their result is disclosed.",
                cx,
            )
            .child(specimens().w_full().child(specimen_block(
                "nested thread · running · completed · failed",
                timeline(showcase, ThreadFixture::Delegated, cx),
                cx,
            ))),
        )
        .into_any_element()
}

fn header(title: &'static str, directory: &'static str, cx: &App) -> impl IntoElement {
    let slot = |icon: IconName, label: &'static str| {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(Icon::new(icon).xsmall())
            .child(div().text_sm().child(label))
    };
    agent_pane_header(
        slot(IconName::Openai, title),
        slot(IconName::FolderOpen, directory),
        cx,
    )
}

fn timeline(showcase: &Showcase, fixture: ThreadFixture, cx: &App) -> AnyElement {
    div()
        .w_full()
        .h(px(TIMELINE_HEIGHT))
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(AgentTimeline::new(
            showcase.agent_rows(fixture),
            showcase.agent_list_state(fixture),
            showcase.agent_timeline_store.clone(),
        ))
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThreadFixture {
    Conversation,
    Tools,
    Payloads,
    Delegated,
}

impl ThreadFixture {
    pub(crate) const ALL: [Self; 4] = [
        Self::Conversation,
        Self::Tools,
        Self::Payloads,
        Self::Delegated,
    ];

    pub(crate) fn entries(self) -> Vec<AgentEntry> {
        match self {
            Self::Conversation => conversation_entries(),
            Self::Tools => tool_entries(),
            Self::Payloads => payload_entries(),
            Self::Delegated => delegated_entries(),
        }
    }
}

fn conversation_entries() -> Vec<AgentEntry> {
    vec![
        AgentEntry::User {
            id: 100,
            markdown: "Why is the browser pane dropping frames when I scroll?".into(),
            images: Arc::from([]),
        },
        AgentEntry::Reasoning {
            id: 101,
            label: "Thinking".into(),
            markdown: "The pane composites through CEF's OSR path, so a dropped frame is \
                       either the begin-frame pump or the GPUI upload. The logs will say \
                       which."
                .into(),
            default_expanded: false,
        },
        AgentEntry::Assistant {
            id: 102,
            markdown: "The stalls come from the **begin-frame pump**, not the upload.\n\n\
                       Two things are happening:\n\n\
                       1. `browser_element` requests a frame per GPUI tick\n\
                       2. CEF answers on its own cadence, so the two drift\n\n\
                       The adaptive throttle in `browser_controller.rs` is opt-in:\n\n\
                       ```toml\n\
                       adaptive-begin-frame = true\n\
                       ```\n\n\
                       Turn it on and the pump follows the compositor instead."
                .into(),
        },
        AgentEntry::Plan {
            id: 103,
            markdown: "- [x] Read the frame log\n\
                       - [x] Confirm the pump is the stall\n\
                       - [ ] Enable the adaptive throttle\n\
                       - [ ] Re-measure at 120hz"
                .into(),
        },
    ]
}

fn tool_entries() -> Vec<AgentEntry> {
    vec![
        AgentEntry::Tool(AgentToolEntry {
                id: 200,
                kind: AgentToolKind::Execute,
                status: AgentToolStatus::Running,
                label: "cargo check -p zz".into(),
                location: None,
                input: None,
                output: Arc::from([]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
        AgentEntry::Tool(AgentToolEntry {
                id: 201,
                kind: AgentToolKind::Edit,
                status: AgentToolStatus::NeedsApproval,
                label: "Write crates/zz/src/browser_controller.rs".into(),
                location: Some("crates/zz/src/browser_controller.rs".into()),
                input: None,
                output: Arc::from([]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
        AgentEntry::Tool(AgentToolEntry {
                id: 202,
                kind: AgentToolKind::Fetch,
                status: AgentToolStatus::Failed,
                label: "GET https://gpui.rs/docs".into(),
                location: None,
                input: None,
                output: Arc::from([AgentToolPayload::Text(
                    "error: connection refused (os error 61)".into(),
                )]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
        AgentEntry::Tool(AgentToolEntry {
                id: 203,
                kind: AgentToolKind::Read,
                status: AgentToolStatus::Completed,
                label: "Read crates/zz-ui/src/pane.rs".into(),
                location: Some("crates/zz-ui/src/pane.rs".into()),
                input: None,
                output: Arc::from([AgentToolPayload::Text("357 lines".into())]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
        AgentEntry::Tool(AgentToolEntry {
                id: 204,
                kind: AgentToolKind::Read,
                status: AgentToolStatus::Completed,
                label: "Read crates/zz-ui/src/browser.rs".into(),
                location: Some("crates/zz-ui/src/browser.rs".into()),
                input: None,
                output: Arc::from([AgentToolPayload::Text("742 lines".into())]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
        AgentEntry::Tool(AgentToolEntry {
                id: 205,
                kind: AgentToolKind::Search,
                status: AgentToolStatus::Completed,
                label: "Grep begin_frame".into(),
                location: None,
                input: None,
                output: Arc::from([AgentToolPayload::Text("11 matches in 4 files".into())]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
        AgentEntry::Tool(AgentToolEntry {
                id: 206,
                kind: AgentToolKind::Execute,
                status: AgentToolStatus::Completed,
                label: "sed -n '1,120p' crates/zz-ui/src/agent.rs\nsed -n '1,80p' crates/zz/src/agent_view.rs"
                    .into(),
                location: None,
                input: None,
                output: Arc::from([AgentToolPayload::Text("200 lines".into())]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
    ]
}

fn delegated_entries() -> Vec<AgentEntry> {
    vec![
        AgentEntry::Tool(AgentToolEntry {
            id: 400,
            kind: AgentToolKind::Other,
            status: AgentToolStatus::Completed,
            label: "Task: audit the pane overlays".into(),
            location: None,
            input: Some(AgentToolPayload::Text(
                "List every overlay that can occupy a pane corner and say which entity owns it."
                    .into(),
            )),
            output: Arc::from([AgentToolPayload::Text(
                "4 overlays, 2 owners: PaneView and TerminalView".into(),
            )]),
            default_expanded: true,
            subagent: true,
            children: Arc::from([
                AgentEntry::Reasoning {
                    id: 401,
                    label: "Thinking".into(),
                    markdown: "Corner overlays are stacked by `pane_overlay_stack`, so the owners \
                               are whoever pushes into it."
                        .into(),
                    default_expanded: false,
                },
                AgentEntry::Tool(AgentToolEntry {
                    id: 402,
                    kind: AgentToolKind::Search,
                    status: AgentToolStatus::Completed,
                    label: "Grep pane_overlay_stack".into(),
                    location: None,
                    input: None,
                    output: Arc::from([AgentToolPayload::Text("6 matches in 3 files".into())]),
                    default_expanded: false,
                    subagent: false,
                    children: Arc::from([]),
                }),
                AgentEntry::Assistant {
                    id: 403,
                    markdown: "Top-right holds the **sync**, **zoom**, and **mode** tags; \
                               bottom-right holds **search**, **status**, and **link**."
                        .into(),
                },
            ]),
        }),
        AgentEntry::Notification {
            id: 410,
            task_id: "task-7".into(),
            status: "running".into(),
            summary: "Re-measuring the browser pane at 120hz".into(),
            result_markdown: AgentMarkdown::default(),
        },
        AgentEntry::Notification {
            id: 411,
            task_id: "task-6".into(),
            status: "completed".into(),
            summary: "Frame log collected".into(),
            result_markdown: "Captured 4,812 frames over 40s.\n\n\
                              - p50 **8.1ms**\n\
                              - p99 **31.4ms**, all inside the begin-frame pump"
                .into(),
        },
        AgentEntry::Notification {
            id: 412,
            task_id: "task-5".into(),
            status: "failed".into(),
            summary: SharedString::default(),
            result_markdown: "`cargo bench -p zz-terminal` exited 101: the bench harness needs a \
                              tty."
                .into(),
        },
    ]
}

fn payload_entries() -> Vec<AgentEntry> {
    vec![
        AgentEntry::Tool(AgentToolEntry {
                id: 300,
                kind: AgentToolKind::Edit,
                status: AgentToolStatus::Completed,
                label: "Edit examples/config".into(),
                location: Some("examples/config".into()),
                input: None,
                output: Arc::from([AgentToolPayload::Diff {
                    path: "examples/config".into(),
                    old: Some(
                        "pane-corner-radius = 8\n\
                         pane-margin = 4\n\
                         show-fps = false\n"
                            .into(),
                    ),
                    new: "pane-corner-radius = 8\n\
                          pane-margin = 6\n\
                          show-fps = false\n\
                          adaptive-begin-frame = true\n"
                        .into(),
                }]),
                default_expanded: true,
                subagent: false,
                children: Arc::from([]),
        }),
        AgentEntry::Tool(AgentToolEntry {
                id: 301,
                kind: AgentToolKind::Execute,
                status: AgentToolStatus::Completed,
                label: "cargo test -p zz-ui --lib".into(),
                location: None,
                input: None,
                output: Arc::from([AgentToolPayload::Text(
                    "running 121 tests\n\
                     test widget::kbd::tests::formats_macos_glyphs ... ok\n\
                     test widget::select::state::tests::initial_index_seeds_the_committed_value ... ok\n\
                     \n\
                     test result: ok. 121 passed; 0 failed; 0 ignored"
                        .into(),
                )]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
        AgentEntry::Tool(AgentToolEntry {
                id: 302,
                kind: AgentToolKind::Think,
                status: AgentToolStatus::Completed,
                label: "Inspect pane geometry".into(),
                location: None,
                input: None,
                output: Arc::from([AgentToolPayload::Json(
                    "{\n  \"pane\": 3,\n  \"zoomed\": false,\n  \"viewport\": {\n    \"cols\": 120,\n    \"rows\": 34\n  }\n}"
                        .into(),
                )]),
                default_expanded: false,
                subagent: false,
                children: Arc::from([]),
        }),
    ]
}
