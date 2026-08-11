//! Three coordinate spaces: buffer lines, wrap rows (soft wrap), display rows (folding).
mod display_map;
mod fold_map;
#[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
mod folding;
#[cfg(any(not(feature = "tree-sitter"), target_family = "wasm"))]
pub mod folding;
mod text_wrapper;
mod wrap_map;

pub use self::display_map::DisplayMap;
pub(crate) use self::text_wrapper::LineLayout;

#[cfg(any(not(feature = "tree-sitter"), target_family = "wasm"))]
#[allow(unused_imports)]
pub use folding::Tree;
#[allow(unused_imports)]
pub use folding::{FoldRange, extract_fold_ranges};

/// Buffer position: 0-based logical line, 0-based byte column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferPoint {
    pub line: usize,
    pub col: usize,
}

impl BufferPoint {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct WrapPoint {
    pub row: usize,
    pub col: usize,
}

impl WrapPoint {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

/// Display position after soft wrap and folding: 0-based row and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayPoint {
    pub row: usize,
    pub col: usize,
}

impl DisplayPoint {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}
