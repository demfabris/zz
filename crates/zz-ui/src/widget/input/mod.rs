//! The text field: a single-line or auto-growing multi-line editor.

// Each allow below is for a lint that fires only on gpui's element and action
// shapes; correctness, perf and suspicious lints stay on.
#![allow(
    clippy::cast_precision_loss,
    clippy::fn_params_excessive_bools,
    clippy::module_name_repetitions,
    clippy::needless_pass_by_value,
    clippy::struct_excessive_bools
)]

mod actions;
mod element;
mod field;
mod history;
mod number;
mod state;

pub use actions::{
    Backspace, Copy, Cut, Delete, DeleteToBeginningOfLine, DeleteToEndOfLine, DeleteToNextWordEnd,
    DeleteToPreviousWordStart, Enter, Escape, IndentInline, MoveDown, MoveEnd, MoveHome, MoveLeft,
    MoveRight, MoveToEnd, MoveToNextWord, MoveToPreviousWord, MoveToStart, MoveUp, OutdentInline,
    Paste, Redo, SelectAll, SelectDown, SelectLeft, SelectRight, SelectToEnd, SelectToEndOfLine,
    SelectToNextWordEnd, SelectToPreviousWordStart, SelectToStart, SelectToStartOfLine, SelectUp,
    ShowCharacterPalette, Undo,
};
pub use field::{Input, InputContentType};
pub use gpui::TextAlign;
pub use number::NumberInput;
pub use state::{InputEvent, InputState};

pub(crate) fn init(cx: &mut gpui::App) {
    actions::init(cx);
    number::init(cx);
}
