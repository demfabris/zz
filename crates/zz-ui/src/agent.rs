use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, OnceLock},
};

use instant::{Duration, Instant};

use crate::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, Icon, IconName, Sizable as _, WindowExt as _,
    attachment::{open_attachment_preview, open_render_image_preview},
    button::{Button, ButtonVariants as _},
    control_shadow, h_flex,
    mend::{PENDING_LINK_URL, mend},
    scroll::ScrollableElement as _,
    text::{
        CodeBlock, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
        TextView, TextViewState, TextViewStyle, markdown_ast,
    },
    v_flex,
};
use gpui::{
    AnyElement, App, ClipboardItem, Context, DispatchPhase, Div, ElementId, Entity, FollowMode,
    FontStyle, FontWeight, Global, HighlightStyle, Hsla, Image, ImageSource, InteractiveText,
    IntoElement, ListSizingBehavior, ListState, ObjectFit, Pixels, RenderImage, Rgba,
    ScrollStrategy, ScrollWheelEvent, SharedString, Stateful, StyledText, Task,
    UniformListScrollHandle, Window, canvas, div, img, list, prelude::*, px, relative,
    uniform_list,
};
use parking_lot::RwLock;
use similar::{ChangeTag, TextDiff};

const MERMAID_NODE_NAME: &str = "zz-mermaid";
const RICH_MARKDOWN_NODE_NAME: &str = "zz-rich-markdown";
const INLINE_CODE_PARAGRAPH_NODE_NAME: &str = "zz-inline-code-paragraph";
const MERMAID_MAX_HEIGHT: f32 = 560.0;
const MERMAID_MAX_SOURCE_BYTES: usize = 32 * 1024;
const MERMAID_RENDER_DEBOUNCE: Duration = Duration::from_millis(250);
const TOOL_CONTENT_MAX_HEIGHT: f32 = 360.0;
const TOOL_CONTENT_MAX_LINES: usize = 2_000;
const TOOL_CONTENT_MAX_BYTES: usize = 64 * 1024;
const TOOL_CONTENT_ROW_HEIGHT: f32 = 20.0;
const MERMAID_CACHE_CAPACITY: usize = 16;
const MAX_STREAMING_MEND_BYTES: usize = 64 * 1024;
const MARKDOWN_PREVIEW_MAX_BYTES: usize = 32 * 1024;
const MARKDOWN_PREVIEW_MAX_LINES: usize = 512;
const MARKDOWN_PREVIEW_HEIGHT: f32 = 420.0;
const MARKDOWN_PREVIEW_MARKER: &str =
    "\n\n… [large message preview stopped; copy the full message below]";
/// Where a link whose URL is still streaming is pointed. The renderer refuses
/// to open `data:` URLs, which is what keeps the mend sentinel inert.
const INERT_LINK_URL: &str = "data:,";
pub const AGENT_CONTENT_MAX_WIDTH: f32 = 680.0;
/// Side of a square attachment tile in a sent message.
pub const TRANSCRIPT_ATTACHMENT: Pixels = px(140.0);
/// Side of a square attachment tile in the composer.
pub const COMPOSER_ATTACHMENT: Pixels = px(56.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentToolKind {
    Read,
    Search,
    Edit,
    Execute,
    Fetch,
    Think,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentToolStatus {
    Pending,
    Running,
    NeedsApproval,
    Completed,
    Failed,
    Canceled,
}

#[derive(Clone, Default)]
pub struct AgentToolText(Arc<RwLock<AgentMarkdownBuffer>>);

impl AgentToolText {
    #[must_use]
    pub fn new(source: impl Into<String>) -> Self {
        Self(Arc::new(RwLock::new(AgentMarkdownBuffer {
            source: source.into(),
            rendered: String::new(),
            revision: 0,
            replaced_at: 0,
            line_breaks: 0,
            truncated: false,
        })))
    }

    pub fn synchronize(&self, source: &str) {
        let mut buffer = self.0.write();
        let len = buffer.source.len();
        if buffer.source == source {
            return;
        }
        buffer.revision = buffer.revision.wrapping_add(1);
        if len < source.len()
            && source.is_char_boundary(len)
            && buffer.source.as_bytes() == &source.as_bytes()[..len]
        {
            buffer.source.push_str(&source[len..]);
        } else {
            buffer.source.clear();
            buffer.source.push_str(source);
            buffer.replaced_at = buffer.revision;
        }
    }

    #[must_use]
    pub fn contains(&self, pattern: &str) -> bool {
        self.0.read().source.contains(pattern)
    }

    fn revisions(&self) -> (u64, u64) {
        let buffer = self.0.read();
        (buffer.revision, buffer.replaced_at)
    }

    fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    fn inspect<R>(&self, inspect: impl FnOnce(&str) -> R) -> R {
        inspect(&self.0.read().source)
    }
}

impl std::fmt::Debug for AgentToolText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inspect(|source| {
            formatter
                .debug_tuple("AgentToolText")
                .field(&source)
                .finish()
        })
    }
}

impl std::fmt::Display for AgentToolText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inspect(|source| formatter.write_str(source))
    }
}

impl PartialEq for AgentToolText {
    fn eq(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.0, &other.0) {
            return true;
        }
        let source = self.0.read().source.clone();
        source == other.0.read().source
    }
}

impl Eq for AgentToolText {}

impl PartialEq<str> for AgentToolText {
    fn eq(&self, other: &str) -> bool {
        self.0.read().source == other
    }
}

impl From<String> for AgentToolText {
    fn from(source: String) -> Self {
        Self::new(source)
    }
}

impl From<&str> for AgentToolText {
    fn from(source: &str) -> Self {
        Self::new(source)
    }
}

