use std::sync::Arc;

use zpui::{Image, ImageFormat};
use zz_protocol::{
    AgentTaskWire,
    agent_stream::{AgentQuestion, AgentQuestionOption},
};
use zz_ui::agent::{
    AgentAside, AgentEntry, AgentToolEntry, AgentToolKind, AgentToolPayload, AgentToolStatus,
    controls::{AgentControlChoice, AgentControlSelection},
};

const WIDE_SCREENSHOT: &[u8] =
    include_bytes!("../../../../../crates/zz-ui/src/fixtures/wide-screenshot.png");

const TAB_STRIP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="480" height="300" viewBox="0 0 480 300">
<rect width="480" height="300" rx="14" fill="#1b1d22"/>
<rect width="480" height="46" rx="14" fill="#262931"/>
<rect y="30" width="480" height="16" fill="#262931"/>
<rect x="14" y="10" width="26" height="26" rx="6" fill="#3a3f4b"/>
<rect x="44" y="10" width="10" height="26" rx="3" fill="#3a3f4b"/>
<rect x="58" y="10" width="6" height="26" rx="2" fill="#3a3f4b"/>
<rect x="70" y="8" width="132" height="30" rx="7" fill="#1b1d22"/>
<rect x="82" y="19" width="12" height="8" rx="2" fill="#7aa2f7"/>
<rect x="100" y="20" width="84" height="6" rx="3" fill="#a9b1c6"/>
<rect x="208" y="14" width="110" height="18" rx="5" fill="#30343e"/>
<rect x="324" y="14" width="110" height="18" rx="5" fill="#30343e"/>
<rect x="30" y="76" width="300" height="12" rx="6" fill="#3a3f4b"/>
<rect x="30" y="102" width="420" height="10" rx="5" fill="#2c3039"/>
<rect x="30" y="122" width="390" height="10" rx="5" fill="#2c3039"/>
<rect x="30" y="142" width="410" height="10" rx="5" fill="#2c3039"/>
<rect x="30" y="182" width="420" height="88" rx="10" fill="#22252c"/>
<circle cx="52" cy="23" r="18" fill="none" stroke="#f7768e" stroke-width="3"/>
</svg>"##;

const BAR_CHART: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="420" height="300" viewBox="0 0 420 300">
<rect width="420" height="300" rx="14" fill="#f6f4ef"/>
<rect x="40" y="250" width="340" height="2" fill="#c9c4b8"/>
<rect x="60" y="150" width="40" height="100" rx="4" fill="#9ab4d8"/>
<rect x="120" y="110" width="40" height="140" rx="4" fill="#9ab4d8"/>
<rect x="180" y="170" width="40" height="80" rx="4" fill="#9ab4d8"/>
<rect x="240" y="60" width="40" height="190" rx="4" fill="#e07a5f"/>
<rect x="300" y="130" width="40" height="120" rx="4" fill="#9ab4d8"/>
<rect x="40" y="30" width="160" height="12" rx="6" fill="#4a4740"/>
</svg>"##;

const TERMINAL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="360" height="300" viewBox="0 0 360 300">
<rect width="360" height="300" rx="14" fill="#0f1115"/>
<circle cx="22" cy="20" r="6" fill="#ff5f57"/>
<circle cx="42" cy="20" r="6" fill="#febc2e"/>
<circle cx="62" cy="20" r="6" fill="#28c840"/>
<rect x="20" y="50" width="180" height="9" rx="2" fill="#7dcfff"/>
<rect x="20" y="70" width="260" height="9" rx="2" fill="#565f89"/>
<rect x="20" y="90" width="230" height="9" rx="2" fill="#565f89"/>
<rect x="20" y="110" width="120" height="9" rx="2" fill="#9ece6a"/>
<rect x="20" y="130" width="300" height="9" rx="2" fill="#f7768e"/>
<rect x="20" y="150" width="270" height="9" rx="2" fill="#f7768e"/>
<rect x="20" y="170" width="140" height="9" rx="2" fill="#565f89"/>
<rect x="20" y="200" width="10" height="14" fill="#c0caf5"/>
</svg>"##;

