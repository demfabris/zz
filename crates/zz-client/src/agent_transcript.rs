use agent_client_protocol_schema::v1::{
    ContentBlock, ContentChunk, ImageContent, PermissionOption, PermissionOptionKind, Plan,
    PlanEntryStatus, SessionUpdate, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    sync::Arc,
};
use zz_protocol::{
    MAX_AGENT_PERMISSION_OPTIONS, MAX_AGENT_TOOL_CONTENT_ITEMS, agent_stream::AgentQuestion,
};

const ENTRY_CHANGE_LOG_CAPACITY: usize = 4_096;
const MAX_TOOL_PAYLOAD_BYTES: usize = 512 * 1024;
const MAX_DIFF_SIDE_BYTES: usize = 1024 * 1024;
const TRUNCATION_MARKER: &str = "… [truncated]";

pub type ImageDecoder<I> = fn(&str, Vec<u8>) -> Option<I>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum AgentToolKindModel {
    Read,
    Search,
    Edit,
    Delete,
    Move,
    Execute,
    Fetch,
    Think,
    SwitchMode,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum AgentToolStatusModel {
    Pending,
    Running,
    NeedsApproval,
    Completed,
    Failed,
    Canceled,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ToolPayload {
    Diff {
        path: String,
        old: Option<String>,
        new: String,
    },
    Text(String),
    Json(String),
    Terminal(String),
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentThreadEntry<I> {
    User {
        id: u64,
        markdown: String,
        images: Vec<I>,
        message_id: Option<String>,
    },
    Assistant {
        id: u64,
        markdown: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        aside: Option<AgentAside>,
    },
    Reasoning {
        id: u64,
        label: String,
        markdown: String,
        default_expanded: bool,
    },
    Tool {
        id: u64,
        protocol_id: String,
        kind: AgentToolKindModel,
        status: AgentToolStatusModel,
        label: String,
        location: Option<String>,
        input: Option<ToolPayload>,
        output: Vec<ToolPayload>,
        default_expanded: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i64>,
    },
    Plan {
        id: u64,
        markdown: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AgentAside {
    pub side: bool,
    pub reply_to: Option<u64>,
}

impl<I> AgentThreadEntry<I> {
    pub const fn id(&self) -> u64 {
        match self {
            Self::User { id, .. }
            | Self::Assistant { id, .. }
            | Self::Reasoning { id, .. }
            | Self::Tool { id, .. }
            | Self::Plan { id, .. } => *id,
        }
    }

    pub fn rewind_id(&self) -> Option<&str> {
        match self {
            Self::User {
                markdown,
                message_id: Some(message_id),
                ..
            } if !markdown.trim_start().starts_with('/') => Some(message_id),
            _ => None,
        }
    }
}

pub fn is_zz_command(text: &str) -> bool {
    zz_protocol::agent_stream::agent_verb(text).is_some()
}

pub fn rewind_command(message_id: &str) -> String {
    format!("/rewind {message_id}")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Serialize)]
enum StreamRole {
    User,
    Assistant,
    Reasoning,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AgentPermissionOption {
    pub id: String,
    pub name: String,
    pub kind: AgentPermissionKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum AgentPermissionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AgentPermissionRequest {
    pub request_id: u64,
    pub tool_call_id: String,
    pub title: String,
    pub options: Vec<AgentPermissionOption>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<AgentQuestion>,
}

#[derive(Debug)]
pub struct AgentTranscript<I> {
    entries: Vec<AgentThreadEntry<I>>,
    entry_revisions: Vec<u64>,
    entry_indices: HashMap<u64, usize>,
    entry_changes: VecDeque<(u64, usize)>,
    entry_change_floor: u64,
    pending_permissions: Arc<[AgentPermissionRequest]>,
    next_entry_id: u64,
    next_entry_revision: u64,
    message_entries: BTreeMap<(StreamRole, String), u64>,
    active_stream: Option<(StreamRole, u64)>,
    tool_entries: HashMap<String, u64>,
    tool_parents: HashMap<u64, String>,
    awaiting_parent: HashMap<String, Vec<u64>>,
    structured_tool_outputs: BTreeSet<String>,
    plan_entry: Option<u64>,
    suppress_user_echo: bool,
    echo_target: Option<u64>,
    decode_image: ImageDecoder<I>,
}

impl<I> AgentTranscript<I> {
    pub fn new(decode_image: ImageDecoder<I>) -> Self {
        Self {
            entries: Vec::new(),
            entry_revisions: Vec::new(),
            entry_indices: HashMap::new(),
            entry_changes: VecDeque::new(),
            entry_change_floor: 1,
            pending_permissions: Arc::from([]),
            next_entry_id: 1,
            next_entry_revision: 1,
            message_entries: BTreeMap::new(),
            active_stream: None,
            tool_entries: HashMap::new(),
            tool_parents: HashMap::new(),
            awaiting_parent: HashMap::new(),
            structured_tool_outputs: BTreeSet::new(),
            plan_entry: None,
            suppress_user_echo: false,
            echo_target: None,
            decode_image,
        }
    }

    pub fn reset(&mut self) {
        let revision = self.next_entry_revision;
        *self = Self::new(self.decode_image);
        self.next_entry_revision = revision;
        self.entry_change_floor = revision;
    }
    pub fn entries(&self) -> &[AgentThreadEntry<I>] {
        &self.entries
    }

    /// The tool call a subagent's tool row belongs to, by the parent's tool
    /// call ID, when the driver said so.
    pub fn tool_parent(&self, entry_id: u64) -> Option<&str> {
        self.tool_parents.get(&entry_id).map(String::as_str)
    }

    pub fn tool_entry(&self, tool_call_id: &str) -> Option<u64> {
        self.tool_entries.get(tool_call_id).copied()
    }
    pub fn entry_revisions(&self) -> &[u64] {
        &self.entry_revisions
    }
    pub fn revision(&self) -> u64 {
        self.next_entry_revision
    }
    pub fn permissions(&self) -> &Arc<[AgentPermissionRequest]> {
        &self.pending_permissions
    }
    pub fn clear_permissions(&mut self) {
        self.pending_permissions = Arc::from([]);
    }
    pub fn changed_entries(&self, since: u64) -> Option<Vec<usize>> {
        if since < self.entry_change_floor {
            return None;
        }
        let mut changes = self
            .entry_changes
            .iter()
            .filter_map(|(revision, index)| (*revision >= since).then_some(*index))
            .collect::<Vec<_>>();
        changes.sort_unstable();
        changes.dedup();
        Some(changes)
    }
    pub fn begin_prompt(&mut self, prompt: String, images: Vec<I>) {
        self.active_stream = None;
        let id = self.allocate_entry_id();
        self.push_entry(AgentThreadEntry::User {
            id,
            markdown: prompt,
            images,
            message_id: None,
        });
        self.suppress_user_echo = true;
        self.echo_target = Some(id);
    }
    pub fn apply_update(&mut self, update: SessionUpdate) {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => {
                self.apply_message_chunk(StreamRole::User, chunk);
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                self.apply_message_chunk(StreamRole::Assistant, chunk);
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                self.append_chunk(StreamRole::Reasoning, chunk);
            }
            SessionUpdate::ToolCall(tool) => {
                self.active_stream = None;
                self.upsert_tool(tool);
            }
            SessionUpdate::ToolCallUpdate(update) => {
                self.active_stream = None;
                self.apply_tool_update(update);
            }
            SessionUpdate::Plan(plan) => {
                self.active_stream = None;
                self.apply_plan(&plan);
            }
            _ => {}
        }
    }
    fn allocate_entry_id(&mut self) -> u64 {
        let id = self.next_entry_id;
        self.next_entry_id = self.next_entry_id.saturating_add(1);
        id
    }

    fn allocate_entry_revision(&mut self) -> u64 {
        let revision = self.next_entry_revision;
        self.next_entry_revision = self.next_entry_revision.saturating_add(1);
        revision
    }

    fn push_entry(&mut self, entry: AgentThreadEntry<I>) {
        let revision = self.allocate_entry_revision();
        let index = self.entries.len();
        self.entry_indices.insert(entry.id(), index);
        self.entries.push(entry);
        self.entry_revisions.push(revision);
        self.record_entry_change(revision, index);
    }

    fn entry_index(&self, id: u64) -> Option<usize> {
        self.entry_indices.get(&id).copied()
    }

    fn touch_entry(&mut self, index: usize) {
        let revision = self.allocate_entry_revision();
        if let Some(entry_revision) = self.entry_revisions.get_mut(index) {
            *entry_revision = revision;
            self.record_entry_change(revision, index);
        }
    }

    fn record_entry_change(&mut self, revision: u64, index: usize) {
        if self.entry_changes.len() == ENTRY_CHANGE_LOG_CAPACITY
            && let Some((discarded, _)) = self.entry_changes.pop_front()
        {
            self.entry_change_floor = discarded.saturating_add(1);
        }
        self.entry_changes.push_back((revision, index));
    }

    fn apply_message_chunk(&mut self, role: StreamRole, chunk: ContentChunk) {
        if role == StreamRole::User && self.suppress_user_echo {
            if let Some(target) = self.echo_target.take()
                && let Some(message_id) = chunk.message_id
            {
                self.set_message_id(target, message_id.0.to_string());
            }
            return;
        }
        self.append_chunk(role, chunk);
    }

    fn set_message_id(&mut self, entry_id: u64, next: String) {
        let Some(index) = self.entry_index(entry_id) else {
            return;
        };
        if let AgentThreadEntry::User { message_id, .. } = &mut self.entries[index]
            && message_id.is_none()
        {
            *message_id = Some(next);
            self.touch_entry(index);
        }
    }

    fn append_chunk(&mut self, role: StreamRole, chunk: ContentChunk) {
        let (markdown, images) = match (role, &chunk.content) {
            (StreamRole::User, ContentBlock::Image(image)) => {
                match inbound_image(image, self.decode_image) {
                    Some(image) => (String::new(), vec![image]),
                    None => (content_block_markdown(&chunk.content), Vec::new()),
                }
            }
            (StreamRole::User, content) => {
                split_inline_images(&content_block_markdown(content), self.decode_image)
            }
            (_, content) => (content_block_markdown(content), Vec::new()),
        };
        if markdown.is_empty() && images.is_empty() {
            return;
        }
        let message_id = chunk.message_id.map(|message_id| message_id.0.to_string());
        let aside = (role == StreamRole::Assistant)
            .then(|| self.aside(chunk.meta.as_ref()))
            .flatten();
        self.append_stream_content(role, message_id, &markdown, images);
        if let Some(aside) = aside
            && let Some((_, entry_id)) = self.active_stream.take()
            && let Some(index) = self.entry_index(entry_id)
            && let AgentThreadEntry::Assistant { aside: slot, .. } = &mut self.entries[index]
            && slot.is_none()
        {
            *slot = Some(aside);
            self.touch_entry(index);
        }
    }

    fn aside(
        &self,
        meta: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Option<AgentAside> {
        let zz = meta?.get("zz")?;
        let side = zz.get("side").and_then(serde_json::Value::as_bool) == Some(true);
        let notice = zz.get("notice").and_then(serde_json::Value::as_bool) == Some(true);
        if !side && !notice {
            return None;
        }
        let reply_to = zz
            .get("reply")
            .and_then(serde_json::Value::as_str)
            .and_then(|reply| {
                self.message_entries
                    .get(&(StreamRole::User, reply.to_owned()))
                    .copied()
            });
        Some(AgentAside { side, reply_to })
    }

    fn append_stream_content(
        &mut self,
        role: StreamRole,
        message_id: Option<String>,
        markdown: &str,
        images: Vec<I>,
    ) {
        if markdown.is_empty() && images.is_empty() {
            return;
        }
        let message_key = message_id.map(|message_id| (role, message_id));
        let entry_id = if let Some(key) = message_key.as_ref() {
            if let Some(id) = self.message_entries.get(key).copied() {
                id
            } else {
                let id = self.push_stream_entry(role, Some(&key.1));
                self.message_entries.insert(key.clone(), id);
                id
            }
        } else if let Some((active_role, id)) = self.active_stream {
            if active_role == role {
                id
            } else {
                self.push_stream_entry(role, None)
            }
        } else {
            self.push_stream_entry(role, None)
        };
        self.active_stream = Some((role, entry_id));
        if let Some(index) = self.entry_index(entry_id) {
            let changed = match &mut self.entries[index] {
                AgentThreadEntry::User {
                    markdown: text,
                    images: attached,
                    ..
                } => {
                    attached.extend(images);
                    text.push_str(markdown);
                    true
                }
                AgentThreadEntry::Assistant { markdown: text, .. }
                | AgentThreadEntry::Reasoning { markdown: text, .. } => {
                    text.push_str(markdown);
                    true
                }
                AgentThreadEntry::Tool { .. } | AgentThreadEntry::Plan { .. } => false,
            };
            if changed {
                self.touch_entry(index);
            }
        }
    }

    fn push_stream_entry(&mut self, role: StreamRole, message_id: Option<&str>) -> u64 {
        let id = self.allocate_entry_id();
        let entry = match role {
            StreamRole::User => AgentThreadEntry::User {
                id,
                markdown: String::new(),
                images: Vec::new(),
                message_id: message_id.map(str::to_owned),
            },
            StreamRole::Assistant => AgentThreadEntry::Assistant {
                id,
                markdown: String::new(),
                aside: None,
            },
            StreamRole::Reasoning => AgentThreadEntry::Reasoning {
                id,
                label: "Reasoning".to_owned(),
                markdown: String::new(),
                default_expanded: false,
            },
        };
        self.push_entry(entry);
        id
    }

    fn upsert_tool(&mut self, tool: ToolCall) {
        let protocol_id = tool.tool_call_id.0.to_string();
        let parent = tool
            .meta
            .as_ref()
            .and_then(|meta| meta.get("zz"))
            .and_then(|zz| zz.get("parent"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if tool.content.is_empty() {
            self.structured_tool_outputs.remove(&protocol_id);
        } else {
            self.structured_tool_outputs.insert(protocol_id.clone());
        }
        let location = tool_location(&tool);
        let input = tool_input(&tool);
        let output = tool_output(&tool);
        let exit = exit_code(tool.meta.as_ref());
        if let Some(entry_id) = self.tool_entries.get(&protocol_id).copied()
            && let Some(index) = self.entry_index(entry_id)
            && let AgentThreadEntry::Tool {
                kind,
                status,
                label,
                location: entry_location,
                input: entry_input,
                output: entry_output,
                exit_code: entry_exit,
                ..
            } = &mut self.entries[index]
        {
            *kind = map_tool_kind(tool.kind);
            *status = map_tool_status(tool.status);
            *label = tool.title;
            *entry_location = location;
            *entry_input = input;
            *entry_output = output;
            if exit.is_some() {
                *entry_exit = exit;
            }
            self.touch_entry(index);
            return;
        }
        let id = self.allocate_entry_id();
        self.tool_entries.insert(protocol_id.clone(), id);
        if let Some(parent) = parent {
            if !self.tool_entries.contains_key(&parent) {
                self.awaiting_parent
                    .entry(parent.clone())
                    .or_default()
                    .push(id);
            }
            self.tool_parents.insert(id, parent);
        }
        self.push_entry(AgentThreadEntry::Tool {
            id,
            protocol_id: protocol_id.clone(),
            kind: map_tool_kind(tool.kind),
            status: map_tool_status(tool.status),
            label: tool.title,
            location,
            input,
            output,
            default_expanded: matches!(tool.status, ToolCallStatus::Failed),
            exit_code: exit,
        });
        for child in self
            .awaiting_parent
            .remove(&protocol_id)
            .unwrap_or_default()
        {
            if let Some(index) = self.entry_index(child) {
                self.touch_entry(index);
            }
        }
    }

    fn apply_tool_update(&mut self, update: ToolCallUpdate) {
        let protocol_id = update.tool_call_id.0.to_string();
        let carries_shape = update_carries_tool_shape(&update.fields);
        let Some(entry_id) = self.tool_entries.get(&protocol_id).copied() else {
            if let Ok(tool) = ToolCall::try_from(update) {
                self.upsert_tool(tool);
            }
            return;
        };
        let Some(index) = self.entry_index(entry_id) else {
            return;
        };
        let had_structured_output = self.structured_tool_outputs.contains(&protocol_id);
        let exit = exit_code(update.meta.as_ref());
        let AgentThreadEntry::Tool {
            kind,
            status,
            label,
            location,
            input,
            output,
            exit_code: entry_exit,
            ..
        } = &mut self.entries[index]
        else {
            return;
        };
        let mut changed = false;
        if exit.is_some() && *entry_exit != exit {
            *entry_exit = exit;
            changed = true;
        }
        if let Some(next) = update.fields.kind
            && reclassifies_tool(*kind, next, carries_shape)
        {
            *kind = map_tool_kind(next);
            changed = true;
        }
        if let Some(next) = update.fields.status {
            *status = map_tool_status(next);
            changed = true;
        }
        if let Some(next) = update.fields.title.filter(|title| !title.trim().is_empty()) {
            *label = next;
            changed = true;
        }
        if let Some(raw_input) = update.fields.raw_input {
            *input = json_payload(&raw_input);
            changed = true;
        }
        if let Some(locations) = update.fields.locations {
            *location = locations.first().map(|location| {
                location.line.map_or_else(
                    || location.path.display().to_string(),
                    |line| format!("{}:{line}", location.path.display()),
                )
            });
            changed = true;
        }
        let raw_output = update
            .fields
            .raw_output
            .and_then(|raw_output| json_payload(&raw_output));
        if let Some(content) = update.fields.content {
            let structured = tool_content_payloads(&content);
            if structured.is_empty() {
                self.structured_tool_outputs.remove(&protocol_id);
            } else {
                self.structured_tool_outputs.insert(protocol_id.clone());
            }
            *output = if structured.is_empty() {
                raw_output.into_iter().collect()
            } else {
                structured
            };
            changed = true;
        } else if let Some(raw_output) = raw_output {
            if !had_structured_output {
                *output = vec![raw_output];
            }
            changed = true;
        }
        if changed {
            self.touch_entry(index);
        }
    }

    fn apply_plan(&mut self, plan: &Plan) {
        let markdown = plan
            .entries
            .iter()
            .map(|entry| {
                let marker = match entry.status {
                    PlanEntryStatus::InProgress => "~",
                    PlanEntryStatus::Completed => "x",
                    _ => " ",
                };
                format!("- [{marker}] {}", entry.content)
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(id) = self.plan_entry
            && let Some(index) = self.entry_index(id)
            && let AgentThreadEntry::Plan {
                markdown: current, ..
            } = &mut self.entries[index]
        {
            if *current != markdown {
                *current = markdown;
                self.touch_entry(index);
            }
            return;
        }
        let id = self.allocate_entry_id();
        self.plan_entry = Some(id);
        self.push_entry(AgentThreadEntry::Plan { id, markdown });
    }

    pub fn request_permission(
        &mut self,
        request_id: u64,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) {
        self.request_questions(request_id, tool_call, options, Vec::new());
    }

    pub fn request_questions(
        &mut self,
        request_id: u64,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
        questions: Vec<AgentQuestion>,
    ) {
        let tool_call_id = tool_call.tool_call_id.0.to_string();
        let updated_title = tool_call.fields.title.clone();
        self.apply_tool_update(tool_call);
        let title = updated_title
            .or_else(|| {
                self.tool_entries
                    .get(&tool_call_id)
                    .and_then(|id| self.entry_index(*id))
                    .and_then(|index| self.entries.get(index))
                    .and_then(|entry| match entry {
                        AgentThreadEntry::Tool { label, .. } => Some(label.clone()),
                        _ => None,
                    })
            })
            .unwrap_or_else(|| "Tool approval".to_owned());
        if let Some(entry_id) = self.tool_entries.get(&tool_call_id).copied()
            && let Some(index) = self.entry_index(entry_id)
            && let AgentThreadEntry::Tool { status, .. } = &mut self.entries[index]
            && *status != AgentToolStatusModel::NeedsApproval
        {
            *status = AgentToolStatusModel::NeedsApproval;
            self.touch_entry(index);
        }
        let request = AgentPermissionRequest {
            request_id,
            tool_call_id,
            title,
            options: options
                .into_iter()
                .take(MAX_AGENT_PERMISSION_OPTIONS)
                .map(|option| AgentPermissionOption {
                    id: option.option_id.0.to_string(),
                    name: option.name,
                    kind: map_permission_kind(option.kind),
                })
                .collect(),
            questions,
        };
        let mut pending_permissions = self.pending_permissions.to_vec();
        if let Some(existing) = pending_permissions
            .iter_mut()
            .find(|pending| pending.request_id == request_id)
        {
            *existing = request;
        } else {
            pending_permissions.push(request);
        }
        self.pending_permissions = pending_permissions.into();
    }

    pub fn resolve_permission(&mut self, request_id: u64, canceled: bool) {
        let Some(index) = self
            .pending_permissions
            .iter()
            .position(|permission| permission.request_id == request_id)
        else {
            return;
        };
        let mut pending_permissions = self.pending_permissions.to_vec();
        let permission = pending_permissions.remove(index);
        self.pending_permissions = pending_permissions.into();
        if let Some(entry_id) = self.tool_entries.get(&permission.tool_call_id).copied()
            && let Some(index) = self.entry_index(entry_id)
            && let AgentThreadEntry::Tool { status, .. } = &mut self.entries[index]
        {
            let next = if canceled {
                AgentToolStatusModel::Canceled
            } else {
                AgentToolStatusModel::Pending
            };
            if *status != next {
                *status = next;
                self.touch_entry(index);
            }
        }
    }

    pub fn cancel_inflight(&mut self) {
        self.settle_inflight(AgentToolStatusModel::Canceled);
    }

    pub fn finish_turn(&mut self) {
        self.pending_permissions = Arc::from([]);
        self.suppress_user_echo = false;
        self.echo_target = None;
        self.active_stream = None;
    }

    fn settle_inflight(&mut self, settled_status: AgentToolStatusModel) {
        debug_assert!(matches!(
            settled_status,
            AgentToolStatusModel::Failed | AgentToolStatusModel::Canceled
        ));
        self.finish_turn();
        for index in 0..self.entries.len() {
            let changed = if let AgentThreadEntry::Tool { status, .. } = &mut self.entries[index] {
                if matches!(
                    status,
                    AgentToolStatusModel::Pending
                        | AgentToolStatusModel::Running
                        | AgentToolStatusModel::NeedsApproval
                ) {
                    *status = settled_status;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if changed {
                self.touch_entry(index);
            }
        }
    }

    pub fn fail_inflight(&mut self) {
        self.settle_inflight(AgentToolStatusModel::Failed);
    }

    pub fn finish_replay(&mut self) {
        self.message_entries.clear();
        self.active_stream = None;
    }
}

fn map_permission_kind(kind: PermissionOptionKind) -> AgentPermissionKind {
    match kind {
        PermissionOptionKind::AllowOnce => AgentPermissionKind::AllowOnce,
        PermissionOptionKind::AllowAlways => AgentPermissionKind::AllowAlways,
        PermissionOptionKind::RejectAlways => AgentPermissionKind::RejectAlways,
        _ => AgentPermissionKind::RejectOnce,
    }
}

fn update_carries_tool_shape(fields: &ToolCallUpdateFields) -> bool {
    fields.title.is_some()
        || fields.raw_input.is_some()
        || fields.locations.is_some()
        || fields.content.as_ref().is_some_and(|content| {
            content
                .iter()
                .any(|content| matches!(content, ToolCallContent::Diff(_)))
        })
}

fn reclassifies_tool(current: AgentToolKindModel, next: ToolKind, carries_shape: bool) -> bool {
    carries_shape
        || current == AgentToolKindModel::Other
        || map_tool_kind(next) != AgentToolKindModel::Other
}

fn map_tool_kind(kind: ToolKind) -> AgentToolKindModel {
    match kind {
        ToolKind::Read => AgentToolKindModel::Read,
        ToolKind::Search => AgentToolKindModel::Search,
        ToolKind::Edit => AgentToolKindModel::Edit,
        ToolKind::Delete => AgentToolKindModel::Delete,
        ToolKind::Move => AgentToolKindModel::Move,
        ToolKind::Execute => AgentToolKindModel::Execute,
        ToolKind::Fetch => AgentToolKindModel::Fetch,
        ToolKind::Think => AgentToolKindModel::Think,
        ToolKind::SwitchMode => AgentToolKindModel::SwitchMode,
        _ => AgentToolKindModel::Other,
    }
}

fn exit_code(meta: Option<&serde_json::Map<String, serde_json::Value>>) -> Option<i64> {
    meta?.get("zz")?.get("exitCode")?.as_i64()
}

fn map_tool_status(status: ToolCallStatus) -> AgentToolStatusModel {
    match status {
        ToolCallStatus::InProgress => AgentToolStatusModel::Running,
        ToolCallStatus::Completed => AgentToolStatusModel::Completed,
        ToolCallStatus::Failed => AgentToolStatusModel::Failed,
        _ => AgentToolStatusModel::Pending,
    }
}

fn inbound_image<I>(image: &ImageContent, decode: ImageDecoder<I>) -> Option<I> {
    decode(&image.mime_type, BASE64.decode(&image.data).ok()?)
}

pub fn split_inline_images<I>(text: &str, decode: ImageDecoder<I>) -> (String, Vec<I>) {
    const MARKER: &str = "data:image/";
    if !text.contains(MARKER) {
        return (text.to_owned(), Vec::new());
    }
    let mut kept = String::with_capacity(text.len());
    let mut images = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find(MARKER) {
        let end = remaining[start..]
            .find(|character: char| {
                character.is_whitespace() || matches!(character, ')' | '"' | '\'' | '<' | '>')
            })
            .map_or(remaining.len(), |offset| start + offset);
        let Some(image) = decode_data_uri(&remaining[start..end], decode) else {
            kept.push_str(&remaining[..end]);
            remaining = &remaining[end..];
            continue;
        };
        images.push(image);
        let before = &remaining[..start];
        let (prefix_end, closer) = before
            .strip_suffix("](")
            .and_then(|before| before.rfind('['))
            .map_or((before.len(), ""), |open| (open, ")"));
        kept.push_str(&remaining[..prefix_end]);
        remaining = remaining[end..]
            .strip_prefix(closer)
            .unwrap_or(&remaining[end..]);
    }
    kept.push_str(remaining);
    (kept.trim().to_owned(), images)
}

fn decode_data_uri<I>(uri: &str, decode: ImageDecoder<I>) -> Option<I> {
    let (mime, payload) = uri.strip_prefix("data:")?.split_once(";base64,")?;
    decode(mime, BASE64.decode(payload).ok()?)
}

fn content_block_markdown(content: &ContentBlock) -> String {
    match content {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::Image(image) => format!("*[Image: {}]*", image.mime_type),
        ContentBlock::Audio(audio) => format!("*[Audio: {}]*", audio.mime_type),
        ContentBlock::ResourceLink(resource) => {
            let label = resource.title.as_deref().unwrap_or(&resource.name);
            format!("[{label}]({})", resource.uri)
        }
        ContentBlock::Resource(resource) => match &resource.resource {
            agent_client_protocol_schema::v1::EmbeddedResourceResource::TextResourceContents(
                resource,
            ) => resource.text.clone(),
            agent_client_protocol_schema::v1::EmbeddedResourceResource::BlobResourceContents(
                resource,
            ) => format!("*[Embedded resource: {}]*", resource.uri),
            _ => pretty_json_markdown(content),
        },
        _ => pretty_json_markdown(content),
    }
}

fn tool_location(tool: &ToolCall) -> Option<String> {
    tool.locations.first().map(|location| {
        location.line.map_or_else(
            || location.path.display().to_string(),
            |line| format!("{}:{line}", location.path.display()),
        )
    })
}

fn tool_input(tool: &ToolCall) -> Option<ToolPayload> {
    tool.raw_input.as_ref().and_then(json_payload)
}

fn tool_output(tool: &ToolCall) -> Vec<ToolPayload> {
    let structured = tool_content_payloads(&tool.content);
    if structured.is_empty() {
        tool.raw_output
            .as_ref()
            .and_then(json_payload)
            .into_iter()
            .collect()
    } else {
        structured
    }
}

fn tool_content_payloads(content: &[ToolCallContent]) -> Vec<ToolPayload> {
    content
        .iter()
        .take(MAX_AGENT_TOOL_CONTENT_ITEMS)
        .map(tool_content_payload)
        .collect()
}

fn tool_content_payload(content: &ToolCallContent) -> ToolPayload {
    match content {
        ToolCallContent::Diff(diff) => ToolPayload::Diff {
            path: diff.path.display().to_string(),
            old: diff
                .old_text
                .as_deref()
                .map(|old| copied_capped(old, MAX_DIFF_SIDE_BYTES)),
            new: copied_capped(&diff.new_text, MAX_DIFF_SIDE_BYTES),
        },
        ToolCallContent::Content(content) => match &content.content {
            ContentBlock::Text(text) => {
                ToolPayload::Text(copied_capped(&text.text, MAX_TOOL_PAYLOAD_BYTES))
            }
            _ => ToolPayload::Json(capped_payload(pretty_json(content).unwrap_or_default())),
        },
        ToolCallContent::Terminal(terminal) => {
            ToolPayload::Terminal(format!("[terminal {}]", terminal.terminal_id.0))
        }
        _ => ToolPayload::Json(capped_payload(pretty_json(content).unwrap_or_default())),
    }
}

fn pretty_json(value: &impl serde::Serialize) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

fn json_payload(value: &impl serde::Serialize) -> Option<ToolPayload> {
    pretty_json(value).map(|json| ToolPayload::Json(capped_payload(json)))
}

fn capped_payload(text: String) -> String {
    capped(text, MAX_TOOL_PAYLOAD_BYTES)
}

pub fn capped(mut text: String, max_bytes: usize) -> String {
    truncate_payload(&mut text, max_bytes);
    text
}

fn truncate_payload(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    text.truncate(payload_prefix_end(text, max_bytes));
    text.push_str(TRUNCATION_MARKER);
}

fn copied_capped(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let end = payload_prefix_end(text, max_bytes);
    let mut retained = String::with_capacity(end + TRUNCATION_MARKER.len());
    retained.push_str(&text[..end]);
    retained.push_str(TRUNCATION_MARKER);
    retained
}

fn payload_prefix_end(text: &str, max_bytes: usize) -> usize {
    let mut end = max_bytes.saturating_sub(TRUNCATION_MARKER.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn pretty_json_markdown(value: &impl serde::Serialize) -> String {
    pretty_json(value).map_or_else(String::new, |value| format!("```json\n{value}\n```"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(json: serde_json::Value) -> SessionUpdate {
        serde_json::from_value(json).expect("ACP update")
    }

    #[test]
    fn subagent_tool_rows_remember_their_parent_and_questions_ride_the_request() {
        let mut transcript = AgentTranscript::<()>::new(|_, _| None);
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "tool_call", "toolCallId": "agent-1", "title": "Survey", "kind": "think",
        })));
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "tool_call", "toolCallId": "read-1", "title": "Read a.rs", "kind": "read",
            "_meta": { "zz": { "parent": "agent-1" } },
        })));
        let child = transcript.tool_entry("read-1").expect("child row");
        let parent = transcript.tool_entry("agent-1").expect("parent row");
        assert_eq!(transcript.tool_parent(child), Some("agent-1"));
        assert_eq!(transcript.tool_parent(parent), None);
        let tool_call: ToolCallUpdate = serde_json::from_value(serde_json::json!({
            "toolCallId": "ask-1", "title": "Which fruit?",
        }))
        .expect("tool call");
        transcript.request_questions(
            9,
            tool_call,
            Vec::new(),
            vec![AgentQuestion {
                id: "fruit".to_owned(),
                question: "Which fruit?".to_owned(),
                multi_select: true,
                ..AgentQuestion::default()
            }],
        );
        let pending = transcript.permissions();
        assert_eq!(pending[0].questions[0].id, "fruit");
        assert!(pending[0].options.is_empty());
    }

    #[test]
    fn a_step_that_arrives_before_its_agent_changes_once_the_agent_appears() {
        let mut transcript = AgentTranscript::<()>::new(|_, _| None);
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "tool_call", "toolCallId": "read-1", "title": "Read a.rs", "kind": "read",
            "_meta": { "zz": { "parent": "agent-1" } },
        })));
        let child = transcript.tool_entry("read-1").expect("child row");
        assert_eq!(transcript.tool_parent(child), Some("agent-1"));
        assert_eq!(transcript.tool_entry("agent-1"), None);
        let before = transcript.revision();
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "tool_call", "toolCallId": "agent-1", "title": "Survey", "kind": "think",
        })));
        let changed = transcript.changed_entries(before).expect("change log");
        assert!(
            changed.contains(&0),
            "the waiting step is touched: {changed:?}"
        );
        assert!(changed.contains(&1));
        let before = transcript.revision();
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "tool_call", "toolCallId": "agent-2", "title": "Survey", "kind": "think",
        })));
        assert_eq!(transcript.changed_entries(before), Some(vec![2]));
    }

    #[test]
    fn zz_replies_attach_to_their_command_and_failed_tools_keep_the_exit_code() {
        let mut transcript = AgentTranscript::<()>::new(|_, _| None);
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "user_message_chunk", "messageId": "zz-command-1-in",
            "content": { "type": "text", "text": "/btw why" },
        })));
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "agent_message_chunk", "messageId": "zz-command-1-out",
            "content": { "type": "text", "text": "because" },
            "_meta": { "zz": { "side": true, "reply": "zz-command-1-in" } },
        })));
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "agent_message_chunk", "messageId": "zz-notice-1",
            "content": { "type": "text", "text": "Forked." },
            "_meta": { "zz": { "notice": true } },
        })));
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "tool_call", "toolCallId": "bash-1", "title": "false", "kind": "execute",
        })));
        transcript.apply_update(update(serde_json::json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "bash-1", "status": "failed",
            "_meta": { "zz": { "exitCode": 2 } },
        })));
        let prompt = transcript.entries()[0].id();
        let asides = transcript
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                AgentThreadEntry::Assistant { aside, .. } => Some(*aside),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            asides,
            [
                Some(AgentAside {
                    side: true,
                    reply_to: Some(prompt)
                }),
                Some(AgentAside {
                    side: false,
                    reply_to: None
                }),
            ]
        );
        assert!(matches!(
            transcript.entries().last(),
            Some(AgentThreadEntry::Tool {
                status: AgentToolStatusModel::Failed,
                exit_code: Some(2),
                ..
            })
        ));
    }

    #[test]
    fn prompt_rows_keep_the_message_id_a_rewind_targets() {
        let mut transcript = AgentTranscript::<()>::new(|_, _| None);
        let prompt = |id: &str, text: &str| {
            update(serde_json::json!({
                "sessionUpdate": "user_message_chunk", "messageId": id,
                "content": { "type": "text", "text": text },
            }))
        };
        transcript.apply_update(prompt("u-1", "first"));
        transcript.apply_update(prompt("zz-command-1-in", "/btw what now"));
        transcript.apply_update(prompt("u-2", "/compact"));
        transcript.finish_replay();
        transcript.begin_prompt("second".to_owned(), Vec::new());
        let before = transcript.revision();
        transcript.apply_update(prompt("u-3", "second"));
        assert_eq!(
            transcript.changed_entries(before),
            Some(vec![3]),
            "the echo names the row it echoes"
        );
        transcript.apply_update(prompt("zz-command-2-in", "/steer faster"));
        transcript.apply_update(prompt("u-3", "second"));

        let message_ids = transcript
            .entries()
            .iter()
            .map(|entry| match entry {
                AgentThreadEntry::User { message_id, .. } => message_id.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            message_ids,
            [
                Some("u-1"),
                Some("zz-command-1-in"),
                Some("u-2"),
                Some("u-3")
            ]
        );
        let rewind_ids = transcript
            .entries()
            .iter()
            .map(AgentThreadEntry::rewind_id)
            .collect::<Vec<_>>();
        assert_eq!(rewind_ids, [Some("u-1"), None, None, Some("u-3")]);
        assert_eq!(rewind_command("u-3"), "/rewind u-3");
        assert!(is_zz_command("  /rewind u-3"));
        assert!(!is_zz_command("//rewind u-3"));
        assert!(!is_zz_command("/compact"));
    }

    #[test]
    fn copied_payloads_preserve_owned_truncation_behavior() {
        let unicode = "é🙂".repeat(16);
        for text in ["", "short", "already … [truncated]", unicode.as_str()] {
            for limit in [
                0,
                1,
                TRUNCATION_MARKER.len(),
                TRUNCATION_MARKER.len() + 5,
                128,
            ] {
                assert_eq!(copied_capped(text, limit), capped(text.to_owned(), limit));
            }
        }
    }

    #[test]
    fn copied_tool_payloads_do_not_retain_discarded_storage() {
        let input = "é".repeat(4 * MAX_TOOL_PAYLOAD_BYTES);
        let text = ContentBlock::Text(agent_client_protocol_schema::v1::TextContent::new(
            input.clone(),
        ));
        let ToolPayload::Text(retained_text) = tool_content_payload(&text.into()) else {
            panic!("text content did not produce a text payload");
        };
        let diff = agent_client_protocol_schema::v1::Diff::new("file.rs", input.clone())
            .old_text(input.clone());
        let ToolPayload::Diff {
            old: Some(old),
            new,
            ..
        } = tool_content_payload(&diff.into())
        else {
            panic!("diff content did not preserve both sides");
        };
        for (retained, limit) in [
            (retained_text, MAX_TOOL_PAYLOAD_BYTES),
            (old, MAX_DIFF_SIDE_BYTES),
            (new, MAX_DIFF_SIDE_BYTES),
        ] {
            assert!(retained.len() <= limit);
            assert!(retained.ends_with(TRUNCATION_MARKER));
            assert!(retained.capacity() <= limit);
            assert_eq!(retained, capped(input.clone(), limit));
        }
    }
}
