//! Keyboard actions for the text field, and the bindings that produce them.

use gpui::{Action, App, KeyBinding, actions};
use serde::Deserialize;

pub(super) const CONTEXT: &str = "ZzInput";

/// `enter`, carrying whether Shift was held. A multi-line field with
/// `submit_on_enter` treats plain Enter as submit and `shift-enter` as "insert a
/// newline".
// The `Action` derive expands to gpui's `unsafe`, not ours.
#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Clone, Action, PartialEq, Eq, Deserialize)]
#[action(namespace = zz_input, no_json)]
pub struct Enter {
    pub shift: bool,
}

actions!(
    zz_input,
    [
        Backspace,
        Copy,
        Cut,
        Delete,
        DeleteToBeginningOfLine,
        DeleteToEndOfLine,
        DeleteToNextWordEnd,
        DeleteToPreviousWordStart,
        Escape,
        IndentInline,
        MoveDown,
        MoveEnd,
        MoveHome,
        MoveLeft,
        MoveRight,
        MoveToEnd,
        MoveToNextWord,
        MoveToPreviousWord,
        MoveToStart,
        MoveUp,
        OutdentInline,
        Paste,
        Redo,
        SelectAll,
        SelectDown,
        SelectLeft,
        SelectRight,
        SelectToEnd,
        SelectToEndOfLine,
        SelectToNextWordEnd,
        SelectToPreviousWordStart,
        SelectToStart,
        SelectToStartOfLine,
        SelectUp,
        ShowCharacterPalette,
        Undo,
    ]
);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("shift-backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new("shift-delete", Delete, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-backspace", DeleteToBeginningOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-delete", DeleteToEndOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-backspace", DeleteToPreviousWordStart, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-backspace", DeleteToPreviousWordStart, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-delete", DeleteToNextWordEnd, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-delete", DeleteToNextWordEnd, Some(CONTEXT)),
        KeyBinding::new("enter", Enter { shift: false }, Some(CONTEXT)),
        KeyBinding::new("shift-enter", Enter { shift: true }, Some(CONTEXT)),
        KeyBinding::new("escape", Escape, Some(CONTEXT)),
        KeyBinding::new("left", MoveLeft, Some(CONTEXT)),
        KeyBinding::new("right", MoveRight, Some(CONTEXT)),
        KeyBinding::new("up", MoveUp, Some(CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(CONTEXT)),
        KeyBinding::new("home", MoveHome, Some(CONTEXT)),
        KeyBinding::new("end", MoveEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("ctrl-a", MoveHome, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("ctrl-e", MoveEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-left", MoveHome, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-right", MoveEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-up", MoveToStart, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-down", MoveToEnd, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-home", MoveToStart, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-end", MoveToEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-left", MoveToPreviousWord, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-right", MoveToNextWord, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-left", MoveToPreviousWord, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-right", MoveToNextWord, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("shift-home", SelectToStartOfLine, Some(CONTEXT)),
        KeyBinding::new("shift-end", SelectToEndOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("ctrl-shift-a", SelectToStartOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("ctrl-shift-e", SelectToEndOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("shift-cmd-left", SelectToStartOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("shift-cmd-right", SelectToEndOfLine, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("shift-cmd-up", SelectToStart, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("shift-cmd-down", SelectToEnd, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-shift-home", SelectToStart, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-shift-end", SelectToEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-shift-left", SelectToPreviousWordStart, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-shift-left", SelectToPreviousWordStart, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("alt-shift-right", SelectToNextWordEnd, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-shift-right", SelectToNextWordEnd, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-a", SelectAll, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-a", SelectAll, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-x", Cut, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-x", Cut, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-v", Paste, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-z", Undo, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("cmd-shift-z", Redo, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-z", Undo, Some(CONTEXT)),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        KeyBinding::new("ctrl-y", Redo, Some(CONTEXT)),
        KeyBinding::new("tab", IndentInline, Some(CONTEXT)),
        KeyBinding::new("shift-tab", OutdentInline, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some(CONTEXT)),
    ]);
}
