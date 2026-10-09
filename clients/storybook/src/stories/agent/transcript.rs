use zpui::{AnyView, App, AppContext as _, Window};
use zz_ui::agent::{AgentToolKind as Kind, AgentToolStatus as Status};

use super::{
    fixtures::{
        TEST_FAILURE, aside, assistant, chart_screenshot, done, rewindable, tab_strip_screenshot,
        terminal_screenshot, thought, tool, user, user_with_images, wide_screenshot,
    },
    timeline::{Case, Cases},
};

const THINKING: &str = "The tab strip hands every tab the same share of the width, so pinned tabs \
shrink with the rest. They need a floor at the icon width, and the remaining width has to be \
divided among the unpinned tabs only. The drag code reads the same widths, so it will need the \
same change or the drop preview will land between the wrong tabs.";

const RICH_ANSWER: &str = "## What changed\n\n\
Pinned tabs now keep a **28px floor**, and only unpinned tabs share what is left.\n\n\
1. `TabStrip::layout` splits the width in two passes\n\
2. The drag code reads the same widths, so the drop preview stays under the pointer\n\
3. A new test covers thirty tabs with three pinned\n\n\
| Tabs | Pinned width | Unpinned width |\n\
| ---: | ---: | ---: |\n\
| 10 | 28px | 96px |\n\
| 30 | 28px | 31px |\n\n\
> The old behavior is still reachable with `browser.pinned-floor = 0`.\n\n\
See the [tab strip notes](https://zzmux.sh/docs/browser) for the full layout rules.";

const CODE_ANSWER: &str = "The floor goes in the first pass:\n\n\
```rust\n\
let pinned = tabs.iter().filter(|tab| tab.pinned).count() as f32;\n\
let floor = px(28.0) * pinned;\n\
let share = (width - floor).max(px(0.0)) / (tabs.len() as f32 - pinned).max(1.0);\n\
```\n\n\
Run it with:\n\n\
```sh\n\
cargo test -p zz-ui browser::tab_strip\n\
```\n\n\
A fence without a language falls back to text:\n\n\
```\n\
test result: ok. 14 passed; 0 failed\n\
```";

const MARKDOWN_FENCE: &str = "Here is the changelog entry, ready to paste:\n\n\
```markdown\n\
### Browser\n\n\
- Pinned tabs keep their icon width however many tabs are open\n\
- The drop preview follows the pointer across the pinned boundary\n\n\
Thanks to **everyone** who sent screenshots.\n\
```";

const STREAMING: &str = "The fix lives in **`tab_strip.rs`, where the pinned tabs are \
measured first. See the [layout notes](https://zzmux.sh/docs/bro";

const CLASSES: &str = "The timeline keeps three things apart:\n\n\
```mermaid\n\
classDiagram\n\
    class AgentTimeline {\n\
        +rows\n\
        +active_turn\n\
        +render()\n\
    }\n\
    class AgentTimelineStore {\n\
        +markdown\n\
        +expanded\n\
        +set_streaming()\n\
    }\n\
    class ListState {\n\
        +scroll_to_end()\n\
    }\n\
    AgentTimeline --> AgentTimelineStore : reads\n\
    AgentTimeline --> ListState : scrolls\n\
```";

const SEQUENCE: &str = "```mermaid\n\
sequenceDiagram\n\
    participant Pane\n\
    participant Daemon\n\
    participant Agent\n\
    Pane->>Daemon: prompt\n\
    Daemon->>Agent: session/prompt\n\
    Agent-->>Daemon: tool call\n\
    Daemon-->>Pane: timeline update\n\
    Agent-->>Pane: answer\n\
```";

const BROKEN_MERMAID: &str = "```mermaid\n\
timelineDiagramX\n\
    Prompt --> Answer\n\
```";

