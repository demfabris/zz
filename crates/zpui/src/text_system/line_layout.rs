use crate::{FontId, GlyphId, Pixels, PlatformTextSystem, Point, SharedString, Size, point, px};
use collections::FxHashMap;
use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard};
use smallvec::SmallVec;
use std::{
    borrow::Borrow,
    hash::{Hash, Hasher},
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::{LineWrapper, recent_shapes::RecentShapes};

/// A laid out and styled line of text
#[derive(Default, Debug)]
pub struct LineLayout {
    /// The font size for this line
    pub font_size: Pixels,
    /// The width of the line
    pub width: Pixels,
    /// The ascent of the line
    pub ascent: Pixels,
    /// The descent of the line
    pub descent: Pixels,
    /// The shaped runs that make up this line
    pub runs: Vec<ShapedRun>,
    /// The length of the line in utf-8 bytes
    pub len: usize,
}

/// A run of text that has been shaped .
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// The font id for this run
    pub font_id: FontId,
    /// The glyphs that make up this run
    pub glyphs: Vec<ShapedGlyph>,
}

/// A single glyph, ready to paint.
#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    /// The ID for this glyph, as determined by the text system.
    pub id: GlyphId,

    /// The position of this glyph in its containing line.
    pub position: Point<Pixels>,

    /// The index of this glyph in the original text.
    pub index: usize,

    /// Whether this glyph is an emoji
    pub is_emoji: bool,
}

impl LineLayout {
    /// The index for the character at the given x coordinate
    pub fn index_for_x(&self, x: Pixels) -> Option<usize> {
        if x >= self.width {
            None
        } else {
            for run in self.runs.iter().rev() {
                for glyph in run.glyphs.iter().rev() {
                    if glyph.position.x <= x {
                        return Some(glyph.index);
                    }
                }
            }
            Some(0)
        }
    }

    /// closest_index_for_x returns the character boundary closest to the given x coordinate
    /// (e.g. to handle aligning up/down arrow keys)
    pub fn closest_index_for_x(&self, x: Pixels) -> usize {
        let mut prev_index = 0;
        let mut prev_x = px(0.);

        for run in self.runs.iter() {
            for glyph in run.glyphs.iter() {
                if glyph.position.x >= x {
                    if glyph.position.x - x < x - prev_x {
                        return glyph.index;
                    } else {
                        return prev_index;
                    }
                }
                prev_index = glyph.index;
                prev_x = glyph.position.x;
            }
        }

        if self.len == 1 {
            if x > self.width / 2. {
                return 1;
            } else {
                return 0;
            }
        }

        self.len
    }

    /// The x position of the character at the given index
    pub fn x_for_index(&self, index: usize) -> Pixels {
        for run in &self.runs {
            for glyph in &run.glyphs {
                if glyph.index >= index {
                    return glyph.position.x;
                }
            }
        }
        self.width
    }

    /// The corresponding Font at the given index
    pub fn font_id_for_index(&self, index: usize) -> Option<FontId> {
        for run in &self.runs {
            for glyph in &run.glyphs {
                if glyph.index >= index {
                    return Some(run.font_id);
                }
            }
        }
        None
    }

    /// Split this layout at a byte index, returning `(prefix, suffix)`.
    ///
    /// - `prefix` contains glyphs for bytes `[0, byte_index)` with original positions.
    ///   Its width equals the x-advance up to the split point.
    /// - `suffix` contains glyphs for bytes `[byte_index, len)` with positions
    ///   shifted left so the first glyph starts at x=0, and byte indices rebased to 0.
    /// - `font_size`, `ascent`, and `descent` are copied to both halves.
    pub fn split_at(&self, byte_index: usize) -> (LineLayout, LineLayout) {
        let x_offset = self.x_for_index(byte_index);

        // Partition glyph runs. A single run may contribute glyphs to both halves.
        let mut left_runs = Vec::new();
        let mut right_runs = Vec::new();

        for run in &self.runs {
            let split_pos = run.glyphs.partition_point(|g| g.index < byte_index);

            if split_pos > 0 {
                left_runs.push(ShapedRun {
                    font_id: run.font_id,
                    glyphs: run.glyphs[..split_pos].to_vec(),
                });
            }

            if split_pos < run.glyphs.len() {
                let right_glyphs = run.glyphs[split_pos..]
                    .iter()
                    .map(|g| ShapedGlyph {
                        id: g.id,
                        position: point(g.position.x - x_offset, g.position.y),
                        index: g.index - byte_index,
                        is_emoji: g.is_emoji,
                    })
                    .collect();
                right_runs.push(ShapedRun {
                    font_id: run.font_id,
                    glyphs: right_glyphs,
                });
            }
        }

        let left = LineLayout {
            font_size: self.font_size,
            width: x_offset,
            ascent: self.ascent,
            descent: self.descent,
            runs: left_runs,
            len: byte_index,
        };

        let right = LineLayout {
            font_size: self.font_size,
            width: self.width - x_offset,
            ascent: self.ascent,
            descent: self.descent,
            runs: right_runs,
            len: self.len - byte_index,
        };

        (left, right)
    }

    fn compute_wrap_boundaries(
        &self,
        text: &str,
        wrap_width: Pixels,
        max_lines: Option<usize>,
    ) -> SmallVec<[WrapBoundary; 1]> {
        let mut boundaries = SmallVec::new();
        let mut first_non_whitespace_ix = None;
        let mut last_candidate_ix = None;
        let mut last_candidate_x = px(0.);
        let mut last_boundary = WrapBoundary {
            run_ix: 0,
            glyph_ix: 0,
        };
        let mut last_boundary_x = px(0.);
        let mut prev_ch = '\0';
        let mut glyphs = self
            .runs
            .iter()
            .enumerate()
            .flat_map(move |(run_ix, run)| {
                run.glyphs.iter().enumerate().map(move |(glyph_ix, glyph)| {
                    let character = text[glyph.index..].chars().next().unwrap();
                    (
                        WrapBoundary { run_ix, glyph_ix },
                        character,
                        glyph.position.x,
                    )
                })
            })
            .peekable();

        while let Some((boundary, ch, x)) = glyphs.next() {
            if ch == '\n' {
                continue;
            }

            // Here is very similar to `LineWrapper::wrap_line` to determine text wrapping,
            // but there are some differences, so we have to duplicate the code here.
            if LineWrapper::is_word_char(ch) {
                if prev_ch == ' ' && ch != ' ' && first_non_whitespace_ix.is_some() {
                    last_candidate_ix = Some(boundary);
                    last_candidate_x = x;
                }
            } else {
                if ch != ' ' && first_non_whitespace_ix.is_some() {
                    last_candidate_ix = Some(boundary);
                    last_candidate_x = x;
                }
            }

            if ch != ' ' && first_non_whitespace_ix.is_none() {
                first_non_whitespace_ix = Some(boundary);
            }

            let next_x = glyphs.peek().map_or(self.width, |(_, _, x)| *x);
            let width = next_x - last_boundary_x;

            if width > wrap_width && boundary > last_boundary {
                // When used line_clamp, we should limit the number of lines.
                if let Some(max_lines) = max_lines
                    && boundaries.len() >= max_lines.saturating_sub(1)
                {
                    break;
                }

                if let Some(last_candidate_ix) = last_candidate_ix.take() {
                    last_boundary = last_candidate_ix;
                    last_boundary_x = last_candidate_x;
                } else {
                    last_boundary = boundary;
                    last_boundary_x = x;
                }
                boundaries.push(last_boundary);
            }
            prev_ch = ch;
        }

        boundaries
    }
}

