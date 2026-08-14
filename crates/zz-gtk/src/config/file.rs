//! `zz/config` on disk: where it lives, how it is read, and the
//! comment-preserving atomic edit every write goes through.
//!
//! Lifted from the desktop's `crates/zz/src/config/mod.rs`, whose parser,
//! writer and poll stamp are gpui-free. Both clients edit the same file, so
//! the byte-level rules — inline-comment boundary, last-occurrence wins,
//! newline dialect, the 64 KiB cap — have to agree exactly rather than be
//! re-derived. Nothing here touches GTK, so it is all testable headless.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Read as _, Write as _},
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

const CONFIG_DIRECTORY_NAME: &str = "zz";
const CONFIG_FILE_NAME: &str = "config";

/// The desktop's cap, restated: a config past it is refused rather than
/// silently truncated, and an edit that would cross it never reaches the disk.
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

/// How often the file is re-stamped. The desktop polls at the same cadence and
/// both clients treat the poll as the single apply path.
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn nonempty_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Every location `zz/config` may live in, most specific first. Identical to
/// the desktop's order and to `zz-daemon`'s `zz/mux.conf` order.
#[must_use]
pub fn candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_candidate(&mut candidates, nonempty_env("XDG_CONFIG_HOME").as_deref());
    let home = nonempty_env("HOME");
    push_home_candidate(&mut candidates, home.as_deref());
    if cfg!(target_os = "macos")
        && let Some(home) = home.as_deref()
    {
        push_candidate_path(&mut candidates, &home.join("Library/Application Support"));
    }
    candidates
}

fn push_home_candidate(candidates: &mut Vec<PathBuf>, home: Option<&Path>) {
    if let Some(home) = home {
        push_candidate_path(candidates, &home.join(".config"));
    }
}

fn push_candidate(candidates: &mut Vec<PathBuf>, base: Option<&Path>) {
    if let Some(base) = base {
        push_candidate_path(candidates, base);
    }
}

fn push_candidate_path(candidates: &mut Vec<PathBuf>, base: &Path) {
    if !base.is_absolute() {
        return;
    }
    let candidate = base.join(CONFIG_DIRECTORY_NAME).join(CONFIG_FILE_NAME);
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn discover(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
}

/// The existing config, or the path one would be created at. A GUI edit and a
/// hand edit therefore always land on the same file.
pub fn path_for_write() -> io::Result<PathBuf> {
    let candidates = candidates();
    if let Some(path) = discover(&candidates) {
        return Ok(path);
    }
    candidates.into_iter().next().ok_or_else(|| {
        io::Error::new(
            ErrorKind::NotFound,
            "cannot create zz/config because neither XDG_CONFIG_HOME nor HOME is available",
        )
    })
}

pub fn read_source(path: &Path) -> io::Result<String> {
    read_bounded(path, MAX_CONFIG_BYTES)
}

/// Read a whole editor-backed file, capped at `max_bytes`. A missing file
/// starts as an empty buffer rather than an error.
pub fn read_editor_source(path: &Path, max_bytes: usize) -> io::Result<String> {
    match read_bounded(path, max_bytes) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
        other => other,
    }
}

fn read_bounded(path: &Path, max_bytes: usize) -> io::Result<String> {
    let file = File::open(path)?;
    let byte_limit = u64::try_from(max_bytes).unwrap_or(u64::MAX - 1);
    let mut source = String::new();
    file.take(byte_limit + 1).read_to_string(&mut source)?;
    if source.len() > max_bytes {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration exceeds the {max_bytes}-byte limit"),
        ));
    }
    Ok(source)
}

/// Atomically replace an editor-backed file after enforcing its cap.
pub fn write_editor_source(path: &Path, source: &str, max_bytes: usize) -> io::Result<()> {
    if source.len() > max_bytes {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration exceeds the {max_bytes}-byte editor limit"),
        ));
    }
    atomic_write(path, source.as_bytes())
}

/// Set one key in `zz/config`. Values are single-line by construction: the
/// file has no continuation syntax, so an embedded newline would silently
/// become a second, malformed entry.
pub fn set_key(key: &str, value: &str) -> io::Result<()> {
    set_key_at(&path_for_write()?, key, value)
}