impl From<SharedString> for AgentToolText {
    fn from(source: SharedString) -> Self {
        Self::new(String::from(source))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentToolPayload {
    Diff {
        path: SharedString,
        old: Option<AgentToolText>,
        new: AgentToolText,
    },
    Text(AgentToolText),
    Json(AgentToolText),
    Terminal(AgentToolText),
}

impl AgentToolPayload {
    fn revisions(&self) -> ((u64, u64), (u64, u64)) {
        match self {
            Self::Diff { old, new, .. } => (
                old.as_ref().map_or((0, 0), AgentToolText::revisions),
                new.revisions(),
            ),
            Self::Text(text) | Self::Json(text) | Self::Terminal(text) => {
                (text.revisions(), (0, 0))
            }
        }
    }

    fn is_same_source(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Diff {
                    path: left_path,
                    old: left_old,
                    new: left_new,
                },
                Self::Diff {
                    path: right_path,
                    old: right_old,
                    new: right_new,
                },
            ) => {
                left_path == right_path
                    && match (left_old, right_old) {
                        (Some(left), Some(right)) => left.is_same(right),
                        (None, None) => true,
                        _ => false,
                    }
                    && left_new.is_same(right_new)
            }
            (Self::Text(left), Self::Text(right))
            | (Self::Json(left), Self::Json(right))
            | (Self::Terminal(left), Self::Terminal(right)) => left.is_same(right),
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum MarkdownSlot {
    Body,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum DisclosureKind {
    Reasoning,
    Tool,
    Group,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum ToolContentSlot {
    Combined,
}

struct MarkdownState {
    source: AgentMarkdown,
    revision: u64,
    len: usize,
    mended: bool,
    state: Entity<TextViewState>,
}

#[derive(Default)]
struct AgentMarkdownBuffer {
    source: String,
    rendered: String,
    revision: u64,
    replaced_at: u64,
    line_breaks: usize,
    truncated: bool,
}

#[derive(Clone, Default)]
pub struct AgentMarkdown(Arc<RwLock<AgentMarkdownBuffer>>);

impl AgentMarkdown {
    #[must_use]
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let line_breaks = source.bytes().filter(|byte| *byte == b'\n').count();
        let (rendered, truncated) = markdown_preview(&source, line_breaks);
        Self(Arc::new(RwLock::new(AgentMarkdownBuffer {
            source,
            rendered,
            revision: 0,
            replaced_at: 0,
            line_breaks,
            truncated,
        })))
    }

    pub fn synchronize_append(&self, source: &str) {
        let mut buffer = self.0.write();
        let len = buffer.source.len();
        if buffer.source == source {
            return;
        }
        if len < source.len()
            && source.is_char_boundary(len)
            && buffer.source.as_bytes() == &source.as_bytes()[..len]
        {
            let appended = &source[len..];
            buffer.source.push_str(appended);
            buffer.line_breaks = buffer
                .line_breaks
                .saturating_add(appended.bytes().filter(|byte| *byte == b'\n').count());
            if buffer.truncated {
                return;
            }
            if markdown_preview_end(&buffer.source, buffer.line_breaks).is_none() {
                buffer.rendered.push_str(appended);
                buffer.revision = buffer.revision.wrapping_add(1);
                return;
            }
        }
        replace_markdown_buffer(&mut buffer, source);
    }

    pub fn replace(&self, source: &str) {
        let mut buffer = self.0.write();
        if buffer.source == source {
            return;
        }
        replace_markdown_buffer(&mut buffer, source);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.read().source.is_empty()
    }

    #[must_use]
    pub fn trim_is_empty(&self) -> bool {
        self.0.read().source.trim().is_empty()
    }

    #[must_use]
    pub fn is_truncated(&self) -> bool {
        self.0.read().truncated
    }

    #[must_use]
    pub fn full_text(&self) -> String {
        self.0.read().source.clone()
    }

    fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    fn inspect<R>(&self, inspect: impl FnOnce(&str, u64, u64) -> R) -> R {
        let buffer = self.0.read();
        inspect(&buffer.rendered, buffer.revision, buffer.replaced_at)
    }
}

fn replace_markdown_buffer(buffer: &mut AgentMarkdownBuffer, source: &str) {
    let line_breaks = source.bytes().filter(|byte| *byte == b'\n').count();
    let (rendered, truncated) = markdown_preview(source, line_breaks);
    buffer.source.clear();
    buffer.source.push_str(source);
    buffer.line_breaks = line_breaks;
    buffer.truncated = truncated;
    if buffer.rendered != rendered {
        buffer.revision = buffer.revision.wrapping_add(1);
        buffer.replaced_at = buffer.revision;
        buffer.rendered = rendered;
    }
}

fn markdown_preview(source: &str, line_breaks: usize) -> (String, bool) {
    let Some(end) = markdown_preview_end(source, line_breaks) else {
        return (source.to_owned(), false);
    };
    let prefix = &source[..end];
    let mut rendered = mend(prefix).unwrap_or_else(|| prefix.to_owned());
    rendered.push_str(MARKDOWN_PREVIEW_MARKER);
    (rendered, true)
}

fn markdown_preview_end(source: &str, line_breaks: usize) -> Option<usize> {
    let mut end = source.len().min(MARKDOWN_PREVIEW_MAX_BYTES);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    if line_breaks >= MARKDOWN_PREVIEW_MAX_LINES
        && let Some((newline, _)) = source[..end]
            .match_indices('\n')
            .nth(MARKDOWN_PREVIEW_MAX_LINES - 1)
    {
        end = newline + 1;
    }
    (end < source.len()).then_some(end)
}

impl std::fmt::Debug for AgentMarkdown {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inspect(|source, _, _| {
            formatter
                .debug_tuple("AgentMarkdown")
                .field(&source)
                .finish()
        })
    }
}

impl PartialEq for AgentMarkdown {
    fn eq(&self, other: &Self) -> bool {
        if self.is_same(other) {
            return true;
        }
        let source = self.0.read().source.clone();
        source == other.0.read().source
    }
}

impl Eq for AgentMarkdown {}

impl PartialEq<str> for AgentMarkdown {
    fn eq(&self, other: &str) -> bool {
        self.0.read().source == other
    }
}

impl PartialEq<&str> for AgentMarkdown {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl From<String> for AgentMarkdown {
    fn from(source: String) -> Self {
        Self::new(source)
    }
}

impl From<&str> for AgentMarkdown {
    fn from(source: &str) -> Self {
        Self::new(source)
    }
}

impl From<SharedString> for AgentMarkdown {
    fn from(source: SharedString) -> Self {
        Self::new(String::from(source))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ToolContentSource {
    location: Option<SharedString>,
    input: Option<AgentToolPayload>,
    output: Arc<[AgentToolPayload]>,
    revisions: Arc<[((u64, u64), (u64, u64))]>,
}

impl ToolContentSource {
    fn is_same_snapshot(&self, other: &Self) -> bool {
        self.location == other.location
            && self.revisions == other.revisions
            && match (&self.input, &other.input) {
                (Some(left), Some(right)) => left.is_same_source(right),
                (None, None) => true,
                _ => false,
            }
            && self.output.len() == other.output.len()
            && self
                .output
                .iter()
                .zip(other.output.iter())
                .all(|(left, right)| left.is_same_source(right))
    }
}

#[derive(Clone, Debug)]
struct ToolContentState {
    source: ToolContentSource,
    rows: Arc<[ToolContentRow]>,
}

#[derive(Clone, Debug)]
enum CachedToolContent {
    Source(ToolContentSource),
    Materialized(Arc<ToolContentState>),
}

impl CachedToolContent {
    fn source(&self) -> &ToolContentSource {
        match self {
            Self::Source(source) => source,
            Self::Materialized(content) => &content.source,
        }
    }
}

fn tool_content_source(
    location: Option<SharedString>,
    input: Option<AgentToolPayload>,
    output: Arc<[AgentToolPayload]>,
) -> ToolContentSource {
    let revisions = input
        .iter()
        .chain(output.iter())
        .map(AgentToolPayload::revisions)
        .collect::<Vec<_>>()
        .into();
    ToolContentSource {
        location,
        input,
        output,
        revisions,
    }
}

#[derive(Clone, Debug)]
enum ToolContentRow {
    Section {
        label: &'static str,
        copy: Option<Arc<[AgentToolPayload]>>,
    },
    Path(SharedString),
    Plain(SharedString),
    Diff {
        kind: DiffLineKind,
        text: SharedString,
    },
    Footer(SharedString),
    Spacer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffLineKind {
    Equal,
    Added,
    Removed,
}

#[derive(Default)]
pub struct AgentTimelineStore {
    markdown: HashMap<(u64, MarkdownSlot), MarkdownState>,
    tool_content: HashMap<(u64, ToolContentSlot), CachedToolContent>,
    expanded: HashMap<(u64, DisclosureKind), bool>,
    tool_scrolls: HashMap<u64, UniformListScrollHandle>,
    cwd: Option<PathBuf>,
    markdown_extensions: HashMap<bool, MarkdownExtensions>,
    /// The entry still receiving deltas, whose display copy is mended.
    streaming: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkdownUpdate {
    Missing,
    Unchanged,
    Appended,
    Replaced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolContentUpdate {
    Missing,
    Unchanged,
    Replaced,
}

impl AgentTimelineStore {
    pub fn markdown(
        &mut self,
        id: u64,
        slot: MarkdownSlot,
        source: AgentMarkdown,
        cx: &mut Context<Self>,
    ) -> Entity<TextViewState> {
        if let Some(markdown) = self.markdown.get(&(id, slot)) {
            return markdown.state.clone();
        }
        let streaming = self.streaming == Some(id);
        let (state, revision, len, mended) = source.inspect(|text, revision, _| {
            let repair = (streaming && text.len() <= MAX_STREAMING_MEND_BYTES)
                .then(|| mend(text))
                .flatten();
            (
                cx.new(|cx| {
                    let display = repair.as_deref().unwrap_or(text);
                    if repair.is_some() || display.len() > MAX_STREAMING_MEND_BYTES {
                        TextViewState::markdown_deferred(display, cx)
                    } else {
                        TextViewState::markdown(display, cx)
                    }
                }),
                revision,
                text.len(),
                repair.is_some(),
            )
        });
        self.markdown.insert(
            (id, slot),
            MarkdownState {
                source,
                revision,
                len,
                mended,
                state: state.clone(),
            },
        );
        state
    }

    /// Name the entry that is still streaming, so its display copy is mended
    /// while markers hang. The entry that leaves the slot settles back to its
    /// raw text: a completed entry always renders exactly what it holds.
    pub fn set_streaming(&mut self, id: Option<u64>, cx: &mut Context<Self>) {
        if self.streaming == id {
            return;
        }
        let settled = self.streaming;
        self.streaming = id;
        if let Some(settled) = settled {
            self.settle_markdown(settled, cx);
        }
    }

    fn settle_markdown(&mut self, id: u64, cx: &mut Context<Self>) {
        let mut settled = false;
        for ((entry, _), markdown) in &mut self.markdown {
            if *entry != id || !markdown.mended {
                continue;
            }
            markdown.mended = false;
            markdown.source.inspect(|source, revision, _| {
                markdown.state.update(cx, |state, cx| {
                    state.replace_markdown(source, source.len() > MAX_STREAMING_MEND_BYTES, cx);
                });
                markdown.revision = revision;
                markdown.len = source.len();
            });
            settled = true;
        }
        if settled {
            cx.notify();
        }
    }

    pub fn synchronize_markdown(
        &mut self,
        id: u64,
        slot: MarkdownSlot,
        source: AgentMarkdown,
        cx: &mut Context<Self>,
    ) {
        _ = self.update_markdown(id, slot, source, cx);
    }

    fn update_markdown(
        &mut self,
        id: u64,
        slot: MarkdownSlot,
        source: AgentMarkdown,
        cx: &mut Context<Self>,
    ) -> MarkdownUpdate {
        let streaming = self.streaming == Some(id);
        let Some(markdown) = self.markdown.get_mut(&(id, slot)) else {
            return MarkdownUpdate::Missing;
        };
        let same_source = markdown.source.is_same(&source);
        let update = source.inspect(|text, revision, replaced_at| {
            if same_source && revision == markdown.revision && !markdown.mended {
                return MarkdownUpdate::Unchanged;
            }
            let repair = (streaming && text.len() <= MAX_STREAMING_MEND_BYTES)
                .then(|| mend(text))
                .flatten();
            if same_source && revision == markdown.revision && repair.is_some() {
                return MarkdownUpdate::Unchanged;
            }
            let update = match repair.as_deref() {
                Some(display) => {
                    markdown.state.update(cx, |state, cx| {
                        state.replace_markdown(display, true, cx);
                    });
                    MarkdownUpdate::Replaced
                }
                None if same_source
                    && !markdown.mended
                    && markdown.revision >= replaced_at
                    && markdown.len <= text.len() =>
                {
                    markdown
                        .state
                        .update(cx, |state, cx| state.push_str(&text[markdown.len..], cx));
                    MarkdownUpdate::Appended
                }
                None => {
                    markdown.state.update(cx, |state, cx| {
                        state.replace_markdown(text, text.len() > MAX_STREAMING_MEND_BYTES, cx);
                    });
                    MarkdownUpdate::Replaced
                }
            };
            markdown.revision = revision;
            markdown.len = text.len();
            markdown.mended = repair.is_some();
            update
        });
        markdown.source = source;
        cx.notify();
        update
    }

    /// Session working directory, used to resolve relative file links in
    /// message bodies. Changing it reparses every retained `TextView`.
    pub fn set_cwd(&mut self, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.cwd == cwd {
            return;
        }
        self.cwd = cwd;
        self.markdown_extensions.clear();
        cx.notify();
    }

    fn markdown_extensions_for(&mut self, assistant: bool) -> MarkdownExtensions {
        let cwd = self.cwd.clone();
        self.markdown_extensions
            .entry(assistant)
            .or_insert_with(|| {
                let base = if assistant {
                    assistant_markdown_extensions()
                } else {
                    standard_markdown_extensions()
                };
                base.link_rewriter(move |url| resolve_workspace_link(cwd.as_deref(), url))
            })
            .clone()
    }

    pub fn expanded(&mut self, id: u64, kind: DisclosureKind, default_expanded: bool) -> bool {
        *self.expanded.entry((id, kind)).or_insert(default_expanded)
    }

    pub fn toggle_expanded(
        &mut self,
        id: u64,
        kind: DisclosureKind,
        default_expanded: bool,
        cx: &mut Context<Self>,
    ) {
        let expanded = self.expanded.entry((id, kind)).or_insert(default_expanded);
        *expanded = !*expanded;
        cx.notify();
    }

    fn tool_content(
        &mut self,
        id: u64,
        location: Option<SharedString>,
        input: Option<AgentToolPayload>,
        output: Arc<[AgentToolPayload]>,
    ) -> Arc<ToolContentState> {
        let source = tool_content_source(location, input, output);
        let key = (id, ToolContentSlot::Combined);
        if let Some(CachedToolContent::Materialized(content)) = self.tool_content.get(&key)
            && content.source.is_same_snapshot(&source)
        {
            return content.clone();
        }

        let tail_terminal = source
            .output
            .iter()
            .any(|payload| matches!(payload, AgentToolPayload::Terminal(_)));
        let content = Arc::new(materialize_tool_content(source));
        if tail_terminal {
            self.tool_scrolls
                .entry(id)
                .or_default()
                .scroll_to_item(content.rows.len().saturating_sub(1), ScrollStrategy::Bottom);
        }
        self.tool_content
            .insert(key, CachedToolContent::Materialized(content.clone()));
        content
    }

    pub fn synchronize_tool_content(
        &mut self,
        id: u64,
        location: Option<SharedString>,
        input: Option<AgentToolPayload>,
        output: Arc<[AgentToolPayload]>,
        cx: &mut Context<Self>,
    ) {
        _ = self.update_tool_content(id, location, input, output, cx);
    }

    fn update_tool_content(
        &mut self,
        id: u64,
        location: Option<SharedString>,
        input: Option<AgentToolPayload>,
        output: Arc<[AgentToolPayload]>,
        cx: &mut Context<Self>,
    ) -> ToolContentUpdate {
        let source = tool_content_source(location, input, output);
        let key = (id, ToolContentSlot::Combined);
        let Some(content) = self.tool_content.get_mut(&key) else {
            self.tool_content
                .insert(key, CachedToolContent::Source(source));
            return ToolContentUpdate::Missing;
        };
        if content.source().is_same_snapshot(&source) {
            return ToolContentUpdate::Unchanged;
        }

        *content = CachedToolContent::Source(source);
        cx.notify();
        ToolContentUpdate::Replaced
    }

    pub fn tool_scroll(&mut self, id: u64) -> UniformListScrollHandle {
        self.tool_scrolls.entry(id).or_default().clone()
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) -> bool {
        self.streaming = None;
        if self.markdown.is_empty()
            && self.tool_content.is_empty()
            && self.expanded.is_empty()
            && self.tool_scrolls.is_empty()
        {
            return false;
        }
        self.markdown.clear();
        self.tool_content.clear();
        self.expanded.clear();
        self.tool_scrolls.clear();
        cx.notify();
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentEntry {
    User {
        id: u64,
        markdown: AgentMarkdown,
        /// Images sent with the message, shown above its text as tiles.
        images: Arc<[Arc<Image>]>,
    },
    Assistant {
        id: u64,
        markdown: AgentMarkdown,
    },
    Reasoning {
        id: u64,
        label: SharedString,
        markdown: AgentMarkdown,
        default_expanded: bool,
    },
    Plan {
        id: u64,
        markdown: AgentMarkdown,
    },
    Tool(AgentToolEntry),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentToolEntry {
    pub id: u64,
    pub kind: AgentToolKind,
    pub status: AgentToolStatus,
    pub label: SharedString,
    pub location: Option<SharedString>,
    pub input: Option<AgentToolPayload>,
    pub output: Arc<[AgentToolPayload]>,
    pub default_expanded: bool,
}

impl AgentEntry {
    #[must_use]
    pub const fn id(&self) -> u64 {
        match self {
            Self::User { id, .. }
            | Self::Assistant { id, .. }
            | Self::Reasoning { id, .. }
            | Self::Plan { id, .. } => *id,
            Self::Tool(tool) => tool.id,
        }
    }
}

/// The entry kinds that collapse into one row when they run consecutively.
/// Every other kind stands alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineGroupKind {
    Tool,
    Reasoning,
}

#[must_use]
pub const fn timeline_group_kind(entry: &AgentEntry) -> Option<TimelineGroupKind> {
    match entry {
        AgentEntry::Tool(_) => Some(TimelineGroupKind::Tool),
        AgentEntry::Reasoning { .. } => Some(TimelineGroupKind::Reasoning),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineRow {
    Single(AgentEntry),
    Group {
        kind: TimelineGroupKind,
        id: u64,
        entries: Arc<Vec<AgentEntry>>,
    },
}

impl TimelineRow {
    #[must_use]
    pub fn entry(&self, id: u64) -> Option<&AgentEntry> {
        match self {
            Self::Single(entry) => (entry.id() == id).then_some(entry),
            Self::Group { entries, .. } => entries.iter().find(|entry| entry.id() == id),
        }
    }

    pub fn replace_entry(&mut self, id: u64, entry: AgentEntry) -> bool {
        if entry.id() != id {
            return false;
        }
        match self {
            Self::Single(current) if current.id() == id => {
                *current = entry;
                true
            }
            Self::Group { entries, .. } => {
                let Some(index) = entries.iter().position(|current| current.id() == id) else {
                    return false;
                };
                Arc::make_mut(entries)[index] = entry;
                true
            }
            Self::Single(_) => false,
        }
    }
}

pub struct FoldedTimelineRows {
    pub rows: Arc<Vec<TimelineRow>>,
    pub entry_to_row: Vec<usize>,
}

#[must_use]
pub fn fold_timeline_rows(entries: &[AgentEntry]) -> FoldedTimelineRows {
    let mut rows = Vec::new();
    let mut entry_to_row = Vec::with_capacity(entries.len());
    for entry in entries.iter().cloned() {
        let (row_index, _) = append_timeline_row(&mut rows, entry);
        entry_to_row.push(row_index);
    }
    FoldedTimelineRows {
        rows: Arc::new(rows),
        entry_to_row,
    }
}

#[must_use]
pub fn append_timeline_row(rows: &mut Vec<TimelineRow>, entry: AgentEntry) -> (usize, bool) {
    if let Some(kind) = timeline_group_kind(&entry)
        && let (Some(row_index), Some(last)) = (rows.len().checked_sub(1), rows.last_mut())
    {
        match last {
            TimelineRow::Single(previous) if timeline_group_kind(previous) == Some(kind) => {
                let id = previous.id();
                let previous = previous.clone();
                *last = TimelineRow::Group {
                    kind,
                    id,
                    entries: Arc::new(vec![previous, entry]),
                };
                return (row_index, false);
            }
            TimelineRow::Group {
                kind: open,
                entries,
                ..
            } if *open == kind => {
                Arc::make_mut(entries).push(entry);
                return (row_index, false);
            }
            TimelineRow::Single(_) | TimelineRow::Group { .. } => {}
        }
    }

    let row_index = rows.len();
    rows.push(TimelineRow::Single(entry));
    (row_index, true)
}

/// Padding above the first timeline row. It lives inside the scrolled content,
/// so a caller measuring the distance to the end has to account for it.
pub const AGENT_TIMELINE_TOP_PADDING: f32 = 16.0;
/// Treat the timeline as exactly pinned within this distance of the end.
pub const AGENT_AT_BOTTOM_PX: f32 = 2.0;
/// Offer the jump-to-bottom pill beyond this distance from the end.
pub const AGENT_JUMP_TO_BOTTOM_PX: f32 = 320.0;
/// Teleport when farther than this many viewports from the end, then glide the
/// rest — a full-history jump would otherwise spend seconds scrolling.
pub const AGENT_GLIDE_MAX_VIEWPORTS: f32 = 2.5;
/// Keep the spring loop warm this long after landing, so a pause between
/// streamed chunks resumes at cruise instead of re-accelerating from zero.
pub const AGENT_SPRING_SETTLE_GRACE: Duration = Duration::from_millis(500);
/// Re-engage the pin when a user scroll returns within this many px of the end.
const AGENT_STICK_THRESHOLD_PX: f32 = 70.0;

const SPRING_DAMPING: f32 = 0.7;
const SPRING_STIFFNESS: f32 = 0.05;
const SPRING_MASS: f32 = 1.25;
const SPRING_FRAME_MS: f32 = 1000.0 / 60.0;
const SPRING_MAX_CATCHUP_FRAMES: f32 = 8.0;
const SPRING_GROWTH_EMA: f32 = 0.12;
const SPRING_CHASE_MAX_LEAD: f32 = 32.0;
const SPRING_CHASE_LEAD_FRAMES: f32 = 9.0;

/// Whether a user scroll should re-engage the bottom pin: inside the stick band
/// *and* moving toward the end. Direction matters — a small wheel-up notch from
/// the pinned bottom stays inside the band, and resticking on it would snap the
/// view straight back, making the pin impossible to break.
pub fn agent_should_restick(distance: f32, previous: f32) -> bool {
    distance <= AGENT_STICK_THRESHOLD_PX && distance < previous
}

/// Pure stick-to-bottom spring stepper. Velocity relaxes toward
/// `(damping·v + stiffness·diff)/mass` per 60fps sub-frame, position advances
/// by `v + target_vel` where `target_vel` is a feed-forward EMA of target
/// growth in px per frame, and the chase point sits up to
/// [`SPRING_CHASE_MAX_LEAD`] px above the true end in proportion to that
/// growth — so a streaming tail is followed at its own speed instead of being
/// hauled after a target that has already moved again.
#[derive(Debug, Clone, Copy)]
pub struct StickSpring {
    velocity: f32,
    target_vel: f32,
    last_target: Option<f32>,
}

impl Default for StickSpring {
    fn default() -> Self {
        Self::new()
    }
}

impl StickSpring {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            velocity: 0.0,
            target_vel: 0.0,
            last_target: None,
        }
    }

    /// Park the spring; the next step starts cold.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Whether the residual motion is below the settle threshold.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.velocity < 0.05 && self.target_vel < 0.05
    }

    /// `elapsed` in 60fps frames, capped so a hitch catches up over a few
    /// sub-steps rather than teleporting a frame's worth of stalled time.
    #[must_use]
    pub fn frames(elapsed: Duration) -> f32 {
        (elapsed.as_secs_f32() * 1000.0 / SPRING_FRAME_MS).min(SPRING_MAX_CATCHUP_FRAMES)
    }

    /// Advance one tick. `pos` and `target` are scroll offsets in px, larger
    /// meaning closer to the end. Never overshoots `target`, is monotone while
    /// approaching, and snaps exactly once within half a pixel.
    #[must_use]
    pub fn step(&mut self, mut pos: f32, target: f32, mut frames: f32) -> f32 {
        let grew = self.last_target.map_or(0.0, |last| target - last);
        self.last_target = Some(target);
        if grew < -1.0 {
            self.target_vel = 0.0;
        } else {
            let observed = grew.max(0.0) / frames.max(0.25);
            self.target_vel += SPRING_GROWTH_EMA * (observed - self.target_vel);
        }
        let chase =
            target - (self.target_vel * SPRING_CHASE_LEAD_FRAMES).min(SPRING_CHASE_MAX_LEAD);
        let mut velocity = self.velocity;
        while frames > 0.0 {
            let step = frames.min(1.0);
            frames -= step;
            let diff = (chase - pos).max(0.0);
            velocity += step
                * ((SPRING_DAMPING * velocity + SPRING_STIFFNESS * diff) / SPRING_MASS - velocity);
            pos = (pos + (velocity + self.target_vel) * step).min(target);
        }
        self.velocity = velocity;
        if target - pos <= 0.5 { target } else { pos }
    }

    #[cfg(test)]
    fn target_vel(&self) -> f32 {
        self.target_vel
    }
}

/// The jump-to-bottom pill. Paint it as an overlay: it must not take part in
/// the timeline's layout, or appearing would resize the scroll viewport and
/// move the very content it is offering to reveal.
pub fn agent_jump_to_bottom_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let pill = Button::new(id)
        .secondary()
        .xsmall()
        .rounded(px(999.0))
        .icon(IconName::ChevronDown)
        .label("Jump to latest");
    if cx.theme().shadow {
        pill.shadow(control_shadow(cx))
    } else {
        pill
    }
}

/// The timeline's tail pin: a spring that chases the end of the transcript
/// instead of teleporting to it on every streamed token.
///
/// The pin belongs to the caller, not to the list, so [`FollowMode::Tail`]
/// stays off unless reduced motion is on — gpui's tail mode both snaps on every
/// layout and re-engages itself from scroll *position*, which would make a
/// deliberate scroll-up impossible to hold while the agent is still writing.
pub struct TimelineStick {
    pinned: bool,
    spring: StickSpring,
    last_tick: Option<Instant>,
    settled_at: Option<Instant>,
    scheduled: bool,
    kick: bool,
    last_distance: f32,
    show_jump: bool,
    bottom_padding: f32,
}

impl TimelineStick {
    pub fn new(list: &ListState, reduce_motion: bool) -> Self {
        let mut stick = Self {
            pinned: true,
            spring: StickSpring::new(),
            last_tick: None,
            settled_at: None,
            scheduled: false,
            kick: false,
            last_distance: 0.0,
            show_jump: false,
            bottom_padding: 0.0,
        };
        stick.engage_now(list, reduce_motion);
        stick
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub fn shows_jump_button(&self) -> bool {
        self.show_jump
    }

    /// The list's own bottom padding, which the caller recomputes from whatever
    /// chrome overlaps the end of the transcript.
    pub fn set_bottom_padding(&mut self, padding: f32) {
        self.bottom_padding = padding;
    }

    /// The end of the scrollable range and the current distance to it, or
    /// `None` while the content is shorter than the viewport.
    ///
    /// `max_offset_for_scrollbar` measures the items alone, but the list also
    /// scrolls through its own padding, so the true end sits that much lower.
    fn bottom(&self, list: &ListState) -> Option<(f32, f32)> {
        let measured = f32::from(list.max_offset_for_scrollbar().y);
        if measured <= 0.0 {
            return None;
        }
        let target = measured + AGENT_TIMELINE_TOP_PADDING + self.bottom_padding;
        let position = -f32::from(list.scroll_px_offset_for_scrollbar().y);
        Some((target, (target - position).max(0.0)))
    }

    pub fn distance_from_bottom(&self, list: &ListState) -> f32 {
        self.bottom(list).map_or(0.0, |(_, distance)| distance)
    }

    /// Re-arm the driver without disturbing the position: content grew, or the
    /// pin was just taken.
    pub fn wake(&mut self) {
        self.settled_at = None;
        self.kick = true;
    }

    fn release(&mut self, list: &ListState) {
        self.pinned = false;
        self.spring.reset();
        self.last_tick = None;
        self.settled_at = None;
        self.kick = false;
        list.set_follow_mode(FollowMode::Normal);
    }

    /// Take the pin and land on the end immediately — for a transcript that is
    /// being replaced wholesale, where there is no motion to show.
    pub fn engage_now(&mut self, list: &ListState, reduce_motion: bool) {
        self.pinned = true;
        self.show_jump = false;
        self.spring.reset();
        self.last_tick = None;
        self.settled_at = None;
        self.kick = false;
        self.last_distance = 0.0;
        if reduce_motion {
            list.set_follow_mode(FollowMode::Tail);
        } else {
            list.set_follow_mode(FollowMode::Normal);
            list.scroll_to_end();
        }
    }

    /// Take the pin and glide to the end. A jump longer than
    /// [`AGENT_GLIDE_MAX_VIEWPORTS`] teleports most of the way first, so a
    /// whole-history return still lands in one gesture's worth of motion.
    pub fn engage(&mut self, list: &ListState, reduce_motion: bool) {
        if reduce_motion {
            self.engage_now(list, reduce_motion);
            return;
        }
        self.pinned = true;
        self.show_jump = false;
        list.set_follow_mode(FollowMode::Normal);
        let viewport = f32::from(list.viewport_bounds().size.height);
        let distance = self.distance_from_bottom(list);
        let glide_max = AGENT_GLIDE_MAX_VIEWPORTS * viewport;
        if viewport > 0.0 && distance > glide_max {
            list.scroll_by(px(distance - glide_max));
        }
        self.last_distance = self.distance_from_bottom(list);
        self.wake();
    }

    /// Wheel or drag input. This is the *only* path that can break the pin: the
    /// list calls its scroll handler from its input path alone, so content
    /// growth — which moves the distance to the end just as far — never reaches
    /// here. Reports whether the jump-button state changed.
    pub fn on_user_scroll(&mut self, list: &ListState, reduce_motion: bool) -> bool {
        let distance = self.distance_from_bottom(list);
        let previous = std::mem::replace(&mut self.last_distance, distance);
        if distance > previous + 1.0 && distance > AGENT_AT_BOTTOM_PX {
            self.release(list);
        } else if !self.pinned
            && (distance <= AGENT_AT_BOTTOM_PX || agent_should_restick(distance, previous))
        {
            self.engage(list, reduce_motion);
        }
        let show = distance > AGENT_JUMP_TO_BOTTOM_PX && !self.pinned;
        let changed = show != self.show_jump;
        self.show_jump = show;
        changed
    }

    /// Whether the driver should schedule a frame. False while one is already
    /// in flight, so the loop can never run more than one callback at a time.
    pub fn wants_frame(&self, list: &ListState) -> bool {
        self.pinned
            && !self.scheduled
            && (self.kick
                || self.settled_at.is_some()
                || !self.spring.is_idle()
                || self.distance_from_bottom(list) > 0.5)
    }

    /// Claim the one frame slot; pair with [`Self::step`], which releases it.
    pub fn arm(&mut self) {
        self.scheduled = true;
    }

    /// One spring frame, reporting whether the view needs another. Call it
    /// after layout, so the measurements it reads are the current frame's.
    pub fn step(&mut self, list: &ListState) -> bool {
        self.scheduled = false;
        self.kick = false;
        if !self.pinned {
            self.last_tick = None;
            return false;
        }
        let now = Instant::now();
        let frames = self
            .last_tick
            .map_or(1.0, |last| StickSpring::frames(now.duration_since(last)));
        self.last_tick = Some(now);
        let Some((target, mut distance)) = self.bottom(list) else {
            self.last_distance = 0.0;
            return false;
        };
        let viewport = f32::from(list.viewport_bounds().size.height);
        let glide_max = AGENT_GLIDE_MAX_VIEWPORTS * viewport;
        if viewport > 0.0 && distance > glide_max {
            list.scroll_by(px(distance - glide_max));
            distance = glide_max;
        }
        let position = target - distance;
        let next = self.spring.step(position, target, frames);
        if next > position {
            list.scroll_by(px(next - position));
        }
        self.last_distance = (target - next).max(0.0);
        if target - next <= 0.5 {
            let settled = *self.settled_at.get_or_insert(now);
            if now.duration_since(settled) >= AGENT_SPRING_SETTLE_GRACE && self.spring.is_idle() {
                self.spring.reset();
                self.last_tick = None;
                self.settled_at = None;
                return false;
            }
        } else {
            self.settled_at = None;
        }
        true
    }
}

#[derive(Clone, IntoElement)]
pub struct AgentTimeline {
    rows: Arc<Vec<TimelineRow>>,
    list_state: ListState,
    store: Entity<AgentTimelineStore>,
    active_turn: bool,
    bottom_padding: f32,
}

impl AgentTimeline {
    #[must_use]
    pub fn new(
        rows: Arc<Vec<TimelineRow>>,
        list_state: ListState,
        store: Entity<AgentTimelineStore>,
    ) -> Self {
        Self {
            rows,
            list_state,
            store,
            active_turn: false,
            bottom_padding: 4.0,
        }
    }

    #[must_use]
    pub fn active_turn(mut self, active_turn: bool) -> Self {
        self.active_turn = active_turn;
        self
    }

    #[must_use]
    pub fn bottom_padding(mut self, bottom_padding: f32) -> Self {
        self.bottom_padding = bottom_padding;
        self
    }
}

impl gpui::RenderOnce for AgentTimeline {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let rows = self.rows;
        let timeline_scroll = self.list_state.clone();
        let store = self.store;
        let copyable_assistant = if self.active_turn {
            None
        } else {
            final_assistant_entry_id(&rows)
        };
        let bottom_padding = self.bottom_padding;

        list(self.list_state, move |index, _window, cx| {
            let Some(row) = rows.get(index).cloned() else {
                return div().into_any_element();
            };
            div()
                .w_full()
                .px_3()
                .pb_3()
                .child(
                    div()
                        .w_full()
                        .max_w(px(AGENT_CONTENT_MAX_WIDTH))
                        .mx_auto()
                        .child(render_timeline_row(
                            &timeline_scroll,
                            &store,
                            row,
                            copyable_assistant,
                            cx,
                        )),
                )
                .into_any_element()
        })
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .size_full()
        .pt(px(AGENT_TIMELINE_TOP_PADDING))
        .pb(px(bottom_padding))
    }
}

fn final_assistant_entry_id(rows: &[TimelineRow]) -> Option<u64> {
    rows.iter().rev().find_map(|row| match row {
        TimelineRow::Single(AgentEntry::Assistant { id, .. }) => Some(*id),
        TimelineRow::Group { entries, .. } => entries.iter().rev().find_map(|entry| match entry {
            AgentEntry::Assistant { id, .. } => Some(*id),
            _ => None,
        }),
        TimelineRow::Single(_) => None,
    })
}

fn render_timeline_row(
    timeline_scroll: &ListState,
    store: &Entity<AgentTimelineStore>,
    row: TimelineRow,
    copyable_assistant: Option<u64>,
    cx: &mut App,
) -> AnyElement {
    match row {
        TimelineRow::Single(entry) => {
            render_entry(timeline_scroll, store, entry, copyable_assistant, cx)
        }
        TimelineRow::Group { kind, id, entries } => render_group(
            timeline_scroll,
            store,
            kind,
            id,
            &entries,
            copyable_assistant,
            cx,
        ),
    }
}

fn render_group(
    timeline_scroll: &ListState,
    store: &Entity<AgentTimelineStore>,
    group: TimelineGroupKind,
    id: u64,
    members: &[AgentEntry],
    copyable_assistant: Option<u64>,
    cx: &mut App,
) -> AnyElement {
    let expanded = store.update(cx, |store, _| {
        store.expanded(id, DisclosureKind::Group, false)
    });
    let toggle = store.clone();
    let (icon, label) = match group {
        TimelineGroupKind::Tool => (
            tool_icon(
                members
                    .first()
                    .and_then(tool_entry_kind)
                    .unwrap_or(AgentToolKind::Other),
            ),
            tool_group_label(members),
        ),
        TimelineGroupKind::Reasoning => (
            IconName::Cpu,
            SharedString::from(format!("Reasoning · {} steps", members.len())),
        ),
    };
    let icon_color = timeline_affordance_color(cx);

    v_flex()
        .id(("agent-timeline-group", id))
        .w_full()
        .child(
            h_flex()
                .id(("agent-timeline-group-toggle", id))
                .w_full()
                .h(px(28.0))
                .flex_none()
                .items_center()
                .gap_2()
                .py(px(3.0))
                .rounded(cx.theme().radius)
                .overflow_hidden()
                .cursor_pointer()
                .child(
                    h_flex()
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .gap_2()
                        .child(Icon::new(icon).small().flex_none().text_color(icon_color))
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(crate::rems_from_px(13.0))
                                .text_color(cx.theme().foreground.muted())
                                .child(label),
                        )
                        .child(
                            Icon::new(disclosure_icon(expanded))
                                .xsmall()
                                .flex_none()
                                .text_color(icon_color),
                        ),
                )
                .on_click(move |_, _, cx| {
                    toggle.update(cx, |store, cx| {
                        store.toggle_expanded(id, DisclosureKind::Group, false, cx);
                    });
                }),
        )
        .when(expanded, |this| {
            this.child(v_flex().w_full().children(
                members.iter().cloned().map(|entry| {
                    render_entry(timeline_scroll, store, entry, copyable_assistant, cx)
                }),
            ))
        })
        .into_any_element()
}

fn tool_entry_kind(entry: &AgentEntry) -> Option<AgentToolKind> {
    match entry {
        AgentEntry::Tool(tool) => Some(tool.kind),
        _ => None,
    }
}

fn tool_group_label(tools: &[AgentEntry]) -> SharedString {
    let mut actions = Vec::new();
    for kind in tools.iter().filter_map(tool_entry_kind) {
        let (singular, plural) = match kind {
            AgentToolKind::Read | AgentToolKind::Search => ("Read file", "Read files"),
            AgentToolKind::Edit => ("Edit file", "Edit files"),
            AgentToolKind::Execute => ("Ran command", "Ran commands"),
            AgentToolKind::Fetch => ("Fetched resource", "Fetched resources"),
            AgentToolKind::Think => ("Thought", "Thought"),
            AgentToolKind::Other => ("Used tool", "Used tools"),
        };
        if let Some((_, _, count)) = actions
            .iter_mut()
            .find(|(existing, _, _)| *existing == singular)
        {
            *count += 1;
        } else {
            actions.push((singular, plural, 1));
        }
    }

    if actions.is_empty() {
        "Used tools".into()
    } else {
        actions
            .into_iter()
            .map(|(singular, plural, count)| if count == 1 { singular } else { plural })
            .collect::<Vec<_>>()
            .join(", ")
            .into()
    }
}

/// One attachment as a `side`-square tile that opens a full view when clicked.
/// The size is definite whatever was pasted, so layout never consults the
/// image; the bytes are hash-keyed in gpui's asset cache, so decoding is shared.
pub fn agent_attachment_thumbnail(
    id: impl Into<ElementId>,
    image: Arc<Image>,
    side: Pixels,
    cx: &App,
) -> Stateful<Div> {
    let preview = Arc::clone(&image);
    div()
        .id(id)
        .size(side)
        .flex_none()
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.raised(2))
        .cursor_pointer()
        .child(
            img(ImageSource::Image(image))
                .size_full()
                .object_fit(ObjectFit::ScaleDown),
        )
        .on_click(move |_, window, cx| {
            open_attachment_preview(Arc::clone(&preview), window, cx);
        })
}

fn single_line(text: SharedString) -> SharedString {
    if !text.contains(['\n', '\r']) {
        return text;
    }
    text.split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
        .into()
}

fn render_entry(
    timeline_scroll: &ListState,
    store: &Entity<AgentTimelineStore>,
    entry: AgentEntry,
    copyable_assistant: Option<u64>,
    cx: &mut App,
) -> AnyElement {
    match entry {
        AgentEntry::User {
            id,
            markdown,
            images,
        } => v_flex()
            .id(("agent-user-entry", id))
            .w_full()
            .items_end()
            .child(
                v_flex()
                    .debug_selector(|| "agent-user-bubble".to_owned())
                    .max_w(relative(1.0))
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background.raised(1))
                    .text_size(crate::rems_from_px(13.0))
                    .when(!images.is_empty(), |this| {
                        this.child(h_flex().flex_wrap().gap_1().children(
                            images.iter().enumerate().map(|(index, image)| {
                                agent_attachment_thumbnail(
                                    (
                                        SharedString::from(format!("agent-user-attachment-{id}")),
                                        index,
                                    ),
                                    Arc::clone(image),
                                    TRANSCRIPT_ATTACHMENT,
                                    cx,
                                )
                                .debug_selector(|| "agent-user-attachment".to_owned())
                            }),
                        ))
                    })
                    .when(!markdown.is_empty(), |this| {
                        this.child(markdown_view(store, id, MarkdownSlot::Body, markdown, cx))
                    }),
            )
            .into_any_element(),
        AgentEntry::Assistant { id, markdown } => {
            let copy = markdown.clone();
            v_flex()
                .id(("agent-assistant-entry", id))
                .w_full()
                .gap_1()
                .text_size(crate::rems_from_px(13.0))
                .child(assistant_markdown_view(store, id, markdown, cx))
                .when(copyable_assistant == Some(id), |this| {
                    this.child(
                        h_flex().w_full().h(px(28.0)).items_center().child(
                            div()
                                .debug_selector(|| "agent-assistant-copy".to_owned())
                                .child(
                                    Button::new(("agent-copy-assistant", id))
                                        .ghost()
                                        .xsmall()
                                        .compact()
                                        .icon(IconName::Copy)
                                        .tooltip("Copy message")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.full_text(),
                                            ));
                                        }),
                                ),
                        ),
                    )
                })
                .into_any_element()
        }
        AgentEntry::Reasoning {
            id,
            label,
            markdown,
            default_expanded,
        } => {
            let expanded = store.update(cx, |store, _| {
                store.expanded(id, DisclosureKind::Reasoning, default_expanded)
            });
            let toggle = store.clone();
            v_flex()
                .id(("agent-reasoning-entry", id))
                .w_full()
                .gap_1()
                .child(
                    h_flex()
                        .id(("agent-reasoning-toggle", id))
                        .w_full()
                        .items_center()
                        .justify_between()
                        .py_1()
                        .cursor_pointer()
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_2()
                                .text_size(crate::rems_from_px(12.0))
                                .text_color(cx.theme().foreground.muted())
                                .child(
                                    Icon::new(IconName::Cpu)
                                        .small()
                                        .text_color(cx.theme().foreground.muted()),
                                )
                                .child(single_line(label)),
                        )
                        .child(
                            Icon::new(disclosure_icon(expanded))
                                .xsmall()
                                .text_color(cx.theme().foreground.muted()),
                        )
                        .on_click(move |_, _, cx| {
                            toggle.update(cx, |store, cx| {
                                store.toggle_expanded(
                                    id,
                                    DisclosureKind::Reasoning,
                                    default_expanded,
                                    cx,
                                );
                            });
                        }),
                )
                .when(expanded, |this| {
                    this.child(
                        div()
                            .ml_1()
                            .pl_4()
                            .py_1()
                            .border_l_1()
                            .border_color(cx.theme().border)
                            .text_size(crate::rems_from_px(12.0))
                            .text_color(cx.theme().foreground.muted())
                            .child(markdown_view(store, id, MarkdownSlot::Body, markdown, cx)),
                    )
                })
                .into_any_element()
        }
        AgentEntry::Plan { id, markdown } => v_flex()
            .id(("agent-plan-entry", id))
            .w_full()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .px_3()
            .py_2()
            .child(
                h_flex()
                    .gap_2()
                    .text_size(crate::rems_from_px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().foreground.muted())
                    .child(
                        Icon::new(IconName::CircleCheck)
                            .small()
                            .text_color(cx.theme().foreground.muted()),
                    )
                    .child("Plan"),
            )
            .child(
                div()
                    .text_size(crate::rems_from_px(12.0))
                    .child(markdown_view(store, id, MarkdownSlot::Body, markdown, cx)),
            )
            .into_any_element(),
        AgentEntry::Tool(tool) => render_tool_entry(timeline_scroll, store, tool, cx),
    }
}

const fn disclosure_icon(expanded: bool) -> IconName {
    if expanded {
        IconName::ChevronUp
    } else {
        IconName::ChevronRight
    }
}

fn render_tool_entry(
    timeline_scroll: &ListState,
    store: &Entity<AgentTimelineStore>,
    tool: AgentToolEntry,
    cx: &mut App,
) -> AnyElement {
    let AgentToolEntry {
        id,
        kind,
        status: _,
        label,
        location,
        input,
        output,
        default_expanded,
    } = tool;
    let expandable = location.is_some() || input.is_some() || !output.is_empty();
    let expanded = expandable
        && store.update(cx, |store, _| {
            store.expanded(id, DisclosureKind::Tool, default_expanded)
        });
    let content = expanded.then(|| {
        store.update(cx, |store, _| {
            store.tool_content(id, location, input, output)
        })
    });
    let toggle = store.clone();
    let icon_color = timeline_affordance_color(cx);

    v_flex()
        .id(("agent-tool-entry", id))
        .w_full()
        .child(
            h_flex()
                .id(("agent-tool-toggle", id))
                .w_full()
                .h(px(28.0))
                .flex_none()
                .items_center()
                .gap_2()
                .py(px(3.0))
                .rounded(cx.theme().radius)
                .overflow_hidden()
                .when(expandable, gpui::Styled::cursor_pointer)
                .child(
                    h_flex()
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .gap_2()
                        .child(
                            Icon::new(tool_icon(kind))
                                .small()
                                .flex_none()
                                .text_color(icon_color),
                        )
                        .child(
                            div()
                                .debug_selector(|| "agent-tool-label".to_owned())
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(crate::rems_from_px(13.0))
                                .text_color(cx.theme().foreground.muted())
                                .child(single_line(label)),
                        )
                        .when(expandable, |this| {
                            this.child(
                                div()
                                    .debug_selector(|| "agent-tool-chevron".to_owned())
                                    .flex_none()
                                    .child(
                                        Icon::new(disclosure_icon(expanded))
                                            .xsmall()
                                            .text_color(icon_color),
                                    ),
                            )
                        }),
                )
                .when(expandable, |this| {
                    this.on_click(move |_, _, cx| {
                        toggle.update(cx, |store, cx| {
                            store.toggle_expanded(id, DisclosureKind::Tool, default_expanded, cx);
                        });
                    })
                }),
        )
        .when_some(
            content.filter(|content| !content.rows.is_empty()),
            |this, content| {
                this.child(render_tool_content(
                    timeline_scroll,
                    store,
                    id,
                    &content,
                    cx,
                ))
            },
        )
        .into_any_element()
}

fn materialize_tool_content(source: ToolContentSource) -> ToolContentState {
    let mut rows = Vec::new();

    if let Some(location) = &source.location {
        rows.push(ToolContentRow::Section {
            label: "Location",
            copy: None,
        });
        rows.push(ToolContentRow::Path(location.clone()));
    }

    if let Some(input) = &source.input {
        if !rows.is_empty() {
            rows.push(ToolContentRow::Spacer);
        }
        let copy = Arc::<[AgentToolPayload]>::from([input.clone()]);
        rows.push(ToolContentRow::Section {
            label: "Input",
            copy: Some(copy),
        });
        let materialized = materialize_tool_payload(input);
        rows.extend(materialized.rows);
    }

    if !source.output.is_empty() {
        if !rows.is_empty() {
            rows.push(ToolContentRow::Spacer);
        }
        rows.push(ToolContentRow::Section {
            label: "Output",
            copy: Some(source.output.clone()),
        });
        for (index, payload) in source.output.iter().enumerate() {
            if index > 0 {
                rows.push(ToolContentRow::Spacer);
            }
            let materialized = materialize_tool_payload(payload);
            rows.extend(materialized.rows);
        }
    }

    ToolContentState {
        source,
        rows: rows.into(),
    }
}

struct MaterializedToolPayload {
    rows: Vec<ToolContentRow>,
}

fn materialize_tool_payload(payload: &AgentToolPayload) -> MaterializedToolPayload {
    match payload {
        AgentToolPayload::Diff { path, old, new } => {
            let old = old.as_ref().map(|old| old.0.read());
            let new = new.0.read();
            let (old, old_truncated) =
                bounded_tool_diff_prefix(old.as_ref().map_or("", |old| old.source.as_str()));
            let (new, new_truncated) = bounded_tool_diff_prefix(new.source.as_str());
            let diff = TextDiff::from_lines(old, new);
            let mut rows = Vec::new();
            let mut total_lines = 0;
            rows.push(ToolContentRow::Path(path.clone()));
            for change in diff.iter_all_changes() {
                let kind = match change.tag() {
                    ChangeTag::Equal => DiffLineKind::Equal,
                    ChangeTag::Insert => DiffLineKind::Added,
                    ChangeTag::Delete => DiffLineKind::Removed,
                };
                total_lines += 1;
                if total_lines <= TOOL_CONTENT_MAX_LINES {
                    rows.push(ToolContentRow::Diff {
                        kind,
                        text: SharedString::from(
                            change.value().trim_end_matches(['\r', '\n']).to_owned(),
                        ),
                    });
                }
            }
            append_line_truncation_footer(&mut rows, total_lines, old_truncated || new_truncated);
            MaterializedToolPayload { rows }
        }
        AgentToolPayload::Text(text) | AgentToolPayload::Json(text) => {
            let mut rows = Vec::new();
            let mut total_lines = 0;
            text.inspect(|text| {
                let (text, bytes_truncated) = bounded_tool_prefix(text);
                for line in text.split('\n').take(TOOL_CONTENT_MAX_LINES + 1) {
                    total_lines += 1;
                    if total_lines <= TOOL_CONTENT_MAX_LINES {
                        rows.push(ToolContentRow::Plain(
                            line.strip_suffix('\r').unwrap_or(line).to_owned().into(),
                        ));
                    }
                }
                append_line_truncation_footer(&mut rows, total_lines, bytes_truncated);
            });
            MaterializedToolPayload { rows }
        }
        AgentToolPayload::Terminal(text) => text.inspect(|text| {
            let (text, bytes_truncated) = bounded_tool_suffix(text);
            let mut lines = text
                .rsplit('\n')
                .take(TOOL_CONTENT_MAX_LINES + 1)
                .collect::<Vec<_>>();
            let lines_truncated = lines.len() > TOOL_CONTENT_MAX_LINES;
            if lines_truncated {
                lines.pop();
            }
            lines.reverse();
            let mut rows =
                Vec::with_capacity(lines.len() + usize::from(bytes_truncated || lines_truncated));
            if bytes_truncated || lines_truncated {
                rows.push(ToolContentRow::Footer(
                    "truncated: showing the latest output; copy to view it all".into(),
                ));
            }
            rows.extend(lines.iter().map(|line| {
                ToolContentRow::Plain(line.strip_suffix('\r').unwrap_or(line).to_string().into())
            }));
            MaterializedToolPayload { rows }
        }),
    }
}

fn bounded_tool_prefix(text: &str) -> (&str, bool) {
    if text.len() <= TOOL_CONTENT_MAX_BYTES {
        return (text, false);
    }
    let mut end = TOOL_CONTENT_MAX_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

fn bounded_tool_diff_prefix(text: &str) -> (&str, bool) {
    let (text, bytes_truncated) = bounded_tool_prefix(text);
    let Some((newline, _)) = text.match_indices('\n').nth(TOOL_CONTENT_MAX_LINES - 1) else {
        return (text, bytes_truncated);
    };
    let end = newline + 1;
    (&text[..end], bytes_truncated || end < text.len())
}

fn bounded_tool_suffix(text: &str) -> (&str, bool) {
    if text.len() <= TOOL_CONTENT_MAX_BYTES {
        return (text, false);
    }
    let mut start = text.len() - TOOL_CONTENT_MAX_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    (&text[start..], true)
}

fn append_line_truncation_footer(
    rows: &mut Vec<ToolContentRow>,
    total_lines: usize,
    bytes_truncated: bool,
) {
    if bytes_truncated {
        rows.push(ToolContentRow::Footer(
            "truncated: copy to view the full output".into(),
        ));
    } else if total_lines > TOOL_CONTENT_MAX_LINES {
        rows.push(ToolContentRow::Footer(
            format!("truncated: showing first {TOOL_CONTENT_MAX_LINES} of {total_lines} lines")
                .into(),
        ));
    }
}

fn render_tool_content(
    timeline_scroll: &ListState,
    store: &Entity<AgentTimelineStore>,
    id: u64,
    content: &ToolContentState,
    cx: &mut App,
) -> impl IntoElement {
    let scroll_handle = store.update(cx, |store, _| store.tool_scroll(id));
    let rows = content.rows.clone();
    let line_list = uniform_list(
        ("agent-tool-content-lines", id),
        rows.len(),
        move |range, _, cx| {
            range
                .filter_map(|index| {
                    rows.get(index)
                        .cloned()
                        .map(|row| render_tool_content_row(id, index, row, cx))
                })
                .collect::<Vec<_>>()
        },
    )
    .w_full()
    .max_h(px(TOOL_CONTENT_MAX_HEIGHT))
    .overflow_hidden()
    .with_sizing_behavior(ListSizingBehavior::Infer)
    .track_scroll(&scroll_handle);

    div()
        .w_full()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(tool_content_scroll_area(
            ("agent-tool-content-scroll", id),
            &scroll_handle,
            timeline_scroll,
            line_list,
        ))
}

fn tool_content_scroll_area(
    id: impl Into<ElementId>,
    scroll_handle: &UniformListScrollHandle,
    timeline_scroll: &ListState,
    content: impl IntoElement,
) -> impl IntoElement {
    let base_scroll_handle = scroll_handle.0.borrow().base_handle.clone();
    let wheel_scroll_handle = base_scroll_handle.clone();
    let scrollbar_handle = base_scroll_handle;
    let timeline_scroll = timeline_scroll.clone();
    let event_timeline_offset = Rc::new(Cell::new(None));
    let capture_timeline_offset = Rc::clone(&event_timeline_offset);
    let capture_timeline_scroll = timeline_scroll.clone();
    div()
        .id(id)
        .w_full()
        .min_h_0()
        .max_h(px(TOOL_CONTENT_MAX_HEIGHT))
        .relative()
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    let event_timeline_offset = Rc::clone(&capture_timeline_offset);
                    let timeline_scroll = capture_timeline_scroll.clone();
                    window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, _| {
                        if phase == DispatchPhase::Capture && bounds.contains(&event.position) {
                            event_timeline_offset
                                .set(Some(timeline_scroll.scroll_px_offset_for_scrollbar()));
                        }
                    });
                },
            )
            .absolute()
            .inset_0(),
        )
        .on_scroll_wheel(move |event, window, cx| {
            let live_timeline_offset = timeline_scroll.scroll_px_offset_for_scrollbar();
            let timeline_offset = event_timeline_offset.take().unwrap_or(live_timeline_offset);
            let current = wheel_scroll_handle.offset();
            let delta = event.delta.pixel_delta(window.line_height());
            let minimum_y = -wheel_scroll_handle.max_offset().y;
            let next_y = (current.y + delta.y).clamp(minimum_y, px(0.0));
            if next_y == current.y {
                return;
            }

            wheel_scroll_handle.set_offset(gpui::point(current.x, next_y));
            timeline_scroll.set_offset_from_scrollbar(timeline_offset);
            window.refresh();
            cx.stop_propagation();
        })
        .child(content)
        .vertical_scrollbar(&scrollbar_handle)
}

