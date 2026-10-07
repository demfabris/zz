use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use std::fmt::Write as _;

use serde_json::{Value, json};

const MAX_TITLE_CHARS: usize = 500;

#[derive(Clone, Debug, Default, PartialEq)]
struct PlanItem {
    content: String,
    active_form: Option<String>,
    status: String,
}

impl PlanItem {
    fn entry(&self) -> Value {
        let content = match (&self.active_form, self.status.as_str()) {
            (Some(active), "in_progress") if !active.is_empty() => active,
            _ => &self.content,
        };
        json!({ "content": content, "priority": "medium", "status": self.status })
    }
}

#[derive(Default)]
pub(crate) struct Translator {
    cwd: PathBuf,
    current_message: HashMap<String, String>,
    open_blocks: HashMap<String, u64>,
    whole_blocks: HashMap<String, u64>,
    streamed: HashSet<String>,
    tools: HashMap<String, String>,
    hidden_tools: HashSet<String>,
    summaries: HashMap<String, String>,
    todos: Vec<PlanItem>,
    tasks: Vec<(String, PlanItem)>,
    pending_tasks: HashMap<String, PlanItem>,
}

impl Translator {
    pub(crate) fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            ..Self::default()
        }
    }

    pub(crate) fn frame(&mut self, frame: &Value) -> Vec<Value> {
        match frame["type"].as_str() {
            Some("stream_event") => self.stream_event(frame),
            Some("assistant") => self.assistant(frame),
            Some("user") => self.user(frame),
            Some("system") => self.system(frame),
            Some("result") => self.result(frame),
            _ => Vec::new(),
        }
    }

    pub(crate) fn history_entry(&mut self, entry: &Value) -> Vec<Value> {
        if entry["isSidechain"] == true || entry["isMeta"] == true {
            return Vec::new();
        }
        match entry["type"].as_str() {
            Some("assistant") => self.assistant(entry),
            Some("user") => {
                let mut updates = self.user_text(entry);
                updates.extend(self.tool_results(entry));
                updates
            }
            _ => Vec::new(),
        }
    }

    fn system(&mut self, frame: &Value) -> Vec<Value> {
        match frame["subtype"].as_str() {
            Some("task_notification") => {
                if let (Some(tool_use_id), Some(summary)) = (
                    frame["tool_use_id"].as_str(),
                    frame["summary"]
                        .as_str()
                        .filter(|summary| !summary.is_empty()),
                ) {
                    self.summaries
                        .insert(tool_use_id.to_owned(), summary.to_owned());
                }
                Vec::new()
            }
            Some("local_command_output") => frame["content"]
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(|text| {
                    let id = frame["uuid"].as_str().unwrap_or("local-command");
                    vec![chunk("agent_message_chunk", id, text)]
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn stream_event(&mut self, frame: &Value) -> Vec<Value> {
        let parent = parent_key(frame);
        let event = &frame["event"];
        match event["type"].as_str() {
            Some("message_start") => {
                if let Some(id) = event["message"]["id"].as_str() {
                    self.current_message.insert(parent, id.to_owned());
                }
                Vec::new()
            }
            Some("content_block_start") => {
                if let (Some(message), Some(index)) =
                    (self.current_message.get(&parent), event["index"].as_u64())
                {
                    self.open_blocks.insert(message.clone(), index);
                }
                Vec::new()
            }
            Some("content_block_delta") if parent.is_empty() => {
                let (Some(message), Some(index)) =
                    (self.current_message.get(&parent), event["index"].as_u64())
                else {
                    return Vec::new();
                };
                let delta = &event["delta"];
                let (kind, text) = match delta["type"].as_str() {
                    Some("text_delta") => ("agent_message_chunk", delta["text"].as_str()),
                    Some("thinking_delta") => ("agent_thought_chunk", delta["thinking"].as_str()),
                    _ => return Vec::new(),
                };
                let Some(text) = text.filter(|text| !text.is_empty()) else {
                    return Vec::new();
                };
                let key = format!("{message}:{index}");
                let update = chunk(kind, &key, text);
                self.streamed.insert(key);
                vec![update]
            }
            _ => Vec::new(),
        }
    }

    fn block_key(&mut self, message_id: &str) -> String {
        if let Some(index) = self.open_blocks.remove(message_id) {
            return format!("{message_id}:{index}");
        }
        let next = self.whole_blocks.entry(message_id.to_owned()).or_default();
        *next += 1;
        format!("{message_id}#{next}")
    }

    fn assistant(&mut self, frame: &Value) -> Vec<Value> {
        let parent = frame["parent_tool_use_id"].as_str().map(str::to_owned);
        let message = &frame["message"];
        let message_id = message["id"]
            .as_str()
            .or_else(|| frame["uuid"].as_str())
            .unwrap_or("assistant")
            .to_owned();
        let mut updates = Vec::new();
        for block in message["content"].as_array().into_iter().flatten() {
            let key = self.block_key(&message_id);
            match block["type"].as_str() {
                Some("text") if parent.is_none() => {
                    if !self.streamed.remove(&key)
                        && let Some(text) = block["text"].as_str().filter(|text| !text.is_empty())
                    {
                        updates.push(chunk("agent_message_chunk", &key, text));
                    }
                }
                Some("thinking") if parent.is_none() => {
                    if !self.streamed.remove(&key)
                        && let Some(text) =
                            block["thinking"].as_str().filter(|text| !text.is_empty())
                    {
                        updates.push(chunk("agent_thought_chunk", &key, text));
                    }
                }
                Some("tool_use" | "server_tool_use" | "mcp_tool_use") => {
                    updates.extend(self.tool_use(block, parent.as_deref()));
                }
                _ => {}
            }
        }
        updates
    }

    fn tool_use(&mut self, block: &Value, parent: Option<&str>) -> Vec<Value> {
        let Some(id) = block["id"].as_str() else {
            return Vec::new();
        };
        let name = block["name"].as_str().unwrap_or("Tool").to_owned();
        let input = block["input"].clone();
        self.tools.insert(id.to_owned(), name.clone());
        match name.as_str() {
            "TodoWrite" => {
                self.hidden_tools.insert(id.to_owned());
                self.todos = input["todos"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|todo| PlanItem {
                        content: todo["content"].as_str().unwrap_or_default().to_owned(),
                        active_form: todo["activeForm"].as_str().map(str::to_owned),
                        status: plan_status(todo["status"].as_str()),
                    })
                    .collect();
                return vec![plan(self.todos.iter())];
            }
            "TaskCreate" => {
                self.hidden_tools.insert(id.to_owned());
                self.pending_tasks.insert(
                    id.to_owned(),
                    PlanItem {
                        content: input["subject"].as_str().unwrap_or_default().to_owned(),
                        active_form: input["activeForm"].as_str().map(str::to_owned),
                        status: "pending".to_owned(),
                    },
                );
                return Vec::new();
            }
            "TaskUpdate" => {
                self.hidden_tools.insert(id.to_owned());
                let task_id = input["taskId"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| input["taskId"].as_u64().map(|id| id.to_string()));
                let Some(task_id) = task_id else {
                    return Vec::new();
                };
                if input["status"].as_str() == Some("deleted") {
                    self.tasks.retain(|(id, _)| *id != task_id);
                } else if let Some((_, task)) = self.tasks.iter_mut().find(|(id, _)| *id == task_id)
                {
                    if let Some(status) = input["status"].as_str() {
                        task.status = plan_status(Some(status));
                    }
                    if let Some(subject) = input["subject"].as_str() {
                        subject.clone_into(&mut task.content);
                    }
                    if let Some(active) = input["activeForm"].as_str() {
                        task.active_form = Some(active.to_owned());
                    }
                } else {
                    return Vec::new();
                }
                return vec![plan(self.tasks.iter().map(|(_, task)| task))];
            }
            "TaskList" | "TaskGet" => {
                self.hidden_tools.insert(id.to_owned());
                return Vec::new();
            }
            _ => {}
        }
        let info = tool_info(&name, &input, &self.cwd);
        let mut call = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": info.title,
            "kind": info.kind,
            "status": "in_progress",
            "rawInput": input,
            "_meta": { "claudeCode": { "toolName": name } },
        });
        if !info.content.is_empty() {
            call["content"] = Value::Array(info.content);
        }
        if !info.locations.is_empty() {
            call["locations"] = Value::Array(info.locations);
        }
        if let Some(parent) = parent {
            call["_meta"]["claudeCode"]["parentToolUseId"] = Value::from(parent);
        }
        vec![call]
    }

    fn user(&mut self, frame: &Value) -> Vec<Value> {
        let mut updates = Vec::new();
        if frame["isReplay"] != true
            && frame["parent_tool_use_id"].is_null()
            && frame["isSynthetic"] != true
        {
            updates.extend(self.user_text(frame));
        }
        updates.extend(self.tool_results(frame));
        updates
    }

    fn user_text(&self, frame: &Value) -> Vec<Value> {
        if !frame["parent_tool_use_id"].is_null() && frame["parent_tool_use_id"].is_string() {
            return Vec::new();
        }
        let id = frame["uuid"].as_str().unwrap_or("user");
        let content = &frame["message"]["content"];
        let text = match content {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        if text.trim().is_empty() || is_internal_user_text(&text) {
            return Vec::new();
        }
        vec![chunk("user_message_chunk", id, &text)]
    }

    fn tool_results(&mut self, frame: &Value) -> Vec<Value> {
        let structured = frame
            .get("tool_use_result")
            .or_else(|| frame.get("toolUseResult"));
        frame["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "tool_result")
            .flat_map(|block| self.tool_result(block, structured))
            .collect()
    }

    fn tool_result(&mut self, block: &Value, structured: Option<&Value>) -> Vec<Value> {
        let Some(id) = block["tool_use_id"].as_str() else {
            return Vec::new();
        };
        let is_error = block["is_error"] == true;
        let text = result_text(&block["content"]);
        if self.hidden_tools.contains(id) {
            if let Some(mut task) = self.pending_tasks.remove(id).filter(|_| !is_error) {
                let task_id = structured
                    .and_then(|structured| structured["task"]["id"].as_str().map(str::to_owned))
                    .or_else(|| created_task_id(&text));
                if let Some(task_id) = task_id {
                    if task.content.is_empty()
                        && let Some(subject) =
                            structured.and_then(|structured| structured["task"]["subject"].as_str())
                    {
                        subject.clone_into(&mut task.content);
                    }
                    self.tasks.push((task_id, task));
                    return vec![plan(self.tasks.iter().map(|(_, task)| task))];
                }
            }
            return Vec::new();
        }
        let name = self.tools.get(id).cloned().unwrap_or_default();
        let output = if is_error {
            Some(text)
        } else {
            match name.as_str() {
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => None,
                "Agent" | "Task" => Some(self.summaries.remove(id).unwrap_or(text)),
                "Bash" | "PowerShell" => Some(structured.and_then(command_output).unwrap_or(text)),
                _ => Some(text),
            }
        };
        let mut update = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": if is_error { "failed" } else { "completed" },
        });
        if let Some(output) = output.filter(|output| !output.trim().is_empty()) {
            update["content"] = json!([text_content(&output)]);
        }
        vec![update]
    }

    fn result(&mut self, frame: &Value) -> Vec<Value> {
        let size = frame["modelUsage"].as_object().and_then(|models| {
            models
                .values()
                .filter_map(|model| model["contextWindow"].as_u64())
                .max()
        });
        let Some(size) = size else {
            return Vec::new();
        };
        let usage = &frame["usage"];
        let last = usage["iterations"]
            .as_array()
            .and_then(|iterations| iterations.last())
            .unwrap_or(usage);
        let used = [
            "input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
            "output_tokens",
        ]
        .iter()
        .filter_map(|key| last[*key].as_u64())
        .sum::<u64>();
        let mut update = json!({ "sessionUpdate": "usage_update", "used": used, "size": size });
        if let Some(cost) = frame["total_cost_usd"].as_f64() {
            update["cost"] = json!({ "amount": cost, "currency": "USD" });
        }
        vec![update]
    }
}

fn parent_key(frame: &Value) -> String {
    frame["parent_tool_use_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
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

fn plan<'a>(items: impl Iterator<Item = &'a PlanItem>) -> Value {
    json!({
        "sessionUpdate": "plan",
        "entries": items.map(PlanItem::entry).collect::<Vec<_>>(),
    })
}

fn plan_status(status: Option<&str>) -> String {
    match status {
        Some("in_progress") => "in_progress",
        Some("completed") => "completed",
        _ => "pending",
    }
    .to_owned()
}

fn created_task_id(text: &str) -> Option<String> {
    let rest = text.trim().strip_prefix("Task #")?;
    let (id, tail) = rest.split_once(' ')?;
    tail.starts_with("created").then(|| id.to_owned())
}

fn is_internal_user_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    [
        "<command-",
        "<local-command-",
        "<system-reminder>",
        "<task-notification>",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

pub(crate) fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| match block["type"].as_str() {
                Some("text") => block["text"].as_str().map(str::to_owned),
                Some("image") => Some("[image]".to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn command_output(structured: &Value) -> Option<String> {
    let stdout = structured["stdout"].as_str()?;
    let stderr = structured["stderr"].as_str().unwrap_or_default();
    let mut output = stdout.trim_end().to_owned();
    if !stderr.trim().is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(stderr.trim_end());
    }
    if structured["interrupted"] == true {
        output.push_str("\n[interrupted]");
    }
    Some(output)
}

pub(crate) struct ToolInfo {
    pub(crate) title: String,
    pub(crate) kind: &'static str,
    pub(crate) content: Vec<Value>,
    pub(crate) locations: Vec<Value>,
}

pub(crate) fn tool_info(name: &str, input: &Value, cwd: &Path) -> ToolInfo {
    let path = input["file_path"]
        .as_str()
        .or_else(|| input["notebook_path"].as_str())
        .or_else(|| input["path"].as_str());
    let shown = path.map(|path| display_path(path, cwd));
    let location = |line: Option<u64>| {
        path.map(|path| {
            let mut location = json!({ "path": path });
            if let Some(line) = line {
                location["line"] = Value::from(line);
            }
            vec![location]
        })
        .unwrap_or_default()
    };
    let (title, kind, content, locations) = match name {
        "Bash" | "PowerShell" => (
            input["command"]
                .as_str()
                .map_or_else(|| "Terminal".to_owned(), str::to_owned),
            "execute",
            Vec::new(),
            Vec::new(),
        ),
        "Read" => {
            let offset = input["offset"].as_u64();
            let range = match (offset, input["limit"].as_u64().filter(|limit| *limit > 0)) {
                (offset, Some(limit)) => {
                    let start = offset.unwrap_or(1);
                    format!(" ({start} - {})", start + limit - 1)
                }
                (Some(offset), None) => format!(" (from line {offset})"),
                (None, None) => String::new(),
            };
            (
                format!("Read {}{range}", shown.as_deref().unwrap_or("file")),
                "read",
                Vec::new(),
                location(Some(offset.unwrap_or(1))),
            )
        }
        "Write" => (
            shown
                .as_deref()
                .map_or_else(|| "Write".to_owned(), |path| format!("Write {path}")),
            "edit",
            path.zip(input["content"].as_str())
                .map(|(path, text)| {
                    vec![json!({ "type": "diff", "path": path, "oldText": null, "newText": text })]
                })
                .unwrap_or_default(),
            location(None),
        ),
        "Edit" => (
            shown
                .as_deref()
                .map_or_else(|| "Edit".to_owned(), |path| format!("Edit {path}")),
            "edit",
            path.map(|path| {
                vec![json!({
                    "type": "diff",
                    "path": path,
                    "oldText": input["old_string"].as_str().filter(|text| !text.is_empty()),
                    "newText": input["new_string"].as_str().unwrap_or_default(),
                })]
            })
            .unwrap_or_default(),
            location(None),
        ),
        "MultiEdit" => (
            shown
                .as_deref()
                .map_or_else(|| "Edit".to_owned(), |path| format!("Edit {path}")),
            "edit",
            path.map(|path| {
                input["edits"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|edit| {
                        json!({
                            "type": "diff",
                            "path": path,
                            "oldText": edit["old_string"].as_str().filter(|text| !text.is_empty()),
                            "newText": edit["new_string"].as_str().unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
            location(None),
        ),
        "NotebookEdit" => (
            shown.as_deref().map_or_else(
                || "Edit notebook".to_owned(),
                |path| format!("Edit notebook {path}"),
            ),
            "edit",
            Vec::new(),
            location(None),
        ),
        "Glob" => {
            let mut title = "Find".to_owned();
            if let Some(path) = &shown {
                let _ = write!(title, " `{path}`");
            }
            if let Some(pattern) = input["pattern"].as_str() {
                let _ = write!(title, " `{pattern}`");
            }
            (title, "search", Vec::new(), location(None))
        }
        "Grep" => (grep_title(input), "search", Vec::new(), Vec::new()),
        "WebFetch" => (
            input["url"]
                .as_str()
                .map_or_else(|| "Fetch".to_owned(), |url| format!("Fetch {url}")),
            "fetch",
            Vec::new(),
            Vec::new(),
        ),
        "WebSearch" => (
            input["query"].as_str().map_or_else(
                || "Web search".to_owned(),
                |query| format!("Search \"{query}\""),
            ),
            "fetch",
            Vec::new(),
            Vec::new(),
        ),
        "Agent" | "Task" => (
            input["description"]
                .as_str()
                .filter(|text| !text.is_empty())
                .map_or_else(|| "Agent".to_owned(), str::to_owned),
            "think",
            Vec::new(),
            Vec::new(),
        ),
        "ExitPlanMode" => (
            "Plan".to_owned(),
            "switch_mode",
            input["plan"]
                .as_str()
                .map(|plan| vec![text_content(plan)])
                .unwrap_or_default(),
            Vec::new(),
        ),
        "AskUserQuestion" => {
            let questions = input["questions"].as_array();
            let title = match questions.map(Vec::as_slice) {
                Some([question]) => question["question"]
                    .as_str()
                    .unwrap_or("Question")
                    .to_owned(),
                _ => "Asking for your input".to_owned(),
            };
            (title, "other", Vec::new(), Vec::new())
        }
        "Skill" => (
            input["skill"].as_str().map_or_else(
                || "Load skill".to_owned(),
                |skill| format!("Load skill: {skill}"),
            ),
            "other",
            Vec::new(),
            Vec::new(),
        ),
        _ => (mcp_title(name), "other", Vec::new(), Vec::new()),
    };
    ToolInfo {
        title: clip_title(&title),
        kind,
        content,
        locations,
    }
}

fn grep_title(input: &Value) -> String {
    let mut title = "grep".to_owned();
    if input["-i"] == true {
        title.push_str(" -i");
    }
    if input["-n"] == true {
        title.push_str(" -n");
    }
    for flag in ["-A", "-B", "-C"] {
        if let Some(value) = input[flag].as_u64() {
            let _ = write!(title, " {flag} {value}");
        }
    }
    match input["output_mode"].as_str() {
        Some("files_with_matches") => title.push_str(" -l"),
        Some("count") => title.push_str(" -c"),
        _ => {}
    }
    if let Some(glob) = input["glob"].as_str() {
        let _ = write!(title, " --include=\"{glob}\"");
    }
    if let Some(kind) = input["type"].as_str() {
        let _ = write!(title, " --type={kind}");
    }
    if let Some(pattern) = input["pattern"].as_str() {
        let _ = write!(title, " \"{pattern}\"");
    }
    if let Some(path) = input["path"].as_str() {
        title.push(' ');
        title.push_str(path);
    }
    title
}

fn mcp_title(name: &str) -> String {
    name.strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
        .map_or_else(
            || name.to_owned(),
            |(server, tool)| format!("{server}: {tool}"),
        )
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

pub(crate) fn available_commands(commands: &[Value], hidden: &HashSet<String>) -> Value {
    let commands = commands
        .iter()
        .filter_map(|command| {
            let name = command["name"].as_str()?;
            if hidden.contains(name) {
                return None;
            }
            let mut entry = json!({
                "name": name,
                "description": command["description"].as_str().unwrap_or_default(),
            });
            if let Some(hint) = command["argumentHint"]
                .as_str()
                .filter(|hint| !hint.is_empty())
            {
                entry["input"] = json!({ "hint": hint });
            }
            Some(entry)
        })
        .collect::<Vec<_>>();
    json!({ "sessionUpdate": "available_commands_update", "availableCommands": commands })
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::SessionUpdate;

    use super::*;

    fn fixture(name: &str) -> Vec<Value> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/agent/claude/fixtures")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<Value>(line).expect("fixture line is JSON"))
            .filter(|record| record["dir"] == "out")
            .map(|record| record["msg"].clone())
            .collect()
    }

    fn replay(name: &str) -> Vec<Value> {
        let mut translator = Translator::new(PathBuf::from("/work"));
        let updates = fixture(name)
            .iter()
            .flat_map(|frame| translator.frame(frame))
            .collect::<Vec<_>>();
        for update in &updates {
            serde_json::from_value::<SessionUpdate>(update.clone())
                .unwrap_or_else(|error| panic!("{update} is not an ACP update: {error}"));
        }
        updates
    }

    fn kinds(updates: &[Value]) -> Vec<&str> {
        updates
            .iter()
            .map(|update| update["sessionUpdate"].as_str().unwrap_or_default())
            .collect()
    }

    fn message_text(updates: &[Value], kind: &str) -> String {
        updates
            .iter()
            .filter(|update| update["sessionUpdate"] == kind)
            .filter_map(|update| update["content"]["text"].as_str())
            .collect()
    }

    #[test]
    fn a_plain_reply_streams_once_and_reports_usage() {
        let updates = replay("hello.ndjson");
        assert_eq!(message_text(&updates, "agent_message_chunk"), "hi there");
        assert_eq!(kinds(&updates).last(), Some(&"usage_update"));
        let usage = updates.last().expect("usage");
        assert_eq!(usage["size"], 200_000);
        assert!(usage["used"].as_u64().is_some_and(|used| used > 0));
    }

    #[test]
    fn tools_become_rows_that_complete_with_their_output() {
        let updates = replay("tools.ndjson");
        let calls = updates
            .iter()
            .filter(|update| update["sessionUpdate"] == "tool_call")
            .collect::<Vec<_>>();
        let titles = calls
            .iter()
            .map(|call| call["title"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            [
                "echo zz-probe > out.txt && cat notes.txt",
                "Read notes.txt",
                "Edit notes.txt",
                "What is your favorite color?",
                "Agent that replies with pong",
            ]
        );
        let edit = calls[2];
        assert_eq!(edit["kind"], "edit");
        assert_eq!(edit["content"][0]["type"], "diff");
        assert_eq!(edit["content"][0]["oldText"], "hello");
        assert_eq!(edit["content"][0]["newText"], "hullo");
        let bash_done = updates
            .iter()
            .find(|update| {
                update["sessionUpdate"] == "tool_call_update"
                    && update["toolCallId"] == calls[0]["toolCallId"]
            })
            .expect("bash completion");
        assert_eq!(bash_done["status"], "completed");
        assert_eq!(bash_done["content"][0]["content"]["text"], "hello");
        let agent_done = updates
            .iter()
            .find(|update| {
                update["sessionUpdate"] == "tool_call_update"
                    && update["toolCallId"] == calls[4]["toolCallId"]
            })
            .expect("agent completion");
        assert_eq!(agent_done["content"][0]["content"]["text"], "pong");
        assert!(
            updates
                .iter()
                .filter(|update| update["sessionUpdate"] == "tool_call_update"
                    && update["toolCallId"] == edit["toolCallId"])
                .all(|update| update.get("content").is_none()),
            "a finished edit keeps its diff"
        );
        assert_eq!(message_text(&updates, "agent_message_chunk"), "done");
    }

    #[test]
    fn steering_and_background_work_keep_one_row_per_tool() {
        let updates = replay("steer.ndjson");
        let mut seen = HashSet::new();
        for update in updates
            .iter()
            .filter(|update| update["sessionUpdate"] == "tool_call")
        {
            assert!(
                seen.insert(update["toolCallId"].as_str().unwrap_or_default()),
                "duplicate tool row {update}"
            );
        }
        assert!(message_text(&updates, "agent_message_chunk").contains("bye"));
        assert!(
            !kinds(&updates).contains(&"user_message_chunk"),
            "prompt echoes stay with the host"
        );
    }

    #[test]
    fn todo_and_task_tools_become_the_plan_not_rows() {
        let mut translator = Translator::new(PathBuf::from("/work"));
        let todo = json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m1","content":[
            {"type":"tool_use","id":"t1","name":"TodoWrite","input":{"todos":[
                {"content":"Write code","activeForm":"Writing code","status":"in_progress"},
                {"content":"Test","activeForm":"Testing","status":"pending"}]}}]}});
        let updates = translator.frame(&todo);
        assert_eq!(kinds(&updates), ["plan"]);
        assert_eq!(updates[0]["entries"][0]["content"], "Writing code");
        let create = json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m2","content":[
            {"type":"tool_use","id":"t2","name":"TaskCreate","input":{"subject":"Ship it","description":"d"}}]}});
        assert!(translator.frame(&create).is_empty());
        let created = json!({"type":"user","parent_tool_use_id":null,"message":{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"t2","content":"Task #7 created successfully: Ship it"}]}});
        let updates = translator.frame(&created);
        assert_eq!(updates[0]["entries"][0]["content"], "Ship it");
        let update = json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m3","content":[
            {"type":"tool_use","id":"t3","name":"TaskUpdate","input":{"taskId":"7","status":"completed"}}]}});
        let updates = translator.frame(&update);
        assert_eq!(updates[0]["entries"][0]["status"], "completed");
        let done = json!({"type":"user","parent_tool_use_id":null,"message":{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"t3","content":"Updated task #7"}]}});
        assert!(translator.frame(&done).is_empty());
    }

    #[test]
    fn unstreamed_blocks_render_whole_and_streamed_ones_never_twice() {
        let mut translator = Translator::new(PathBuf::from("/work"));
        let whole = json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m","content":[
            {"type":"text","text":"one"},{"type":"text","text":"two"}]}});
        let updates = translator.frame(&whole);
        assert_eq!(message_text(&updates, "agent_message_chunk"), "onetwo");
        assert_ne!(updates[0]["messageId"], updates[1]["messageId"]);
    }

    #[test]
    fn subagent_text_stays_out_of_the_main_thread() {
        let mut translator = Translator::new(PathBuf::from("/work"));
        let frame = json!({"type":"assistant","parent_tool_use_id":"toolu_parent","message":{"id":"m","content":[
            {"type":"text","text":"inner"},
            {"type":"tool_use","id":"inner_tool","name":"Read","input":{"file_path":"/work/a.rs"}}]}});
        let updates = translator.frame(&frame);
        assert_eq!(kinds(&updates), ["tool_call"]);
        assert_eq!(updates[0]["title"], "Read a.rs");
        assert_eq!(
            updates[0]["_meta"]["claudeCode"]["parentToolUseId"],
            "toolu_parent"
        );
    }

    #[test]
    fn peer_messages_show_and_internal_ones_do_not() {
        let mut translator = Translator::new(PathBuf::from("/work"));
        let peer = json!({"type":"user","uuid":"u1","parent_tool_use_id":null,"origin":{"kind":"peer"},
            "message":{"role":"user","content":"hello from another session"}});
        assert_eq!(kinds(&translator.frame(&peer)), ["user_message_chunk"]);
        let internal = json!({"type":"user","uuid":"u2","parent_tool_use_id":null,
            "message":{"role":"user","content":"<local-command-stdout>x</local-command-stdout>"}});
        assert!(translator.frame(&internal).is_empty());
    }

    #[test]
    fn commands_carry_their_hint_and_hide_terminal_ones() {
        let hidden = HashSet::from(["doctor".to_owned()]);
        let update = available_commands(
            &[
                json!({"name":"compact","description":"Compact","argumentHint":"<focus>"}),
                json!({"name":"doctor","description":"Doctor","argumentHint":""}),
            ],
            &hidden,
        );
        serde_json::from_value::<SessionUpdate>(update.clone()).expect("ACP update");
        assert_eq!(
            update["availableCommands"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(update["availableCommands"][0]["input"]["hint"], "<focus>");
    }

    #[test]
    fn titles_are_one_line_and_relative_to_the_workspace() {
        let info = tool_info(
            "Bash",
            &json!({"command":"cat <<EOF\nhello\nEOF"}),
            Path::new("/work"),
        );
        assert_eq!(info.title, "cat <<EOF…");
        let info = tool_info("mcp__github__create_issue", &json!({}), Path::new("/work"));
        assert_eq!(info.title, "github: create_issue");
        let info = tool_info(
            "Read",
            &json!({"file_path":"/elsewhere/x"}),
            Path::new("/work"),
        );
        assert_eq!(info.title, "Read /elsewhere/x");
    }
}