/// Reset one key to its built-in default by deleting its line outright —
/// key, value and trailing comment. Every other byte of the file survives.
pub fn remove_key(key: &str) -> io::Result<()> {
    remove_key_at(&path_for_write()?, key)
}

pub fn set_key_at(path: &Path, key: &str, value: &str) -> io::Result<()> {
    if value.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "configuration values must fit on one line",
        ));
    }
    write_edit_at(path, key, Some(value)).map(drop)
}

pub fn remove_key_at(path: &Path, key: &str) -> io::Result<()> {
    write_edit_at(path, key, None).map(drop)
}

/// Delete every line a key has, not only the effective one. What removing a
/// repeatable entry needs: dropping the last `host-desktop` would promote an
/// earlier one rather than remove the host.
pub fn remove_key_group(key: &str) -> io::Result<()> {
    remove_key_group_at(&path_for_write()?, key)
}

pub fn remove_key_group_at(path: &Path, key: &str) -> io::Result<()> {
    let source = match read_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let edited = replace_key_group(&source, key, &[]);
    if edited == source {
        return Ok(());
    }
    atomic_write(path, edited.as_bytes())
}

fn write_edit_at(path: &Path, key: &str, value: Option<&str>) -> io::Result<bool> {
    let source = match read_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let edited = edit_source(&source, key, value);
    if edited == source {
        return Ok(false);
    }
    if edited.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration edit exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    atomic_write(path, edited.as_bytes())?;
    Ok(true)
}

/// Splice one key's line, never the file. `None` removes it; a new key is
/// appended. The last occurrence is the target because later entries win, so
/// removing it promotes an earlier duplicate rather than the default.
pub fn edit_source(source: &str, key: &str, value: Option<&str>) -> String {
    let last_line = last_key_line_range(source, key);
    match (last_line, value) {
        (Some(line), Some(value)) => {
            let replacement = replace_line_value(&source[line.clone()], value);
            let mut edited =
                String::with_capacity(source.len() + replacement.len().saturating_sub(line.len()));
            edited.push_str(&source[..line.start]);
            edited.push_str(&replacement);
            edited.push_str(&source[line.end..]);
            edited
        }
        (Some(line), None) => {
            let mut edited = String::with_capacity(source.len().saturating_sub(line.len()));
            edited.push_str(&source[..line.start]);
            edited.push_str(&source[line.end..]);
            edited
        }
        (None, Some(value)) => append_line(source, key, value),
        (None, None) => source.to_owned(),
    }
}

fn last_key_line_range(source: &str, key: &str) -> Option<Range<usize>> {
    let mut offset = 0;
    let mut last = None;
    for line in source.split_inclusive('\n') {
        let end = offset + line.len();
        if key_for_line(line) == Some(key) {
            last = Some(offset..end);
        }
        offset = end;
    }
    last
}

pub fn key_for_line(line: &str) -> Option<&str> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    Some(key.trim())
}

/// Overwrite only the value span, so the key's own spelling, the surrounding
/// whitespace, any trailing comment and the line ending all survive.
fn replace_line_value(line: &str, value: &str) -> String {
    let without_lf = line.strip_suffix('\n').unwrap_or(line);
    let body = without_lf.strip_suffix('\r').unwrap_or(without_lf);
    let equals = body
        .find('=')
        .expect("a matched configuration line has an equals sign");
    let value_area_start = equals + 1;
    let comment_start = comment_start(&body[value_area_start..])
        .map_or(body.len(), |index| value_area_start + index);
    let value_area = &body[value_area_start..comment_start];
    let (value_start, value_end) = if value_area.trim().is_empty() {
        (comment_start, comment_start)
    } else {
        (
            value_area_start + value_area.len() - value_area.trim_start().len(),
            value_area_start + value_area.trim_end().len(),
        )
    };

    let mut replacement = String::with_capacity(line.len() + value.len());
    replacement.push_str(&line[..value_start]);
    replacement.push_str(value);
    replacement.push_str(&line[value_end..]);
    replacement
}

