use instant::{Duration, Instant};
use std::fmt::Debug;

pub trait HistoryItem: Clone + PartialEq {
    fn version(&self) -> usize;
    fn set_version(&mut self, version: usize);
}

#[derive(Debug)]
pub struct History<I: HistoryItem> {
    undos: Vec<I>,
    redos: Vec<I>,
    last_changed_at: Instant,
    version: usize,
    pub(crate) ignore: bool,
    max_undos: usize,
    group_interval: Option<Duration>,
    grouping: bool,
}

impl<I> History<I>
where
    I: HistoryItem,
{
    pub fn new() -> Self {
        Self {
            undos: Default::default(),
            redos: Default::default(),
            ignore: false,
            last_changed_at: Instant::now(),
            version: 0,
            max_undos: 1000,
            group_interval: None,
            grouping: false,
        }
    }

    /// Groups changes that land within this interval into one undo step.
    pub fn group_interval(mut self, group_interval: Duration) -> Self {
        self.group_interval = Some(group_interval);
        self
    }

    /// Freezes the version, so every change until `end_grouping` undoes as one.
    pub fn start_grouping(&mut self) {
        self.grouping = true;
    }

    /// Resumes version increments.
    pub fn end_grouping(&mut self) {
        self.grouping = false;
    }

    /// End the current edit group. The next pushed item starts a new version.
    pub fn break_group(&mut self) {
        self.grouping = false;
        self.version = self.version.saturating_add(1);
    }

    fn inc_version(&mut self) -> usize {
        let t = Instant::now();
        if !self.grouping && Some(self.last_changed_at.elapsed()) > self.group_interval {
            self.version += 1;
        }

        self.last_changed_at = t;
        self.version
    }

    pub fn version(&self) -> usize {
        self.version
    }

    pub fn push(&mut self, item: I) {
        let version = self.inc_version();

        if self.undos.len() >= self.max_undos {
            self.undos.remove(0);
        }

        let mut item = item;
        item.set_version(version);
        self.undos.push(item);
    }

    pub fn clear(&mut self) {
        self.undos.clear();
        self.redos.clear();
    }

    fn pop_group(stack: &mut Vec<I>) -> Option<Vec<I>> {
        let first_change = stack.pop()?;
        let version = first_change.version();
        let mut changes = vec![first_change];

        while stack
            .last()
            .is_some_and(|change| change.version() == version)
        {
            changes.push(stack.pop().expect("the matching change is at the top"));
        }

        Some(changes)
    }

    /// Undoes the newest version group and returns its changes.
    pub fn undo(&mut self) -> Option<Vec<I>> {
        let changes = Self::pop_group(&mut self.undos)?;
        self.redos.extend(changes.iter().cloned());
        Some(changes)
    }

    /// Redoes the newest undone version group and returns its changes.
    pub fn redo(&mut self) -> Option<Vec<I>> {
        let changes = Self::pop_group(&mut self.redos)?;
        self.undos.extend(changes.iter().cloned());
        Some(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TabIndex {
        tab_index: usize,
        version: usize,
    }

    impl PartialEq for TabIndex {
        fn eq(&self, other: &Self) -> bool {
            self.tab_index == other.tab_index
        }
    }

    impl From<usize> for TabIndex {
        fn from(value: usize) -> Self {
            TabIndex {
                tab_index: value,
                version: 0,
            }
        }
    }

    impl HistoryItem for TabIndex {
        fn version(&self) -> usize {
            self.version
        }
        fn set_version(&mut self, version: usize) {
            self.version = version;
        }
    }

    #[test]
    fn test_history() {
        let mut history: History<TabIndex> = History::new();
        history.push(0.into());
        history.push(3.into());
        history.push(2.into());
        history.push(1.into());

        assert_eq!(history.version(), 4);
        let changes = history.undo().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].tab_index, 1);

        let changes = history.undo().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].tab_index, 2);

        history.push(5.into());

        let changes = history.redo().unwrap();
        assert_eq!(changes[0].tab_index, 2);

        let changes = history.redo().unwrap();
        assert_eq!(changes[0].tab_index, 1);

        let changes = history.undo().unwrap();
        assert_eq!(changes[0].tab_index, 1);

        let changes = history.undo().unwrap();
        assert_eq!(changes[0].tab_index, 2);

        let changes = history.undo().unwrap();
        assert_eq!(changes[0].tab_index, 5);

        let changes = history.undo().unwrap();
        assert_eq!(changes[0].tab_index, 3);

        let changes = history.undo().unwrap();
        assert_eq!(changes[0].tab_index, 0);

        assert_eq!(history.undo().is_none(), true);
    }

    #[test]
    fn undo_and_redo_only_pop_the_contiguous_top_group() {
        fn item(tab_index: usize, version: usize) -> TabIndex {
            TabIndex { tab_index, version }
        }

        let mut history: History<TabIndex> = History::new();
        history.undos = vec![item(0, 2), item(1, 1), item(2, 2)];

        let undone = history.undo().unwrap();
        assert_eq!(
            undone.iter().map(|item| item.tab_index).collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(
            history
                .undos
                .iter()
                .map(|item| item.tab_index)
                .collect::<Vec<_>>(),
            [0, 1]
        );

        history.redos = vec![item(3, 2), item(4, 1), item(5, 2)];
        let redone = history.redo().unwrap();
        assert_eq!(
            redone.iter().map(|item| item.tab_index).collect::<Vec<_>>(),
            [5]
        );
        assert_eq!(
            history
                .redos
                .iter()
                .map(|item| item.tab_index)
                .collect::<Vec<_>>(),
            [3, 4]
        );
    }
}
