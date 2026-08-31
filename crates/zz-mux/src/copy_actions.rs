//! The pinned `window-copy` action vocabulary.
//!
//! Every name in the pin's `window_copy_cmd_table` is listed here with the
//! behaviour category that owns it and the pin's `WINDOW_COPY_CMD_FLAG_READONLY`
//! bit. Support is derived from the `send-keys -X` parser rather than stored,
//! so a newly mapped action shrinks the missing set without a second edit.

use CopyActionCategory::{
    CopyFormatAndDestination, CursorGeometry, GotoLine, JumpPagePrompt, LogicalLineAndModeKeys,
    SelectionLifecycle, Vocabulary,
};

use crate::command::copy_mode_probe_action;

/// Which `copy-mode.action-fidelity` item owns a pinned action's behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CopyActionCategory {
    /// Names no behaviour item has claimed yet: search entry points, the
    /// refresh toggles, and the client-owned mouse scroll.
    Vocabulary,
    CursorGeometry,
    LogicalLineAndModeKeys,
    GotoLine,
    SelectionLifecycle,
    JumpPagePrompt,
    CopyFormatAndDestination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinnedCopyAction {
    pub name: &'static str,
    pub category: CopyActionCategory,
    pub read_only: bool,
}

const fn entry(
    name: &'static str,
    category: CopyActionCategory,
    read_only: bool,
) -> PinnedCopyAction {
    PinnedCopyAction {
        name,
        category,
        read_only,
    }
}

pub const PINNED_COPY_MODE_ACTIONS: &[PinnedCopyAction] = &[
    entry("append-selection", CopyFormatAndDestination, false),
    entry(
        "append-selection-and-cancel",
        CopyFormatAndDestination,
        false,
    ),
    entry("back-to-indentation", LogicalLineAndModeKeys, true),
    entry("begin-selection", SelectionLifecycle, false),
    entry("bottom-line", CursorGeometry, true),
    entry("cancel", JumpPagePrompt, true),
    entry("clear-selection", SelectionLifecycle, false),
    entry("copy-end-of-line", CopyFormatAndDestination, false),
    entry(
        "copy-end-of-line-and-cancel",
        CopyFormatAndDestination,
        false,
    ),
    entry("copy-line", CopyFormatAndDestination, false),
    entry("copy-line-and-cancel", CopyFormatAndDestination, false),
    entry("copy-pipe", CopyFormatAndDestination, false),
    entry("copy-pipe-and-cancel", CopyFormatAndDestination, false),
    entry("copy-pipe-end-of-line", CopyFormatAndDestination, false),
    entry(
        "copy-pipe-end-of-line-and-cancel",
        CopyFormatAndDestination,
        false,
    ),
    entry("copy-pipe-line", CopyFormatAndDestination, false),
    entry("copy-pipe-line-and-cancel", CopyFormatAndDestination, false),
    entry("copy-pipe-no-clear", CopyFormatAndDestination, false),
    entry("copy-selection", CopyFormatAndDestination, false),
    entry("copy-selection-and-cancel", CopyFormatAndDestination, false),
    entry("copy-selection-no-clear", CopyFormatAndDestination, false),
    entry("cursor-centre-horizontal", CursorGeometry, true),
    entry("cursor-centre-vertical", CursorGeometry, true),
    entry("cursor-down", CursorGeometry, true),
    entry("cursor-down-and-cancel", JumpPagePrompt, true),
    entry("cursor-left", CursorGeometry, true),
    entry("cursor-right", CursorGeometry, true),
    entry("cursor-up", CursorGeometry, true),
    entry("end-of-line", LogicalLineAndModeKeys, true),
    entry("goto-line", GotoLine, true),
    entry("halfpage-down", JumpPagePrompt, true),
    entry("halfpage-down-and-cancel", JumpPagePrompt, true),
    entry("halfpage-up", JumpPagePrompt, true),
    entry("history-bottom", CursorGeometry, true),
    entry("history-top", CursorGeometry, true),
    entry("jump-again", JumpPagePrompt, false),
    entry("jump-backward", JumpPagePrompt, false),
    entry("jump-forward", JumpPagePrompt, false),
    entry("jump-reverse", JumpPagePrompt, false),
    entry("jump-to-backward", JumpPagePrompt, false),
    entry("jump-to-forward", JumpPagePrompt, false),
    entry("jump-to-mark", JumpPagePrompt, true),
    entry("middle-line", CursorGeometry, true),
    entry("next-matching-bracket", JumpPagePrompt, true),
    entry("next-paragraph", LogicalLineAndModeKeys, true),
    entry("next-prompt", JumpPagePrompt, true),
    entry("next-space", LogicalLineAndModeKeys, true),
    entry("next-space-end", LogicalLineAndModeKeys, true),
    entry("next-word", LogicalLineAndModeKeys, true),
    entry("next-word-end", LogicalLineAndModeKeys, true),
    entry("other-end", SelectionLifecycle, false),
    entry("page-down", JumpPagePrompt, true),
    entry("page-down-and-cancel", JumpPagePrompt, true),
    entry("page-up", JumpPagePrompt, true),
    entry("pipe", CopyFormatAndDestination, false),
    entry("pipe-and-cancel", CopyFormatAndDestination, false),
    entry("pipe-no-clear", CopyFormatAndDestination, false),
    entry("previous-matching-bracket", JumpPagePrompt, true),
    entry("previous-paragraph", LogicalLineAndModeKeys, true),
    entry("previous-prompt", JumpPagePrompt, true),
    entry("previous-space", LogicalLineAndModeKeys, true),
    entry("previous-word", LogicalLineAndModeKeys, true),
    entry("recentre-top-bottom", CursorGeometry, true),
    entry("rectangle-off", SelectionLifecycle, false),
    entry("rectangle-on", SelectionLifecycle, false),
    entry("rectangle-toggle", SelectionLifecycle, false),
    entry("refresh-off", Vocabulary, true),
    entry("refresh-on", Vocabulary, true),
    entry("refresh-toggle", Vocabulary, true),
    entry("scroll-bottom", CursorGeometry, true),
    entry("scroll-down", JumpPagePrompt, true),
    entry("scroll-down-and-cancel", JumpPagePrompt, true),
    entry("scroll-exit-off", JumpPagePrompt, false),
    entry("scroll-exit-on", JumpPagePrompt, false),
    entry("scroll-exit-toggle", JumpPagePrompt, false),
    entry("scroll-middle", CursorGeometry, true),
    entry("scroll-to-mouse", Vocabulary, true),
    entry("scroll-top", CursorGeometry, true),
    entry("scroll-up", JumpPagePrompt, true),
    entry("search-again", Vocabulary, false),
    entry("search-backward", Vocabulary, false),
    entry("search-backward-incremental", Vocabulary, false),
    entry("search-backward-text", Vocabulary, false),
    entry("search-forward", Vocabulary, false),
    entry("search-forward-incremental", Vocabulary, false),
    entry("search-forward-text", Vocabulary, false),
    entry("search-reverse", Vocabulary, false),
    entry("select-line", SelectionLifecycle, false),
    entry("select-word", SelectionLifecycle, false),
    entry("selection-mode", SelectionLifecycle, false),
    entry("set-mark", JumpPagePrompt, true),
    entry("start-of-line", LogicalLineAndModeKeys, true),
    entry("stop-selection", SelectionLifecycle, false),
    entry("toggle-position", CursorGeometry, true),
    entry("top-line", CursorGeometry, true),
];

