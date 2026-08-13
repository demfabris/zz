//! Undo/redo for the text field.

use std::ops::Range;

use gpui::SharedString;

const MAX_ENTRIES: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum EditKind {
    Insert,
    Delete,
    Other,
}

impl EditKind {
    const fn groupable(self) -> bool {
        matches!(self, Self::Insert | Self::Delete)
    }
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub(super) text: SharedString,
    pub(super) selection: Range<usize>,
    pub(super) reversed: bool,
}

#[derive(Default)]
pub(super) struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    group: Option<EditKind>,
}

impl History {
    pub(super) fn push(&mut self, before: Snapshot, kind: EditKind) {
        self.redo.clear();

        if self.group == Some(kind) {
            return;
        }

        self.undo.push(before);
        if self.undo.len() > MAX_ENTRIES {
            self.undo.remove(0);
        }
        self.group = kind.groupable().then_some(kind);
    }

    pub(super) fn break_group(&mut self) {
        self.group = None;
    }

    pub(super) fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.group = None;
        Some(previous)
    }

    pub(super) fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.group = None;
        Some(next)
    }

    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.group = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(text: &str) -> Snapshot {
        Snapshot {
            text: text.into(),
            selection: text.len()..text.len(),
            reversed: false,
        }
    }

    #[test]
    fn typed_runs_collapse_into_one_step() {
        let mut history = History::default();
        history.push(snapshot(""), EditKind::Insert);
        history.push(snapshot("h"), EditKind::Insert);
        history.push(snapshot("he"), EditKind::Insert);

        let restored = history.undo(snapshot("hey")).expect("one step");
        assert_eq!(restored.text, "");
        assert!(history.undo(snapshot("")).is_none());
    }

    #[test]
    fn a_different_kind_starts_a_new_step() {
        let mut history = History::default();
        history.push(snapshot(""), EditKind::Insert);
        history.push(snapshot("hey"), EditKind::Delete);

        assert_eq!(history.undo(snapshot("he")).expect("delete").text, "hey");
        assert_eq!(history.undo(snapshot("hey")).expect("insert").text, "");
    }

    #[test]
    fn other_edits_never_coalesce() {
        let mut history = History::default();
        history.push(snapshot(""), EditKind::Other);
        history.push(snapshot("a"), EditKind::Other);

        assert_eq!(history.undo(snapshot("ab")).expect("second").text, "a");
        assert_eq!(history.undo(snapshot("a")).expect("first").text, "");
    }

    #[test]
    fn breaking_a_group_splits_a_typed_run() {
        let mut history = History::default();
        history.push(snapshot(""), EditKind::Insert);
        history.break_group();
        history.push(snapshot("ab"), EditKind::Insert);

        assert_eq!(history.undo(snapshot("abc")).expect("second").text, "ab");
        assert_eq!(history.undo(snapshot("ab")).expect("first").text, "");
    }

    #[test]
    fn redo_replays_what_undo_took_back() {
        let mut history = History::default();
        history.push(snapshot(""), EditKind::Other);

        let undone = history.undo(snapshot("hey")).expect("step");
        assert_eq!(undone.text, "");
        assert_eq!(history.redo(snapshot("")).expect("redo").text, "hey");
    }

    #[test]
    fn a_new_edit_drops_the_redo_stack() {
        let mut history = History::default();
        history.push(snapshot(""), EditKind::Other);
        history.undo(snapshot("hey"));

        history.push(snapshot(""), EditKind::Other);
        assert!(history.redo(snapshot("yo")).is_none());
    }
}
