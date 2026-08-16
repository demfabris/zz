use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use zz_protocol::AgentGitSummary;

const MAX_GIT_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 256 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn capture_git_summary(cwd: &Path) -> Result<AgentGitSummary, String> {
    let deadline = Instant::now() + GIT_TIMEOUT;
    let root = repo_root(cwd, deadline)?;
    let branch = current_branch(&root, deadline)?;
    let base = head_tree(&root, deadline)?;
    let current = write_tree(&root, deadline)?;
    let names = capture_git(
        &root,
        &[
            "diff-tree",
            "-r",
            "--name-status",
            "-z",
            "--find-renames",
            &base,
            &current,
            "--",
        ],
        deadline,
    )?;
    let numbers = capture_git(
        &root,
        &[
            "diff-tree",
            "-r",
            "--numstat",
            "-z",
            "--find-renames",
            &base,
            &current,
            "--",
        ],
        deadline,
    )?;
    let (additions, deletions) = parse_numstat(&numbers);
    Ok(AgentGitSummary {
        branch,
        changed_files: u32::try_from(count_name_status(&names)).unwrap_or(u32::MAX),
        additions,
        deletions,
    })
}

fn repo_root(cwd: &Path, deadline: Instant) -> Result<PathBuf, String> {
    let output = git_output(cwd, &["rev-parse", "--show-toplevel"], deadline)?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || root.is_empty() {
        return Err(format!("{} is not inside a git worktree", cwd.display()));
    }
    Ok(PathBuf::from(root))
}

fn current_branch(root: &Path, deadline: Instant) -> Result<Option<String>, String> {
    let output = git_output(
        root,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        deadline,
    )?;
    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Ok((!branch.is_empty()).then_some(branch));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    Err(git_failure(&output))
}

fn head_tree(root: &Path, deadline: Instant) -> Result<String, String> {
    let head = git_output(root, &["rev-parse", "--verify", "HEAD^{tree}"], deadline)?;
    if head.status.success() {
        let tree = String::from_utf8_lossy(&head.stdout).trim().to_owned();
        if !tree.is_empty() {
            return Ok(tree);
        }
    }
    let empty = git_output(root, &["mktree"], deadline)?;
    let tree = String::from_utf8_lossy(&empty.stdout).trim().to_owned();
    if !empty.status.success() || tree.is_empty() {
        return Err(git_failure(&empty));
    }
    Ok(tree)
}

fn write_tree(root: &Path, deadline: Instant) -> Result<String, String> {
    let scratch = tempfile::tempdir()
        .map_err(|error| format!("no scratch directory for the Git summary: {error}"))?;
    let index = scratch.path().join("index");
    let added = git_output_with_index(
        root,
        &["add", "-A", "--ignore-errors", "."],
        &index,
        deadline,
    )?;
    if !index.exists() {
        return Err(git_failure(&added));
    }
    let written = git_output_with_index(root, &["write-tree"], &index, deadline)?;
    let tree = String::from_utf8_lossy(&written.stdout).trim().to_owned();
    if !written.status.success() || tree.is_empty() {
        return Err(git_failure(&written));
    }
    Ok(tree)
}

fn capture_git(root: &Path, args: &[&str], deadline: Instant) -> Result<Vec<u8>, String> {
    let output = git_output(root, args, deadline)?;
    if !output.status.success() {
        return Err(git_failure(&output));
    }
    Ok(output.stdout)
}

fn git_output(root: &Path, args: &[&str], deadline: Instant) -> Result<Output, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args).stdin(Stdio::null());
    run_output_until(command, MAX_GIT_OUTPUT_BYTES, deadline)
}

fn git_output_with_index(
    root: &Path,
    args: &[&str],
    index: &Path,
    deadline: Instant,
) -> Result<Output, String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_INDEX_FILE", index)
        .stdin(Stdio::null());
    run_output_until(command, MAX_GIT_OUTPUT_BYTES, deadline)
}

fn git_failure(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("git exited {}", output.status)
    } else {
        format!("git: {stderr}")
    }
}

#[cfg(test)]
fn run_output(command: Command, max_stdout: usize) -> Result<Output, String> {
    let deadline = Instant::now() + GIT_TIMEOUT;
    run_output_until(command, max_stdout, deadline)
}