fn append_line(source: &str, key: &str, value: &str) -> String {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut edited = String::with_capacity(source.len() + key.len() + value.len() + 4);
    edited.push_str(source);
    if !source.is_empty() && !source.ends_with('\n') {
        edited.push_str(newline);
    }
    edited.push_str(key);
    edited.push_str(" = ");
    edited.push_str(value);
    edited.push_str(newline);
    edited
}

/// Replace every occurrence of `key` with `values`, appended in order. What an
/// import needs: a key the donor repeats — `palette`, `font-family` — has no
/// single effective line to splice, and an empty group is a pure removal.
pub fn replace_key_group(source: &str, key: &str, values: &[String]) -> String {
    let mut edited: String = source
        .split_inclusive('\n')
        .filter(|line| key_for_line(line) != Some(key))
        .collect();
    for value in values {
        edited = append_line(&edited, key, value);
    }
    edited
}

pub fn value_without_comment(value: &str) -> &str {
    comment_start(value).map_or(value, |index| &value[..index])
}

/// Where an inline comment starts, if any. A `#` only opens one when it is
/// unquoted, preceded by whitespace, and follows a non-empty value — which is
/// what keeps `background = #112233` and tmux word separators intact.
fn comment_start(value: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut saw_value = false;
    let mut previous_was_whitespace = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            saw_value |= !character.is_whitespace();
            previous_was_whitespace = character.is_whitespace();
            continue;
        }
        match character {
            '\\' if quote.is_some() => escaped = true,
            '"' | '\'' if quote == Some(character) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(character),
            '#' if quote.is_none() && saw_value && previous_was_whitespace => {
                return Some(index);
            }
            _ => {}
        }
        saw_value |= !character.is_whitespace();
        previous_was_whitespace = character.is_whitespace();
    }
    None
}

/// Replace a file through a temporary sibling and a rename, so a reader — the
/// poller of another client included — never observes a half-written config.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "configuration path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let (temporary_path, mut temporary_file) = create_temp_file(path, parent)?;
    let write_result = (|| {
        if let Ok(metadata) = fs::metadata(path) {
            temporary_file.set_permissions(metadata.permissions())?;
        }
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()
    })();
    drop(temporary_file);

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(())
}

fn create_temp_file(path: &Path, parent: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(CONFIG_FILE_NAME))
        .to_string_lossy();
    for _ in 0..128 {
        let nonce = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary_path =
            parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique temporary configuration file",
    ))
}

/// What the poller compares. Path, mtime and length together: a rename between
/// candidate directories changes the path, and an in-place edit that keeps the
/// length still moves the mtime.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Stamp {
    pub path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: Option<u64>,
}

