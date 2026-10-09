use super::{FontRun, LineLayout, ShapedGlyph, ShapedRun};
use crate::Pixels;
use collections::{FxHashMap, FxHasher};
use smallvec::SmallVec;
use std::{
    hash::{Hash, Hasher},
    mem,
};

const GLYPHS_PER_GENERATION: usize = 16 * 1024;
const BYTES_PER_GENERATION: usize = 1024 * 1024;

struct RecentShape {
    text: Box<str>,
    font_size: Pixels,
    runs: SmallVec<[FontRun; 1]>,
    layout: LineLayout,
}

#[derive(Default)]
pub(super) struct RecentShapes {
    current: FxHashMap<u64, SmallVec<[RecentShape; 1]>>,
    current_glyphs: usize,
    current_bytes: usize,
    old: FxHashMap<u64, SmallVec<[RecentShape; 1]>>,
    font_generation: usize,
}

impl RecentShapes {
    pub(super) fn hash(text: &str, font_size: Pixels, runs: &[FontRun]) -> u64 {
        let mut hasher = FxHasher::default();
        text.hash(&mut hasher);
        font_size.0.to_bits().hash(&mut hasher);
        runs.hash(&mut hasher);
        hasher.finish()
    }

    pub(super) fn get(
        &mut self,
        font_generation: usize,
        hash: u64,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
    ) -> Option<LineLayout> {
        if font_generation != self.font_generation {
            *self = Self {
                font_generation,
                ..Self::default()
            };
            return None;
        }
        let matches = |shape: &RecentShape| {
            &*shape.text == text
                && shape.font_size.0.to_bits() == font_size.0.to_bits()
                && shape.runs.as_slice() == runs
        };
        if let Some(shape) = self
            .current
            .get(&hash)
            .and_then(|shapes| shapes.iter().find(|shape| matches(shape)))
        {
            return Some(copy_layout(&shape.layout));
        }
        let shapes = self.old.get_mut(&hash)?;
        let ix = shapes.iter().position(matches)?;
        let shape = shapes.swap_remove(ix);
        if shapes.is_empty() {
            self.old.remove(&hash);
        }
        let layout = copy_layout(&shape.layout);
        self.push(hash, shape);
        Some(layout)
    }

    pub(super) fn insert(
        &mut self,
        font_generation: usize,
        hash: u64,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
        layout: &LineLayout,
    ) {
        if self.font_generation != font_generation {
            return;
        }
        let (glyphs, bytes) = shape_weight(text.len(), runs.len(), layout);
        if glyphs > GLYPHS_PER_GENERATION || bytes > BYTES_PER_GENERATION {
            return;
        }
        self.push(
            hash,
            RecentShape {
                text: text.into(),
                font_size,
                runs: SmallVec::from(runs),
                layout: copy_layout(layout),
            },
        );
    }

    fn push(&mut self, hash: u64, shape: RecentShape) {
        let (glyphs, bytes) = shape_weight(shape.text.len(), shape.runs.len(), &shape.layout);
        if self.current_glyphs + glyphs > GLYPHS_PER_GENERATION
            || self.current_bytes + bytes > BYTES_PER_GENERATION
        {
            self.old = mem::take(&mut self.current);
            self.current_glyphs = 0;
            self.current_bytes = 0;
        }
        self.current_glyphs += glyphs;
        self.current_bytes += bytes;
        self.current.entry(hash).or_default().push(shape);
    }
}

fn shape_weight(text_len: usize, font_runs: usize, layout: &LineLayout) -> (usize, usize) {
    let glyphs = layout.runs.iter().fold(0usize, |glyphs, run| {
        glyphs.saturating_add(run.glyphs.len())
    });
    let bytes = mem::size_of::<RecentShape>()
        .saturating_add(text_len)
        .saturating_add(font_runs.saturating_mul(mem::size_of::<FontRun>()))
        .saturating_add(
            layout
                .runs
                .len()
                .saturating_mul(mem::size_of::<ShapedRun>()),
        )
        .saturating_add(glyphs.saturating_mul(mem::size_of::<ShapedGlyph>()));
    (glyphs.max(1), bytes)
}