fn run_output_until(
    mut command: Command,
    max_stdout: usize,
    deadline: Instant,
) -> Result<Output, String> {
    let timeout = deadline.saturating_duration_since(Instant::now());
    if timeout.is_zero() {
        return Err(format!(
            "git timed out after {} seconds",
            GIT_TIMEOUT.as_secs()
        ));
    }
    let (output, truncated) = collect_output(spawn_output(&mut command)?, max_stdout, timeout)?;
    if truncated {
        return Err(format!("git output exceeded {max_stdout} bytes"));
    }
    Ok(output)
}

fn spawn_output(command: &mut Command) -> Result<Child, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;

        command.process_group(0);
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git could not start: {error}"))
}

fn collect_output(
    mut child: Child,
    max_stdout: usize,
    timeout: Duration,
) -> Result<(Output, bool), String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "git offered no output pipe".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "git offered no error pipe".to_owned())?;
    let (overflow_tx, overflow_rx) = mpsc::sync_channel(1);
    let (stdout_tx, stdout_rx) = mpsc::sync_channel(1);
    let (stderr_tx, stderr_rx) = mpsc::sync_channel(1);
    let stdout_reader = thread::spawn(move || {
        let _ = stdout_tx.send(read_limited(stdout, max_stdout, Some(&overflow_tx)));
    });
    let stderr_reader = thread::spawn(move || {
        let _ = stderr_tx.send(read_limited(stderr, MAX_STDERR_BYTES, None));
    });
    let deadline = Instant::now() + timeout;
    let mut truncated = false;
    let mut status = None;
    let mut stdout = None;
    let mut stderr = None;
    loop {
        if overflow_rx.try_recv().is_ok() {
            truncated = true;
            status = Some(terminate_output(&mut child)?);
        }
        if stdout.is_none() {
            match stdout_rx.try_recv() {
                Ok(value) => stdout = Some(value),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("git output reader stopped".to_owned());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if stderr.is_none() {
            match stderr_rx.try_recv() {
                Ok(value) => stderr = Some(value),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("git error reader stopped".to_owned());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if status.is_none()
            && child
                .try_wait()
                .map_err(|error| format!("git could not be polled: {error}"))?
                .is_some()
        {
            status = Some(terminate_output(&mut child)?);
        }
        if status.is_some() && stdout.is_some() && stderr.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = terminate_output(&mut child);
            return Err(format!("git timed out after {} seconds", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(10));
    }
    stdout_reader
        .join()
        .map_err(|_| "git output reader panicked".to_owned())?;
    stderr_reader
        .join()
        .map_err(|_| "git error reader panicked".to_owned())?;
    Ok((
        Output {
            status: status.expect("git status set before output completes"),
            stdout: stdout.expect("git output set before completion")?,
            stderr: stderr.expect("git error set before completion")?,
        },
        truncated,
    ))
}

fn terminate_output(child: &mut Child) -> Result<std::process::ExitStatus, String> {
    #[cfg(unix)]
    let _ = rustix::process::kill_process_group(
        rustix::process::Pid::from_child(&*child),
        rustix::process::Signal::KILL,
    );
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", pid.as_str(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    child
        .wait()
        .map_err(|error| format!("git did not exit: {error}"))
}

fn read_limited(
    mut pipe: impl Read,
    limit: usize,
    overflow: Option<&mpsc::SyncSender<()>>,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    let mut reported = false;
    loop {
        let filled = pipe
            .read(&mut buffer)
            .map_err(|error| format!("git output could not be read: {error}"))?;
        if filled == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..filled.min(remaining)]);
        if filled > remaining && !reported {
            if let Some(overflow) = overflow {
                let _ = overflow.try_send(());
            }
            reported = true;
        }
    }
    Ok(output)
}

fn count_name_status(value: &[u8]) -> usize {
    let fields = value.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut count = 0usize;
    let mut index = 0usize;
    while index < fields.len() {
        let Some(code) = fields[index].first().copied() else {
            index += 1;
            continue;
        };
        count = count.saturating_add(1);
        index = index.saturating_add(if matches!(code, b'R' | b'C') { 3 } else { 2 });
    }
    count
}

fn parse_numstat(value: &[u8]) -> (u32, u32) {
    value
        .split(|byte| *byte == 0)
        .filter_map(|record| {
            let mut columns = record.splitn(3, |byte| *byte == b'\t');
            let additions = parse_count(columns.next()?)?;
            let deletions = parse_count(columns.next()?)?;
            columns.next()?;
            Some((additions, deletions))
        })
        .fold((0u32, 0u32), |(additions, deletions), next| {
            (
                additions.saturating_add(next.0),
                deletions.saturating_add(next.1),
            )
        })
}

fn parse_count(value: &[u8]) -> Option<u32> {
    let value = std::str::from_utf8(value).ok()?.parse::<u64>().ok()?;
    Some(u32::try_from(value).unwrap_or(u32::MAX))
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

    fn seeded_repo() -> Option<(TempDir, PathBuf)> {
        if !git_available() {
            return None;
        }
        let scratch = tempfile::tempdir().expect("a temp dir");
        let root = scratch.path().canonicalize().expect("a resolved temp path");
        git(&root, &["init", "--quiet", "--initial-branch=main"]);
        git(&root, &["config", "user.email", "git-summary@zz.test"]);
        git(&root, &["config", "user.name", "zz git summary"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("tracked.txt"), "one\ntwo\n").expect("seed file");
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "seed"]);
        Some((scratch, root))
    }

    #[test]
    fn clean_repo_reports_branch_with_zero_changes() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };

        let summary = capture_git_summary(&root).expect("clean summary");

        assert_eq!(summary.branch.as_deref(), Some("main"));
        assert_eq!(summary.changed_files, 0);
        assert_eq!(summary.additions, 0);
        assert_eq!(summary.deletions, 0);
    }

    #[test]
    fn dirty_repo_counts_tracked_and_untracked_content() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        fs::write(root.join("tracked.txt"), "one\nchanged\n").expect("tracked edit");
        fs::write(root.join("fresh.txt"), "new\nfile\n").expect("untracked file");

        let summary = capture_git_summary(&root).expect("dirty summary");

        assert_eq!(summary.changed_files, 2);
        assert_eq!(summary.additions, 3);
        assert_eq!(summary.deletions, 1);
    }

    #[test]
    fn detached_head_has_no_branch_but_keeps_changes() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        git(&root, &["checkout", "--quiet", "--detach"]);
        fs::write(root.join("fresh.txt"), "new\n").expect("untracked file");

        let summary = capture_git_summary(&root).expect("detached summary");

        assert_eq!(summary.branch, None);
        assert_eq!(summary.changed_files, 1);
        assert_eq!(summary.additions, 1);
    }

    #[test]
    fn directory_outside_git_has_no_summary() {
        if !git_available() {
            return;
        }
        let scratch = tempfile::tempdir().expect("temporary directory");

        let error = capture_git_summary(scratch.path()).expect_err("not a worktree");

        assert!(error.contains("not inside a git worktree"), "{error}");
    }

    #[test]
    fn summary_leaves_the_real_index_unchanged() {
        let Some((_scratch, root)) = seeded_repo() else {
            return;
        };
        fs::write(root.join("fresh.txt"), "new\n").expect("untracked file");

        capture_git_summary(&root).expect("summary");

        let output = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["status", "--porcelain"])
            .output()
            .expect("git status");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "?? fresh.txt\n");
    }

    #[cfg(unix)]
    #[test]
    fn command_output_drains_stderr_without_blocking_stdout() {
        let mut command = Command::new("sh");
        command.arg("-c").arg(
            "i=0; while [ $i -lt 20000 ]; do echo error-output-line >&2; i=$((i + 1)); done; printf ok",
        );
        let output = run_output(command, 1024).expect("command should finish");
        assert_eq!(output.stdout, b"ok");
        assert!(output.stderr.len() <= MAX_STDERR_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn command_output_has_a_hard_deadline() {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("(trap '' TERM; while :; do sleep 1; done >&2) & while :; do sleep 1; done");
        let started = Instant::now();
        let error = collect_output(
            spawn_output(&mut command).expect("command should start"),
            1024,
            Duration::from_millis(50),
        )
        .expect_err("command should time out");
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