pub fn wide_screenshot() -> Arc<Image> {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        WIDE_SCREENSHOT.to_vec(),
    ))
}

fn svg(source: &str) -> Arc<Image> {
    Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        source.as_bytes().to_vec(),
    ))
}

pub fn tab_strip_screenshot() -> Arc<Image> {
    svg(TAB_STRIP)
}

pub fn chart_screenshot() -> Arc<Image> {
    svg(BAR_CHART)
}

pub fn terminal_screenshot() -> Arc<Image> {
    svg(TERMINAL)
}

pub fn user(id: u64, text: &str) -> AgentEntry {
    AgentEntry::User {
        id,
        markdown: text.into(),
        images: Arc::from([]),
        rewind_id: None,
    }
}

pub fn user_with_images(id: u64, text: &str, images: Vec<Arc<Image>>) -> AgentEntry {
    AgentEntry::User {
        id,
        markdown: text.into(),
        images: images.into(),
        rewind_id: None,
    }
}

pub fn rewindable(id: u64, text: &str) -> AgentEntry {
    AgentEntry::User {
        id,
        markdown: text.into(),
        images: Arc::from([]),
        rewind_id: Some(format!("message-{id}").into()),
    }
}

pub fn assistant(id: u64, text: &str) -> AgentEntry {
    AgentEntry::Assistant {
        id,
        markdown: text.into(),
        aside: None,
    }
}

pub fn aside(id: u64, text: &str, side: bool, reply_to: Option<u64>) -> AgentEntry {
    AgentEntry::Assistant {
        id,
        markdown: text.into(),
        aside: Some(AgentAside { side, reply_to }),
    }
}

pub fn thought(id: u64, text: &str) -> AgentEntry {
    AgentEntry::Reasoning {
        id,
        label: "Reasoning".into(),
        markdown: text.into(),
        default_expanded: false,
    }
}

pub fn plan(id: u64, text: &str) -> AgentEntry {
    AgentEntry::Plan {
        id,
        markdown: text.into(),
    }
}

pub struct Tool(AgentToolEntry);

pub fn tool(id: u64, kind: AgentToolKind, status: AgentToolStatus, label: &str) -> Tool {
    Tool(AgentToolEntry {
        id,
        kind,
        status,
        label: label.to_owned().into(),
        location: None,
        input: None,
        output: Arc::from([]),
        default_expanded: false,
        parent: None,
        exit_code: None,
    })
}

pub fn done(id: u64, kind: AgentToolKind, label: &str) -> AgentEntry {
    tool(id, kind, AgentToolStatus::Completed, label).entry()
}

impl Tool {
    pub fn text(mut self, output: &str) -> Self {
        self.0.output = Arc::from([AgentToolPayload::Text(output.into())]);
        self
    }

    pub fn terminal(mut self, output: &str) -> Self {
        self.0.output = Arc::from([AgentToolPayload::Terminal(output.into())]);
        self
    }

    pub fn exit(mut self, code: i64) -> Self {
        self.0.exit_code = Some(code);
        self
    }

    pub fn parent(mut self, parent: u64) -> Self {
        self.0.parent = Some(parent);
        self
    }

    pub fn entry(self) -> AgentEntry {
        AgentEntry::Tool(self.0)
    }
}

pub const TEST_FAILURE: &str = "running 14 tests\n\
test browser::tab_strip::pinned_tabs_keep_their_icon ... ok\n\
test browser::drag::reorder_across_pinned_boundary ... FAILED\n\
test browser::drag::drop_preview_tracks_pointer ... FAILED\n\
\n\
failures:\n\
    browser::drag::reorder_across_pinned_boundary\n\
    browser::drag::drop_preview_tracks_pointer\n\
\n\
test result: FAILED. 12 passed; 2 failed; 0 ignored\n\
error: test failed, to rerun pass `-p zz-ui --lib`";