fn render_tool_content_row(id: u64, index: usize, row: ToolContentRow, cx: &App) -> AnyElement {
    let base = h_flex()
        .w_full()
        .min_w_0()
        .h(px(TOOL_CONTENT_ROW_HEIGHT))
        .px_2()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(crate::rems_from_px(11.0))
        .overflow_hidden();

    match row {
        ToolContentRow::Section { label, copy } => base
            .justify_between()
            .items_center()
            .font_family(cx.theme().font_family.clone())
            .text_size(crate::rems_from_px(10.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(cx.theme().foreground.muted())
            .child(label)
            .when_some(copy, |this, copy| {
                let hover_background = cx.theme().background.hover();
                this.child(
                    div()
                        .id(format!("agent-tool-copy-{id}-{index}"))
                        .px_1()
                        .rounded(cx.theme().radius)
                        .cursor_pointer()
                        .text_color(cx.theme().foreground)
                        .hover(move |style| style.bg(hover_background))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                tool_payload_copy_text(&copy),
                            ));
                        })
                        .child("Copy"),
                )
            })
            .into_any_element(),
        ToolContentRow::Path(path) => base
            .items_center()
            .bg(cx.theme().background.raised(2).wash())
            .font_weight(FontWeight::MEDIUM)
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(single_line(path)),
            )
            .into_any_element(),
        ToolContentRow::Plain(text) => render_machine_line(base, "", text, None, None),
        ToolContentRow::Diff { kind, text } => {
            let (gutter, foreground, background) = match kind {
                DiffLineKind::Equal => (" ", None, None),
                DiffLineKind::Added => (
                    "+",
                    Some(cx.theme().success),
                    Some(cx.theme().success.fill()),
                ),
                DiffLineKind::Removed => {
                    ("−", Some(cx.theme().danger), Some(cx.theme().danger.fill()))
                }
            };
            render_machine_line(base, gutter, text, foreground, background)
        }
        ToolContentRow::Footer(note) => base
            .items_center()
            .text_size(crate::rems_from_px(10.0))
            .text_color(cx.theme().foreground.muted())
            .child(note)
            .into_any_element(),
        ToolContentRow::Spacer => base.into_any_element(),
    }
}

