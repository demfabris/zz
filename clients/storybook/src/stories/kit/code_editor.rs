use zpui::{
    AnyView, App, AppContext as _, Context, Entity, EntityInputHandler as _, IntoElement,
    ParentElement as _, Render, Styled as _, Window, div, px,
};
use zz_ui::{
    ActiveTheme as _,
    code_editor::{CodeEditor, CodeEditorState, TabSize},
};

use super::support::{Probe, when_visible};
use crate::story::{Section, Story, states};

pub const STORY: Story = Story {
    id: "code-editor",
    name: "Code editor",
    group: "Kit",
    summary: "CodeEditor is the full-height editor behind the editor pane: a rope buffer with a line-number rail, tree-sitter highlighting, soft wrap and vim.",
    sections: &[
        Section {
            id: "languages",
            name: "Languages",
            summary: "Rust and JSON with the theme's syntax colors, line numbers on and soft wrap off.",
            build: |window, cx| Editors::view(Mode::Languages, window, cx),
        },
        Section {
            id: "rails",
            name: "Line number rails",
            summary: "Relative numbers count away from the cursor line; the rail can also be hidden, here with soft wrap on.",
            build: |window, cx| Editors::view(Mode::Rails, window, cx),
        },
        Section {
            id: "vim-visual",
            name: "Vim visual mode",
            summary: "set_vim_enabled(true), then v and two l motions typed through the input handler select three characters.",
            build: |window, cx| Editors::view(Mode::Vim, window, cx),
        },
        Section {
            id: "scrolled-sideways",
            name: "Scrolled sideways",
            summary: "With soft wrap off, a cursor past the right edge scrolls the code sideways. The rail stays put while the code moves under it.",
            build: |window, cx| Editors::view(Mode::Sideways, window, cx),
        },
        Section {
            id: "states",
            name: "States",
            summary: "A focused editor with a selection, and a disabled one. The caret blinks only while the window is active.",
            build: |window, cx| Editors::view(Mode::States, window, cx),
        },
    ],
};

const RUST: &str = r"use std::collections::HashMap;

/// One tmux-style session: named windows, each a tree of panes.
pub struct Session {
    name: String,
    windows: Vec<Window>,
    options: HashMap<&'static str, String>,
}

impl Session {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), windows: Vec::new(), options: HashMap::new() }
    }

    pub fn rename(&mut self, name: &str) -> bool {
        let changed = self.name != name;
        self.name = name.to_owned();
        changed
    }
}
";

const JSON: &str = r##"{
  "theme": "nord",
  "font": { "family": "Lilex", "size": 13 },
  "status": { "left": "#S", "interval": 5 },
  "panes": [1, 2, 3],
  "motion": false,
  "shell": null
}
"##;

const PROSE: &str = "Soft wrap folds long lines at the editor's edge instead of scrolling sideways, so a paragraph like this one reads as several visual rows while staying one buffer line.\nThe second buffer line is short.\nThe third one is also long enough to wrap once the rail is hidden and the editor is narrow.\n";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Languages,
    Rails,
    Vim,
    Sideways,
    States,
}

struct Editors {
    left: Entity<CodeEditorState>,
    right: Entity<CodeEditorState>,
    mode: Mode,
    frame: Probe,
}

fn editor(
    window: &mut Window,
    cx: &mut App,
    value: &str,
    language: &str,
    wrap: bool,
) -> Entity<CodeEditorState> {
    let value = value.to_owned();
    let language = language.to_owned();
    cx.new(|cx| {
        CodeEditorState::new(window, cx)
            .default_value(value)
            .language(language)
            .soft_wrap(wrap)
            .tab_size(TabSize {
                tab_size: 4,
                hard_tabs: false,
            })
    })
}

impl Editors {
    fn view(mode: Mode, window: &mut Window, cx: &mut App) -> AnyView {
        let frame = Probe::default();
        let (left, right) = match mode {
            Mode::Languages => (
                editor(window, cx, RUST, "rust", false),
                editor(window, cx, JSON, "json", false),
            ),
            Mode::Rails => {
                let relative = editor(window, cx, RUST, "rust", false);
                relative.update(cx, |state, cx| state.set_relative_line_numbers(true, cx));
                let target = relative.clone();
                when_visible(&frame, window, move |_, cx| {
                    target.update(cx, |state, cx| state.set_selected_range(260..260, cx));
                });
                let bare = editor(window, cx, PROSE, "text", true);
                bare.update(cx, |state, cx| state.set_line_numbers(false, cx));
                (relative, bare)
            }
            Mode::Vim => {
                let vim = editor(window, cx, RUST, "rust", false);
                vim.update(cx, |state, cx| state.set_vim_enabled(true, cx));
                let target = vim.clone();
                when_visible(&frame, window, move |window, cx| {
                    target.update(cx, |state, cx| {
                        state.set_selected_range(122..122, cx);
                        state.replace_text_in_range(None, "vll", window, cx);
                    });
                });
                (vim, editor(window, cx, JSON, "json", false))
            }
            Mode::Sideways => {
                let wide = editor(window, cx, RUST, "rust", false);
                let target = wide.clone();
                when_visible(&frame, window, move |_, cx| {
                    target.update(cx, |state, cx| state.set_selected_range(96..96, cx));
                });
                (wide, editor(window, cx, RUST, "rust", true))
            }
            Mode::States => {
                let focused = editor(window, cx, JSON, "json", false);
                focused.update(cx, |state, cx| state.set_selected_range(4..11, cx));
                let target = focused.clone();
                when_visible(&frame, window, move |window, cx| {
                    target.update(cx, |state, cx| state.focus(window, cx));
                });
                (focused, editor(window, cx, JSON, "json", false))
            }
        };
        cx.new(|_| Self {
            left,
            right,
            mode,
            frame,
        })
        .into()
    }
}

impl Render for Editors {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let frame = |state: &Entity<CodeEditorState>, disabled: bool| {
            div()
                .relative()
                .h(px(300.0))
                .font_family(theme.mono_font_family.clone())
                .text_size(theme.mono_font_size)
                .child(
                    CodeEditor::new(state)
                        .bordered(true)
                        .focus_bordered(true)
                        .disabled(disabled),
                )
        };
        let (left, right) = match self.mode {
            Mode::Languages => ("rust", "json"),
            Mode::Rails => ("relative line numbers", "no line numbers, soft wrap"),
            Mode::Vim => ("vim, visual", "vim off"),
            Mode::Sideways => ("cursor at the end of line 3", "soft wrap on"),
            Mode::States => ("focused, selection", "disabled"),
        };
        states()
            .columns(2)
            .state(left, frame(&self.left, false).child(self.frame.measure()))
            .state(right, frame(&self.right, self.mode == Mode::States))
    }
}
