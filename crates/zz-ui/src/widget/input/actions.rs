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
        KeyBinding::new("tab", IndentInline, Some(CONTEXT)),
        KeyBinding::new("shift-tab", OutdentInline, Some(CONTEXT)),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some(CONTEXT)),
    ]);
    cx.bind_keys(edit_key_bindings());
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn edit_key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-z", Undo, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-z", Redo, Some(CONTEXT)),
        KeyBinding::new("cmd-x", Cut, Some(CONTEXT)),
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-v", Paste, Some(CONTEXT)),
        KeyBinding::new("cmd-a", SelectAll, Some(CONTEXT)),
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn edit_key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("ctrl-z", Undo, Some(CONTEXT)),
        KeyBinding::new("ctrl-y", Redo, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-z", Redo, Some(CONTEXT)),
        KeyBinding::new("ctrl-x", Cut, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
        KeyBinding::new("ctrl-v", Paste, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-v", Paste, Some(CONTEXT)),
        KeyBinding::new("ctrl-a", SelectAll, Some(CONTEXT)),
    ]
}

#[cfg(test)]
mod tests {
    use std::{any::TypeId, collections::HashSet};

    use gpui::{AsKeystroke, Keystroke};

    use super::*;

    #[test]
    fn text_edit_keymap_matches_the_audited_platform_set() {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let expected = [
            ("cmd-z", TypeId::of::<Undo>()),
            ("cmd-shift-z", TypeId::of::<Redo>()),
            ("cmd-x", TypeId::of::<Cut>()),
            ("cmd-c", TypeId::of::<Copy>()),
            ("cmd-v", TypeId::of::<Paste>()),
            ("cmd-shift-v", TypeId::of::<Paste>()),
            ("cmd-a", TypeId::of::<SelectAll>()),
        ];
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let expected = [
            ("ctrl-z", TypeId::of::<Undo>()),
            ("ctrl-y", TypeId::of::<Redo>()),
            ("ctrl-shift-z", TypeId::of::<Redo>()),
            ("ctrl-x", TypeId::of::<Cut>()),
            ("ctrl-c", TypeId::of::<Copy>()),
            ("ctrl-v", TypeId::of::<Paste>()),
            ("ctrl-shift-v", TypeId::of::<Paste>()),
            ("ctrl-a", TypeId::of::<SelectAll>()),
        ];
        let bindings = edit_key_bindings();
        let actual = bindings
            .iter()
            .map(|binding| {
                let [keystroke] = binding.keystrokes() else {
                    panic!("text edit bindings must be single keystrokes");
                };
                (
                    keystroke.as_keystroke().clone(),
                    binding.action().as_any().type_id(),
                )
            })
            .collect::<HashSet<_>>();
        let expected = expected
            .into_iter()
            .map(|(source, action)| {
                (
                    Keystroke::parse(source).expect("valid audited text edit keystroke"),
                    action,
                )
            })
            .collect::<HashSet<_>>();

        assert_eq!(actual, expected);
        assert_eq!(bindings.len(), expected.len());
    }
}