/// A line of text that has been wrapped to fit a given width
#[derive(Default, Debug)]
pub struct WrappedLineLayout {
    /// The line layout, pre-wrapping.
    pub unwrapped_layout: Arc<LineLayout>,

    /// The boundaries at which the line was wrapped
    pub wrap_boundaries: SmallVec<[WrapBoundary; 1]>,

    /// The width of the line, if it was wrapped
    pub wrap_width: Option<Pixels>,
}

/// A boundary at which a line was wrapped
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WrapBoundary {
    /// The index in the run just before the line was wrapped
    pub run_ix: usize,
    /// The index of the glyph just before the line was wrapped
    pub glyph_ix: usize,
}

impl WrappedLineLayout {
    /// The length of the underlying text, in utf8 bytes.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.unwrapped_layout.len
    }

    /// The width of this line, in pixels, whether or not it was wrapped.
    pub fn width(&self) -> Pixels {
        self.wrap_width
            .unwrap_or(Pixels::MAX)
            .min(self.unwrapped_layout.width)
    }

    /// The size of the whole wrapped text, for the given line_height.
    /// can span multiple lines if there are multiple wrap boundaries.
    pub fn size(&self, line_height: Pixels) -> Size<Pixels> {
        Size {
            width: self.width(),
            height: line_height * (self.wrap_boundaries.len() + 1),
        }
    }

    /// The ascent of a line in this layout
    pub fn ascent(&self) -> Pixels {
        self.unwrapped_layout.ascent
    }

    /// The descent of a line in this layout
    pub fn descent(&self) -> Pixels {
        self.unwrapped_layout.descent
    }

    /// The wrap boundaries in this layout
    pub fn wrap_boundaries(&self) -> &[WrapBoundary] {
        &self.wrap_boundaries
    }

    /// The font size of this layout
    pub fn font_size(&self) -> Pixels {
        self.unwrapped_layout.font_size
    }

    /// The runs in this layout, sans wrapping
    pub fn runs(&self) -> &[ShapedRun] {
        &self.unwrapped_layout.runs
    }

    /// The index corresponding to a given position in this layout for the given line height.
    ///
    /// See also [`Self::closest_index_for_position`].
    pub fn index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        self._index_for_position(position, line_height, false)
    }

    /// The closest index to a given position in this layout for the given line height.
    ///
    /// Closest means the character boundary closest to the given position.
    ///
    /// See also [`LineLayout::closest_index_for_x`].
    pub fn closest_index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        self._index_for_position(position, line_height, true)
    }

    fn _index_for_position(
        &self,
        mut position: Point<Pixels>,
        line_height: Pixels,
        closest: bool,
    ) -> Result<usize, usize> {
        let wrapped_line_ix = (position.y / line_height) as usize;

        let wrapped_line_start_index;
        let wrapped_line_start_x;
        if wrapped_line_ix > 0 {
            let Some(line_start_boundary) = self.wrap_boundaries.get(wrapped_line_ix - 1) else {
                return Err(0);
            };
            let run = &self.unwrapped_layout.runs[line_start_boundary.run_ix];
            let glyph = &run.glyphs[line_start_boundary.glyph_ix];
            wrapped_line_start_index = glyph.index;
            wrapped_line_start_x = glyph.position.x;
        } else {
            wrapped_line_start_index = 0;
            wrapped_line_start_x = Pixels::ZERO;
        };

        let wrapped_line_end_index;
        let wrapped_line_end_x;
        if wrapped_line_ix < self.wrap_boundaries.len() {
            let next_wrap_boundary_ix = wrapped_line_ix;
            let next_wrap_boundary = self.wrap_boundaries[next_wrap_boundary_ix];
            let run = &self.unwrapped_layout.runs[next_wrap_boundary.run_ix];
            let glyph = &run.glyphs[next_wrap_boundary.glyph_ix];
            wrapped_line_end_index = glyph.index;
            wrapped_line_end_x = glyph.position.x;
        } else {
            wrapped_line_end_index = self.unwrapped_layout.len;
            wrapped_line_end_x = self.unwrapped_layout.width;
        };

        let mut position_in_unwrapped_line = position;
        position_in_unwrapped_line.x += wrapped_line_start_x;
        if position_in_unwrapped_line.x < wrapped_line_start_x {
            Err(wrapped_line_start_index)
        } else if position_in_unwrapped_line.x >= wrapped_line_end_x {
            Err(wrapped_line_end_index)
        } else {
            if closest {
                Ok(self
                    .unwrapped_layout
                    .closest_index_for_x(position_in_unwrapped_line.x))
            } else {
                // The shaper can place a trailing zero-width wrap boundary glyph slightly past
                // the line's width, so the row can extend past where `index_for_x` has glyphs.
                self.unwrapped_layout
                    .index_for_x(position_in_unwrapped_line.x)
                    .ok_or(wrapped_line_end_index)
            }
        }
    }

    /// Returns the pixel position for the given byte index.
    pub fn position_for_index(&self, index: usize, line_height: Pixels) -> Option<Point<Pixels>> {
        let mut line_start_ix = 0;
        let mut line_end_indices = self
            .wrap_boundaries
            .iter()
            .map(|wrap_boundary| {
                let run = &self.unwrapped_layout.runs[wrap_boundary.run_ix];
                let glyph = &run.glyphs[wrap_boundary.glyph_ix];
                glyph.index
            })
            .chain([self.len()])
            .enumerate();
        for (ix, line_end_ix) in line_end_indices {
            let line_y = ix as f32 * line_height;
            if index < line_start_ix {
                break;
            } else if index > line_end_ix {
                line_start_ix = line_end_ix;
                continue;
            } else {
                let line_start_x = self.unwrapped_layout.x_for_index(line_start_ix);
                let x = self.unwrapped_layout.x_for_index(index) - line_start_x;
                return Some(point(x, line_y));
            }
        }

        None
    }
}

