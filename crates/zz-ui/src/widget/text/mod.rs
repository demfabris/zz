//! Markdown renderer: [`TextView`], its parsed state, and the window-level text
//! selection that spans several of them.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]
// Keeps the full node/inline API even where this app's markdown subset never
// reaches a given constructor.
#![allow(dead_code)]

mod auto_scroll;
mod document;
mod global;
mod inline;
mod inline_flow;
mod markdown_ext;
mod markdown_parse;
mod node;
mod scroll_area;
mod selection;
mod state;
mod style;
mod text_view;
mod utils;
mod veil;
mod window_selection;

use gpui::{App, KeyBinding, actions};

pub use markdown_ext::{
    MarkdownBlockParserFn, MarkdownBlockRenderFn, MarkdownExtensions, MarkdownNode,
    MarkdownParseContext, MarkdownPlugin, markdown_ast,
};
pub use node::CodeBlock;
pub use state::TextViewState;
pub use style::TextViewStyle;
// `TextViewLayoutState` is exported because it is `<TextView as
// Element>::RequestLayoutState`, which cannot be more private than the impl.
pub use text_view::{TextView, TextViewLayoutState};

pub use global::suppress_text_selection;

pub(crate) use window_selection::{SelectionScope, TextSelectionController, WindowTextSelection};

const CONTEXT: &str = "TextView";

/// Key context [`crate::Root`] dispatches the window-level [`Copy`] under.
pub const ROOT_KEY_CONTEXT: &str = "ZzRoot";

actions!(zz_text, [Copy, SelectAll]);

/// Selected block text carries one renderer-added trailing line feed.
pub(crate) fn clipboard_selection_text(selected_text: &str) -> &str {
    selected_text.strip_suffix('\n').unwrap_or(selected_text)
}

pub(crate) fn init(cx: &mut App) {
    global::init(cx);

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let (copy, select_all) = ("cmd-c", "cmd-a");
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let (copy, select_all) = ("ctrl-c", "ctrl-a");

    cx.bind_keys([
        KeyBinding::new(copy, Copy, Some(CONTEXT)),
        KeyBinding::new(select_all, SelectAll, Some(CONTEXT)),
        KeyBinding::new(copy, Copy, Some(ROOT_KEY_CONTEXT)),
    ]);
}
