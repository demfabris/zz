#![allow(clippy::complexity, clippy::pedantic, clippy::style)]
// Display-map and folding seams stay unused until zz ships a folding UI.
#![allow(dead_code)]

mod blink_cursor;
mod change;
mod cursor;
mod display_map;
mod element;
mod history;
mod indent;
mod input;
mod mode;
mod movement;
mod rope_ext;
mod selection;
mod state;
mod vim;

pub use cursor::Selection;
pub use display_map::{BufferPoint, DisplayMap, DisplayPoint, FoldRange};
pub(crate) use element::{LastLayout, WhitespaceIndicators};
pub use indent::TabSize;
pub use input::CodeEditor;
pub use rope_ext::{InputEdit, Point, RopeExt, RopeLines};
pub use ropey::Rope;
pub use state::{CodeEditorEvent, CodeEditorState};
pub use vim::VimMode;

/// Line/UTF-16-column position used by the rope conversion helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

pub(crate) fn init(cx: &mut gpui::App) {
    state::init(cx);
}
