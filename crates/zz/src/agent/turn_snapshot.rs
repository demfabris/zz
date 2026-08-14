//! Git tree snapshots taken at prompt dispatch, backing the "changes this
//! turn" diff.
//!
//! A snapshot writes the tracked *and* untracked-unignored worktree into the
//! object database through a throwaway index, so the turn diff is a plain
//! tree-to-tree comparison: a file that was already untracked when the turn
//! started diffs correctly instead of reading as brand new.

// Consumed once the controller wires turn capture into prompt dispatch.
#![allow(dead_code)]

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// Ceiling on the unified patch; the pane renders a diff, not a repository.
const MAX_PATCH_BYTES: usize = 3 * 1024 * 1024;

/// Ceiling on the per-file summaries, which are a fraction of the patch.
const MAX_SUMMARY_BYTES: usize = 2 * 1024 * 1024;

const TRUNCATION_NOTICE: &str = "\n[diff truncated]\n";

/// The worktree as it stood at prompt dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TurnTree {
    pub(crate) root: PathBuf,
    pub(crate) tree: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TurnDiff {
    pub(crate) files: Vec<TurnFile>,
    pub(crate) patch: String,
    pub(crate) additions: u32,
    pub(crate) deletions: u32,
    pub(crate) truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TurnFile {
    pub(crate) path: String,
    pub(crate) old_path: Option<String>,
    pub(crate) status: TurnFileStatus,
    pub(crate) additions: u32,
    pub(crate) deletions: u32,
    pub(crate) binary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TurnFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Unmerged,
}

impl TurnFileStatus {
    const fn from_code(code: char) -> Self {
        match code {
            'A' => Self::Added,
            'D' => Self::Deleted,
            'R' => Self::Renamed,
            'C' => Self::Copied,
            'U' => Self::Unmerged,
            _ => Self::Modified,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
            Self::Copied => "copied",
            Self::Unmerged => "unmerged",
        }
    }
}

/// Record the worktree containing `cwd`, to diff against once the turn ends.
pub(crate) fn snapshot_tree(cwd: &Path) -> Result<TurnTree, String> {
    write_tree(&repo_root(cwd)?)
}

/// Diff `base` against a fresh snapshot of the worktree containing `cwd`.
pub(crate) fn capture_turn_diff(cwd: &Path, base: &TurnTree) -> Result<TurnDiff, String> {
    let root = repo_root(cwd)?;
    if root != base.root {
        return Err(format!(
            "the working directory moved from {} to {}, so this turn cannot be diffed",
            base.root.display(),
            root.display()
        ));
    }
    let current = write_tree(&root)?;
    diff_trees(&root, &base.tree, &current.tree, MAX_PATCH_BYTES)
}

fn repo_root(cwd: &Path) -> Result<PathBuf, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git could not start: {error}"))?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || root.is_empty() {
        return Err(format!("{} is not inside a git worktree", cwd.display()));
    }
    Ok(PathBuf::from(root))
}

fn write_tree(root: &Path) -> Result<TurnTree, String> {
    // git writes `index.lock` beside the index, so the throwaway index gets a
    // directory of its own; dropping it clears both on every path out.
    let scratch = tempfile::tempdir()
        .map_err(|error| format!("no scratch directory for the turn snapshot: {error}"))?;
    let index = scratch.path().join("index");

    let added = run_with_index(root, &["add", "-A", "--ignore-errors", "."], &index)?;
    if !added.status.success() {
        log::debug!(
            target: "zz::agent",
            "git add reported errors snapshotting {}: {}",
            root.display(),
            stderr_of(&added)
        );
    }
    // A missing `GIT_INDEX_FILE` reads as an empty index, so a hard `git add`
    // failure would leave write-tree handing back the empty tree and calling
    // the whole worktree deleted.
    if !index.exists() {
        return Err(format!(
            "git add wrote no index for {}: {}",
            root.display(),
            stderr_of(&added)
        ));
    }

    let written = run_with_index(root, &["write-tree"], &index)?;
    if !written.status.success() {
        return Err(format!(
            "git write-tree failed in {}: {}",
            root.display(),
            stderr_of(&written)
        ));
    }
    let tree = String::from_utf8_lossy(&written.stdout).trim().to_owned();
    if tree.is_empty() {
        return Err(format!(
            "git write-tree named no tree in {}",
            root.display()
        ));
    }
    Ok(TurnTree {
        root: root.to_path_buf(),
        tree,
    })
}

