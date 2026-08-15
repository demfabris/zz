//! Provider quirks the runtime has to know about: the `_meta` opt-ins each
//! adapter needs, and the claude adapter's raw SDK message passthrough, which
//! is how background tasks report at all.
//!
//! The prose-level artifact scrubbing that shares this name in the GUI stays
//! there — it shapes a transcript for reading, which is a client concern.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zz_protocol::AgentProvider;

/// Provider-specific ACP `_meta` capability opt-ins.
pub(crate) fn client_meta_caps(provider: AgentProvider) -> Map<String, Value> {
    let mut capabilities = Map::from_iter([("terminal_output".to_owned(), Value::Bool(true))]);
    if provider == AgentProvider::ClaudeCode {
        capabilities.insert("subagent-transcript".to_owned(), Value::Bool(true));
    }
    capabilities
}

/// The extension notification method the claude adapter uses for raw SDK
/// message passthrough.
const SDK_MESSAGE_METHOD: &str = "_claude/sdkMessage";

/// Whether an incoming extension notification is the SDK passthrough. The ACP
/// crate strips the reserved `_` prefix from method names, so match both.
pub(crate) fn is_sdk_message_method(method: &str) -> bool {
    method.trim_start_matches('_') == SDK_MESSAGE_METHOD.trim_start_matches('_')
}

/// Provider-specific `_meta` attached to `session/new` and `session/load`.
pub(crate) fn session_meta(provider: AgentProvider) -> Option<Map<String, Value>> {
    match provider {
        AgentProvider::ClaudeCode => Some(Map::from_iter([(
            "claudeCode".to_owned(),
            serde_json::json!({
                "emitRawSDKMessages": [
                    {"type": "system", "subtype": "task_started"},
                    {"type": "system", "subtype": "task_updated"},
                    {"type": "system", "subtype": "task_notification"},
                ]
            }),
        )])),
        AgentProvider::Codex => None,
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
}

/// Parse a `_claude/sdkMessage` payload into the task lifecycle event it
/// carries, if it is one. Returns the session id it belongs to alongside.
pub(crate) fn parse_sdk_task_event(params: &Value) -> Option<(String, SdkTaskEvent)> {
    let session_id = params.get("sessionId")?.as_str()?.to_owned();
    let message = params.get("message")?;
    if message.get("type")?.as_str()? != "system" {
        return None;
    }
    let text = |key: &str| {
        message
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let event = match message.get("subtype")?.as_str()? {
        "task_started" => SdkTaskEvent::Started {
            task_id: text("task_id"),
            tool_use_id: text("tool_use_id"),
            is_agent: message.get("task_type").and_then(Value::as_str) == Some("local_agent"),
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
        _ => return None,
    };
    Some((session_id, event))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_message_method_matches_with_and_without_the_reserved_prefix() {
        assert!(is_sdk_message_method("_claude/sdkMessage"));
        assert!(is_sdk_message_method("claude/sdkMessage"));
        assert!(!is_sdk_message_method("claude/other"));
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
        let (session_id, event) = parse_sdk_task_event(&params).expect("task notification");
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
        assert_eq!(parse_sdk_task_event(&other), None);
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
            parse_sdk_task_event(&started),
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
            parse_sdk_task_event(&shell),
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
            parse_sdk_task_event(&updated),
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
        assert_eq!(parse_sdk_task_event(&nonterminal), None);
    }

    #[test]
    fn session_meta_opts_claude_into_task_notification_passthrough() {
        let meta = session_meta(AgentProvider::ClaudeCode).expect("claude session meta");
        let filters = meta["claudeCode"]["emitRawSDKMessages"]
            .as_array()
            .expect("filter list");
        assert_eq!(
            filters,
            &[
                serde_json::json!({"type": "system", "subtype": "task_started"}),
                serde_json::json!({"type": "system", "subtype": "task_updated"}),
                serde_json::json!({"type": "system", "subtype": "task_notification"}),
            ]
        );
        assert_eq!(session_meta(AgentProvider::Codex), None);
    }

    #[test]
    fn client_capabilities_follow_each_provider_profile() {
        let codex = client_meta_caps(AgentProvider::Codex);
        assert_eq!(codex.get("terminal_output"), Some(&Value::Bool(true)));
        assert!(!codex.contains_key("subagent-transcript"));

        let claude = client_meta_caps(AgentProvider::ClaudeCode);
        assert_eq!(claude.get("subagent-transcript"), Some(&Value::Bool(true)));
    }
}