fn carry_over<K: Eq + Hash, V>(
    previous: &mut FxHashMap<Arc<K>, Arc<V>>,
    current: &mut FxHashMap<Arc<K>, Arc<V>>,
) {
    previous.retain(|_, layout| Arc::strong_count(layout) > 1);
    if previous.len() < current.len() {
        std::mem::swap(previous, current);
    }
    previous.extend(current.drain());
}

pub(crate) struct LineLayoutCache {
    previous_frame: Mutex<FrameCache>,
    current_frame: RwLock<FrameCache>,
    platform_text_system: Arc<dyn PlatformTextSystem>,
    /// Advances when [`TextSystem::add_fonts`] successfully changes the font database.
    font_generation: Arc<AtomicUsize>,
    /// Records the generation represented by both frame caches.
    cached_font_generation: AtomicUsize,
    recent_shapes: Mutex<RecentShapes>,
}

#[derive(Default)]
struct FrameCache {
    lines: FxHashMap<Arc<CacheKey>, Arc<LineLayout>>,
    wrapped_lines: FxHashMap<Arc<CacheKey>, Arc<WrappedLineLayout>>,
    used_lines: Vec<Arc<CacheKey>>,
    used_wrapped_lines: Vec<Arc<CacheKey>>,

    // Content-addressable caches keyed by caller-provided text hash + layout params.
    // These allow cache hits without materializing a contiguous `SharedString`.
    //
    // IMPORTANT: To support allocation-free lookups, we store these maps using a key type
    // (`HashedCacheKeyRef`) that can be computed without building a contiguous `&str`/`SharedString`.
    // On miss, we allocate once and store under an owned `HashedCacheKey`.
    lines_by_hash: FxHashMap<Arc<HashedCacheKey>, Arc<LineLayout>>,
    wrapped_lines_by_hash: FxHashMap<Arc<HashedCacheKey>, Arc<WrappedLineLayout>>,
    used_lines_by_hash: Vec<Arc<HashedCacheKey>>,
    used_wrapped_lines_by_hash: Vec<Arc<HashedCacheKey>>,
}

#[derive(Clone, Default)]
pub(crate) struct LineLayoutIndex {
    font_generation: usize,
    lines_index: usize,
    wrapped_lines_index: usize,
    lines_by_hash_index: usize,
    wrapped_lines_by_hash_index: usize,
}

impl LineLayoutCache {
    pub fn new(
        platform_text_system: Arc<dyn PlatformTextSystem>,
        font_generation: Arc<AtomicUsize>,
    ) -> Self {
        let cached_font_generation = font_generation.load(Ordering::Acquire);
        Self {
            previous_frame: Mutex::default(),
            current_frame: RwLock::default(),
            platform_text_system,
            font_generation,
            cached_font_generation: AtomicUsize::new(cached_font_generation),
            recent_shapes: Mutex::default(),
        }
    }

    pub fn layout_index(&self) -> LineLayoutIndex {
        let font_generation = self.clear_if_font_generation_changed();
        let frame = self.current_frame.read();
        LineLayoutIndex {
            font_generation,
            lines_index: frame.used_lines.len(),
            wrapped_lines_index: frame.used_wrapped_lines.len(),
            lines_by_hash_index: frame.used_lines_by_hash.len(),
            wrapped_lines_by_hash_index: frame.used_wrapped_lines_by_hash.len(),
        }
    }

    pub fn reuse_layouts(&self, range: Range<LineLayoutIndex>) {
        let font_generation = self.clear_if_font_generation_changed();
        if range.start.font_generation != font_generation
            || range.end.font_generation != font_generation
        {
            return;
        }
        let mut current_frame = &mut *self.current_frame.write();
        let mut previous_frame = &mut *self.previous_frame.lock();

        for key in &previous_frame.used_lines[range.start.lines_index..range.end.lines_index] {
            if let Some((key, line)) = previous_frame.lines.remove_entry(key) {
                current_frame.lines.insert(key, line);
            }
            current_frame.used_lines.push(key.clone());
        }

        for key in &previous_frame.used_wrapped_lines
            [range.start.wrapped_lines_index..range.end.wrapped_lines_index]
        {
            if let Some((key, line)) = previous_frame.wrapped_lines.remove_entry(key) {
                current_frame.wrapped_lines.insert(key, line);
            }
            current_frame.used_wrapped_lines.push(key.clone());
        }

        for key in &previous_frame.used_lines_by_hash
            [range.start.lines_by_hash_index..range.end.lines_by_hash_index]
        {
            if let Some((key, line)) = previous_frame.lines_by_hash.remove_entry(key) {
                current_frame.lines_by_hash.insert(key, line);
            }
            current_frame.used_lines_by_hash.push(key.clone());
        }

        for key in &previous_frame.used_wrapped_lines_by_hash
            [range.start.wrapped_lines_by_hash_index..range.end.wrapped_lines_by_hash_index]
        {
            if let Some((key, line)) = previous_frame.wrapped_lines_by_hash.remove_entry(key) {
                current_frame.wrapped_lines_by_hash.insert(key, line);
            }
            current_frame.used_wrapped_lines_by_hash.push(key.clone());
        }
    }

    pub fn truncate_layouts(&self, index: LineLayoutIndex) {
        let font_generation = self.clear_if_font_generation_changed();
        if index.font_generation != font_generation {
            return;
        }
        let mut current_frame = &mut *self.current_frame.write();
        current_frame.used_lines.truncate(index.lines_index);
        current_frame
            .used_wrapped_lines
            .truncate(index.wrapped_lines_index);
        current_frame
            .used_lines_by_hash
            .truncate(index.lines_by_hash_index);
        current_frame
            .used_wrapped_lines_by_hash
            .truncate(index.wrapped_lines_by_hash_index);
    }

