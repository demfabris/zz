//! Provider quirks the runtime has to know about, one declaration per adapter:
//! the `_meta` opt-ins it needs, the `_meta` its sessions are opened with, and
//! the extension notification it reports background work through — the claude
//! adapter's raw SDK passthrough, which is how background tasks report at all.
//!
//! A new provider is one [`ProviderProfile`] and the arm that resolves it; the
//! runtime never matches on the provider itself.
//!
//! The prose-level artifact scrubbing that shares this name in the GUI stays
//! there — it shapes a transcript for reading, which is a client concern.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zz_protocol::AgentProvider;

/// Everything one adapter needs the runtime to do differently.
pub struct ProviderProfile {
    /// ACP `_meta` capability opt-ins sent with `initialize`. Every one of them
    /// is a boolean opt-in, so a profile only names the ones it wants.
    meta_caps: &'static [&'static str],
    /// Builds the `_meta` attached to `session/new` and `session/load`. It is a
    /// function rather than a constant because the payload is owned JSON.
    session_meta: Option<fn() -> Map<String, Value>>,
    /// How this adapter reports out of band, if it does.
    ext_messages: Option<ExtMessages>,
}

/// An adapter's extension-notification passthrough: the method it arrives on,
/// and what its params mean. Both halves belong to the profile — a method in
/// another adapter's reserved namespace is not this adapter speaking.
struct ExtMessages {
    method: &'static str,
    parse: fn(&Value) -> Option<(String, SdkMessage)>,
}

static CODEX: ProviderProfile = ProviderProfile {
    meta_caps: &["terminal_output"],
    session_meta: None,
    ext_messages: None,
};

static CLAUDE_CODE: ProviderProfile = ProviderProfile {
    meta_caps: &["terminal_output", "subagent-transcript"],
    session_meta: Some(claude_code::session_meta),
    ext_messages: Some(ExtMessages {
        method: claude_code::SDK_MESSAGE_METHOD,
        parse: claude_code::parse_sdk_message,
    }),
};

impl ProviderProfile {
    pub const fn of(provider: AgentProvider) -> &'static Self {
        match provider {
            AgentProvider::Codex => &CODEX,
            AgentProvider::ClaudeCode => &CLAUDE_CODE,
        }
    }

    pub(crate) fn client_meta_caps(&self) -> Map<String, Value> {
        self.meta_caps
            .iter()
            .map(|capability| ((*capability).to_owned(), Value::Bool(true)))
            .collect()
    }

    pub(crate) fn session_meta(&self) -> Option<Map<String, Value>> {
        self.session_meta.map(|build| build())
    }

    /// What an extension notification tells the daemon, if this adapter is the
    /// one that speaks it. The ACP crate strips the reserved `_` prefix from
    /// method names, so match both; params stay unparsed until the method is
    /// recognized, since every other extension flows through here too.
    pub fn ext_message(&self, method: &str, params: &str) -> Option<(String, SdkMessage)> {
        let ext = self.ext_messages.as_ref()?;
        if method.trim_start_matches('_') != ext.method.trim_start_matches('_') {
            return None;
        }
        (ext.parse)(&serde_json::from_str::<Value>(params).ok()?)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNotification {
    pub task_id: String,
    /// The Task tool call that spawned the agent this notification is about.
    pub tool_use_id: String,
    /// Whether this notification is about an agent task. Background shell tasks
    /// notify too.
    pub agent_task: bool,
    pub status: String,
    pub summary: String,
    pub result_markdown: String,
}

/// A background-task lifecycle event from the SDK passthrough.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SdkTaskEvent {
    /// A background task began, or an existing one was re-attached. `is_agent`
    /// is true for subagent tasks, false for background shells.
    Started {
        task_id: String,
        tool_use_id: String,
        is_agent: bool,
    },
    /// The task settled and the harness produced its notification.
    Notification(TaskNotification),
    /// Terminal status patch. Fires per transition even when the notification
    /// is deduplicated, so it is the settle backstop.
    Settled { task_id: String, status: String },
    /// A tool call is still running. The SDK heartbeats these, so they are
    /// liveness rather than state.
    ToolProgress {
        tool_use_id: String,
        parent_tool_use_id: Option<String>,
        elapsed_seconds: f64,
        subagent_type: Option<String>,
    },
    /// A background task is still running, with whatever it is doing now.
    TaskProgress {
        task_id: String,
        tool_use_id: Option<String>,
        description: String,
        last_tool_name: Option<String>,
        subagent_type: Option<String>,
    },
    /// Every background task the SDK still considers live. REPLACE semantics: a
    /// tracked task absent from this set has ended, however its bookends
    /// arrived. The payload carries no subagent marker, so it may only retire
    /// tasks, never introduce them.
    Reconcile { task_ids: Vec<String> },
}

