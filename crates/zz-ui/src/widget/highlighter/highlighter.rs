#![allow(dead_code)]

use super::{HighlightTheme, LanguageRegistry};

use anyhow::{Context, Result, anyhow};
use gpui::{HighlightStyle, SharedString};

use instant::{Duration, Instant};
use ropey::{ChunkCursor, Rope};
use std::sync::Arc;
use std::{
    collections::{BTreeSet, HashMap},
    ops::{ControlFlow, Range},
    usize,
};
use tree_sitter::{
    InputEdit, ParseOptions, Parser, Point, Query, QueryCursor, StreamingIterator, Tree,
};

const LARGE_NODE_THRESHOLD: usize = 8 * 1024;
const MAX_INJECTION_RANGES: usize = 4096;
const MAX_INJECTION_BYTES: usize = 512 * 1024;
const MAX_INJECTION_LANGUAGE_BYTES: usize = 64;
const INJECTION_PARSE_TIMEOUT: Duration = Duration::from_millis(20);

/// Incremental tree-sitter highlighter for one language, with injection layers.
pub struct SyntaxHighlighter {
    language: SharedString,
    query: Option<Query>,
    injections_query: Option<Arc<Query>>,

    injection_content_capture_index: Option<u32>,
    injection_language_capture_index: Option<u32>,

    text: Rope,
    parser: Parser,
    tree: Option<Tree>,

    injection_layers: Vec<InjectionLayer>,
}

pub(crate) struct InjectionLayer {
    pub(crate) language_name: SharedString,
    highlight_query: Arc<Query>,
    pub(crate) ranges: Vec<tree_sitter::Range>,
    pub(crate) byte_range: Range<usize>,
    pub(crate) tree: Tree,
}

pub(crate) struct InjectionParseData {
    pub(crate) query: Arc<Query>,
    pub(crate) content_capture_index: Option<u32>,
    pub(crate) language_capture_index: Option<u32>,
    pub(crate) old_layers: Vec<ReusableInjectionLayer>,
}

pub(crate) struct ReusableInjectionLayer {
    pub(crate) language_name: SharedString,
    highlight_query: Arc<Query>,
    pub(crate) ranges: Vec<tree_sitter::Range>,
    pub(crate) tree: Tree,
}

struct TextProvider<'a>(&'a Rope);
struct ByteChunks<'a> {
    cursor: ChunkCursor<'a>,
    node_start: usize,
    node_end: usize,
    at_first: bool,
}
impl<'a> tree_sitter::TextProvider<&'a [u8]> for TextProvider<'a> {
    type I = ByteChunks<'a>;

    fn text(&mut self, node: tree_sitter::Node) -> Self::I {
        let range = node.byte_range();
        let cursor = self.0.chunk_cursor_at(range.start);

        ByteChunks {
            cursor,
            node_start: range.start,
            node_end: range.end,
            at_first: true,
        }
    }
}

impl<'a> Iterator for ByteChunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if !self.at_first {
            if !self.cursor.next() {
                return None;
            }
        }
        self.at_first = false;

        let chunk_byte_start = self.cursor.byte_offset();
        if chunk_byte_start >= self.node_end {
            return None;
        }

        let chunk = self.cursor.chunk().as_bytes();

        let start_in_chunk = self.node_start.saturating_sub(chunk_byte_start);
        let end_in_chunk = (self.node_end - chunk_byte_start).min(chunk.len());

        if start_in_chunk >= end_in_chunk {
            return None;
        }

        Some(&chunk[start_in_chunk..end_in_chunk])
    }
}

fn injection_range_len(range: &tree_sitter::Range) -> usize {
    range.end_byte.saturating_sub(range.start_byte)
}

fn injection_ranges_byte_count(ranges: &[tree_sitter::Range]) -> usize {
    ranges.iter().map(injection_range_len).sum()
}

