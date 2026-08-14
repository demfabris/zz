use std::{collections::VecDeque, sync::Arc};

use zz_terminal::{PackedCell, ScrollbarState, TerminalDictionary, TerminalViewport};

/// The ring never grows past this, matching the desktop's cap.
pub const MAX_HISTORY_ROWS: usize = 10_000;
/// The daemon clamps one `HistoryChunk` to this many rows, so asking for more
/// only costs a round trip.
pub const MAX_HISTORY_CHUNK_ROWS: u32 = 512;

/// One scrollback row exactly as the daemon sent it. The dictionary is shared
/// with every other row of the same chunk, so a row costs one cell plane.
#[derive(Clone)]
pub struct HistoryRow {
    pub cells: Arc<[PackedCell]>,
    pub dictionary: Arc<TerminalDictionary>,
}

/// One `EventPayload::HistoryChunk`, narrowed to what the ring stores.
pub struct HistoryChunk {
    pub start: u32,
    pub total: u32,
    pub offset: u32,
    pub columns: u16,
    pub rows: Vec<Vec<PackedCell>>,
    pub dictionary: TerminalDictionary,
}

/// Where the retained rows sit in the daemon's scrollback.
///
/// Scrollback indices are absolute and count from the oldest row, so a capped
/// scrollback shifts every one of them for each new line — without moving
/// `total` or `offset`, which both sit still once the cap is reached. The
/// viewport generation is therefore the only honest witness that the retained
/// indices still mean what they meant: any content change retires the ring.
#[derive(Clone, Copy)]
struct Anchor {
    generation: u64,
    columns: u16,
    scrollbar: ScrollbarState,
}

/// Scrollback rows the client keeps so a wheel scroll repaints out of local
/// memory instead of waiting for the daemon. Filled only by
/// `ProtocolMessage::HistoryRequest` backfill: the live-frame path never
/// touches it, which is what keeps the frame budget at zero.
#[derive(Default)]
pub struct HistoryRing {
    rows: VecDeque<HistoryRow>,
    front: u32,
    anchor: Option<Anchor>,
}

impl HistoryRing {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The absolute index of the oldest retained row, or of the live viewport
    /// top when nothing is retained.
    pub fn front(&self) -> u32 {
        if self.rows.is_empty() {
            self.anchor.map_or(0, |anchor| anchor.scrollbar.offset)
        } else {
            self.front
        }
    }

    pub fn row(&self, index: u32) -> Option<&HistoryRow> {
        let offset = index.checked_sub(self.front())?;
        self.rows.get(usize::try_from(offset).ok()?)
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.anchor = None;
    }

    /// Bring the ring in line with a live frame. Rows survive only while the
    /// pane's content and width are unchanged; everything else is retired
    /// rather than re-anchored, because a shifted index paints the wrong row.
    /// True when rows survived.
    pub fn observe(&mut self, viewport: &TerminalViewport) -> bool {
        let matches = self.anchor.is_some_and(|anchor| {
            anchor.generation == viewport.generation && anchor.columns == viewport.columns
        });
        if matches {
            if let Some(anchor) = self.anchor.as_mut() {
                anchor.scrollbar = viewport.scrollbar;
            }
            return !self.rows.is_empty();
        }
        self.rows.clear();
        self.anchor = Some(Anchor {
            generation: viewport.generation,
            columns: viewport.columns,
            scrollbar: viewport.scrollbar,
        });
        false
    }