#[must_use]
pub fn pinned_copy_action(name: &str) -> Option<&'static PinnedCopyAction> {
    PINNED_COPY_MODE_ACTIONS
        .binary_search_by(|entry| entry.name.cmp(name))
        .ok()
        .map(|index| &PINNED_COPY_MODE_ACTIONS[index])
}

#[must_use]
pub fn copy_mode_action_is_mapped(name: &str) -> bool {
    copy_mode_probe_action(name).is_some()
}

#[must_use]
pub fn missing_copy_mode_actions() -> Vec<&'static PinnedCopyAction> {
    PINNED_COPY_MODE_ACTIONS
        .iter()
        .filter(|entry| !copy_mode_action_is_mapped(entry.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        CopyActionCategory, PINNED_COPY_MODE_ACTIONS, copy_mode_action_is_mapped,
        missing_copy_mode_actions, pinned_copy_action,
    };
    use crate::command::{copy_mode_action_is_read_only_safe, copy_mode_probe_action};

    #[test]
    fn the_inventory_holds_every_pinned_action_name_once_in_sorted_order() {
        assert_eq!(PINNED_COPY_MODE_ACTIONS.len(), 95);
        for pair in PINNED_COPY_MODE_ACTIONS.windows(2) {
            assert!(
                pair[0].name < pair[1].name,
                "{} vs {}",
                pair[0].name,
                pair[1].name
            );
        }
        for entry in PINNED_COPY_MODE_ACTIONS {
            assert_eq!(pinned_copy_action(entry.name), Some(entry));
        }
        assert_eq!(pinned_copy_action("search-forward-cursor-word"), None);
    }

    #[test]
    fn mapped_actions_carry_the_pins_read_only_classification() {
        let mapped = PINNED_COPY_MODE_ACTIONS
            .iter()
            .filter(|entry| copy_mode_action_is_mapped(entry.name))
            .count();
        assert_eq!(mapped, 79);
        for entry in PINNED_COPY_MODE_ACTIONS {
            let Some(action) = copy_mode_probe_action(entry.name) else {
                continue;
            };
            assert_eq!(
                copy_mode_action_is_read_only_safe(&action),
                entry.read_only,
                "{}",
                entry.name
            );
        }
    }

    #[test]
    fn missing_actions_stay_explicit_in_their_categories() {
        let mut missing: BTreeMap<CopyActionCategory, Vec<&str>> = BTreeMap::new();
        for entry in missing_copy_mode_actions() {
            missing.entry(entry.category).or_default().push(entry.name);
        }
        let expected: BTreeMap<CopyActionCategory, Vec<&str>> = BTreeMap::from([
            (
                CopyActionCategory::Vocabulary,
                vec![
                    "refresh-off",
                    "refresh-on",
                    "refresh-toggle",
                    "scroll-to-mouse",
                    "search-backward",
                    "search-backward-incremental",
                    "search-backward-text",
                    "search-forward",
                    "search-forward-incremental",
                    "search-forward-text",
                ],
            ),
            (
                CopyActionCategory::CursorGeometry,
                vec!["recentre-top-bottom"],
            ),
            (
                CopyActionCategory::JumpPagePrompt,
                vec!["previous-matching-bracket"],
            ),
            (
                CopyActionCategory::CopyFormatAndDestination,
                vec![
                    "copy-line",
                    "copy-line-and-cancel",
                    "copy-pipe-line",
                    "copy-pipe-line-and-cancel",
                ],
            ),
        ]);
        assert_eq!(missing, expected);
    }
}
