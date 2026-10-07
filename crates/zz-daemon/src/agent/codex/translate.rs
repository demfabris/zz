use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

const MAX_TITLE_CHARS: usize = 500;

#[derive(Default)]
pub(crate) struct Translator {
    cwd: PathBuf,
    streamed: HashSet<String>,
    commands: HashMap<String, String>,
    prompts: HashSet<String>,
}

impl Translator {
    pub(crate) fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            ..Self::default()
        }
    }

    pub(crate) fn expect_prompt(&mut self, text: &str) {
        self.prompts.insert(text.trim().to_owned());
    }

    pub(crate) fn notification(&mut self, method: &str, params: &Value) -> Vec<Value> {
        match method {
            "item/started" => self.started(&params["item"]),
            "item/completed" => self.completed(&params["item"]),
            "item/agentMessage/delta" => self.delta("agent_message_chunk", params),
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                self.delta("agent_thought_chunk", params)
            }
            "item/commandExecution/outputDelta" => {
                let (Some(id), Some(delta)) = (params["itemId"].as_str(), params["delta"].as_str())
                else {
                    return Vec::new();
                };
                let output = self.commands.entry(id.to_owned()).or_default();
                output.push_str(delta);
                vec![json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": id,
                    "content": [text_content(&tail(output))],
                })]
            }
            "turn/plan/updated" => vec![json!({
                "sessionUpdate": "plan",
                "entries": params["plan"].as_array().into_iter().flatten().map(|step| json!({
                    "content": step["step"].as_str().unwrap_or_default(),
                    "priority": "medium",
                    "status": match step["status"].as_str() {
                        Some("inProgress" | "in_progress") => "in_progress",
                        Some("completed") => "completed",
                        _ => "pending",
                    },
                })).collect::<Vec<_>>(),
            })],
            "thread/tokenUsage/updated" => {
                let usage = &params["tokenUsage"];
                let Some(size) = usage["modelContextWindow"].as_u64() else {
                    return Vec::new();
                };
                let used = usage["last"]["totalTokens"].as_u64().unwrap_or_default();
                vec![json!({ "sessionUpdate": "usage_update", "used": used, "size": size })]
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn history_item(&mut self, item: &Value) -> Vec<Value> {
        if item["type"] == "userMessage" {
            return user_text(item)
                .map(|text| vec![chunk("user_message_chunk", &id_of(item), &text)])
                .unwrap_or_default();
        }
        let mut updates = self.started(item);
        updates.extend(self.completed(item));
        updates
    }

    fn delta(&mut self, kind: &str, params: &Value) -> Vec<Value> {
        let (Some(id), Some(delta)) = (params["itemId"].as_str(), params["delta"].as_str()) else {
            return Vec::new();
        };
        if delta.is_empty() {
            return Vec::new();
        }
        self.streamed.insert(id.to_owned());
        vec![chunk(kind, id, delta)]
    }

    fn started(&mut self, item: &Value) -> Vec<Value> {
        let id = id_of(item);
        let Some(info) = tool_info(item, &self.cwd) else {
            return Vec::new();
        };
        let mut call = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": info.title,
            "kind": info.kind,
            "status": "in_progress",
            "rawInput": info.input,
            "_meta": { "codex": { "itemType": item["type"] } },
        });
        if !info.content.is_empty() {
            call["content"] = Value::Array(info.content);
        }
        if !info.locations.is_empty() {
            call["locations"] = Value::Array(info.locations);
        }
        vec![call]
    }

    fn completed(&mut self, item: &Value) -> Vec<Value> {
        let id = id_of(item);
        match item["type"].as_str() {
            Some("userMessage") => {
                let Some(text) = user_text(item) else {
                    return Vec::new();
                };
                if self.prompts.remove(text.trim()) {
                    return Vec::new();
                }
                vec![chunk("user_message_chunk", &id, &text)]
            }
            Some("agentMessage" | "plan") => {
                self.whole(&id, "agent_message_chunk", item["text"].as_str())
            }
            Some("reasoning") => {
                let text = item["summary"]
                    .as_array()
                    .into_iter()
                    .chain(item["content"].as_array())
                    .flatten()
                    .filter_map(|part| part.as_str().or_else(|| part["text"].as_str()))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                self.whole(&id, "agent_thought_chunk", Some(&text))
            }
            Some(_) => {
                let Some(status) = tool_status(item) else {
                    return Vec::new();
                };
                let mut update = json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": id,
                    "status": status,
                });
                let streamed = self.commands.remove(&id);
                if let Some(output) = tool_output(item)
                    .or(streamed)
                    .filter(|output| !output.trim().is_empty())
                {
                    update["content"] = json!([text_content(&tail(&output))]);
                }
                vec![update]
            }
            None => Vec::new(),
        }
    }

    fn whole(&mut self, id: &str, kind: &str, text: Option<&str>) -> Vec<Value> {
        if self.streamed.remove(id) {
            return Vec::new();
        }
        text.filter(|text| !text.trim().is_empty())
            .map(|text| vec![chunk(kind, id, text)])
            .unwrap_or_default()
    }
}

