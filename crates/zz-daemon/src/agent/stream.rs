//! What an agent pane emits. One JSON-serializable item per runtime event:
//! this is simultaneously the journal record, the wire payload the clients
//! deserialize, and the only thing the host hands its sink.
//!
//! Postcard cannot carry the ACP SDK's JSON-shaped types, so everything the
//! adapter produces verbatim (`session/update` payloads, permission options,
//! config options) rides as an opaque `Value` and is re-typed client-side.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zz_protocol::{AgentPaneWire, ClientId, ClientInstanceId};

use crate::agent::turn_snapshot::TurnDiff;

/// One stamped item of a pane's stream. `seq` counts from 1 per pane and never
/// repeats, so a reattaching client replays from the last one it applied.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentStreamItem {
    pub seq: u64,
    #[serde(flatten)]
    pub payload: AgentStreamPayload,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "item")]
pub enum AgentStreamPayload {
    /// The adapter answered `initialize`; nothing about a session yet.
    Ready {
        agent_name: String,
        agent_key: String,
        auth_methods: Vec<AgentAuthMethod>,
        capabilities: AgentSessionCapabilities,
    },
    SessionReset {
        restoring: bool,
    },
    SessionReady {
        session_id: String,
        modes: Option<Value>,
        config_options: Option<Value>,
    },
    StateSynced {
        state: AgentPaneWire,
    },
    SessionsListed {
        client: ClientId,
        sessions: Vec<AgentSessionSummary>,
        next_cursor: Option<String>,
        cwd_filter: Option<PathBuf>,
        replace: bool,
    },
    SessionListFailed {
        client: ClientId,
        message: String,
    },
    SessionSwitched {
        session_id: String,
        cwd: PathBuf,
        modes: Option<Value>,
        config_options: Option<Value>,
        replay: Vec<Value>,
    },
    SessionSwitchFailed {
        message: String,
    },
    SessionDeleted {
        client: ClientId,
        session_id: String,
    },
    SessionDeleteFailed {
        client: ClientId,
        message: String,
    },
    /// A raw `session/update` payload, the transcript's atom.
    Update {
        update: Value,
    },
    PermissionRequested {
        request_id: u64,
        tool_call: Value,
        options: Value,
    },
    PermissionResolved {
        request_id: u64,
        canceled: bool,
    },
    PromptFinished {
        turn_id: u64,
        outcome: AgentPromptOutcome,
    },
    PromptAccepted {
        turn_id: u64,
    },
    TurnStarted {
        turn_id: u64,
    },
    Authenticated,
    AuthenticationFailed {
        message: String,
    },
    ConfigOptionsChanged {
        option_id: String,
        value: String,
        config_options: Value,
    },
    ModeChanged {
        mode_id: String,
    },
    SettingFailed {
        option_id: String,
        message: String,
    },
    PaneFailed {
        message: String,
    },
    /// Prompts the pane had queued and is handing back, so the composer can
    /// refill: a prompt is either sent or visible in the draft again.
    PromptsReclaimed {
        prompts: Vec<AgentPrompt>,
    },
    PromptsRestored {
        reclaim_id: u64,
        prompts: Vec<AgentPrompt>,
    },
    TurnDiff {
        client: ClientId,
        request_id: u64,
        outcome: AgentTurnDiffOutcome,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum AgentPromptOutcome {
    Finished { stop_reason: Value },
    Failed { message: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum AgentTurnDiffOutcome {
    Captured { diff: TurnDiff },
    Unavailable { message: String },
    Failed { message: String },
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPrompt {
    #[serde(default)]
    pub owner: ClientInstanceId,
    #[serde(with = "base64_text")]
    pub text: String,
    pub images: Vec<AgentImage>,
}

impl AgentPrompt {
    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty() && self.images.is_empty()
    }

    pub(crate) fn byte_len(&self) -> usize {
        self.images.iter().fold(self.text.len(), |total, image| {
            total.saturating_add(image.data.len())
        })
    }
}

/// A prompt attachment as it crosses process boundaries: the MIME format the
/// adapter is told about, plus the encoded bytes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentImage {
    pub format: String,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthMethod {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCapabilities {
    pub load: bool,
    pub list: bool,
    pub close: bool,
    pub delete: bool,
    pub additional_directories: bool,
    pub images: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSummary {
    pub session_id: String,
    pub cwd: PathBuf,
    pub additional_directories: Vec<PathBuf>,
    pub title: Option<String>,
    pub updated_at: Option<String>,
}

/// Image bytes are JSON only because the whole stream is; base64 keeps a
/// screenshot from becoming a megabyte-long array of decimal numbers.
mod base64_bytes {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use serde::{Deserialize as _, Deserializer, Serializer, de::Error as _};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        BASE64.decode(&encoded).map_err(D::Error::custom)
    }
}

mod base64_text {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use serde::{Deserialize as _, Deserializer, Serializer, de::Error as _};

    pub(super) fn serialize<S: Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64.encode(text.as_bytes()))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<String, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        let bytes = BASE64.decode(&encoded).map_err(D::Error::custom)?;
        String::from_utf8(bytes).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(item: &AgentStreamItem) -> AgentStreamItem {
        let json = serde_json::to_vec(item).expect("encode stream item");
        serde_json::from_slice(&json).expect("decode stream item")
    }

    #[test]
    fn every_payload_shape_round_trips_through_json() {
        let payloads = [
            AgentStreamPayload::Ready {
                agent_name: "Codex".to_owned(),
                agent_key: "codex".to_owned(),
                auth_methods: vec![AgentAuthMethod {
                    id: "api".to_owned(),
                    name: "API key".to_owned(),
                    description: None,
                }],
                capabilities: AgentSessionCapabilities {
                    load: true,
                    ..AgentSessionCapabilities::default()
                },
            },
            AgentStreamPayload::SessionReset { restoring: true },
            AgentStreamPayload::SessionReady {
                session_id: "s-1".to_owned(),
                modes: Some(serde_json::json!({"currentModeId": "default"})),
                config_options: None,
            },
            AgentStreamPayload::StateSynced {
                state: AgentPaneWire::default(),
            },
            AgentStreamPayload::SessionsListed {
                client: ClientId(3),
                sessions: vec![AgentSessionSummary {
                    session_id: "s-1".to_owned(),
                    cwd: PathBuf::from("/work"),
                    additional_directories: Vec::new(),
                    title: Some("a turn".to_owned()),
                    updated_at: None,
                }],
                next_cursor: Some("c".to_owned()),
                cwd_filter: Some(PathBuf::from("/work")),
                replace: true,
            },
            AgentStreamPayload::SessionListFailed {
                client: ClientId(3),
                message: "no".to_owned(),
            },
            AgentStreamPayload::SessionSwitched {
                session_id: "s-2".to_owned(),
                cwd: PathBuf::from("/work"),
                modes: None,
                config_options: None,
                replay: vec![serde_json::json!({"sessionUpdate": "agent_message_chunk"})],
            },
            AgentStreamPayload::SessionSwitchFailed {
                message: "no".to_owned(),
            },
            AgentStreamPayload::SessionDeleted {
                client: ClientId(3),
                session_id: "s-1".to_owned(),
            },
            AgentStreamPayload::SessionDeleteFailed {
                client: ClientId(3),
                message: "no".to_owned(),
            },
            AgentStreamPayload::Update {
                update: serde_json::json!({"sessionUpdate": "agent_message_chunk"}),
            },
            AgentStreamPayload::PermissionRequested {
                request_id: 7,
                tool_call: serde_json::json!({"toolCallId": "call-1"}),
                options: serde_json::json!([{"optionId": "allow", "kind": "allow_once"}]),
            },
            AgentStreamPayload::PermissionResolved {
                request_id: 7,
                canceled: false,
            },
            AgentStreamPayload::PromptFinished {
                turn_id: 1,
                outcome: AgentPromptOutcome::Finished {
                    stop_reason: serde_json::json!("end_turn"),
                },
            },
            AgentStreamPayload::PromptFinished {
                turn_id: 2,
                outcome: AgentPromptOutcome::Failed {
                    message: "boom".to_owned(),
                },
            },
            AgentStreamPayload::PromptAccepted { turn_id: 2 },
            AgentStreamPayload::Authenticated,
            AgentStreamPayload::AuthenticationFailed {
                message: "denied".to_owned(),
            },
            AgentStreamPayload::ConfigOptionsChanged {
                option_id: "model".to_owned(),
                value: "gpt".to_owned(),
                config_options: serde_json::json!([]),
            },
            AgentStreamPayload::ModeChanged {
                mode_id: "plan".to_owned(),
            },
            AgentStreamPayload::SettingFailed {
                option_id: "model".to_owned(),
                message: "no".to_owned(),
            },
            AgentStreamPayload::PaneFailed {
                message: "gone".to_owned(),
            },
            AgentStreamPayload::PromptsReclaimed {
                prompts: vec![AgentPrompt {
                    owner: ClientInstanceId::default(),
                    text: "retry".to_owned(),
                    images: vec![AgentImage {
                        format: "image/png".to_owned(),
                        data: vec![0, 1, 2, 3, 255],
                    }],
                }],
            },
            AgentStreamPayload::TurnDiff {
                client: ClientId(3),
                request_id: 3,
                outcome: AgentTurnDiffOutcome::Failed {
                    message: "not a worktree".to_owned(),
                },
            },
        ];

        for (index, payload) in payloads.into_iter().enumerate() {
            let seq = index as u64 + 1;
            let item = AgentStreamItem { seq, payload };
            assert_eq!(round_trip(&item), item);
        }
    }

    #[test]
    fn the_seq_and_the_variant_tag_sit_side_by_side_in_the_object() {
        let item = AgentStreamItem {
            seq: 42,
            payload: AgentStreamPayload::SessionReset { restoring: false },
        };
        let encoded = serde_json::to_value(&item).expect("encode");

        assert_eq!(encoded["seq"], serde_json::json!(42));
        assert_eq!(encoded["item"], serde_json::json!("sessionReset"));
        assert_eq!(encoded["restoring"], serde_json::json!(false));
    }

    #[test]
    fn image_bytes_ride_as_base64_rather_than_a_number_array() {
        let item = AgentStreamItem {
            seq: 1,
            payload: AgentStreamPayload::PromptsReclaimed {
                prompts: vec![AgentPrompt {
                    owner: ClientInstanceId::default(),
                    text: String::new(),
                    images: vec![AgentImage {
                        format: "image/png".to_owned(),
                        data: b"zz".to_vec(),
                    }],
                }],
            },
        };
        let encoded = serde_json::to_value(&item).expect("encode");

        assert_eq!(encoded["prompts"][0]["images"][0]["data"], "eno=");
        assert_eq!(round_trip(&item), item);
    }
}