pub fn user_message(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new(
                    "short prompt",
                    &[user(100, "Why do pinned tabs shrink when I open thirty tabs?")],
                    cx,
                ),
                Case::new(
                    "markdown prompt",
                    &[user(
                        110,
                        "Two things:\n\n1. Pinned tabs should keep their icon width\n2. The drop preview lags behind the pointer\n\nThe layout lives in `crates/zz-ui/src/browser/tab_strip.rs`.",
                    )],
                    cx,
                ),
                Case::new(
                    "with images",
                    &[user_with_images(
                        120,
                        "This is what it looks like with thirty tabs open.",
                        vec![
                            tab_strip_screenshot(),
                            terminal_screenshot(),
                            chart_screenshot(),
                        ],
                    )],
                    cx,
                ),
                Case::new(
                    "wide image",
                    &[user_with_images(
                        130,
                        "hi can you read this image properly?",
                        vec![wide_screenshot()],
                    )],
                    cx,
                ),
                Case::new(
                    "images only",
                    &[user_with_images(
                        140,
                        "",
                        vec![tab_strip_screenshot(), chart_screenshot()],
                    )],
                    cx,
                ),
                Case::new(
                    "unbroken text",
                    &[user(
                        150,
                        "CI failed overnight: https://github.com/demfabris/zz/actions/runs/18234567890/job/51987654321?pr=1234#step:7:212",
                    )],
                    cx,
                ),
                Case::new(
                    "narrow pane",
                    &[user_with_images(
                        170,
                        "Pinned tabs shrink to nothing when I open thirty tabs, see `TabStrip::layout_pinned_and_unpinned_widths`.",
                        vec![wide_screenshot(), tab_strip_screenshot()],
                    )],
                    cx,
                )
                .width(320.0),
                Case::new(
                    "rewind enabled, hover for the History button",
                    &[rewindable(
                        160,
                        "Undo that and give the pinned tabs a min-width instead.",
                    )],
                    cx,
                )
                .rewind(),
            ],
        )
    })
    .into()
}

pub fn answer(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new(
                    "final answer, with copy",
                    &[
                        user(200, "Did the browser tests pass?"),
                        assistant(
                            201,
                            "Yes. All 14 browser tests pass, including the new pinned tab test.",
                        ),
                    ],
                    cx,
                ),
                Case::new("rich answer", &[assistant(210, RICH_ANSWER)], cx),
                Case::new(
                    "turn still active, no copy",
                    &[assistant(220, "Yes. All 14 browser tests pass.")],
                    cx,
                )
                .active(),
                Case::new(
                    "two answers, copy on the last",
                    &[
                        assistant(230, "The tab strip is fixed."),
                        assistant(231, "I also tightened the drop preview while I was there."),
                    ],
                    cx,
                ),
                Case::new(
                    "answer after work",
                    &[
                        thought(240, THINKING),
                        done(
                            241,
                            Kind::Read,
                            "Read crates/zz-ui/src/browser/tab_strip.rs",
                        ),
                        done(
                            242,
                            Kind::Edit,
                            "Edit crates/zz-ui/src/browser/tab_strip.rs",
                        ),
                        assistant(243, "Pinned tabs now keep their icon width."),
                    ],
                    cx,
                ),
            ],
        )
    })
    .into()
}

pub fn turn_live(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new(
                    "working, latest thought below",
                    &[
                        thought(300, THINKING),
                        done(301, Kind::Search, "rg -n \"pinned\" crates/zz-ui/src/browser"),
                        done(302, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
                    ],
                    cx,
                )
                .active(),
                Case::new(
                    "running a tool",
                    &[
                        done(310, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
                        tool(311, Kind::Execute, Status::Running, "cargo test -p zz-ui browser")
                            .entry(),
                    ],
                    cx,
                )
                .active(),
                Case::new(
                    "thinking",
                    &[
                        done(320, Kind::Read, "Read crates/zz-ui/src/browser/drag.rs"),
                        thought(321, "The drop preview uses the old widths. If the layout pass stores its widths, drag can read them back instead of measuring again."),
                    ],
                    cx,
                )
                .active(),
                Case::new(
                    "waiting for approval",
                    &[
                        done(330, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
                        tool(
                            331,
                            Kind::Edit,
                            Status::NeedsApproval,
                            "Edit crates/zz-ui/src/browser/tab_strip.rs",
                        )
                        .entry(),
                    ],
                    cx,
                )
                .active(),
                Case::new(
                    "narration between steps",
                    &[
                        assistant(340, "I'll read the tab strip first, then the drag code that depends on it."),
                        tool(341, Kind::Read, Status::Running, "Read crates/zz-ui/src/browser/tab_strip.rs")
                            .entry(),
                    ],
                    cx,
                )
                .active(),
                Case::new(
                    "answer streaming",
                    &[
                        done(350, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
                        done(351, Kind::Edit, "Edit crates/zz-ui/src/browser/tab_strip.rs"),
                        assistant(352, "Pinned tabs now keep a **28px floor**, and the unpinned tabs share"),
                    ],
                    cx,
                )
                .active()
                .streaming(352, cx),
            ],
        )
    })
    .into()
}