    pub fn finish_frame(&self) {
        let _font_generation = self.clear_if_font_generation_changed();
        let mut current = self.current_frame.write();
        let mut previous = self.previous_frame.lock();
        let (previous, current) = (&mut *previous, &mut *current);

        carry_over(&mut previous.wrapped_lines, &mut current.wrapped_lines);
        carry_over(
            &mut previous.wrapped_lines_by_hash,
            &mut current.wrapped_lines_by_hash,
        );
        carry_over(&mut previous.lines, &mut current.lines);
        carry_over(&mut previous.lines_by_hash, &mut current.lines_by_hash);

        std::mem::swap(&mut previous.used_lines, &mut current.used_lines);
        std::mem::swap(
            &mut previous.used_wrapped_lines,
            &mut current.used_wrapped_lines,
        );
        std::mem::swap(
            &mut previous.used_lines_by_hash,
            &mut current.used_lines_by_hash,
        );
        std::mem::swap(
            &mut previous.used_wrapped_lines_by_hash,
            &mut current.used_wrapped_lines_by_hash,
        );
        current.used_lines.clear();
        current.used_wrapped_lines.clear();
        current.used_lines_by_hash.clear();
        current.used_wrapped_lines_by_hash.clear();
    }

    pub fn layout_wrapped_line<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        wrap_width: Option<Pixels>,
        max_lines: Option<usize>,
    ) -> Arc<WrappedLineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        let _font_generation = self.clear_if_font_generation_changed();
        let key = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width,
            max_lines,
            force_width: None,
        } as &dyn AsCacheKeyRef;

        let current_frame = self.current_frame.upgradable_read();
        if let Some(layout) = current_frame.wrapped_lines.get(key) {
            return layout.clone();
        }

        let previous_frame_entry = self.previous_frame.lock().wrapped_lines.remove_entry(key);
        if let Some((key, layout)) = previous_frame_entry {
            let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
            current_frame
                .wrapped_lines
                .insert(key.clone(), layout.clone());
            current_frame.used_wrapped_lines.push(key);
            layout
        } else {
            drop(current_frame);
            let text = SharedString::from(text);
            let unwrapped_layout = self.layout_line::<&SharedString>(&text, font_size, runs, None);
            let wrap_boundaries = if let Some(wrap_width) = wrap_width {
                unwrapped_layout.compute_wrap_boundaries(text.as_ref(), wrap_width, max_lines)
            } else {
                SmallVec::new()
            };
            let layout = Arc::new(WrappedLineLayout {
                unwrapped_layout,
                wrap_boundaries,
                wrap_width,
            });
            let key = Arc::new(CacheKey {
                text,
                font_size,
                runs: SmallVec::from(runs),
                wrap_width,
                max_lines,
                force_width: None,
            });

            let mut current_frame = self.current_frame.write();
            current_frame
                .wrapped_lines
                .insert(key.clone(), layout.clone());
            current_frame.used_wrapped_lines.push(key);

            layout
        }
    }

    pub fn layout_line<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
    ) -> Arc<LineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        let font_generation = self.clear_if_font_generation_changed();
        let key = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width: None,
            max_lines: None,
            force_width,
        } as &dyn AsCacheKeyRef;

        let current_frame = self.current_frame.upgradable_read();
        if let Some(layout) = current_frame.lines.get(key) {
            return layout.clone();
        }

        let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
        if let Some((key, layout)) = self.previous_frame.lock().lines.remove_entry(key) {
            current_frame.lines.insert(key.clone(), layout.clone());
            current_frame.used_lines.push(key);
            layout
        } else {
            let text = SharedString::from(text);
            let mut layout = self.shape_line(&text, font_size, runs, font_generation);

            if let Some(force_width) = force_width {
                apply_force_width_to_layout(&mut layout, force_width);
            }

            let key = Arc::new(CacheKey {
                text,
                font_size,
                runs: SmallVec::from(runs),
                wrap_width: None,
                max_lines: None,
                force_width,
            });
            let layout = Arc::new(layout);
            current_frame.lines.insert(key.clone(), layout.clone());
            current_frame.used_lines.push(key);
            layout
        }
    }

    /// Try to retrieve a previously-shaped line layout using a caller-provided content hash.
    ///
    /// This is a *non-allocating* cache probe: it does not materialize any text. If the layout
    /// is not already cached in either the current frame or previous frame, returns `None`.
    ///
    /// Contract (caller enforced):
    /// - Same `text_hash` implies identical text content (collision risk accepted by caller).
    /// - `text_len` should be the UTF-8 byte length of the text (helps reduce accidental collisions).
    pub fn try_layout_line_by_hash(
        &self,
        text_hash: u64,
        text_len: usize,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
    ) -> Option<Arc<LineLayout>> {
        let _font_generation = self.clear_if_font_generation_changed();
        let key_ref = HashedCacheKeyRef {
            text_hash,
            text_len,
            font_size,
            runs,
            wrap_width: None,
            force_width,
        };

        let current_frame = self.current_frame.read();
        if let Some((_, layout)) = current_frame.lines_by_hash.iter().find(|(key, _)| {
            HashedCacheKeyRef {
                text_hash: key.text_hash,
                text_len: key.text_len,
                font_size: key.font_size,
                runs: key.runs.as_slice(),
                wrap_width: key.wrap_width,
                force_width: key.force_width,
            } == key_ref
        }) {
            return Some(layout.clone());
        }

        let previous_frame = self.previous_frame.lock();
        if let Some((_, layout)) = previous_frame.lines_by_hash.iter().find(|(key, _)| {
            HashedCacheKeyRef {
                text_hash: key.text_hash,
                text_len: key.text_len,
                font_size: key.font_size,
                runs: key.runs.as_slice(),
                wrap_width: key.wrap_width,
                force_width: key.force_width,
            } == key_ref
        }) {
            return Some(layout.clone());
        }

        None
    }

    /// Layout a line of text using a caller-provided content hash as the cache key.
    ///
    /// This enables cache hits without materializing a contiguous `SharedString` for `text`.
    /// If the cache misses, `materialize_text` is invoked to produce the `SharedString` for shaping.
    ///
    /// Contract (caller enforced):
    /// - Same `text_hash` implies identical text content (collision risk accepted by caller).
    /// - `text_len` should be the UTF-8 byte length of the text (helps reduce accidental collisions).
    pub fn layout_line_by_hash(
        &self,
        text_hash: u64,
        text_len: usize,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
        materialize_text: impl FnOnce() -> SharedString,
    ) -> Arc<LineLayout> {
        let font_generation = self.clear_if_font_generation_changed();
        let key_ref = HashedCacheKeyRef {
            text_hash,
            text_len,
            font_size,
            runs,
            wrap_width: None,
            force_width,
        };

        // Fast path: already cached (no allocation).
        let current_frame = self.current_frame.upgradable_read();
        if let Some((_, layout)) = current_frame.lines_by_hash.iter().find(|(key, _)| {
            HashedCacheKeyRef {
                text_hash: key.text_hash,
                text_len: key.text_len,
                font_size: key.font_size,
                runs: key.runs.as_slice(),
                wrap_width: key.wrap_width,
                force_width: key.force_width,
            } == key_ref
        }) {
            return layout.clone();
        }

        let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);

        // Try to reuse from previous frame without allocating; do a linear scan to find a matching key.
        // (We avoid `drain()` here because it would eagerly move all entries.)
        let mut previous_frame = self.previous_frame.lock();
        if let Some(existing_key) = previous_frame
            .lines_by_hash
            .keys()
            .find(|key| {
                HashedCacheKeyRef {
                    text_hash: key.text_hash,
                    text_len: key.text_len,
                    font_size: key.font_size,
                    runs: key.runs.as_slice(),
                    wrap_width: key.wrap_width,
                    force_width: key.force_width,
                } == key_ref
            })
            .cloned()
        {
            if let Some((key, layout)) = previous_frame.lines_by_hash.remove_entry(&existing_key) {
                current_frame
                    .lines_by_hash
                    .insert(key.clone(), layout.clone());
                current_frame.used_lines_by_hash.push(key);
                return layout;
            }
        }

        let text = materialize_text();
        let mut layout = self.shape_line(&text, font_size, runs, font_generation);

        if let Some(force_width) = force_width {
            apply_force_width_to_layout(&mut layout, force_width);
        }

        let key = Arc::new(HashedCacheKey {
            text_hash,
            text_len,
            font_size,
            runs: SmallVec::from(runs),
            wrap_width: None,
            force_width,
        });
        let layout = Arc::new(layout);
        current_frame
            .lines_by_hash
            .insert(key.clone(), layout.clone());
        current_frame.used_lines_by_hash.push(key);
        layout
    }

    fn shape_line(
        &self,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
        font_generation: usize,
    ) -> LineLayout {
        let hash = RecentShapes::hash(text, font_size, runs);
        if let Some(layout) =
            self.recent_shapes
                .lock()
                .get(font_generation, hash, text, font_size, runs)
        {
            return layout;
        }
        let layout = self.platform_text_system.layout_line(text, font_size, runs);
        if self.font_generation.load(Ordering::Acquire) == font_generation {
            self.recent_shapes
                .lock()
                .insert(font_generation, hash, text, font_size, runs, &layout);
        }
        layout
    }

    fn clear_if_font_generation_changed(&self) -> usize {
        let font_generation = self.font_generation.load(Ordering::Acquire);
        if self.cached_font_generation.load(Ordering::Acquire) == font_generation {
            return font_generation;
        }

        let mut current_frame = self.current_frame.write();
        if self.cached_font_generation.load(Ordering::Acquire) == font_generation {
            return font_generation;
        }

        *current_frame = FrameCache::default();
        *self.previous_frame.lock() = FrameCache::default();
        *self.recent_shapes.lock() = RecentShapes::default();
        self.cached_font_generation
            .store(font_generation, Ordering::Release);
        font_generation
    }
}

