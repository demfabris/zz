use serde::{Deserialize, Serialize};

pub const MAX_PATH_LIST_TEXT_BYTES: usize = 4096;
pub const MAX_PATH_LIST_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_PATH_LIST_ENTRIES: usize = 50_000;
pub const MAX_PATH_LIST_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathKind {
    File,
    Dir,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathEntry {
    pub rel: String,
    pub kind: PathKind,
    pub symlink: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitMark {
    Modified,
    Added,
    Untracked,
    Conflicted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellKind {
    Posix,
    Fish,
    Pwsh,
    Nu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsertStyle {
    Shell(ShellKind),
    Claude,
    Codex,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathListRoot {
    pub root: String,
    pub display_root: String,
    pub cwd: Option<String>,
    pub insert: InsertStyle,
}