fn diff_trees(
    root: &Path,
    base: &str,
    current: &str,
    max_patch_bytes: usize,
) -> Result<TurnDiff, String> {
    let names = capture_git(
        root,
        &[
            "diff-tree",
            "-r",
            "--name-status",
            "-z",
            "--find-renames",
            base,
            current,
            "--",
        ],
        MAX_SUMMARY_BYTES,
    )?;
    let numbers = capture_git(
        root,
        &[
            "diff-tree",
            "-r",
            "--numstat",
            "-z",
            "--find-renames",
            base,
            current,
            "--",
        ],
        MAX_SUMMARY_BYTES,
    )?;
    let patch = capture_git(
        root,
        &[
            "diff-tree",
            "-r",
            "-p",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--find-renames",
            "--unified=3",
            base,
            current,
            "--",
        ],
        max_patch_bytes,
    )?;

    let mut files = parse_name_status(&names.stdout);
    apply_numstat(&mut files, &numbers.stdout);

    let mut text = String::from_utf8_lossy(&patch.stdout).into_owned();
    if patch.truncated {
        text.truncate(text.rfind('\n').unwrap_or(0));
        text.push_str(TRUNCATION_NOTICE);
    }

    let additions = files
        .iter()
        .fold(0u32, |total, file| total.saturating_add(file.additions));
    let deletions = files
        .iter()
        .fold(0u32, |total, file| total.saturating_add(file.deletions));

    Ok(TurnDiff {
        files,
        patch: text,
        additions,
        deletions,
        truncated: names.truncated || numbers.truncated || patch.truncated,
    })
}

fn run_with_index(root: &Path, args: &[&str], index: &Path) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_INDEX_FILE", index)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git could not start: {error}"))
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

struct Capture {
    stdout: Vec<u8>,
    truncated: bool,
}

/// Run git under a hard byte ceiling, killing the child once the cap is hit so
/// a repository-sized diff never buffers in full.
fn capture_git(root: &Path, args: &[&str], max_bytes: usize) -> Result<Capture, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git could not start: {error}"))?;
    let mut pipe = child
        .stdout
        .take()
        .ok_or_else(|| "git offered no output pipe".to_owned())?;

    let mut stdout: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        let filled = match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(filled) => filled,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git output could not be read: {error}"));
            }
        };
        let remaining = max_bytes.saturating_sub(stdout.len());
        if filled > remaining {
            stdout.extend_from_slice(&buffer[..remaining]);
            truncated = true;
            let _ = child.kill();
            break;
        }
        stdout.extend_from_slice(&buffer[..filled]);
    }
    drop(pipe);

    let output = child
        .wait_with_output()
        .map_err(|error| format!("git did not exit: {error}"))?;
    if !output.status.success() && !truncated {
        let stderr = stderr_of(&output);
        return Err(if stderr.is_empty() {
            format!("git exited {}", output.status)
        } else {
            format!("git: {stderr}")
        });
    }
    Ok(Capture { stdout, truncated })
}

fn parse_name_status(value: &[u8]) -> Vec<TurnFile> {
    let fields = split_nul(value);
    let mut files = Vec::new();
    let mut index = 0usize;
    while index < fields.len() {
        let status = TurnFileStatus::from_code(fields[index].chars().next().unwrap_or('M'));
        index += 1;
        let Some(first) = fields.get(index).cloned() else {
            break;
        };
        index += 1;
        let (path, old_path) = match (status, fields.get(index).cloned()) {
            (TurnFileStatus::Renamed | TurnFileStatus::Copied, Some(second)) => {
                index += 1;
                (second, Some(first))
            }
            _ => (first, None),
        };
        files.push(TurnFile {
            path,
            old_path,
            status,
            additions: 0,
            deletions: 0,
            binary: false,
        });
    }
    files
}