fn render_machine_line(
    row: gpui::Div,
    gutter: &'static str,
    text: SharedString,
    gutter_color: Option<Hsla>,
    background: Option<Hsla>,
) -> AnyElement {
    row.when_some(background, gpui::Styled::bg)
        .child(
            div()
                .w(px(18.0))
                .flex_none()
                .text_color(gutter_color.unwrap_or_default())
                .child(gutter),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(text),
        )
        .into_any_element()
}

fn tool_payload_copy_text(payloads: &[AgentToolPayload]) -> String {
    let mut copied = String::new();
    for (index, payload) in payloads.iter().enumerate() {
        if index > 0 {
            copied.push_str("\n\n");
        }
        match payload {
            AgentToolPayload::Text(text)
            | AgentToolPayload::Json(text)
            | AgentToolPayload::Terminal(text) => text.inspect(|text| copied.push_str(text)),
            AgentToolPayload::Diff { path, old, new } => {
                copied.push_str("Path: ");
                copied.push_str(path);
                copied.push_str("\n\nOld:\n");
                if let Some(old) = old {
                    old.inspect(|old| copied.push_str(old));
                } else {
                    copied.push_str("<new file>");
                }
                copied.push_str("\n\nNew:\n");
                new.inspect(|new| copied.push_str(new));
            }
        }
    }
    copied
}

fn tool_icon(kind: AgentToolKind) -> IconName {
    match kind {
        AgentToolKind::Read | AgentToolKind::Edit => IconName::File,
        AgentToolKind::Search => IconName::Search,
        AgentToolKind::Execute => IconName::SquareTerminal,
        AgentToolKind::Fetch => IconName::Globe,
        AgentToolKind::Think => IconName::Cpu,
        AgentToolKind::Other => IconName::Asterisk,
    }
}

fn timeline_affordance_color(cx: &App) -> Hsla {
    cx.theme().foreground.muted()
}

fn markdown_view(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    slot: MarkdownSlot,
    markdown: AgentMarkdown,
    cx: &mut App,
) -> AgentMarkdownView {
    markdown_view_with_extensions(store, id, slot, markdown, false, cx)
}

fn assistant_markdown_view(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    markdown: AgentMarkdown,
    cx: &mut App,
) -> AgentMarkdownView {
    markdown_view_with_extensions(store, id, MarkdownSlot::Body, markdown, true, cx)
}

fn markdown_view_with_extensions(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    slot: MarkdownSlot,
    markdown: AgentMarkdown,
    assistant: bool,
    cx: &mut App,
) -> AgentMarkdownView {
    let truncated = markdown.is_truncated();
    let full_source = markdown.clone();
    let style = TextViewStyle {
        highlight_theme: Arc::clone(&cx.theme().highlight_theme),
        is_dark: cx.theme().is_dark(),
        ..TextViewStyle::default()
    };

    let (state, extensions, streaming) = store.update(cx, |store, cx| {
        (
            store.markdown(id, slot, markdown, cx),
            store.markdown_extensions_for(assistant),
            store.streaming == Some(id),
        )
    });
    AgentMarkdownView {
        state,
        extensions,
        style,
        streaming,
        full_source: (!assistant).then_some(full_source),
        truncated,
    }
}

fn resolve_workspace_link(cwd: Option<&Path>, url: &str) -> Option<String> {
    if url == PENDING_LINK_URL {
        return Some(INERT_LINK_URL.to_owned());
    }
    if url.is_empty() || url.starts_with('#') || url.starts_with("//") {
        return None;
    }
    let link = Path::new(url);
    let path = if link.is_absolute() {
        link.to_owned()
    } else {
        let scheme_like = url.split_once(':').is_some_and(|(scheme, _)| {
            scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        });
        if scheme_like {
            return None;
        }
        cwd?.join(link)
    };
    file_url(&path)
}

#[cfg(not(target_family = "wasm"))]
fn file_url(path: &Path) -> Option<String> {
    url::Url::from_file_path(path).ok().map(Into::into)
}

#[cfg(target_family = "wasm")]
fn file_url(_path: &Path) -> Option<String> {
    None
}

#[derive(IntoElement)]
struct AgentMarkdownView {
    state: Entity<TextViewState>,
    extensions: MarkdownExtensions,
    style: TextViewStyle,
    streaming: bool,
    full_source: Option<AgentMarkdown>,
    truncated: bool,
}

impl gpui::RenderOnce for AgentMarkdownView {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let text = TextView::new(&self.state)
            .style(self.style)
            .max_w_full()
            .min_w_0()
            .selectable(true)
            .streaming(self.streaming)
            .code_block_actions(agent_code_block_chrome)
            .markdown_extensions(self.extensions)
            .when(self.truncated, |text| {
                text.scrollable(true).h(px(MARKDOWN_PREVIEW_HEIGHT))
            });
        let state_id = self.state.entity_id();
        v_flex().w_full().min_w_0().gap_2().child(text).when_some(
            self.truncated.then_some(self.full_source).flatten(),
            move |this, source| {
                this.child(
                    h_flex().w_full().justify_end().child(
                        Button::new(("agent-copy-full-markdown", state_id))
                            .secondary()
                            .small()
                            .icon(IconName::Copy)
                            .label("Copy full message")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    source.full_text(),
                                ));
                            }),
                    ),
                )
            },
        )
    }
}

fn agent_code_block_chrome(
    code_block: &CodeBlock,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let language = code_block_language(code_block.lang());
    let code = code_block.code();
    let source_offset = code_block.span.map_or(0, |span| span.start);

    h_flex()
        .w_full()
        .h(px(28.0))
        .items_center()
        .justify_between()
        .pl_1()
        .child(
            div()
                .debug_selector(|| "agent-code-language".to_owned())
                .text_size(crate::rems_from_px(10.0))
                .text_color(cx.theme().foreground.muted())
                .child(language),
        )
        .child(
            div().debug_selector(|| "agent-code-copy".to_owned()).child(
                Button::new(("agent-copy-code", source_offset))
                    .ghost()
                    .xsmall()
                    .compact()
                    .icon(IconName::Copy)
                    .tooltip("Copy code")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                    }),
            ),
        )
        .into_any_element()
}