// Combining marks (e.g. Thai vowel signs, Arabic diacritics) are shaped by
// HarfBuzz at the same x position as their base character. The force-width
// loop must not advance the cell counter for these zero-advance glyphs,
// otherwise they get displaced into the next cell. We detect them by checking
// whether shaped x has advanced by at least half a cell beyond the last base.
fn apply_force_width_to_layout(layout: &mut LineLayout, force_width: Pixels) {
    let mut glyph_pos: usize = 0;
    // NEG_INFINITY ensures the first glyph is always classified as a base.
    let mut last_base_shaped_x = px(f32::NEG_INFINITY);
    let mut last_base_actual_x = px(0.);

    for run in layout.runs.iter_mut() {
        for glyph in run.glyphs.iter_mut() {
            let shaped_x = glyph.position.x;

            if shaped_x > last_base_shaped_x + force_width * 0.5 {
                let forced_x = glyph_pos * force_width;
                if (shaped_x - forced_x).abs() > px(1.) {
                    glyph.position.x = forced_x;
                }
                last_base_shaped_x = shaped_x;
                last_base_actual_x = glyph.position.x;
                glyph_pos += 1;
            } else {
                glyph.position.x = last_base_actual_x + (shaped_x - last_base_shaped_x);
            }
        }
    }
}

/// A run of text with a single font.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[expect(missing_docs)]
pub struct FontRun {
    pub len: usize,
    pub font_id: FontId,
}

trait AsCacheKeyRef {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_>;
}

#[derive(Clone, Debug, Eq)]
struct CacheKey {
    text: SharedString,
    font_size: Pixels,
    runs: SmallVec<[FontRun; 1]>,
    wrap_width: Option<Pixels>,
    max_lines: Option<usize>,
    force_width: Option<Pixels>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct CacheKeyRef<'a> {
    text: &'a str,
    font_size: Pixels,
    runs: &'a [FontRun],
    wrap_width: Option<Pixels>,
    max_lines: Option<usize>,
    force_width: Option<Pixels>,
}

#[derive(Clone, Debug)]
struct HashedCacheKey {
    text_hash: u64,
    text_len: usize,
    font_size: Pixels,
    runs: SmallVec<[FontRun; 1]>,
    wrap_width: Option<Pixels>,
    force_width: Option<Pixels>,
}

#[derive(Copy, Clone)]
struct HashedCacheKeyRef<'a> {
    text_hash: u64,
    text_len: usize,
    font_size: Pixels,
    runs: &'a [FontRun],
    wrap_width: Option<Pixels>,
    force_width: Option<Pixels>,
}

impl PartialEq for dyn AsCacheKeyRef + '_ {
    fn eq(&self, other: &dyn AsCacheKeyRef) -> bool {
        self.as_cache_key_ref() == other.as_cache_key_ref()
    }
}

impl PartialEq for HashedCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.text_hash == other.text_hash
            && self.text_len == other.text_len
            && self.font_size == other.font_size
            && self.runs.as_slice() == other.runs.as_slice()
            && self.wrap_width == other.wrap_width
            && self.force_width == other.force_width
    }
}

impl Eq for HashedCacheKey {}

impl Hash for HashedCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text_hash.hash(state);
        self.text_len.hash(state);
        self.font_size.hash(state);
        self.runs.as_slice().hash(state);
        self.wrap_width.hash(state);
        self.force_width.hash(state);
    }
}