fn injection_ranges_within_limits(ranges: &[tree_sitter::Range]) -> bool {
    ranges.len() <= MAX_INJECTION_RANGES
        && injection_ranges_byte_count(ranges) <= MAX_INJECTION_BYTES
}

fn captured_injection_language(text: &Rope, range: Range<usize>) -> Option<SharedString> {
    if range.end > text.len()
        || range.start >= range.end
        || range.end.saturating_sub(range.start) > MAX_INJECTION_LANGUAGE_BYTES
    {
        return None;
    }

    let language = text.slice(range).to_string();
    let language = language.trim();
    (!language.is_empty()).then(|| SharedString::from(language.to_string()))
}

fn normalize_combined_injection_ranges(
    language_name: &SharedString,
    ranges: Vec<tree_sitter::Range>,
) -> Vec<tree_sitter::Range> {
    if language_name.as_ref() != "markdown_inline" || ranges.len() <= 1 {
        return ranges;
    }

    let mut normalized = Vec::with_capacity(ranges.len().min(MAX_INJECTION_RANGES));
    let mut byte_count = 0usize;
    let mut previous_range: Option<tree_sitter::Range> = None;

    for range in ranges {
        let mut pending_ranges = Vec::with_capacity(2);
        if let Some(previous) = previous_range {
            if previous.end_byte < range.start_byte {
                pending_ranges.push(tree_sitter::Range {
                    start_byte: previous.end_byte,
                    end_byte: range.start_byte,
                    start_point: previous.end_point,
                    end_point: range.start_point,
                });
            }
        }
        pending_ranges.push(range);

        let pending_len = pending_ranges
            .iter()
            .map(injection_range_len)
            .sum::<usize>();
        if normalized.len().saturating_add(pending_ranges.len()) > MAX_INJECTION_RANGES
            || byte_count.saturating_add(pending_len) > MAX_INJECTION_BYTES
        {
            break;
        }

        byte_count += pending_len;
        normalized.extend(pending_ranges);
        previous_range = Some(range);
    }

    normalized
}

fn should_include_injection_range(
    language_name: &SharedString,
    range: &tree_sitter::Range,
    text: &Rope,
) -> bool {
    if language_name.as_ref() != "markdown_inline" {
        return true;
    }

    markdown_inline_range_has_trigger(text, range.start_byte..range.end_byte)
}

fn markdown_inline_range_has_trigger(text: &Rope, range: Range<usize>) -> bool {
    text.slice(range).bytes().any(|byte| {
        matches!(
            byte,
            b'*' | b'_' | b'`' | b'[' | b']' | b'(' | b')' | b'<' | b'>' | b'!' | b'~' | b'$'
        )
    })
}

#[derive(Debug, Default, Clone)]
struct HighlightSummary {
    count: usize,
    start: usize,
    end: usize,
    min_start: usize,
    max_end: usize,
}

#[derive(Debug, Default, Clone)]
struct HighlightItem {
    range: Range<usize>,
    name: SharedString,
}

impl HighlightItem {
    pub fn new(range: Range<usize>, name: impl Into<SharedString>) -> Self {
        Self {
            range,
            name: name.into(),
        }
    }
}

impl sum_tree::Item for HighlightItem {
    type Summary = HighlightSummary;
    fn summary(&self, _cx: &()) -> Self::Summary {
        HighlightSummary {
            count: 1,
            start: self.range.start,
            end: self.range.end,
            min_start: self.range.start,
            max_end: self.range.end,
        }
    }
}

impl sum_tree::Summary for HighlightSummary {
    type Context<'a> = &'a ();
    fn zero(_: Self::Context<'_>) -> Self {
        HighlightSummary {
            count: 0,
            start: usize::MIN,
            end: usize::MAX,
            min_start: usize::MAX,
            max_end: usize::MIN,
        }
    }

    fn add_summary(&mut self, other: &Self, _: Self::Context<'_>) {
        self.min_start = self.min_start.min(other.min_start);
        self.max_end = self.max_end.max(other.max_end);
        self.start = other.start;
        self.end = other.end;
        self.count += other.count;
    }
}

