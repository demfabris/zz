use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde_json::Value;

use super::translate::Translator;
use crate::agent::stream::AgentSessionSummary;

const MAX_SANITIZED_LENGTH: usize = 200;
const LITE_READ_BYTES: u64 = 64 * 1024;
const MAX_TRANSCRIPT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TITLE_CHARS: usize = 200;

pub(crate) fn config_home() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
}

fn sanitize(path: &str) -> String {
    let sanitized = path
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.len() <= MAX_SANITIZED_LENGTH {
        return sanitized;
    }
    format!(
        "{}-{}",
        &sanitized[..MAX_SANITIZED_LENGTH],
        simple_hash(path)
    )
}

fn simple_hash(text: &str) -> String {
    let mut hash: i32 = 0;
    for unit in text.encode_utf16() {
        hash = hash
            .wrapping_shl(5)
            .wrapping_sub(hash)
            .wrapping_add(i32::from(unit));
    }
    let mut value = hash.unsigned_abs();
    if value == 0 {
        return "0".to_owned();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while value > 0 {
        out.push(digits[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn project_dir(projects: &Path, cwd: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let sanitized = sanitize(&canonical.to_string_lossy());
    let exact = projects.join(&sanitized);
    if exact.is_dir() {
        return Some(exact);
    }
    if sanitized.len() <= MAX_SANITIZED_LENGTH {
        return None;
    }
    let prefix = format!("{}-", &sanitized[..MAX_SANITIZED_LENGTH]);
    std::fs::read_dir(projects)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
        })
}

pub(crate) fn session_file(config: &Path, cwd: &Path, session_id: &str) -> Option<PathBuf> {
    if !valid_uuid(session_id) {
        return None;
    }
    let projects = config.join("projects");
    let name = format!("{session_id}.jsonl");
    if let Some(path) = project_dir(&projects, cwd)
        .map(|dir| dir.join(&name))
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    std::fs::read_dir(&projects)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(&name))
        .find(|path| path.is_file())
}

pub(crate) fn valid_uuid(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

struct Lite {
    head: String,
    tail: String,
    modified_ms: u128,
}

fn read_lite(path: &Path) -> Option<Lite> {
    let mut file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let size = metadata.len();
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis());
    let mut head = Vec::new();
    (&mut file)
        .take(LITE_READ_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    if head.is_empty() {
        return None;
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let tail = if size > LITE_READ_BYTES {
        file.seek(SeekFrom::Start(size - LITE_READ_BYTES)).ok()?;
        let mut tail = Vec::new();
        file.take(LITE_READ_BYTES).read_to_end(&mut tail).ok()?;
        String::from_utf8_lossy(&tail).into_owned()
    } else {
        head.clone()
    };
    Some(Lite {
        head,
        tail,
        modified_ms,
    })
}

fn lines(text: &str) -> impl DoubleEndedIterator<Item = Value> + '_ {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
}

fn last_field(text: &str, key: &str) -> Option<String> {
    lines(text).rev().find_map(|entry| {
        entry[key]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn first_field(text: &str, key: &str) -> Option<String> {
    lines(text).find_map(|entry| {
        entry[key]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn first_prompt(head: &str) -> Option<String> {
    let mut command = None;
    for entry in lines(head) {
        if entry["type"] != "user" || entry["isMeta"] == true || entry["isCompactSummary"] == true {
            continue;
        }
        let texts = match &entry["message"]["content"] {
            Value::String(text) => vec![text.clone()],
            Value::Array(blocks) => {
                if blocks.iter().any(|block| block["type"] == "tool_result") {
                    continue;
                }
                blocks
                    .iter()
                    .filter_map(|block| block["text"].as_str().map(str::to_owned))
                    .collect()
            }
            _ => continue,
        };
        for text in texts {
            let text = text.replace('\n', " ");
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            if let Some(name) = text
                .split_once("<command-name>")
                .and_then(|(_, rest)| rest.split_once("</command-name>"))
                .map(|(name, _)| name.to_owned())
            {
                command.get_or_insert(name);
                continue;
            }
            if text.starts_with('<') || text.starts_with("[Request interrupted") {
                continue;
            }
            return Some(clip(text));
        }
    }
    command
}

fn clip(text: &str) -> String {
    let mut clipped = text.chars().take(MAX_TITLE_CHARS).collect::<String>();
    if text.chars().count() > MAX_TITLE_CHARS {
        clipped = clipped.trim_end().to_owned();
        clipped.push('…');
    }
    clipped
}

fn summary(
    session_id: &str,
    path: &Path,
    project: Option<&Path>,
) -> Option<(u128, AgentSessionSummary)> {
    let lite = read_lite(path)?;
    let first_line = lite.head.lines().next().unwrap_or_default();
    if first_line.contains("\"isSidechain\":true") {
        return None;
    }
    let title = last_field(&lite.tail, "customTitle")
        .or_else(|| last_field(&lite.head, "customTitle"))
        .or_else(|| last_field(&lite.tail, "aiTitle"))
        .or_else(|| last_field(&lite.head, "aiTitle"))
        .or_else(|| last_field(&lite.tail, "lastPrompt"))
        .or_else(|| last_field(&lite.tail, "summary"))
        .or_else(|| first_prompt(&lite.head))?;
    let cwd = first_field(&lite.head, "cwd")
        .map(PathBuf::from)
        .or_else(|| project.map(Path::to_path_buf))?;
    let updated_at = humantime_rfc3339(lite.modified_ms);
    Some((
        lite.modified_ms,
        AgentSessionSummary {
            session_id: session_id.to_owned(),
            cwd,
            additional_directories: Vec::new(),
            title: Some(clip(&title)),
            updated_at,
        },
    ))
}

fn humantime_rfc3339(ms: u128) -> Option<String> {
    let secs = i64::try_from(ms / 1000).ok()?;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

pub(crate) fn list(config: &Path, cwd: Option<&Path>) -> Vec<AgentSessionSummary> {
    let projects = config.join("projects");
    let dirs = match cwd {
        Some(cwd) => project_dir(&projects, cwd).into_iter().collect::<Vec<_>>(),
        None => std::fs::read_dir(&projects)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect()
            })
            .unwrap_or_default(),
    };
    let mut seen = HashSet::new();
    let mut sessions = dirs
        .iter()
        .flat_map(|dir| {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        })
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?.to_owned();
            (path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") && valid_uuid(&stem))
                .then_some((stem, path))
        })
        .filter_map(|(session_id, path)| summary(&session_id, &path, cwd))
        .collect::<Vec<_>>();
    sessions.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    sessions
        .into_iter()
        .map(|(_, summary)| summary)
        .filter(|summary| seen.insert(summary.session_id.clone()))
        .collect()
}

pub(crate) fn transcript(path: &Path, cwd: PathBuf) -> Vec<Value> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut text = String::new();
    if file
        .take(MAX_TRANSCRIPT_BYTES)
        .read_to_string(&mut text)
        .is_err()
    {
        return Vec::new();
    }
    let entries = lines(&text)
        .filter(|entry| {
            matches!(
                entry["type"].as_str(),
                Some("user" | "assistant" | "progress" | "system" | "attachment")
            ) && entry["uuid"].is_string()
        })
        .collect::<Vec<_>>();
    let mut translator = Translator::new(cwd);
    chain(&entries)
        .into_iter()
        .flat_map(|entry| translator.history_entry(entry))
        .collect()
}

fn chain(entries: &[Value]) -> Vec<&Value> {
    let by_uuid = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| Some((entry["uuid"].as_str()?, (index, entry))))
        .collect::<HashMap<_, _>>();
    let parents = entries
        .iter()
        .filter_map(|entry| entry["parentUuid"].as_str())
        .collect::<HashSet<_>>();
    let parent_of = |entry: &Value| {
        entry["parentUuid"]
            .as_str()
            .and_then(|parent| by_uuid.get(parent))
            .map(|(_, entry)| *entry)
    };
    let mut leaves = Vec::new();
    for terminal in entries.iter().filter(|entry| {
        entry["uuid"]
            .as_str()
            .is_some_and(|uuid| !parents.contains(uuid))
    }) {
        let mut seen = HashSet::new();
        let mut current = Some(terminal);
        while let Some(entry) = current {
            if !seen.insert(entry["uuid"].as_str().unwrap_or_default()) {
                break;
            }
            if matches!(entry["type"].as_str(), Some("user" | "assistant")) {
                leaves.push(entry);
                break;
            }
            current = parent_of(entry);
        }
    }
    let index_of = |entry: &Value| {
        entry["uuid"]
            .as_str()
            .and_then(|uuid| by_uuid.get(uuid))
            .map_or(0, |(index, _)| *index)
    };
    let main = leaves
        .iter()
        .copied()
        .filter(|leaf| {
            leaf["isSidechain"] != true && leaf["isMeta"] != true && leaf["teamName"].is_null()
        })
        .max_by_key(|leaf| index_of(leaf));
    let Some(leaf) = main.or_else(|| leaves.iter().copied().max_by_key(|leaf| index_of(leaf)))
    else {
        return Vec::new();
    };
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut current = Some(leaf);
    while let Some(entry) = current {
        if !seen.insert(entry["uuid"].as_str().unwrap_or_default()) {
            break;
        }
        chain.push(entry);
        current = parent_of(entry);
    }
    chain.reverse();
    chain
        .into_iter()
        .filter(|entry| {
            matches!(entry["type"].as_str(), Some("user" | "assistant"))
                && entry["isMeta"] != true
                && entry["isSidechain"] != true
                && entry["teamName"].is_null()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_directories_match_claude_codes_naming() {
        assert_eq!(sanitize("/home/user/.cache/ws"), "-home-user--cache-ws");
        let long = format!("/{}", "a".repeat(250));
        let sanitized = sanitize(&long);
        assert!(sanitized.starts_with(&format!("-{}", "a".repeat(199))));
        assert_eq!(simple_hash("hello"), "1n1e4y");
    }

    #[test]
    fn listing_reads_titles_and_skips_sidechains() {
        let config = tempfile::tempdir().expect("tempdir");
        let cwd = config.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let canonical = std::fs::canonicalize(&cwd).expect("canonical");
        let dir = config
            .path()
            .join("projects")
            .join(sanitize(&canonical.to_string_lossy()));
        std::fs::create_dir_all(&dir).expect("project dir");
        let first = "11111111-1111-1111-1111-111111111111";
        std::fs::write(
            dir.join(format!("{first}.jsonl")),
            format!(
                "{{\"type\":\"user\",\"uuid\":\"a\",\"parentUuid\":null,\"cwd\":\"{}\",\"message\":{{\"role\":\"user\",\"content\":\"fix the build\"}}}}\n",
                canonical.display()
            ),
        )
        .expect("session");
        let side = "22222222-2222-2222-2222-222222222222";
        std::fs::write(
            dir.join(format!("{side}.jsonl")),
            "{\"type\":\"user\",\"isSidechain\":true,\"uuid\":\"b\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        )
        .expect("sidechain");
        std::fs::write(dir.join("notes.jsonl"), "{}\n").expect("stray");
        let sessions = list(config.path(), Some(&cwd));
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, first);
        assert_eq!(sessions[0].title.as_deref(), Some("fix the build"));
        assert_eq!(sessions[0].cwd, canonical);
        assert!(
            sessions[0]
                .updated_at
                .as_deref()
                .is_some_and(|at| at.ends_with('Z'))
        );
        assert_eq!(
            session_file(config.path(), &cwd, first),
            Some(dir.join(format!("{first}.jsonl")))
        );
    }

    #[test]
    fn a_transcript_follows_the_live_branch_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let lines = [
            r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":"first"}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"id":"m1","content":[{"type":"text","text":"abandoned"}]}}"#,
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u1","message":{"id":"m2","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"user","uuid":"u2","parentUuid":"a2","toolUseResult":{"stdout":"a.rs","stderr":""},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"a.rs"}]}}"#,
            r#"{"type":"assistant","uuid":"a3","parentUuid":"u2","isSidechain":true,"message":{"id":"m3","content":[{"type":"text","text":"side"}]}}"#,
            r#"{"type":"assistant","uuid":"a4","parentUuid":"u2","message":{"id":"m4","content":[{"type":"text","text":"kept"}]}}"#,
        ];
        std::fs::write(&path, lines.join("\n")).expect("write");
        let updates = transcript(&path, PathBuf::from("/work"));
        let kinds = updates
            .iter()
            .map(|update| update["sessionUpdate"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                "user_message_chunk",
                "tool_call",
                "tool_call_update",
                "agent_message_chunk"
            ]
        );
        assert_eq!(updates[2]["content"][0]["content"]["text"], "a.rs");
        assert_eq!(updates[3]["content"]["text"], "kept");
    }

    #[test]
    fn timestamps_render_as_utc() {
        assert_eq!(
            humantime_rfc3339(1_791_399_341_959).as_deref(),
            Some("2026-10-07T18:55:41Z")
        );
    }
}
