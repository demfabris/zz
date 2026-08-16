use std::{
    collections::HashMap,
    ops::Range,
    sync::{Arc, Mutex},
};

use gpui::{App, EntityId, TextRun};
use instant::Instant;

use crate::pulse::pulse_lease;

const EMA_SEED_MS: f32 = 160.0;
const MIN_FADE_MS: f32 = 120.0;
const MAX_FADE_MS: f32 = 400.0;
const CURVE_POWER: f32 = 1.6;
const MAX_GAP_MS: f32 = 1_000.0;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) struct VeilKey {
    source_start: usize,
    element: usize,
}

impl VeilKey {
    pub(super) fn new(source_start: usize, element: usize) -> Self {
        Self {
            source_start,
            element,
        }
    }
}

#[derive(Clone, Debug)]
struct Chunk {
    range: Range<usize>,
    started: Instant,
    duration_ms: f32,
}

#[derive(Debug)]
struct ElementVeil {
    previous: String,
    chunks: Vec<Chunk>,
    ema_ms: f32,
    last_append: Option<Instant>,
}

impl Default for ElementVeil {
    fn default() -> Self {
        Self {
            previous: String::new(),
            chunks: Vec::new(),
            ema_ms: EMA_SEED_MS,
            last_append: None,
        }
    }
}

impl ElementVeil {
    fn seed(&mut self, text: &str) {
        self.previous.clear();
        self.previous.push_str(text);
        self.chunks.clear();
        self.ema_ms = EMA_SEED_MS;
        self.last_append = None;
    }

