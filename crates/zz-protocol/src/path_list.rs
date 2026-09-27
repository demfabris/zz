use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

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
    #[serde(deserialize_with = "deserialize_path_list_text")]
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
    #[serde(deserialize_with = "deserialize_path_list_text")]
    pub root: String,
    #[serde(deserialize_with = "deserialize_path_list_text")]
    pub display_root: String,
    #[serde(deserialize_with = "deserialize_optional_path_list_text")]
    pub cwd: Option<String>,
    pub insert: InsertStyle,
}

pub(crate) fn deserialize_path_list_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    crate::message::deserialize_bounded_text(deserializer, MAX_PATH_LIST_TEXT_BYTES)
}

pub(crate) fn deserialize_optional_path_list_text<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    crate::message::deserialize_bounded_optional_text(deserializer, MAX_PATH_LIST_TEXT_BYTES)
}

pub(crate) fn deserialize_path_list_result<'de, D>(
    deserializer: D,
) -> Result<Result<PathListRoot, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let result = Result::<PathListRoot, String>::deserialize(deserializer)?;
    if let Err(error) = &result
        && error.len() > MAX_PATH_LIST_TEXT_BYTES
    {
        return Err(D::Error::invalid_length(
            error.len(),
            &"a path listing error within the wire byte limit",
        ));
    }
    Ok(result)
}

pub(crate) fn deserialize_path_entries<'de, D>(deserializer: D) -> Result<Vec<PathEntry>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries = Vec::<PathEntry>::deserialize(deserializer)?;
    if entries.len() > MAX_PATH_LIST_ENTRIES {
        return Err(D::Error::invalid_length(
            entries.len(),
            &"a path listing chunk within the wire entry limit",
        ));
    }
    Ok(entries)
}

pub(crate) fn deserialize_git_marks<'de, D>(
    deserializer: D,
) -> Result<Vec<(String, GitMark)>, D::Error>
where
    D: Deserializer<'de>,
{
    let marks = Vec::<(String, GitMark)>::deserialize(deserializer)?;
    if marks.len() > MAX_PATH_LIST_ENTRIES {
        return Err(D::Error::invalid_length(
            marks.len(),
            &"a git mark batch within the wire entry limit",
        ));
    }
    if let Some((rel, _)) = marks
        .iter()
        .find(|(rel, _)| rel.len() > MAX_PATH_LIST_TEXT_BYTES)
    {
        return Err(D::Error::invalid_length(
            rel.len(),
            &"a git mark path within the wire byte limit",
        ));
    }
    Ok(marks)
}