pub const PLAN: &str = "- [x] Find where the tab strip divides its width\n\
- [x] Give pinned tabs a floor at their icon width\n\
- [~] Fix drag reorder offsets across the pinned boundary\n\
- [ ] Run the browser test suite\n\
- [ ] Update the tab strip notes in knowledge/";

pub const PLAN_DONE: &str = "- [x] Find where the tab strip divides its width\n\
- [x] Give pinned tabs a floor at their icon width\n\
- [x] Fix drag reorder offsets across the pinned boundary";

pub fn tasks() -> Vec<AgentTaskWire> {
    vec![
        AgentTaskWire {
            id: "task-1".into(),
            kind: "shell".into(),
            description: "cargo watch -x 'test -p zz-ui browser'".into(),
            tool_call_id: Some("call-1".into()),
        },
        AgentTaskWire {
            id: "task-2".into(),
            kind: "agent".into(),
            description: "Survey every caller of TabStrip::layout".into(),
            tool_call_id: Some("call-2".into()),
        },
        AgentTaskWire {
            id: "task-3".into(),
            kind: "monitor".into(),
            description: "Watch target/storybook for a fresh wasm build".into(),
            tool_call_id: None,
        },
    ]
}

pub fn choices(items: &[(&str, &str, Option<&str>)]) -> Vec<AgentControlChoice> {
    items
        .iter()
        .map(|(value, name, description)| AgentControlChoice {
            value: (*value).to_owned(),
            name: (*name).to_owned(),
            description: description.map(str::to_owned),
        })
        .collect()
}

pub fn claude_modes() -> Vec<AgentControlChoice> {
    choices(&[
        ("default", "Default", Some("Ask before edits and commands")),
        (
            "acceptEdits",
            "Accept edits",
            Some("Edit files without asking"),
        ),
        ("plan", "Plan", Some("Plan before changing anything")),
    ])
}

pub fn codex_model() -> AgentControlSelection {
    AgentControlSelection {
        current_value: "gpt-5".into(),
        choices: choices(&[("gpt-5", "GPT-5", None), ("gpt-5-mini", "GPT-5 mini", None)]),
    }
}

pub fn claude_model() -> AgentControlSelection {
    AgentControlSelection {
        current_value: "opus".into(),
        choices: choices(&[
            ("opus", "Opus", None),
            ("sonnet", "Sonnet", None),
            ("haiku", "Haiku", None),
        ]),
    }
}

pub fn effort(current: &str) -> AgentControlSelection {
    AgentControlSelection {
        current_value: current.to_owned(),
        choices: choices(&[
            ("low", "Low", None),
            ("medium", "Medium", None),
            ("high", "High", None),
        ]),
    }
}

pub fn option(label: &str, description: Option<&str>) -> AgentQuestionOption {
    AgentQuestionOption {
        label: label.to_owned(),
        description: description.map(str::to_owned),
    }
}

pub fn question(id: &str, text: &str, options: Vec<AgentQuestionOption>) -> AgentQuestion {
    AgentQuestion {
        id: id.to_owned(),
        header: None,
        question: text.to_owned(),
        options,
        multi_select: false,
        allow_other: true,
        secret: false,
    }
}

pub fn branch_question() -> AgentQuestion {
    question(
        "base",
        "Which branch should the pull request target?",
        vec![
            option("main", Some("Ships with the next nightly")),
            option("release/0.16", Some("Backport to the current release")),
        ],
    )
}

pub fn checks_question() -> AgentQuestion {
    AgentQuestion {
        multi_select: true,
        ..question(
            "checks",
            "Which checks should run before I push?",
            vec![
                option("cargo test --workspace", None),
                option("cargo clippy", None),
                option("just web build", None),
                option("The storybook screenshots", None),
            ],
        )
    }
}