    fn advance(&mut self, text: &str, now: Instant) -> Vec<(Range<usize>, f32)> {
        if text != self.previous {
            let prefix = common_prefix(&self.previous, text);
            self.chunks.retain_mut(|chunk| {
                chunk.range.end = chunk.range.end.min(prefix);
                chunk.range.start < chunk.range.end
            });
            if text.len() > prefix {
                if let Some(last_append) = self.last_append {
                    let gap_ms = now.saturating_duration_since(last_append).as_secs_f32() * 1_000.0;
                    self.ema_ms = ema_next(self.ema_ms, gap_ms);
                }
                self.last_append = Some(now);
                self.chunks.push(Chunk {
                    range: prefix..text.len(),
                    started: now,
                    duration_ms: fade_duration_ms(self.ema_ms),
                });
            }
            self.previous.clear();
            self.previous.push_str(text);
        }

        let boost = chunk_boost(self.chunks.len());
        self.chunks.retain(|chunk| {
            now.saturating_duration_since(chunk.started).as_secs_f32() * 1_000.0 * boost
                < chunk.duration_ms
        });
        let boost = chunk_boost(self.chunks.len());
        self.chunks
            .iter()
            .map(|chunk| {
                let elapsed_ms =
                    now.saturating_duration_since(chunk.started).as_secs_f32() * 1_000.0;
                let progress = (elapsed_ms * boost / chunk.duration_ms).clamp(0.0, 1.0);
                (chunk.range.clone(), veil_opacity(progress))
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub(super) struct RowVeil {
    elements: HashMap<VeilKey, ElementVeil>,
}

impl RowVeil {
    fn advance(
        &mut self,
        key: VeilKey,
        text: &str,
        now: Instant,
        seed: bool,
        baseline_source_len: usize,
    ) -> Vec<(Range<usize>, f32)> {
        let seed =
            seed || (!self.elements.contains_key(&key) && key.source_start < baseline_source_len);
        let element = self.elements.entry(key).or_default();
        if seed {
            element.seed(text);
            Vec::new()
        } else {
            element.advance(text, now)
        }
    }
}

#[derive(Clone)]
pub(super) struct StreamingVeil {
    state: Arc<Mutex<RowVeil>>,
    view: EntityId,
    seed: bool,
    baseline_source_len: usize,
}

impl StreamingVeil {
    pub(super) fn new(
        state: Arc<Mutex<RowVeil>>,
        view: EntityId,
        seed: bool,
        baseline_source_len: usize,
    ) -> Self {
        Self {
            state,
            view,
            seed,
            baseline_source_len,
        }
    }

    pub(super) fn runs(
        &self,
        key: VeilKey,
        text: &str,
        runs: Vec<TextRun>,
        cx: &mut App,
    ) -> Vec<TextRun> {
        let spans = self.state.lock().map_or_else(
            |_| Vec::new(),
            |mut state| {
                state.advance(
                    key,
                    text,
                    Instant::now(),
                    self.seed,
                    self.baseline_source_len,
                )
            },
        );
        if !spans.is_empty() {
            pulse_lease(self.view, cx);
        }
        apply_veil(runs, &spans)
    }
}

fn common_prefix(left: &str, right: &str) -> usize {
    let mut prefix = left
        .as_bytes()
        .iter()
        .zip(right.as_bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while prefix > 0 && !right.is_char_boundary(prefix) {
        prefix -= 1;
    }
    prefix
}

fn veil_opacity(progress: f32) -> f32 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powf(CURVE_POWER)
}

fn fade_duration_ms(ema_ms: f32) -> f32 {
    (ema_ms * 3.0).clamp(MIN_FADE_MS, MAX_FADE_MS)
}

fn chunk_boost(active_chunks: usize) -> f32 {
    1.0 + 0.3 * active_chunks.saturating_sub(2) as f32
}

fn ema_next(ema_ms: f32, gap_ms: f32) -> f32 {
    ema_ms * 0.7 + gap_ms.min(MAX_GAP_MS) * 0.3
}

fn apply_veil(runs: Vec<TextRun>, spans: &[(Range<usize>, f32)]) -> Vec<TextRun> {
    if spans.is_empty() || spans.iter().all(|(_, opacity)| *opacity >= 1.0) {
        return runs;
    }

    let mut output = Vec::with_capacity(runs.len() + spans.len() * 2);
    let mut position = 0;
    for run in runs {
        let start = position;
        let end = start + run.len;
        position = end;
        let mut cuts = vec![start, end];
        for (range, _) in spans {
            if range.start > start && range.start < end {
                cuts.push(range.start);
            }
            if range.end > start && range.end < end {
                cuts.push(range.end);
            }
        }
        cuts.sort_unstable();
        cuts.dedup();

        for interval in cuts.windows(2) {
            let (piece_start, piece_end) = (interval[0], interval[1]);
            let mut piece = run.clone();
            piece.len = piece_end - piece_start;
            if let Some(opacity) = spans
                .iter()
                .find(|(range, _)| range.start <= piece_start && piece_end <= range.end)
                .map(|(_, opacity)| *opacity)
                .filter(|opacity| *opacity < 1.0)
            {
                piece.color = piece.color.opacity(opacity);
                piece.background_color = piece.background_color.map(|color| color.opacity(opacity));
                if let Some(underline) = &mut piece.underline {
                    underline.color = underline.color.map(|color| color.opacity(opacity));
                }
                if let Some(strikethrough) = &mut piece.strikethrough {
                    strikethrough.color = strikethrough.color.map(|color| color.opacity(opacity));
                }
            }
            output.push(piece);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TextRun, font};
    use instant::Duration;

    fn at(base: Instant, millis: u64) -> Instant {
        base + Duration::from_millis(millis)
    }

    fn run(len: usize) -> TextRun {
        TextRun {
            len,
            font: font("Test"),
            color: gpui::white(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    #[test]
    fn appended_chunks_fade_once_and_independently() {
        let start = Instant::now();
        let mut veil = ElementVeil::default();
        assert_eq!(veil.advance("one ", start), vec![(0..4, 0.0)]);
        let spans = veil.advance("one two", at(start, 100));
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].0, 0..4);
        assert_eq!(spans[1].0, 4..7);
        assert!(spans[0].1 > spans[1].1);
        assert!(veil.advance("one two", at(start, 600)).is_empty());
    }

    #[test]
    fn seeded_elements_do_not_refade_existing_content() {
        let start = Instant::now();
        let mut veil = RowVeil::default();
        let key = VeilKey::new(0, 0);
        assert!(veil.advance(key, "already here", start, true, 0).is_empty());
        assert_eq!(
            veil.advance(key, "already here plus", at(start, 100), false, 0),
            vec![(12..17, 0.0)]
        );
    }

    #[test]
    fn unseen_elements_inside_the_attach_baseline_start_at_full_opacity() {
        let start = Instant::now();
        let mut veil = RowVeil::default();
        let existing = VeilKey::new(4, 0);
        let appended = VeilKey::new(12, 0);

        assert!(
            veil.advance(existing, "existing", start, false, 12)
                .is_empty()
        );
        assert_eq!(
            veil.advance(existing, "existing tail", at(start, 100), false, 12),
            vec![(8..13, 0.0)]
        );
        assert_eq!(
            veil.advance(appended, "new", at(start, 100), false, 12),
            vec![(0..3, 0.0)]
        );
    }

    #[test]
    fn markdown_rewrites_keep_the_common_prefix() {
        let start = Instant::now();
        let mut veil = ElementVeil::default();
        veil.advance("intro **bol", start);
        let spans = veil.advance("intro bold", at(start, 100));
        assert_eq!(spans[0].0, 0..6);
        assert_eq!(spans[1].0, 6..10);
    }

    #[test]
    fn veil_splits_runs_without_changing_layout_lengths() {
        let runs = vec![run(4), run(6)];
        let faded = apply_veil(runs.clone(), &[(2..8, 0.5)]);
        assert_eq!(
            faded.iter().map(|run| run.len).collect::<Vec<_>>(),
            vec![2, 2, 4, 2]
        );
        assert_eq!(faded.iter().map(|run| run.len).sum::<usize>(), 10);
        assert!(faded.iter().all(|run| run.font == runs[0].font));
        assert_eq!(faded[0].color.a, 1.0);
        assert_eq!(faded[1].color.a, 0.5);
    }

    #[test]
    fn cadence_curve_is_bounded() {
        assert_eq!(fade_duration_ms(160.0), 400.0);
        assert_eq!(fade_duration_ms(30.0), 120.0);
        assert_eq!(chunk_boost(2), 1.0);
        assert!((chunk_boost(3) - 1.3).abs() < f32::EPSILON);
        assert_eq!(veil_opacity(0.0), 0.0);
        assert_eq!(veil_opacity(1.0), 1.0);
        assert!(veil_opacity(0.5) > 0.5);
    }
}