impl<'a> sum_tree::Dimension<'a, HighlightSummary> for usize {
    fn zero(_: &()) -> Self {
        0
    }

    fn add_summary(&mut self, _: &'a HighlightSummary, _: &()) {}
}

impl<'a> sum_tree::Dimension<'a, HighlightSummary> for Range<usize> {
    fn zero(_: &()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a HighlightSummary, _: &()) {
        self.start = summary.start;
        self.end = summary.end;
    }
}

impl SyntaxHighlighter {
    pub fn new(lang: &str) -> Self {
        match Self::build_for_language(&lang) {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!(
                    "SyntaxHighlighter init failed, fallback to use `text`, {}",
                    err
                );
                Self::build_for_language("text").unwrap()
            }
        }
    }

    fn build_inert(language: SharedString) -> Self {
        Self {
            language,
            query: None,
            injections_query: None,
            injection_content_capture_index: None,
            injection_language_capture_index: None,
            text: Rope::new(),
            parser: Parser::new(),
            tree: None,
            injection_layers: Vec::new(),
        }
    }

    /// https://github.com/tree-sitter/tree-sitter/blob/v0.26.8/crates/highlight/src/highlight.rs#L339
    fn build_for_language(lang: &str) -> Result<Self> {
        let Some(config) = LanguageRegistry::singleton().language(&lang) else {
            return Err(anyhow!(
                "language {:?} is not registered in `LanguageRegistry`",
                lang
            ));
        };

        let Some(grammar) = config.language.as_ref() else {
            return Ok(Self::build_inert(config.name.clone()));
        };

        let mut parser = Parser::new();
        parser.set_language(grammar).context("parse set_language")?;

        let mut query_source = String::new();
        query_source.push_str(&config.injections);
        let locals_query_offset = query_source.len();
        query_source.push_str(&config.locals);
        query_source.push_str(&config.highlights);

        let mut query = Query::new(grammar, &query_source).context("new query")?;

        let mut injection_pattern_count = 0;
        for i in 0..(query.pattern_count()) {
            if query.start_byte_for_pattern(i) < locals_query_offset {
                injection_pattern_count += 1;
            }
        }

        let injections_query = if !config.injections.is_empty() {
            Query::new(grammar, &config.injections).ok().map(Arc::new)
        } else {
            None
        };

        for pattern_index in 0..injection_pattern_count {
            query.disable_pattern(pattern_index);
        }

        let injection_content_capture_index = injections_query.as_ref().and_then(|q| {
            q.capture_names()
                .iter()
                .position(|name| *name == "injection.content")
                .map(|i| i as u32)
        });
        let injection_language_capture_index = injections_query.as_ref().and_then(|q| {
            q.capture_names()
                .iter()
                .position(|name| *name == "injection.language")
                .map(|i| i as u32)
        });

        Ok(Self {
            language: config.name.clone(),
            query: Some(query),
            injections_query,

            injection_content_capture_index,
            injection_language_capture_index,
            text: Rope::new(),
            parser,
            tree: None,
            injection_layers: Vec::new(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.text.len() == 0
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    pub fn language(&self) -> &SharedString {
        &self.language
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    /// Reparses around `edit`, or the whole text when it is `None`.
    ///
    /// A `timeout` that expires returns false and keeps the old tree for stale
    /// highlighting, but `self.text` still advances so a caller can reparse elsewhere.
    pub fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        timeout: Option<Duration>,
    ) -> bool {
        if self.text.eq(text) {
            return true;
        }

        if self.parser.language().is_none() {
            self.text = text.clone();
            return true;
        }

        let edit = edit.unwrap_or(InputEdit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: text.len(),
            start_position: Point::new(0, 0),
            old_end_position: Point::new(0, 0),
            new_end_position: Point::new(0, 0),
        });

        let mut old_tree = self
            .tree
            .take()
            .unwrap_or(self.parser.parse("", None).unwrap());
        old_tree.edit(&edit);

        let mut timed_out = false;
        let start = Instant::now();
        let mut progress = |_: &tree_sitter::ParseState| -> ControlFlow<()> {
            let Some(budget) = timeout else {
                return ControlFlow::Continue(());
            };

            if start.elapsed() > budget {
                timed_out = true;
                return ControlFlow::Break(());
            }

            ControlFlow::Continue(())
        };

        let options = ParseOptions::new().progress_callback(&mut progress);
        let new_tree = self.parser.parse_with_options(
            &mut move |offset, _| {
                if offset >= text.len() {
                    ""
                } else {
                    let (chunk, chunk_byte_ix) = text.chunk(offset);
                    &chunk[offset - chunk_byte_ix..]
                }
            },
            Some(&old_tree),
            Some(options),
        );

        if timed_out || new_tree.is_none() {
            self.tree = Some(old_tree);
            self.text = text.clone();
            return false;
        }

        let new_tree = new_tree.unwrap();
        self.tree = Some(new_tree.clone());
        self.text = text.clone();
        self.parse_injection_layers(&new_tree);
        true
    }

    pub(crate) fn injection_parse_data(&self) -> Option<InjectionParseData> {
        let query = self.injections_query.clone()?;
        Some(InjectionParseData {
            query,
            content_capture_index: self.injection_content_capture_index,
            language_capture_index: self.injection_language_capture_index,
            old_layers: self
                .injection_layers
                .iter()
                .map(|layer| ReusableInjectionLayer {
                    language_name: layer.language_name.clone(),
                    highlight_query: layer.highlight_query.clone(),
                    ranges: layer.ranges.clone(),
                    tree: layer.tree.clone(),
                })
                .collect(),
        })
    }

    pub(crate) fn compute_injection_layers(
        data: InjectionParseData,
        tree: &Tree,
        text: &Rope,
    ) -> Vec<InjectionLayer> {
        struct CombinedRanges {
            ranges: Vec<tree_sitter::Range>,
            byte_count: usize,
        }

        impl CombinedRanges {
            fn push_limited(&mut self, ranges: Vec<tree_sitter::Range>) {
                for range in ranges {
                    if self.ranges.len() >= MAX_INJECTION_RANGES {
                        break;
                    }

                    let range_len = injection_range_len(&range);
                    if self.byte_count.saturating_add(range_len) > MAX_INJECTION_BYTES {
                        break;
                    }

                    self.byte_count += range_len;
                    self.ranges.push(range);
                }
            }
        }

        fn sort_ranges(ranges: &mut [tree_sitter::Range]) {
            ranges.sort_unstable_by(|a, b| {
                a.start_byte
                    .cmp(&b.start_byte)
                    .then_with(|| a.end_byte.cmp(&b.end_byte))
            });
        }

        fn ranges_cache_key(ranges: &[tree_sitter::Range]) -> Vec<(usize, usize)> {
            ranges.iter().map(|r| (r.start_byte, r.end_byte)).collect()
        }

        fn resolve_language(
            language_name: &str,
            query_cache: &mut HashMap<SharedString, Arc<Query>>,
        ) -> Option<(SharedString, Arc<Query>)> {
            let config = LanguageRegistry::singleton().language(language_name)?;
            if let Some(query) = query_cache.get(&config.name) {
                return Some((config.name, query.clone()));
            }

            let query = match Query::new(config.language.as_ref()?, &config.highlights) {
                Ok(query) => Arc::new(query),
                Err(error) => {
                    tracing::error!(
                        "failed to build injection query for {:?}: {:?}",
                        config.name,
                        error
                    );
                    return None;
                }
            };
            query_cache.insert(config.name.clone(), query.clone());
            Some((config.name, query))
        }

        let root_node = tree.root_node();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&data.query, root_node, TextProvider(text));

        let mut combined_ranges: HashMap<SharedString, CombinedRanges> = HashMap::new();
        let old_layer_trees: HashMap<_, _> = data
            .old_layers
            .iter()
            .map(|layer| {
                (
                    (layer.language_name.clone(), ranges_cache_key(&layer.ranges)),
                    &layer.tree,
                )
            })
            .collect();
        let mut highlight_queries: HashMap<SharedString, Arc<Query>> = data
            .old_layers
            .iter()
            .map(|layer| (layer.language_name.clone(), layer.highlight_query.clone()))
            .collect();
        let mut resolved_languages: HashMap<SharedString, Option<(SharedString, Arc<Query>)>> =
            HashMap::new();
        let mut new_layers = Vec::new();
        while let Some(query_match) = matches.next() {
            let mut language_name: Option<SharedString> = None;
            let mut combined = false;
            for prop in data.query.property_settings(query_match.pattern_index) {
                match prop.key.as_ref() {
                    "injection.language" => {
                        language_name = prop
                            .value
                            .as_ref()
                            .map(|v| SharedString::from(v.to_string()));
                    }
                    "injection.combined" => combined = true,
                    _ => {}
                }
            }

            if language_name.is_none() {
                language_name = query_match
                    .captures
                    .iter()
                    .find(|cap| Some(cap.index) == data.language_capture_index)
                    .and_then(|capture| {
                        captured_injection_language(text, capture.node.byte_range())
                    });
            }

            let Some(raw_language_name) = language_name else {
                continue;
            };
            let resolved_language =
                if let Some(resolved) = resolved_languages.get(&raw_language_name) {
                    resolved.clone()
                } else {
                    let resolved = resolve_language(&raw_language_name, &mut highlight_queries);
                    resolved_languages.insert(raw_language_name, resolved.clone());
                    resolved
                };
            let Some((language_name, highlight_query)) = resolved_language else {
                continue;
            };

            let mut ranges = query_match
                .captures
                .iter()
                .filter(|cap| Some(cap.index) == data.content_capture_index)
                .map(|capture| capture.node.range())
                .collect::<Vec<_>>();

            if ranges.is_empty() {
                continue;
            }
            ranges.retain(|range| should_include_injection_range(&language_name, range, text));
            if ranges.is_empty() {
                continue;
            }
            sort_ranges(&mut ranges);

            if combined {
                combined_ranges
                    .entry(language_name.clone())
                    .or_insert_with(|| CombinedRanges {
                        ranges: Vec::new(),
                        byte_count: 0,
                    })
                    .push_limited(ranges);
            } else {
                if !injection_ranges_within_limits(&ranges) {
                    continue;
                }

                let old_tree = old_layer_trees
                    .get(&(language_name.clone(), ranges_cache_key(&ranges)))
                    .copied();
                if let Some(layer) = Self::parse_injection_layer(
                    &language_name,
                    highlight_query,
                    ranges,
                    old_tree,
                    text,
                ) {
                    new_layers.push(layer);
                }
            }
        }

        for (language_name, combined) in combined_ranges {
            let mut ranges = combined.ranges;
            if ranges.is_empty() {
                continue;
            }
            sort_ranges(&mut ranges);
            ranges = normalize_combined_injection_ranges(&language_name, ranges);
            if ranges.is_empty() {
                continue;
            }
            let old_tree = old_layer_trees
                .get(&(language_name.clone(), ranges_cache_key(&ranges)))
                .copied();
            let Some(highlight_query) = highlight_queries.get(&language_name).cloned() else {
                continue;
            };
            if let Some(layer) =
                Self::parse_injection_layer(&language_name, highlight_query, ranges, old_tree, text)
            {
                new_layers.push(layer);
            }
        }
        new_layers.sort_by_key(|layer| layer.byte_range.start);
        new_layers
    }

    fn parse_injection_layer(
        language_name: &SharedString,
        highlight_query: Arc<Query>,
        ranges: Vec<tree_sitter::Range>,
        old_tree: Option<&Tree>,
        text: &Rope,
    ) -> Option<InjectionLayer> {
        fn bounding_byte_range(ranges: &[tree_sitter::Range]) -> Option<Range<usize>> {
            let start = ranges.iter().map(|r| r.start_byte).min()?;
            let end = ranges.iter().map(|r| r.end_byte).max()?;
            Some(start..end)
        }
        let config = LanguageRegistry::singleton().language(language_name)?;
        let mut parser = Parser::new();
        parser.set_language(config.language.as_ref()?).ok()?;
        parser.set_included_ranges(&ranges).ok()?;
        let parse_start = Instant::now();
        let mut timed_out = false;
        let mut progress = |_: &tree_sitter::ParseState| -> ControlFlow<()> {
            if parse_start.elapsed() > INJECTION_PARSE_TIMEOUT {
                timed_out = true;
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let options = ParseOptions::new().progress_callback(&mut progress);

        let new_tree = parser.parse_with_options(
            &mut |offset, _| {
                if offset >= text.len() {
                    ""
                } else {
                    let (chunk, chunk_byte_ix) = text.chunk(offset);
                    &chunk[offset - chunk_byte_ix..]
                }
            },
            old_tree,
            Some(options),
        )?;
        if timed_out {
            return None;
        }

        let byte_range = bounding_byte_range(&ranges)?;
        Some(InjectionLayer {
            language_name: language_name.clone(),
            highlight_query,
            ranges,
            byte_range,
            tree: new_tree,
        })
    }

    pub(crate) fn apply_background_tree(
        &mut self,
        tree: Tree,
        text: &Rope,
        injection_layers: Vec<InjectionLayer>,
    ) {
        if !self.text.eq(text) {
            return;
        }

        self.tree = Some(tree);
        self.injection_layers = injection_layers;
    }

    fn parse_injection_layers(&mut self, tree: &Tree) {
        let Some(data) = self.injection_parse_data() else {
            self.injection_layers.clear();
            return;
        };
        self.injection_layers = Self::compute_injection_layers(data, tree, &self.text.clone());
    }

    fn match_styles(&self, range: Range<usize>) -> Vec<HighlightItem> {
        let mut highlights = vec![];
        let mut injection_highlights = vec![];
        let Some(tree) = &self.tree else {
            return highlights;
        };

        let Some(query) = &self.query else {
            return highlights;
        };

        let root_node = tree.root_node();
        let source = &self.text;

        let mut last_layer_start = 0;
        for layer in &self.injection_layers {
            debug_assert!(layer.byte_range.start >= last_layer_start);
            last_layer_start = layer.byte_range.start;

            if layer.byte_range.end <= range.start {
                continue;
            }

            if layer.byte_range.start >= range.end {
                break;
            }

            let query = &layer.highlight_query;

            let mut query_cursor = QueryCursor::new();
            query_cursor.set_byte_range(range.clone());

            let mut matches =
                query_cursor.matches(query, layer.tree.root_node(), TextProvider(&self.text));

            let mut last_end = 0usize;
            while let Some(m) = matches.next() {
                let allow_overlapping_captures = query
                    .property_settings(m.pattern_index)
                    .iter()
                    .any(|prop| prop.key.as_ref() == "highlight.allow-overlap");

                for cap in m.captures {
                    let node_range = cap.node.start_byte()..cap.node.end_byte();

                    if !allow_overlapping_captures && node_range.start < last_end {
                        continue;
                    }

                    if let Some(highlight_name) = query.capture_names().get(cap.index as usize) {
                        if !allow_overlapping_captures {
                            last_end = node_range.end;
                        }
                        injection_highlights.push(HighlightItem::new(
                            node_range,
                            SharedString::from(highlight_name.to_string()),
                        ));
                    }
                }
            }
        }

        let query_nodes = collect_query_nodes(root_node, &range);

        for query_node in &query_nodes {
            let mut query_cursor = QueryCursor::new();
            query_cursor.set_byte_range(range.clone());

            let mut matches = query_cursor.matches(&query, *query_node, TextProvider(&source));

            while let Some(query_match) = matches.next() {
                for cap in query_match.captures {
                    let node = cap.node;

                    let Some(highlight_name) = query.capture_names().get(cap.index as usize) else {
                        continue;
                    };

                    let node_range: Range<usize> = node.start_byte()..node.end_byte();
                    let highlight_name = SharedString::from(highlight_name.to_string());

                    let last_item = highlights.last();
                    let last_range = last_item.map(|item| &item.range).unwrap_or(&(0..0));
                    let last_highlight_name = last_item.map(|item| item.name.clone());

                    if last_range == &node_range {
                        highlights.push(HighlightItem::new(
                            node_range,
                            last_highlight_name.unwrap_or(highlight_name),
                        ));
                    } else {
                        highlights.push(HighlightItem::new(node_range, highlight_name.clone()));
                    }
                }
            }
        }

        highlights.extend(injection_highlights);

        highlights
    }

    /// Styles covering the byte `range`, each paired with the byte range it applies to.
    ///
    /// ```no_run
    /// use zz_ui::highlighter::{HighlightTheme, SyntaxHighlighter};
    /// use ropey::Rope;
    ///
    /// let code = "fn main() {\n    println!(\"Hello\");\n}";
    /// let rope = Rope::from_str(code);
    /// let mut highlighter = SyntaxHighlighter::new("rust");
    /// highlighter.update(None, &rope, None);
    ///
    /// let theme = HighlightTheme::default_dark();
    /// let range = 0..code.len();
    /// let styles = highlighter.styles(&range, &theme);
    /// ```
    pub fn styles(
        &self,
        range: &Range<usize>,
        theme: &HighlightTheme,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut styles = vec![];
        let start_offset = range.start;

        let highlights = self.match_styles(range.clone());

        for item in highlights {
            let node_range = &item.range;
            let name = &item.name;

            let mut node_range = node_range.start.max(range.start)..node_range.end.min(range.end);
            if node_range.start > node_range.end {
                node_range.end = node_range.start;
            }
            if node_range.is_empty() {
                continue;
            }

            styles.push((node_range, theme.style(name.as_ref()).unwrap_or_default()));
        }

        if styles.len() == 0 {
            return vec![(start_offset..range.end, HighlightStyle::default())];
        }

        let styles = unique_styles(&range, styles);

        styles
    }
}

/// Flattens overlapping styles: a later range wins over the ranges it covers,
/// splitting them around it.
pub(crate) fn unique_styles(
    total_range: &Range<usize>,
    styles: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let styles: Vec<_> = styles
        .into_iter()
        .filter(|(range, _)| !range.is_empty())
        .collect();

    if styles.is_empty() {
        return styles;
    }

    let mut intervals: Vec<(usize, bool, usize)> = Vec::with_capacity(styles.len() * 2 + 2);
    for (i, (range, _)) in styles.iter().enumerate() {
        intervals.push((range.start, true, i));
        intervals.push((range.end, false, i));
    }

    intervals.push((total_range.start, true, usize::MAX));
    intervals.push((total_range.end, false, usize::MAX));

    intervals.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut significant_intervals: BTreeSet<usize> = BTreeSet::new();
    for (range, _) in &styles {
        significant_intervals.insert(range.end);
    }

    let mut result: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut active_styles: Vec<usize> = Vec::new();
    let mut last_pos = total_range.start;

    for (pos, is_start, style_idx) in intervals {
        let is_boundary = style_idx == usize::MAX;

        if pos > last_pos {
            let interval = last_pos..pos;
            let combined_style = if active_styles.is_empty() {
                HighlightStyle::default()
            } else {
                let mut combined = HighlightStyle::default();
                for &idx in &active_styles {
                    merge_highlight_style(&mut combined, &styles[idx].1);
                }
                combined
            };
            result.push((interval, combined_style));
        }

        if !is_boundary {
            if is_start {
                active_styles.push(style_idx);
            } else {
                active_styles.retain(|&i| i != style_idx);
            }
        }

        last_pos = pos;
    }

    let mut merged: Vec<(Range<usize>, HighlightStyle)> = Vec::with_capacity(result.len());
    for (range, style) in result {
        if let Some((last_range, last_style)) = merged.last_mut() {
            if last_range.end == range.start
                && *last_style == style
                && !significant_intervals.contains(&range.start)
            {
                last_range.end = range.end;
                continue;
            }
        }
        merged.push((range, style));
    }

    merged
}

fn collect_query_nodes<'a>(
    root: tree_sitter::Node<'a>,
    range: &Range<usize>,
) -> Vec<tree_sitter::Node<'a>> {
    let mut nodes = Vec::new();
    collect_query_nodes_inner(root, range, &mut nodes);
    if nodes.is_empty() {
        nodes.push(root);
    }
    nodes
}

fn collect_query_nodes_inner<'a>(
    node: tree_sitter::Node<'a>,
    range: &Range<usize>,
    out: &mut Vec<tree_sitter::Node<'a>>,
) {
    if node.end_byte() <= range.start || node.start_byte() >= range.end {
        return;
    }

    let node_span = node.end_byte() - node.start_byte();
    let range_span = range.end - range.start;

    if node_span > range_span + LARGE_NODE_THRESHOLD && node.child_count() > 0 {
        let mut cursor = node.walk();
        if cursor.goto_first_child_for_byte(range.start).is_some() {
            loop {
                let child = cursor.node();
                if child.start_byte() >= range.end {
                    break;
                }
                collect_query_nodes_inner(child, range, out);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        return;
    }

    out.push(node);
}

fn merge_highlight_style(style: &mut HighlightStyle, other: &HighlightStyle) {
    if let Some(color) = other.color {
        style.color = Some(color);
    }
    if let Some(font_weight) = other.font_weight {
        style.font_weight = Some(font_weight);
    }
    if let Some(font_style) = other.font_style {
        style.font_style = Some(font_style);
    }
    if let Some(background_color) = other.background_color {
        style.background_color = Some(background_color);
    }
    if let Some(underline) = other.underline {
        style.underline = Some(underline);
    }
    if let Some(strikethrough) = other.strikethrough {
        style.strikethrough = Some(strikethrough);
    }
    if let Some(fade_out) = other.fade_out {
        style.fade_out = Some(fade_out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_grammars_produce_palette_styles() {
        let theme = HighlightTheme::default_dark();
        for (language, source) in [
            ("rust", "fn main() { let answer = 42; }\n"),
            ("markdown", "# Editor\n\n`code`\n"),
            ("json", "{\"editor\": true}\n"),
            ("toml", "editor = true\n"),
            (
                "tmux",
                "# Prefix\nunbind C-b\nset -g prefix C-a\nset -g history-limit 10000\n\
                 set -g default-terminal \"tmux-256color\"\n",
            ),
        ] {
            let text = Rope::from(source);
            let mut highlighter = SyntaxHighlighter::new(language);
            assert!(highlighter.update(None, &text, None), "{language}");
            assert!(
                highlighter
                    .styles(&(0..source.len()), &theme)
                    .iter()
                    .any(|(_, style)| style.color.is_some()),
                "{language} did not produce a colored capture"
            );
        }
    }

    #[test]
    fn languages_without_a_grammar_are_never_parsed() {
        let text = Rope::from("hello {\"a\": 1}\nworld");

        for language in ["text", "bash", "no-such-language"] {
            let mut highlighter = SyntaxHighlighter::new(language);
            assert!(highlighter.update(None, &text, None), "{language}");
            assert!(highlighter.tree().is_none(), "{language} built a tree");
            assert_eq!(highlighter.text().to_string(), text.to_string());

            assert_eq!(
                highlighter.styles(&(0..text.len()), &HighlightTheme::default_dark()),
                vec![(0..text.len(), HighlightStyle::default())],
                "{language}"
            );
        }
    }
}
