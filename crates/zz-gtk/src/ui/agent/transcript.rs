use std::collections::VecDeque;

use serde_json::Value;

const MAX_BLOCKS: usize = 300;
const MAX_TEXT_BYTES: usize = 128 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    User,
    Agent,
    Thought,
    Tool,
    Notice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Block {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub text: String,
}

#[derive(Default)]
pub(super) struct Transcript {
    pub cursor: u64,
    pub blocks: VecDeque<Block>,
    pub trimmed: bool,
    stream_open: bool,
    local_echoes: VecDeque<LocalEcho>,
    next_echo: u64,
    replay_until: u64,
    session_id: Option<String>,
}

struct LocalEcho {
    block: Block,
    prompt_text: String,
    after_seq: u64,
}

#[derive(Default)]
pub(super) struct Batch {
    pub needs_replay: bool,
    pub restores: Vec<Value>,
    pub capabilities: Option<Value>,
}

impl Transcript {
    pub fn set_session(&mut self, session_id: &str) -> bool {
        if self.session_id.as_deref() == Some(session_id) {
            return false;
        }
        let changed = self.session_id.is_some();
        self.session_id = Some(session_id.to_owned());
        if changed {
            self.local_echoes.clear();
            self.blocks.clear();
            self.stream_open = false;
            self.trimmed = false;
        }
        changed
    }

    pub fn local_echo(&mut self, text: &str, images: usize) {
        self.next_echo = self.next_echo.saturating_add(1);
        let mut displayed = bounded(text);
        if images > 0 {
            append_bounded(&mut displayed, &format!("\n[{images} image attachments]"));
        }
        let block = Block {
            id: format!("local-{}", self.next_echo),
            kind: Kind::User,
            title: "You".into(),
            text: displayed,
        };
        self.local_echoes.push_back(LocalEcho {
            block: block.clone(),
            prompt_text: bounded(text),
            after_seq: self.cursor,
        });
        self.blocks.push_back(block);
        self.stream_open = false;
        self.trim();
    }

    pub fn prepare_replay(&mut self) {
        self.replay_until = self.replay_until.max(self.cursor);
        self.cursor = 0;
        self.blocks.clear();
        self.stream_open = false;
        self.trimmed = false;
        self.restore_echoes();
    }

    fn restore_echoes(&mut self) {
        for echo in &self.local_echoes {
            if echo.after_seq <= self.cursor
                && !self.blocks.iter().any(|block| block.id == echo.block.id)
            {
                self.blocks.push_back(echo.block.clone());
            }
        }
        self.trim();
    }

    pub fn apply(&mut self, first_seq: u64, items: &[Vec<u8>]) -> Batch {
        let mut result = Batch::default();
        for (offset, bytes) in items.iter().enumerate() {
            let parsed = serde_json::from_slice::<Value>(bytes);
            let seq = parsed
                .as_ref()
                .ok()
                .and_then(|v| v["seq"].as_u64())
                .unwrap_or_else(|| first_seq.saturating_add(offset as u64));
            if seq <= self.cursor {
                continue;
            }
            let restoring = parsed
                .as_ref()
                .is_ok_and(|v| v["item"] == "sessionReset" && v["restoring"] == true);
            if seq.saturating_sub(self.cursor) > 1 && !restoring {
                result.needs_replay = true;
                break;
            }
            self.cursor = seq;
            let Ok(item) = parsed else {
                self.notice(seq, "An Agent update could not be displayed.");
                continue;
            };
            match item["item"].as_str().unwrap_or_default() {
                "ready" => result.capabilities = Some(item["capabilities"].clone()),
                "sessionReady" => {
                    if (seq > self.replay_until || self.session_id.is_none())
                        && let Some(session_id) = item["session_id"].as_str()
                    {
                        self.set_session(session_id);
                    }
                }
                "sessionReset" => {
                    self.blocks.clear();
                    self.trimmed = false;
                    self.stream_open = false;
                    if item["restoring"] != true && seq > self.replay_until {
                        self.local_echoes.clear();
                    }
                }
                "sessionSwitched" => {
                    self.blocks.clear();
                    self.trimmed = false;
                    self.stream_open = false;
                    if seq > self.replay_until {
                        self.local_echoes.clear();
                        if let Some(session_id) = item["session_id"].as_str() {
                            self.set_session(session_id);
                        }
                    }
                    if let Some(updates) = item["replay"].as_array() {
                        for (index, update) in updates.iter().enumerate() {
                            self.update(update, &format!("{seq}-{index}"));
                        }
                    }
                }
                "update" => self.update(&item["update"], &seq.to_string()),
                "paneFailed" | "authenticationFailed" | "sessionSwitchFailed" | "settingFailed" => {
                    self.notice(
                        seq,
                        item["message"]
                            .as_str()
                            .unwrap_or("The Agent request failed."),
                    );
                }
                "turnStarted" | "promptAccepted" => self.stream_open = false,
                "promptFinished" => {
                    self.stream_open = false;
                    if item["outcome"]["outcome"] == "failed" {
                        self.notice(
                            seq,
                            item["outcome"]["message"]
                                .as_str()
                                .unwrap_or("The Agent turn failed."),
                        );
                    }
                }
                "promptsReclaimed" | "promptsRestored" => result.restores.push(item),
                _ => {}
            }
            self.restore_echoes();
        }
        result
    }