fn copy_layout(layout: &LineLayout) -> LineLayout {
    let LineLayout {
        font_size,
        width,
        ascent,
        descent,
        runs,
        len,
    } = layout;
    LineLayout {
        font_size: *font_size,
        width: *width,
        ascent: *ascent,
        descent: *descent,
        runs: runs.clone(),
        len: *len,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FontId, GlyphId, point, px};

    fn layout(glyphs: usize) -> LineLayout {
        LineLayout {
            font_size: px(16.0),
            width: px(glyphs as f32 * 8.0),
            ascent: px(12.0),
            descent: px(4.0),
            len: glyphs,
            runs: vec![ShapedRun {
                font_id: FontId(1),
                glyphs: (0..glyphs)
                    .map(|index| ShapedGlyph {
                        id: GlyphId(index as u32),
                        position: point(px(index as f32 * 8.0), px(0.25)),
                        index,
                        is_emoji: index % 2 == 0,
                    })
                    .collect(),
            }],
        }
    }

    fn runs() -> [FontRun; 1] {
        [FontRun {
            len: 3,
            font_id: FontId(1),
        }]
    }

    fn assert_bounded(cache: &RecentShapes) {
        for generation in [&cache.current, &cache.old] {
            let (glyphs, bytes) =
                generation
                    .values()
                    .flatten()
                    .fold((0, 0), |(glyphs, bytes), shape| {
                        let weight =
                            shape_weight(shape.text.len(), shape.runs.len(), &shape.layout);
                        (glyphs + weight.0, bytes + weight.1)
                    });
            assert!(glyphs <= GLYPHS_PER_GENERATION);
            assert!(bytes <= BYTES_PER_GENERATION);
        }
    }

    #[test]
    fn recent_shapes_verify_collisions_and_copy_all_layout_fields() {
        let mut cache = RecentShapes::default();
        let runs = runs();
        let original = layout(3);
        cache.insert(0, 17, "abc", px(16.0), &runs, &original);
        let mut different = layout(2);
        different.runs[0].font_id = FontId(2);
        cache.insert(0, 17, "xyz", px(16.0), &runs, &different);
        let mut copied = cache.get(0, 17, "abc", px(16.0), &runs).unwrap();
        assert_eq!(copied.font_size, original.font_size);
        assert_eq!(copied.width, original.width);
        assert_eq!(copied.ascent, original.ascent);
        assert_eq!(copied.descent, original.descent);
        assert_eq!(copied.len, original.len);
        assert_eq!(copied.runs[0].font_id, original.runs[0].font_id);
        for (copied, original) in copied.runs[0].glyphs.iter().zip(&original.runs[0].glyphs) {
            assert_eq!(copied.id, original.id);
            assert_eq!(copied.position, original.position);
            assert_eq!(copied.index, original.index);
            assert_eq!(copied.is_emoji, original.is_emoji);
        }
        copied.runs[0].glyphs[0].position.x = px(999.0);
        let fresh = cache.get(0, 17, "abc", px(16.0), &runs).unwrap();
        assert_eq!(
            fresh.runs[0].glyphs[0].position,
            original.runs[0].glyphs[0].position
        );
        assert_eq!(
            cache.get(0, 17, "xyz", px(16.0), &runs).unwrap().runs[0].font_id,
            FontId(2)
        );
        assert!(cache.get(0, 17, "abc", px(20.0), &runs).is_none());
        assert!(
            cache
                .get(
                    0,
                    17,
                    "abc",
                    px(16.0),
                    &[FontRun {
                        font_id: FontId(2),
                        ..runs[0]
                    }]
                )
                .is_none()
        );
        assert!(cache.get(0, 17, "unrelated", px(16.0), &runs).is_none());
    }

    #[test]
    fn recent_shapes_promote_hits_and_evict_old_generations() {
        let mut cache = RecentShapes::default();
        let runs = runs();
        let full = layout(GLYPHS_PER_GENERATION);
        cache.insert(0, 1, "one", px(16.0), &runs, &full);
        cache.insert(0, 2, "two", px(16.0), &runs, &full);
        assert!(cache.old.contains_key(&1));
        assert!(cache.get(0, 1, "one", px(16.0), &runs).is_some());
        assert!(cache.current.contains_key(&1));
        assert!(cache.old.contains_key(&2));
        cache.insert(0, 3, "new", px(16.0), &runs, &full);
        assert!(cache.get(0, 2, "two", px(16.0), &runs).is_none());
        assert_bounded(&cache);
    }

    #[test]
    fn recent_shapes_skip_oversized_lines_and_bound_empty_layout_storage() {
        let mut cache = RecentShapes::default();
        let runs = runs();
        cache.insert(
            0,
            1,
            "big",
            px(16.0),
            &runs,
            &layout(GLYPHS_PER_GENERATION + 1),
        );
        let huge_text = "x".repeat(BYTES_PER_GENERATION + 1);
        cache.insert(0, 2, &huge_text, px(16.0), &runs, &layout(0));
        assert!(cache.current.is_empty());
        assert!(cache.old.is_empty());
        let long_text = "x".repeat(BYTES_PER_GENERATION / 2);
        cache.insert(0, 3, &long_text, px(16.0), &runs, &layout(0));
        cache.insert(0, 4, &long_text, px(16.0), &runs, &layout(0));
        assert!(cache.old.contains_key(&3));
        assert!(cache.current.contains_key(&4));
        assert_bounded(&cache);
        for index in 5..10005 {
            let text = index.to_string();
            cache.insert(0, index, &text, px(16.0), &runs, &layout(0));
            assert!(cache.current_glyphs <= GLYPHS_PER_GENERATION);
            assert!(cache.current_bytes <= BYTES_PER_GENERATION);
        }
        assert_bounded(&cache);
    }

    #[test]
    fn recent_shapes_generation_changes_clear_both_maps_and_reject_stale_inserts() {
        let mut cache = RecentShapes::default();
        let runs = runs();
        let full = layout(GLYPHS_PER_GENERATION);
        cache.insert(0, 1, "one", px(16.0), &runs, &full);
        cache.insert(0, 2, "two", px(16.0), &runs, &full);
        assert!(cache.get(1, 1, "one", px(16.0), &runs).is_none());
        assert!(cache.current.is_empty());
        assert!(cache.old.is_empty());
        cache.insert(0, 1, "one", px(16.0), &runs, &full);
        assert!(cache.current.is_empty());
        cache.insert(1, 1, "one", px(16.0), &runs, &layout(3));
        assert!(cache.get(1, 1, "one", px(16.0), &runs).is_some());
    }
}