fn code_block_language(language: Option<SharedString>) -> SharedString {
    language
        .filter(|language| !language.trim().is_empty())
        .unwrap_or_else(|| SharedString::from("text"))
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct InlineCodeTextStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    code: bool,
    link: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InlineCodeTextSpan {
    range: Range<usize>,
    style: InlineCodeTextStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InlineCodeParagraph {
    text: String,
    spans: Vec<InlineCodeTextSpan>,
    source_offset: usize,
}

#[derive(Default)]
struct InlineCodeParagraphBuilder {
    text: String,
    spans: Vec<InlineCodeTextSpan>,
    has_code: bool,
}

impl InlineCodeParagraphBuilder {
    fn push(&mut self, text: &str, style: InlineCodeTextStyle) {
        if text.is_empty() {
            return;
        }

        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if let Some(previous) = self.spans.last_mut()
            && previous.range.end == start
            && previous.style == style
        {
            previous.range.end = end;
        } else {
            self.spans.push(InlineCodeTextSpan {
                range: start..end,
                style,
            });
        }
    }
}

fn inline_code_paragraph(
    node: &markdown_ast::Node,
    document_offset: usize,
) -> Option<InlineCodeParagraph> {
    let markdown_ast::Node::Paragraph(paragraph) = node else {
        return None;
    };
    let mut builder = InlineCodeParagraphBuilder::default();
    let style = InlineCodeTextStyle::default();
    if !collect_inline_code_text(&paragraph.children, &style, &mut builder) || !builder.has_code {
        return None;
    }

    Some(InlineCodeParagraph {
        text: builder.text,
        spans: builder.spans,
        source_offset: document_offset
            + paragraph
                .position
                .as_ref()
                .map_or(0, |position| position.start.offset),
    })
}

fn collect_inline_code_text(
    nodes: &[markdown_ast::Node],
    style: &InlineCodeTextStyle,
    builder: &mut InlineCodeParagraphBuilder,
) -> bool {
    nodes
        .iter()
        .all(|node| collect_inline_code_node(node, style, builder))
}

fn collect_inline_code_node(
    node: &markdown_ast::Node,
    style: &InlineCodeTextStyle,
    builder: &mut InlineCodeParagraphBuilder,
) -> bool {
    match node {
        markdown_ast::Node::Text(text) => {
            builder.push(&text.value, style.clone());
            true
        }
        markdown_ast::Node::Break(_) => {
            builder.push("\n", style.clone());
            true
        }
        markdown_ast::Node::InlineCode(code) => {
            let mut style = style.clone();
            style.code = true;
            builder.has_code = true;
            builder.push(&code.value, style);
            true
        }
        markdown_ast::Node::InlineMath(math) => {
            let mut style = style.clone();
            style.code = true;
            builder.has_code = true;
            builder.push(&math.value, style);
            true
        }
        markdown_ast::Node::Strong(strong) => {
            let mut style = style.clone();
            style.bold = true;
            collect_inline_code_text(&strong.children, &style, builder)
        }
        markdown_ast::Node::Emphasis(emphasis) => {
            let mut style = style.clone();
            style.italic = true;
            collect_inline_code_text(&emphasis.children, &style, builder)
        }
        markdown_ast::Node::Delete(delete) => {
            let mut style = style.clone();
            style.strikethrough = true;
            collect_inline_code_text(&delete.children, &style, builder)
        }
        markdown_ast::Node::Link(link) => {
            let mut style = style.clone();
            style.link = Some(link.url.clone().into());
            collect_inline_code_text(&link.children, &style, builder)
        }
        markdown_ast::Node::MdxJsxTextElement(element) => {
            collect_inline_code_text(&element.children, style, builder)
        }
        markdown_ast::Node::MdxTextExpression(expression) => {
            builder.push(&expression.value, style.clone());
            true
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct InlineCodeParagraphPlugin;

impl MarkdownPlugin for InlineCodeParagraphPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        INLINE_CODE_PARAGRAPH_NODE_NAME
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let paragraph = inline_code_paragraph(node, cx.offset())?;
        Some(
            MarkdownNode::new(INLINE_CODE_PARAGRAPH_NODE_NAME, paragraph.clone())
                .text(paragraph.text.clone())
                .markdown(cx.node_source(node).unwrap_or(&paragraph.text)),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let paragraph = node
            .data::<InlineCodeParagraph>()
            .expect("inline code paragraph data");
        let mut highlights = Vec::with_capacity(paragraph.spans.len());
        let mut font_overrides = Vec::new();
        let mut link_ranges = Vec::new();
        let mut link_urls = Vec::new();

        for span in &paragraph.spans {
            let mut highlight = HighlightStyle::default();
            if span.style.bold {
                highlight.font_weight = Some(FontWeight::BOLD);
            }
            if span.style.italic {
                highlight.font_style = Some(FontStyle::Italic);
            }
            if span.style.strikethrough {
                highlight.strikethrough = Some(gpui::StrikethroughStyle {
                    thickness: px(1.0),
                    ..Default::default()
                });
            }
            if span.style.code {
                highlight.background_color = Some(cx.theme().background.raised(2));
                font_overrides.push((span.range.clone(), cx.theme().mono_font_family.clone()));
            }
            if let Some(url) = &span.style.link {
                highlight.color = Some(cx.theme().foreground);
                highlight.underline = Some(gpui::UnderlineStyle {
                    thickness: px(1.0),
                    ..Default::default()
                });
                link_ranges.push(span.range.clone());
                link_urls.push(url.clone());
            }
            highlights.push((span.range.clone(), highlight));
        }

        let styled_text = StyledText::new(paragraph.text.clone())
            .with_highlights(highlights)
            .with_font_family_overrides(font_overrides);
        let interactive_text = InteractiveText::new(
            SharedString::from(format!(
                "agent-inline-code-paragraph-{}",
                paragraph.source_offset
            )),
            styled_text,
        );
        let interactive_text = if link_ranges.is_empty() {
            interactive_text
        } else {
            interactive_text.on_click(link_ranges, move |index, window, cx| {
                let Some(url) = link_urls.get(index) else {
                    return;
                };
                window.end_text_selection(cx);
                cx.stop_propagation();
                cx.open_url(url.as_ref());
            })
        };

        div().w_full().min_w_0().child(interactive_text)
    }
}

fn standard_markdown_extensions() -> MarkdownExtensions {
    static EXTENSIONS: OnceLock<MarkdownExtensions> = OnceLock::new();
    EXTENSIONS
        .get_or_init(|| {
            MarkdownExtensions::default()
                .plugin(InlineCodeParagraphPlugin)
                .plugin(MermaidPlugin)
        })
        .clone()
}

fn assistant_markdown_extensions() -> MarkdownExtensions {
    static EXTENSIONS: OnceLock<MarkdownExtensions> = OnceLock::new();
    EXTENSIONS
        .get_or_init(|| {
            MarkdownExtensions::default()
                .plugin(InlineCodeParagraphPlugin)
                .plugin(MermaidPlugin)
                .plugin(RichMarkdownPlugin)
        })
        .clone()
}

#[derive(Clone)]
struct RichMarkdownSource {
    source: String,
    source_offset: usize,
}

#[derive(Clone, Copy)]
struct RichMarkdownPlugin;

impl MarkdownPlugin for RichMarkdownPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        RICH_MARKDOWN_NODE_NAME
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Code(code) = node else {
            return None;
        };
        if !code.lang.as_deref().is_some_and(is_markdown_language) {
            return None;
        }
        if code.value.len() > MAX_STREAMING_MEND_BYTES {
            return None;
        }
        let source_offset = cx.offset()
            + code
                .position
                .as_ref()
                .map_or(0, |position| position.start.offset);
        Some(
            MarkdownNode::new(
                RICH_MARKDOWN_NODE_NAME,
                RichMarkdownSource {
                    source: code.value.clone(),
                    source_offset,
                },
            )
            .text(code.value.clone())
            .markdown(cx.node_source(node).unwrap_or(&code.value)),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let source = node
            .data::<RichMarkdownSource>()
            .expect("rich markdown node data");
        let key = SharedString::from(format!("zz-rich-markdown/{}", source.source_offset));
        let initial_source = source.source.clone();
        let retained = window.use_keyed_state(key, cx, move |_, cx| {
            cx.new(|cx| TextViewState::markdown(&initial_source, cx))
        });
        let state = retained.read(cx).clone();
        state.update(cx, |state, cx| {
            state.synchronize_markdown(&source.source, false, cx);
        });
        let style = TextViewStyle {
            highlight_theme: Arc::clone(&cx.theme().highlight_theme),
            is_dark: cx.theme().is_dark(),
            ..TextViewStyle::default()
        };

        div().w_full().min_w_0().child(AgentMarkdownView {
            state,
            extensions: standard_markdown_extensions(),
            style,
            streaming: false,
            full_source: None,
            truncated: false,
        })
    }
}

fn is_markdown_language(language: &str) -> bool {
    language.eq_ignore_ascii_case("markdown") || language.eq_ignore_ascii_case("md")
}

#[derive(Clone)]
struct MermaidSource {
    source: String,
    source_offset: usize,
    scale: f32,
}

#[derive(Clone, Copy)]
struct MermaidPlugin;

impl MarkdownPlugin for MermaidPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        MERMAID_NODE_NAME
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Code(code) = node else {
            return None;
        };
        if !code
            .lang
            .as_deref()
            .is_some_and(|language| language.eq_ignore_ascii_case("mermaid"))
            || code.value.len() > MERMAID_MAX_SOURCE_BYTES
        {
            return None;
        }
        let scale = code
            .meta
            .as_deref()
            .and_then(|meta| meta.split_whitespace().next())
            .and_then(|scale| scale.parse::<f32>().ok())
            .unwrap_or(100.0)
            .clamp(25.0, 200.0);
        let source_offset = cx.offset()
            + code
                .position
                .as_ref()
                .map_or(0, |position| position.start.offset);
        Some(
            MarkdownNode::new(
                MERMAID_NODE_NAME,
                MermaidSource {
                    source: code.value.clone(),
                    source_offset,
                    scale,
                },
            )
            .text(code.value.clone())
            .markdown(cx.node_source(node).unwrap_or(&code.value)),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let source = node
            .data::<MermaidSource>()
            .expect("mermaid markdown node data");
        let theme_key = mermaid_theme_key(cx);
        let mut hasher = DefaultHasher::new();
        source.source.hash(&mut hasher);
        source.scale.to_bits().hash(&mut hasher);
        theme_key.hash(&mut hasher);
        let render_key = hasher.finish();
        let key = SharedString::from(format!(
            "zz-mermaid/{}/{theme_key}/{}",
            source.source_offset,
            source.scale.to_bits()
        ));
        let render_state = window.use_keyed_state(key, cx, {
            let source = source.clone();
            move |_, cx| MermaidRenderState::new(source, render_key, cx)
        });
        render_state.update(cx, |state, cx| {
            state.synchronize(source.clone(), render_key, cx);
        });

        let content = match &render_state.read(cx).result {
            MermaidRenderResult::Pending => h_flex()
                .w_full()
                .min_h(px(120.0))
                .items_center()
                .justify_center()
                .gap_2()
                .text_size(crate::rems_from_px(11.0))
                .text_color(cx.theme().foreground.muted())
                .child(
                    Icon::new(IconName::Loader)
                        .small()
                        .text_color(cx.theme().foreground.muted()),
                )
                .child("Rendering Mermaid…")
                .into_any_element(),
            MermaidRenderResult::Ready(image) => div()
                .id(("agent-mermaid", source.source_offset))
                .relative()
                .w_full()
                .min_w_0()
                .max_h(px(MERMAID_MAX_HEIGHT))
                .overflow_hidden()
                .child(
                    div().flex().w_full().justify_center().child(
                        img(ImageSource::Render(image.clone()))
                            .max_w_full()
                            .max_h(px(MERMAID_MAX_HEIGHT))
                            .mx_auto(),
                    ),
                )
                .child(
                    div().absolute().top_2().right_2().child(
                        Button::new(("agent-mermaid-preview", source.source_offset))
                            .secondary()
                            .xsmall()
                            .compact()
                            .icon(IconName::WindowMaximize)
                            .label("Full diagram")
                            .tooltip("Open full diagram")
                            .on_click({
                                let image = Arc::clone(image);
                                move |_, window, cx| {
                                    open_render_image_preview(Arc::clone(&image), window, cx);
                                }
                            }),
                    ),
                )
                .into_any_element(),
            MermaidRenderResult::Failed(error) => v_flex()
                .w_full()
                .gap_2()
                .text_size(crate::rems_from_px(11.0))
                .child(
                    h_flex()
                        .gap_2()
                        .text_color(cx.theme().danger)
                        .child(Icon::new(IconName::TriangleAlert).small())
                        .child("Mermaid could not be rendered"),
                )
                .child(
                    div()
                        .text_color(cx.theme().foreground.muted())
                        .child(error.clone()),
                )
                .into_any_element(),
        };

        div()
            .w_full()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .overflow_hidden()
            .child(content)
    }
}

#[derive(Clone)]
struct MermaidTheme {
    background: String,
    svg_css: String,
    config: merman::MermaidConfig,
}

impl MermaidTheme {
    fn from_app(cx: &App) -> Self {
        Self::new(
            MermaidThemeColors {
                background: cx.theme().background,
                surface: cx.theme().background.raised(1),
                muted: cx.theme().background.hover(),
                foreground: cx.theme().foreground,
                muted_foreground: cx.theme().foreground.muted(),
                border: cx.theme().border,
                primary: cx.theme().foreground,
                primary_foreground: cx.theme().foreground.on(),
                success: cx.theme().success,
                success_foreground: cx.theme().success.on(),
                warning: cx.theme().warning,
                warning_foreground: cx.theme().warning.on(),
                danger: cx.theme().danger,
                danger_foreground: cx.theme().danger.on(),
            },
            cx.theme().font_family.as_ref(),
            cx.theme().is_dark(),
        )
    }

    fn new(colors: MermaidThemeColors, font_family: &str, dark: bool) -> Self {
        let background = css_color(colors.background);
        let surface = css_color(colors.surface);
        let muted = css_color(colors.muted);
        let foreground = css_color(colors.foreground);
        let muted_foreground = css_color(colors.muted_foreground);
        let border = css_color(colors.border);
        let primary = css_color(colors.primary);
        let primary_foreground = css_color(colors.primary_foreground);
        let success = css_color(colors.success);
        let success_foreground = css_color(colors.success_foreground);
        let warning = css_color(colors.warning);
        let warning_foreground = css_color(colors.warning_foreground);
        let danger = css_color(colors.danger);
        let danger_foreground = css_color(colors.danger_foreground);
        let font_family = mermaid_font_family(font_family);
        let mut theme_variables: serde_json::Map<String, serde_json::Value> = [
            ("background", background.as_str()),
            ("primaryColor", surface.as_str()),
            ("primaryTextColor", foreground.as_str()),
            ("primaryBorderColor", border.as_str()),
            ("secondaryColor", muted.as_str()),
            ("secondaryTextColor", foreground.as_str()),
            ("tertiaryColor", background.as_str()),
            ("tertiaryTextColor", foreground.as_str()),
            ("mainBkg", surface.as_str()),
            ("nodeBorder", border.as_str()),
            ("nodeTextColor", foreground.as_str()),
            ("lineColor", primary.as_str()),
            ("textColor", foreground.as_str()),
            ("titleColor", foreground.as_str()),
            ("edgeLabelBackground", background.as_str()),
            ("clusterBkg", muted.as_str()),
            ("clusterBorder", border.as_str()),
            ("noteBkgColor", surface.as_str()),
            ("noteBorderColor", border.as_str()),
            ("noteTextColor", foreground.as_str()),
            ("actorBkg", surface.as_str()),
            ("actorBorder", border.as_str()),
            ("actorTextColor", foreground.as_str()),
            ("activationBkgColor", muted.as_str()),
            ("activationBorderColor", border.as_str()),
            ("labelTextColor", foreground.as_str()),
            ("loopTextColor", foreground.as_str()),
            ("signalColor", foreground.as_str()),
            ("signalTextColor", foreground.as_str()),
            ("classText", foreground.as_str()),
            ("labelColor", foreground.as_str()),
            ("attributeBackgroundColorOdd", surface.as_str()),
            ("attributeBackgroundColorEven", muted.as_str()),
            ("fontFamily", font_family.as_str()),
            ("fontSize", "13px"),
            ("pieTitleTextColor", foreground.as_str()),
            ("pieSectionTextColor", foreground.as_str()),
            ("pieLegendTextColor", foreground.as_str()),
            ("pieStrokeColor", border.as_str()),
            ("pieOuterStrokeColor", border.as_str()),
            ("pie1", primary.as_str()),
            ("pie2", success.as_str()),
            ("pie3", warning.as_str()),
            ("pie4", danger.as_str()),
            ("pie5", muted_foreground.as_str()),
            ("git0", primary.as_str()),
            ("git1", success.as_str()),
            ("git2", warning.as_str()),
            ("git3", danger.as_str()),
            ("gitBranchLabel0", primary_foreground.as_str()),
            ("commitLabelColor", foreground.as_str()),
            ("commitLabelBackground", muted.as_str()),
            ("tagLabelColor", foreground.as_str()),
            ("tagLabelBackground", surface.as_str()),
            ("tagLabelBorder", border.as_str()),
            ("quadrant1Fill", surface.as_str()),
            ("quadrant2Fill", muted.as_str()),
            ("quadrant3Fill", surface.as_str()),
            ("quadrant4Fill", muted.as_str()),
            ("quadrant1TextFill", foreground.as_str()),
            ("quadrant2TextFill", foreground.as_str()),
            ("quadrant3TextFill", foreground.as_str()),
            ("quadrant4TextFill", foreground.as_str()),
            ("quadrantPointFill", primary.as_str()),
            ("quadrantPointTextFill", foreground.as_str()),
            ("quadrantTitleFill", foreground.as_str()),
            ("quadrantXAxisTextFill", foreground.as_str()),
            ("quadrantYAxisTextFill", foreground.as_str()),
            ("quadrantExternalBorderStrokeFill", border.as_str()),
            ("quadrantInternalBorderStrokeFill", border.as_str()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.into()))
        .collect();
        theme_variables.insert(
            "xyChart".to_owned(),
            serde_json::json!({
                "backgroundColor": &background,
                "titleColor": &foreground,
                "xAxisTitleColor": &foreground,
                "xAxisLabelColor": &foreground,
                "xAxisTickColor": &border,
                "xAxisLineColor": &border,
                "yAxisTitleColor": &foreground,
                "yAxisLabelColor": &foreground,
                "yAxisTickColor": &border,
                "yAxisLineColor": &border,
                "plotColorPalette": format!(
                    "{primary},{success},{warning},{danger},{muted_foreground}"
                )
            }),
        );
        let config = merman::MermaidConfig::from_value(serde_json::json!({
            "theme": "base",
            "darkMode": dark,
            "fontFamily": &font_family,
            "fontSize": 13,
            "htmlLabels": false,
            "flowchart": {
                "htmlLabels": false,
                "padding": 16
            },
            "gantt": {
                "useWidth": 640,
                "fontSize": 13,
                "sectionFontSize": 13,
                "barHeight": 24,
                "barGap": 6
            },
            "xyChart": {
                "width": 640,
                "height": 420
            },
            "themeVariables": theme_variables
        }));
        let svg_css = mermaid_svg_theme_css(MermaidCssPalette {
            font_family: &font_family,
            background: &background,
            surface: &surface,
            muted: &muted,
            foreground: &foreground,
            muted_foreground: &muted_foreground,
            border: &border,
            primary: &primary,
            primary_foreground: &primary_foreground,
            success: &success,
            success_foreground: &success_foreground,
            warning: &warning,
            warning_foreground: &warning_foreground,
            danger: &danger,
            danger_foreground: &danger_foreground,
        });
        Self {
            background,
            svg_css,
            config,
        }
    }
}

#[derive(Clone, Copy)]
struct MermaidThemeColors {
    background: Hsla,
    surface: Hsla,
    muted: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    border: Hsla,
    primary: Hsla,
    primary_foreground: Hsla,
    success: Hsla,
    success_foreground: Hsla,
    warning: Hsla,
    warning_foreground: Hsla,
    danger: Hsla,
    danger_foreground: Hsla,
}

fn mermaid_font_family(font_family: &str) -> String {
    let mapped = gpui::font_name_with_fallbacks(font_family, "system-ui");
    let sanitized = mapped
        .chars()
        .filter(|character| !matches!(character, ';' | '{' | '}'))
        .collect::<String>();
    let sanitized = if sanitized.trim().is_empty() {
        "system-ui"
    } else {
        sanitized.trim()
    };
    if sanitized
        .split(',')
        .any(|family| family.trim().eq_ignore_ascii_case("sans-serif"))
    {
        sanitized.to_owned()
    } else {
        format!("{sanitized}, sans-serif")
    }
}

#[derive(Clone, Copy)]
struct MermaidCssPalette<'a> {
    font_family: &'a str,
    background: &'a str,
    surface: &'a str,
    muted: &'a str,
    foreground: &'a str,
    muted_foreground: &'a str,
    border: &'a str,
    primary: &'a str,
    primary_foreground: &'a str,
    success: &'a str,
    success_foreground: &'a str,
    warning: &'a str,
    warning_foreground: &'a str,
    danger: &'a str,
    danger_foreground: &'a str,
}

fn mermaid_svg_theme_css(palette: MermaidCssPalette<'_>) -> String {
    let MermaidCssPalette {
        font_family,
        background,
        surface,
        muted,
        foreground,
        muted_foreground,
        border,
        primary,
        primary_foreground,
        success,
        success_foreground,
        warning,
        warning_foreground,
        danger,
        danger_foreground,
    } = palette;
    format!(
        r"
svg {{ background-color: {background} !important; }}
text, tspan {{ font-family: {font_family} !important; fill: {foreground} !important; }}
.background {{ fill: {background} !important; }}
marker path {{ fill: {primary} !important; stroke: {primary} !important; }}

.actor {{ fill: {surface} !important; stroke: {border} !important; }}
.actor-line, .messageLine0, .messageLine1 {{ stroke: {primary} !important; }}
.messageText, text.actor, text.actor tspan {{ fill: {foreground} !important; }}

.statediagram-state .label-container path, .statediagram-state .label-container rect,
.statediagram-state .label-container polygon {{ fill: {surface} !important; stroke: {border} !important; }}
.statediagram-state .nodeLabel {{ fill: {foreground} !important; }}
.transition {{ stroke: {primary} !important; }}
.state-start {{ fill: {primary} !important; stroke: {primary} !important; }}

.mindmap-node .label-container, .mindmap-node .node-bkg {{ fill: {surface} !important; stroke: {primary} !important; }}
.mindmap-node text, .mindmap-node tspan {{ fill: {foreground} !important; }}
.mindmapDiagram .edge {{ stroke: {primary} !important; }}

.timeline-node .node-bkg {{ fill: {surface} !important; stroke: {border} !important; }}
.timeline-node text, .timeline-node tspan {{ fill: {foreground} !important; }}
.timelineDiagram .lineWrapper line {{ stroke: {primary} !important; }}

.entityBox {{ fill: {surface} !important; stroke: {border} !important; }}
.node .row-rect-odd path {{ fill: {surface} !important; }}
.node .row-rect-even path {{ fill: {muted} !important; }}
.entityLabel, .entityLabel text, .entityLabel tspan, .erDiagramTitleText {{ fill: {foreground} !important; }}
.relationshipLabelBox {{ fill: {background} !important; opacity: 1 !important; }}
.relationshipLine {{ stroke: {primary} !important; }}
.marker.er {{ fill: none !important; stroke: {primary} !important; }}
.edgeLabel .label text, .edgeLabel .label tspan {{ fill: {foreground} !important; }}

.pieTitleText, .legend text {{ fill: {foreground} !important; }}
.slice {{ fill: {foreground} !important; }}
.pieCircle, .pieOuterCircle {{ stroke: {border} !important; }}

.titleText, .sectionTitle0, .sectionTitle1, .sectionTitle2, .sectionTitle3,
.grid .tick text, .taskTextOutside0, .taskTextOutside1, .taskTextOutside2,
.taskTextOutside3, .taskTextOutsideLeft, .taskTextOutsideRight {{ fill: {foreground} !important; }}
.grid .tick {{ stroke: {border} !important; }}
.section0, .section2 {{ fill: {surface} !important; opacity: 1 !important; }}
.section1, .section3 {{ fill: {muted} !important; opacity: 1 !important; }}
.task0, .task1, .task2, .task3 {{ fill: {primary} !important; stroke: {border} !important; }}
.taskText0, .taskText1, .taskText2, .taskText3 {{ fill: {primary_foreground} !important; }}
.active0, .active1, .active2, .active3 {{ fill: {warning} !important; stroke: {border} !important; }}
.activeText0, .activeText1, .activeText2, .activeText3 {{ fill: {warning_foreground} !important; }}
.done0, .done1, .done2, .done3 {{ fill: {success} !important; stroke: {border} !important; }}
.doneText0, .doneText1, .doneText2, .doneText3 {{ fill: {success_foreground} !important; }}
.crit0, .crit1, .crit2, .crit3, .doneCrit0, .doneCrit1, .doneCrit2, .doneCrit3 {{ fill: {danger} !important; stroke: {border} !important; }}
.critText0, .critText1, .critText2, .critText3,
.doneCritText0, .doneCritText1, .doneCritText2, .doneCritText3 {{ fill: {danger_foreground} !important; }}
.today {{ stroke: {danger} !important; }}

.face {{ fill: {surface} !important; stroke: {border} !important; }}
.mouth {{ stroke: {foreground} !important; }}
.task-type-0, .task-type-2, .task-type-4, .task-type-6,
.section-type-0, .section-type-2, .section-type-4, .section-type-6 {{ fill: {surface} !important; stroke: {border} !important; }}
.task-type-1, .task-type-3, .task-type-5, .task-type-7,
.section-type-1, .section-type-3, .section-type-5, .section-type-7 {{ fill: {muted} !important; stroke: {border} !important; }}
text.journey-section, text.task, .legend {{ fill: {foreground} !important; }}

.commit-label-bkg, .tag-label-bkg, .branchLabelBkg {{ fill: {surface} !important; stroke: {border} !important; }}
.commit-label, .tag-label, .commit-id, .commit-msg, .branch-label {{ fill: {foreground} !important; }}
.branchLabel text, .branchLabel tspan {{ fill: {foreground} !important; }}
.gitTitleText, .statediagramTitleText, .treemapTitle, .treemapLabel {{ fill: {foreground} !important; }}
.treemapValue {{ fill: {muted_foreground} !important; }}
"
    )
}

fn mermaid_theme_key(cx: &App) -> u64 {
    let mut hasher = DefaultHasher::new();
    for color in [
        cx.theme().background,
        cx.theme().background.raised(1),
        cx.theme().background.hover(),
        cx.theme().foreground,
        cx.theme().foreground.muted(),
        cx.theme().border,
        cx.theme().foreground,
        cx.theme().foreground.on(),
        cx.theme().success,
        cx.theme().success.on(),
        cx.theme().warning,
        cx.theme().warning.on(),
        cx.theme().danger,
        cx.theme().danger.on(),
    ] {
        color.h.to_bits().hash(&mut hasher);
        color.s.to_bits().hash(&mut hasher);
        color.l.to_bits().hash(&mut hasher);
        color.a.to_bits().hash(&mut hasher);
    }
    cx.theme().font_family.hash(&mut hasher);
    cx.theme().is_dark().hash(&mut hasher);
    hasher.finish()
}

#[derive(Default)]
struct MermaidImageCache {
    images: HashMap<u64, Arc<RenderImage>>,
    insertion_order: VecDeque<u64>,
}

impl Global for MermaidImageCache {}

impl MermaidImageCache {
    fn get(&self, key: u64) -> Option<Arc<RenderImage>> {
        self.images.get(&key).cloned()
    }

    fn insert(&mut self, key: u64, image: Arc<RenderImage>) {
        if self.images.contains_key(&key) {
            return;
        }
        self.images.insert(key, image);
        self.insertion_order.push_back(key);
        while self.images.len() > MERMAID_CACHE_CAPACITY {
            if let Some(expired) = self.insertion_order.pop_front() {
                self.images.remove(&expired);
            }
        }
    }
}

enum MermaidRenderResult {
    Pending,
    Ready(Arc<RenderImage>),
    Failed(SharedString),
}

struct MermaidRenderState {
    result: MermaidRenderResult,
    render_key: Option<u64>,
    _task: Option<Task<()>>,
}

impl MermaidRenderState {
    fn new(source: MermaidSource, render_key: u64, cx: &mut Context<Self>) -> Self {
        let mut state = Self {
            result: MermaidRenderResult::Pending,
            render_key: None,
            _task: None,
        };
        state.synchronize(source, render_key, cx);
        state
    }

    fn synchronize(&mut self, source: MermaidSource, render_key: u64, cx: &mut Context<Self>) {
        if self.render_key == Some(render_key) {
            return;
        }
        self.render_key = Some(render_key);
        if let Some(image) = cx
            .try_global::<MermaidImageCache>()
            .and_then(|cache| cache.get(render_key))
        {
            self.result = MermaidRenderResult::Ready(image);
            self._task = None;
            cx.notify();
            return;
        }
        self.result = MermaidRenderResult::Pending;
        let theme = MermaidTheme::from_app(cx);
        let svg_renderer = cx.svg_renderer();
        let delay = cx.background_executor().timer(MERMAID_RENDER_DEBOUNCE);
        let task = cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            delay.await;
            let result = cx
                .background_spawn(async move {
                    let svg = render_mermaid_svg(&source.source, &theme)?;
                    svg_renderer
                        .render_single_frame(svg.as_bytes(), source.scale / 100.0)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                if state.render_key != Some(render_key) {
                    return;
                }
                state.result = match result {
                    Ok(image) => {
                        if cx.try_global::<MermaidImageCache>().is_none() {
                            cx.set_global(MermaidImageCache::default());
                        }
                        cx.global_mut::<MermaidImageCache>()
                            .insert(render_key, image.clone());
                        MermaidRenderResult::Ready(image)
                    }
                    Err(error) => MermaidRenderResult::Failed(error.into()),
                };
                cx.notify();
            });
        });
        self._task = Some(task);
        cx.notify();
    }
}

