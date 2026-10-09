pub mod composer;
pub mod controls;
pub mod presentation;
pub mod question;
pub mod slash;
pub mod tasks;
pub mod title;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, OnceLock},
};

use web_time::{Duration, Instant};

use crate::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, Icon, IconName, Sizable as _, StyledExt as _,
    attachment::{open_attachment_preview, open_render_image_preview},
    button::{Button, ButtonVariants as _},
    h_flex,
    mend::{PENDING_LINK_URL, mend},
    text::{
        CodeBlock, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
        TextView, TextViewState, TextViewStyle, markdown_ast,
    },
    v_flex,
};
use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, ElementId, Entity, FollowMode, FontWeight,
    Global, Hsla, Image, ImageSource, IntoElement, ListOffset, ListSizingBehavior, ListState,
    ObjectFit, Pixels, RenderImage, Rgba, SharedString, Stateful, Task, Window, div, img, list,
    prelude::*, px, relative,
};
use parking_lot::RwLock;
use presentation::{spinner, spinner_phase};

const MERMAID_NODE_NAME: &str = "zz-mermaid";
const RICH_MARKDOWN_NODE_NAME: &str = "zz-rich-markdown";
const MERMAID_MAX_HEIGHT: f32 = 560.0;
const MERMAID_MAX_SOURCE_BYTES: usize = 32 * 1024;
const MERMAID_RENDER_DEBOUNCE: Duration = Duration::from_millis(250);
const ACTIVITY_ROW_HEIGHT: f32 = 28.0;
const ACTIVITY_ROW_FONT_SIZE: f32 = 13.0;
/// Tall enough to clear the system font's ascent-plus-descent at
/// [`ACTIVITY_ROW_FONT_SIZE`] (15.31px), so `overflow_hidden` never clips a
/// descender.
const ACTIVITY_ROW_LINE_HEIGHT: f32 = 16.0;
const ACTIVITY_DISCLOSURE_SIZE: f32 = 12.0;
/// gpui centres a glyph on its ascent/descent box, but the ink a reader sees
/// sits below that centre — 0.33px for caps, 1.49px for x-height at 13px. Drop
/// the icons by less than either so they never overshoot the letters.
const ACTIVITY_ICON_OPTICAL_DROP: f32 = 0.5;
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
const TURN_GAP: f32 = 16.0;
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
            rendered: None,
            revision: 0,
            replaced_at: 0,
            line_breaks: 0,
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

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum MarkdownSlot {
    Body,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum DisclosureKind {
    Turn,
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
    rendered: Option<String>,
    revision: u64,
    replaced_at: u64,
    line_breaks: usize,
}

#[derive(Clone, Default)]
pub struct AgentMarkdown(Arc<RwLock<AgentMarkdownBuffer>>);

impl AgentMarkdown {
    #[must_use]
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let line_breaks = source.bytes().filter(|byte| *byte == b'\n').count();
        let rendered = markdown_preview(&source, line_breaks);
        Self(Arc::new(RwLock::new(AgentMarkdownBuffer {
            source,
            rendered,
            revision: 0,
            replaced_at: 0,
            line_breaks,
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
            let line_breaks = buffer
                .line_breaks
                .saturating_add(appended.bytes().filter(|byte| *byte == b'\n').count());
            if buffer.rendered.is_some() || markdown_preview_end(source, line_breaks).is_none() {
                buffer.source.push_str(appended);
                buffer.line_breaks = line_breaks;
                if buffer.rendered.is_none() {
                    buffer.revision = buffer.revision.wrapping_add(1);
                }
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
        self.0.read().rendered.is_some()
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
        inspect(
            buffer.rendered.as_deref().unwrap_or(&buffer.source),
            buffer.revision,
            buffer.replaced_at,
        )
    }
}

fn replace_markdown_buffer(buffer: &mut AgentMarkdownBuffer, source: &str) {
    let line_breaks = source.bytes().filter(|byte| *byte == b'\n').count();
    let rendered = markdown_preview(source, line_breaks);
    let changed = buffer.rendered.as_deref().unwrap_or(&buffer.source)
        != rendered.as_deref().unwrap_or(source);
    buffer.source.clear();
    buffer.source.push_str(source);
    buffer.line_breaks = line_breaks;
    if changed {
        buffer.revision = buffer.revision.wrapping_add(1);
        buffer.replaced_at = buffer.revision;
    }
    buffer.rendered = rendered;
}

fn markdown_preview(source: &str, line_breaks: usize) -> Option<String> {
    let end = markdown_preview_end(source, line_breaks)?;
    let prefix = &source[..end];
    let mut rendered = mend(prefix).unwrap_or_else(|| prefix.to_owned());
    rendered.push_str(MARKDOWN_PREVIEW_MARKER);
    Some(rendered)
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

struct TurnClock {
    started: Instant,
    finished: Option<Duration>,
}

#[derive(Default)]
pub struct AgentTimelineStore {
    markdown: HashMap<(u64, MarkdownSlot), MarkdownState>,
    expanded: HashMap<(u64, DisclosureKind), bool>,
    turn_clocks: HashMap<u64, TurnClock>,
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

    pub fn set_expanded(
        &mut self,
        id: u64,
        kind: DisclosureKind,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        if self.expanded.insert((id, kind), expanded) != Some(expanded) {
            cx.notify();
        }
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

    pub fn tick_turn_clocks(&mut self, live: Option<u64>) {
        for (id, clock) in &mut self.turn_clocks {
            if Some(*id) != live && clock.finished.is_none() {
                clock.finished = Some(clock.started.elapsed());
            }
        }
        if let Some(live) = live {
            self.turn_clocks.entry(live).or_insert_with(|| TurnClock {
                started: Instant::now(),
                finished: None,
            });
        }
    }

    #[must_use]
    pub fn turn_elapsed(&self, id: u64) -> Option<Duration> {
        let clock = self.turn_clocks.get(&id)?;
        Some(clock.finished.unwrap_or_else(|| clock.started.elapsed()))
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) -> bool {
        self.streaming = None;
        if self.markdown.is_empty() && self.expanded.is_empty() && self.turn_clocks.is_empty() {
            return false;
        }
        self.markdown.clear();
        self.expanded.clear();
        self.turn_clocks.clear();
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
        rewind_id: Option<SharedString>,
    },
    Assistant {
        id: u64,
        markdown: AgentMarkdown,
        aside: Option<AgentAside>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentAside {
    pub side: bool,
    pub reply_to: Option<u64>,
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
    pub parent: Option<u64>,
    pub exit_code: Option<i64>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineGroupKind {
    Turn,
    Reply,
}

#[must_use]
pub const fn timeline_group_kind(entry: &AgentEntry) -> Option<TimelineGroupKind> {
    match entry {
        AgentEntry::Tool(_)
        | AgentEntry::Reasoning { .. }
        | AgentEntry::Plan { .. }
        | AgentEntry::Assistant { aside: None, .. } => Some(TimelineGroupKind::Turn),
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
    pub fn id(&self) -> u64 {
        match self {
            Self::Single(entry) => entry.id(),
            Self::Group { id, .. } => *id,
        }
    }

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
    let positions = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id(), index))
        .collect::<HashMap<_, _>>();
    let roots = (0..entries.len())
        .map(|index| step_root(entries, &positions, index))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut entry_to_row = vec![0; entries.len()];
    for (index, entry) in entries.iter().enumerate() {
        if roots[index].is_none() {
            entry_to_row[index] = append_timeline_row(&mut rows, entry.clone()).0;
        }
    }
    for (index, entry) in entries.iter().enumerate() {
        if let Some(root) = roots[index] {
            let row_index = entry_to_row[root];
            nest_in_row(&mut rows[row_index], entry.clone());
            entry_to_row[index] = row_index;
        }
    }
    FoldedTimelineRows {
        rows: Arc::new(rows),
        entry_to_row,
    }
}

fn step_root(
    entries: &[AgentEntry],
    positions: &HashMap<u64, usize>,
    index: usize,
) -> Option<usize> {
    let mut at = index;
    for _ in 0..entries.len() {
        match timeline_parent(&entries[at]).and_then(|parent| positions.get(&parent).copied()) {
            Some(parent) => at = parent,
            None => return (at != index).then_some(at),
        }
    }
    None
}

fn nest_in_row(row: &mut TimelineRow, entry: AgentEntry) {
    match row {
        TimelineRow::Group { entries, .. } => Arc::make_mut(entries).push(entry),
        TimelineRow::Single(previous) => {
            let id = previous.id();
            let kind = if matches!(previous, AgentEntry::User { .. }) {
                TimelineGroupKind::Reply
            } else {
                TimelineGroupKind::Turn
            };
            let previous = previous.clone();
            *row = TimelineRow::Group {
                kind,
                id,
                entries: Arc::new(vec![previous, entry]),
            };
        }
    }
}

#[must_use]
pub const fn timeline_parent(entry: &AgentEntry) -> Option<u64> {
    match entry {
        AgentEntry::Tool(tool) => tool.parent,
        AgentEntry::Assistant {
            aside: Some(aside), ..
        } => aside.reply_to,
        _ => None,
    }
}

#[must_use]
pub fn append_timeline_row(rows: &mut Vec<TimelineRow>, entry: AgentEntry) -> (usize, bool) {
    if let Some(parent) = timeline_parent(&entry)
        && let Some(row_index) = rows.iter().rposition(|row| row.entry(parent).is_some())
    {
        nest_in_row(&mut rows[row_index], entry);
        return (row_index, false);
    }
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

/// The jump-to-bottom disc, an arrow-down mirror of the composer's send
/// button. Paint it as an overlay: it must not take part in the timeline's
/// layout, or appearing would resize the scroll viewport and move the very
/// content it is offering to reveal.
pub fn agent_jump_to_bottom_button(id: impl Into<ElementId>, cx: &App) -> Button {
    Button::compact_icon(id, IconName::ArrowDown)
        .secondary()
        .rounded_full()
        .control_surface(cx)
        .tooltip("Jump to latest")
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

    pub fn reveal(&mut self, list: &ListState, row: usize) {
        self.release(list);
        self.show_jump = true;
        list.scroll_to(ListOffset {
            item_ix: row,
            offset_in_item: px(0.0),
        });
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

type RewindHandler = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;
type OpenOutputHandler = Rc<dyn Fn(&AgentToolEntry, &mut Window, &mut App)>;

#[derive(Clone)]
struct TimelineRewind {
    enabled: bool,
    handler: RewindHandler,
}

#[derive(Clone, IntoElement)]
pub struct AgentTimeline {
    rows: Arc<Vec<TimelineRow>>,
    list_state: ListState,
    store: Entity<AgentTimelineStore>,
    active_turn: bool,
    bottom_padding: f32,
    rewind: Option<TimelineRewind>,
    open_output: Option<OpenOutputHandler>,
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
            rewind: None,
            open_output: None,
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

    #[must_use]
    pub fn rewind(
        mut self,
        enabled: bool,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.rewind = Some(TimelineRewind {
            enabled,
            handler: Rc::new(handler),
        });
        self
    }

    #[must_use]
    pub fn open_output(
        mut self,
        handler: impl Fn(&AgentToolEntry, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.open_output = Some(Rc::new(handler));
        self
    }
}

impl gpui::RenderOnce for AgentTimeline {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let rows = self.rows;
        let store = self.store;
        let copyable_assistant = if self.active_turn {
            None
        } else {
            final_assistant_entry_id(&rows)
        };
        let bottom_padding = self.bottom_padding;
        let rewind = self.rewind.map(|rewind| TimelineRewind {
            enabled: rewind.enabled && !self.active_turn,
            handler: rewind.handler,
        });

        let open_output = self.open_output;
        let live_row = self
            .active_turn
            .then(|| rows.len().checked_sub(1))
            .flatten()
            .filter(|last| is_turn_row(&rows[*last]));
        let live_turn = live_row.map(|row| rows[row].id());
        store.update(cx, |store, _| store.tick_turn_clocks(live_turn));
        list(self.list_state, move |index, _window, cx| {
            let Some(row) = rows.get(index).cloned() else {
                return div().into_any_element();
            };
            let starts_turn = matches!(
                row,
                TimelineRow::Single(AgentEntry::User { .. })
                    | TimelineRow::Group {
                        kind: TimelineGroupKind::Reply,
                        ..
                    }
            );
            div()
                .w_full()
                .px_3()
                .pb_3()
                .when(index > 0 && starts_turn, |row| row.pt(px(TURN_GAP)))
                .child(
                    div()
                        .w_full()
                        .max_w(px(AGENT_CONTENT_MAX_WIDTH))
                        .mx_auto()
                        .child(render_timeline_row(
                            &store,
                            row,
                            copyable_assistant,
                            rewind.as_ref(),
                            live_row == Some(index),
                            open_output.as_ref(),
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
        TimelineRow::Single(AgentEntry::Assistant {
            id, aside: None, ..
        }) => Some(*id),
        TimelineRow::Group { entries, .. } => entries.iter().rev().find_map(|entry| match entry {
            AgentEntry::Assistant {
                id, aside: None, ..
            } => Some(*id),
            _ => None,
        }),
        TimelineRow::Single(_) => None,
    })
}

fn is_turn_row(row: &TimelineRow) -> bool {
    match row {
        TimelineRow::Group { kind, .. } => *kind == TimelineGroupKind::Turn,
        TimelineRow::Single(entry) => timeline_group_kind(entry) == Some(TimelineGroupKind::Turn),
    }
}

fn render_timeline_row(
    store: &Entity<AgentTimelineStore>,
    row: TimelineRow,
    copyable_assistant: Option<u64>,
    rewind: Option<&TimelineRewind>,
    live: bool,
    open_output: Option<&OpenOutputHandler>,
    cx: &mut App,
) -> AnyElement {
    match row {
        TimelineRow::Single(AgentEntry::User {
            id,
            markdown,
            images,
            rewind_id,
        }) => render_user_entry(store, id, markdown, &images, rewind.zip(rewind_id), cx),
        TimelineRow::Single(AgentEntry::Assistant {
            id,
            markdown,
            aside: Some(_),
        }) => render_notice(store, id, markdown, cx),
        TimelineRow::Single(entry) => {
            let id = entry.id();
            render_turn(
                store,
                id,
                std::slice::from_ref(&entry),
                copyable_assistant,
                live,
                open_output,
                cx,
            )
        }
        TimelineRow::Group {
            kind: TimelineGroupKind::Reply,
            entries,
            ..
        } => render_replied_prompt(store, &entries, rewind, cx),
        TimelineRow::Group { id, entries, .. } => render_turn(
            store,
            id,
            &entries,
            copyable_assistant,
            live,
            open_output,
            cx,
        ),
    }
}

struct Trace<'a> {
    members: &'a [AgentEntry],
    nesting: &'a StepNesting,
    steps: &'a [usize],
    answering: bool,
}

impl Trace<'_> {
    fn entries(&self) -> impl DoubleEndedIterator<Item = &AgentEntry> {
        self.steps.iter().map(|index| &self.members[*index])
    }

    fn tools(&self) -> impl DoubleEndedIterator<Item = &AgentToolEntry> {
        self.entries().filter_map(|entry| match entry {
            AgentEntry::Tool(tool) => Some(tool),
            _ => None,
        })
    }

    fn substeps(&self, id: u64) -> usize {
        self.nesting.steps.get(&id).map_or(0, Vec::len)
    }

    fn ending_failure(&self) -> Option<&AgentToolEntry> {
        self.tools()
            .next_back()
            .filter(|tool| tool.status == AgentToolStatus::Failed)
    }

    fn latest_thought(&self) -> Option<SharedString> {
        self.entries().rev().find_map(|entry| match entry {
            AgentEntry::Reasoning { markdown, .. } | AgentEntry::Assistant { markdown, .. } => {
                markdown.inspect(|text, _, _| thought_tail(text))
            }
            _ => None,
        })
    }
}

fn split_turn(members: &[AgentEntry], nesting: &StepNesting) -> (Vec<usize>, Vec<usize>) {
    let mut shown = nesting
        .top
        .iter()
        .copied()
        .filter(|index| !matches!(members[*index], AgentEntry::Plan { .. }))
        .collect::<Vec<_>>();
    let answer_start = shown
        .iter()
        .rposition(|index| !matches!(members[*index], AgentEntry::Assistant { .. }))
        .map_or(0, |last| last + 1);
    let answer = shown.split_off(answer_start);
    (shown, answer)
}

fn render_turn(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    members: &[AgentEntry],
    copyable_assistant: Option<u64>,
    live: bool,
    open_output: Option<&OpenOutputHandler>,
    cx: &mut App,
) -> AnyElement {
    let nesting = StepNesting::new(members);
    let (steps, answer) = split_turn(members, &nesting);
    let trace = Trace {
        members,
        nesting: &nesting,
        steps: &steps,
        answering: !answer.is_empty(),
    };
    v_flex()
        .id(("agent-turn", id))
        .w_full()
        .gap_3()
        .when(!steps.is_empty(), |turn| {
            turn.child(render_trace(store, id, &trace, live, open_output, cx))
        })
        .children(answer.iter().filter_map(|index| match &members[*index] {
            AgentEntry::Assistant { id, markdown, .. } => Some(render_answer(
                store,
                *id,
                markdown.clone(),
                copyable_assistant == Some(*id),
                cx,
            )),
            _ => None,
        }))
        .into_any_element()
}

fn render_answer(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    markdown: AgentMarkdown,
    copyable: bool,
    cx: &mut App,
) -> AnyElement {
    let copy = markdown.clone();
    v_flex()
        .id(("agent-assistant-entry", id))
        .w_full()
        .gap_1()
        .text_size(crate::rems_from_px(13.0))
        .child(assistant_markdown_view(store, id, markdown, cx))
        .when(copyable, |this| {
            this.child(
                h_flex().w_full().h(px(28.0)).items_center().child(
                    div()
                        .debug_selector(|| "agent-assistant-copy".to_owned())
                        .child(
                            Button::compact_icon(("agent-copy-assistant", id), IconName::Copy)
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

fn render_trace(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    trace: &Trace<'_>,
    live: bool,
    open_output: Option<&OpenOutputHandler>,
    cx: &mut App,
) -> AnyElement {
    let elapsed = store.read(cx).turn_elapsed(id);
    let expanded = store.update(cx, |store, _| {
        store.expanded(id, DisclosureKind::Turn, false)
    });
    let (mark, label) = trace_header(trace, live, elapsed, store, cx);
    let thought = (live && !expanded && !trace.answering)
        .then(|| trace.latest_thought())
        .flatten();
    let failure = (!live && !expanded)
        .then(|| trace.ending_failure())
        .flatten();
    let toggle = store.clone();
    v_flex()
        .id(("agent-trace", id))
        .w_full()
        .child(
            activity_row_marked(
                ("agent-trace-toggle", id),
                mark,
                label,
                RowEnd::Disclosure(expanded),
                cx,
            )
            .debug_selector(|| "agent-trace".to_owned())
            .on_click(move |_, _, cx| {
                toggle.update(cx, |store, cx| {
                    store.toggle_expanded(id, DisclosureKind::Turn, false, cx);
                });
            }),
        )
        .when_some(thought, |this, thought| {
            this.child(
                div()
                    .debug_selector(|| "agent-trace-thought".to_owned())
                    .pl(px(TRACE_INDENT))
                    .text_size(crate::rems_from_px(12.0))
                    .line_height(px(18.0))
                    .text_color(timeline_affordance_color(cx))
                    .line_clamp(2)
                    .child(thought),
            )
        })
        .when(expanded, |this| {
            this.child(render_trace_steps(store, trace, live, open_output, cx))
        })
        .when_some(failure, |this, tool| {
            this.child(render_failure(store, tool, open_output, cx))
        })
        .into_any_element()
}

const TRACE_INDENT: f32 = ACTIVITY_ROW_FONT_SIZE + 8.0;

fn trace_header(
    trace: &Trace<'_>,
    live: bool,
    elapsed: Option<Duration>,
    store: &Entity<AgentTimelineStore>,
    cx: &mut App,
) -> (ActivityMark, SharedString) {
    let mut mark = ActivityMark::bare();
    if live && !trace.answering {
        if let Some(waiting) = trace
            .tools()
            .find(|tool| tool.status == AgentToolStatus::NeedsApproval)
        {
            let warning = cx.theme().warning;
            mark.icon = Some(IconName::TriangleAlert);
            mark.tint = Some(warning);
            mark.label_color = Some(warning);
            mark.detail = Some(single_line(waiting.label.clone()));
            return (mark, "Waiting for you".into());
        }
        mark.spin = Some(spinner_phase(store.entity_id(), cx));
        let steps = trace.tools().count();
        mark.trailing = Some(match elapsed {
            Some(elapsed) if steps > 0 => {
                format!("{} · {}", step_count(steps), clock_label(elapsed)).into()
            }
            Some(elapsed) => clock_label(elapsed).into(),
            None => step_count(steps).into(),
        });
        let label = match trace.entries().next_back() {
            Some(AgentEntry::Tool(tool)) if tool_running(tool.status) => {
                mark.label_color = Some(cx.theme().foreground);
                single_line(tool.label.clone())
            }
            Some(AgentEntry::Reasoning { .. }) => "Thinking".into(),
            _ => "Working".into(),
        };
        return (mark, label);
    }
    mark.counts = trace_counts(trace);
    let thought_only = trace.tools().next().is_none();
    let label = match (thought_only, elapsed) {
        (true, Some(elapsed)) => format!("Thought for {}", duration_label(elapsed)),
        (true, None) => "Thought".to_owned(),
        (false, Some(elapsed)) => format!("Worked {}", duration_label(elapsed)),
        (false, None) => "Worked".to_owned(),
    };
    (mark, label.into())
}

fn step_count(count: usize) -> String {
    if count == 1 {
        "1 step".to_owned()
    } else {
        format!("{count} steps")
    }
}

fn trace_counts(trace: &Trace<'_>) -> Vec<TraceCount> {
    let mut counts = [0_usize; 7];
    let mut failed = 0;
    for tool in trace.tools() {
        if tool.status == AgentToolStatus::Failed {
            failed += 1;
        }
        let slot = if trace.substeps(tool.id) > 0 {
            6
        } else {
            match tool.kind {
                AgentToolKind::Read => 0,
                AgentToolKind::Search => 1,
                AgentToolKind::Edit => 2,
                AgentToolKind::Execute => 3,
                AgentToolKind::Fetch => 4,
                AgentToolKind::Other => 5,
                AgentToolKind::Think => continue,
            }
        };
        counts[slot] += 1;
    }
    const KINDS: [(IconName, (&str, &str)); 7] = [
        (IconName::File, ("read", "reads")),
        (IconName::Search, ("search", "searches")),
        (IconName::Pencil, ("edit", "edits")),
        (IconName::SquareTerminal, ("command", "commands")),
        (IconName::Globe, ("fetch", "fetches")),
        (IconName::Asterisk, ("tool call", "tool calls")),
        (IconName::Bot, ("agent", "agents")),
    ];
    KINDS
        .into_iter()
        .zip(counts)
        .map(|((icon, noun), count)| TraceCount {
            icon,
            count,
            noun,
            failed: false,
        })
        .chain((failed > 0).then_some(TraceCount {
            icon: IconName::TriangleAlert,
            count: failed,
            noun: ("failed", "failed"),
            failed: true,
        }))
        .filter(|count| count.count > 0)
        .collect()
}

fn clock_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn duration_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs().max(1);
    match (seconds / 3600, seconds / 60 % 60, seconds % 60) {
        (0, 0, seconds) => format!("{seconds}s"),
        (0, minutes, seconds) => format!("{minutes}m {seconds}s"),
        (hours, minutes, _) => format!("{hours}h {minutes}m"),
    }
}

fn thought_tail(text: &str) -> Option<SharedString> {
    const KEEP: usize = 220;
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return None;
    }
    if flat.len() <= KEEP {
        return Some(flat.into());
    }
    let mut start = flat.len() - KEEP;
    while !flat.is_char_boundary(start) {
        start += 1;
    }
    let tail = &flat[start..];
    let tail = tail.split_once(' ').map_or(tail, |(_, rest)| rest);
    Some(format!("…{tail}").into())
}

fn render_trace_steps(
    store: &Entity<AgentTimelineStore>,
    trace: &Trace<'_>,
    live: bool,
    open_output: Option<&OpenOutputHandler>,
    cx: &mut App,
) -> Div {
    v_flex()
        .debug_selector(|| "agent-trace-steps".to_owned())
        .w_full()
        .ml_1()
        .pl_4()
        .border_l_1()
        .border_color(cx.theme().border())
        .children(trace.steps.iter().map(|index| {
            match &trace.members[*index] {
                AgentEntry::Reasoning { id, markdown, .. } => {
                    render_thought(store, *id, markdown.clone(), cx)
                }
                AgentEntry::Assistant { id, markdown, .. } => div()
                    .py_1()
                    .text_size(crate::rems_from_px(12.0))
                    .text_color(timeline_affordance_color(cx))
                    .child(markdown_view(
                        store,
                        *id,
                        MarkdownSlot::Body,
                        markdown.clone(),
                        cx,
                    ))
                    .into_any_element(),
                AgentEntry::Tool(tool) => {
                    render_trace_tool(store, tool, trace.substeps(tool.id), live, open_output, cx)
                }
                AgentEntry::User { .. } | AgentEntry::Plan { .. } => div().into_any_element(),
            }
        }))
}

fn render_thought(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    markdown: AgentMarkdown,
    cx: &mut App,
) -> AnyElement {
    let empty = markdown.trim_is_empty();
    v_flex()
        .id(("agent-trace-thought", id))
        .w_full()
        .child(activity_row_marked(
            ("agent-trace-thought-row", id),
            ActivityMark::plain(IconName::Cpu),
            "Thought".into(),
            RowEnd::None,
            cx,
        ))
        .when(!empty, |this| {
            this.child(
                div()
                    .pl(px(TRACE_INDENT))
                    .pb_1()
                    .text_size(crate::rems_from_px(12.0))
                    .text_color(timeline_affordance_color(cx))
                    .child(markdown_view(store, id, MarkdownSlot::Body, markdown, cx)),
            )
        })
        .into_any_element()
}

fn render_trace_tool(
    store: &Entity<AgentTimelineStore>,
    tool: &AgentToolEntry,
    substeps: usize,
    live: bool,
    open_output: Option<&OpenOutputHandler>,
    cx: &mut App,
) -> AnyElement {
    let mut mark = tool_mark(tool, live, store, cx);
    if substeps > 0 {
        if mark.spin.is_none() {
            mark.icon = Some(IconName::Bot);
        }
        mark.trailing = Some(step_count(substeps).into());
    }
    let open = open_output.filter(|_| tool_opens_output(tool)).cloned();
    let row = activity_row_marked(
        ("agent-trace-tool", tool.id),
        mark,
        tool.label.clone(),
        if open.is_some() {
            RowEnd::Open
        } else {
            RowEnd::None
        },
        cx,
    );
    match open {
        Some(open) => {
            let tool = tool.clone();
            row.tooltip(|window, cx| {
                crate::tooltip::Tooltip::new("Open the output in a pane").build(window, cx)
            })
            .on_click(move |_, window, cx| open(&tool, window, cx))
            .into_any_element()
        }
        None => row.into_any_element(),
    }
}

fn render_failure(
    store: &Entity<AgentTimelineStore>,
    tool: &AgentToolEntry,
    open_output: Option<&OpenOutputHandler>,
    cx: &mut App,
) -> AnyElement {
    let output = tool_output_text(tool);
    let tail = output
        .lines()
        .rev()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    v_flex()
        .debug_selector(|| "agent-trace-failure".to_owned())
        .w_full()
        .child(render_trace_tool(store, tool, 0, false, open_output, cx))
        .children(tail.into_iter().rev().map(|line| {
            div()
                .pl(px(TRACE_INDENT))
                .h(px(18.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(crate::rems_from_px(11.0))
                .text_color(timeline_affordance_color(cx))
                .child(line)
        }))
        .into_any_element()
}

#[must_use]
pub fn tool_opens_output(tool: &AgentToolEntry) -> bool {
    tool.kind == AgentToolKind::Execute
        && !tool_running(tool.status)
        && tool.output.iter().any(|payload| match payload {
            AgentToolPayload::Text(text) | AgentToolPayload::Terminal(text) => {
                text.inspect(|text| !text.trim().is_empty())
            }
            AgentToolPayload::Json(_) | AgentToolPayload::Diff { .. } => false,
        })
}

#[must_use]
pub fn tool_output_text(tool: &AgentToolEntry) -> String {
    let mut output = String::new();
    for payload in tool.output.iter() {
        if let AgentToolPayload::Text(text) | AgentToolPayload::Terminal(text) = payload {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            text.inspect(|text| output.push_str(text));
        }
    }
    output
}

fn render_replied_prompt(
    store: &Entity<AgentTimelineStore>,
    entries: &[AgentEntry],
    rewind: Option<&TimelineRewind>,
    cx: &mut App,
) -> AnyElement {
    let Some((
        AgentEntry::User {
            id,
            markdown,
            images,
            rewind_id,
        },
        replies,
    )) = entries.split_first()
    else {
        return div().into_any_element();
    };
    let prompt = render_user_entry(
        store,
        *id,
        markdown.clone(),
        images,
        rewind.zip(rewind_id.clone()),
        cx,
    );
    v_flex()
        .w_full()
        .items_end()
        .gap(px(CHROME_GAP))
        .child(prompt)
        .children(replies.iter().filter_map(|entry| match entry {
            AgentEntry::Assistant {
                id,
                markdown,
                aside,
            } => Some(reply_popover(
                store,
                *id,
                markdown.clone(),
                aside.is_some_and(|aside| aside.side),
                cx,
            )),
            _ => None,
        }))
        .into_any_element()
}

fn reply_popover(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    markdown: AgentMarkdown,
    side: bool,
    cx: &mut App,
) -> Div {
    v_flex()
        .debug_selector(|| "agent-reply-popover".to_owned())
        .max_w(relative(0.85))
        .gap_1()
        .px_3()
        .py_2()
        .popover_style(cx)
        .text_size(crate::rems_from_px(13.0))
        .when(side, |popover| {
            popover.child(
                div()
                    .text_size(crate::rems_from_px(10.0))
                    .text_color(cx.theme().foreground.muted())
                    .child("Side answer · not in the conversation"),
            )
        })
        .child(markdown_view(store, id, MarkdownSlot::Body, markdown, cx))
}

fn render_notice(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    markdown: AgentMarkdown,
    cx: &mut App,
) -> AnyElement {
    h_flex()
        .id(("agent-notice-entry", id))
        .debug_selector(|| "agent-notice".to_owned())
        .w_full()
        .items_start()
        .gap_2()
        .text_size(crate::rems_from_px(12.0))
        .text_color(timeline_affordance_color(cx))
        .child(
            div()
                .flex_none()
                .h(px(ACTIVITY_ROW_HEIGHT - 8.0))
                .flex()
                .items_center()
                .child(Icon::new(IconName::Info).size(px(ACTIVITY_ROW_FONT_SIZE))),
        )
        .child(div().flex_1().min_w_0().child(markdown_view(
            store,
            id,
            MarkdownSlot::Body,
            markdown,
            cx,
        )))
        .into_any_element()
}

struct StepNesting {
    top: Vec<usize>,
    steps: HashMap<u64, Vec<usize>>,
}

impl StepNesting {
    fn new(members: &[AgentEntry]) -> Self {
        let positions = members
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.id(), index))
            .collect::<HashMap<_, _>>();
        let parent_of = |index: usize| {
            timeline_parent(&members[index]).and_then(|parent| positions.get(&parent).copied())
        };
        let reaches_top = |index: usize| {
            let mut visited = HashSet::new();
            let mut at = index;
            loop {
                if !visited.insert(at) {
                    return false;
                }
                match parent_of(at) {
                    Some(parent) => at = parent,
                    None => return true,
                }
            }
        };
        let mut top = Vec::new();
        let mut steps = HashMap::<u64, Vec<usize>>::new();
        for index in 0..members.len() {
            match parent_of(index).filter(|_| reaches_top(index)) {
                Some(parent) => steps.entry(members[parent].id()).or_default().push(index),
                None => top.push(index),
            }
        }
        Self { top, steps }
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
        .border_color(cx.theme().border())
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

fn activity_row_glyph(icon: IconName, size: f32) -> Div {
    div()
        .flex_none()
        .relative()
        .top(px(ACTIVITY_ICON_OPTICAL_DROP))
        .child(Icon::new(icon).size(px(size)))
}

struct ActivityMark {
    icon: Option<IconName>,
    spin: Option<f32>,
    tint: Option<Hsla>,
    label_color: Option<Hsla>,
    detail: Option<SharedString>,
    counts: Vec<TraceCount>,
    tag: Option<(SharedString, Hsla)>,
    trailing: Option<SharedString>,
}

impl ActivityMark {
    const fn bare() -> Self {
        Self {
            icon: None,
            spin: None,
            tint: None,
            label_color: None,
            detail: None,
            counts: Vec::new(),
            tag: None,
            trailing: None,
        }
    }

    const fn plain(icon: IconName) -> Self {
        let mut mark = Self::bare();
        mark.icon = Some(icon);
        mark
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TraceCount {
    icon: IconName,
    count: usize,
    noun: (&'static str, &'static str),
    failed: bool,
}

impl TraceCount {
    fn describe(&self) -> String {
        let (one, many) = self.noun;
        format!(
            "{} {}",
            self.count,
            if self.count == 1 { one } else { many }
        )
    }
}

const fn tool_running(status: AgentToolStatus) -> bool {
    matches!(status, AgentToolStatus::Pending | AgentToolStatus::Running)
}

fn tool_mark(
    tool: &AgentToolEntry,
    live: bool,
    store: &Entity<AgentTimelineStore>,
    cx: &mut App,
) -> ActivityMark {
    let mut mark = ActivityMark::plain(tool_icon(tool.kind));
    match tool.status {
        status if tool_running(status) && live => {
            mark.spin = Some(spinner_phase(store.entity_id(), cx));
        }
        AgentToolStatus::NeedsApproval => mark.tint = Some(cx.theme().warning),
        AgentToolStatus::Failed => {
            let danger = cx.theme().danger;
            mark.tint = Some(danger);
            let tag = tool
                .exit_code
                .filter(|code| *code != 0)
                .map_or_else(|| "failed".to_owned(), |code| format!("exit {code}"));
            mark.tag = Some((tag.into(), danger));
        }
        AgentToolStatus::Canceled => {
            mark.tag = Some(("canceled".into(), timeline_affordance_color(cx)));
        }
        _ => {}
    }
    mark
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowEnd {
    None,
    Disclosure(bool),
    Open,
}

/// The one row shape every activity wears: the trace line, a thought, a tool
/// call. One builder so the font, the metrics and the hover can never drift
/// apart between the callers.
fn activity_row_marked(
    id: impl Into<ElementId>,
    mark: ActivityMark,
    label: SharedString,
    end: RowEnd,
    cx: &App,
) -> Stateful<Div> {
    let foreground = cx.theme().foreground;
    let glyph = match (mark.spin, mark.icon) {
        (Some(phase), _) => Some(spinner(phase)),
        (None, Some(icon)) => Some(Icon::new(icon)),
        (None, None) => None,
    }
    .map(|glyph| {
        let glyph = glyph.size(px(ACTIVITY_ROW_FONT_SIZE));
        match mark.tint {
            Some(tint) => glyph.text_color(tint),
            None => glyph,
        }
    });
    let end_icon = match end {
        RowEnd::None => None,
        RowEnd::Disclosure(expanded) => Some(disclosure_icon(expanded)),
        RowEnd::Open => Some(IconName::ExternalLink),
    };
    h_flex()
        .id(id)
        .w_full()
        .h(px(ACTIVITY_ROW_HEIGHT))
        .flex_none()
        .gap_2()
        .overflow_hidden()
        .font_family(cx.theme().font_family.clone())
        .text_size(crate::rems_from_px(ACTIVITY_ROW_FONT_SIZE))
        .line_height(px(ACTIVITY_ROW_LINE_HEIGHT))
        .text_color(timeline_affordance_color(cx))
        .when(end != RowEnd::None, |this| {
            this.cursor_pointer()
                .hover(move |this| this.text_color(foreground))
        })
        .when_some(glyph, |this, glyph| {
            this.child(
                div()
                    .debug_selector(|| "agent-activity-glyph".to_owned())
                    .flex_none()
                    .relative()
                    .top(px(ACTIVITY_ICON_OPTICAL_DROP))
                    .child(glyph),
            )
        })
        .child(
            div()
                .debug_selector(|| "agent-activity-label".to_owned())
                .min_w_0()
                .flex_shrink_0()
                .max_w_full()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .when_some(mark.label_color, gpui::Styled::text_color)
                .child(single_line(label)),
        )
        .when_some(mark.detail, |this, detail| {
            this.child(
                div()
                    .debug_selector(|| "agent-activity-detail".to_owned())
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(timeline_affordance_color(cx).opacity(0.7))
                    .child(detail),
            )
        })
        .when(!mark.counts.is_empty(), |this| {
            this.child(
                h_flex()
                    .debug_selector(|| "agent-activity-counts".to_owned())
                    .flex_none()
                    .gap(px(TRACE_COUNT_GAP))
                    .pl_1()
                    .pr(px(TRACE_COUNT_BADGE_OVERHANG))
                    .children(
                        mark.counts
                            .into_iter()
                            .enumerate()
                            .map(|(slot, count)| trace_count_glyph(slot, count, cx)),
                    ),
            )
        })
        .when_some(mark.tag, |this, (tag, color)| {
            this.child(
                div()
                    .debug_selector(|| "agent-activity-tag".to_owned())
                    .flex_none()
                    .text_size(crate::rems_from_px(12.0))
                    .text_color(color)
                    .child(tag),
            )
        })
        .when_some(end_icon, |this, icon| {
            this.child(
                activity_row_glyph(icon, ACTIVITY_DISCLOSURE_SIZE)
                    .debug_selector(|| "agent-activity-chevron".to_owned()),
            )
        })
        .when_some(mark.trailing, |this, trailing| {
            this.child(div().flex_1()).child(
                div()
                    .debug_selector(|| "agent-activity-trailing".to_owned())
                    .flex_none()
                    .text_size(crate::rems_from_px(11.0))
                    .text_color(timeline_affordance_color(cx).opacity(0.7))
                    .child(trailing),
            )
        })
}

const TRACE_COUNT_BADGE: f32 = 15.0;
const TRACE_COUNT_RING: f32 = 1.5;
const TRACE_COUNT_BADGE_LEFT: f32 = 7.5;
const TRACE_COUNT_BADGE_OVERHANG: f32 =
    TRACE_COUNT_BADGE_LEFT + TRACE_COUNT_BADGE - ACTIVITY_ROW_FONT_SIZE;
const TRACE_COUNT_GAP: f32 = TRACE_COUNT_BADGE_OVERHANG + 6.0;

fn trace_count_glyph(slot: usize, count: TraceCount, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (badge, number) = if count.failed {
        (theme.danger, theme.background)
    } else {
        (theme.background.raised(3), theme.foreground)
    };
    let tooltip = SharedString::from(count.describe());
    div()
        .id(("agent-trace-count", slot))
        .debug_selector(|| "agent-trace-count".to_owned())
        .flex_none()
        .relative()
        .top(px(ACTIVITY_ICON_OPTICAL_DROP))
        .when(count.failed, |this| this.text_color(theme.danger))
        .child(Icon::new(count.icon).size(px(ACTIVITY_ROW_FONT_SIZE)))
        .child(
            h_flex()
                .debug_selector(|| "agent-trace-count-badge".to_owned())
                .absolute()
                .top(px(-6.5))
                .left(px(TRACE_COUNT_BADGE_LEFT))
                .min_w(px(TRACE_COUNT_BADGE))
                .h(px(TRACE_COUNT_BADGE))
                .px(px(3.0))
                .justify_center()
                .rounded_full()
                .border(px(TRACE_COUNT_RING))
                .border_color(theme.background)
                .bg(badge)
                .text_color(number)
                .text_size(crate::rems_from_px(8.5))
                .line_height(px(TRACE_COUNT_BADGE - 2.0 * TRACE_COUNT_RING))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(count.count.to_string()),
        )
        .tooltip(move |window, cx| crate::tooltip::Tooltip::new(tooltip.clone()).build(window, cx))
}

const USER_ENTRY_GROUP: &str = "agent-user-entry";

fn render_user_entry(
    store: &Entity<AgentTimelineStore>,
    id: u64,
    markdown: AgentMarkdown,
    images: &[Arc<Image>],
    rewind: Option<(&TimelineRewind, SharedString)>,
    cx: &mut App,
) -> AnyElement {
    let bubble = v_flex()
        .debug_selector(|| "agent-user-bubble".to_owned())
        .max_w(relative(1.0))
        .gap_2()
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .bg(cx.theme().background.raised(1))
        .text_size(crate::rems_from_px(13.0))
        .when(!images.is_empty(), |this| {
            this.child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .children(images.iter().enumerate().map(|(index, image)| {
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
                    })),
            )
        })
        .when(!markdown.is_empty(), |this| {
            this.child(markdown_view(store, id, MarkdownSlot::Body, markdown, cx))
        });
    let entry = v_flex().id(("agent-user-entry", id)).w_full().items_end();
    let Some((rewind, message_id)) = rewind else {
        return entry.child(bubble).into_any_element();
    };
    entry
        .group(USER_ENTRY_GROUP)
        .child(
            h_flex()
                .w_full()
                .justify_end()
                .items_center()
                .gap_1()
                .child(rewind_button(id, rewind, message_id))
                .child(bubble.min_w_0()),
        )
        .into_any_element()
}

fn rewind_button(id: u64, rewind: &TimelineRewind, message_id: SharedString) -> Div {
    let handler = Rc::clone(&rewind.handler);
    div()
        .flex_none()
        .invisible()
        .when(rewind.enabled, |slot| {
            slot.group_hover(USER_ENTRY_GROUP, gpui::Styled::visible)
        })
        .child(
            div()
                .debug_selector(|| "agent-user-rewind".to_owned())
                .child(
                Button::compact_icon(("agent-rewind", id), IconName::History)
                    .tooltip(
                        "Rewind to here: continue from before this prompt. Files stay as they are.",
                    )
                    .on_click(move |_, window, cx| handler(&message_id, window, cx)),
            ),
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlanItemState {
    Pending,
    InProgress,
    Done,
}

pub(crate) fn plan_items(source: &str) -> Vec<(PlanItemState, &str)> {
    source
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("- [")?;
            let mut characters = rest.chars();
            let state = match characters.next()? {
                'x' | 'X' => PlanItemState::Done,
                '~' => PlanItemState::InProgress,
                _ => PlanItemState::Pending,
            };
            let text = characters.as_str().strip_prefix(']')?.trim();
            Some((state, text))
        })
        .collect()
}

#[must_use]
pub fn plan_progress(source: &str) -> (usize, usize, Option<&str>) {
    let items = plan_items(source);
    let done = items
        .iter()
        .filter(|(state, _)| *state == PlanItemState::Done)
        .count();
    let current = items
        .iter()
        .find(|(state, _)| *state == PlanItemState::InProgress)
        .or_else(|| {
            items
                .iter()
                .find(|(state, _)| *state == PlanItemState::Pending)
        })
        .map(|(_, text)| *text);
    (done, items.len(), current)
}

pub(crate) fn render_plan_items(source: &str, cx: &App) -> Div {
    const MARKER: f32 = 12.0;
    const LINE: f32 = 19.0;
    let foreground = cx.theme().foreground;
    let muted = cx.theme().foreground.muted();
    let accent = cx.theme().accent;
    v_flex()
        .w_full()
        .text_size(crate::rems_from_px(12.0))
        .line_height(px(LINE))
        .children(plan_items(source).into_iter().map(|(state, text)| {
            let marker = div()
                .size(px(MARKER))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(cx.theme().radius / 2.0)
                .border_1();
            let marker = match state {
                PlanItemState::Pending => marker.border_color(muted),
                PlanItemState::InProgress => marker
                    .border_color(accent)
                    .child(div().size(px(6.0)).rounded(px(1.5)).bg(accent)),
                PlanItemState::Done => marker.border_color(foreground).bg(foreground).child(
                    Icon::new(IconName::Check)
                        .size(px(MARKER * 0.65))
                        .text_color(foreground.on()),
                ),
            };
            h_flex()
                .w_full()
                .items_start()
                .gap(px(CHROME_GAP))
                .child(
                    div()
                        .h(px(LINE))
                        .flex_none()
                        .flex()
                        .items_center()
                        .relative()
                        .top(px(ACTIVITY_ICON_OPTICAL_DROP))
                        .child(marker),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .when(state == PlanItemState::Done, |text| {
                            text.text_color(muted).line_through()
                        })
                        .when(state == PlanItemState::InProgress, |text| {
                            text.font_weight(FontWeight::MEDIUM)
                        })
                        .child(text.to_owned()),
                )
        }))
}

const fn disclosure_icon(expanded: bool) -> IconName {
    if expanded {
        IconName::ChevronUp
    } else {
        IconName::ChevronRight
    }
}

fn tool_icon(kind: AgentToolKind) -> IconName {
    match kind {
        AgentToolKind::Read => IconName::File,
        AgentToolKind::Edit => IconName::Pencil,
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
                Button::compact_icon(("agent-copy-code", source_offset), IconName::Copy)
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

fn standard_markdown_extensions() -> MarkdownExtensions {
    static EXTENSIONS: OnceLock<MarkdownExtensions> = OnceLock::new();
    EXTENSIONS
        .get_or_init(|| MarkdownExtensions::default().plugin(MermaidPlugin))
        .clone()
}

fn assistant_markdown_extensions() -> MarkdownExtensions {
    static EXTENSIONS: OnceLock<MarkdownExtensions> = OnceLock::new();
    EXTENSIONS
        .get_or_init(|| {
            MarkdownExtensions::default()
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

        let preview_image = match &render_state.read(cx).result {
            MermaidRenderResult::Ready(image) => Some(Arc::clone(image)),
            _ => None,
        };
        let mermaid_source = source.source.clone();
        let source_offset = source.source_offset;
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
            .relative()
            .w_full()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border())
            .bg(cx.theme().background)
            .overflow_hidden()
            .child(content)
            .child(
                div().absolute().top_1().right_1().child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::compact_icon(
                                ("agent-mermaid-copy", source_offset),
                                IconName::Copy,
                            )
                            .tooltip("Copy source")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    mermaid_source.clone(),
                                ));
                            }),
                        )
                        .when_some(preview_image, |this, image| {
                            this.child(
                                Button::compact_icon(
                                    ("agent-mermaid-preview", source_offset),
                                    IconName::WindowMaximize,
                                )
                                .tooltip("Open full diagram")
                                .on_click(move |_, window, cx| {
                                    open_render_image_preview(Arc::clone(&image), window, cx);
                                }),
                            )
                        }),
                ),
            )
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
                border: cx.theme().border(),
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
        cx.theme().border(),
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

pub const AGENT_HEADER_HEIGHT: f32 = 36.0;
/// The agent pane's chrome bar: `leading` and `trailing` pinned to each end.
/// The bar owns only the height, the inset, and the rule beneath it.
pub fn agent_pane_header(
    active: bool,
    leading: impl IntoElement,
    status: Option<AnyElement>,
    trailing: impl IntoElement,
    show_separator: bool,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .id("agent-pane-header")
        .group("agent-pane-header")
        .w_full()
        .h(px(AGENT_HEADER_HEIGHT))
        .flex_none()
        .items_center()
        .justify_between()
        .gap_3()
        .px(px(CHROME_GAP))
        .when(show_separator, |header| {
            header.border_b_1().border_color(cx.theme().border())
        })
        .child(div().flex_1().min_w_0().overflow_hidden().child(leading))
        .children(status)
        .child(
            h_flex()
                .flex_none()
                .when(!active, |actions| {
                    actions
                        .invisible()
                        .group_hover("agent-pane-header", gpui::Styled::visible)
                })
                .child(trailing),
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentPaneStatus {
    Running,
    Stopping,
    Waiting,
    Exited,
    Offline,
}

pub fn agent_status_pill(
    id: impl Into<ElementId>,
    status: AgentPaneStatus,
    phase: f32,
    restart: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (icon, label, color, fill) = match status {
        AgentPaneStatus::Running => (
            None,
            "Running",
            theme.foreground,
            theme.background.washed(2),
        ),
        AgentPaneStatus::Stopping => (
            None,
            "Stopping",
            theme.foreground.muted(),
            theme.background.washed(2),
        ),
        AgentPaneStatus::Waiting => (
            Some(IconName::TriangleAlert),
            "Waiting for you",
            theme.warning,
            theme.warning.opacity(0.12),
        ),
        AgentPaneStatus::Exited => (
            Some(IconName::CircleX),
            "Exited",
            theme.danger,
            theme.danger.opacity(0.12),
        ),
        AgentPaneStatus::Offline => (
            None,
            "Offline",
            theme.foreground.muted(),
            theme.background.washed(2),
        ),
    };
    let spinning = matches!(status, AgentPaneStatus::Running | AgentPaneStatus::Stopping);
    let foreground = theme.foreground;
    let divider = theme.foreground.opacity(0.1);
    h_flex()
        .id(id)
        .debug_selector(|| "agent-status-pill".to_owned())
        .flex_none()
        .h(px(22.0))
        .px_2()
        .gap_1()
        .rounded(px(11.0))
        .bg(fill)
        .text_size(crate::rems_from_px(11.0))
        .line_height(px(14.0))
        .text_color(color)
        .when(spinning, |pill| pill.child(spinner(phase).size(px(11.0))))
        .when_some(icon, |pill, icon| {
            pill.child(Icon::new(icon).size(px(11.0)))
        })
        .child(label)
        .when_some(restart, |pill, restart| {
            pill.child(
                div()
                    .id("agent-status-restart")
                    .debug_selector(|| "agent-status-restart".to_owned())
                    .pl_1p5()
                    .ml_0p5()
                    .border_l_1()
                    .border_color(divider)
                    .text_color(foreground)
                    .cursor_pointer()
                    .hover(gpui::Styled::underline)
                    .child("Restart")
                    .on_click(move |_, window, cx| restart(window, cx)),
            )
        })
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
        rewinds: Option<Rc<std::cell::RefCell<Vec<SharedString>>>>,
    }

    impl Render for UserEntryTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let timeline = AgentTimeline::new(
                Arc::new(vec![TimelineRow::Single(self.entry.clone())]),
                ListState::new(1, gpui::ListAlignment::Top, px(600.0)),
                self.store.clone(),
            )
            .active_turn(self.active_turn);
            let timeline = match self.rewinds.clone() {
                Some(rewinds) => timeline.rewind(true, move |id, _, _| {
                    rewinds.borrow_mut().push(id.clone());
                }),
                None => timeline,
            };
            div().w(self.pane_width).h(px(600.0)).child(timeline)
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
            parent: None,
            exit_code: None,
        }
    }

    #[test]
    fn zz_replies_fold_into_their_prompt_row() {
        let entries = [
            AgentEntry::User {
                id: 1,
                markdown: "/btw why".into(),
                images: Arc::from([]),
                rewind_id: None,
            },
            test_tool(2, "Read file", AgentToolStatus::Running),
            AgentEntry::Assistant {
                id: 3,
                markdown: "because".into(),
                aside: Some(AgentAside {
                    side: true,
                    reply_to: Some(1),
                }),
            },
            AgentEntry::Assistant {
                id: 4,
                markdown: "Forked.".into(),
                aside: Some(AgentAside {
                    side: false,
                    reply_to: None,
                }),
            },
        ];
        let folded = fold_timeline_rows(&entries).rows;
        let mut appended = Vec::new();
        for entry in &entries {
            let _ = append_timeline_row(&mut appended, entry.clone());
        }
        assert_eq!(*folded, appended);
        assert_eq!(folded.len(), 3);
        assert!(matches!(
            &folded[0],
            TimelineRow::Group {
                kind: TimelineGroupKind::Reply,
                entries,
                ..
            } if entries.len() == 2
        ));
        assert_eq!(final_assistant_entry_id(&folded), None);
    }

    #[test]
    fn plan_items_give_the_item_in_progress_its_own_state() {
        assert_eq!(
            plan_items("- [x] a\n- [~] b\n- [ ] c\nnot an item"),
            [
                (PlanItemState::Done, "a"),
                (PlanItemState::InProgress, "b"),
                (PlanItemState::Pending, "c"),
            ]
        );
    }

    fn test_assistant(id: u64, text: &str) -> AgentEntry {
        AgentEntry::Assistant {
            id,
            markdown: text.into(),
            aside: None,
        }
    }

    fn test_reasoning(id: u64) -> AgentEntry {
        AgentEntry::Reasoning {
            id,
            label: "Reasoning".into(),
            markdown: format!("thought {id}").into(),
            default_expanded: false,
        }
    }

    #[test]
    fn a_turn_folds_into_one_row_until_the_next_prompt() {
        let entries = vec![
            test_reasoning(1),
            test_tool(2, "Edit a.rs", AgentToolStatus::Completed),
            test_assistant(3, "now the tests"),
            AgentEntry::Plan {
                id: 4,
                markdown: "- [x] edit\n- [~] test".into(),
            },
            test_tool_kind(
                5,
                "cargo test",
                AgentToolKind::Execute,
                AgentToolStatus::Failed,
            ),
            test_assistant(6, "done"),
            AgentEntry::User {
                id: 7,
                markdown: "again".into(),
                images: Arc::from([]),
                rewind_id: None,
            },
            test_tool_kind(
                8,
                "Read a.rs",
                AgentToolKind::Read,
                AgentToolStatus::Completed,
            ),
        ];

        let folded = fold_timeline_rows(&entries);
        let mut appended = Vec::new();
        for entry in &entries {
            let _ = append_timeline_row(&mut appended, entry.clone());
        }

        assert_eq!(*folded.rows, appended);
        assert_eq!(folded.entry_to_row, [0, 0, 0, 0, 0, 0, 1, 2]);
        assert!(matches!(
            &folded.rows[0],
            TimelineRow::Group {
                kind: TimelineGroupKind::Turn,
                id: 1,
                entries
            } if entries.len() == 6
        ));
        assert!(is_turn_row(&folded.rows[2]));
        assert!(!is_turn_row(&folded.rows[1]));
        assert_eq!(final_assistant_entry_id(&folded.rows), Some(6));
    }

    #[test]
    fn a_turn_splits_into_its_trace_and_the_messages_that_close_it() {
        let members = [
            test_reasoning(1),
            test_assistant(2, "first"),
            test_tool(3, "Edit a.rs", AgentToolStatus::Completed),
            AgentEntry::Plan {
                id: 4,
                markdown: "- [x] edit".into(),
            },
            test_assistant(5, "done"),
            test_assistant(6, "and one more thing"),
        ];
        let nesting = StepNesting::new(&members);
        assert_eq!(split_turn(&members, &nesting), (vec![0, 1, 2], vec![4, 5]));

        let still_working = &members[..3];
        let nesting = StepNesting::new(still_working);
        assert_eq!(
            split_turn(still_working, &nesting),
            (vec![0, 1, 2], Vec::new()),
            "text followed by a tool call is narration"
        );

        let answer_only = [test_assistant(1, "hi")];
        let nesting = StepNesting::new(&answer_only);
        assert_eq!(split_turn(&answer_only, &nesting), (Vec::new(), vec![0]));
    }

    #[test]
    fn the_trace_counts_each_kind_once_and_agents_by_their_steps() {
        let members = [
            test_tool_kind(
                1,
                "Read a.rs",
                AgentToolKind::Read,
                AgentToolStatus::Completed,
            ),
            test_tool_kind(
                2,
                "Read b.rs",
                AgentToolKind::Read,
                AgentToolStatus::Completed,
            ),
            test_tool_kind(
                3,
                "cargo test",
                AgentToolKind::Execute,
                AgentToolStatus::Failed,
            ),
            test_tool(4, "Edit a.rs", AgentToolStatus::Completed),
            test_tool_kind(
                5,
                "Survey",
                AgentToolKind::Other,
                AgentToolStatus::Completed,
            ),
            test_step(6, 5),
            test_reasoning(7),
        ];
        let nesting = StepNesting::new(&members);
        let (steps, answer) = split_turn(&members, &nesting);
        let trace = Trace {
            members: &members,
            nesting: &nesting,
            steps: &steps,
            answering: !answer.is_empty(),
        };
        assert_eq!(
            trace_counts(&trace)
                .iter()
                .map(TraceCount::describe)
                .collect::<Vec<_>>(),
            ["2 reads", "1 edit", "1 command", "1 agent", "1 failed"]
        );
        assert_eq!(trace.ending_failure().map(|tool| tool.id), None);
        assert_eq!(trace.latest_thought().as_deref(), Some("thought 7"));

        let failing = &members[..3];
        let nesting = StepNesting::new(failing);
        let (steps, _) = split_turn(failing, &nesting);
        let trace = Trace {
            members: failing,
            nesting: &nesting,
            steps: &steps,
            answering: false,
        };
        assert_eq!(trace.ending_failure().map(|tool| tool.id), Some(3));
    }

    #[test]
    fn trace_times_read_as_a_clock_while_live_and_words_after() {
        assert_eq!(clock_label(Duration::from_secs(42)), "0:42");
        assert_eq!(clock_label(Duration::from_secs(134)), "2:14");
        assert_eq!(duration_label(Duration::from_millis(300)), "1s");
        assert_eq!(duration_label(Duration::from_secs(38)), "38s");
        assert_eq!(duration_label(Duration::from_secs(134)), "2m 14s");
        assert_eq!(duration_label(Duration::from_mins(63)), "1h 3m");
    }

    #[test]
    fn the_live_thought_keeps_the_end_of_the_text_on_one_line() {
        assert_eq!(thought_tail("  \n "), None);
        assert_eq!(
            thought_tail("one\n\ntwo   three").as_deref(),
            Some("one two three")
        );
        let long = "word ".repeat(100);
        let tail = thought_tail(&long).expect("text");
        assert!(tail.starts_with('…'));
        assert!(tail.len() <= 230);
        assert!(tail.ends_with("word"));
    }

    #[test]
    fn only_finished_commands_with_output_open_in_a_pane() {
        let mut tool = test_tool_entry(
            1,
            "cargo test",
            AgentToolKind::Execute,
            AgentToolStatus::Failed,
        );
        assert!(!tool_opens_output(&tool), "nothing printed");
        tool.output = Arc::from([
            AgentToolPayload::Json("{}".into()),
            AgentToolPayload::Terminal("running 3 tests".into()),
            AgentToolPayload::Text("error: 1 failed".into()),
        ]);
        assert!(tool_opens_output(&tool));
        assert_eq!(tool_output_text(&tool), "running 3 tests\nerror: 1 failed");
        tool.status = AgentToolStatus::Running;
        assert!(!tool_opens_output(&tool), "still running");
        tool.status = AgentToolStatus::Completed;
        tool.kind = AgentToolKind::Read;
        assert!(!tool_opens_output(&tool), "reads open nowhere yet");
    }

    fn test_step(id: u64, parent: u64) -> AgentEntry {
        let mut tool = test_tool_entry(
            id,
            &format!("Read {id}.rs"),
            AgentToolKind::Read,
            AgentToolStatus::Completed,
        );
        tool.parent = Some(parent);
        AgentEntry::Tool(tool)
    }

    #[test]
    fn subagent_steps_nest_under_their_agent_inside_the_turn() {
        let entries = vec![
            test_tool_kind(1, "Survey", AgentToolKind::Think, AgentToolStatus::Running),
            test_step(2, 1),
            test_assistant(3, "meanwhile"),
            test_step(4, 1),
            test_step(5, 99),
            test_tool_kind(7, "Survey", AgentToolKind::Think, AgentToolStatus::Running),
            test_step(8, 7),
        ];

        let folded = fold_timeline_rows(&entries);

        assert_eq!(folded.entry_to_row, [0; 7]);
        let TimelineRow::Group { id: 1, entries, .. } = &folded.rows[0] else {
            panic!("the turn is one row: {:?}", folded.rows[0]);
        };
        assert_eq!(
            entries.iter().map(AgentEntry::id).collect::<Vec<_>>(),
            [1, 3, 5, 7, 2, 4, 8],
            "steps follow the turn's own rows, in arrival order"
        );
        let nesting = StepNesting::new(entries);
        assert_eq!(
            nesting.top,
            [0, 1, 2, 3],
            "a step whose agent is unknown stands alone"
        );
        assert_eq!(nesting.steps[&1], [4, 5]);
        assert_eq!(nesting.steps[&7], [6]);
    }

    #[test]
    fn a_step_listed_before_its_agent_still_nests_but_a_loop_stays_flat() {
        let members = [
            test_step(1, 2),
            test_tool_kind(2, "Survey", AgentToolKind::Think, AgentToolStatus::Running),
            test_step(3, 1),
        ];
        let nesting = StepNesting::new(&members);
        assert_eq!(nesting.top, [1]);
        assert_eq!(nesting.steps[&2], [0]);
        assert_eq!(nesting.steps[&1], [2]);

        let mut agent =
            test_tool_entry(2, "Survey", AgentToolKind::Think, AgentToolStatus::Running);
        agent.parent = Some(1);
        let members = [
            test_step(1, 2),
            AgentEntry::Tool(agent),
            test_step(3, 1),
            test_step(4, 4),
        ];
        let nesting = StepNesting::new(&members);
        assert_eq!(nesting.top, [0, 1, 2, 3], "a loop renders every row flat");
        assert!(nesting.steps.is_empty());
    }

    struct TimelineRowsTest {
        store: Entity<AgentTimelineStore>,
        rows: Arc<Vec<TimelineRow>>,
    }

    impl Render for TimelineRowsTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(520.0)).h(px(600.0)).child(AgentTimeline::new(
                self.rows.clone(),
                ListState::new(self.rows.len(), gpui::ListAlignment::Top, px(600.0)),
                self.store.clone(),
            ))
        }
    }

    #[gpui::test]
    fn a_turn_shows_one_trace_line_until_opened(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let rows = fold_timeline_rows(&[
            test_tool_kind(
                1,
                "Survey",
                AgentToolKind::Other,
                AgentToolStatus::Completed,
            ),
            test_step(2, 1),
            test_step(3, 1),
            test_tool_kind(
                4,
                "Read a.rs",
                AgentToolKind::Read,
                AgentToolStatus::Completed,
            ),
            test_assistant(5, "done"),
        ])
        .rows;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| TimelineRowsTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                rows,
            });
            crate::Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let trace = cx
            .debug_bounds("agent-trace")
            .expect("the trace line should be painted");
        assert!(trace.size.height <= px(ACTIVITY_ROW_HEIGHT + 1.0));
        assert!(cx.debug_bounds("agent-trace-steps").is_none());
        assert!(
            cx.debug_bounds("agent-assistant-copy").is_some(),
            "the answer stays out"
        );

        cx.simulate_click(trace.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let steps = cx
            .debug_bounds("agent-trace-steps")
            .expect("the open trace lists its steps");
        assert!(steps.size.height >= px(ACTIVITY_ROW_HEIGHT * 2.0));
        assert!(steps.top() >= trace.bottom());
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
                aside: None,
            }),
            TimelineRow::Single(test_tool(21, "Read file", AgentToolStatus::Completed)),
            TimelineRow::Single(AgentEntry::Assistant {
                id: 22,
                markdown: "second".into(),
                aside: None,
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
            aside: None,
        };
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width: px(520.0),
                active_turn: false,
                rewinds: None,
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
            aside: None,
        };
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width: px(520.0),
                active_turn: true,
                rewinds: None,
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
    fn inline_code_fills_paint_after_the_text_lays_out(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let entry = AgentEntry::Assistant {
            id: 31,
            markdown: "A tiny bot named `cronkitty` had a single mission: `sleep(5)`.".into(),
            aside: None,
        };
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width: px(520.0),
                active_turn: false,
                rewinds: None,
            });
            crate::Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn trace_disclosure_sits_right_after_the_counts(cx: &mut TestAppContext) {
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
                rewinds: None,
            });
            crate::Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let counts = cx
            .debug_bounds("agent-activity-counts")
            .expect("the trace counts should be painted");
        let badge = cx
            .debug_bounds("agent-trace-count-badge")
            .expect("the count should sit in a badge");
        let chevron = cx
            .debug_bounds("agent-activity-chevron")
            .expect("the trace disclosure should be painted");
        assert!(cx.debug_bounds("agent-activity-glyph").is_none());
        assert!(chevron.left() >= counts.right());
        assert!(chevron.left() - counts.right() <= px(8.0));
        assert!(chevron.left() > badge.right());
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
            rewind_id: None,
        };
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width: PANE_WIDTH,
                active_turn: false,
                rewinds: None,
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

    fn rewind_point(
        cx: &mut TestAppContext,
        pane_width: Pixels,
        active_turn: bool,
    ) -> (
        Rc<std::cell::RefCell<Vec<SharedString>>>,
        &mut VisualTestContext,
    ) {
        cx.update(crate::init);
        let rewinds = Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = Rc::clone(&rewinds);
        let entry = AgentEntry::User {
            id: 7,
            markdown: "please rename every helper in this module and keep the tests passing, then explain what changed".into(),
            images: Arc::from([]),
            rewind_id: Some("prompt-7".into()),
        };
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| UserEntryTest {
                store: cx.new(|_| AgentTimelineStore::default()),
                entry,
                pane_width,
                active_turn,
                rewinds: Some(recorded),
            });
            crate::Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        (rewinds, cx)
    }

    #[gpui::test]
    fn a_prompt_row_offers_rewind_on_hover_and_hands_over_its_id(cx: &mut TestAppContext) {
        const PANE_WIDTH: Pixels = px(420.0);
        let (rewinds, cx) = rewind_point(cx, PANE_WIDTH, false);
        assert!(
            cx.debug_bounds("agent-user-rewind").is_none(),
            "the action waits for the pointer"
        );
        let bubble = cx
            .debug_bounds("agent-user-bubble")
            .expect("the user bubble should be painted");
        assert!(bubble.origin.x >= px(0.0) && bubble.right() <= PANE_WIDTH);

        cx.simulate_mouse_move(bubble.center(), None, gpui::Modifiers::none());
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let rewind = cx
            .debug_bounds("agent-user-rewind")
            .expect("hovering the prompt reveals the action");
        assert!(
            rewind.right() <= bubble.left(),
            "the action sits beside the bubble"
        );
        assert_eq!(
            cx.debug_bounds("agent-user-bubble"),
            Some(bubble),
            "revealing the action does not move the bubble"
        );

        cx.simulate_click(rewind.center(), gpui::Modifiers::none());
        assert_eq!(rewinds.borrow().as_slice(), ["prompt-7"]);
    }

    #[gpui::test]
    fn rewind_stays_hidden_while_a_turn_runs(cx: &mut TestAppContext) {
        let (rewinds, cx) = rewind_point(cx, px(420.0), true);
        let bubble = cx
            .debug_bounds("agent-user-bubble")
            .expect("the user bubble should be painted");
        cx.simulate_mouse_move(bubble.center(), None, gpui::Modifiers::none());
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert!(cx.debug_bounds("agent-user-rewind").is_none());
        cx.simulate_click(
            point(bubble.left() - px(14.0), bubble.center().y),
            gpui::Modifiers::none(),
        );
        assert!(rewinds.borrow().is_empty());
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
            store.expanded(7, DisclosureKind::Turn, false)
        }));
        store.update(cx, |store, cx| {
            store.toggle_expanded(7, DisclosureKind::Turn, false, cx);
        });
        store.update(cx, |store, _| store.tick_turn_clocks(Some(7)));
        assert!(
            store
                .read_with(cx, |store, _| store.turn_elapsed(7))
                .is_some()
        );
        assert_eq!(store.read_with(cx, |store, _| store.turn_elapsed(8)), None);

        cx.run_until_parked();

        let retained_markdown = store.update(cx, |store, cx| {
            store.markdown(7, MarkdownSlot::Body, "before".into(), cx)
        });
        assert_eq!(retained_markdown, markdown);
        assert!(store.update(cx, |store, _| {
            store.expanded(7, DisclosureKind::Turn, false)
        }));
        store.update(cx, |store, _| store.tick_turn_clocks(None));
        let finished = store.read_with(cx, |store, _| store.turn_elapsed(7));
        assert!(finished.is_some());
        store.update(cx, |store, _| store.tick_turn_clocks(Some(9)));
        assert_eq!(
            store.read_with(cx, |store, _| store.turn_elapsed(7)),
            finished,
            "a finished turn keeps its time"
        );
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
    fn ordinary_markdown_displays_the_original_allocation() {
        let mut text = String::with_capacity(128);
        text.push_str("héllo");
        let allocation = text.as_ptr();
        let source = AgentMarkdown::new(text);
        source.inspect(|rendered, _, _| {
            assert_eq!(rendered, "héllo");
            assert_eq!(rendered.as_ptr(), allocation);
        });
        source.synchronize_append("héllo world");
        source.inspect(|rendered, _, _| {
            assert_eq!(rendered, "héllo world");
            assert_eq!(rendered.as_ptr(), allocation);
        });
        source.replace("short replacement");
        source.inspect(|rendered, _, _| {
            assert_eq!(rendered, "short replacement");
            assert_eq!(rendered.as_ptr(), allocation);
        });
    }

    #[test]
    fn appending_a_preview_marker_at_the_limit_updates_the_display_revision() {
        let original = "line\n".repeat(MARKDOWN_PREVIEW_MAX_LINES);
        let source = AgentMarkdown::new(original.clone());
        assert!(!source.is_truncated());
        let previous_revision = source.inspect(|_, revision, _| revision);
        let appended = format!("{original}{MARKDOWN_PREVIEW_MARKER}");
        source.synchronize_append(&appended);
        assert!(source.is_truncated());
        source.inspect(|rendered, revision, replaced_at| {
            assert_eq!(rendered, appended);
            assert_eq!(revision, previous_revision + 1);
            assert_eq!(replaced_at, revision);
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

        let previous_preview = source
            .inspect(|preview, revision, replaced_at| (preview.to_owned(), revision, replaced_at));
        let appended = format!("{original}tail that remains available to copy");
        source.synchronize_append(&appended);
        assert_eq!(source.full_text(), appended);
        source.inspect(|preview, revision, replaced_at| {
            assert_eq!(
                (preview, revision, replaced_at),
                (
                    previous_preview.0.as_str(),
                    previous_preview.1,
                    previous_preview.2
                )
            );
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