pub fn turn_done(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new(
                    "worked, one badge per kind",
                    &[
                        thought(400, THINKING),
                        done(401, Kind::Search, "rg -n \"pinned\" crates/zz-ui/src/browser"),
                        done(402, Kind::Search, "rg -n \"drop_preview\" crates"),
                        done(403, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
                        done(404, Kind::Read, "Read crates/zz-ui/src/browser/drag.rs"),
                        done(405, Kind::Read, "Read knowledge/designs/browser-tabs.md"),
                        done(406, Kind::Edit, "Edit crates/zz-ui/src/browser/tab_strip.rs"),
                        done(407, Kind::Edit, "Edit crates/zz-ui/src/browser/drag.rs"),
                        done(408, Kind::Execute, "cargo test -p zz-ui browser"),
                        done(409, Kind::Fetch, "Fetch https://zzmux.sh/docs/browser"),
                        done(410, Kind::Other, "linear.get_issue ZZ-412"),
                        assistant(411, "Pinned tabs now keep their icon width."),
                    ],
                    cx,
                ),
                Case::new(
                    "one step failed along the way",
                    &[
                        done(420, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
                        tool(421, Kind::Execute, Status::Failed, "cargo test -p zz-ui browser")
                            .terminal(TEST_FAILURE)
                            .exit(101)
                            .entry(),
                        done(422, Kind::Edit, "Edit crates/zz-ui/src/browser/drag.rs"),
                        done(423, Kind::Execute, "cargo test -p zz-ui browser"),
                        assistant(424, "Both drag tests pass now."),
                    ],
                    cx,
                ),
                Case::new(
                    "ended on a failure",
                    &[
                        done(430, Kind::Edit, "Edit crates/zz-ui/src/browser/drag.rs"),
                        tool(431, Kind::Execute, Status::Failed, "cargo test -p zz-ui browser")
                            .terminal(TEST_FAILURE)
                            .exit(101)
                            .entry(),
                    ],
                    cx,
                )
                .open_output(),
                Case::new(
                    "thought only",
                    &[
                        thought(440, THINKING),
                        assistant(441, "Nothing to change: the pinned floor already exists behind `browser.pinned-floor`."),
                    ],
                    cx,
                ),
                Case::new(
                    "with a subagent",
                    &[
                        tool(450, Kind::Other, Status::Completed, "Survey every caller of TabStrip::layout")
                            .entry(),
                        tool(451, Kind::Search, Status::Completed, "rg -n \"TabStrip::layout\"")
                            .parent(450)
                            .entry(),
                        tool(452, Kind::Read, Status::Completed, "Read crates/zz/src/browser/view.rs")
                            .parent(450)
                            .entry(),
                        tool(453, Kind::Read, Status::Completed, "Read clients/app/src/app/browser_pane.rs")
                            .parent(450)
                            .entry(),
                        done(454, Kind::Edit, "Edit crates/zz-ui/src/browser/tab_strip.rs"),
                        assistant(455, "Three callers, all updated."),
                    ],
                    cx,
                ),
                Case::new(
                    "narrow pane, every badge",
                    &[
                        done(470, Kind::Search, "rg -n \"pinned\" crates"),
                        done(471, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
                        done(472, Kind::Edit, "Edit crates/zz-ui/src/browser/tab_strip.rs"),
                        done(473, Kind::Execute, "cargo test -p zz-ui browser"),
                        done(474, Kind::Fetch, "Fetch https://zzmux.sh/docs/browser"),
                        done(475, Kind::Other, "linear.get_issue ZZ-412"),
                        tool(476, Kind::Other, Status::Completed, "Survey callers").entry(),
                        tool(477, Kind::Read, Status::Completed, "Read crates/zz/src/browser/view.rs").parent(476).entry(),
                        tool(478, Kind::Execute, Status::Failed, "just web build")
                            .text("error: wasm-bindgen 0.2.128 is required")
                            .exit(2)
                            .entry(),
                        assistant(479, "Done, except the web build."),
                    ],
                    cx,
                )
                .width(320.0),
                Case::new(
                    "stopped by the user",
                    &[
                        done(460, Kind::Read, "Read crates/zz-ui/src/browser/tab_strip.rs"),
                        tool(461, Kind::Execute, Status::Canceled, "cargo test --workspace").entry(),
                    ],
                    cx,
                ),
            ],
        )
    })
    .into()
}

