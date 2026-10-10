use crate::story::{Section, Story};

pub(super) mod cards;
mod chrome;
mod composer;
mod fixtures;
mod pane;
mod timeline;
mod transcript;

pub const STORIES: &[Story] = &[STORY];

const STORY: Story = Story {
    id: "agent",
    name: "Agent pane",
    group: "Agent",
    summary: "Every piece of the agent pane, from the header down to the composer, in the states zz draws it in. Each piece is built from the same zz-ui calls the app makes, fed with fixed transcripts.",
    sections: &[
        Section {
            id: "header",
            name: "Header",
            summary: "The 36px bar: provider, session title, status pill, split, drag and close. Actions on an inactive pane appear on hover.",
            build: chrome::headers,
        },
        Section {
            id: "status-pill",
            name: "Status pill",
            summary: "What the header says about the agent process. Running and Stopping spin; Exited can offer Restart.",
            build: chrome::status_pills,
        },
        Section {
            id: "user-message",
            name: "User message",
            summary: "Prompts sit right-aligned in a raised bubble. Images come first as 140px tiles that open a full view.",
            build: transcript::user_message,
        },
        Section {
            id: "answer",
            name: "Answer",
            summary: "Assistant text after the last step of a turn. Only the final answer of a finished turn gets a copy button.",
            build: transcript::answer,
        },
        Section {
            id: "turn-live",
            name: "Live turn",
            summary: "A running turn folds into one line that says what the agent is doing now, with a step count and a clock.",
            build: transcript::turn_live,
        },
        Section {
            id: "turn-done",
            name: "Finished turn",
            summary: "A finished turn shows one badge per kind of step and how long it worked. A turn that ended on a failure shows the last two lines of output.",
            build: transcript::turn_done,
        },
        Section {
            id: "turn-expanded",
            name: "Expanded trace",
            summary: "Opening a turn lists its thoughts, narration and tool calls in order, with subagent steps folded under their agent.",
            build: transcript::turn_expanded,
        },
        Section {
            id: "tool-marks",
            name: "Tool marks",
            summary: "Every tool kind in every status, as rows of an expanded trace. Spinners only run while the turn is live.",
            build: transcript::tool_marks,
        },
        Section {
            id: "replies-and-notices",
            name: "Replies and notices",
            summary: "Answers to /btw and other zz verbs hang under their prompt. Notices from zz itself are muted lines with an info mark.",
            build: transcript::replies,
        },
        Section {
            id: "markdown",
            name: "Markdown",
            summary: "Code blocks get a language label and a copy button. Markdown fences render rich in answers, and a streaming answer closes its open markers.",
            build: transcript::markdown,
        },
        Section {
            id: "mermaid",
            name: "Mermaid",
            summary: "Mermaid fences render as diagrams off the main thread. The first frames read Rendering Mermaid, and a source that does not parse shows the error.",
            build: transcript::mermaid,
        },
        Section {
            id: "large-message",
            name: "Large message",
            summary: "Messages over 32 KiB or 512 lines render a preview in a 420px scroll box. Prompts also offer Copy full message.",
            build: transcript::large_message,
        },
        Section {
            id: "composer",
            name: "Composer",
            summary: "The prompt box and its footer. The round action button sends, queues behind a running turn, or stops it.",
            build: composer::composers,
        },
        Section {
            id: "composer-controls",
            name: "Composer controls",
            summary: "The pieces of the composer on their own: action button, attach, mode and model pickers, context meter, git summary and working directory.",
            build: composer::controls,
        },
        Section {
            id: "permission",
            name: "Permission card",
            summary: "A tool call waiting for approval, above the composer. Digits pick an option, Enter confirms, Esc cancels.",
            build: cards::permissions,
        },
        Section {
            id: "question",
            name: "Question card",
            summary: "Questions the agent asks, one card per request. A single plain question submits on the first pick; anything else waits for Submit.",
            build: cards::questions,
        },
        Section {
            id: "slash-menu",
            name: "Slash menu",
            summary: "Commands offered while typing a slash: the agent's own plus the zz verbs, ranked by the query.",
            build: cards::slash_menu,
        },
        Section {
            id: "task-tray",
            name: "Task tray",
            summary: "The plan chip and background tasks, above the composer. Each opens a panel; click the chips to toggle.",
            build: cards::trays,
        },
        Section {
            id: "empty-states",
            name: "Empty states",
            summary: "What the pane shows before the first message: the welcome, waiting and failure messages, and the error card.",
            build: chrome::empty_states,
        },
        Section {
            id: "jump-to-bottom",
            name: "Jump to latest",
            summary: "The round button that appears over the timeline once you scroll away from the newest message.",
            build: chrome::jump,
        },
        Section {
            id: "full-pane",
            name: "Full pane",
            summary: "The pieces together as zz lays out an agent pane: a conversation with a turn still running, the plan and a background task above the composer.",
            build: pane::full_pane,
        },
    ],
};
