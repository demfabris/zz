use std::{
    fs::File,
    io::{self, Write as _},
    os::unix::fs::{FileExt as _, PermissionsExt as _},
    path::Path,
    process::{Child, Command, Stdio},
    str::FromStr as _,
    thread,
    time::{Duration, Instant},
};

use agent_client_protocol::AcpAgent;
use serde_json::{Value, json};

use super::{claude_peers, environment::with_platform_environment};

const TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_OUTPUT: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum QueueError {
    #[error(
        "Codex session has no name yet; send it a first prompt in the pane or run /rename there"
    )]
    NoName,
    #[error("no Codex session matches the pane title; run /rename in the pane")]
    NoThread,
    #[error("{0} Codex sessions match the pane title; run /rename in the pane")]
    Ambiguous(usize),
    #[error("codex executable not found on the login-shell PATH")]
    MissingCodex,
    #[error("Codex command timed out after ten seconds")]
    Timeout,
    #[error("{0}")]
    Failed(String),
    #[error("Codex command failed: {0}")]
    Io(#[from] io::Error),
}

impl QueueError {
    pub(crate) fn for_pane(&self, pane: impl std::fmt::Display, title: &str, cwd: &Path) -> String {
        match self {
            Self::NoName => format!(
                "Codex session in {pane} has no name yet; send it a first prompt in the pane or run /rename there"
            ),
            Self::Ambiguous(count) => format!(
                "{count} Codex sessions named {} in {}; run /rename in the pane",
                thread_name_from_title(title).unwrap_or(title),
                cwd.display()
            ),
            Self::NoThread => format!(
                "no Codex session named {} in {} for {pane}; run /rename in the pane",
                thread_name_from_title(title).unwrap_or(title),
                cwd.display()
            ),
            _ => format!("{pane}: {self}"),
        }
    }
}

pub(crate) struct Delivered {
    pub(crate) thread_id: String,
    pub(crate) message_id: String,
}

fn thread_name_from_title(title: &str) -> Option<&str> {
    let (name, _) = title.split_once(" | ")?;
    let name = name
        .trim_start_matches(|c: char| ('\u{2800}'..='\u{28ff}').contains(&c) || c.is_whitespace())
        .trim_end();
    (!name.is_empty()).then_some(name)
}

pub(crate) fn pane_runs_codex(pane_pid: u32) -> bool {
    let Ok(parents) = claude_peers::process_parents() else {
        return false;
    };
    std::iter::once(pane_pid)
        .chain(claude_peers::descendants_by_depth(&parents, pane_pid))
        .any(|pid| {
            #[cfg(target_os = "linux")]
            let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
            #[cfg(not(target_os = "linux"))]
            let executable = Command::new("ps")
                .args(["-p", &pid.to_string(), "-o", "comm="])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| {
                    std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
                });
            executable.is_some_and(|path| path.file_name().is_some_and(|name| name == "codex"))
        })
}

fn executable_path() -> Result<std::ffi::OsString, QueueError> {
    let agent =
        AcpAgent::from_str("codex").map_err(|error| QueueError::Failed(error.to_string()))?;
    Ok(with_platform_environment(agent)
        .into_config()
        .environment()
        .get("PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default())
}

fn command(codex: &Path) -> Result<Command, QueueError> {
    let mut command = Command::new(codex);
    command.env("PATH", executable_path()?);
    Ok(command)
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        self.0.stdin.take();
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn captured(file: &File, offset: &mut u64, bytes: &mut Vec<u8>) -> Result<(), QueueError> {
    let mut buffer = [0; 8192];
    loop {
        let count = file.read_at(&mut buffer, *offset)?;
        if count == 0 {
            return Ok(());
        }
        if bytes.len() + count > MAX_OUTPUT {
            return Err(QueueError::Failed("Codex output exceeded 4 MiB".to_owned()));
        }
        bytes.extend_from_slice(&buffer[..count]);
        *offset += count as u64;
    }
}

fn send(child: &mut Child, value: &Value) -> Result<(), QueueError> {
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| io::Error::other("Codex stdin closed"))?;
    writeln!(stdin, "{value}")?;
    Ok(())
}

fn response(
    file: &File,
    offset: &mut u64,
    pending: &mut Vec<u8>,
    id: u64,
    child: &mut Child,
    deadline: Instant,
) -> Result<Value, QueueError> {
    let mut exited = false;
    loop {
        if Instant::now() >= deadline {
            return Err(QueueError::Timeout);
        }
        captured(file, offset, pending)?;
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<_> = pending.drain(..=end).collect();
            let value: Value = serde_json::from_slice(&line)
                .map_err(|error| QueueError::Failed(format!("invalid Codex response: {error}")))?;
            if value["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(QueueError::Failed(format!(
                    "Codex thread lookup failed: {error}"
                )));
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| QueueError::Failed("Codex response has no result".to_owned()));
        }
        if exited {
            return Err(QueueError::Failed(
                "Codex exited before answering thread lookup".to_owned(),
            ));
        }
        exited = child.try_wait()?.is_some();
        if !exited {
            thread::sleep(POLL_INTERVAL);
        }
    }
}