pub fn turn_expanded(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new(
                    "finished turn, every step shape",
                    &[
                        thought(500, THINKING),
                        assistant(501, "I'll start with the layout, then the drag code."),
                        done(
                            502,
                            Kind::Search,
                            "rg -n \"pinned\" crates/zz-ui/src/browser",
                        ),
                        done(
                            503,
                            Kind::Read,
                            "Read crates/zz-ui/src/browser/tab_strip.rs",
                        ),
                        done(
                            504,
                            Kind::Edit,
                            "Edit crates/zz-ui/src/browser/tab_strip.rs",
                        ),
                        tool(
                            505,
                            Kind::Execute,
                            Status::Failed,
                            "cargo test -p zz-ui browser",
                        )
                        .terminal(TEST_FAILURE)
                        .exit(101)
                        .entry(),
                        tool(
                            506,
                            Kind::Execute,
                            Status::Completed,
                            "cargo test -p zz-ui browser",
                        )
                        .terminal("running 14 tests\ntest result: ok. 14 passed; 0 failed")
                        .entry(),
                        tool(
                            507,
                            Kind::Fetch,
                            Status::Canceled,
                            "Fetch https://zzmux.sh/docs/browser",
                        )
                        .entry(),
                        tool(
                            508,
                            Kind::Other,
                            Status::Failed,
                            "linear.update_issue ZZ-412",
                        )
                        .entry(),
                        tool(
                            509,
                            Kind::Other,
                            Status::Completed,
                            "Survey every caller of TabStrip::layout",
                        )
                        .entry(),
                        tool(
                            510,
                            Kind::Search,
                            Status::Completed,
                            "rg -n \"TabStrip::layout\"",
                        )
                        .parent(509)
                        .entry(),
                        tool(
                            511,
                            Kind::Read,
                            Status::Completed,
                            "Read crates/zz/src/browser/view.rs",
                        )
                        .parent(509)
                        .entry(),
                        assistant(
                            512,
                            "Pinned tabs now keep their icon width, and the drag tests pass.",
                        ),
                    ],
                    cx,
                )
                .open_output()
                .expand(500, cx),
                Case::new(
                    "live turn, running step",
                    &[
                        thought(520, "Checking the drag offsets next."),
                        done(521, Kind::Read, "Read crates/zz-ui/src/browser/drag.rs"),
                        tool(
                            522,
                            Kind::Execute,
                            Status::Running,
                            "cargo test -p zz-ui browser::drag",
                        )
                        .entry(),
                    ],
                    cx,
                )
                .active()
                .expand(520, cx),
                Case::new(
                    "live turn, waiting for approval",
                    &[
                        done(530, Kind::Read, "Read crates/zz-ui/src/browser/drag.rs"),
                        tool(
                            531,
                            Kind::Edit,
                            Status::NeedsApproval,
                            "Edit crates/zz-ui/src/browser/drag.rs",
                        )
                        .entry(),
                    ],
                    cx,
                )
                .active()
                .expand(530, cx),
                Case::new(
                    "a thought with no text",
                    &[
                        thought(540, ""),
                        done(541, Kind::Read, "Read crates/zz-ui/src/browser/drag.rs"),
                    ],
                    cx,
                )
                .expand(540, cx),
            ],
        )
    })
    .into()
}

const KINDS: [(Kind, &str); 7] = [
    (Kind::Read, "Read crates/zz-ui/src/agent.rs"),
    (Kind::Search, "rg -n \"TimelineStick\" crates"),
    (Kind::Edit, "Edit crates/zz-ui/src/agent/composer.rs"),
    (Kind::Execute, "cargo test -p zz-ui agent"),
    (Kind::Fetch, "Fetch https://docs.rs/zpui"),
    (Kind::Think, "Think about the row layout"),
    (Kind::Other, "linear.get_issue ZZ-412"),
];

const STATUSES: [(Status, &str, bool); 6] = [
    (Status::Pending, "pending, live turn", true),
    (Status::Running, "running, live turn", true),
    (Status::NeedsApproval, "needs approval", true),
    (Status::Completed, "completed", false),
    (Status::Failed, "failed, exit code on commands", false),
    (Status::Canceled, "canceled", false),
];