fn render_mermaid_svg(source: &str, theme: &MermaidTheme) -> Result<String, String> {
    let renderer = merman::render::HeadlessRenderer::new()
        .with_site_config(theme.config.clone())
        .with_vendored_text_measurer()
        .with_diagram_id("zz-agent-mermaid");
    let pipeline = merman::render::SvgPipeline::resvg_safe().with_postprocessor(
        merman::render::ScopedCssPostprocessor::new(theme.svg_css.clone())
            .with_override_policy(merman::render::CssOverridePolicy::StripExistingImportant),
    );
    let svg = renderer
        .render_svg_with_pipeline_sync(source, &pipeline)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Mermaid returned no diagram".to_owned())?;
    Ok(themed_mermaid_canvas(svg, &theme.background))
}

fn themed_mermaid_canvas(mut svg: String, background: &str) -> String {
    const MERMAN_CANVASES: [&str; 2] = ["background-color:white", "background-color: white"];
    for canvas in MERMAN_CANVASES {
        let Some(offset) = svg.find(canvas) else {
            continue;
        };
        svg.replace_range(
            offset..offset + canvas.len(),
            &format!("background-color:{background}"),
        );
        break;
    }
    svg
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn css_color(color: Hsla) -> String {
    let rgba = Rgba::from(color);
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b)
    )
}

/// Height of the agent pane's chrome bar, matching the browser toolbar.
pub const AGENT_HEADER_HEIGHT: f32 = 40.0;
/// Height of a control that sits evenly inset in a chrome bar, leaving
/// [`CHROME_GAP`] above and below it.
pub const AGENT_CHROME_CONTROL_HEIGHT: f32 = AGENT_HEADER_HEIGHT - 2.0 * CHROME_GAP;

/// The agent pane's chrome bar: `leading` and `trailing` pinned to each end.
/// The bar owns only the height, the inset, and the rule beneath it.
pub fn agent_pane_header(
    leading: impl IntoElement,
    trailing: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(px(AGENT_HEADER_HEIGHT))
        .flex_none()
        .items_center()
        .justify_between()
        .gap_3()
        .px(px(CHROME_GAP))
        .border_b_1()
        .border_color(cx.theme().border)
        .child(div().min_w_0().child(leading))
        .child(div().flex_none().child(trailing))
}

#[cfg(test)]
mod workspace_link_tests {
    use super::{INERT_LINK_URL, PENDING_LINK_URL, file_url, resolve_workspace_link};
    use std::path::Path;
    use url::Url;

    const CRATE_DIR: &str = env!("CARGO_MANIFEST_DIR");

    #[test]
    fn urls_with_a_scheme_or_anchor_are_left_alone() {
        let cwd = Some(Path::new(CRATE_DIR));
        assert_eq!(resolve_workspace_link(cwd, "https://zed.dev"), None);
        assert_eq!(resolve_workspace_link(cwd, "mailto:a@b.c"), None);
        assert_eq!(resolve_workspace_link(cwd, "#section"), None);
        assert_eq!(resolve_workspace_link(cwd, "//host/share"), None);
        assert_eq!(resolve_workspace_link(cwd, ""), None);
    }

    /// The half-streamed link a mend rewrites must never open anything, with or
    /// without a working directory to resolve against.
    #[test]
    fn the_pending_link_sentinel_is_made_inert() {
        for cwd in [Some(Path::new(CRATE_DIR)), None] {
            assert_eq!(
                resolve_workspace_link(cwd, PENDING_LINK_URL).as_deref(),
                Some(INERT_LINK_URL)
            );
        }
        assert!(INERT_LINK_URL.starts_with("data:"));
    }

    #[test]
    fn a_relative_link_without_a_working_directory_is_left_alone() {
        assert_eq!(resolve_workspace_link(None, "Cargo.toml"), None);
    }

    #[test]
    fn relative_and_absolute_paths_become_file_urls() {
        let cwd = Path::new(CRATE_DIR);
        let expected = cwd.join("Cargo.toml");
        let absolute = expected.to_string_lossy();
        for link in ["Cargo.toml", absolute.as_ref()] {
            let resolved = resolve_workspace_link(Some(cwd), link).expect("workspace link");
            let url = Url::parse(&resolved).expect("file URL");
            assert_eq!(url.scheme(), "file");
            assert_eq!(url.to_file_path().expect("absolute file URL"), expected);
        }
    }

    #[test]
    fn a_missing_path_is_still_resolved_without_io() {
        let cwd = Path::new(CRATE_DIR);
        let resolved =
            resolve_workspace_link(Some(cwd), "definitely-not-here.md").expect("workspace link");
        let url = Url::parse(&resolved).expect("file URL");
        assert_eq!(
            url.to_file_path().expect("absolute file URL"),
            cwd.join("definitely-not-here.md")
        );
    }