fn resolve_thread(codex: &Path, cwd: &Path, name: &str) -> Result<String, QueueError> {
    let deadline = Instant::now() + TIMEOUT;
    let stdout = tempfile::tempfile()?;
    let mut child = Process(
        command(codex)?
            .arg("app-server")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(stdout.try_clone()?)
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut offset = 0;
    let mut pending = Vec::new();
    send(
        &mut child.0,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"zz","title":"zz","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}}),
    )?;
    response(
        &stdout,
        &mut offset,
        &mut pending,
        1,
        &mut child.0,
        deadline,
    )?;
    send(
        &mut child.0,
        &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    )?;
    let mut cursor = None;
    let mut matches = Vec::new();
    for page in 0..4 {
        let id = page + 2;
        let mut params = json!({
            "limit": 50,
            "archived": false,
            "cwd": cwd,
            "useStateDbOnly": true,
            "searchTerm": name,
        });
        if let Some(cursor) = cursor.take() {
            params["cursor"] = cursor;
        }
        send(
            &mut child.0,
            &json!({"jsonrpc":"2.0","id":id,"method":"thread/list","params":params}),
        )?;
        let result = response(
            &stdout,
            &mut offset,
            &mut pending,
            id,
            &mut child.0,
            deadline,
        )?;
        let data = result["data"].as_array().ok_or_else(|| {
            QueueError::Failed("Codex thread/list response has no data array".to_owned())
        })?;
        collect_matches(data, name, cwd, &mut matches)?;
        cursor = result
            .get("nextCursor")
            .filter(|value| !value.is_null())
            .cloned();
        if cursor.is_none() {
            return select_thread(matches);
        }
    }
    Err(QueueError::Failed(
        "Codex thread lookup exceeded four pages; run /rename in the pane or archive old sessions"
            .to_owned(),
    ))
}

fn collect_matches(
    data: &[Value],
    name: &str,
    cwd: &Path,
    matches: &mut Vec<String>,
) -> Result<(), QueueError> {
    for item in data {
        if item["name"].as_str() == Some(name)
            && item["cwd"]
                .as_str()
                .is_some_and(|path| Path::new(path) == cwd)
        {
            let thread_id = item["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| QueueError::Failed("Codex thread has no id".to_owned()))?;
            if !matches.iter().any(|id| id == thread_id) {
                matches.push(thread_id.to_owned());
            }
        }
    }
    Ok(())
}

fn select_thread(mut matches: Vec<String>) -> Result<String, QueueError> {
    match matches.len() {
        0 => Err(QueueError::NoThread),
        1 => Ok(matches.remove(0)),
        count => Err(QueueError::Ambiguous(count)),
    }
}

fn message_id(stdout: &str, thread_id: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let (id, thread) = line
            .strip_prefix("Queued message ")?
            .split_once(" for thread ")?;
        (!id.is_empty() && thread.strip_suffix('.') == Some(thread_id)).then(|| id.to_owned())
    })
}

fn queue(codex: &Path, thread_id: &str, text: &str) -> Result<String, QueueError> {
    let deadline = Instant::now() + TIMEOUT;
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    let mut child = Process(
        command(codex)?
            .args(["queue", "--thread", thread_id, "--message", text])
            .stdin(Stdio::null())
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?)
            .spawn()?,
    );
    let status = loop {
        if Instant::now() >= deadline {
            return Err(QueueError::Timeout);
        }
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        thread::sleep(POLL_INTERVAL);
    };
    let mut bytes = Vec::new();
    captured(
        if status.success() { &stdout } else { &stderr },
        &mut 0,
        &mut bytes,
    )?;
    let output = String::from_utf8_lossy(&bytes);
    if !status.success() {
        return Err(QueueError::Failed(if output.trim().is_empty() {
            format!("codex queue exited with {status}")
        } else {
            output.trim().to_owned()
        }));
    }
    message_id(&output, thread_id).ok_or_else(|| {
        QueueError::Failed("codex queue succeeded but returned no queued message id".to_owned())
    })
}

