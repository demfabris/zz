use serde_json::{Map, Value};
use zz_protocol::AgentProvider;

const CLAUDE_TASK_OPEN: &str = "<task-notification>";
const CLAUDE_TASK_CLOSE: &str = "</task-notification>";
const CLAUDE_REMINDER_OPEN: &str = "<system-reminder>";
const CLAUDE_REMINDER_CLOSE: &str = "</system-reminder>";
const CODEX_MEMORY_OPEN: &str = "<oai-mem-citation>";
const CODEX_MEMORY_CLOSE: &str = "</oai-mem-citation>";
const CODEX_STAGE_OPEN: &str = "::git-stage{";
const CODEX_COMMIT_OPEN: &str = "::git-commit{";

/// The daemon defines the task vocabulary — it parses the adapter's SDK
/// passthrough on its own side of the wire — and the transcript's
/// `<task-notification>` scrubbing produces the same shape.
pub use zz_daemon::TaskNotification;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryCitation {
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub note: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Segment {
    Clean(String),
    Notification(TaskNotification),
    Stripped {
        kind: &'static str,
        memory_citations: Vec<MemoryCitation>,
    },
}

#[derive(Clone, Copy)]
enum Artifact {
    TaskNotification,
    SystemReminder,
    MemoryCitation,
    GitDirective,
}

#[derive(Clone, Copy)]
struct Pattern {
    open: &'static str,
    close: &'static str,
    artifact: Artifact,
}

const CLAUDE_PATTERNS: &[Pattern] = &[
    Pattern {
        open: CLAUDE_TASK_OPEN,
        close: CLAUDE_TASK_CLOSE,
        artifact: Artifact::TaskNotification,
    },
    Pattern {
        open: CLAUDE_REMINDER_OPEN,
        close: CLAUDE_REMINDER_CLOSE,
        artifact: Artifact::SystemReminder,
    },
];

const CODEX_PATTERNS: &[Pattern] = &[
    Pattern {
        open: CODEX_MEMORY_OPEN,
        close: CODEX_MEMORY_CLOSE,
        artifact: Artifact::MemoryCitation,
    },
    Pattern {
        open: CODEX_STAGE_OPEN,
        close: "}",
        artifact: Artifact::GitDirective,
    },
    Pattern {
        open: CODEX_COMMIT_OPEN,
        close: "}",
        artifact: Artifact::GitDirective,
    },
];

/// Split provider-specific harness artifacts out of a streamed text chunk.
///
/// `carry` retains only the suffixes that could still open a marker.
pub fn scan_text(provider: AgentProvider, text: &str, carry: &mut String) -> Vec<Segment> {
    let patterns = match provider {
        AgentProvider::Codex => CODEX_PATTERNS,
        AgentProvider::ClaudeCode => CLAUDE_PATTERNS,
    };
    let mut input = std::mem::take(carry);
    input.push_str(text);
    let mut segments = Vec::new();
    let mut cursor = 0;

    while cursor < input.len() {
        let next = patterns
            .iter()
            .filter_map(|pattern| {
                input[cursor..]
                    .find(pattern.open)
                    .map(|offset| (cursor + offset, *pattern))
            })
            .min_by_key(|(start, _)| *start);
        let Some((start, pattern)) = next else {
            let tail = partial_open_suffix(&input[cursor..], patterns);
            let clean_end = input.len() - tail;
            push_clean(&mut segments, &input[cursor..clean_end]);
            carry.push_str(&input[clean_end..]);
            break;
        };

        push_clean(&mut segments, &input[cursor..start]);
        let content_start = start + pattern.open.len();
        let Some(close_offset) = input[content_start..].find(pattern.close) else {
            carry.push_str(&input[start..]);
            break;
        };
        let end = content_start + close_offset + pattern.close.len();
        let raw = &input[start..end];
        segments.push(match pattern.artifact {
            Artifact::TaskNotification => Segment::Notification(parse_task_notification(raw)),
            Artifact::SystemReminder => Segment::Stripped {
                kind: "system-reminder",
                memory_citations: Vec::new(),
            },
            Artifact::MemoryCitation => Segment::Stripped {
                kind: "oai-mem-citation",
                memory_citations: parse_memory_citations(raw),
            },
            Artifact::GitDirective => Segment::Stripped {
                kind: "git-directive",
                memory_citations: Vec::new(),
            },
        });
        cursor = end;
    }

    segments
}

/// Codex collaboration tool call (`_meta.codex.collaboration`), merged with
/// the per-thread agent states the adapter mirrors into the tool's raw input.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodexCollaboration {
    pub tool: String,
    pub receiver_thread_ids: Vec<String>,
    pub prompt: Option<String>,
    pub agents_states: Vec<(String, CodexAgentState)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexAgentState {
    pub status: String,
    pub message: Option<String>,
}

pub fn codex_collaboration(
    meta: Option<&Map<String, Value>>,
    raw_input: Option<&Value>,
) -> Option<CodexCollaboration> {
    let collaboration = meta?
        .get("codex")?
        .as_object()?
        .get("collaboration")?
        .as_object()?;
    let tool = collaboration.get("tool")?.as_str()?.to_owned();
    let receiver_thread_ids = collaboration
        .get("receiverThreadIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let prompt = raw_input
        .and_then(|input| input.get("prompt"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let agents_states = raw_input
        .and_then(|input| input.get("agentsStates"))
        .and_then(Value::as_object)
        .map(|states| {
            states
                .iter()
                .filter_map(|(thread, state)| {
                    Some((
                        thread.clone(),
                        CodexAgentState {
                            status: state.get("status")?.as_str()?.to_owned(),
                            message: state
                                .get("message")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(CodexCollaboration {
        tool,
        receiver_thread_ids,
        prompt,
        agents_states,
    })
}

/// Whether a codex tool call represents a subagent: a subagent activity item,
/// or a collab tool that puts an agent to work (spawn/resume).
pub fn codex_tool_subagent(meta: Option<&Map<String, Value>>) -> bool {
    let Some(codex) = meta
        .and_then(|meta| meta.get("codex"))
        .and_then(Value::as_object)
    else {
        return false;
    };
    if codex.contains_key("subagent") {
        return true;
    }
    codex
        .get("collaboration")
        .and_then(Value::as_object)
        .and_then(|collaboration| collaboration.get("tool"))
        .and_then(Value::as_str)
        .is_some_and(|tool| matches!(tool, "spawnAgent" | "resumeAgent"))
}

/// A codex subagent activity item (`_meta.codex.subagent`): a named agent
/// starting, being messaged, or interrupted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexSubagentActivity {
    pub thread_id: String,
    pub name: String,
}

pub fn codex_subagent_activity(meta: Option<&Map<String, Value>>) -> Option<CodexSubagentActivity> {
    let subagent = meta?
        .get("codex")?
        .as_object()?
        .get("subagent")?
        .as_object()?;
    let thread_id = subagent.get("threadId")?.as_str()?.to_owned();
    let name = subagent
        .get("path")
        .and_then(Value::as_str)
        .and_then(|path| path.split('/').rfind(|part| !part.is_empty()))
        .unwrap_or("subagent")
        .to_owned();
    Some(CodexSubagentActivity { thread_id, name })
}

/// Human label for a collab tool row, in place of the raw tool name.
pub fn codex_collab_label(collab: &CodexCollaboration) -> Option<String> {
    let name = match collab.tool.as_str() {
        "spawnAgent" => "Spawn subagent",
        "resumeAgent" => "Resume subagent",
        "sendInput" => "Message subagent",
        "wait" => "Wait for subagents",
        "closeAgent" => "Close subagent",
        _ => return None,
    };
    let prompt = collab
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty());
    Some(match prompt {
        Some(prompt) => {
            let snippet: String = prompt
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(60)
                .collect();
            format!("{name} \u{2014} {snippet}")
        }
        None => name.to_owned(),
    })
}

/// One line per tracked agent thread for a collab tool's payload.
pub fn format_codex_collaboration(collab: &CodexCollaboration) -> Option<String> {
    if collab.agents_states.is_empty() {
        return None;
    }
    Some(
        collab
            .agents_states
            .iter()
            .map(|(thread, state)| {
                let short: String = thread.chars().take(8).collect();
                match state
                    .message
                    .as_deref()
                    .filter(|message| !message.is_empty())
                {
                    Some(message) => {
                        format!("agent {short}\u{2026} {}: {message}", state.status)
                    }
                    None => format!("agent {short}\u{2026} {}", state.status),
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn push_clean(segments: &mut Vec<Segment>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Segment::Clean(existing)) = segments.last_mut() {
        existing.push_str(text);
    } else {
        segments.push(Segment::Clean(text.to_owned()));
    }
}

fn partial_open_suffix(text: &str, patterns: &[Pattern]) -> usize {
    patterns
        .iter()
        .map(|pattern| {
            let limit = text.len().min(pattern.open.len().saturating_sub(1));
            (1..=limit)
                .rev()
                .find(|length| text.ends_with(&pattern.open[..*length]))
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0)
}

fn parse_task_notification(raw: &str) -> TaskNotification {
    TaskNotification {
        task_id: tag_text(raw, "task-id").unwrap_or_default(),
        tool_use_id: tag_text(raw, "tool-use-id").unwrap_or_default(),
        agent_task: tag_text(raw, "output-file").is_some_and(|file| !file.is_empty()),
        status: tag_text(raw, "status").unwrap_or_default(),
        summary: tag_text(raw, "summary").unwrap_or_default(),
        result_markdown: tag_text(raw, "result").unwrap_or_default(),
    }
}

fn parse_memory_citations(raw: &str) -> Vec<MemoryCitation> {
    let Some(entries) = tag_text(raw, "citation_entries") else {
        return Vec::new();
    };
    entries.lines().filter_map(parse_memory_citation).collect()
}

fn parse_memory_citation(line: &str) -> Option<MemoryCitation> {
    let line = line.trim();
    let (location, note) = line.split_once("|note=[")?;
    let note = note.strip_suffix(']')?;
    let (path, range) = location.rsplit_once(':')?;
    let (line_start, line_end) = range.split_once('-')?;
    Some(MemoryCitation {
        path: path.to_owned(),
        line_start: line_start.parse().ok()?,
        line_end: line_end.parse().ok()?,
        note: note.to_owned(),
    })
}

fn tag_text(raw: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = raw.find(&open)? + open.len();
    let end = raw[start..].find(&close)? + start;
    Some(raw[start..end].trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASK_NOTIFICATION: &str = r#"<task-notification>
<task-id>a27cef9442328a156</task-id>
<tool-use-id>toolu_01U1bVbZb8V55Vx3eLB9Zaxb</tool-use-id>
<output-file>/private/tmp/claude-501/.../tasks/a27cef9442328a156.output</output-file>
<status>completed</status>
<summary>Agent "Fix zz-daemon/zz-terminal slop findings" finished</summary>
<note>A task-notification fires each time this agent stops...</note>
<result>All 12 findings addressed. ...</result>
</task-notification>"#;

    const MEMORY_CITATION: &str = r"<oai-mem-citation>
<citation_entries>
MEMORY.md:872-886|note=[used scoped commit guidance for a shared dirty tree]
MEMORY.md:899-904|note=[used explicit and partial staging guidance]
</citation_entries>
<rollout_ids>
019f7ff0-6773-7c13-9ef5-a46d634a02ff
</rollout_ids>
</oai-mem-citation>";

    #[test]
    fn codex_collaboration_is_parsed_labeled_and_formatted() {
        let meta = serde_json::json!({
            "codex": {
                "collaboration": {
                    "tool": "wait",
                    "senderThreadId": "019fa615-36a5-7482-9f57-b9ba7f7b93bf",
                    "receiverThreadIds": ["019fa615-87ad-7a10-ad7b-13ef2eb87235"],
                },
            },
        });
        let raw_input = serde_json::json!({
            "prompt": null,
            "agentsStates": {
                "019fa615-87ad-7a10-ad7b-13ef2eb87235": {
                    "status": "completed",
                    "message": "done",
                },
            },
        });
        let collab = codex_collaboration(meta.as_object(), Some(&raw_input)).expect("collab");
        assert_eq!(collab.tool, "wait");
        assert_eq!(
            collab.receiver_thread_ids,
            ["019fa615-87ad-7a10-ad7b-13ef2eb87235"]
        );
        assert_eq!(
            collab.agents_states,
            [(
                "019fa615-87ad-7a10-ad7b-13ef2eb87235".to_owned(),
                CodexAgentState {
                    status: "completed".to_owned(),
                    message: Some("done".to_owned()),
                }
            )]
        );
        assert_eq!(
            codex_collab_label(&collab).as_deref(),
            Some("Wait for subagents")
        );
        assert_eq!(
            format_codex_collaboration(&collab).as_deref(),
            Some("agent 019fa615\u{2026} completed: done")
        );

        let spawn_meta = serde_json::json!({
            "codex": {"collaboration": {"tool": "spawnAgent", "receiverThreadIds": ["t1"]}},
        });
        let spawn_input = serde_json::json!({"prompt": "sleep 5s and return done"});
        let spawn = codex_collaboration(spawn_meta.as_object(), Some(&spawn_input)).expect("spawn");
        assert_eq!(
            codex_collab_label(&spawn).as_deref(),
            Some("Spawn subagent \u{2014} sleep 5s and return done")
        );
        assert!(codex_tool_subagent(spawn_meta.as_object()));
        assert!(!codex_tool_subagent(meta.as_object()));

        let activity = serde_json::json!({
            "codex": {"subagent": {"threadId": "t1", "path": "/agents/Pauli", "activity": "started"}},
        });
        assert!(codex_tool_subagent(activity.as_object()));
    }

    #[test]
    fn claude_notification_is_parsed() {
        let mut carry = String::new();
        let segments = scan_text(AgentProvider::ClaudeCode, TASK_NOTIFICATION, &mut carry);

        assert!(carry.is_empty());
        assert_eq!(
            segments,
            [Segment::Notification(TaskNotification {
                task_id: "a27cef9442328a156".to_owned(),
                tool_use_id: "toolu_01U1bVbZb8V55Vx3eLB9Zaxb".to_owned(),
                agent_task: true,
                status: "completed".to_owned(),
                summary: "Agent \"Fix zz-daemon/zz-terminal slop findings\" finished".to_owned(),
                result_markdown: "All 12 findings addressed. ...".to_owned(),
            })]
        );
    }

    #[test]
    fn recognized_envelope_can_cross_chunk_boundaries() {
        let mut carry = String::new();
        let first = scan_text(AgentProvider::ClaudeCode, "Before <task-notifi", &mut carry);
        assert_eq!(first, [Segment::Clean("Before ".to_owned())]);
        assert_eq!(carry, "<task-notifi");

        let second = scan_text(
            AgentProvider::ClaudeCode,
            &TASK_NOTIFICATION["<task-notifi".len()..],
            &mut carry,
        );
        assert!(carry.is_empty());
        assert!(matches!(second.as_slice(), [Segment::Notification(_)]));
    }

    #[test]
    fn unknown_tags_remain_literal() {
        let mut carry = String::new();
        let text = "<task-update>keep me</task-update>";
        assert_eq!(
            scan_text(AgentProvider::ClaudeCode, text, &mut carry),
            [Segment::Clean(text.to_owned())]
        );
        assert!(carry.is_empty());
    }

    #[test]
    fn notification_preserves_surrounding_prose() {
        let mut carry = String::new();
        let text = format!("Before\n{TASK_NOTIFICATION}\nAfter");
        let segments = scan_text(AgentProvider::ClaudeCode, &text, &mut carry);

        assert!(matches!(
            segments.as_slice(),
            [
                Segment::Clean(before),
                Segment::Notification(_),
                Segment::Clean(after)
            ] if before == "Before\n" && after == "\nAfter"
        ));
    }

    #[test]
    fn provider_profiles_do_not_hide_each_others_artifacts() {
        let mut carry = String::new();
        assert_eq!(
            scan_text(AgentProvider::Codex, TASK_NOTIFICATION, &mut carry),
            [Segment::Clean(TASK_NOTIFICATION.to_owned())]
        );
        assert!(carry.is_empty());
    }

    #[test]
    fn codex_memory_envelope_is_parsed_and_stripped() {
        let mut carry = String::new();
        let segments = scan_text(AgentProvider::Codex, MEMORY_CITATION, &mut carry);

        assert!(carry.is_empty());
        assert!(matches!(
            segments.as_slice(),
            [Segment::Stripped {
                kind: "oai-mem-citation",
                memory_citations,
            }] if memory_citations == &[
                MemoryCitation {
                    path: "MEMORY.md".to_owned(),
                    line_start: 872,
                    line_end: 886,
                    note: "used scoped commit guidance for a shared dirty tree".to_owned(),
                },
                MemoryCitation {
                    path: "MEMORY.md".to_owned(),
                    line_start: 899,
                    line_end: 904,
                    note: "used explicit and partial staging guidance".to_owned(),
                },
            ]
        ));
    }

    #[test]
    fn codex_git_directives_are_stripped_but_unknown_actions_are_not() {
        let mut carry = String::new();
        let text = concat!(
            "Done\n\n",
            "::git-stage{cwd=\"/Users/demfabris/Documents/Development/Clairvo/backend\"}\n",
            "::git-commit{cwd=\"/Users/demfabris/Documents/Development/Clairvo/backend\"}\n",
            "::git-push{cwd=\"/repo\" branch=\"main\"}",
        );
        let segments = scan_text(AgentProvider::Codex, text, &mut carry);

        assert!(carry.is_empty());
        assert_eq!(
            segments,
            [
                Segment::Clean("Done\n\n".to_owned()),
                Segment::Stripped {
                    kind: "git-directive",
                    memory_citations: Vec::new(),
                },
                Segment::Clean("\n".to_owned()),
                Segment::Stripped {
                    kind: "git-directive",
                    memory_citations: Vec::new(),
                },
                Segment::Clean("\n::git-push{cwd=\"/repo\" branch=\"main\"}".to_owned()),
            ]
        );
    }

    #[test]
    fn system_reminder_is_the_only_other_claude_artifact_hidden() {
        let mut carry = String::new();
        let segments = scan_text(
            AgentProvider::ClaudeCode,
            "before<system-reminder>private</system-reminder>after",
            &mut carry,
        );

        assert_eq!(
            segments,
            [
                Segment::Clean("before".to_owned()),
                Segment::Stripped {
                    kind: "system-reminder",
                    memory_citations: Vec::new(),
                },
                Segment::Clean("after".to_owned()),
            ]
        );
    }
}