    #[test]
    fn file_urls_percent_encode_reserved_bytes() {
        let path = Path::new(CRATE_DIR).join("with space").join("file.md");
        let encoded = file_url(&path).expect("absolute file URL");
        assert!(encoded.contains("with%20space"));
        assert_eq!(
            Url::parse(&encoded)
                .expect("file URL")
                .to_file_path()
                .expect("absolute file URL"),
            path
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Render, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, point};

    struct ToolContentScrollTest {
        scroll_handle: UniformListScrollHandle,
        timeline_scroll: ListState,
    }

    struct EmptyAgentTimelineTest {
        store: Entity<AgentTimelineStore>,
    }

    const TAIL_PIN_ROW_HEIGHT: f32 = 50.0;
    const TAIL_PIN_BOTTOM_PADDING: f32 = 120.0;

    struct TailPinTest {
        state: ListState,
    }

    impl Render for TailPinTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.0)).h(px(300.0)).child(
                list(self.state.clone(), |_, _, _| {
                    div().w_full().h(px(TAIL_PIN_ROW_HEIGHT)).into_any_element()
                })
                .with_sizing_behavior(ListSizingBehavior::Auto)
                .size_full()
                .pt(px(AGENT_TIMELINE_TOP_PADDING))
                .pb(px(TAIL_PIN_BOTTOM_PADDING)),
            )
        }
    }

    fn scroll_position(state: &ListState) -> f32 {
        -f32::from(state.scroll_px_offset_for_scrollbar().y)
    }

    struct UserEntryTest {
        store: Entity<AgentTimelineStore>,
        entry: AgentEntry,
        pane_width: Pixels,
        active_turn: bool,
    }

    impl Render for UserEntryTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(self.pane_width).h(px(600.0)).child(
                AgentTimeline::new(
                    Arc::new(vec![TimelineRow::Single(self.entry.clone())]),
                    ListState::new(1, gpui::ListAlignment::Top, px(600.0)),
                    self.store.clone(),
                )
                .active_turn(self.active_turn),
            )
        }
    }

    impl Render for EmptyAgentTimelineTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            AgentTimeline::new(
                Arc::new(Vec::new()),
                ListState::new(0, gpui::ListAlignment::Top, px(0.0)),
                self.store.clone(),
            )
        }
    }

    impl Render for ToolContentScrollTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let scroll_handle = self.scroll_handle.clone();
            let timeline_scroll = self.timeline_scroll.clone();
            list(timeline_scroll.clone(), move |index, _window, _cx| {
                if index == 0 {
                    let rows = uniform_list("tool-content-scroll-test-lines", 5, |range, _, _| {
                        range
                            .map(|index| {
                                div().h(px(100.0)).flex_none().when(index == 3, |this| {
                                    this.debug_selector(|| "tool-scroll-visible-row".to_owned())
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .w_full()
                    .max_h(px(TOOL_CONTENT_MAX_HEIGHT))
                    .overflow_hidden()
                    .with_sizing_behavior(ListSizingBehavior::Infer)
                    .track_scroll(&scroll_handle);
                    tool_content_scroll_area(
                        "tool-content-scroll-test",
                        &scroll_handle,
                        &timeline_scroll,
                        rows,
                    )
                    .into_any_element()
                } else {
                    div().h(px(400.0)).flex_none().into_any_element()
                }
            })
            .with_sizing_behavior(ListSizingBehavior::Auto)
            .w(px(500.0))
            .h(px(400.0))
        }
    }

    fn tool_scroll_y(scroll_handle: &UniformListScrollHandle) -> gpui::Pixels {
        scroll_handle.0.borrow().base_handle.offset().y
    }

    fn test_tool(id: u64, label: &str, status: AgentToolStatus) -> AgentEntry {
        test_tool_kind(id, label, AgentToolKind::Edit, status)
    }

    fn test_tool_kind(
        id: u64,
        label: &str,
        kind: AgentToolKind,
        status: AgentToolStatus,
    ) -> AgentEntry {
        AgentEntry::Tool(test_tool_entry(id, label, kind, status))
    }

    fn test_tool_entry(
        id: u64,
        label: &str,
        kind: AgentToolKind,
        status: AgentToolStatus,
    ) -> AgentToolEntry {
        AgentToolEntry {
            id,
            kind,
            status,
            label: label.to_owned().into(),
            location: None,
            input: None,
            output: Arc::from([]),
            default_expanded: false,
        }
    }

    #[test]
    fn timeline_rows_fold_all_consecutive_tools() {
        let entries = vec![
            test_tool(1, "Editing files", AgentToolStatus::Completed),
            test_tool(2, "Editing files", AgentToolStatus::Completed),
            test_tool_kind(
                3,
                "Running command",
                AgentToolKind::Execute,
                AgentToolStatus::Completed,
            ),
            AgentEntry::Assistant {
                id: 4,
                markdown: "done".into(),
            },
            test_tool(5, "Editing files", AgentToolStatus::Completed),
            test_tool(6, "Editing files", AgentToolStatus::Completed),
            test_tool(7, "Editing files", AgentToolStatus::Completed),
            test_tool_kind(
                8,
                "Reading file",
                AgentToolKind::Read,
                AgentToolStatus::Completed,
            ),
        ];

        let folded = fold_timeline_rows(&entries);

        assert_eq!(folded.rows.len(), 3);
        assert_eq!(folded.entry_to_row, [0, 0, 0, 1, 2, 2, 2, 2]);
        assert!(matches!(
            &folded.rows[0],
            TimelineRow::Group {
                kind: TimelineGroupKind::Tool,
                id: 1,
                entries
            } if entries.len() == 3
        ));
        assert!(matches!(
            &folded.rows[1],
            TimelineRow::Single(AgentEntry::Assistant { id: 4, .. })
        ));
        assert!(matches!(
            &folded.rows[2],
            TimelineRow::Group {
                kind: TimelineGroupKind::Tool,
                id: 5,
                entries
            } if entries.len() == 4
        ));
        assert_eq!(
            tool_group_label(match &folded.rows[0] {
                TimelineRow::Group { entries, .. } => entries,
                TimelineRow::Single(_) => unreachable!(),
            }),
            "Edit files, Ran command"
        );
        assert_eq!(
            tool_group_label(match &folded.rows[2] {
                TimelineRow::Group { entries, .. } => entries,
                TimelineRow::Single(_) => unreachable!(),
            }),
            "Edit files, Read file"
        );
    }

    #[test]
    fn timeline_rows_fold_reasoning_separately_from_tools() {
        let reasoning = |id: u64| AgentEntry::Reasoning {
            id,
            label: "Reasoning".into(),
            markdown: format!("thought {id}").into(),
            default_expanded: false,
        };
        let entries = vec![
            reasoning(1),
            test_tool(2, "Editing files", AgentToolStatus::Completed),
            reasoning(3),
            reasoning(4),
            reasoning(5),
            test_tool(6, "Editing files", AgentToolStatus::Completed),
            test_tool(7, "Editing files", AgentToolStatus::Completed),
        ];

        let folded = fold_timeline_rows(&entries);

        assert_eq!(folded.rows.len(), 4);
        assert_eq!(folded.entry_to_row, [0, 1, 2, 2, 2, 3, 3]);
        assert!(
            matches!(
                &folded.rows[0],
                TimelineRow::Single(AgentEntry::Reasoning { .. })
            ),
            "a lone thought is not a group"
        );
        assert!(matches!(
            &folded.rows[2],
            TimelineRow::Group {
                kind: TimelineGroupKind::Reasoning,
                id: 3,
                entries
            } if entries.len() == 3
        ));
        assert!(matches!(
            &folded.rows[3],
            TimelineRow::Group {
                kind: TimelineGroupKind::Tool,
                ..
            },
        ));
    }

    #[test]
    fn transcript_disclosures_point_right_when_closed_and_up_when_open() {
        assert_eq!(disclosure_icon(false), IconName::ChevronRight);
        assert_eq!(disclosure_icon(true), IconName::ChevronUp);
    }

    #[gpui::test]
    fn transcript_affordances_use_the_muted_foreground(cx: &mut TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| {
            assert_eq!(timeline_affordance_color(cx), cx.theme().foreground.muted());
        });
    }

    #[test]
    fn final_assistant_copy_target_is_the_latest_assistant() {
        let rows = vec![
            TimelineRow::Single(AgentEntry::Assistant {
                id: 20,
                markdown: "first".into(),
            }),
            TimelineRow::Single(test_tool(21, "Read file", AgentToolStatus::Completed)),
            TimelineRow::Single(AgentEntry::Assistant {
                id: 22,
                markdown: "second".into(),
            }),
            TimelineRow::Single(AgentEntry::Reasoning {
                id: 23,
                label: "Finished".into(),
                markdown: "done".into(),
                default_expanded: false,
            }),
        ];

        assert_eq!(final_assistant_entry_id(&rows), Some(22));
        assert_eq!(
            final_assistant_entry_id(&[TimelineRow::Single(test_tool(
                24,
                "Read file",
                AgentToolStatus::Completed,
            ))]),
            None
        );
    }

    #[gpui::test]
    fn assistant_and_code_block_copy_buttons_keep_raw_markdown(cx: &mut TestAppContext) {
        cx.update(crate::init);
        const RAW: &str = "Result\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```";
        const CODE: &str = "fn main() {\n    println!(\"hi\");\n}";
        let entry = AgentEntry::Assistant {
            id: 17,
            markdown: RAW.into(),
        };
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width: px(520.0),
                active_turn: false,
            });
            crate::Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let language = cx
            .debug_bounds("agent-code-language")
            .expect("the code language should be painted");
        let code_copy = cx
            .debug_bounds("agent-code-copy")
            .expect("the code copy button should be painted");
        assert!(language.right() < code_copy.left());
        cx.simulate_click(code_copy.center(), gpui::Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
            Some(CODE.to_owned())
        );

        let message_copy = cx
            .debug_bounds("agent-assistant-copy")
            .expect("the message copy button should be painted");
        cx.simulate_click(message_copy.center(), gpui::Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
            Some(RAW.to_owned())
        );
    }

    #[gpui::test]
    fn assistant_copy_is_hidden_while_the_turn_is_active(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let entry = AgentEntry::Assistant {
            id: 25,
            markdown: "Still streaming".into(),
        };
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width: px(520.0),
                active_turn: true,
            });
            crate::Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        assert!(cx.debug_bounds("agent-assistant-copy").is_none());
    }

    #[gpui::test]
    fn tool_disclosure_sits_beside_the_label(cx: &mut TestAppContext) {
        cx.update(crate::init);
        const PANE_WIDTH: Pixels = px(520.0);
        let mut tool = test_tool_entry(
            18,
            "Ran command",
            AgentToolKind::Execute,
            AgentToolStatus::Completed,
        );
        tool.input = Some(AgentToolPayload::Text("cargo test".into()));
        let entry = AgentEntry::Tool(tool);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width: PANE_WIDTH,
                active_turn: false,
            });
            crate::Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let label = cx
            .debug_bounds("agent-tool-label")
            .expect("the tool label should be painted");
        let chevron = cx
            .debug_bounds("agent-tool-chevron")
            .expect("the tool disclosure should be painted");
        assert!(chevron.left() >= label.right());
        assert!(chevron.left() - label.right() <= px(8.0));
        assert!(PANE_WIDTH - chevron.right() > px(200.0));
    }

    #[gpui::test]
    fn a_wide_attachment_keeps_its_bubble_inside_the_pane(cx: &mut TestAppContext) {
        cx.update(crate::init);
        const PANE_WIDTH: Pixels = px(420.0);

        let bytes = include_bytes!("fixtures/wide-screenshot.png").to_vec();
        let entry = AgentEntry::User {
            id: 1,
            markdown: "hi can you read this image properly?".into(),
            images: Arc::from([Arc::new(Image::from_bytes(gpui::ImageFormat::Png, bytes))]),
        };
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width: PANE_WIDTH,
                active_turn: false,
            });
            crate::Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let bubble = cx
            .debug_bounds("agent-user-bubble")
            .expect("the user bubble should be painted");
        assert!(
            bubble.origin.x >= px(0.0),
            "the bubble overhangs the left edge at {:?}",
            bubble.origin.x
        );
        assert!(
            bubble.right() <= PANE_WIDTH,
            "the bubble runs past the pane: {:?} > {PANE_WIDTH:?}",
            bubble.right()
        );

        let tile = cx
            .debug_bounds("agent-user-attachment")
            .expect("the attachment tile should be painted");
        assert_eq!(
            tile.size.width, tile.size.height,
            "the tile is square whatever shape was pasted"
        );
        assert_eq!(tile.size.width, TRANSCRIPT_ATTACHMENT);

        assert!(
            !cx.update(crate::WindowExt::has_active_dialog),
            "nothing should be open before the click"
        );
        cx.simulate_click(tile.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(
            cx.update(crate::WindowExt::has_active_dialog),
            "clicking an attachment should open it"
        );
    }

    #[test]
    fn single_line_collapses_embedded_breaks() {
        assert_eq!(
            single_line("sed -n '110,280p' worker.ts\nsed -n '90,230p' api.rs".into()),
            "sed -n '110,280p' worker.ts · sed -n '90,230p' api.rs"
        );
        assert_eq!(
            single_line("first\r\n\n   second   \n".into()),
            "first · second"
        );
        assert_eq!(single_line("no breaks at all".into()), "no breaks at all");
    }

    fn test_color([r, g, b]: [u8; 3]) -> Hsla {
        Rgba {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a: 1.0,
        }
        .into()
    }

    fn test_mermaid_theme() -> MermaidTheme {
        MermaidTheme::new(
            MermaidThemeColors {
                background: test_color([0x10, 0x11, 0x12]),
                surface: test_color([0x19, 0x1a, 0x1d]),
                muted: test_color([0x24, 0x26, 0x2b]),
                foreground: test_color([0xe8, 0xe9, 0xed]),
                muted_foreground: test_color([0x97, 0x9b, 0xa6]),
                border: test_color([0x38, 0x3a, 0x40]),
                primary: test_color([0x8b, 0x5c, 0xf6]),
                primary_foreground: test_color([0xff, 0xff, 0xff]),
                success: test_color([0x2f, 0x85, 0x5a]),
                success_foreground: test_color([0xff, 0xff, 0xff]),
                warning: test_color([0xb7, 0x79, 0x1f]),
                warning_foreground: test_color([0xff, 0xff, 0xff]),
                danger: test_color([0xc5, 0x30, 0x30]),
                danger_foreground: test_color([0xff, 0xff, 0xff]),
            },
            ".SystemUIFont",
            true,
        )
    }

    #[gpui::test]
    fn agent_timeline_converts_without_recursing(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| EmptyAgentTimelineTest {
            store: cx.new(|_| AgentTimelineStore::default()),
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn nowrap_shaping_still_breaks_on_an_embedded_newline(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| EmptyAgentTimelineTest {
            store: cx.new(|_| AgentTimelineStore::default()),
        });
        let label = SharedString::from("git branch --show-current\ngit log --oneline -12");

        let (raw_lines, collapsed_lines) = cx.update(|window, _| {
            let shape = |text: SharedString| {
                let run = window.text_style().to_run(text.len());
                window
                    .text_system()
                    .shape_text(text, px(13.0), &[run], None, None)
                    .expect("label should shape")
                    .len()
            };
            (shape(label.clone()), shape(single_line(label.clone())))
        });

        assert_eq!(raw_lines, 2, "nowrap does not join lines split by `\\n`");
        assert_eq!(collapsed_lines, 1);
    }

    #[test]
    fn mermaid_renderer_produces_resvg_safe_svg() {
        let theme = test_mermaid_theme();
        let canvas = format!("background-color:{}", theme.background);
        let svg = render_mermaid_svg("flowchart LR\n    Picker --> Agent", &theme)
            .expect("Mermaid fixture should render");

        assert!(svg.contains("<svg"));
        assert!(!svg.contains("<foreignObject"));
        assert!(svg.contains(&canvas));
        assert!(!svg.contains("background-color:white"));
        assert!(svg.contains("data-merman-postprocess=\"scoped-css\""));
    }

    #[test]
    fn mermaid_common_variants_render_with_readable_theme_and_width() {
        const FIXTURES: [(&str, &str); 13] = [
            (
                "flowchart",
                "flowchart TD\n    Start([Prompt]) --> Agent[Agent loop]\n    Agent --> Tool{Use a tool?}\n    Tool -->|Yes| Result[Read result]\n    Tool -->|No| Done([Answer])",
            ),
            (
                "sequence",
                "sequenceDiagram\n    participant User\n    participant Agent\n    participant Tool\n    User->>Agent: Fix Mermaid rendering\n    Agent->>Tool: Render fixture\n    Tool-->>Agent: SVG\n    Agent-->>User: Verified result",
            ),
            (
                "state",
                "stateDiagram-v2\n    [*] --> Idle\n    Idle --> Rendering\n    Rendering --> Ready\n    Rendering --> Failed\n    Ready --> [*]",
            ),
            (
                "er",
                "erDiagram\n    USER ||--o{ SESSION : owns\n    SESSION ||--o{ MESSAGE : contains\n    USER {\n        string id PK\n        string email UK\n    }\n    SESSION {\n        string id PK\n        datetime created_at\n    }\n    MESSAGE {\n        string id PK\n        string content\n    }",
            ),
            (
                "class",
                "classDiagram\n    class Agent {\n        +String model\n        +run() Result\n    }\n    class Tool {\n        +String name\n        +execute() Output\n    }\n    Agent --> Tool : invokes",
            ),
            (
                "pie",
                "pie showData\n    title Tool Usage Breakdown\n    \"Read\" : 42\n    \"Edit\" : 28\n    \"Execute\" : 18\n    \"Search\" : 12",
            ),
            (
                "gantt",
                "gantt\n    title Project Timeline\n    dateFormat YYYY-MM-DD\n    section Design\n        Wireframes :done, design, 2026-07-01, 3d\n    section Build\n        Core Agent Loop :active, core, 2026-07-05, 5d\n        Tool Integration :tools, after core, 4d\n    section Ship\n        Testing :crit, test, after tools, 3d",
            ),
            (
                "mindmap",
                "mindmap\n  root((Agent))\n    Context\n      Files\n      Knowledge\n    Work\n      Reason\n      Tools\n    Verify\n      Tests\n      Visuals",
            ),
            (
                "journey",
                "journey\n    title Rendering repair\n    section Diagnose\n      Reproduce variants: 3: Agent\n      Inspect SVG: 4: Agent\n    section Verify\n      Run tests: 5: Agent\n      Review snapshots: 5: User, Agent",
            ),
            (
                "gitgraph",
                "gitGraph\n    commit id: \"baseline\"\n    branch fix/mermaid\n    commit id: \"theme\"\n    commit id: \"sizing\"\n    checkout main\n    merge fix/mermaid",
            ),
            (
                "quadrant",
                "quadrantChart\n    title Rendering quality\n    x-axis Clipped --> Fits\n    y-axis Low contrast --> Readable\n    ER: [0.8, 0.85]\n    Gantt: [0.75, 0.8]\n    Pie: [0.9, 0.9]",
            ),
            (
                "timeline",
                "timeline\n    title Mermaid renderer\n    section Diagnose\n        Font metrics : ER clipping\n        Fixed canvas : Tiny Gantt\n    section Repair\n        Theme CSS : Visible labels\n        Snapshots : Visual verification",
            ),
            (
                "xychart",
                "xychart-beta\n    title \"Render readability\"\n    x-axis [\"Before\", \"After\"]\n    y-axis \"Score\" 0 --> 10\n    bar [3, 9]",
            ),
        ];

        let fixture = |name: &str| {
            FIXTURES
                .into_iter()
                .find_map(|(candidate, source)| (candidate == name).then_some(source))
                .unwrap_or_else(|| panic!("{name} should be one of the fixtures"))
        };

        let theme = test_mermaid_theme();
        let canvas = format!("background-color:{}", theme.background);
        let snapshot_directory = std::env::var_os("ZZ_MERMAID_SNAPSHOT_DIR");
        if let Some(directory) = &snapshot_directory {
            std::fs::create_dir_all(directory).expect("create Mermaid snapshot directory");
        }

        for (name, source) in FIXTURES {
            let svg = render_mermaid_svg(source, &theme)
                .unwrap_or_else(|error| panic!("{name} fixture should render: {error}"));
            assert!(!svg.contains("<foreignObject"), "{name} was not resvg-safe");
            assert!(
                svg.contains("data-merman-postprocess=\"scoped-css\""),
                "{name} omitted the theme override"
            );
            assert!(svg.contains(&canvas), "{name} retained the light canvas");

            if let Some(directory) = &snapshot_directory {
                std::fs::write(
                    std::path::Path::new(directory).join(format!("{name}.svg")),
                    svg,
                )
                .expect("write Mermaid snapshot");
            }
        }

        let gantt = render_mermaid_svg(fixture("gantt"), &theme).expect("Gantt should render");
        assert!(
            gantt.contains("viewBox=\"0 0 640 "),
            "Gantt should use the transcript-sized canvas: {gantt}"
        );

        let er = render_mermaid_svg(fixture("er"), &theme).expect("ER should render");
        assert!(
            er.contains("system-ui, sans-serif"),
            "ER should use the conservatively measured font stack: {er}"
        );
    }

    #[test]
    fn markdown_fence_language_matching_is_case_insensitive() {
        assert!(is_markdown_language("markdown"));
        assert!(is_markdown_language("MARKDOWN"));
        assert!(is_markdown_language("md"));
        assert!(!is_markdown_language("rust"));
    }

    #[test]
    fn code_block_header_uses_the_fence_language_or_text() {
        assert_eq!(code_block_language(Some("rust".into())), "rust");
        assert_eq!(code_block_language(Some("".into())), "text");
        assert_eq!(code_block_language(None), "text");
    }

    #[gpui::test]
    fn timeline_store_retains_row_state_while_the_row_is_absent(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let store = cx.new(|_| AgentTimelineStore::default());
        let markdown = store.update(cx, |store, cx| {
            store.markdown(7, MarkdownSlot::Body, "before".into(), cx)
        });
        assert!(!store.update(cx, |store, _| {
            store.expanded(7, DisclosureKind::Tool, false)
        }));
        assert!(!store.update(cx, |store, _| {
            store.expanded(7, DisclosureKind::Group, false)
        }));
        store.update(cx, |store, cx| {
            store.toggle_expanded(7, DisclosureKind::Tool, false, cx);
            store.toggle_expanded(7, DisclosureKind::Group, false, cx);
        });
        let payloads = Arc::<[AgentToolPayload]>::from([AgentToolPayload::Text("output".into())]);
        let content = store.update(cx, |store, _| {
            store.tool_content(7, None, None, payloads.clone())
        });
        let scroll = store.update(cx, |store, _| store.tool_scroll(7));
        scroll
            .0
            .borrow()
            .base_handle
            .set_offset(gpui::point(px(0.0), px(-42.0)));

        cx.run_until_parked();

        let retained_markdown = store.update(cx, |store, cx| {
            store.markdown(7, MarkdownSlot::Body, "before".into(), cx)
        });
        let retained_content =
            store.update(cx, |store, _| store.tool_content(7, None, None, payloads));
        let retained_scroll = store.update(cx, |store, _| store.tool_scroll(7));
        assert_eq!(retained_markdown, markdown);
        assert!(Arc::ptr_eq(&retained_content, &content));
        assert!(store.update(cx, |store, _| {
            store.expanded(7, DisclosureKind::Tool, false)
        }));
        assert!(store.update(cx, |store, _| {
            store.expanded(7, DisclosureKind::Group, false)
        }));
        assert_eq!(retained_scroll.0.borrow().base_handle.offset().y, px(-42.0));
    }

    #[gpui::test]
    fn timeline_store_sync_uses_append_and_replacement_paths(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let store = cx.new(|_| AgentTimelineStore::default());
        let source = AgentMarkdown::new("hé");
        let markdown = store.update(cx, |store, cx| {
            store.markdown(9, MarkdownSlot::Body, source.clone(), cx)
        });

        source.synchronize_append("héllo");
        let append = store.update(cx, |store, cx| {
            store.update_markdown(9, MarkdownSlot::Body, source.clone(), cx)
        });
        assert_eq!(append, MarkdownUpdate::Appended);
        cx.read(|cx| {
            assert_eq!(
                store.read(cx).markdown[&(9, MarkdownSlot::Body)].source,
                "héllo"
            );
            assert_eq!(
                store.read(cx).markdown[&(9, MarkdownSlot::Body)].state,
                markdown
            );
        });

        source.replace("help");
        let replacement = store.update(cx, |store, cx| {
            store.update_markdown(9, MarkdownSlot::Body, source, cx)
        });
        assert_eq!(replacement, MarkdownUpdate::Replaced);
        cx.read(|cx| {
            assert_eq!(
                store.read(cx).markdown[&(9, MarkdownSlot::Body)].source,
                "help"
            );
            assert_eq!(
                store.read(cx).markdown[&(9, MarkdownSlot::Body)].state,
                markdown
            );
        });
    }

    #[test]
    fn large_markdown_keeps_a_bounded_preview_and_full_copy() {
        let original = format!("# Result\n\n{}", "long response line\n".repeat(4_000));
        let source = AgentMarkdown::new(original.clone());

        assert!(source.is_truncated());
        source.inspect(|preview, _, _| {
            assert!(preview.len() <= MARKDOWN_PREVIEW_MAX_BYTES + MARKDOWN_PREVIEW_MARKER.len());
            assert!(preview.ends_with(MARKDOWN_PREVIEW_MARKER));
        });
        assert_eq!(source.full_text(), original);

        let appended = format!("{original}tail that remains available to copy");
        source.synchronize_append(&appended);
        assert_eq!(source.full_text(), appended);
        source.inspect(|preview, _, _| {
            assert!(preview.ends_with(MARKDOWN_PREVIEW_MARKER));
        });

        source.replace("short again");
        assert!(!source.is_truncated());
        assert_eq!(source.full_text(), "short again");
        source.inspect(|preview, _, _| assert_eq!(preview, "short again"));
    }

    /// A hanging marker is closed for the reader while the entry streams, and
    /// the settle hands back exactly the bytes the thread holds.
    #[gpui::test]
    fn a_streaming_entry_renders_mended_and_settles_raw(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let store = cx.new(|_| AgentTimelineStore::default());
        store.update(cx, |store, cx| store.set_streaming(Some(3), cx));
        let source = AgentMarkdown::new("a **partly");
        let state = store.update(cx, |store, cx| {
            store.markdown(3, MarkdownSlot::Body, source.clone(), cx)
        });
        cx.run_until_parked();
        assert_eq!(
            state.read_with(cx, |state, _| state.source()),
            "a **partly**"
        );

        source.synchronize_append("a **partly bold");
        let appended = store.update(cx, |store, cx| {
            store.update_markdown(3, MarkdownSlot::Body, source.clone(), cx)
        });
        cx.run_until_parked();
        assert_eq!(appended, MarkdownUpdate::Replaced);
        assert_eq!(
            state.read_with(cx, |state, _| state.source()),
            "a **partly bold**"
        );

        source.synchronize_append("a **partly bold** run");
        let closed = store.update(cx, |store, cx| {
            store.update_markdown(3, MarkdownSlot::Body, source.clone(), cx)
        });
        cx.run_until_parked();
        assert_eq!(closed, MarkdownUpdate::Replaced);
        assert_eq!(
            state.read_with(cx, |state, _| state.source()),
            "a **partly bold** run"
        );
        assert!(!store.read_with(cx, |store, _| {
            store.markdown[&(3, MarkdownSlot::Body)].mended
        }));

        let raw = "a **partly bold** run, then *more";
        source.synchronize_append(raw);
        store.update(cx, |store, cx| {
            store.update_markdown(3, MarkdownSlot::Body, source, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            state.read_with(cx, |state, _| state.source()),
            "a **partly bold** run, then *more*"
        );

        store.update(cx, |store, cx| store.set_streaming(None, cx));
        cx.run_until_parked();
        assert_eq!(state.read_with(cx, |state, _| state.source()), raw);
    }

    #[gpui::test]
    fn a_large_hanging_inline_marker_is_repaired_off_the_ui_thread(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let store = cx.new(|_| AgentTimelineStore::default());
        store.update(cx, |store, cx| store.set_streaming(Some(8), cx));
        let raw = format!("{}**partly", "word ".repeat(4_000));
        assert!(raw.len() < MARKDOWN_PREVIEW_MAX_BYTES);
        let source = AgentMarkdown::new(raw.clone());
        let state = store.update(cx, |store, cx| {
            store.markdown(8, MarkdownSlot::Body, source.clone(), cx)
        });
        cx.run_until_parked();
        assert_eq!(
            state.read_with(cx, |state, _| state.source()),
            format!("{raw}**")
        );

        let next = format!("{raw} bold");
        source.synchronize_append(&next);
        store.update(cx, |store, cx| {
            store.update_markdown(8, MarkdownSlot::Body, source, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            state.read_with(cx, |state, _| state.source()),
            format!("{next}**")
        );
    }

    /// The prefix fast path survives the mend: appends are diffed against the
    /// raw text, so an entry that never hangs a marker never reparses.
    #[gpui::test]
    fn streaming_appends_without_hanging_markers_stay_incremental(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let store = cx.new(|_| AgentTimelineStore::default());
        store.update(cx, |store, cx| store.set_streaming(Some(4), cx));
        let source = AgentMarkdown::new("plain");
        store.update(cx, |store, cx| {
            store.markdown(4, MarkdownSlot::Body, source.clone(), cx)
        });

        source.synchronize_append("plain text");
        let appended = store.update(cx, |store, cx| {
            store.update_markdown(4, MarkdownSlot::Body, source, cx)
        });

        assert_eq!(appended, MarkdownUpdate::Appended);
        assert!(!store.read_with(cx, |store, _| {
            store.markdown[&(4, MarkdownSlot::Body)].mended
        }));
    }

    #[gpui::test]
    fn a_settled_entry_is_never_mended(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let store = cx.new(|_| AgentTimelineStore::default());
        let source = AgentMarkdown::new("a **partly");
        let state = store.update(cx, |store, cx| {
            store.markdown(5, MarkdownSlot::Body, source.clone(), cx)
        });
        cx.run_until_parked();
        assert_eq!(state.read_with(cx, |state, _| state.source()), "a **partly");

        source.synchronize_append("a **partly bold");
        let update = store.update(cx, |store, cx| {
            store.update_markdown(5, MarkdownSlot::Body, source, cx)
        });
        cx.run_until_parked();
        assert_eq!(update, MarkdownUpdate::Appended);
        assert_eq!(
            state.read_with(cx, |state, _| state.source()),
            "a **partly bold"
        );
    }

    #[gpui::test]
    fn equal_tool_payload_sync_does_not_rematerialize(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let payloads = Arc::<[AgentToolPayload]>::from([AgentToolPayload::Json(
            "{\n  \"ok\": true\n}".into(),
        )]);
        let store = cx.new(|_| AgentTimelineStore::default());
        let content = store.update(cx, |store, _| {
            store.tool_content(10, None, None, payloads.clone())
        });
        let update = store.update(cx, |store, cx| {
            store.update_tool_content(10, None, None, payloads.clone(), cx)
        });
        let retained = store.update(cx, |store, _| store.tool_content(10, None, None, payloads));

        assert_eq!(update, ToolContentUpdate::Unchanged);
        assert!(Arc::ptr_eq(&content, &retained));
    }

    #[test]
    fn diff_materialization_computes_lines() {
        let materialized = materialize_tool_payload(&AgentToolPayload::Diff {
            path: "/workspace/src/lib.rs".into(),
            old: Some("same\nremoved\n".into()),
            new: "same\nadded one\nadded two\n".into(),
        });

        assert_eq!(
            materialized
                .rows
                .iter()
                .filter(|row| matches!(
                    row,
                    ToolContentRow::Diff {
                        kind: DiffLineKind::Added,
                        ..
                    }
                ))
                .count(),
            2
        );
        assert_eq!(
            materialized
                .rows
                .iter()
                .filter(|row| matches!(
                    row,
                    ToolContentRow::Diff {
                        kind: DiffLineKind::Removed,
                        ..
                    }
                ))
                .count(),
            1
        );
    }

    #[test]
    fn diff_materialization_caps_inputs_before_myers() {
        use std::fmt::Write as _;

        let mut old = String::new();
        let mut new = String::new();
        for line in 0..TOOL_CONTENT_MAX_LINES * 2 {
            writeln!(&mut old, "old-{line}").expect("write old diff fixture");
            writeln!(&mut new, "new-{line}").expect("write new diff fixture");
        }
        let (bounded_old, old_truncated) = bounded_tool_diff_prefix(&old);
        let (bounded_new, new_truncated) = bounded_tool_diff_prefix(&new);

        assert_eq!(bounded_old.lines().count(), TOOL_CONTENT_MAX_LINES);
        assert_eq!(bounded_new.lines().count(), TOOL_CONTENT_MAX_LINES);
        assert!(old_truncated && new_truncated);

        let materialized = materialize_tool_payload(&AgentToolPayload::Diff {
            path: "/workspace/src/lib.rs".into(),
            old: Some(old.into()),
            new: new.into(),
        });
        assert!(materialized.rows.len() <= TOOL_CONTENT_MAX_LINES + 2);
        assert!(matches!(
            materialized.rows.last(),
            Some(ToolContentRow::Footer(note)) if note.contains("truncated")
        ));
    }

    #[test]
    fn tool_content_line_cap_adds_a_truncation_footer() {
        let text = (0..TOOL_CONTENT_MAX_LINES + 2)
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let payload = AgentToolPayload::Text(text.clone().into());
        let materialized = materialize_tool_payload(&payload);

        assert_eq!(materialized.rows.len(), TOOL_CONTENT_MAX_LINES + 1);
        assert!(matches!(
            materialized.rows.last(),
            Some(ToolContentRow::Footer(note))
                if note.contains("truncated")
        ));
        assert_eq!(tool_payload_copy_text(std::slice::from_ref(&payload)), text);
    }

    #[test]
    fn terminal_payload_keeps_the_latest_lines_for_tail_following() {
        let text = (0..TOOL_CONTENT_MAX_LINES + 2)
            .map(|line| format!("line-{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let materialized = materialize_tool_payload(&AgentToolPayload::Terminal(text.into()));

        assert_eq!(materialized.rows.len(), TOOL_CONTENT_MAX_LINES + 1);
        assert!(matches!(
            &materialized.rows[0],
            ToolContentRow::Footer(label) if label.contains("latest output")
        ));
        assert!(matches!(
            &materialized.rows[1],
            ToolContentRow::Plain(line) if line == "line-2"
        ));
        assert!(matches!(
            materialized.rows.last(),
            Some(ToolContentRow::Plain(line))
                if line == &format!("line-{}", TOOL_CONTENT_MAX_LINES + 1)
        ));
    }

    #[test]
    fn inline_code_paragraph_preserves_text_and_marks_code_range() {
        let node = markdown_ast::Node::Paragraph(markdown_ast::Paragraph {
            children: vec![
                markdown_ast::Node::Text(markdown_ast::Text {
                    value: "Run ".to_owned(),
                    position: None,
                }),
                markdown_ast::Node::InlineCode(markdown_ast::InlineCode {
                    value: "cargo test".to_owned(),
                    position: None,
                }),
                markdown_ast::Node::Text(markdown_ast::Text {
                    value: " now".to_owned(),
                    position: None,
                }),
            ],
            position: None,
        });

        let paragraph = inline_code_paragraph(&node, 41).expect("inline code paragraph");

        assert_eq!(paragraph.text, "Run cargo test now");
        assert_eq!(paragraph.source_offset, 41);
        assert_eq!(paragraph.spans.len(), 3);
        assert_eq!(paragraph.spans[1].range, 4..14);
        assert!(paragraph.spans[1].style.code);
    }

    #[gpui::test]
    fn expanded_tool_content_scrolls_inside_its_bounded_viewport(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let scroll_handle = UniformListScrollHandle::new();
        let timeline_scroll = ListState::new(2, gpui::ListAlignment::Top, px(360.0));
        let (_, cx) = cx.add_window_view({
            let scroll_handle = scroll_handle.clone();
            let timeline_scroll = timeline_scroll.clone();
            move |_, _| ToolContentScrollTest {
                scroll_handle: scroll_handle.clone(),
                timeline_scroll: timeline_scroll.clone(),
            }
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let initial_y = cx
            .debug_bounds("tool-scroll-visible-row")
            .expect("visible row bounds")
            .origin
            .y;
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(10.0), px(10.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-80.0))),
            ..Default::default()
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let scrolled_y = cx
            .debug_bounds("tool-scroll-visible-row")
            .expect("visible row bounds after scroll")
            .origin
            .y;
        assert!(scrolled_y < initial_y);
        assert!(tool_scroll_y(&scroll_handle) < px(0.0));
        assert_eq!(timeline_scroll.scroll_px_offset_for_scrollbar().y, px(0.0));

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(10.0), px(10.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-80.0))),
            ..Default::default()
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert_eq!(tool_scroll_y(&scroll_handle), px(-140.0));
        assert_eq!(timeline_scroll.scroll_px_offset_for_scrollbar().y, px(0.0));

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(10.0), px(10.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-80.0))),
            ..Default::default()
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let outer_offset = timeline_scroll.scroll_px_offset_for_scrollbar();
        assert!(outer_offset.y < px(0.0));

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(10.0), px(10.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(80.0))),
            ..Default::default()
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert!(tool_scroll_y(&scroll_handle) > px(-140.0));
        assert_eq!(
            timeline_scroll.scroll_px_offset_for_scrollbar(),
            outer_offset
        );
    }
    fn tail_pin_window(
        cx: &mut TestAppContext,
        rows: usize,
    ) -> (ListState, &mut VisualTestContext) {
        cx.update(crate::init);
        let state = ListState::new(0, gpui::ListAlignment::Top, px(200.0));
        state.splice(0..0, rows);
        let (_, cx) = cx.add_window_view({
            let state = state.clone();
            move |_, _| TailPinTest {
                state: state.clone(),
            }
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        (state, cx)
    }

    #[gpui::test]
    fn the_pin_measures_the_end_through_the_lists_own_padding(cx: &mut TestAppContext) {
        let (state, cx) = tail_pin_window(cx, 40);
        let mut stick = TimelineStick::new(&state, false);
        stick.set_bottom_padding(TAIL_PIN_BOTTOM_PADDING);
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert!(stick.distance_from_bottom(&state) <= AGENT_AT_BOTTOM_PX);

        // Whatever the list's padding is, the distance to the end has to equal
        // the travel the list itself just performed; measuring the items alone
        // would report the padding short.
        let landed = scroll_position(&state);
        state.scroll_by(px(-500.0));
        let moved = landed - scroll_position(&state);
        assert!(moved > 0.0);
        assert!(
            (stick.distance_from_bottom(&state) - moved).abs() < 1.0,
            "distance {} should equal the {moved}px just scrolled away",
            stick.distance_from_bottom(&state)
        );
    }

    #[gpui::test]
    fn growing_the_transcript_cannot_break_the_pin(cx: &mut TestAppContext) {
        let (state, cx) = tail_pin_window(cx, 40);
        let mut stick = TimelineStick::new(&state, false);
        stick.set_bottom_padding(TAIL_PIN_BOTTOM_PADDING);
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let count = state.item_count();
        state.splice(count..count, 20);
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        // The end ran away from the viewport — the same position change a
        // wheel notch would make — and the pin is untouched, with a frame
        // asked for to chase it.
        assert!(stick.distance_from_bottom(&state) > AGENT_AT_BOTTOM_PX);
        assert!(stick.is_pinned());
        assert!(!stick.shows_jump_button());
        assert!(stick.wants_frame(&state));
    }

    #[gpui::test]
    fn a_wheel_scroll_away_breaks_the_pin_and_returning_restores_it(cx: &mut TestAppContext) {
        let (state, cx) = tail_pin_window(cx, 40);
        let mut stick = TimelineStick::new(&state, false);
        stick.set_bottom_padding(TAIL_PIN_BOTTOM_PADDING);
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let landed = scroll_position(&state);
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(10.0), px(10.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(400.0))),
            ..Default::default()
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert!(landed - scroll_position(&state) > AGENT_JUMP_TO_BOTTOM_PX);
        stick.on_user_scroll(&state, false);
        assert!(!stick.is_pinned());
        assert!(stick.shows_jump_button());

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(10.0), px(10.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-4_000.0))),
            ..Default::default()
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        stick.on_user_scroll(&state, false);
        assert!(stick.is_pinned());
        assert!(!stick.shows_jump_button());
    }

    #[gpui::test]
    fn reduced_motion_keeps_the_lists_own_tail_follow(cx: &mut TestAppContext) {
        let (state, cx) = tail_pin_window(cx, 40);
        let mut stick = TimelineStick::new(&state, true);
        stick.set_bottom_padding(TAIL_PIN_BOTTOM_PADDING);
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert!(state.is_following_tail());

        let count = state.item_count();
        state.splice(count..count, 20);
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert!(state.is_following_tail());
        assert!(stick.distance_from_bottom(&state) <= AGENT_AT_BOTTOM_PX);
    }
}