impl PartialEq for HashedCacheKeyRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.text_hash == other.text_hash
            && self.text_len == other.text_len
            && self.font_size == other.font_size
            && self.runs == other.runs
            && self.wrap_width == other.wrap_width
            && self.force_width == other.force_width
    }
}

impl Eq for HashedCacheKeyRef<'_> {}

impl Hash for HashedCacheKeyRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text_hash.hash(state);
        self.text_len.hash(state);
        self.font_size.hash(state);
        self.runs.hash(state);
        self.wrap_width.hash(state);
        self.force_width.hash(state);
    }
}

impl Eq for dyn AsCacheKeyRef + '_ {}

impl Hash for dyn AsCacheKeyRef + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_cache_key_ref().hash(state)
    }
}

impl AsCacheKeyRef for CacheKey {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_> {
        CacheKeyRef {
            text: &self.text,
            font_size: self.font_size,
            runs: self.runs.as_slice(),
            wrap_width: self.wrap_width,
            max_lines: self.max_lines,
            force_width: self.force_width,
        }
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.as_cache_key_ref().eq(&other.as_cache_key_ref())
    }
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_cache_key_ref().hash(state);
    }
}

impl<'a> Borrow<dyn AsCacheKeyRef + 'a> for Arc<CacheKey> {
    fn borrow(&self) -> &(dyn AsCacheKeyRef + 'a) {
        self.as_ref() as &dyn AsCacheKeyRef
    }
}