    fn notice(&mut self, seq: u64, text: &str) {
        self.blocks.push_back(Block {
            id: format!("notice-{seq}"),
            kind: Kind::Notice,
            title: "Agent".into(),
            text: bounded(text),
        });
        self.trim();
    }

    fn update(&mut self, update: &Value, seq: &str) {
        let kind = match update["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => Kind::Agent,
            "agent_thought_chunk" => Kind::Thought,
            "user_message_chunk" => Kind::User,
            "tool_call" | "tool_call_update" => {
                self.tool(update, seq);
                return;
            }
            _ => return,
        };
        let text = content_text(&update["content"]);
        if text.is_empty() {
            return;
        }
        let is_user = kind == Kind::User;
        let message_id = update["messageId"].as_str();
        let id = message_id.map(|id| format!("{kind:?}-{id}"));
        let existing = if let Some(id) = &id {
            self.blocks.iter_mut().find(|block| &block.id == id)
        } else if self.stream_open {
            self.blocks
                .back_mut()
                .filter(|block| block.kind == kind && block.id.starts_with("chunk-"))
        } else {
            None
        };
        let updated_id = if let Some(existing) = existing {
            append_bounded(&mut existing.text, &text);
            existing.id.clone()
        } else {
            let title = match kind {
                Kind::User => "You",
                Kind::Thought => "Thinking",
                _ => "Agent",
            };
            let id = id.unwrap_or_else(|| format!("chunk-{seq}"));
            self.blocks.push_back(Block {
                id: id.clone(),
                kind,
                title: title.into(),
                text: bounded(&text),
            });
            id
        };
        if is_user
            && let Some(block) = self.blocks.iter().find(|block| block.id == updated_id)
            && let Some(index) = self
                .local_echoes
                .iter()
                .position(|echo| echo.prompt_text == block.text)
            && let Some(echo) = self.local_echoes.remove(index)
        {
            self.blocks.retain(|block| block.id != echo.block.id);
        }
        self.stream_open = true;
        self.trim();
    }