impl SdkTaskEvent {
    /// A log tag: which event this is, with none of what it carries.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Started { .. } => "task-started",
            Self::Notification(_) => "task-notification",
            Self::Settled { .. } => "task-settled",
            Self::ToolProgress { .. } => "tool-progress",
            Self::TaskProgress { .. } => "task-progress",
            Self::Reconcile { .. } => "task-reconcile",
        }
    }

    /// Whether this event is history rather than liveness, and so belongs in
    /// the journal a rebuilt pane replays.
    ///
    /// The bookends and the reconciled set are what a task row is made of, and
    /// `Reconcile` is the retire backstop for a settle that never arrived. The
    /// two progress events are heartbeats: they say a tool is busy *now*, they
    /// repeat every few seconds, and replaying one would re-hold a tool the
    /// transcript around it has already settled.
    pub const fn is_history(&self) -> bool {
        match self {
            Self::Started { .. }
            | Self::Notification(_)
            | Self::Settled { .. }
            | Self::Reconcile { .. } => true,
            Self::ToolProgress { .. } | Self::TaskProgress { .. } => false,
        }
    }

    /// The task this event names, for the events that name one. Liveness and
    /// the reconciled set do not.
    pub fn task_id(&self) -> Option<&str> {
        match self {
            Self::Started { task_id, .. }
            | Self::Settled { task_id, .. }
            | Self::TaskProgress { task_id, .. } => Some(task_id),
            Self::Notification(notification) => Some(&notification.task_id),
            Self::ToolProgress { .. } | Self::Reconcile { .. } => None,
        }
    }
}

/// What a `_claude/sdkMessage` passthrough carries that the daemon acts on.
#[derive(Clone, Debug, PartialEq)]
pub enum SdkMessage {
    Task(SdkTaskEvent),
    /// The SDK's authoritative turn-over signal. It also closes cycles the
    /// agent continued on its own, which no `session/prompt` response can
    /// settle because none was ever sent.
    TurnIdle,
}

impl SdkMessage {
    /// A log tag: what this passthrough is, with none of what it carries.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Task(event) => event.kind(),
            Self::TurnIdle => "turn-idle",
        }
    }
}

/// The claude adapter: an opt-in filter list on the session, and a raw SDK
/// message passthrough carrying everything about background tasks.
mod claude_code {
    use serde_json::{Map, Value};

    use super::{SdkMessage, SdkTaskEvent, TaskNotification};

    /// The extension notification method the adapter uses for raw SDK message
    /// passthrough.
    pub(super) const SDK_MESSAGE_METHOD: &str = "_claude/sdkMessage";

    /// Longest live-task set the daemon believes. `background_tasks_changed` is
    /// authoritative, so an oversized set is dropped rather than truncated: a
    /// short set would settle work that is still running.
    pub(super) const MAX_RECONCILED_TASKS: usize = 256;

    /// The adapter matches each filter on `type`, then on `subtype` and
    /// `origin` only when the entry carries them, so a bare `type` is a
    /// wildcard over subtypes. `tool_progress` is a top-level message type
    /// rather than a `system` subtype, which is why its entry looks different.
    pub(super) fn session_meta() -> Map<String, Value> {
        Map::from_iter([(
            "claudeCode".to_owned(),
            serde_json::json!({
                "emitRawSDKMessages": [
                    {"type": "system", "subtype": "task_started"},
                    {"type": "system", "subtype": "task_updated"},
                    {"type": "system", "subtype": "task_notification"},
                    {"type": "system", "subtype": "task_progress"},
                    {"type": "system", "subtype": "background_tasks_changed"},
                    {"type": "system", "subtype": "session_state_changed"},
                    {"type": "tool_progress"},
                ]
            }),
        )])
    }