pub(crate) fn deliver(
    pane_pid: u32,
    title: &str,
    cwd: &Path,
    text: &str,
) -> Result<Delivered, QueueError> {
    let name = thread_name_from_title(title).ok_or(QueueError::NoName)?;
    let codex = std::env::split_paths(&executable_path()?)
        .map(|directory| directory.join("codex"))
        .find(|candidate| {
            std::fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
        .ok_or(QueueError::MissingCodex)?;
    let thread_id = resolve_thread(&codex, cwd, name)?;
    log::debug!(target: "zz::agent", "resolved Codex thread {thread_id} for pane process {pane_pid}");
    let message_id = queue(&codex, &thread_id, text)?;
    Ok(Delivered {
        thread_id,
        message_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_yield_the_thread_name_only_when_codex_named_one() {
        assert_eq!(thread_name_from_title("zz"), None);
        assert_eq!(thread_name_from_title(""), None);
        assert_eq!(thread_name_from_title(" | zz"), None);
        assert_eq!(
            thread_name_from_title("Reply pong-title | zz"),
            Some("Reply pong-title")
        );
        assert_eq!(
            thread_name_from_title("titleprobe-42 | zz"),
            Some("titleprobe-42")
        );
        assert_eq!(thread_name_from_title("a | b | c"), Some("a"));
        assert_eq!(
            thread_name_from_title("\u{280f} Reply pong-a2 | zz"),
            Some("Reply pong-a2")
        );
        assert_eq!(thread_name_from_title("\u{280f} | zz"), None);
    }

    #[test]
    fn thread_selection_needs_exactly_one_name_and_directory_match() {
        let cwd = Path::new("/Users/x/dev/zz");
        let data = vec![
            json!({"id":"one","name":"probe","cwd":"/Users/x/dev/zz"}),
            json!({"id":"two","name":"probe","cwd":"/Users/x/other"}),
            json!({"id":"three","name":"other","cwd":"/Users/x/dev/zz"}),
            json!({"id":"one","name":"probe","cwd":"/Users/x/dev/zz"}),
        ];
        let mut matches = Vec::new();
        collect_matches(&data, "probe", cwd, &mut matches).expect("matches");
        assert_eq!(select_thread(matches).expect("single"), "one");
        let mut none = Vec::new();
        collect_matches(&data, "missing", cwd, &mut none).expect("matches");
        assert!(matches!(select_thread(none), Err(QueueError::NoThread)));
        let mut many = Vec::new();
        collect_matches(
            &[
                json!({"id":"a","name":"probe","cwd":"/Users/x/dev/zz"}),
                json!({"id":"b","name":"probe","cwd":"/Users/x/dev/zz"}),
            ],
            "probe",
            cwd,
            &mut many,
        )
        .expect("matches");
        assert!(matches!(select_thread(many), Err(QueueError::Ambiguous(2))));
        assert!(
            collect_matches(
                &[json!({"id":"","name":"probe","cwd":"/Users/x/dev/zz"})],
                "probe",
                cwd,
                &mut Vec::new()
            )
            .is_err()
        );
    }

    #[test]
    fn queued_message_ids_are_parsed_from_codex_output() {
        assert_eq!(
            message_id(
                "Queued message 01a08d6a-a6c5 for thread 01a08d6a-7be4.\n",
                "01a08d6a-7be4"
            )
            .as_deref(),
            Some("01a08d6a-a6c5")
        );
        assert_eq!(
            message_id("Queued message x for thread other.", "mine"),
            None
        );
        assert_eq!(message_id("something else", "mine"), None);
    }

    #[test]
    fn errors_name_the_pane_and_the_title() {
        let cwd = Path::new("/Users/x/dev/zz");
        assert!(
            QueueError::NoName
                .for_pane("%3", "zz", cwd)
                .contains("%3 has no name yet")
        );
        assert_eq!(
            QueueError::Ambiguous(2).for_pane("%3", "probe | zz", cwd),
            "2 Codex sessions named probe in /Users/x/dev/zz; run /rename in the pane"
        );
        assert!(
            QueueError::NoThread
                .for_pane("%3", "probe | zz", cwd)
                .starts_with("no Codex session named probe in /Users/x/dev/zz for %3")
        );
    }
}
