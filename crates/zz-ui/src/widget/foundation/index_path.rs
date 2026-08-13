//! Addressing a cell in a sectioned list.

use std::fmt::Display;

use gpui::ElementId;

/// A section / row / column address. All three default to `0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexPath {
    pub section: usize,
    pub row: usize,
    pub column: usize,
}

impl From<IndexPath> for ElementId {
    fn from(path: IndexPath) -> Self {
        ElementId::Name(format!("index-path({},{},{})", path.section, path.row, path.column).into())
    }
}

impl Display for IndexPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "IndexPath(section: {}, row: {}, column: {})",
            self.section, self.row, self.column
        )
    }
}

impl IndexPath {
    /// A path to `row` in section `0`, column `0`.
    pub fn new(row: usize) -> Self {
        IndexPath {
            section: 0,
            row,
            ..Default::default()
        }
    }

    pub fn section(mut self, section: usize) -> Self {
        self.section = section;
        self
    }

    pub fn row(mut self, row: usize) -> Self {
        self.row = row;
        self
    }

    pub fn column(mut self, column: usize) -> Self {
        self.column = column;
        self
    }
}