pub fn tool_marks(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        let mut cases = STATUSES
            .iter()
            .enumerate()
            .map(|(column, (status, label, live))| {
                let base = 600 + column as u64 * 10;
                let entries = KINDS
                    .iter()
                    .enumerate()
                    .map(|(row, (kind, text))| {
                        let step = tool(base + row as u64, *kind, *status, text);
                        if *status == Status::Failed && *kind == Kind::Execute {
                            step.terminal(TEST_FAILURE).exit(101).entry()
                        } else {
                            step.entry()
                        }
                    })
                    .collect::<Vec<_>>();
                let case = Case::new(*label, &entries, cx).expand(base, cx);
                if *live { case.active() } else { case }
            })
            .collect::<Vec<_>>();
        cases.push(
            Case::new(
                "a step with substeps",
                &[
                    tool(
                        700,
                        Kind::Other,
                        Status::Completed,
                        "Survey every caller of TabStrip::layout",
                    )
                    .entry(),
                    tool(
                        701,
                        Kind::Search,
                        Status::Completed,
                        "rg -n \"TabStrip::layout\"",
                    )
                    .parent(700)
                    .entry(),
                    tool(
                        702,
                        Kind::Read,
                        Status::Completed,
                        "Read crates/zz/src/browser/view.rs",
                    )
                    .parent(700)
                    .entry(),
                    tool(703, Kind::Other, Status::Running, "Review the diff").entry(),
                    tool(
                        704,
                        Kind::Read,
                        Status::Running,
                        "Read crates/zz-ui/src/browser/drag.rs",
                    )
                    .parent(703)
                    .entry(),
                ],
                cx,
            )
            .active()
            .expand(700, cx),
        );
        cases.push(
            Case::new(
                "a command that opens its output",
                &[
                    tool(
                        710,
                        Kind::Execute,
                        Status::Completed,
                        "cargo test -p zz-ui browser",
                    )
                    .terminal("running 14 tests\ntest result: ok. 14 passed; 0 failed")
                    .entry(),
                    tool(711, Kind::Execute, Status::Failed, "just web build")
                        .text("error: wasm-bindgen 0.2.128 is required")
                        .exit(2)
                        .entry(),
                    tool(712, Kind::Execute, Status::Completed, "git status --short").entry(),
                ],
                cx,
            )
            .open_output()
            .expand(710, cx),
        );
        Cases::new(2, cases)
    })
    .into()
}

pub fn replies(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new(
                    "side answer to /btw",
                    &[
                        user(800, "/btw what does `TimelineStick` do?"),
                        aside(
                            801,
                            "It is the spring that keeps the timeline pinned to the newest message while the agent streams, and lets go as soon as you scroll up.",
                            true,
                            Some(800),
                        ),
                    ],
                    cx,
                ),
                Case::new(
                    "reply that joins the conversation",
                    &[
                        user(810, "/steer skip the docs, just fix the drag test"),
                        aside(811, "Got it. I'll leave knowledge/ alone and fix the drag test.", false, Some(810)),
                    ],
                    cx,
                ),
                Case::new(
                    "notices",
                    &[
                        aside(820, "Forked this conversation into a new session.", false, None),
                        aside(
                            821,
                            "Rewound to before your last 2 prompts. Files stay as they are.",
                            false,
                            None,
                        ),
                    ],
                    cx,
                ),
                Case::new(
                    "side question while a turn runs",
                    &[
                        user(830, "Fix the drag test."),
                        done(831, Kind::Read, "Read crates/zz-ui/src/browser/drag.rs"),
                        tool(832, Kind::Execute, Status::Running, "cargo test -p zz-ui browser::drag")
                            .entry(),
                        user(833, "/btw how long does the browser suite take?"),
                        aside(834, "About 40 seconds on this machine.", true, Some(833)),
                    ],
                    cx,
                )
                .active(),
            ],
        )
    })
    .into()
}

pub fn markdown(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new("code blocks", &[assistant(900, CODE_ANSWER)], cx),
                Case::new(
                    "markdown fence renders rich in answers",
                    &[assistant(910, MARKDOWN_FENCE)],
                    cx,
                ),
                Case::new(
                    "markdown fence in a prompt stays code",
                    &[user(920, MARKDOWN_FENCE)],
                    cx,
                ),
                Case::new(
                    "streaming, hanging markers closed",
                    &[assistant(930, STREAMING)],
                    cx,
                )
                .active()
                .streaming(930, cx),
                Case::new(
                    "the same text once settled",
                    &[assistant(940, STREAMING)],
                    cx,
                ),
            ],
        )
    })
    .into()
}

pub fn mermaid(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new("class diagram", &[assistant(1000, CLASSES)], cx),
                Case::new("sequence diagram", &[assistant(1010, SEQUENCE)], cx),
                Case::new(
                    "source that does not parse",
                    &[assistant(1020, BROKEN_MERMAID)],
                    cx,
                ),
            ],
        )
    })
    .into()
}

pub fn large_message(_: &mut Window, cx: &mut App) -> AnyView {
    let lines = (1..=600)
        .map(|line| format!("test browser::tab_strip::case_{line:03} ... ok"))
        .collect::<Vec<_>>()
        .join("\n");
    let log = format!("Here is the whole run:\n\n```\n{lines}\n```");
    cx.new(|cx| {
        Cases::new(
            1,
            vec![
                Case::new(
                    "long answer, scrolls inside the message",
                    &[assistant(1100, &log)],
                    cx,
                ),
                Case::new(
                    "long prompt, with Copy full message",
                    &[user(1110, &log)],
                    cx,
                ),
            ],
        )
    })
    .into()
}