    fn tool(&mut self, update: &Value, seq: &str) {
        let id = format!("tool-{}", update["toolCallId"].as_str().unwrap_or(seq));
        let title = update["title"].as_str();
        let status = update["status"].as_str();
        let text = update["content"].as_array().map(|items| {
            items
                .iter()
                .map(|item| match item["type"].as_str() {
                    Some("content") => content_text(&item["content"]),
                    Some("diff") => format!(
                        "{}\n{}",
                        item["path"].as_str().unwrap_or("Changes"),
                        item["newText"].as_str().unwrap_or_default()
                    ),
                    Some("terminal") => "Terminal command".into(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\n")
        });
        if let Some(block) = self.blocks.iter_mut().find(|block| block.id == id) {
            if let Some(title) = title {
                block.title = bounded(title);
            }
            if let Some(status) = status {
                let base = block.title.split(" · ").next().unwrap_or("Tool");
                block.title = format!("{base} · {}", tool_status(status));
            }
            if let Some(text) = text {
                block.text = bounded(&text);
            }
        } else {
            self.blocks.push_back(Block {
                id,
                kind: Kind::Tool,
                title: format!(
                    "{} · {}",
                    title.unwrap_or("Tool"),
                    tool_status(status.unwrap_or("pending"))
                ),
                text: bounded(text.as_deref().unwrap_or_default()),
            });
        }
        self.trim();
    }

    fn trim(&mut self) {
        let mut bytes: usize = self
            .blocks
            .iter()
            .map(|block| block.text.len() + block.title.len())
            .sum();
        while self.blocks.len() > MAX_BLOCKS || bytes > MAX_TRANSCRIPT_BYTES {
            let Some(block) = self.blocks.pop_front() else {
                break;
            };
            bytes = bytes.saturating_sub(block.text.len() + block.title.len());
            self.trimmed = true;
            self.local_echoes.retain(|echo| echo.block.id != block.id);
        }
    }
}

fn tool_status(status: &str) -> &str {
    match status {
        "pending" => "Pending",
        "in_progress" => "Running",
        "completed" => "Done",
        "failed" => "Failed",
        _ => status,
    }
}

pub(super) fn content_text(content: &Value) -> String {
    match content["type"].as_str().unwrap_or_default() {
        "text" => content["text"].as_str().unwrap_or_default().into(),
        "image" => "[Image]".into(),
        "audio" => "[Audio]".into(),
        "resource_link" => format!(
            "{}\n{}",
            content["title"]
                .as_str()
                .or_else(|| content["name"].as_str())
                .unwrap_or("Resource"),
            content["uri"].as_str().unwrap_or_default()
        ),
        "resource" => content["resource"]["text"]
            .as_str()
            .unwrap_or("[Embedded resource]")
            .into(),
        _ => String::new(),
    }
}

fn bounded(text: &str) -> String {
    let mut result = String::new();
    append_bounded(&mut result, text);
    result
}

fn append_bounded(target: &mut String, text: &str) {
    if target.len() >= MAX_TEXT_BYTES {
        return;
    }
    let available = MAX_TEXT_BYTES.saturating_sub(target.len());
    if text.len() <= available {
        target.push_str(text);
    } else {
        let boundary = text.floor_char_boundary(available.saturating_sub(40));
        target.push_str(&text[..boundary]);
        target.push_str("\n[Long output truncated]");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chunk(seq: u64, tag: &str, text: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"seq":seq,"item":"update","update":{"sessionUpdate":tag,"content":{"type":"text","text":text}}})).unwrap()
    }

    #[test]
    fn replay_deduplicates_and_stops_before_a_gap() {
        let mut thread = Transcript::default();
        let first = chunk(1, "agent_message_chunk", "Hello");
        assert!(!thread.apply(1, std::slice::from_ref(&first)).needs_replay);
        assert!(
            thread
                .apply(1, &[first, chunk(3, "agent_message_chunk", "!")])
                .needs_replay
        );
        assert_eq!(thread.cursor, 1);
        assert_eq!(thread.blocks[0].text, "Hello");
        assert!(
            !thread
                .apply(
                    2,
                    &[
                        chunk(2, "agent_message_chunk", " world"),
                        chunk(3, "agent_message_chunk", "!")
                    ]
                )
                .needs_replay
        );
        assert_eq!(thread.blocks[0].text, "Hello world!");
    }

    #[test]
    fn restoring_reset_allows_a_compacted_journal_start() {
        let mut thread = Transcript::default();
        thread.apply(1, &[chunk(1, "user_message_chunk", "Old")]);
        let reset =
            serde_json::to_vec(&json!({"seq":50,"item":"sessionReset","restoring":true})).unwrap();
        assert!(
            !thread
                .apply(50, &[reset, chunk(51, "user_message_chunk", "New")])
                .needs_replay
        );
        assert_eq!(thread.blocks.len(), 1);
        assert_eq!(thread.blocks[0].text, "New");
    }

    #[test]
    fn unknown_and_malformed_items_do_not_wedge_the_stream() {
        let mut thread = Transcript::default();
        let unknown = serde_json::to_vec(&json!({"seq":2,"item":"futureEvent"})).unwrap();
        assert!(
            !thread
                .apply(
                    1,
                    &[
                        b"{".to_vec(),
                        unknown,
                        chunk(3, "agent_message_chunk", "Ready")
                    ]
                )
                .needs_replay
        );
        assert_eq!(thread.cursor, 3);
        assert_eq!(thread.blocks.back().unwrap().text, "Ready");
    }

    #[test]
    fn transcript_is_bounded_without_breaking_utf8_or_cursor() {
        let mut thread = Transcript::default();
        for seq in 1..=400 {
            let tag = if seq % 2 == 0 {
                "user_message_chunk"
            } else {
                "agent_message_chunk"
            };
            thread.apply(seq, &[chunk(seq, tag, &"🦀".repeat(40_000))]);
        }
        assert_eq!(thread.cursor, 400);
        assert!(thread.trimmed);
        assert!(thread.blocks.len() <= MAX_BLOCKS);
        assert!(
            thread
                .blocks
                .iter()
                .map(|b| b.text.len() + b.title.len())
                .sum::<usize>()
                <= MAX_TRANSCRIPT_BYTES
        );
        assert!(thread.blocks.iter().all(|b| b.text.len() <= MAX_TEXT_BYTES));
    }

    #[test]
    fn tool_deltas_update_the_original_row() {
        let mut thread = Transcript::default();
        let make = |seq, update| {
            serde_json::to_vec(&json!({"seq":seq,"item":"update","update":update})).unwrap()
        };
        thread.apply(1,&[make(1,json!({"sessionUpdate":"tool_call","toolCallId":"x","title":"Read file","status":"in_progress"})),chunk(2,"agent_message_chunk","Thinking"),make(3,json!({"sessionUpdate":"tool_call_update","toolCallId":"x","status":"completed","content":[{"type":"content","content":{"type":"text","text":"contents"}}]}))]);
        assert_eq!(thread.blocks.len(), 2);
        assert_eq!(thread.blocks[0].title, "Read file · Done");
        assert_eq!(thread.blocks[0].text, "contents");
    }

    #[test]
    fn idless_chunks_from_separate_turns_remain_separate_messages() {
        let mut thread = Transcript::default();
        let finish = serde_json::to_vec(
            &json!({"seq":2,"item":"promptFinished","outcome":{"outcome":"finished"}}),
        )
        .unwrap();
        thread.apply(
            1,
            &[
                chunk(1, "agent_message_chunk", "First"),
                finish,
                chunk(3, "agent_message_chunk", "Second"),
            ],
        );
        assert_eq!(thread.blocks.len(), 2);
        assert_eq!(thread.blocks[0].text, "First");
        assert_eq!(thread.blocks[1].text, "Second");
    }

    #[test]
    fn successful_send_is_visible_before_an_adapter_emits_any_message() {
        let mut thread = Transcript::default();
        thread.local_echo("Read the fixture", 0);
        assert_eq!(thread.blocks.len(), 1);
        assert_eq!(thread.blocks[0].kind, Kind::User);
        assert_eq!(thread.blocks[0].text, "Read the fixture");
        thread.apply(1, &[chunk(1, "user_message_chunk", "Read the fixture")]);
        assert_eq!(thread.blocks.len(), 1);
        assert!(thread.local_echoes.is_empty());
    }

    #[test]
    fn replay_keeps_unconfirmed_local_turns_in_conversation_order() {
        let mut thread = Transcript::default();
        let ready = serde_json::to_vec(&json!({"seq":1,"item":"ready"})).unwrap();
        thread.apply(1, std::slice::from_ref(&ready));
        thread.local_echo("First question", 0);
        let first = chunk(2, "agent_message_chunk", "First answer");
        thread.apply(2, std::slice::from_ref(&first));
        thread.local_echo("Second question", 0);
        let second = chunk(3, "agent_message_chunk", "Second answer");
        thread.apply(3, std::slice::from_ref(&second));
        thread.prepare_replay();
        thread.apply(1, &[ready, first, second]);
        assert_eq!(
            thread
                .blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "First question",
                "First answer",
                "Second question",
                "Second answer"
            ]
        );
    }