impl Stamp {
    #[must_use]
    pub fn detect(candidates: &[PathBuf]) -> Self {
        let Some(path) = discover(candidates) else {
            return Self::default();
        };
        let metadata = fs::metadata(&path).ok();
        Self {
            modified: metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok()),
            len: metadata.as_ref().map(fs::Metadata::len),
            path: Some(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private directory per test, without pulling in a temp-file crate for
    /// four assertions' worth of scratch space.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let nonce = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!("zz-gtk-config-{name}-{nonce}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("scratch directory");
            Self(path)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const COMMENTED: &str = "\
# zz configuration

pane-margin = 8   # keep this comment
background = #112233
future syntax without equals
pane-gaps = false
";

    #[test]
    fn setting_a_key_rewrites_only_its_value() {
        let edited = edit_source(COMMENTED, "pane-margin", Some("12"));

        assert_eq!(
            edited,
            "\
# zz configuration

pane-margin = 12   # keep this comment
background = #112233
future syntax without equals
pane-gaps = false
"
        );
    }

    #[test]
    fn resetting_a_key_deletes_its_whole_line() {
        let edited = edit_source(COMMENTED, "pane-margin", None);

        assert_eq!(
            edited,
            "\
# zz configuration

background = #112233
future syntax without equals
pane-gaps = false
"
        );
    }

    #[test]
    fn an_absent_key_is_appended_in_the_files_own_newline_dialect() {
        assert_eq!(
            edit_source("pane-gaps = true\n", "pane-margin", Some("4")),
            "pane-gaps = true\npane-margin = 4\n"
        );
        assert_eq!(
            edit_source("pane-gaps = true\r\n", "pane-margin", Some("4")),
            "pane-gaps = true\r\npane-margin = 4\r\n"
        );
        assert_eq!(
            edit_source("pane-gaps = true", "pane-margin", Some("4")),
            "pane-gaps = true\npane-margin = 4\n"
        );
    }

    #[test]
    fn the_last_occurrence_is_the_one_edited() {
        let source = "prefix = C-a\nprefix = C-b\n";

        assert_eq!(
            edit_source(source, "prefix", Some("C-z")),
            "prefix = C-a\nprefix = C-z\n"
        );
        assert_eq!(edit_source(source, "prefix", None), "prefix = C-a\n");
    }

    #[test]
    fn a_commented_out_key_is_never_the_target() {
        let source = "# pane-margin = 8\n";

        assert_eq!(
            edit_source(source, "pane-margin", Some("4")),
            "# pane-margin = 8\npane-margin = 4\n"
        );
    }

    #[test]
    fn a_hash_only_opens_a_comment_after_whitespace_and_a_value() {
        assert_eq!(
            value_without_comment(" #112233 # trailing").trim(),
            "#112233"
        );
        assert_eq!(value_without_comment(" !@#").trim(), "!@#");
        assert_eq!(
            value_without_comment(" sh -c 'printf #copied' # trailing").trim(),
            "sh -c 'printf #copied'"
        );
    }

    #[test]
    fn a_group_replacement_drops_every_prior_occurrence() {
        let source = "palette = 0=#000000\npane-gaps = true\npalette = 1=#111111\n";

        assert_eq!(
            replace_key_group(source, "palette", &["2=#222222".to_owned()]),
            "pane-gaps = true\npalette = 2=#222222\n"
        );
        assert_eq!(
            replace_key_group(source, "palette", &[]),
            "pane-gaps = true\n"
        );
    }

    #[test]
    fn an_atomic_write_replaces_the_file_and_leaves_no_temporary_behind() {
        let scratch = Scratch::new("atomic");
        let path = scratch.file("nested/zz/config");

        atomic_write(&path, b"pane-gaps = true\n").expect("first write");
        atomic_write(&path, b"pane-gaps = false\n").expect("second write");

        assert_eq!(
            fs::read_to_string(&path).expect("read back"),
            "pane-gaps = false\n"
        );
        let siblings: Vec<_> = fs::read_dir(path.parent().expect("parent"))
            .expect("list")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(siblings.len(), 1, "a temporary file survived: {siblings:?}");
    }

    #[test]
    fn a_source_past_the_cap_is_refused_rather_than_truncated() {
        let scratch = Scratch::new("cap");
        let path = scratch.file("config");
        atomic_write(&path, &vec![b'#'; MAX_CONFIG_BYTES + 1]).expect("write oversized");

        let error = read_source(&path).expect_err("an oversized config must fail");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn the_stamp_moves_when_the_file_is_touched_or_replaced_by_a_better_candidate() {
        let scratch = Scratch::new("stamp");
        let preferred = scratch.file("preferred/zz/config");
        let fallback = scratch.file("fallback/zz/config");
        let candidates = vec![preferred.clone(), fallback.clone()];

        assert_eq!(Stamp::detect(&candidates), Stamp::default());

        atomic_write(&fallback, b"pane-gaps = true\n").expect("write fallback");
        let first = Stamp::detect(&candidates);
        assert_eq!(first.path.as_deref(), Some(fallback.as_path()));

        atomic_write(&fallback, b"pane-gaps = false\n").expect("rewrite fallback");
        let second = Stamp::detect(&candidates);
        assert_ne!(second, first);

        atomic_write(&preferred, b"pane-gaps = true\n").expect("write preferred");
        let third = Stamp::detect(&candidates);
        assert_eq!(third.path.as_deref(), Some(preferred.as_path()));
        assert_ne!(third, second);
    }
}