    /// Merge one chunk against the frame that was live when it arrived. A chunk
    /// the pane has already outrun is dropped: the daemon answers from its own
    /// current state, so a mismatched scrollbar means the rows describe a
    /// scrollback that has already moved.
    pub fn absorb(&mut self, chunk: HistoryChunk, viewport: &TerminalViewport) -> bool {
        if chunk.columns != viewport.columns
            || chunk.total != viewport.scrollbar.total
            || chunk.offset != viewport.scrollbar.offset
        {
            return false;
        }
        self.observe(viewport);
        let Ok(count) = u32::try_from(chunk.rows.len()) else {
            return false;
        };
        if count == 0 {
            return false;
        }
        let Some(end) = chunk.start.checked_add(count) else {
            return false;
        };
        if end != self.front() {
            self.rows.clear();
        }
        let dictionary = Arc::new(chunk.dictionary);
        for cells in chunk.rows.into_iter().rev() {
            self.rows.push_front(HistoryRow {
                cells: Arc::from(cells),
                dictionary: Arc::clone(&dictionary),
            });
        }
        self.front = chunk.start;
        while self.rows.len() > MAX_HISTORY_ROWS {
            self.rows.pop_back();
        }
        true
    }

    /// The next `(start, count)` to ask for so the ring reaches back to
    /// `target`, or nothing when it already does — or when the budget is spent.
    pub fn next_request(&self, target: u32, budget: usize) -> Option<(u32, u32)> {
        let budget = budget.min(MAX_HISTORY_ROWS);
        let room = u32::try_from(budget.checked_sub(self.rows.len())?).unwrap_or(u32::MAX);
        let front = self.front();
        if front == 0 || room == 0 {
            return None;
        }
        let needed = front.checked_sub(target.min(front))?;
        let count = needed.min(MAX_HISTORY_CHUNK_ROWS).min(room).min(front);
        (count > 0).then(|| (front - count, count))
    }
}

/// True when a wheel notch should move a local overlay rather than travel to
/// the daemon: only a live, untracked pane with scrollback and retained rows
/// can be painted from client memory.
pub fn local_scroll_gate(viewport: &TerminalViewport, retained_rows: usize) -> bool {
    matches!(viewport.mode, zz_terminal::TerminalMode::Live)
        && !viewport.mouse_tracking
        && viewport.scrollbar.total > viewport.scrollbar.len
        && retained_rows != 0
}