    /// Parse a `_claude/sdkMessage` payload into what it tells the daemon, if
    /// anything. Returns the session id it belongs to alongside.
    pub(super) fn parse_sdk_message(params: &Value) -> Option<(String, SdkMessage)> {
        let session_id = params.get("sessionId")?.as_str()?.to_owned();
        let message = params.get("message")?;
        let text = |key: &str| {
            message
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let present = |key: &str| {
            message
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        };
        // `tool_progress` is a top-level message type, not a `system` subtype.
        if message.get("type")?.as_str()? == "tool_progress" {
            return Some((
                session_id,
                SdkMessage::Task(SdkTaskEvent::ToolProgress {
                    tool_use_id: present("tool_use_id")?,
                    parent_tool_use_id: present("parent_tool_use_id"),
                    elapsed_seconds: message
                        .get("elapsed_time_seconds")
                        .and_then(Value::as_f64)
                        .unwrap_or_default(),
                    subagent_type: present("subagent_type"),
                }),
            ));
        }
        if message.get("type")?.as_str()? != "system" {
            return None;
        }
        let event = match message.get("subtype")?.as_str()? {
            "task_started" => SdkTaskEvent::Started {
                task_id: text("task_id"),
                tool_use_id: text("tool_use_id"),
                is_agent: is_agent_task(message),
            },
            "task_notification" => SdkTaskEvent::Notification(TaskNotification {
                task_id: text("task_id"),
                tool_use_id: text("tool_use_id"),
                agent_task: !text("output_file").is_empty(),
                status: text("status"),
                summary: text("summary"),
                result_markdown: String::new(),
            }),
            "task_updated" => SdkTaskEvent::Settled {
                status: message
                    .get("patch")?
                    .get("status")?
                    .as_str()
                    .filter(|status| matches!(*status, "completed" | "failed" | "killed"))?
                    .to_owned(),
                task_id: text("task_id"),
            },
            "task_progress" => SdkTaskEvent::TaskProgress {
                task_id: present("task_id")?,
                tool_use_id: present("tool_use_id"),
                description: text("description"),
                last_tool_name: present("last_tool_name"),
                subagent_type: present("subagent_type"),
            },
            "background_tasks_changed" => SdkTaskEvent::Reconcile {
                task_ids: reconciled_task_ids(message)?,
            },
            "session_state_changed" => {
                if message.get("state")?.as_str()? != "idle" {
                    return None;
                }
                return Some((session_id, SdkMessage::TurnIdle));
            }
            _ => return None,
        };
        Some((session_id, SdkMessage::Task(event)))
    }

    /// Whether a background task is a subagent rather than a shell. The
    /// reference adapter discriminates on a non-empty `subagent_type`;
    /// `local_agent` is what `background_tasks_changed` entries carry and what
    /// older CLIs reported.
    fn is_agent_task(message: &Value) -> bool {
        message
            .get("subagent_type")
            .and_then(Value::as_str)
            .is_some_and(|subagent_type| !subagent_type.is_empty())
            || message.get("task_type").and_then(Value::as_str) == Some("local_agent")
    }

    /// The live task ids in a `background_tasks_changed` payload, or nothing
    /// when the set is too large to be believed in one wire item.
    fn reconciled_task_ids(message: &Value) -> Option<Vec<String>> {
        let tasks = message.get("tasks")?.as_array()?;
        if tasks.len() > MAX_RECONCILED_TASKS {
            return None;
        }
        Some(
            tasks
                .iter()
                .filter_map(|task| task.get("task_id").and_then(Value::as_str))
                .filter(|task_id| !task_id.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The task event a passthrough carries, for the tests that only care
    /// about that half of the classification.
    fn task_event(params: &Value) -> Option<(String, SdkTaskEvent)> {
        match claude_code::parse_sdk_message(params)? {
            (session_id, SdkMessage::Task(event)) => Some((session_id, event)),
            (_, SdkMessage::TurnIdle) => None,
        }
    }

    #[test]
    fn sdk_message_method_matches_with_and_without_the_reserved_prefix() {
        let params = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "session_state_changed",
                "state": "idle",
            },
        })
        .to_string();
        let claude = ProviderProfile::of(AgentProvider::ClaudeCode);
        assert!(claude.ext_message("_claude/sdkMessage", &params).is_some());
        assert!(claude.ext_message("claude/sdkMessage", &params).is_some());
        assert!(claude.ext_message("claude/other", &params).is_none());
        assert!(
            ProviderProfile::of(AgentProvider::Codex)
                .ext_message("_claude/sdkMessage", &params)
                .is_none(),
            "a reserved namespace belongs to the adapter that declares it"
        );
    }

    #[test]
    fn sdk_task_notification_is_parsed_from_ext_params() {
        let params = serde_json::json!({
            "sessionId": "393d4054-22ba-43f6-b863-8a2fe99c36c3",
            "message": {
                "type": "system",
                "subtype": "task_notification",
                "task_id": "ad993a708fd3fad08",
                "tool_use_id": "toolu_011aRpqniEcXQ4krLLjgYXtU",
                "status": "completed",
                "output_file": "/tmp/tasks/ad993a708fd3fad08.output",
                "summary": "done",
            },
        });
        let (session_id, event) = task_event(&params).expect("task notification");
        let SdkTaskEvent::Notification(notification) = event else {
            panic!("expected notification event");
        };
        assert_eq!(session_id, "393d4054-22ba-43f6-b863-8a2fe99c36c3");
        assert_eq!(notification.task_id, "ad993a708fd3fad08");
        assert_eq!(notification.tool_use_id, "toolu_011aRpqniEcXQ4krLLjgYXtU");
        assert!(notification.agent_task);
        assert_eq!(notification.status, "completed");
        assert_eq!(notification.summary, "done");
        assert!(notification.result_markdown.is_empty());

        let other = serde_json::json!({
            "sessionId": "s",
            "message": {"type": "system", "subtype": "informational", "content": "hi"},
        });
        assert_eq!(task_event(&other), None);
    }

    #[test]
    fn sdk_task_started_and_updated_events_are_parsed() {
        let started = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "task_started",
                "task_id": "a5ee3d5032432ddf6",
                "tool_use_id": "toolu_018pqsELs6Roip3xWKU1ZhtF",
                "task_type": "local_agent",
            },
        });
        assert_eq!(
            task_event(&started),
            Some((
                "s".to_owned(),
                SdkTaskEvent::Started {
                    task_id: "a5ee3d5032432ddf6".to_owned(),
                    tool_use_id: "toolu_018pqsELs6Roip3xWKU1ZhtF".to_owned(),
                    is_agent: true,
                }
            ))
        );

        let shell = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "task_started",
                "task_id": "bc5stcnro",
                "tool_use_id": "toolu_shell",
                "task_type": "local_bash",
            },
        });
        assert!(matches!(
            task_event(&shell),
            Some((
                _,
                SdkTaskEvent::Started {
                    is_agent: false,
                    ..
                }
            ))
        ));

        let updated = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "task_updated",
                "task_id": "a5ee3d5032432ddf6",
                "patch": {"status": "completed", "end_time": 1_785_195_060_210u64},
            },
        });
        assert_eq!(
            task_event(&updated),
            Some((
                "s".to_owned(),
                SdkTaskEvent::Settled {
                    task_id: "a5ee3d5032432ddf6".to_owned(),
                    status: "completed".to_owned(),
                }
            ))
        );

        let nonterminal = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "task_updated",
                "task_id": "a5ee3d5032432ddf6",
                "patch": {"status": "running"},
            },
        });
        assert_eq!(task_event(&nonterminal), None);
    }

    #[test]
    fn session_meta_opts_claude_into_the_task_and_turn_passthrough() {
        let meta = ProviderProfile::of(AgentProvider::ClaudeCode)
            .session_meta()
            .expect("claude session meta");
        let filters = meta["claudeCode"]["emitRawSDKMessages"]
            .as_array()
            .expect("filter list");
        assert_eq!(
            filters,
            &[
                serde_json::json!({"type": "system", "subtype": "task_started"}),
                serde_json::json!({"type": "system", "subtype": "task_updated"}),
                serde_json::json!({"type": "system", "subtype": "task_notification"}),
                serde_json::json!({"type": "system", "subtype": "task_progress"}),
                serde_json::json!({"type": "system", "subtype": "background_tasks_changed"}),
                serde_json::json!({"type": "system", "subtype": "session_state_changed"}),
                serde_json::json!({"type": "tool_progress"}),
            ]
        );
        assert_eq!(
            ProviderProfile::of(AgentProvider::Codex).session_meta(),
            None
        );
    }

    #[test]
    fn a_subagent_type_marks_an_agent_task_and_a_shell_still_does_not() {
        let started = |extra: Value| {
            let mut message = serde_json::json!({
                "type": "system",
                "subtype": "task_started",
                "task_id": "t",
                "tool_use_id": "u",
            });
            let (Value::Object(message_fields), Value::Object(extra)) = (&mut message, extra)
            else {
                panic!("expected objects");
            };
            message_fields.extend(extra);
            serde_json::json!({"sessionId": "s", "message": message})
        };
        let is_agent = |params: &Value| match task_event(params) {
            Some((_, SdkTaskEvent::Started { is_agent, .. })) => is_agent,
            other => panic!("expected a started event, got {other:?}"),
        };

        assert!(is_agent(&started(
            serde_json::json!({"subagent_type": "Explore"})
        )));
        assert!(is_agent(&started(
            serde_json::json!({"task_type": "local_agent"})
        )));
        assert!(!is_agent(&started(
            serde_json::json!({"subagent_type": ""})
        )));
        assert!(!is_agent(&started(
            serde_json::json!({"task_type": "local_bash"})
        )));
        assert!(!is_agent(&started(serde_json::json!({}))));
    }

    #[test]
    fn tool_progress_is_parsed_from_its_top_level_message_type() {
        let params = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "tool_progress",
                "tool_use_id": "toolu_01BashRun",
                "tool_name": "Bash",
                "parent_tool_use_id": "toolu_01TaskParent",
                "elapsed_time_seconds": 30,
                "heartbeat": true,
                "subagent_type": "code-reviewer",
                "uuid": "8a1f0d0e-2f4f-4d5c-9a0b-1f2e3d4c5b6a",
                "session_id": "cli-session",
            },
        });
        assert_eq!(
            task_event(&params),
            Some((
                "s".to_owned(),
                SdkTaskEvent::ToolProgress {
                    tool_use_id: "toolu_01BashRun".to_owned(),
                    parent_tool_use_id: Some("toolu_01TaskParent".to_owned()),
                    elapsed_seconds: 30.0,
                    subagent_type: Some("code-reviewer".to_owned()),
                }
            ))
        );

        let top_level = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "tool_progress",
                "tool_use_id": "toolu_01Plain",
                "tool_name": "Read",
                "parent_tool_use_id": Value::Null,
                "elapsed_time_seconds": 1.5,
            },
        });
        assert_eq!(
            task_event(&top_level),
            Some((
                "s".to_owned(),
                SdkTaskEvent::ToolProgress {
                    tool_use_id: "toolu_01Plain".to_owned(),
                    parent_tool_use_id: None,
                    elapsed_seconds: 1.5,
                    subagent_type: None,
                }
            ))
        );

        let anonymous = serde_json::json!({
            "sessionId": "s",
            "message": {"type": "tool_progress", "elapsed_time_seconds": 2},
        });
        assert_eq!(task_event(&anonymous), None);
    }

    #[test]
    fn task_progress_carries_what_the_task_is_doing_now() {
        let params = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "task_progress",
                "task_id": "a5ee3d5032432ddf6",
                "tool_use_id": "toolu_018pqsELs6Roip3xWKU1ZhtF",
                "description": "Explore the project",
                "subagent_type": "Explore",
                "usage": {"total_tokens": 812, "tool_uses": 3, "duration_ms": 4_210},
                "last_tool_name": "Grep",
            },
        });
        assert_eq!(
            task_event(&params),
            Some((
                "s".to_owned(),
                SdkTaskEvent::TaskProgress {
                    task_id: "a5ee3d5032432ddf6".to_owned(),
                    tool_use_id: Some("toolu_018pqsELs6Roip3xWKU1ZhtF".to_owned()),
                    description: "Explore the project".to_owned(),
                    last_tool_name: Some("Grep".to_owned()),
                    subagent_type: Some("Explore".to_owned()),
                }
            ))
        );

        let bare = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "task_progress",
                "task_id": "bc5stcnro",
                "description": "",
                "usage": {"total_tokens": 0, "tool_uses": 0, "duration_ms": 12},
            },
        });
        assert_eq!(
            task_event(&bare),
            Some((
                "s".to_owned(),
                SdkTaskEvent::TaskProgress {
                    task_id: "bc5stcnro".to_owned(),
                    tool_use_id: None,
                    description: String::new(),
                    last_tool_name: None,
                    subagent_type: None,
                }
            ))
        );
    }

    #[test]
    fn background_tasks_changed_reconciles_the_whole_live_set() {
        let params = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "background_tasks_changed",
                "tasks": [
                    {"task_id": "a5ee", "task_type": "local_agent", "description": "explore"},
                    {"task_id": "bc5s", "task_type": "local_bash", "description": "watch"},
                ],
            },
        });
        assert_eq!(
            task_event(&params),
            Some((
                "s".to_owned(),
                SdkTaskEvent::Reconcile {
                    task_ids: vec!["a5ee".to_owned(), "bc5s".to_owned()],
                }
            ))
        );

        let drained = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "background_tasks_changed",
                "tasks": [],
            },
        });
        assert_eq!(
            task_event(&drained),
            Some((
                "s".to_owned(),
                SdkTaskEvent::Reconcile {
                    task_ids: Vec::new()
                }
            ))
        );

        let tasks = (0..=claude_code::MAX_RECONCILED_TASKS)
            .map(|index| serde_json::json!({"task_id": format!("task-{index}")}))
            .collect::<Vec<_>>();
        let flood = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "background_tasks_changed",
                "tasks": tasks,
            },
        });
        assert_eq!(
            task_event(&flood),
            None,
            "a set past the bound settles live work if it is believed"
        );
    }

    #[test]
    fn an_idle_session_state_is_the_turn_over_signal_and_nothing_else_is() {
        let idle = serde_json::json!({
            "sessionId": "s",
            "message": {
                "type": "system",
                "subtype": "session_state_changed",
                "state": "idle",
            },
        });
        assert_eq!(
            claude_code::parse_sdk_message(&idle),
            Some(("s".to_owned(), SdkMessage::TurnIdle))
        );

        for state in ["running", "requires_action"] {
            let params = serde_json::json!({
                "sessionId": "s",
                "message": {
                    "type": "system",
                    "subtype": "session_state_changed",
                    "state": state,
                },
            });
            assert_eq!(
                claude_code::parse_sdk_message(&params),
                None,
                "{state} is not turn over"
            );
        }
    }

    #[test]
    fn client_capabilities_follow_each_provider_profile() {
        let codex = ProviderProfile::of(AgentProvider::Codex).client_meta_caps();
        assert_eq!(codex.get("terminal_output"), Some(&Value::Bool(true)));
        assert!(!codex.contains_key("subagent-transcript"));

        let claude = ProviderProfile::of(AgentProvider::ClaudeCode).client_meta_caps();
        assert_eq!(claude.get("subagent-transcript"), Some(&Value::Bool(true)));
    }
}