impl AsCacheKeyRef for CacheKeyRef<'_> {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_> {
        *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GlyphId;

    #[test]
    fn wrapped_line_cache_separates_line_clamps() {
        let text = "one two three four five";
        let runs = [FontRun {
            len: text.len(),
            font_id: FontId(1),
        }];
        for first_clamp in [None, Some(1), Some(2)] {
            let cache = LineLayoutCache::new(Arc::new(crate::NoopTextSystem), Arc::default());
            let first =
                cache.layout_wrapped_line(text, px(16.0), &runs, Some(px(30.0)), first_clamp);
            for max_lines in [None, Some(1), Some(2)] {
                let line =
                    cache.layout_wrapped_line(text, px(16.0), &runs, Some(px(30.0)), max_lines);
                let expected_boundaries = match max_lines {
                    None => cache
                        .layout_line(text, px(16.0), &runs, None)
                        .compute_wrap_boundaries(text, px(30.0), None)
                        .len(),
                    Some(lines) => lines - 1,
                };
                assert_eq!(line.wrap_boundaries.len(), expected_boundaries);
                assert_eq!(Arc::ptr_eq(&first, &line), first_clamp == max_lines);
            }
            cache.finish_frame();
            let line = cache.layout_wrapped_line(text, px(16.0), &runs, Some(px(30.0)), Some(1));
            assert!(line.wrap_boundaries.is_empty());
        }
    }

    fn held_line_cache() -> (LineLayoutCache, Arc<AtomicUsize>, [FontRun; 1]) {
        let generation = Arc::new(AtomicUsize::new(0));
        let cache = LineLayoutCache::new(Arc::new(crate::NoopTextSystem), generation.clone());
        let runs = [FontRun {
            len: 9,
            font_id: FontId(1),
        }];
        (cache, generation, runs)
    }

    #[test]
    fn held_lines_survive_idle_frames_and_are_removed_after_release() {
        let (cache, _, runs) = held_line_cache();
        let line = cache.layout_line("held line", px(16.0), &runs, None);
        for _ in 0..5 {
            cache.finish_frame();
        }
        let reused = cache.layout_line("held line", px(16.0), &runs, None);
        assert!(Arc::ptr_eq(&line, &reused));
        assert_eq!(cache.current_frame.read().used_lines.len(), 1);
        drop((line, reused));
        cache.finish_frame();
        cache.finish_frame();
        assert!(cache.previous_frame.lock().lines.is_empty());
        assert!(cache.current_frame.read().lines.is_empty());
    }

    #[test]
    fn held_hash_lines_survive_idle_frames_without_materialization() {
        let (cache, _, runs) = held_line_cache();
        let line =
            cache.layout_line_by_hash(17, 9, px(16.0), &runs, Some(px(8.0)), || "held line".into());
        for _ in 0..5 {
            cache.finish_frame();
        }
        assert!(cache.previous_frame.lock().used_lines_by_hash.is_empty());
        let probed = cache
            .try_layout_line_by_hash(17, 9, px(16.0), &runs, Some(px(8.0)))
            .unwrap();
        assert!(Arc::ptr_eq(&line, &probed));
        let reused = cache.layout_line_by_hash(17, 9, px(16.0), &runs, Some(px(8.0)), || {
            panic!("held hash line should not be materialized")
        });
        assert!(Arc::ptr_eq(&line, &reused));
        assert_eq!(cache.current_frame.read().used_lines_by_hash.len(), 1);
        assert!(
            cache
                .try_layout_line_by_hash(17, 9, px(16.0), &runs, None)
                .is_none()
        );
        drop((line, probed, reused));
        cache.finish_frame();
        cache.finish_frame();
        assert!(cache.previous_frame.lock().lines_by_hash.is_empty());
    }

    #[test]
    fn releasing_wrapped_lines_releases_their_unwrapped_cache_entries() {
        let (cache, _, runs) = held_line_cache();
        let wrapped = cache.layout_wrapped_line("held line", px(16.0), &runs, Some(px(20.0)), None);
        for _ in 0..5 {
            cache.finish_frame();
        }
        let reused = cache.layout_wrapped_line("held line", px(16.0), &runs, Some(px(20.0)), None);
        assert!(Arc::ptr_eq(&wrapped, &reused));
        cache.finish_frame();
        drop((wrapped, reused));
        cache.finish_frame();
        let previous = cache.previous_frame.lock();
        assert!(previous.wrapped_lines.is_empty());
        assert!(previous.lines.is_empty());
    }

    #[test]
    fn font_generation_invalidates_externally_held_layouts() {
        let (cache, generation, runs) = held_line_cache();
        let line = cache.layout_line("held line", px(16.0), &runs, None);
        let hashed = cache.layout_line_by_hash(17, 9, px(16.0), &runs, None, || "held line".into());
        let wrapped = cache.layout_wrapped_line("held line", px(16.0), &runs, Some(px(20.0)), None);
        cache.finish_frame();
        generation.fetch_add(1, Ordering::Release);
        cache.finish_frame();
        let fresh = cache.layout_line("held line", px(16.0), &runs, None);
        let fresh_hash =
            cache.layout_line_by_hash(17, 9, px(16.0), &runs, None, || "held line".into());
        let fresh_wrap =
            cache.layout_wrapped_line("held line", px(16.0), &runs, Some(px(20.0)), None);
        assert!(!Arc::ptr_eq(&line, &fresh));
        assert!(!Arc::ptr_eq(&hashed, &fresh_hash));
        assert!(!Arc::ptr_eq(&wrapped, &fresh_wrap));
    }

    struct RecentShapesTestPlatform {
        owner: u32,
        shaped: AtomicUsize,
        installed: AtomicUsize,
        native_generation: std::sync::atomic::AtomicU64,
        change_during_shape: Mutex<Option<Arc<AtomicUsize>>>,
    }

    impl RecentShapesTestPlatform {
        fn new(owner: u32) -> Self {
            Self {
                owner,
                shaped: AtomicUsize::new(0),
                installed: AtomicUsize::new(0),
                native_generation: std::sync::atomic::AtomicU64::new(0),
                change_during_shape: Mutex::default(),
            }
        }
    }

    impl PlatformTextSystem for RecentShapesTestPlatform {
        fn add_fonts(&self, _: Vec<std::borrow::Cow<'static, [u8]>>) -> crate::Result<()> {
            self.installed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn all_font_names(&self) -> Vec<String> {
            crate::NoopTextSystem.all_font_names()
        }

        fn font_id(&self, descriptor: &crate::Font) -> crate::Result<FontId> {
            crate::NoopTextSystem.font_id(descriptor)
        }

        fn font_generation(&self) -> u64 {
            self.native_generation.load(Ordering::Relaxed)
        }

        fn font_metrics(&self, font_id: FontId) -> crate::FontMetrics {
            crate::NoopTextSystem.font_metrics(font_id)
        }

        fn typographic_bounds(
            &self,
            font_id: FontId,
            glyph_id: GlyphId,
        ) -> crate::Result<crate::Bounds<f32>> {
            crate::NoopTextSystem.typographic_bounds(font_id, glyph_id)
        }

        fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> crate::Result<Size<f32>> {
            crate::NoopTextSystem.advance(font_id, glyph_id)
        }

        fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
            crate::NoopTextSystem.glyph_for_char(font_id, ch)
        }

        fn glyph_raster_bounds(
            &self,
            params: &crate::RenderGlyphParams,
        ) -> crate::Result<crate::Bounds<crate::DevicePixels>> {
            crate::NoopTextSystem.glyph_raster_bounds(params)
        }

        fn rasterize_glyph(
            &self,
            params: &crate::RenderGlyphParams,
            bounds: crate::Bounds<crate::DevicePixels>,
        ) -> crate::Result<(Size<crate::DevicePixels>, Vec<u8>)> {
            crate::NoopTextSystem.rasterize_glyph(params, bounds)
        }

        fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
            self.shaped.fetch_add(1, Ordering::Relaxed);
            let mut layout = crate::NoopTextSystem.layout_line(text, font_size, runs);
            let glyph_offset = self.owner
                + self.installed.load(Ordering::Relaxed) as u32
                + self.native_generation.load(Ordering::Relaxed) as u32;
            for glyph in layout.runs.iter_mut().flat_map(|run| &mut run.glyphs) {
                glyph.id.0 += glyph_offset;
            }
            if let Some(generation) = self.change_during_shape.lock().take() {
                generation.fetch_add(1, Ordering::Release);
            }
            layout
        }

        fn recommended_rendering_mode(
            &self,
            font_id: FontId,
            font_size: Pixels,
        ) -> crate::TextRenderingMode {
            crate::NoopTextSystem.recommended_rendering_mode(font_id, font_size)
        }
    }

    fn recent_shapes_window_system(
        owner: u32,
    ) -> (
        crate::WindowTextSystem,
        Arc<RecentShapesTestPlatform>,
        [crate::TextRun; 1],
    ) {
        let platform = Arc::new(RecentShapesTestPlatform::new(owner));
        let text_system = Arc::new(crate::TextSystem::new(platform.clone()));
        let runs = [crate::TextRun {
            len: 3,
            font: crate::font("Test Font"),
            ..crate::TextRun::default()
        }];
        (crate::WindowTextSystem::new(text_system), platform, runs)
    }

    #[test]
    fn recent_shapes_reuse_raw_geometry_after_frame_eviction_and_before_force_width() {
        let (system, platform, runs) = recent_shapes_window_system(100);
        let native = system.layout_line("abc", px(16.0), &runs, None);
        let forced = system.layout_line("abc", px(16.0), &runs, Some(px(8.0)));
        assert_eq!(platform.shaped.load(Ordering::Relaxed), 1);
        assert_eq!(forced.runs[0].glyphs[1].position.x, px(8.0));
        assert!(native.runs[0].glyphs[1].position.x > px(9.0));
        let native_position = native.runs[0].glyphs[1].position;
        let native_width = native.width;
        let original = Arc::downgrade(&native);
        drop((native, forced));
        for _ in 0..3 {
            system.finish_frame();
        }
        assert!(original.upgrade().is_none());
        let fresh = system.layout_line("abc", px(16.0), &runs, None);
        assert_eq!(platform.shaped.load(Ordering::Relaxed), 1);
        assert!(!original.ptr_eq(&Arc::downgrade(&fresh)));
        assert_eq!(fresh.runs[0].glyphs[1].position, native_position);
        assert_eq!(fresh.width, native_width);
    }

    #[test]
    fn recent_shapes_keep_hash_probe_and_materialization_contracts() {
        let (system, platform, runs) = recent_shapes_window_system(100);
        system.layout_line("abc", px(16.0), &runs, None);
        let materialized = AtomicUsize::new(0);
        let first = system.layout_line_by_hash(17, 3, px(16.0), &runs, None, || {
            materialized.fetch_add(1, Ordering::Relaxed);
            "abc".into()
        });
        assert_eq!(materialized.load(Ordering::Relaxed), 1);
        assert_eq!(platform.shaped.load(Ordering::Relaxed), 1);
        let current = system.layout_line_by_hash(17, 3, px(16.0), &runs, None, || {
            panic!("current hit must not materialize")
        });
        assert!(Arc::ptr_eq(&first, &current));
        system.finish_frame();
        let previous = system.layout_line_by_hash(17, 3, px(16.0), &runs, None, || {
            panic!("previous hit must not materialize")
        });
        assert!(Arc::ptr_eq(&first, &previous));
        let original_id = first.runs[0].glyphs[0].id;
        let original = Arc::downgrade(&first);
        drop((first, current, previous));
        for _ in 0..3 {
            system.finish_frame();
        }
        assert!(original.upgrade().is_none());
        assert!(
            system
                .try_layout_line_by_hash(17, 3, px(16.0), &runs, None)
                .is_none()
        );
        let fresh = system.layout_line_by_hash(17, 3, px(16.0), &runs, None, || {
            materialized.fetch_add(1, Ordering::Relaxed);
            "abc".into()
        });
        assert_eq!(materialized.load(Ordering::Relaxed), 2);
        assert_eq!(platform.shaped.load(Ordering::Relaxed), 1);
        assert_eq!(fresh.runs[0].glyphs[0].id, original_id);
        assert!(!original.ptr_eq(&Arc::downgrade(&fresh)));
    }

    #[test]
    fn recent_shapes_isolate_owners_and_invalidate_on_installed_and_native_fonts() {
        let (first, first_platform, runs) = recent_shapes_window_system(100);
        let (second, second_platform, _) = recent_shapes_window_system(200);
        let original_first = first.layout_line("abc", px(16.0), &runs, None);
        let original_second = second.layout_line("abc", px(16.0), &runs, None);
        let original_first_id = original_first.runs[0].glyphs[0].id;
        let original_second_id = original_second.runs[0].glyphs[0].id;
        let second_before = Arc::downgrade(&original_second);
        assert_ne!(original_first_id, original_second_id);
        drop((original_first, original_second));
        for _ in 0..3 {
            first.finish_frame();
            second.finish_frame();
        }
        assert!(second_before.upgrade().is_none());
        first.add_fonts(Vec::new()).unwrap();
        let installed = first.layout_line("abc", px(16.0), &runs, None);
        let second_hit = second.layout_line("abc", px(16.0), &runs, None);
        assert_ne!(installed.runs[0].glyphs[0].id, original_first_id);
        assert_eq!(second_hit.runs[0].glyphs[0].id, original_second_id);
        assert_eq!(first_platform.shaped.load(Ordering::Relaxed), 2);
        assert_eq!(second_platform.shaped.load(Ordering::Relaxed), 1);
        first_platform
            .native_generation
            .fetch_add(1, Ordering::Relaxed);
        let native = first.layout_line("abc", px(16.0), &runs, None);
        assert_ne!(native.runs[0].glyphs[0].id, installed.runs[0].glyphs[0].id);
        assert_eq!(first_platform.shaped.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn recent_shapes_do_not_insert_inflight_old_font_generation_results() {
        let platform = Arc::new(RecentShapesTestPlatform::new(100));
        let generation = Arc::new(AtomicUsize::new(0));
        *platform.change_during_shape.lock() = Some(generation.clone());
        let cache = LineLayoutCache::new(platform.clone(), generation);
        let runs = [FontRun {
            len: 3,
            font_id: FontId(1),
        }];
        cache.layout_line("abc", px(16.0), &runs, None);
        let hash = RecentShapes::hash("abc", px(16.0), &runs);
        assert!(
            cache
                .recent_shapes
                .lock()
                .get(0, hash, "abc", px(16.0), &runs)
                .is_none()
        );
        cache.layout_line("abc", px(16.0), &runs, None);
        assert_eq!(platform.shaped.load(Ordering::Relaxed), 2);
    }

    fn glyph_at(x: f32, index: usize) -> ShapedGlyph {
        ShapedGlyph {
            id: GlyphId(0),
            position: point(px(x), px(0.)),
            index,
            is_emoji: false,
        }
    }

    fn make_layout(glyphs: Vec<ShapedGlyph>) -> LineLayout {
        LineLayout {
            font_size: px(16.),
            width: px(100.),
            ascent: px(12.),
            descent: px(4.),
            runs: vec![ShapedRun {
                font_id: FontId(0),
                glyphs,
            }],
            len: 0,
        }
    }

    fn glyph_x_positions(layout: &LineLayout) -> Vec<f32> {
        layout.runs[0]
            .glyphs
            .iter()
            .map(|g| f32::from(g.position.x))
            .collect()
    }

    #[test]
    fn test_force_width_latin_unchanged() {
        let cell_width = px(8.);
        let mut layout = make_layout(vec![glyph_at(0., 0), glyph_at(8., 1), glyph_at(16., 2)]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0., 8., 16.]);
    }

    #[test]
    fn test_force_width_combining_marks_not_advanced() {
        let cell_width = px(8.);
        // Simulates Thai "กี" — base consonant at x=0, combining vowel also at x=0
        let mut layout = make_layout(vec![
            glyph_at(0., 0), // ก (base)
            glyph_at(0., 3), // ี (combining mark, same x)
        ]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0., 0.]);
    }

    #[test]
    fn test_force_width_base_after_combining_mark() {
        let cell_width = px(8.);
        let mut layout = make_layout(vec![glyph_at(0., 0), glyph_at(0., 3), glyph_at(8., 6)]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0., 0., 8.]);
    }

    #[test]
    fn test_force_width_multiple_combining_marks() {
        let cell_width = px(8.);
        // Simulates "ก้" — base + vowel + tone mark (two combining marks stacked)
        let mut layout = make_layout(vec![
            glyph_at(0., 0), // ก (base)
            glyph_at(0., 3), // vowel (combining)
            glyph_at(0., 6), // tone mark (combining)
            glyph_at(8., 9), // next base
        ]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0., 0., 0., 8.]);
    }

    #[test]
    fn test_force_width_corrects_drifted_base_positions() {
        let cell_width = px(8.);
        // Font metrics don't perfectly match cell grid — glyphs drift >1px from cell boundary
        let mut layout = make_layout(vec![
            glyph_at(0.5, 0),  // within 1px tolerance, kept as-is
            glyph_at(10.2, 1), // >1px off from 8.0, corrected
            glyph_at(19.8, 2), // >1px off from 16.0, corrected
        ]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0.5, 8., 16.]);
    }

    #[test]
    fn test_force_width_combining_mark_after_within_tolerance_base() {
        let cell_width = px(8.);
        // Base glyph is within 1px of grid so it keeps its shaped position.
        // The combining mark must align to the base's actual position, not the grid slot.
        let mut layout = make_layout(vec![glyph_at(0.5, 0), glyph_at(0.5, 3)]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0.5, 0.5]);
    }
}
