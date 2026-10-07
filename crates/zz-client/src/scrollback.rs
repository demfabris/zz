use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use web_time::Instant;
use zz_protocol::PaneId;
use zz_terminal::{
    GRAPHEME_TABLE_BIT, PackedCell, PatchError, ScrollbarState, TerminalDictionary,
    TerminalViewport, TerminalViewportPatch,
};

pub const MAX_HISTORY_ROWS: usize = 10_000;
const MIN_HISTORY_DICTIONARY_COMPACTION_BYTES: usize = 1024;
pub const MAX_HISTORY_CHUNK_ROWS: u32 = 512;
pub const HISTORY_BACKFILL_QUIET: Duration = Duration::from_millis(100);
pub const HISTORY_REQUEST_RETRY: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct HistoryRow {
    pub cells: Box<[PackedCell]>,
    pub dictionary: Arc<TerminalDictionary>,
    pub revision: u64,
}

#[derive(Debug)]
pub struct HistoryRing {
    pub rows: VecDeque<HistoryRow>,
    limit: usize,
}

impl Default for HistoryRing {
    fn default() -> Self {
        Self::with_limit(MAX_HISTORY_ROWS)
    }
}

impl HistoryRing {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            rows: VecDeque::new(),
            limit: limit.min(MAX_HISTORY_ROWS),
        }
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }

    fn push_back(&mut self, row: HistoryRow) {
        self.rows.push_back(row);
    }

    fn enforce_cap(&mut self) {
        while self.rows.len() > self.limit {
            self.rows.pop_front();
        }
    }

    fn prepend(
        &mut self,
        rows: Vec<Vec<PackedCell>>,
        dictionary: TerminalDictionary,
        next_row_revision: &mut u64,
    ) {
        let dictionary = Arc::new(dictionary);
        for cells in rows.into_iter().rev() {
            self.rows.push_front(HistoryRow {
                cells: cells.into_boxed_slice(),
                dictionary: Arc::clone(&dictionary),
                revision: allocate_row_revision(next_row_revision),
            });
        }
        self.enforce_cap();
    }

    fn clear(&mut self) {
        self.rows.clear();
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

fn compact_history_dictionary(
    rows: &mut VecDeque<HistoryRow>,
    source: &Arc<TerminalDictionary>,
    current: &TerminalDictionary,
) {
    let style_bytes = if Arc::ptr_eq(&source.styles, &current.styles) {
        0
    } else {
        std::mem::size_of_val(source.styles.as_ref())
    };
    let grapheme_bytes = if !Arc::ptr_eq(&source.grapheme_offsets, &current.grapheme_offsets)
        || !Arc::ptr_eq(&source.grapheme_bytes, &current.grapheme_bytes)
    {
        std::mem::size_of_val(source.grapheme_offsets.as_ref()) + source.grapheme_bytes.len()
    } else {
        0
    };
    let styles_start = compact_history_plane_start(rows, style_bytes, |dictionary| {
        Arc::ptr_eq(&dictionary.styles, &source.styles)
    });
    let graphemes_start = compact_history_plane_start(rows, grapheme_bytes, |dictionary| {
        Arc::ptr_eq(&dictionary.grapheme_offsets, &source.grapheme_offsets)
            && Arc::ptr_eq(&dictionary.grapheme_bytes, &source.grapheme_bytes)
    });
    let start = styles_start.min(graphemes_start);
    if start == rows.len() {
        return;
    }

    let mut style_ids = HashMap::new();
    let mut styles = Vec::new();
    let mut grapheme_ids = HashMap::new();
    let mut grapheme_offsets = vec![0];
    let mut grapheme_bytes = Vec::new();
    for (offset, row) in rows.range(start..).enumerate() {
        let index = start + offset;
        for cell in &row.cells {
            if index >= styles_start
                && let std::collections::hash_map::Entry::Vacant(entry) =
                    style_ids.entry(cell.style_id())
            {
                let Some(style) = source.styles.get(usize::from(cell.style_id())) else {
                    return;
                };
                entry.insert(u16::try_from(styles.len()).expect("source styles fit in u16"));
                styles.push(*style);
            }
            let glyph = cell.glyph();
            if index >= graphemes_start
                && glyph & GRAPHEME_TABLE_BIT != 0
                && let std::collections::hash_map::Entry::Vacant(entry) = grapheme_ids.entry(glyph)
            {
                let index = (glyph & !GRAPHEME_TABLE_BIT) as usize;
                let Some((&start, &end)) = source
                    .grapheme_offsets
                    .get(index)
                    .zip(source.grapheme_offsets.get(index + 1))
                else {
                    return;
                };
                let Some(bytes) = source.grapheme_bytes.get(start as usize..end as usize) else {
                    return;
                };
                let index = u32::try_from(grapheme_offsets.len() - 1)
                    .expect("source grapheme indices fit in u32");
                entry.insert(GRAPHEME_TABLE_BIT | index);
                grapheme_bytes.extend_from_slice(bytes);
                grapheme_offsets.push(
                    u32::try_from(grapheme_bytes.len()).expect("source grapheme bytes fit in u32"),
                );
            }
        }
    }
    let styles: Arc<[_]> = styles.into();
    let grapheme_offsets: Arc<[_]> = grapheme_offsets.into();
    let grapheme_bytes: Arc<[_]> = grapheme_bytes.into();
    let mut previous = None;
    let mut replacement = None;
    for (offset, row) in rows.range_mut(start..).enumerate() {
        let index = start + offset;
        if previous
            .as_ref()
            .is_none_or(|dictionary| !Arc::ptr_eq(dictionary, &row.dictionary))
        {
            previous = Some(Arc::clone(&row.dictionary));
            replacement = Some(Arc::new(TerminalDictionary::from_shared(
                if index >= styles_start {
                    Arc::clone(&styles)
                } else {
                    Arc::clone(&row.dictionary.styles)
                },
                if index >= graphemes_start {
                    Arc::clone(&grapheme_offsets)
                } else {
                    Arc::clone(&row.dictionary.grapheme_offsets)
                },
                if index >= graphemes_start {
                    Arc::clone(&grapheme_bytes)
                } else {
                    Arc::clone(&row.dictionary.grapheme_bytes)
                },
            )));
        }
        for cell in &mut row.cells {
            let glyph = if index >= graphemes_start {
                grapheme_ids
                    .get(&cell.glyph())
                    .copied()
                    .unwrap_or(cell.glyph())
            } else {
                cell.glyph()
            };
            let style = if index >= styles_start {
                style_ids[&cell.style_id()]
            } else {
                cell.style_id()
            };
            *cell = PackedCell::from_raw(glyph, style, cell.flags());
        }
        row.dictionary = Arc::clone(replacement.as_ref().expect("replacement dictionary"));
    }
}

fn compact_history_plane_start(
    rows: &VecDeque<HistoryRow>,
    dictionary_bytes: usize,
    shares_plane: impl Fn(&TerminalDictionary) -> bool,
) -> usize {
    if dictionary_bytes <= MIN_HISTORY_DICTIONARY_COMPACTION_BYTES {
        return rows.len();
    }
    let mut start = rows.len();
    let mut cell_bytes = 0;
    for row in rows.iter().rev() {
        if !shares_plane(&row.dictionary) {
            break;
        }
        cell_bytes += std::mem::size_of_val(row.cells.as_ref());
        if cell_bytes >= dictionary_bytes {
            return rows.len();
        }
        start -= 1;
    }
    start
}

#[derive(Debug)]
pub struct RetainedTerminalViewport {
    pub viewport: TerminalViewport,
    pub history: HistoryRing,
    pub history_scrollbar: ScrollbarState,
    /// Bumped on every live-patch ring mutation or drop. A `HistoryChunk` applies
    /// only when this still matches the value snapshotted at request time.
    pub history_mutations: u64,
    /// Bumped only when the ring is dropped, which is what retires the
    /// local-scroll overlay.
    pub history_invalidations: u64,
    pub row_revisions: Box<[u64]>,
    pub row_revision_epoch: u64,
    pub revision_scratch: Vec<u16>,
    /// Bumped each time a selection this pane served lands in the system
    /// clipboard.
    pub copy_generation: u64,
}

fn history_chunk_is_valid(
    columns: u16,
    rows: &[Vec<PackedCell>],
    dictionary: &TerminalDictionary,
) -> bool {
    if rows.is_empty()
        || rows.len() > usize::try_from(MAX_HISTORY_CHUNK_ROWS).unwrap_or(usize::MAX)
        || rows.iter().any(|row| row.len() != usize::from(columns))
        || dictionary.grapheme_offsets.first() != Some(&0)
        || dictionary
            .grapheme_offsets
            .windows(2)
            .any(|offsets| offsets[0] > offsets[1])
        || usize::try_from(dictionary.grapheme_offsets.last().copied().unwrap_or(0))
            .unwrap_or(usize::MAX)
            != dictionary.grapheme_bytes.len()
    {
        return false;
    }
    for offsets in dictionary.grapheme_offsets.windows(2) {
        let Some(bytes) = dictionary
            .grapheme_bytes
            .get(offsets[0] as usize..offsets[1] as usize)
        else {
            return false;
        };
        if std::str::from_utf8(bytes).is_err() {
            return false;
        }
    }
    rows.iter().flatten().all(|cell| {
        if usize::from(cell.style_id()) >= dictionary.styles.len() {
            return false;
        }
        let glyph = cell.glyph();
        if glyph == 0 {
            return true;
        }
        if glyph & GRAPHEME_TABLE_BIT == 0 {
            return char::from_u32(glyph).is_some();
        }
        let index = usize::try_from(glyph & !GRAPHEME_TABLE_BIT).unwrap_or(usize::MAX);
        index.saturating_add(1) < dictionary.grapheme_offsets.len()
    })
}

pub fn apply_history_chunk(
    retained: &mut RetainedTerminalViewport,
    start: u32,
    total: u32,
    offset: u32,
    columns: u16,
    rows: Vec<Vec<PackedCell>>,
    dictionary: TerminalDictionary,
    next_row_revision: &mut u64,
) -> bool {
    let Ok(retained_rows) = u32::try_from(retained.history.len()) else {
        return false;
    };
    let Some(front) = retained.history_scrollbar.offset.checked_sub(retained_rows) else {
        return false;
    };
    let Some(end) = start.checked_add(u32::try_from(rows.len()).unwrap_or(u32::MAX)) else {
        return false;
    };
    if columns != retained.viewport.columns
        || total != retained.history_scrollbar.total
        || offset != retained.history_scrollbar.offset
        || end != front
        || !history_chunk_is_valid(columns, &rows, &dictionary)
    {
        return false;
    }
    retained
        .history
        .prepend(rows, dictionary, next_row_revision);
    retained.row_revision_epoch = allocate_row_revision(next_row_revision);
    true
}

fn allocate_row_revisions(counter: &mut u64, rows: usize) -> Vec<u64> {
    (0..rows).map(|_| allocate_row_revision(counter)).collect()
}

pub fn allocate_row_revision(counter: &mut u64) -> u64 {
    let revision = *counter;
    *counter = counter.wrapping_add(1).max(1);
    revision
}

pub fn new_retained_viewport(
    viewport: TerminalViewport,
    next_row_revision: &mut u64,
) -> RetainedTerminalViewport {
    let row_revisions = allocate_row_revisions(next_row_revision, usize::from(viewport.rows));
    let history_scrollbar = viewport.scrollbar;
    RetainedTerminalViewport {
        viewport,
        history: HistoryRing::default(),
        history_scrollbar,
        history_mutations: 0,
        history_invalidations: 0,
        row_revisions: row_revisions.into_boxed_slice(),
        row_revision_epoch: allocate_row_revision(next_row_revision),
        revision_scratch: Vec::new(),
        copy_generation: 0,
    }
}

fn drop_retained_history(retained: &mut RetainedTerminalViewport) {
    retained.history.clear();
    retained.history_mutations = retained.history_mutations.wrapping_add(1);
    retained.history_invalidations = retained.history_invalidations.wrapping_add(1);
}

fn shift_rows(revisions: &mut [u64], scroll: i16) {
    let rows = revisions.len();
    let shift = isize::from(scroll);
    if shift > 0 {
        let shift = shift.unsigned_abs();
        revisions.copy_within(0..rows - shift, shift);
    } else if shift < 0 {
        let shift = shift.unsigned_abs();
        revisions.copy_within(shift..rows, 0);
    }
}

pub fn apply_retained_patch(
    retained: &mut RetainedTerminalViewport,
    patch: TerminalViewportPatch,
    next_row_revision: &mut u64,
) -> Result<(), PatchError> {
    let scroll = patch.scroll;
    let previous_scrollbar = retained.history_scrollbar;
    let next_scrollbar = patch.scrollbar_after(&retained.viewport);
    let rows = usize::from(patch.rows);
    let shift = usize::from(scroll.unsigned_abs());
    let total_delta = next_scrollbar.total.checked_sub(previous_scrollbar.total);
    let offset_forward = next_scrollbar.offset.checked_sub(previous_scrollbar.offset);
    let offset_reverse = previous_scrollbar.offset.checked_sub(next_scrollbar.offset);
    let full_row_replacement = scroll == 0
        && rows != 0
        && patch.changed_rows.len() == rows
        && patch.generation != patch.base_generation;
    let mut invalidate_history =
        patch.columns != retained.viewport.columns || total_delta.is_none() || full_row_replacement;
    let mut departing_rows = Vec::new();

    if !invalidate_history {
        match scroll.cmp(&0) {
            std::cmp::Ordering::Less => {
                let shift_u32 = u32::try_from(shift).unwrap_or(u32::MAX);
                invalidate_history = total_delta.is_none_or(|delta| delta > shift_u32)
                    || offset_forward.is_none_or(|delta| delta > shift_u32);
                if !invalidate_history {
                    let dictionary = Arc::clone(&retained.viewport.dictionary);
                    for row in shift.saturating_sub(retained.history.limit)..shift {
                        let Some(cells) = u16::try_from(row)
                            .ok()
                            .and_then(|row| retained.viewport.row(row))
                        else {
                            invalidate_history = true;
                            departing_rows.clear();
                            break;
                        };
                        departing_rows.push(HistoryRow {
                            cells: Box::from(cells),
                            dictionary: Arc::clone(&dictionary),
                            revision: allocate_row_revision(next_row_revision),
                        });
                    }
                }
            }
            std::cmp::Ordering::Greater => {
                let shift_u32 = u32::try_from(shift).unwrap_or(u32::MAX);
                invalidate_history = total_delta
                    .is_none_or(|delta| delta != 0 && offset_reverse != Some(shift_u32))
                    || offset_reverse.is_none_or(|delta| delta > shift_u32)
                    || retained.history.len() < shift;
            }
            std::cmp::Ordering::Equal => {
                invalidate_history = previous_scrollbar.offset != next_scrollbar.offset;
            }
        }
    }
    retained.revision_scratch.clear();
    retained
        .revision_scratch
        .extend(patch.changed_rows.row_indices());
    let outgoing_dictionary =
        (!patch.dictionary.is_empty()).then(|| Arc::clone(&retained.viewport.dictionary));
    if let Err(error) = retained.viewport.apply_patch(patch) {
        retained.revision_scratch.clear();
        return Err(error);
    }
    if invalidate_history {
        drop_retained_history(retained);
    } else if scroll < 0 {
        for row in departing_rows {
            retained.history.push_back(row);
        }
        let advanced = next_scrollbar
            .offset
            .saturating_sub(retained.history_scrollbar.offset);
        let evicted = shift.saturating_sub(usize::try_from(advanced).unwrap_or(0));
        for _ in 0..evicted {
            retained.history.rows.pop_front();
        }
        retained.history.enforce_cap();
        retained.history_mutations = retained.history_mutations.wrapping_add(1);
    } else if scroll > 0 {
        for _ in 0..shift {
            retained.history.rows.pop_back();
        }
        retained.history_mutations = retained.history_mutations.wrapping_add(1);
    }
    if let Some(dictionary) = outgoing_dictionary {
        compact_history_dictionary(
            &mut retained.history.rows,
            &dictionary,
            &retained.viewport.dictionary,
        );
    }
    retained.history_scrollbar = next_scrollbar;
    if scroll != 0 || !retained.revision_scratch.is_empty() {
        shift_rows(&mut retained.row_revisions, scroll);
        for row in retained.revision_scratch.iter().copied() {
            retained.row_revisions[usize::from(row)] = allocate_row_revision(next_row_revision);
        }
        retained.row_revision_epoch = allocate_row_revision(next_row_revision);
    }
    retained.revision_scratch.clear();
    Ok(())
}

pub fn replace_retained_viewport(
    retained: &mut RetainedTerminalViewport,
    viewport: TerminalViewport,
    next_row_revision: &mut u64,
) {
    let rows = usize::from(viewport.rows);
    if retained.row_revisions.len() == rows {
        for revision in &mut retained.row_revisions {
            *revision = allocate_row_revision(next_row_revision);
        }
    } else {
        retained.row_revisions = allocate_row_revisions(next_row_revision, rows).into_boxed_slice();
    }
    retained.row_revision_epoch = allocate_row_revision(next_row_revision);
    retained.revision_scratch.clear();
    drop_retained_history(retained);
    retained.history_scrollbar = viewport.scrollbar;
    retained.viewport = viewport;
}

pub fn history_request_range(
    retained: &RetainedTerminalViewport,
    budget: usize,
    prefetch_target: Option<u32>,
) -> Option<(u32, u32)> {
    let budget = budget.min(retained.history.limit);
    let retained_rows = retained.history.len();
    if retained_rows >= budget {
        return None;
    }
    let front = retained
        .history_scrollbar
        .offset
        .checked_sub(u32::try_from(retained_rows).ok()?)?;
    let desired = prefetch_target.map_or(front, |target| {
        front.saturating_sub(target.saturating_sub(retained.viewport.scrollbar.len))
    });
    let count = desired
        .min(MAX_HISTORY_CHUNK_ROWS)
        .min(u32::try_from(budget - retained_rows).unwrap_or(u32::MAX));
    (count != 0).then(|| (front - count, count))
}

#[derive(Clone, Copy, Debug)]
struct PendingHistoryRequest {
    mutations: u64,
    prefetch_target: Option<u32>,
    sent: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryFollowUp {
    Prefetch(u32),
    Defer(u64),
    Backfill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeferredBackfill {
    Request,
    Wait,
    Done,
}

#[derive(Debug, Default)]
pub struct HistoryPacer {
    pending: HashMap<PaneId, PendingHistoryRequest>,
    deferred: HashMap<PaneId, u64>,
}

impl HistoryPacer {
    pub fn request(
        &mut self,
        pane: PaneId,
        prefetch_target: Option<u32>,
        trickle_budget: usize,
        retained: Option<&RetainedTerminalViewport>,
        now: Instant,
    ) -> Option<(u32, u32)> {
        if prefetch_target.is_none() && self.deferred.contains_key(&pane) {
            return None;
        }
        let mut prefetch_target = prefetch_target;
        if let Some(pending) = self.pending.get_mut(&pane) {
            let target = match (prefetch_target, pending.prefetch_target) {
                (Some(target), Some(previous)) => Some(target.min(previous)),
                (target, previous) => target.or(previous),
            };
            if now.saturating_duration_since(pending.sent) < HISTORY_REQUEST_RETRY {
                pending.prefetch_target = target;
                return None;
            }
            prefetch_target = target;
            self.pending.remove(&pane);
        }
        let budget = prefetch_target.map_or(trickle_budget, |_| MAX_HISTORY_ROWS);
        let retained = retained?;
        let range = history_request_range(retained, budget, prefetch_target)?;
        self.pending.insert(
            pane,
            PendingHistoryRequest {
                mutations: retained.history_mutations,
                prefetch_target,
                sent: now,
            },
        );
        Some(range)
    }

    pub fn chunk_arrived(
        &mut self,
        pane: PaneId,
        mutations: Option<u64>,
    ) -> (bool, HistoryFollowUp) {
        let pending = self.pending.remove(&pane);
        let apply = mutations.is_some() && pending.map(|request| request.mutations) == mutations;
        let follow_up = match (pending, mutations) {
            (
                Some(PendingHistoryRequest {
                    prefetch_target: Some(target),
                    ..
                }),
                _,
            ) => HistoryFollowUp::Prefetch(target),
            (Some(_), Some(mutations)) if !apply => HistoryFollowUp::Defer(mutations),
            _ => HistoryFollowUp::Backfill,
        };
        (apply, follow_up)
    }

    pub fn defer(&mut self, pane: PaneId, mutations: u64) -> bool {
        self.deferred.insert(pane, mutations).is_none()
    }

    pub fn resume_deferred(&mut self, pane: PaneId, mutations: Option<u64>) -> DeferredBackfill {
        let Some(recorded) = self.deferred.get_mut(&pane) else {
            return DeferredBackfill::Done;
        };
        let Some(current) = mutations else {
            return DeferredBackfill::Done;
        };
        if *recorded == current {
            self.deferred.remove(&pane);
            DeferredBackfill::Request
        } else {
            *recorded = current;
            DeferredBackfill::Wait
        }
    }

    pub fn is_deferred(&self, pane: PaneId) -> bool {
        self.deferred.contains_key(&pane)
    }

    pub fn forget(&mut self, pane: PaneId) {
        self.pending.remove(&pane);
        self.deferred.remove(&pane);
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.deferred.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zz_terminal::{CellWidth, OverlayKind, OverlaySpan, SessionStatus};

    use super::*;

    fn history_fixture_viewport(
        row_ids: &[u32],
        generation: u64,
        total: u32,
        offset: u32,
    ) -> TerminalViewport {
        let mut viewport = TerminalViewport::blank(
            1,
            u16::try_from(row_ids.len()).expect("small fixture"),
            SessionStatus::Running,
        );
        viewport.generation = generation;
        viewport.view_generation = generation;
        viewport.scrollbar = ScrollbarState {
            total,
            offset,
            len: u32::try_from(row_ids.len()).expect("small fixture"),
        };
        for (cell, id) in Arc::make_mut(&mut viewport.cells).iter_mut().zip(row_ids) {
            *cell = PackedCell::new(0xe000 + *id, 0, CellWidth::Narrow);
        }
        viewport
    }

    fn retained_history_ids(retained: &RetainedTerminalViewport) -> Vec<u32> {
        retained
            .history
            .rows
            .iter()
            .map(|row| row.cells[0].glyph() - 0xe000)
            .collect()
    }

    #[test]
    fn retained_history_does_not_keep_every_appended_dictionary_version() {
        retained_history_dictionary_growth(false);
    }

    #[test]
    fn retained_history_compacts_alternating_dictionary_and_scroll_updates() {
        retained_history_dictionary_growth(true);
    }

    #[test]
    fn retained_history_compacts_independently_growing_dictionary_planes() {
        let mut next_revision = 1;
        let mut retained = new_retained_viewport(
            history_fixture_viewport(&[0, 1, 2], 1, 3, 0),
            &mut next_revision,
        );
        let mut expected = VecDeque::from([
            ("\u{e000}".to_owned(), retained.viewport.foreground),
            ("\u{e001}".to_owned(), retained.viewport.foreground),
            ("\u{e002}".to_owned(), retained.viewport.foreground),
        ]);
        for frame in 1_u16..=1_000 {
            let mut next = retained.viewport.clone();
            next.generation += 1;
            next.view_generation += 1;
            next.scrollbar.total += 1;
            next.scrollbar.offset += 1;
            let dictionary = Arc::make_mut(&mut next.dictionary);
            let (glyph, text) = if frame % 2 == 1 {
                let mut styles = dictionary.styles.to_vec();
                styles.push(zz_terminal::PackedStyle::new(
                    zz_terminal::Color::rgb((frame >> 8) as u8, frame as u8, 0),
                    next.background,
                    None,
                    0,
                    zz_terminal::UnderlineStyle::None,
                ));
                dictionary.styles = styles.into();
                let glyph = 0xe002 + u32::from(frame);
                (glyph, char::from_u32(glyph).unwrap().to_string())
            } else {
                let text = format!(
                    "{}\u{301}",
                    char::from_u32(0x400 + u32::from(frame)).unwrap()
                );
                let glyph = GRAPHEME_TABLE_BIT | (dictionary.grapheme_offsets.len() - 1) as u32;
                let mut offsets = dictionary.grapheme_offsets.to_vec();
                let mut bytes = dictionary.grapheme_bytes.to_vec();
                bytes.extend_from_slice(text.as_bytes());
                offsets.push(bytes.len() as u32);
                dictionary.grapheme_offsets = offsets.into();
                dictionary.grapheme_bytes = bytes.into();
                (glyph, text)
            };
            let style_id = (dictionary.styles.len() - 1) as u16;
            expected.push_back((text, dictionary.styles[usize::from(style_id)].foreground()));
            let cells = Arc::make_mut(&mut next.cells);
            cells.copy_within(1.., 0);
            cells[2] = PackedCell::new(glyph, style_id, CellWidth::Narrow);
            let patch = TerminalViewport::diff(&retained.viewport, &next).unwrap();
            assert_eq!(patch.scroll, -1);
            apply_retained_patch(&mut retained, patch, &mut next_revision).unwrap();
        }

        let mut style_planes = BTreeMap::new();
        let mut grapheme_planes = BTreeMap::new();
        for (row, (text, foreground)) in retained.history.rows.iter().zip(expected) {
            style_planes.insert(
                row.dictionary.styles.as_ptr() as usize,
                std::mem::size_of_val(row.dictionary.styles.as_ref()),
            );
            grapheme_planes.insert(
                row.dictionary.grapheme_offsets.as_ptr() as usize,
                std::mem::size_of_val(row.dictionary.grapheme_offsets.as_ref())
                    + row.dictionary.grapheme_bytes.len(),
            );
            let mut decoded = retained.viewport.clone();
            decoded.dictionary = Arc::clone(&row.dictionary);
            assert_eq!(decoded.cell_text(row.cells[0]), text);
            assert_eq!(
                decoded.style(row.cells[0]).unwrap().foreground(),
                foreground
            );
        }
        let dictionary_bytes =
            style_planes.values().sum::<usize>() + grapheme_planes.values().sum::<usize>();
        eprintln!(
            "mixed_dictionary_growth history_rows={} style_planes={} grapheme_planes={} retained_dictionary_bytes={dictionary_bytes}",
            retained.history.len(),
            style_planes.len(),
            grapheme_planes.len(),
        );
        assert_eq!(retained.history.len(), 1_000);
        assert!(dictionary_bytes <= retained.history.len() * 128);
    }

    fn retained_history_dictionary_growth(separate_dictionary_updates: bool) {
        let mut next_revision = 1;
        let mut retained = new_retained_viewport(
            history_fixture_viewport(&[0, 1, 2], 1, 3, 0),
            &mut next_revision,
        );
        for frame in 1_u16..=1_000 {
            let mut next = retained.viewport.clone();
            next.generation += 1;
            next.view_generation += 1;
            let mut styles = next.dictionary.styles.to_vec();
            styles.push(zz_terminal::PackedStyle::new(
                zz_terminal::Color::rgb((frame >> 8) as u8, frame as u8, 0),
                next.background,
                None,
                0,
                zz_terminal::UnderlineStyle::None,
            ));
            Arc::make_mut(&mut next.dictionary).styles = styles.into();
            if separate_dictionary_updates {
                let patch = TerminalViewport::diff(&retained.viewport, &next).unwrap();
                assert!(patch.changed_rows.is_empty());
                assert_eq!(patch.scroll, 0);
                apply_retained_patch(&mut retained, patch, &mut next_revision).unwrap();
                next.generation += 1;
                next.view_generation += 1;
            }
            next.scrollbar.total += 1;
            next.scrollbar.offset += 1;
            let cells = Arc::make_mut(&mut next.cells);
            cells.copy_within(1.., 0);
            cells[2] = PackedCell::new(0xe002 + u32::from(frame), frame, CellWidth::Narrow);
            let patch = TerminalViewport::diff(&retained.viewport, &next).unwrap();
            assert_eq!(patch.scroll, -1);
            assert_eq!(patch.dictionary.is_empty(), separate_dictionary_updates);
            apply_retained_patch(&mut retained, patch, &mut next_revision).unwrap();
        }

        assert_eq!(retained.history.len(), 1_000);
        let mut style_planes = BTreeMap::new();
        for row in &retained.history.rows {
            style_planes.insert(
                row.dictionary.styles.as_ptr() as usize,
                std::mem::size_of_val(row.dictionary.styles.as_ref()),
            );
            let cell = row.cells[0];
            if cell.glyph() >= 0xe003 {
                let frame = cell.glyph() - 0xe002;
                assert_eq!(
                    row.dictionary.styles[usize::from(cell.style_id())].foreground(),
                    zz_terminal::Color::rgb((frame >> 8) as u8, frame as u8, 0)
                );
            }
        }
        let style_bytes = style_planes.values().sum::<usize>();
        eprintln!(
            "separate_dictionary_updates={separate_dictionary_updates} history_rows={} unique_style_planes={} retained_style_bytes={style_bytes}",
            retained.history.len(),
            style_planes.len(),
        );
        let bytes_per_row = if separate_dictionary_updates { 80 } else { 64 };
        assert!(style_bytes <= retained.history.len() * bytes_per_row);
        assert_eq!(retained.viewport.dictionary.styles.len(), 1_001);
        assert_eq!(retained.viewport.cells[2].style_id(), 1_000);
    }

    #[test]
    fn stable_large_history_dictionaries_keep_one_shared_style_plane() {
        let mut viewport = TerminalViewport::blank(80, 3, SessionStatus::Running);
        Arc::make_mut(&mut viewport.dictionary).styles = (0..80)
            .map(|index| {
                zz_terminal::PackedStyle::new(
                    zz_terminal::Color::rgb(index, 0, 0),
                    viewport.background,
                    None,
                    0,
                    zz_terminal::UnderlineStyle::None,
                )
            })
            .collect();
        for (row, cells) in Arc::make_mut(&mut viewport.cells)
            .chunks_mut(80)
            .enumerate()
        {
            for (column, cell) in cells.iter_mut().enumerate() {
                *cell = PackedCell::new(0xe000 + row as u32, column as u16, CellWidth::Narrow);
            }
        }
        let styles = Arc::clone(&viewport.dictionary.styles);
        let mut next_revision = 1;
        let mut retained = new_retained_viewport(viewport, &mut next_revision);
        for frame in 1..=1_000 {
            let mut next = retained.viewport.clone();
            next.generation += 1;
            next.view_generation += 1;
            next.scrollbar.total += 1;
            next.scrollbar.offset += 1;
            let cells = Arc::make_mut(&mut next.cells);
            cells.copy_within(80.., 0);
            for (column, cell) in cells[160..].iter_mut().enumerate() {
                *cell = PackedCell::new(0xe002 + frame, column as u16, CellWidth::Narrow);
            }
            let patch = TerminalViewport::diff(&retained.viewport, &next).unwrap();
            assert_eq!(patch.scroll, -1);
            assert!(patch.dictionary.is_empty());
            apply_retained_patch(&mut retained, patch, &mut next_revision).unwrap();
        }
        assert_eq!(retained.history.len(), 1_000);
        for frame in 1_001..=1_256 {
            let mut next = retained.viewport.clone();
            next.generation += 1;
            next.view_generation += 1;
            next.scrollbar.total += 1;
            next.scrollbar.offset += 1;
            let dictionary = Arc::make_mut(&mut next.dictionary);
            let glyph = GRAPHEME_TABLE_BIT | (dictionary.grapheme_offsets.len() - 1) as u32;
            let text = format!("{}\u{301}", char::from_u32(0x400 + frame).unwrap());
            let mut offsets = dictionary.grapheme_offsets.to_vec();
            let mut bytes = dictionary.grapheme_bytes.to_vec();
            bytes.extend_from_slice(text.as_bytes());
            offsets.push(bytes.len() as u32);
            dictionary.grapheme_offsets = offsets.into();
            dictionary.grapheme_bytes = bytes.into();
            let cells = Arc::make_mut(&mut next.cells);
            cells.copy_within(80.., 0);
            for (column, cell) in cells[160..].iter_mut().enumerate() {
                *cell = PackedCell::new(glyph, column as u16, CellWidth::Narrow);
            }
            let patch = TerminalViewport::diff(&retained.viewport, &next).unwrap();
            assert_eq!(patch.scroll, -1);
            assert!(!patch.dictionary.is_empty());
            apply_retained_patch(&mut retained, patch, &mut next_revision).unwrap();
        }
        assert_eq!(retained.history.len(), 1_256);
        assert!(
            retained
                .history
                .rows
                .iter()
                .all(|row| Arc::ptr_eq(&row.dictionary.styles, &styles))
        );
        assert_eq!(std::mem::size_of_val(styles.as_ref()), 1_280);
    }

    #[test]
    fn history_compaction_preserves_shared_rows_styles_graphemes_and_cell_flags() {
        let mut source_viewport = TerminalViewport::blank(2, 2, SessionStatus::Running);
        let unused = "unused".repeat(256);
        let combining = "e\u{301}";
        let wide = "👩‍💻";
        source_viewport.dictionary = Arc::new(TerminalDictionary::from_shared(
            (0..128)
                .map(|index| {
                    zz_terminal::PackedStyle::new(
                        zz_terminal::Color::rgb(index, 0, 0),
                        source_viewport.background,
                        None,
                        0,
                        zz_terminal::UnderlineStyle::None,
                    )
                })
                .collect(),
            Arc::from([
                0,
                unused.len() as u32,
                (unused.len() + combining.len()) as u32,
                (unused.len() + combining.len() + wide.len()) as u32,
            ]),
            [unused.as_str(), combining, wide]
                .concat()
                .into_bytes()
                .into(),
        ));
        let source = Arc::clone(&source_viewport.dictionary);
        let mut rows = VecDeque::from([
            HistoryRow {
                cells: Box::new([
                    PackedCell::from_raw(GRAPHEME_TABLE_BIT | 1, 17, 0x400),
                    PackedCell::new(u32::from('A'), 42, CellWidth::Narrow),
                ]),
                dictionary: Arc::clone(&source),
                revision: 10,
            },
            HistoryRow {
                cells: Box::new([
                    PackedCell::from_raw(
                        GRAPHEME_TABLE_BIT | 2,
                        17,
                        0x800 | CellWidth::Wide as u16,
                    ),
                    PackedCell::new(0, 42, CellWidth::SpacerTail),
                ]),
                dictionary: Arc::clone(&source),
                revision: 11,
            },
        ]);
        let before = rows.clone();
        let mut glyph_only_rows = before.clone();
        let current = TerminalDictionary::from_shared(
            Arc::clone(&source.styles),
            Arc::from([0]),
            Arc::from([]),
        );
        compact_history_dictionary(&mut glyph_only_rows, &source, &current);
        assert!(Arc::ptr_eq(
            &glyph_only_rows[0].dictionary.styles,
            &source.styles
        ));
        assert_eq!(glyph_only_rows[0].cells[0].style_id(), 17);
        assert_eq!(
            glyph_only_rows[0].dictionary.grapheme_bytes.len(),
            combining.len() + wide.len()
        );

        compact_history_dictionary(&mut rows, &source, &TerminalDictionary::default());
        assert!(!Arc::ptr_eq(&rows[0].dictionary, &source));
        assert!(Arc::ptr_eq(&rows[0].dictionary, &rows[1].dictionary));
        assert_eq!(rows[0].dictionary.styles.len(), 2);
        assert_eq!(rows[0].dictionary.grapheme_offsets.len(), 3);
        assert_eq!(
            rows[0].dictionary.grapheme_bytes.len(),
            combining.len() + wide.len()
        );
        for (row, original) in rows.iter().zip(&before) {
            assert_eq!(row.revision, original.revision);
            let mut decoded = source_viewport.clone();
            decoded.dictionary = Arc::clone(&row.dictionary);
            for (cell, original) in row.cells.iter().zip(original.cells.iter()) {
                assert_eq!(cell.flags(), original.flags());
                assert_eq!(decoded.style(*cell), source_viewport.style(*original));
                assert_eq!(
                    decoded.cell_text(*cell),
                    source_viewport.cell_text(*original)
                );
            }
        }

        let mut history = HistoryRing {
            rows,
            ..HistoryRing::default()
        };
        let mut next_revision = 12;
        history.prepend(
            vec![before[0].cells.to_vec()],
            source.as_ref().clone(),
            &mut next_revision,
        );
        assert_eq!(history.len(), 3);
        assert!(Arc::ptr_eq(
            &history.rows[0].dictionary.styles,
            &source.styles
        ));
        assert_eq!(history.rows[0].cells, before[0].cells);
        assert_eq!(history.rows[1].dictionary.styles.len(), 2);
        assert_eq!(source.styles.len(), 128);
    }

    #[test]
    fn small_history_dictionaries_keep_the_existing_allocation() {
        let viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        let mut rows = VecDeque::from([HistoryRow {
            cells: viewport.cells.to_vec().into_boxed_slice(),
            dictionary: Arc::clone(&viewport.dictionary),
            revision: 7,
        }]);
        compact_history_dictionary(
            &mut rows,
            &viewport.dictionary,
            &TerminalDictionary::default(),
        );
        assert!(Arc::ptr_eq(&rows[0].dictionary, &viewport.dictionary));
        assert_eq!(&*rows[0].cells, &*viewport.cells);
        assert_eq!(rows[0].revision, 7);
    }

    fn chunk_rows(ids: &[u32]) -> Vec<Vec<PackedCell>> {
        ids.iter()
            .map(|id| vec![PackedCell::new(0xe000 + *id, 0, CellWidth::Narrow)])
            .collect()
    }

    #[test]
    fn history_ring_matches_seeded_scrolls_eviction_and_invalidation() {
        for seed in [1_u32, 7, 42] {
            let mut next_revision = 1;
            let mut next_id = 11_u32;
            let mut history = vec![0, 1, 2];
            let mut live = (3..11).collect::<Vec<_>>();
            let initial = history_fixture_viewport(&live, 1, 11, 3);
            let dictionary = initial.dictionary.as_ref().clone();
            let mut retained = new_retained_viewport(initial, &mut next_revision);
            assert!(apply_history_chunk(
                &mut retained,
                0,
                11,
                3,
                1,
                chunk_rows(&history),
                dictionary,
                &mut next_revision,
            ));

            let mut random = seed;
            for generation in 2..82 {
                random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let shift = usize::try_from(random % 3 + 1).expect("small shift");
                history.extend_from_slice(&live[..shift]);
                if history.len() > 17 {
                    history.drain(..history.len() - 17);
                }
                live.drain(..shift);
                for _ in 0..shift {
                    live.push(next_id);
                    next_id += 1;
                }
                let next = history_fixture_viewport(
                    &live,
                    generation,
                    u32::try_from(history.len() + live.len()).expect("small fixture"),
                    u32::try_from(history.len()).expect("small fixture"),
                );
                let patch = TerminalViewport::diff(&retained.viewport, &next)
                    .expect("compatible scrolling viewport");
                assert_eq!(patch.scroll, -i16::try_from(shift).expect("small shift"));
                apply_retained_patch(&mut retained, patch, &mut next_revision)
                    .expect("apply output scroll");
                assert_eq!(retained_history_ids(&retained), history, "seed {seed}");
            }

            let reverse = 2_usize;
            let reverse_offset = history.len() - reverse;
            let reverse_rows = history[reverse_offset..]
                .iter()
                .chain(live[..live.len() - reverse].iter())
                .copied()
                .collect::<Vec<_>>();
            let reverse_viewport = history_fixture_viewport(
                &reverse_rows,
                82,
                u32::try_from(history.len() + live.len()).expect("small fixture"),
                u32::try_from(reverse_offset).expect("small fixture"),
            );
            let patch = TerminalViewport::diff(&retained.viewport, &reverse_viewport)
                .expect("compatible reverse scroll");
            assert_eq!(patch.scroll, i16::try_from(reverse).expect("small reverse"));
            apply_retained_patch(&mut retained, patch, &mut next_revision)
                .expect("apply reverse scroll");
            assert_eq!(
                retained_history_ids(&retained),
                history[..reverse_offset],
                "seed {seed} reverse"
            );

            let refill_dictionary = reverse_viewport.dictionary.as_ref().clone();
            replace_retained_viewport(&mut retained, reverse_viewport, &mut next_revision);
            assert!(retained.history.rows.is_empty());
            assert!(apply_history_chunk(
                &mut retained,
                0,
                u32::try_from(history.len() + live.len()).expect("small fixture"),
                u32::try_from(reverse_offset).expect("small fixture"),
                1,
                chunk_rows(&history[..reverse_offset]),
                refill_dictionary,
                &mut next_revision,
            ));

            let mut cleared = retained.viewport.clone();
            cleared.generation += 1;
            cleared.view_generation += 1;
            cleared.scrollbar = ScrollbarState {
                total: u32::try_from(live.len()).expect("small fixture"),
                offset: 0,
                len: u32::try_from(live.len()).expect("small fixture"),
            };
            let patch =
                TerminalViewport::diff(&retained.viewport, &cleared).expect("metadata clear patch");
            apply_retained_patch(&mut retained, patch, &mut next_revision)
                .expect("apply history clear");
            assert!(retained.history.rows.is_empty());

            retained.history.rows.push_back(HistoryRow {
                cells: chunk_rows(&[999]).remove(0).into_boxed_slice(),
                dictionary: Arc::clone(&retained.viewport.dictionary),
                revision: allocate_row_revision(&mut next_revision),
            });
            let wider = TerminalViewport::blank(2, 8, SessionStatus::Running);
            replace_retained_viewport(&mut retained, wider, &mut next_revision);
            assert!(retained.history.rows.is_empty());
        }
    }

    #[test]
    fn retained_command_output_scroll_reuses_unchanged_row_revisions() {
        let mut previous = TerminalViewport::blank(1, 3, SessionStatus::Running);
        previous.generation = 1;
        for (cell, glyph) in Arc::make_mut(&mut previous.cells)
            .iter_mut()
            .zip(['a', 'b', 'c'])
        {
            *cell = PackedCell::new(u32::from(glyph), 0, CellWidth::Narrow);
        }
        let mut current = previous.clone();
        current.generation = 2;
        current.view_generation = 2;
        let cells = Arc::make_mut(&mut current.cells);
        cells[0] = cells[1];
        cells[1] = cells[2];
        cells[2] = PackedCell::new(u32::from('d'), 0, CellWidth::Narrow);
        let patch = TerminalViewport::diff(&previous, &current).expect("compatible output frame");
        assert_eq!(patch.scroll, -1);

        let history_scrollbar = previous.scrollbar;
        let mut retained = RetainedTerminalViewport {
            viewport: previous,
            history: HistoryRing::default(),
            history_scrollbar,
            history_mutations: 0,
            history_invalidations: 0,
            row_revisions: Box::new([10, 11, 12]),
            row_revision_epoch: 9,
            revision_scratch: Vec::new(),
            copy_generation: 0,
        };
        let revision_address = retained.row_revisions.as_ptr();
        let mut next_revision = 20;
        apply_retained_patch(&mut retained, patch, &mut next_revision).expect("apply output patch");

        assert_eq!(retained.viewport, current);
        assert_eq!(&*retained.row_revisions, &[11, 12, 21]);
        assert_eq!(retained.row_revisions.as_ptr(), revision_address);
        assert_eq!(retained.row_revision_epoch, 22);
        assert_eq!(next_revision, 23);
        assert!(retained.revision_scratch.is_empty());
        assert!(retained.revision_scratch.capacity() > 0);

        let scratch_address = retained.revision_scratch.as_ptr();
        let mut next = current.clone();
        next.generation = 3;
        next.view_generation = 3;
        let cells = Arc::make_mut(&mut next.cells);
        cells[0] = cells[1];
        cells[1] = cells[2];
        cells[2] = PackedCell::new(u32::from('e'), 0, CellWidth::Narrow);
        let patch = TerminalViewport::diff(&current, &next).expect("second output patch");
        apply_retained_patch(&mut retained, patch, &mut next_revision)
            .expect("apply second output patch");

        assert_eq!(retained.viewport, next);
        assert_eq!(&*retained.row_revisions, &[12, 21, 24]);
        assert_eq!(retained.row_revisions.as_ptr(), revision_address);
        assert_eq!(retained.revision_scratch.as_ptr(), scratch_address);
        assert_eq!(retained.row_revision_epoch, 25);
        assert_eq!(next_revision, 26);

        let mut rejected = next.clone();
        rejected.generation = 4;
        rejected.view_generation = 4;
        Arc::make_mut(&mut rejected.cells)[2] =
            PackedCell::new(u32::from('f'), 0, CellWidth::Narrow);
        let mut patch = TerminalViewport::diff(&next, &rejected).expect("rejected patch");
        patch.base_generation = u64::MAX;
        let revisions_before = retained.row_revisions.to_vec();
        let epoch_before = retained.row_revision_epoch;
        assert!(apply_retained_patch(&mut retained, patch, &mut next_revision).is_err());
        assert_eq!(&*retained.row_revisions, revisions_before);
        assert_eq!(retained.row_revision_epoch, epoch_before);
        assert!(retained.revision_scratch.is_empty());

        let mut metadata = next.clone();
        metadata.view_generation = 4;
        metadata.overlays = Arc::from([OverlaySpan::new(0, 0, 1, OverlayKind::Selection)]);
        let patch = TerminalViewport::diff(&next, &metadata).expect("metadata patch");
        assert!(patch.changed_rows.is_empty());
        apply_retained_patch(&mut retained, patch, &mut next_revision)
            .expect("apply metadata patch");
        assert_eq!(retained.viewport, metadata);
        assert_eq!(retained.row_revisions.as_ptr(), revision_address);
        assert_eq!(retained.row_revision_epoch, epoch_before);
        assert_eq!(next_revision, 26);
    }

    fn output_scroll(
        retained: &mut RetainedTerminalViewport,
        live: &[u32],
        shift: u32,
        next_revision: &mut u64,
    ) {
        let scrollbar = retained.viewport.scrollbar;
        let next = history_fixture_viewport(
            live,
            retained.viewport.generation + 1,
            scrollbar.total + shift,
            scrollbar.offset + shift,
        );
        let patch = TerminalViewport::diff(&retained.viewport, &next).unwrap();
        apply_retained_patch(retained, patch, next_revision).unwrap();
    }

    #[test]
    fn a_bounded_ring_keeps_only_the_rows_nearest_the_viewport() {
        let mut next_revision = 1;
        let mut bounded = new_retained_viewport(
            history_fixture_viewport(&[0, 1, 2, 3, 4], 1, 5, 0),
            &mut next_revision,
        );
        bounded.history = HistoryRing::with_limit(2);
        output_scroll(&mut bounded, &[3, 4, 5, 6, 7], 3, &mut next_revision);
        assert_eq!(retained_history_ids(&bounded), [1, 2]);
        assert_eq!(
            history_request_range(&bounded, MAX_HISTORY_ROWS, None),
            None
        );

        let mut disabled = new_retained_viewport(
            history_fixture_viewport(&[0, 1, 2, 3, 4], 1, 5, 0),
            &mut next_revision,
        );
        disabled.history = HistoryRing::with_limit(0);
        output_scroll(&mut disabled, &[3, 4, 5, 6, 7], 3, &mut next_revision);
        assert!(disabled.history.is_empty());
        assert_eq!(disabled.viewport.scrollbar.offset, 3);
        assert_eq!(history_request_range(&disabled, 2_000, Some(0)), None);
    }

    #[test]
    fn history_pacer_coalesces_retries_and_defers_backfill_while_output_streams() {
        let pane = PaneId(7);
        let mut next_revision = 1;
        let retained = new_retained_viewport(
            history_fixture_viewport(&[1_200, 1_201, 1_202], 1, 1_203, 1_200),
            &mut next_revision,
        );
        let mutations = retained.history_mutations;
        let started = Instant::now();
        let mut pacer = HistoryPacer::default();

        assert_eq!(
            pacer.request(pane, None, 600, Some(&retained), started),
            Some((688, 512))
        );
        assert_eq!(
            pacer.request(pane, None, 600, Some(&retained), started),
            None
        );
        assert_eq!(
            pacer.request(pane, Some(1_199), 600, Some(&retained), started),
            None
        );
        assert_eq!(
            pacer.chunk_arrived(pane, Some(mutations)),
            (true, HistoryFollowUp::Prefetch(1_199))
        );

        assert_eq!(
            pacer.request(pane, None, 600, Some(&retained), started),
            Some((688, 512))
        );
        assert_eq!(
            pacer.request(
                pane,
                None,
                600,
                Some(&retained),
                started + HISTORY_REQUEST_RETRY
            ),
            Some((688, 512))
        );
        assert_eq!(
            pacer.chunk_arrived(pane, Some(mutations + 1)),
            (false, HistoryFollowUp::Defer(mutations + 1))
        );
        assert!(pacer.defer(pane, mutations + 1));
        assert!(!pacer.defer(pane, mutations + 1));
        assert_eq!(
            pacer.request(pane, None, 600, Some(&retained), started),
            None
        );
        assert_eq!(
            pacer.resume_deferred(pane, Some(mutations + 2)),
            DeferredBackfill::Wait
        );
        assert_eq!(
            pacer.request(pane, Some(1_100), 600, Some(&retained), started),
            Some((1_097, 103))
        );
        assert_eq!(
            pacer.resume_deferred(pane, Some(mutations + 2)),
            DeferredBackfill::Request
        );
        assert!(!pacer.is_deferred(pane));
        assert_eq!(
            pacer.chunk_arrived(PaneId(8), Some(mutations)),
            (false, HistoryFollowUp::Backfill)
        );
        pacer.clear();
        assert_eq!(pacer.request(pane, None, 0, Some(&retained), started), None);
    }

    #[test]
    fn scrolling_back_while_output_appends_below_keeps_the_ring() {
        let mut next_revision = 1;
        let mut retained = new_retained_viewport(
            history_fixture_viewport(&[10, 11, 12, 13], 1, 14, 10),
            &mut next_revision,
        );
        let dictionary = retained.viewport.dictionary.as_ref().clone();
        assert!(apply_history_chunk(
            &mut retained,
            0,
            14,
            10,
            1,
            chunk_rows(&(0..10).collect::<Vec<_>>()),
            dictionary,
            &mut next_revision,
        ));
        let back = history_fixture_viewport(&[8, 9, 10, 11], 2, 15, 8);
        let patch = TerminalViewport::diff(&retained.viewport, &back).unwrap();
        assert_eq!(patch.scroll, 2);
        apply_retained_patch(&mut retained, patch, &mut next_revision).unwrap();
        assert_eq!(retained_history_ids(&retained), (0..8).collect::<Vec<_>>());
        assert_eq!(retained.history_invalidations, 0);

        let trimmed = history_fixture_viewport(&[6, 7, 8, 9], 3, 16, 5);
        let patch = TerminalViewport::diff(&retained.viewport, &trimmed).unwrap();
        assert_eq!(patch.scroll, 2);
        apply_retained_patch(&mut retained, patch, &mut next_revision).unwrap();
        assert!(retained.history.is_empty());
        assert_eq!(retained.history_invalidations, 1);
    }
}