fn id_of(item: &Value) -> String {
    item["id"].as_str().unwrap_or("item").to_owned()
}

fn user_text(item: &Value) -> Option<String> {
    let text = item["content"]
        .as_array()?
        .iter()
        .filter(|part| part["type"] == "text")
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

fn chunk(kind: &str, message_id: &str, text: &str) -> Value {
    json!({
        "sessionUpdate": kind,
        "messageId": message_id,
        "content": { "type": "text", "text": text },
    })
}

fn text_content(text: &str) -> Value {
    json!({ "type": "content", "content": { "type": "text", "text": text } })
}

const MAX_OUTPUT_CHARS: usize = 256 * 1024;

fn tail(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_OUTPUT_CHARS {
        return text.trim_end().to_owned();
    }
    text.chars()
        .skip(count - MAX_OUTPUT_CHARS)
        .collect::<String>()
}

fn tool_status(item: &Value) -> Option<&'static str> {
    let status = item["status"].as_str()?;
    Some(match status {
        "completed" if item["exitCode"].as_i64().is_some_and(|code| code != 0) => "failed",
        "failed" | "declined" | "error" => "failed",
        "inProgress" | "in_progress" => "in_progress",
        _ => "completed",
    })
}

fn tool_output(item: &Value) -> Option<String> {
    match item["type"].as_str()? {
        "commandExecution" => item["aggregatedOutput"].as_str().map(str::to_owned),
        "mcpToolCall" => {
            if let Some(error) = item["error"]["message"].as_str() {
                return Some(error.to_owned());
            }
            let parts = item["result"]["content"]
                .as_array()?
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        "dynamicToolCall" => {
            let parts = item["contentItems"]
                .as_array()?
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        "collabAgentToolCall" => item["agentsStates"]
            .as_object()
            .map(|states| {
                states
                    .values()
                    .filter_map(|state| state["message"].as_str().or(state["status"].as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|text| !text.is_empty()),
        _ => None,
    }
}

pub(crate) struct ToolInfo {
    pub(crate) title: String,
    pub(crate) kind: &'static str,
    pub(crate) input: Value,
    pub(crate) content: Vec<Value>,
    pub(crate) locations: Vec<Value>,
}

pub(crate) fn tool_info(item: &Value, cwd: &Path) -> Option<ToolInfo> {
    let (title, kind, content, locations) = match item["type"].as_str()? {
        "commandExecution" => {
            let action = item["commandActions"]
                .as_array()
                .and_then(|actions| actions.first());
            let command = action
                .and_then(|action| action["command"].as_str())
                .or_else(|| item["command"].as_str())
                .unwrap_or("command");
            match action.and_then(|action| action["type"].as_str()) {
                Some("read") => {
                    let path = action.and_then(|action| action["path"].as_str());
                    let shown =
                        path.map_or_else(|| command.to_owned(), |path| display_path(path, cwd));
                    (
                        format!("Read {shown}"),
                        "read",
                        Vec::new(),
                        path.map(|path| vec![json!({ "path": path })])
                            .unwrap_or_default(),
                    )
                }
                Some("listFiles" | "search") => {
                    (command.to_owned(), "search", Vec::new(), Vec::new())
                }
                _ => (command.to_owned(), "execute", Vec::new(), Vec::new()),
            }
        }
        "fileChange" => {
            let changes = item["changes"].as_array().cloned().unwrap_or_default();
            let paths = changes
                .iter()
                .filter_map(|change| change["path"].as_str())
                .collect::<Vec<_>>();
            let title = match paths.as_slice() {
                [only] => format!("Edit {}", display_path(only, cwd)),
                [] => "Edit".to_owned(),
                many => format!("Edit {} files", many.len()),
            };
            let content = changes
                .iter()
                .filter_map(|change| {
                    let path = change["path"].as_str()?;
                    let (old, new) = split_diff(change["diff"].as_str().unwrap_or_default());
                    let created = change["kind"]["type"] == "add";
                    Some(json!({
                        "type": "diff",
                        "path": path,
                        "oldText": if created { Value::Null } else { Value::from(old) },
                        "newText": new,
                    }))
                })
                .collect();
            let locations = paths.iter().map(|path| json!({ "path": path })).collect();
            (title, "edit", content, locations)
        }
        "mcpToolCall" => (
            format!(
                "{}: {}",
                item["server"].as_str().unwrap_or("mcp"),
                item["tool"].as_str().unwrap_or("tool")
            ),
            "other",
            Vec::new(),
            Vec::new(),
        ),
        "dynamicToolCall" => (
            item["tool"].as_str().unwrap_or("tool").to_owned(),
            "other",
            Vec::new(),
            Vec::new(),
        ),
        "webSearch" => (
            item["query"].as_str().map_or_else(
                || "Web search".to_owned(),
                |query| format!("Search \"{query}\""),
            ),
            "fetch",
            Vec::new(),
            Vec::new(),
        ),
        "collabAgentToolCall" => (
            item["prompt"]
                .as_str()
                .map_or_else(|| "Agent".to_owned(), str::to_owned),
            "think",
            Vec::new(),
            Vec::new(),
        ),
        "imageView" => (
            format!(
                "View {}",
                display_path(item["path"].as_str().unwrap_or("image"), cwd)
            ),
            "read",
            Vec::new(),
            Vec::new(),
        ),
        "imageGeneration" => ("Generate image".to_owned(), "other", Vec::new(), Vec::new()),
        "contextCompaction" => (
            "Compact context".to_owned(),
            "think",
            Vec::new(),
            Vec::new(),
        ),
        _ => return None,
    };
    let mut input = item.clone();
    if let Some(fields) = input.as_object_mut() {
        for key in [
            "aggregatedOutput",
            "result",
            "contentItems",
            "changes",
            "status",
        ] {
            fields.remove(key);
        }
    }
    Some(ToolInfo {
        title: clip_title(&title),
        kind,
        input,
        content,
        locations,
    })
}

fn split_diff(diff: &str) -> (String, String) {
    let mut old = String::new();
    let mut new = String::new();
    for line in diff.lines() {
        if line.starts_with("@@") || line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('-') {
            old.push_str(rest);
            old.push('\n');
        } else if let Some(rest) = line.strip_prefix('+') {
            new.push_str(rest);
            new.push('\n');
        } else {
            let rest = line.strip_prefix(' ').unwrap_or(line);
            old.push_str(rest);
            old.push('\n');
            new.push_str(rest);
            new.push('\n');
        }
    }
    (old, new)
}

fn clip_title(title: &str) -> String {
    let line = title.lines().next().unwrap_or_default();
    let multiline = title.lines().nth(1).is_some();
    let mut clipped = line.chars().take(MAX_TITLE_CHARS).collect::<String>();
    if multiline || line.chars().count() > MAX_TITLE_CHARS {
        clipped.push('…');
    }
    clipped
}

fn display_path(path: &str, cwd: &Path) -> String {
    Path::new(path)
        .strip_prefix(cwd)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map_or_else(
            || path.to_owned(),
            |relative| relative.display().to_string(),
        )
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::SessionUpdate;

    use super::*;

    fn replay() -> Vec<Value> {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/agent/codex/fixtures/tools.ndjson");
        let mut translator = Translator::new(PathBuf::from("/work"));
        translator.expect_prompt("Reply with exactly: hi there");
        let updates = std::fs::read_to_string(&path)
            .expect("fixture")
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|record| record["dir"] == "out")
            .map(|record| record["msg"].clone())
            .filter(|message| message.get("id").is_none())
            .flat_map(|message| {
                translator.notification(
                    message["method"].as_str().unwrap_or_default(),
                    &message["params"],
                )
            })
            .collect::<Vec<_>>();
        for update in &updates {
            serde_json::from_value::<SessionUpdate>(update.clone())
                .unwrap_or_else(|error| panic!("{update} is not an ACP update: {error}"));
        }
        updates
    }

    #[test]
    fn a_recorded_codex_turn_becomes_rows_and_streamed_text() {
        let updates = replay();
        let text = updates
            .iter()
            .filter(|update| update["sessionUpdate"] == "agent_message_chunk")
            .filter_map(|update| update["content"]["text"].as_str())
            .collect::<String>();
        assert!(text.starts_with("hi there"), "{text}");
        assert!(text.ends_with("done"), "{text}");
        let calls = updates
            .iter()
            .filter(|update| update["sessionUpdate"] == "tool_call")
            .collect::<Vec<_>>();
        assert_eq!(calls[0]["title"], "echo cx > cx.txt && cat notes.txt");
        assert_eq!(calls[0]["kind"], "execute");
        assert_eq!(calls[1]["title"], "Edit notes.txt");
        assert_eq!(calls[1]["content"][0]["oldText"], "hullo\n");
        assert_eq!(calls[1]["content"][0]["newText"], "hallo\n");
        assert_eq!(calls[2]["title"], "Read notes.txt");
        assert_eq!(calls[2]["kind"], "read");
        let done = updates
            .iter()
            .find(|update| {
                update["sessionUpdate"] == "tool_call_update"
                    && update["toolCallId"] == calls[0]["toolCallId"]
                    && update.get("status").is_some()
            })
            .expect("command completion");
        assert_eq!(done["status"], "completed");
        assert_eq!(done["content"][0]["content"]["text"], "hullo");
        assert!(
            updates
                .iter()
                .any(|update| update["sessionUpdate"] == "usage_update")
        );
        assert!(
            !updates
                .iter()
                .any(|update| update["sessionUpdate"] == "user_message_chunk"
                    && update["content"]["text"] == "Reply with exactly: hi there"),
            "the prompt echo stays with the host"
        );
    }

    #[test]
    fn a_failed_command_and_an_added_file_render_honestly() {
        let mut translator = Translator::new(PathBuf::from("/work"));
        let failed = json!({"type":"commandExecution","id":"c1","command":"false","commandActions":[],
            "status":"completed","exitCode":1,"aggregatedOutput":"boom"});
        translator.notification("item/started", &json!({ "item": failed }));
        let updates = translator.notification("item/completed", &json!({ "item": failed }));
        assert_eq!(updates[0]["status"], "failed");
        let added = json!({"type":"fileChange","id":"f1","status":"completed","changes":[
            {"path":"/work/new.rs","kind":{"type":"add"},"diff":"+fn main() {}\n"}]});
        let updates = translator.notification("item/started", &json!({ "item": added }));
        assert_eq!(updates[0]["content"][0]["oldText"], Value::Null);
        assert_eq!(updates[0]["content"][0]["newText"], "fn main() {}\n");
    }
}