    #[test]
    fn split_user_chunks_reconcile_one_local_echo_after_accumulating() {
        for message_id in [None, Some("prompt-message")] {
            let mut thread = Transcript::default();
            thread.local_echo("Read the fixture", 0);
            let make = |seq, text| {
                serde_json::to_vec(&json!({"seq":seq,"item":"update","update":{"sessionUpdate":"user_message_chunk","messageId":message_id,"content":{"type":"text","text":text}}})).unwrap()
            };
            thread.apply(1, &[make(1, "Read ")]);
            assert_eq!(thread.local_echoes.len(), 1);
            thread.apply(2, &[make(2, "the "), make(3, "fixture")]);
            assert!(thread.local_echoes.is_empty());
            assert_eq!(thread.blocks.len(), 1);
            assert_eq!(thread.blocks[0].text, "Read the fixture");
        }
    }

    #[test]
    fn authoritative_new_session_discards_old_echoes_after_daemon_restart() {
        let mut thread = Transcript::default();
        thread.set_session("old-session");
        thread.apply(1, &[chunk(1, "agent_message_chunk", "Old reply")]);
        thread.local_echo("Old question", 0);
        thread.prepare_replay();
        assert!(thread.set_session("new-session"));
        let reset =
            serde_json::to_vec(&json!({"seq":1,"item":"sessionReset","restoring":false})).unwrap();
        thread.apply(1, &[reset, chunk(2, "agent_message_chunk", "New reply")]);
        assert!(thread.local_echoes.is_empty());
        assert_eq!(thread.blocks.len(), 1);
        assert_eq!(thread.blocks[0].text, "New reply");
    }

    #[test]
    fn replaying_an_older_session_boundary_keeps_current_session_echoes() {
        let mut thread = Transcript::default();
        let ready =
            serde_json::to_vec(&json!({"seq":1,"item":"sessionReady","session_id":"old-session"}))
                .unwrap();
        let switched = serde_json::to_vec(
            &json!({"seq":2,"item":"sessionSwitched","session_id":"current-session","replay":[]}),
        )
        .unwrap();
        thread.apply(1, &[ready.clone(), switched.clone()]);
        thread.local_echo("Current question", 0);
        thread.prepare_replay();
        assert!(!thread.set_session("current-session"));
        thread.apply(1, &[ready, switched]);
        assert_eq!(thread.session_id.as_deref(), Some("current-session"));
        assert_eq!(thread.blocks.len(), 1);
        assert_eq!(thread.blocks[0].text, "Current question");
    }
}