#[cfg(test)]
mod stick_spring_tests {
    use super::{
        AGENT_STICK_THRESHOLD_PX, SPRING_CHASE_MAX_LEAD, StickSpring, agent_should_restick,
    };
    use std::time::Duration;

    #[test]
    fn the_spring_lands_exactly_on_a_fixed_target() {
        let mut spring = StickSpring::new();
        let target = 400.0;
        let mut pos = 0.0;
        let mut frames = 0;
        while pos < target && frames < 600 {
            pos = spring.step(pos, target, 1.0);
            frames += 1;
        }
        assert_eq!(pos, target, "the spring must land exactly on the target");
        assert!(
            frames < 300,
            "400px should converge in under 5s, took {frames}"
        );
        for _ in 0..120 {
            pos = spring.step(pos, target, 1.0);
            assert_eq!(pos, target);
        }
        assert!(spring.is_idle(), "no residual motion at rest");
    }

    #[test]
    fn the_spring_never_overshoots_or_oscillates() {
        let mut spring = StickSpring::new();
        let target = 250.0;
        let mut pos = 0.0;
        let mut last = pos;
        for _ in 0..600 {
            pos = spring.step(pos, target, 1.0);
            assert!(pos <= target, "overshoot: {pos} > {target}");
            assert!(
                pos >= last - 1e-3,
                "oscillation: position moved backwards {last} -> {pos}"
            );
            last = pos;
        }
        assert_eq!(pos, target);
    }

    #[test]
    fn the_feed_forward_tracks_constant_growth() {
        let growth = 2.0;
        let mut spring = StickSpring::new();
        let mut target = 600.0;
        let mut pos = 600.0;
        let mut deltas: Vec<f32> = Vec::new();
        for frame in 0..400 {
            target += growth;
            let next = spring.step(pos, target, 1.0);
            if frame >= 200 {
                deltas.push(next - pos);
            }
            pos = next;
        }
        let mean = deltas.iter().sum::<f32>() / deltas.len() as f32;
        assert!(
            (mean - growth).abs() < 0.2,
            "steady-state speed {mean} should track growth {growth}"
        );
        for delta in &deltas {
            assert!(*delta > 0.0, "the viewport stalled mid-stream");
            assert!(
                *delta < growth * 3.0,
                "the viewport jumped {delta}px in one frame"
            );
        }
        assert!((spring.target_vel() - growth).abs() < 0.3);
        assert!(target - pos <= SPRING_CHASE_MAX_LEAD + growth);
    }

    #[test]
    fn the_feed_forward_resets_when_the_target_shrinks() {
        let mut spring = StickSpring::new();
        let mut pos = 0.0;
        for i in 1..=50u8 {
            pos = spring.step(pos, 100.0 + f32::from(i) * 4.0, 1.0);
        }
        assert!(spring.target_vel() > 1.0);
        let _ = spring.step(pos.min(120.0), 120.0, 1.0);
        assert_eq!(spring.target_vel(), 0.0);
    }

    #[test]
    fn a_hitch_catches_up_instead_of_teleporting() {
        let target = 300.0;
        let mut stepped = StickSpring::new();
        let mut pos_stepped = 0.0;
        for _ in 0..5 {
            pos_stepped = stepped.step(pos_stepped, target, 1.0);
        }
        let mut hitched = StickSpring::new();
        let pos_hitched = hitched.step(0.0, target, 5.0);
        assert!(
            (pos_stepped - pos_hitched).abs() < 1.0,
            "{pos_stepped} vs {pos_hitched}"
        );
        assert!(pos_hitched <= target);
    }

    #[test]
    fn a_long_stall_is_capped_at_the_catchup_budget() {
        assert!((StickSpring::frames(Duration::from_millis(16)) - 0.96).abs() < 1e-4);
        assert_eq!(StickSpring::frames(Duration::from_secs(2)), 8.0);
    }

    #[test]
    fn resticking_is_direction_aware() {
        assert!(!agent_should_restick(20.0, 0.0));
        assert!(!agent_should_restick(69.0, 30.0));
        assert!(agent_should_restick(30.0, 69.0));
        assert!(agent_should_restick(AGENT_STICK_THRESHOLD_PX, 400.0));
        assert!(!agent_should_restick(AGENT_STICK_THRESHOLD_PX + 1.0, 400.0));
    }
}