/// The topmost offset a viewport can be scrolled to.
pub fn max_scroll_offset(scrollbar: ScrollbarState) -> u32 {
    scrollbar.total.saturating_sub(scrollbar.len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_terminal::{Color, PackedStyle, SessionStatus, UnderlineStyle};

    fn viewport(generation: u64, total: u32, offset: u32) -> TerminalViewport {
        let mut viewport = TerminalViewport::blank(4, 2, SessionStatus::Running);
        viewport.generation = generation;
        viewport.scrollbar = ScrollbarState {
            total,
            offset,
            len: 2,
        };
        viewport
    }

    fn chunk(start: u32, rows: usize, total: u32, offset: u32) -> HistoryChunk {
        HistoryChunk {
            start,
            total,
            offset,
            columns: 4,
            rows: (0..rows).map(|_| vec![PackedCell::EMPTY; 4]).collect(),
            dictionary: TerminalDictionary::from_shared(
                Arc::from([PackedStyle::new(
                    Color::default(),
                    Color::default(),
                    None,
                    0,
                    UnderlineStyle::None,
                )]),
                Arc::from([0_u32]),
                Arc::from([] as [u8; 0]),
            ),
        }
    }

    #[test]
    fn a_chunk_lands_immediately_above_the_live_viewport() {
        let mut ring = HistoryRing::default();
        let live = viewport(7, 102, 100);
        ring.observe(&live);

        assert!(ring.absorb(chunk(90, 10, 102, 100), &live));

        assert_eq!(ring.len(), 10);
        assert_eq!(ring.front(), 90);
        assert!(ring.row(90).is_some());
        assert!(ring.row(99).is_some());
        assert!(ring.row(100).is_none(), "the live top is not history");
        assert!(ring.row(89).is_none());
    }

    #[test]
    fn chunks_stack_downwards_and_a_gap_restarts_the_span() {
        let mut ring = HistoryRing::default();
        let live = viewport(7, 102, 100);
        ring.observe(&live);
        ring.absorb(chunk(90, 10, 102, 100), &live);

        assert!(ring.absorb(chunk(80, 10, 102, 100), &live));
        assert_eq!(ring.len(), 20);
        assert_eq!(ring.front(), 80);

        assert!(ring.absorb(chunk(20, 5, 102, 100), &live));
        assert_eq!(ring.len(), 5, "a non-contiguous chunk replaces the span");
        assert_eq!(ring.front(), 20);
    }

    /// A capped scrollback evicts its oldest row for every new line without
    /// moving `total` or `offset`, so only the generation can retire the ring.
    #[test]
    fn any_content_change_retires_the_retained_rows() {
        let mut ring = HistoryRing::default();
        let live = viewport(7, 102, 100);
        ring.observe(&live);
        ring.absorb(chunk(90, 10, 102, 100), &live);

        assert!(ring.observe(&viewport(7, 102, 90)), "scrolling keeps rows");
        assert_eq!(ring.front(), 90);
        assert!(
            !ring.observe(&viewport(8, 102, 100)),
            "new output drops them"
        );
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn a_chunk_the_pane_has_outrun_is_dropped() {
        let mut ring = HistoryRing::default();
        let live = viewport(7, 102, 100);
        ring.observe(&live);

        assert!(!ring.absorb(chunk(90, 10, 140, 138), &live));
        assert!(ring.is_empty());
    }

    #[test]
    fn requests_walk_backwards_in_chunk_sized_steps_and_stop_at_the_budget() {
        let mut ring = HistoryRing::default();
        let live = viewport(7, 5_000, 4_000);
        ring.observe(&live);

        assert_eq!(
            ring.next_request(0, MAX_HISTORY_ROWS),
            Some((4_000 - MAX_HISTORY_CHUNK_ROWS, MAX_HISTORY_CHUNK_ROWS))
        );
        assert_eq!(
            ring.next_request(3_990, MAX_HISTORY_ROWS),
            Some((3_990, 10))
        );
        assert_eq!(ring.next_request(4_000, MAX_HISTORY_ROWS), None);
        assert_eq!(ring.next_request(0, 0), None);

        ring.absorb(chunk(3_990, 10, 5_000, 4_000), &live);
        assert_eq!(ring.next_request(3_990, MAX_HISTORY_ROWS), None);
        assert_eq!(ring.next_request(3_985, MAX_HISTORY_ROWS), Some((3_985, 5)));
        assert_eq!(
            ring.next_request(0, 10),
            None,
            "the budget is already spent"
        );
    }

    #[test]
    fn the_oldest_rows_fall_out_once_the_ring_is_full() {
        let mut ring = HistoryRing::default();
        let live = viewport(7, 60_000, 50_000);
        ring.observe(&live);

        let mut front = 50_000;
        while ring.len() < MAX_HISTORY_ROWS {
            front -= 500;
            assert!(ring.absorb(chunk(front, 500, 60_000, 50_000), &live));
        }
        let full = ring.len();
        front -= 500;
        ring.absorb(chunk(front, 500, 60_000, 50_000), &live);

        assert_eq!(ring.len(), full);
        assert_eq!(ring.len(), MAX_HISTORY_ROWS);
        assert_eq!(ring.front(), front);
        assert!(ring.row(front).is_some());
        assert!(
            ring.row(front + MAX_HISTORY_ROWS as u32).is_none(),
            "the newest rows are the ones the cap keeps"
        );
    }

    #[test]
    fn a_local_scroll_needs_history_a_live_untracked_pane_and_room_to_move() {
        let mut live = viewport(7, 102, 100);
        assert!(local_scroll_gate(&live, 10));
        assert!(!local_scroll_gate(&live, 0));

        live.mouse_tracking = true;
        assert!(!local_scroll_gate(&live, 10));
        live.mouse_tracking = false;
        live.mode = zz_terminal::TerminalMode::Copy {
            position: 0,
            total: 0,
        };
        assert!(!local_scroll_gate(&live, 10));

        let flat = viewport(7, 2, 0);
        assert!(!local_scroll_gate(&flat, 10));
        assert_eq!(max_scroll_offset(flat.scrollbar), 0);
        assert_eq!(max_scroll_offset(live.scrollbar), 100);
    }
}