fn apply_numstat(files: &mut [TurnFile], value: &[u8]) {
    let records = split_nul_raw(value);
    let mut index = 0usize;
    while index < records.len() {
        let record = &records[index];
        index += 1;
        if record.is_empty() {
            continue;
        }
        let mut columns = record.splitn(3, '\t');
        let additions = columns.next().unwrap_or_default();
        let deletions = columns.next().unwrap_or_default();
        let inline = columns.next().unwrap_or_default();
        // A rename record stops at the second tab: the old and the new path
        // follow it as their own NUL-separated records.
        let path = if inline.is_empty() {
            let renamed = records.get(index + 1).cloned().unwrap_or_default();
            index += 2;
            renamed
        } else {
            inline.to_owned()
        };
        let Some(file) = files.iter_mut().find(|file| file.path == path) else {
            continue;
        };
        file.binary = additions == "-" || deletions == "-";
        file.additions = additions.parse().unwrap_or(0);
        file.deletions = deletions.parse().unwrap_or(0);
    }
}

fn split_nul(value: &[u8]) -> Vec<String> {
    split_nul_raw(value)
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect()
}

fn split_nul_raw(value: &[u8]) -> Vec<String> {
    value
        .split(|byte| *byte == 0)
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A seeded repository, or `None` when git is missing.
    fn seeded_repo() -> Option<(TempDir, PathBuf)> {
        if !git_available() {
            return None;
        }
        let scratch = tempfile::tempdir().expect("a temp dir");
        // The temp root reaches git through a symlink on macOS, and git always
        // reports the resolved path.
        let root = scratch.path().canonicalize().expect("a resolved temp path");
        git(&root, &["init", "--quiet"]);
        git(&root, &["config", "user.email", "turn@zz.test"]);
        git(&root, &["config", "user.name", "zz turn snapshot"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("tracked.txt"), "one\ntwo\nthree\n").expect("seed file");
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "seed"]);
        Some((scratch, root))
    }

    #[test]
    fn a_snapshot_is_stable_across_no_op_resnapshots() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        let first = snapshot_tree(&root).expect("a seeded repo should snapshot");
        let second = snapshot_tree(&root).expect("a second snapshot should agree");

        assert_eq!(first, second);
        assert_eq!(first.root, root);
        assert_eq!(first.tree.len(), 40);
    }

    #[test]
    fn a_snapshot_leaves_the_real_index_alone() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        fs::write(root.join("fresh.txt"), "brand new\n").expect("untracked file");
        snapshot_tree(&root).expect("the repo should snapshot");

        let status = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["status", "--porcelain"])
            .output()
            .expect("git status should run");
        assert_eq!(
            String::from_utf8_lossy(&status.stdout),
            "?? fresh.txt\n",
            "the file must still read as untracked"
        );
    }

    #[test]
    fn a_turn_diff_reports_the_edit_and_the_untracked_file() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        let base = snapshot_tree(&root).expect("the turn should start from a snapshot");
        fs::write(root.join("tracked.txt"), "one\ntwo\nthree\nfour\n").expect("edit");
        fs::write(root.join("fresh.txt"), "brand new\n").expect("untracked file");

        let diff = capture_turn_diff(&root, &base).expect("the turn should diff");

        let tracked = diff
            .files
            .iter()
            .find(|file| file.path == "tracked.txt")
            .expect("the edited file should be reported");
        assert_eq!(tracked.status, TurnFileStatus::Modified);
        assert_eq!((tracked.additions, tracked.deletions), (1, 0));
        assert!(!tracked.binary);

        let fresh = diff
            .files
            .iter()
            .find(|file| file.path == "fresh.txt")
            .expect("the untracked file should be reported");
        assert_eq!(fresh.status, TurnFileStatus::Added);
        assert_eq!((fresh.additions, fresh.deletions), (1, 0));

        assert_eq!((diff.additions, diff.deletions), (2, 0));
        assert!(diff.patch.contains("+four"), "{}", diff.patch);
        assert!(diff.patch.contains("+brand new"), "{}", diff.patch);
        assert!(!diff.truncated);
    }

    #[test]
    fn an_untracked_file_from_before_the_turn_is_not_reported() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        fs::write(root.join("fresh.txt"), "brand new\n").expect("untracked file");
        let base = snapshot_tree(&root).expect("the turn should start from a snapshot");

        let diff = capture_turn_diff(&root, &base).expect("an idle turn should diff");

        assert!(diff.files.is_empty(), "{:?}", diff.files);
        assert!(diff.patch.is_empty());
    }

    #[test]
    fn a_rename_is_detected() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        let base = snapshot_tree(&root).expect("the turn should start from a snapshot");
        fs::rename(root.join("tracked.txt"), root.join("renamed.txt")).expect("rename");

        let diff = capture_turn_diff(&root, &base).expect("the turn should diff");

        assert_eq!(diff.files.len(), 1, "{:?}", diff.files);
        let renamed = &diff.files[0];
        assert_eq!(renamed.status, TurnFileStatus::Renamed);
        assert_eq!(renamed.path, "renamed.txt");
        assert_eq!(renamed.old_path.as_deref(), Some("tracked.txt"));
        assert_eq!((renamed.additions, renamed.deletions), (0, 0));
    }

    #[test]
    fn a_binary_file_is_flagged_rather_than_counted() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        let base = snapshot_tree(&root).expect("the turn should start from a snapshot");
        fs::write(root.join("blob.bin"), [0u8, 159, 146, 150, 0, 7]).expect("binary file");

        let diff = capture_turn_diff(&root, &base).expect("the turn should diff");

        let blob = diff
            .files
            .iter()
            .find(|file| file.path == "blob.bin")
            .expect("the binary file should be reported");
        assert!(blob.binary);
        assert_eq!((blob.additions, blob.deletions), (0, 0));
    }

    #[test]
    fn a_retargeted_working_directory_is_refused() {
        let Some((_first, first_root)) = seeded_repo() else {
            return;
        };
        let Some((_second, second_root)) = seeded_repo() else {
            return;
        };
        let base = snapshot_tree(&first_root).expect("the turn should start from a snapshot");

        let error =
            capture_turn_diff(&second_root, &base).expect_err("another worktree should be refused");

        assert!(error.contains("cannot be diffed"), "{error}");
    }

    #[test]
    fn a_directory_outside_a_worktree_is_refused() {
        if !git_available() {
            return;
        }
        let scratch = tempfile::tempdir().expect("a temp dir");

        let error =
            snapshot_tree(scratch.path()).expect_err("a bare directory should not snapshot");

        assert!(error.contains("not inside a git worktree"), "{error}");
    }

    #[test]
    fn an_oversized_patch_is_truncated() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        let base = snapshot_tree(&root).expect("the turn should start from a snapshot");
        fs::write(root.join("bulk.txt"), "a line of bulk text\n".repeat(2000)).expect("bulk file");
        let current = snapshot_tree(&root).expect("the repo should re-snapshot");

        let diff = diff_trees(&root, &base.tree, &current.tree, 512).expect("the turn should diff");

        assert!(diff.truncated);
        assert!(diff.patch.len() <= 512 + TRUNCATION_NOTICE.len());
        assert!(diff.patch.ends_with(TRUNCATION_NOTICE), "{}", diff.patch);
        assert!(
            diff.files.iter().any(|file| file.path == "bulk.txt"),
            "the summaries stay intact when only the patch overflows"
        );
        assert_eq!(diff.additions, 2000);
    }
}
