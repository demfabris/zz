use std::sync::Arc;

use zz_terminal::{
    CellWidth, Color, Cursor, CursorStyle, GRAPHEME_TABLE_BIT, KittyLayer, KittyPlacement,
    MAX_KITTY_PLACEMENTS, NO_COLOR, OverlayKind, OverlaySpan, PackedCell, PackedStyle,
    ScrollbarState, SearchStatus, SessionStatus, TerminalDictionary, TerminalDictionaryPatch,
    TerminalMode, TerminalPatchFields, TerminalPatchRows, TerminalPatchSpan, TerminalPatchSpans,
    TerminalPresentation, TerminalViewport, TerminalViewportPatch, exposed_rows_are_replaced,
    shared_default_presentation, shared_empty_kitty_placements, shared_empty_overlays,
};

use crate::{
    PaneId,
    framing::{Lane, ProtocolError, begin_enveloped_into, finish_enveloped_in_place},
};

pub(crate) const FULL_VIEWPORT: u8 = 0;
pub(crate) const VIEWPORT_PATCH: u8 = 1;
pub(crate) const COMMAND_OUTPUT_VIEWPORT: u8 = 2;
pub(crate) const HISTORY_CHUNK: u8 = 3;

pub(crate) const MAX_TITLE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_WORKING_DIRECTORY_BYTES: usize = 16 * 1024;
pub(crate) const MAX_HOVER_URI_BYTES: usize = 16 * 1024;
pub(crate) const MAX_STATUS_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_STYLE_COUNT: usize = 65_536;
pub(crate) const MAX_GRAPHEME_COUNT: usize = 1024 * 1024;
pub(crate) const MAX_GRAPHEME_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_OVERLAY_COUNT: usize = 1024 * 1024;
pub const MAX_HISTORY_CHUNK_ROWS: usize = 512;
const MAX_GRID_CELLS: usize = crate::MAX_FRAME_BYTES / 8;
const KITTY_PLACEMENT_WIRE_BYTES: usize = 72;

const RUN_TEXT: u64 = 0;
const RUN_WIDE: u64 = 1;
const RUN_REPEAT: u64 = 2;
const RUN_RAW: u64 = 3;
const RUN_KIND_MASK: u64 = 0b11;
const RUN_STYLE: u64 = 1 << 2;
const RUN_COUNT_SHIFT: u32 = 3;
const REPEAT_MIN: usize = 6;
const WIDE_FLAGS: u16 = CellWidth::Wide as u16;
const SPACER_TAIL_FLAGS: u16 = CellWidth::SpacerTail as u16;

const STYLE_UNDERLINE_COLOR: u8 = 1 << 0;
const STYLE_UNDERLINE_KIND_SHIFT: u8 = 1;
const STYLE_UNDERLINE_KIND_MASK: u8 = 0b111;
const STYLE_ATTRIBUTES: u8 = 1 << 4;
const STYLE_CLASSES: u8 = 1 << 5;
const STYLE_FLAGS_MASK: u8 = 0b0011_1111;

const CURSOR_PRESENT: u8 = 1 << 0;
const CURSOR_VISIBLE: u8 = 1 << 1;
const CURSOR_BLINKING: u8 = 1 << 2;
const CURSOR_WIDE_TAIL: u8 = 1 << 3;
const CURSOR_STYLE_SHIFT: u8 = 4;

const SEARCH_PRESENT: u8 = 1 << 0;
const SEARCH_PENDING: u8 = 1 << 1;
const SEARCH_INVALID_PATTERN: u8 = 1 << 2;

const INPUT_KITTY_KEYBOARD: u8 = 1 << 0;
const INPUT_MOUSE_TRACKING: u8 = 1 << 1;

const METADATA_FIELDS: u16 = TerminalPatchFields::CURSOR_AT.bits()
    | TerminalPatchFields::SCROLLBAR.bits()
    | TerminalPatchFields::OVERLAYS.bits()
    | TerminalPatchFields::CURSOR.bits()
    | TerminalPatchFields::PRESENTATION.bits()
    | TerminalPatchFields::COLORS.bits()
    | TerminalPatchFields::MODE.bits()
    | TerminalPatchFields::SEARCH.bits()
    | TerminalPatchFields::UNSEEN.bits()
    | TerminalPatchFields::INPUT_MODES.bits()
    | TerminalPatchFields::STATUS.bits()
    | TerminalPatchFields::KITTY.bits();
const FULL_FIELDS: u16 = METADATA_FIELDS & !TerminalPatchFields::CURSOR_AT.bits();

pub(crate) struct HistoryChunkRef<'a> {
    pub start: u32,
    pub total: u32,
    pub offset: u32,
    pub columns: u16,
    pub rows: &'a [Vec<PackedCell>],
    pub dictionary: &'a TerminalDictionary,
}

pub(crate) struct HistoryChunk {
    pub pane: PaneId,
    pub sequence: u64,
    pub start: u32,
    pub total: u32,
    pub offset: u32,
    pub columns: u16,
    pub rows: Vec<Vec<PackedCell>>,
    pub dictionary: TerminalDictionary,
}

struct Metadata<'a> {
    foreground: Color,
    background: Color,
    presentation: &'a TerminalPresentation,
    overlays: &'a [OverlaySpan],
    kitty_placements: &'a [KittyPlacement],
    cursor: Option<Cursor>,
    scrollbar: ScrollbarState,
    mode: TerminalMode,
    search: Option<SearchStatus>,
    unseen_output: u32,
    kitty_keyboard: bool,
    mouse_tracking: bool,
    status: &'a SessionStatus,
}

impl<'a> Metadata<'a> {
    fn of_viewport(viewport: &'a TerminalViewport) -> Self {
        Self {
            foreground: viewport.foreground,
            background: viewport.background,
            presentation: &viewport.presentation,
            overlays: &viewport.overlays,
            kitty_placements: &viewport.kitty_placements,
            cursor: viewport.cursor,
            scrollbar: viewport.scrollbar,
            mode: viewport.mode,
            search: viewport.search,
            unseen_output: viewport.unseen_output,
            kitty_keyboard: viewport.kitty_keyboard,
            mouse_tracking: viewport.mouse_tracking,
            status: &viewport.status,
        }
    }

    fn of_patch(patch: &'a TerminalViewportPatch) -> Self {
        Self {
            foreground: patch.foreground,
            background: patch.background,
            presentation: &patch.presentation,
            overlays: &patch.overlays,
            kitty_placements: &patch.kitty_placements,
            cursor: patch.cursor,
            scrollbar: patch.scrollbar,
            mode: patch.mode,
            search: patch.search,
            unseen_output: patch.unseen_output,
            kitty_keyboard: patch.kitty_keyboard,
            mouse_tracking: patch.mouse_tracking,
            status: &patch.status,
        }
    }

    fn fields_differing_from_default(&self) -> TerminalPatchFields {
        let mut fields = TerminalPatchFields::empty();
        fields.set(
            TerminalPatchFields::COLORS,
            self.foreground != Color::default() || self.background != Color::default(),
        );
        fields.set(
            TerminalPatchFields::PRESENTATION,
            *self.presentation != *shared_default_presentation(),
        );
        fields.set(TerminalPatchFields::OVERLAYS, !self.overlays.is_empty());
        fields.set(
            TerminalPatchFields::KITTY,
            !self.kitty_placements.is_empty(),
        );
        fields.set(TerminalPatchFields::CURSOR, self.cursor.is_some());
        fields.set(
            TerminalPatchFields::SCROLLBAR,
            self.scrollbar != ScrollbarState::default(),
        );
        fields.set(TerminalPatchFields::MODE, self.mode != TerminalMode::Live);
        fields.set(TerminalPatchFields::SEARCH, self.search.is_some());
        fields.set(TerminalPatchFields::UNSEEN, self.unseen_output != 0);
        fields.set(
            TerminalPatchFields::INPUT_MODES,
            self.kitty_keyboard || self.mouse_tracking,
        );
        fields.set(
            TerminalPatchFields::STATUS,
            *self.status != SessionStatus::default(),
        );
        fields
    }
}

struct DecodedMetadata {
    foreground: Color,
    background: Color,
    presentation: Arc<TerminalPresentation>,
    overlays: Arc<[OverlaySpan]>,
    kitty_placements: Arc<[KittyPlacement]>,
    cursor: Option<Cursor>,
    scrollbar: ScrollbarState,
    mode: TerminalMode,
    search: Option<SearchStatus>,
    unseen_output: u32,
    kitty_keyboard: bool,
    mouse_tracking: bool,
    status: SessionStatus,
}

impl Default for DecodedMetadata {
    fn default() -> Self {
        Self {
            foreground: Color::default(),
            background: Color::default(),
            presentation: shared_default_presentation(),
            overlays: shared_empty_overlays(),
            kitty_placements: shared_empty_kitty_placements(),
            cursor: None,
            scrollbar: ScrollbarState::default(),
            mode: TerminalMode::Live,
            search: None,
            unseen_output: 0,
            kitty_keyboard: false,
            mouse_tracking: false,
            status: SessionStatus::default(),
        }
    }
}

#[derive(Clone, Copy)]
struct CellLimits {
    styles: usize,
    graphemes: usize,
}

pub(crate) fn encode_viewport_frame(
    output: &mut Vec<u8>,
    kind: u8,
    pane: PaneId,
    sequence: u64,
    output_id: Option<u64>,
    viewport: &TerminalViewport,
) -> Result<(), ProtocolError> {
    match (kind, output_id) {
        (COMMAND_OUTPUT_VIEWPORT, Some(0)) => {
            return invalid("command output viewport has a zero output ID");
        }
        (COMMAND_OUTPUT_VIEWPORT, None) => {
            return invalid("command output viewport is missing its output ID");
        }
        (FULL_VIEWPORT, Some(_)) => {
            return invalid("terminal viewport carries a command output ID");
        }
        _ => {}
    }
    begin_enveloped_into(output, Lane::Terminal, viewport_capacity_hint(viewport))?;
    output.push(kind);
    push_varint(output, pane.0);
    push_varint(output, sequence);
    if let Some(output_id) = output_id {
        push_varint(output, output_id);
    }
    encode_viewport_body(output, viewport)?;
    finish_enveloped_in_place(output)
}

pub(crate) fn encode_patch_frame(
    output: &mut Vec<u8>,
    pane: PaneId,
    sequence: u64,
    patch: &TerminalViewportPatch,
) -> Result<(), ProtocolError> {
    begin_enveloped_into(output, Lane::Terminal, patch_capacity_hint(patch))?;
    output.push(VIEWPORT_PATCH);
    push_varint(output, pane.0);
    push_varint(output, sequence);
    encode_patch_body(output, patch)?;
    finish_enveloped_in_place(output)
}

pub(crate) fn encode_history_frame(
    output: &mut Vec<u8>,
    pane: PaneId,
    sequence: u64,
    chunk: &HistoryChunkRef<'_>,
) -> Result<(), ProtocolError> {
    let columns = usize::from(chunk.columns);
    if chunk.rows.len() > MAX_HISTORY_CHUNK_ROWS {
        return invalid("history chunk row count is outside its limit");
    }
    if chunk.rows.iter().any(|row| row.len() != columns) {
        return invalid("history chunk row width does not match its columns");
    }
    if chunk.rows.len() * columns > MAX_GRID_CELLS {
        return invalid("history chunk exceeds its cell limit");
    }
    let hint = 64
        + chunk.dictionary.styles.len() * 12
        + chunk.dictionary.grapheme_bytes.len()
        + chunk.rows.len() * columns.min(96);
    begin_enveloped_into(output, Lane::Terminal, hint)?;
    output.push(HISTORY_CHUNK);
    push_varint(output, pane.0);
    push_varint(output, sequence);
    push_varint(output, u64::from(chunk.start));
    push_varint(output, u64::from(chunk.total));
    push_varint(output, u64::from(chunk.offset));
    push_varint(output, u64::from(chunk.columns));
    push_varint(output, chunk.rows.len() as u64);
    encode_styles(output, &chunk.dictionary.styles)?;
    let graphemes = encode_full_graphemes(
        output,
        &chunk.dictionary.grapheme_offsets,
        &chunk.dictionary.grapheme_bytes,
    )?;
    let limits = CellLimits {
        styles: chunk.dictionary.styles.len(),
        graphemes,
    };
    let mut previous = None;
    for (index, row) in chunk.rows.iter().enumerate() {
        encode_content_row(output, &mut previous, index, row, limits)?;
    }
    output.push(0);
    finish_enveloped_in_place(output)
}

pub(crate) fn encode_viewport_body(
    output: &mut Vec<u8>,
    viewport: &TerminalViewport,
) -> Result<(), ProtocolError> {
    let columns = usize::from(viewport.columns);
    let rows = usize::from(viewport.rows);
    if columns.checked_mul(rows) != Some(viewport.cells.len()) {
        return invalid("cell count does not match grid dimensions");
    }
    if viewport.cells.len() > MAX_GRID_CELLS {
        return invalid("terminal grid exceeds its cell limit");
    }
    push_varint(output, viewport.view_generation);
    push_zigzag(
        output,
        viewport.view_generation.wrapping_sub(viewport.generation),
    );
    push_varint(output, u64::from(viewport.dictionary_generation));
    push_varint(output, u64::from(viewport.columns));
    push_varint(output, u64::from(viewport.rows));
    let metadata = Metadata::of_viewport(viewport);
    let fields = metadata.fields_differing_from_default();
    push_varint(output, u64::from(fields.bits()));
    encode_metadata(
        output,
        fields,
        &metadata,
        viewport.columns,
        viewport.rows,
        metadata.scrollbar,
    )?;
    if viewport.styles().is_empty() {
        return invalid("style dictionary is empty");
    }
    encode_styles(output, viewport.styles())?;
    let graphemes = encode_full_graphemes(
        output,
        viewport.grapheme_offsets(),
        viewport.grapheme_bytes(),
    )?;
    let limits = CellLimits {
        styles: viewport.styles().len(),
        graphemes,
    };
    let mut previous = None;
    if columns != 0 {
        for (index, row) in viewport.cells.chunks_exact(columns).enumerate() {
            encode_content_row(output, &mut previous, index, row, limits)?;
        }
    }
    output.push(0);
    Ok(())
}

fn encode_patch_body(
    output: &mut Vec<u8>,
    patch: &TerminalViewportPatch,
) -> Result<(), ProtocolError> {
    let base_view = patch.base_view_generation;
    push_varint(output, base_view);
    push_zigzag(output, base_view.wrapping_sub(patch.base_generation));
    push_zigzag(output, patch.view_generation.wrapping_sub(base_view));
    push_zigzag(output, patch.generation.wrapping_sub(patch.base_generation));
    push_varint(output, u64::from(patch.dictionary_generation));
    push_varint(output, u64::from(patch.columns));
    push_varint(output, u64::from(patch.rows));
    let mut fields =
        TerminalPatchFields::from_bits(patch.fields.bits() & METADATA_FIELDS).unwrap_or_default();
    if fields.contains(TerminalPatchFields::CURSOR) {
        fields.set(TerminalPatchFields::CURSOR_AT, false);
    }
    if fields.contains(TerminalPatchFields::KITTY) {
        fields.insert(TerminalPatchFields::SCROLLBAR);
    }
    fields.set(TerminalPatchFields::ROWS, !patch.changed_rows.is_empty());
    fields.set(TerminalPatchFields::SCROLL, patch.scroll != 0);
    fields.set(
        TerminalPatchFields::DICTIONARY,
        !patch.dictionary.is_empty(),
    );
    push_varint(output, u64::from(fields.bits()));
    if patch.scroll != 0 {
        let shift = usize::from(patch.scroll.unsigned_abs());
        if shift >= usize::from(patch.rows) {
            return invalid("terminal row shift is outside the viewport");
        }
        push_zigzag(output, i64::from(patch.scroll).cast_unsigned());
    }
    let metadata = Metadata::of_patch(patch);
    encode_metadata(
        output,
        fields,
        &metadata,
        patch.columns,
        patch.rows,
        patch.scrollbar,
    )?;
    let appended_styles = patch.dictionary.appended_styles();
    let appended_lengths = patch.dictionary.appended_grapheme_lengths();
    let style_count = usize::try_from(patch.style_base)
        .ok()
        .and_then(|base| base.checked_add(appended_styles.len()))
        .filter(|count| *count <= MAX_STYLE_COUNT)
        .ok_or_else(|| terminal_error("terminal patch style dictionary exceeds limit"))?;
    let grapheme_count = usize::try_from(patch.grapheme_base)
        .ok()
        .and_then(|base| base.checked_add(appended_lengths.len()))
        .filter(|count| *count <= MAX_GRAPHEME_COUNT)
        .ok_or_else(|| terminal_error("terminal patch grapheme dictionary exceeds limit"))?;
    if !patch.dictionary.is_empty() {
        push_varint(output, u64::from(patch.style_base));
        encode_styles(output, appended_styles)?;
        push_varint(output, u64::from(patch.grapheme_base));
        encode_appended_graphemes(
            output,
            appended_lengths,
            patch.dictionary.appended_grapheme_bytes(),
        )?;
    }
    if !patch.changed_rows.is_empty() {
        let limits = CellLimits {
            styles: style_count,
            graphemes: grapheme_count,
        };
        encode_spans(output, patch, limits)?;
    } else if patch.scroll != 0 {
        return invalid("terminal patch does not replace newly exposed rows");
    }
    Ok(())
}

fn encode_spans(
    output: &mut Vec<u8>,
    patch: &TerminalViewportPatch,
    limits: CellLimits,
) -> Result<(), ProtocolError> {
    let spans = patch.changed_rows.spans();
    let cells = patch.changed_rows.cells();
    let mut previous_row = None::<u16>;
    let mut source = 0_usize;
    for span in spans {
        if span.row >= patch.rows
            || previous_row.is_some_and(|previous| previous >= span.row)
            || span.end() > usize::from(patch.columns)
        {
            return invalid("terminal patch contains an invalid, duplicate, or out-of-order row");
        }
        let end = source
            .checked_add(usize::from(span.len))
            .filter(|end| *end <= cells.len())
            .ok_or_else(|| {
                terminal_error("terminal patch flat cell plane does not match its rows")
            })?;
        push_varint(
            output,
            u64::from(span.row) - previous_row.map_or(0, |row| u64::from(row) + 1) + 1,
        );
        push_varint(output, u64::from(span.start) << 1 | u64::from(span.clear));
        encode_runs(output, &cells[source..end], limits)?;
        previous_row = Some(span.row);
        source = end;
    }
    if source != cells.len() {
        return invalid("terminal patch flat cell plane does not match its rows");
    }
    if !exposed_rows_are_replaced(spans, patch.scroll, patch.rows, patch.columns) {
        return invalid("terminal patch does not replace newly exposed rows");
    }
    output.push(0);
    Ok(())
}

fn encode_content_row(
    output: &mut Vec<u8>,
    previous: &mut Option<usize>,
    index: usize,
    row: &[PackedCell],
    limits: CellLimits,
) -> Result<(), ProtocolError> {
    let Some(first) = row.iter().position(|cell| *cell != PackedCell::EMPTY) else {
        return Ok(());
    };
    let end = row
        .iter()
        .rposition(|cell| *cell != PackedCell::EMPTY)
        .map_or(first, |last| last + 1);
    push_varint(
        output,
        (index - previous.map_or(0, |previous| previous + 1) + 1) as u64,
    );
    push_varint(output, (first as u64) << 1);
    encode_runs(output, &row[first..end], limits)?;
    *previous = Some(index);
    Ok(())
}

fn encode_runs(
    output: &mut Vec<u8>,
    cells: &[PackedCell],
    limits: CellLimits,
) -> Result<(), ProtocolError> {
    let mut style = 0_u16;
    let mut index = 0;
    while index < cells.len() {
        let cell = cells[index];
        let repeat = repeat_len(cells, index);
        let (kind, count, advance) = if repeat >= REPEAT_MIN {
            (RUN_REPEAT, repeat, repeat)
        } else if is_text(cell) {
            let count = text_len(cells, index);
            (RUN_TEXT, count, count)
        } else if is_wide_pair(cells, index) {
            let count = wide_len(cells, index);
            (RUN_WIDE, count, count * 2)
        } else {
            let count = raw_len(cells, index);
            (RUN_RAW, count, count)
        };
        let restyle = cell.style_id() != style;
        push_varint(
            output,
            (count as u64) << RUN_COUNT_SHIFT | if restyle { RUN_STYLE } else { 0 } | kind,
        );
        if restyle {
            push_varint(output, u64::from(cell.style_id()));
            style = cell.style_id();
        }
        let run = &cells[index..index + advance];
        match kind {
            RUN_TEXT => {
                for cell in run {
                    check_cell(*cell, limits)?;
                    push_scalar(output, cell.glyph());
                }
            }
            RUN_WIDE => {
                for pair in run.chunks_exact(2) {
                    check_cell(pair[0], limits)?;
                    push_scalar(output, pair[0].glyph());
                }
            }
            RUN_REPEAT => {
                check_cell(cell, limits)?;
                push_varint(output, glyph_code(cell.glyph()));
                push_varint(output, u64::from(cell.flags()));
            }
            _ => {
                for cell in run {
                    check_cell(*cell, limits)?;
                    push_varint(output, glyph_code(cell.glyph()));
                    push_varint(output, u64::from(cell.flags()));
                }
            }
        }
        index += advance;
    }
    output.push(0);
    Ok(())
}

fn repeat_len(cells: &[PackedCell], index: usize) -> usize {
    let cell = cells[index];
    cells[index..]
        .iter()
        .take_while(|candidate| **candidate == cell)
        .count()
}

fn is_text(cell: PackedCell) -> bool {
    cell.flags() == 0 && cell.glyph() & GRAPHEME_TABLE_BIT == 0
}

fn text_len(cells: &[PackedCell], index: usize) -> usize {
    let style = cells[index].style_id();
    let mut end = index;
    let mut same = 0;
    while end < cells.len() && is_text(cells[end]) && cells[end].style_id() == style {
        if end > index && cells[end] == cells[end - 1] {
            same += 1;
            if same + 1 >= REPEAT_MIN {
                end -= same;
                break;
            }
        } else {
            same = 0;
        }
        end += 1;
    }
    end - index
}

fn is_wide_pair(cells: &[PackedCell], index: usize) -> bool {
    let head = cells[index];
    head.flags() == WIDE_FLAGS
        && head.glyph() & GRAPHEME_TABLE_BIT == 0
        && head.glyph() != 0
        && cells.get(index + 1).is_some_and(|tail| {
            *tail == PackedCell::from_raw(0, head.style_id(), SPACER_TAIL_FLAGS)
        })
}

fn wide_len(cells: &[PackedCell], index: usize) -> usize {
    let style = cells[index].style_id();
    let mut end = index;
    while end < cells.len() && cells[end].style_id() == style && is_wide_pair(cells, end) {
        end += 2;
    }
    (end - index) / 2
}

fn raw_len(cells: &[PackedCell], index: usize) -> usize {
    let style = cells[index].style_id();
    let mut end = index + 1;
    while end < cells.len()
        && cells[end].style_id() == style
        && !is_text(cells[end])
        && !is_wide_pair(cells, end)
        && repeat_len(cells, end) < REPEAT_MIN
    {
        end += 1;
    }
    end - index
}

fn check_cell(cell: PackedCell, limits: CellLimits) -> Result<(), ProtocolError> {
    if usize::from(cell.style_id()) >= limits.styles {
        return invalid("cell references a missing style");
    }
    let glyph = cell.glyph();
    if glyph & GRAPHEME_TABLE_BIT != 0 {
        if (glyph & !GRAPHEME_TABLE_BIT) as usize >= limits.graphemes {
            return invalid("cell references a missing grapheme");
        }
    } else if glyph != 0 && char::from_u32(glyph).is_none() {
        return invalid("cell contains an invalid Unicode scalar");
    }
    Ok(())
}

fn push_scalar(output: &mut Vec<u8>, glyph: u32) {
    if glyph < 0x80 {
        output.push(glyph as u8);
    } else if let Some(character) = char::from_u32(glyph) {
        let mut buffer = [0_u8; 4];
        output.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
    }
}

const fn glyph_code(glyph: u32) -> u64 {
    if glyph & GRAPHEME_TABLE_BIT != 0 {
        ((glyph & !GRAPHEME_TABLE_BIT) as u64) << 1 | 1
    } else {
        (glyph as u64) << 1
    }
}

fn encode_metadata(
    output: &mut Vec<u8>,
    fields: TerminalPatchFields,
    metadata: &Metadata<'_>,
    columns: u16,
    rows: u16,
    scrollbar: ScrollbarState,
) -> Result<(), ProtocolError> {
    if fields.contains(TerminalPatchFields::CURSOR_AT) {
        let Some(cursor) = metadata.cursor else {
            return invalid("cursor move carries no cursor");
        };
        check_cursor(cursor, columns, rows)?;
        push_varint(
            output,
            u64::from(cursor.column()) << 1 | u64::from(cursor.at_wide_tail()),
        );
        push_varint(output, u64::from(cursor.row()));
    }
    if fields.contains(TerminalPatchFields::SCROLLBAR) {
        check_scrollbar(metadata.scrollbar)?;
        push_varint(output, u64::from(metadata.scrollbar.total));
        push_varint(output, u64::from(metadata.scrollbar.offset));
        push_varint(output, u64::from(metadata.scrollbar.len));
    }
    if fields.contains(TerminalPatchFields::OVERLAYS) {
        if metadata.overlays.len() > MAX_OVERLAY_COUNT {
            return invalid("overlay count exceeds its limit");
        }
        push_varint(output, metadata.overlays.len() as u64);
        for overlay in metadata.overlays {
            check_overlay(*overlay, columns, rows)?;
            push_varint(output, u64::from(overlay.row));
            push_varint(output, u64::from(overlay.start));
            push_varint(output, u64::from(overlay.end));
            push_varint(output, u64::from(overlay.kind_and_flags()));
        }
    }
    if fields.contains(TerminalPatchFields::CURSOR) {
        match metadata.cursor {
            None => output.push(0),
            Some(cursor) => {
                check_cursor(cursor, columns, rows)?;
                let style = match cursor.style() {
                    CursorStyle::Bar => 0,
                    CursorStyle::Block => 1,
                    CursorStyle::Underline => 2,
                    CursorStyle::BlockHollow => 3,
                };
                output.push(
                    CURSOR_PRESENT
                        | if cursor.visible() { CURSOR_VISIBLE } else { 0 }
                        | if cursor.blinking() {
                            CURSOR_BLINKING
                        } else {
                            0
                        }
                        | if cursor.at_wide_tail() {
                            CURSOR_WIDE_TAIL
                        } else {
                            0
                        }
                        | style << CURSOR_STYLE_SHIFT,
                );
                push_varint(output, u64::from(cursor.column()));
                push_varint(output, u64::from(cursor.row()));
                push_rgb(output, cursor.color().packed());
            }
        }
    }
    if fields.contains(TerminalPatchFields::PRESENTATION) {
        let presentation = metadata.presentation;
        if presentation.title.len() > MAX_TITLE_BYTES {
            return invalid("title exceeds terminal metadata limit");
        }
        validate_working_directory(presentation.working_directory.as_deref())?;
        validate_hovered_uri(presentation.hovered_uri.as_deref())?;
        push_bytes(output, presentation.title.as_bytes());
        push_optional_bytes(
            output,
            presentation.working_directory.as_deref().map(str::as_bytes),
        );
        push_optional_bytes(
            output,
            presentation.hovered_uri.as_deref().map(str::as_bytes),
        );
    }
    if fields.contains(TerminalPatchFields::COLORS) {
        push_rgb(output, metadata.foreground.packed());
        push_rgb(output, metadata.background.packed());
    }
    if fields.contains(TerminalPatchFields::MODE) {
        check_mode(metadata.mode)?;
        match metadata.mode {
            TerminalMode::Live => output.push(0),
            TerminalMode::Copy {
                position,
                total,
                hide_position,
            } => {
                output.push(1);
                push_varint(output, u64::from(position));
                push_varint(output, u64::from(total));
                output.push(u8::from(hide_position));
            }
            TerminalMode::View { position, total } => {
                output.push(2);
                push_varint(output, u64::from(position));
                push_varint(output, u64::from(total));
            }
        }
    }
    if fields.contains(TerminalPatchFields::SEARCH) {
        match metadata.search {
            None => output.push(0),
            Some(search) => {
                if search.current() > search.total {
                    return invalid("search status is inconsistent");
                }
                output.push(
                    SEARCH_PRESENT
                        | if search.pending() { SEARCH_PENDING } else { 0 }
                        | if search.invalid_pattern() {
                            SEARCH_INVALID_PATTERN
                        } else {
                            0
                        },
                );
                push_varint(output, u64::from(search.current()));
                push_varint(output, u64::from(search.total));
            }
        }
    }
    if fields.contains(TerminalPatchFields::UNSEEN) {
        push_varint(output, u64::from(metadata.unseen_output));
    }
    if fields.contains(TerminalPatchFields::INPUT_MODES) {
        output.push(
            if metadata.kitty_keyboard {
                INPUT_KITTY_KEYBOARD
            } else {
                0
            } | if metadata.mouse_tracking {
                INPUT_MOUSE_TRACKING
            } else {
                0
            },
        );
    }
    if fields.contains(TerminalPatchFields::STATUS) {
        encode_status(output, metadata.status)?;
    }
    if fields.contains(TerminalPatchFields::KITTY) {
        validate_kitty_placements(metadata.kitty_placements, columns, rows, scrollbar)?;
        push_varint(output, metadata.kitty_placements.len() as u64);
        encode_kitty_placements(output, metadata.kitty_placements);
    }
    Ok(())
}

fn encode_styles(output: &mut Vec<u8>, styles: &[PackedStyle]) -> Result<(), ProtocolError> {
    if styles.len() > MAX_STYLE_COUNT {
        return invalid("style dictionary exceeds its limit");
    }
    push_varint(output, styles.len() as u64);
    for style in styles {
        if !valid_style(*style) {
            return invalid("style contains an invalid packed value");
        }
        let underline_color = style.underline_color_raw();
        let attributes = style.attributes();
        let classes = style.class_word();
        output.push(
            if underline_color == NO_COLOR {
                0
            } else {
                STYLE_UNDERLINE_COLOR
            } | style.underline_kind_raw() << STYLE_UNDERLINE_KIND_SHIFT
                | if attributes == 0 { 0 } else { STYLE_ATTRIBUTES }
                | if classes == 0 { 0 } else { STYLE_CLASSES },
        );
        push_rgb(output, style.foreground_raw());
        push_rgb(output, style.background_raw());
        if underline_color != NO_COLOR {
            push_rgb(output, underline_color);
        }
        if attributes != 0 {
            push_varint(output, u64::from(attributes));
        }
        if classes != 0 {
            push_varint(output, u64::from(classes));
        }
    }
    Ok(())
}

fn encode_full_graphemes(
    output: &mut Vec<u8>,
    offsets: &[u32],
    bytes: &[u8],
) -> Result<usize, ProtocolError> {
    if bytes.len() > MAX_GRAPHEME_BYTES || offsets.len() > MAX_GRAPHEME_COUNT.saturating_add(1) {
        return invalid("grapheme dictionary exceeds its limit");
    }
    if offsets.first() != Some(&0) || offsets.last().copied() != u32::try_from(bytes.len()).ok() {
        return invalid("grapheme offsets do not cover the byte arena");
    }
    let count = offsets.len() - 1;
    push_varint(output, count as u64);
    let mut previous = 0_usize;
    for offset in &offsets[1..] {
        let end = *offset as usize;
        if end < previous || end > bytes.len() {
            return invalid("grapheme offsets are not monotonic");
        }
        std::str::from_utf8(&bytes[previous..end])
            .map_err(|_| terminal_error("grapheme is not valid UTF-8"))?;
        push_varint(output, (end - previous) as u64);
        previous = end;
    }
    output.extend_from_slice(bytes);
    Ok(count)
}

fn encode_appended_graphemes(
    output: &mut Vec<u8>,
    lengths: &[u32],
    bytes: &[u8],
) -> Result<(), ProtocolError> {
    if lengths.len() > MAX_GRAPHEME_COUNT || bytes.len() > MAX_GRAPHEME_BYTES {
        return invalid("terminal patch grapheme dictionary exceeds limit");
    }
    push_varint(output, lengths.len() as u64);
    let mut cursor = 0_usize;
    for length in lengths {
        let end = cursor
            .checked_add(*length as usize)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| terminal_error("grapheme lengths exceed appended arena"))?;
        std::str::from_utf8(&bytes[cursor..end])
            .map_err(|_| terminal_error("grapheme is not valid UTF-8"))?;
        push_varint(output, u64::from(*length));
        cursor = end;
    }
    if cursor != bytes.len() {
        return invalid("grapheme lengths do not cover appended arena");
    }
    output.extend_from_slice(bytes);
    Ok(())
}

fn encode_status(output: &mut Vec<u8>, status: &SessionStatus) -> Result<(), ProtocolError> {
    match status {
        SessionStatus::Starting => output.push(0),
        SessionStatus::Running => output.push(1),
        SessionStatus::Exited(exit) => {
            output.push(2);
            push_varint(output, u64::from(exit.code));
            if exit
                .signal
                .as_ref()
                .is_some_and(|signal| signal.len() > MAX_STATUS_BYTES)
            {
                return invalid("terminal status string exceeds limit");
            }
            push_optional_bytes(output, exit.signal.as_deref().map(str::as_bytes));
        }
        SessionStatus::Failed(error) => {
            if error.len() > MAX_STATUS_BYTES {
                return invalid("terminal status string exceeds limit");
            }
            output.push(3);
            push_bytes(output, error.as_bytes());
        }
    }
    Ok(())
}

fn encode_kitty_placements(output: &mut Vec<u8>, placements: &[KittyPlacement]) {
    for placement in placements {
        output.extend_from_slice(&placement.image_id.to_le_bytes());
        output.extend_from_slice(&placement.image_generation.to_le_bytes());
        output.push(placement.layer as u8);
        output.push(u8::from(placement.source_rect.is_some()));
        output.extend_from_slice(&0_u16.to_le_bytes());
        output.extend_from_slice(&placement.viewport_col.to_le_bytes());
        output.extend_from_slice(&placement.viewport_row.to_le_bytes());
        output.extend_from_slice(&placement.absolute_row.to_le_bytes());
        for value in [
            placement.cell_offset_x,
            placement.cell_offset_y,
            placement.grid_cols,
            placement.grid_rows,
            placement.pixel_width,
            placement.pixel_height,
        ] {
            output.extend_from_slice(&value.to_le_bytes());
        }
        let (x, y, width, height) = placement.source_rect.unwrap_or_default();
        for value in [x, y, width, height] {
            output.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn viewport_capacity_hint(viewport: &TerminalViewport) -> usize {
    96 + viewport.title().len()
        + viewport.working_directory().map_or(0, str::len)
        + viewport.hovered_uri().map_or(0, str::len)
        + viewport.styles().len() * 12
        + viewport.grapheme_bytes().len()
        + viewport.grapheme_offsets().len() * 2
        + viewport.overlays.len() * 6
        + viewport.kitty_placements.len() * KITTY_PLACEMENT_WIRE_BYTES
        + viewport.cells.len() / 2
}

fn patch_capacity_hint(patch: &TerminalViewportPatch) -> usize {
    64 + patch.changed_rows.cells().len()
        + patch.changed_rows.len() * 4
        + patch.dictionary.appended_styles().len() * 12
        + patch.dictionary.appended_grapheme_bytes().len()
        + if patch.fields.contains(TerminalPatchFields::PRESENTATION) {
            patch.title().len()
                + patch.working_directory().map_or(0, str::len)
                + patch.hovered_uri().map_or(0, str::len)
        } else {
            0
        }
        + if patch.fields.contains(TerminalPatchFields::OVERLAYS) {
            patch.overlays.len() * 6
        } else {
            0
        }
        + if patch.fields.contains(TerminalPatchFields::KITTY) {
            patch.kitty_placements.len() * KITTY_PLACEMENT_WIRE_BYTES
        } else {
            0
        }
}

pub(crate) fn decode_viewport_frame(
    payload: &[u8],
) -> Result<(PaneId, u64, Option<u64>, TerminalViewport), ProtocolError> {
    let mut reader = Reader::new(payload);
    let kind = reader.u8()?;
    let pane = PaneId(reader.varint()?);
    let sequence = reader.varint()?;
    let output_id = match kind {
        FULL_VIEWPORT => None,
        COMMAND_OUTPUT_VIEWPORT => {
            let output_id = reader.varint()?;
            if output_id == 0 {
                return invalid("command output viewport has a zero output ID");
            }
            Some(output_id)
        }
        _ => return invalid("unknown terminal update type"),
    };
    let viewport = decode_viewport_body(&mut reader)?;
    reader.finish("terminal update has trailing bytes")?;
    Ok((pane, sequence, output_id, viewport))
}

pub(crate) fn decode_viewport_body(
    reader: &mut Reader<'_>,
) -> Result<TerminalViewport, ProtocolError> {
    let view_generation = reader.varint()?;
    let generation = view_generation.wrapping_sub(reader.zigzag()?);
    let dictionary_generation = reader.u32("dictionary generation")?;
    let columns = reader.u16("columns")?;
    let rows = reader.u16("rows")?;
    let fields = reader.fields()?;
    if fields.bits() & !FULL_FIELDS != 0 {
        return invalid("full viewport carries patch-only fields");
    }
    let metadata = decode_metadata(reader, fields, columns, rows)?;
    let styles = decode_styles(reader)?;
    if styles.is_empty() {
        return invalid("style dictionary is empty");
    }
    let (grapheme_offsets, grapheme_bytes) = decode_full_graphemes(reader)?;
    let limits = CellLimits {
        styles: styles.len(),
        graphemes: grapheme_offsets.len() - 1,
    };
    let width = usize::from(columns);
    let cell_count = width * usize::from(rows);
    if cell_count > MAX_GRID_CELLS {
        return invalid("terminal grid exceeds its cell limit");
    }
    let mut cells: Arc<[PackedCell]> = std::iter::repeat_n(PackedCell::EMPTY, cell_count).collect();
    let plane = Arc::get_mut(&mut cells).expect("a freshly collected cell plane is unique");
    decode_content_rows(
        reader,
        &mut FlatRows {
            cells: plane,
            width,
        },
        usize::from(rows),
        width,
        limits,
    )?;
    Ok(TerminalViewport {
        generation,
        view_generation,
        dictionary_generation,
        columns,
        rows,
        foreground: metadata.foreground,
        background: metadata.background,
        presentation: metadata.presentation,
        cells,
        dictionary: Arc::new(TerminalDictionary::from_shared(
            styles.into(),
            grapheme_offsets.into(),
            grapheme_bytes.into(),
        )),
        overlays: metadata.overlays,
        kitty_placements: metadata.kitty_placements,
        cursor: metadata.cursor,
        scrollbar: metadata.scrollbar,
        mode: metadata.mode,
        search: metadata.search,
        unseen_output: metadata.unseen_output,
        kitty_keyboard: metadata.kitty_keyboard,
        mouse_tracking: metadata.mouse_tracking,
        status: metadata.status,
    })
}

pub(crate) fn decode_patch_frame(
    payload: &[u8],
) -> Result<(PaneId, u64, TerminalViewportPatch), ProtocolError> {
    let mut reader = Reader::new(payload);
    if reader.u8()? != VIEWPORT_PATCH {
        return invalid("unknown terminal patch type");
    }
    let pane = PaneId(reader.varint()?);
    let sequence = reader.varint()?;
    let base_view_generation = reader.varint()?;
    let base_generation = base_view_generation.wrapping_sub(reader.zigzag()?);
    let view_generation = base_view_generation.wrapping_add(reader.zigzag()?);
    let generation = base_generation.wrapping_add(reader.zigzag()?);
    let dictionary_generation = reader.u32("dictionary generation")?;
    let columns = reader.u16("columns")?;
    let rows = reader.u16("rows")?;
    if usize::from(columns) * usize::from(rows) > MAX_GRID_CELLS {
        return invalid("terminal grid exceeds its cell limit");
    }
    let fields = reader.fields()?;
    if fields.contains(TerminalPatchFields::CURSOR)
        && fields.contains(TerminalPatchFields::CURSOR_AT)
    {
        return invalid("terminal patch moves and replaces the cursor");
    }
    if fields.contains(TerminalPatchFields::KITTY)
        && !fields.contains(TerminalPatchFields::SCROLLBAR)
    {
        return invalid("terminal patch places images without a scrollbar");
    }
    let scroll = if fields.contains(TerminalPatchFields::SCROLL) {
        let scroll = i16::try_from(reader.zigzag()?.cast_signed())
            .map_err(|_| terminal_error("terminal row shift is outside the viewport"))?;
        if scroll == 0 || usize::from(scroll.unsigned_abs()) >= usize::from(rows) {
            return invalid("terminal row shift is outside the viewport");
        }
        scroll
    } else {
        0
    };
    let metadata = decode_metadata(&mut reader, fields, columns, rows)?;
    let (style_base, grapheme_base, dictionary) =
        if fields.contains(TerminalPatchFields::DICTIONARY) {
            let style_base = reader.u32("style base")?;
            let styles = decode_styles(&mut reader)?;
            let grapheme_base = reader.u32("grapheme base")?;
            let (lengths, bytes) = decode_appended_graphemes(&mut reader)?;
            let dictionary = TerminalDictionaryPatch::from_parts(styles, lengths, bytes);
            if dictionary.is_empty() {
                return invalid("terminal patch dictionary append is empty");
            }
            (style_base, grapheme_base, dictionary)
        } else {
            (0, 0, TerminalDictionaryPatch::default())
        };
    let limits = if dictionary.is_empty() {
        CellLimits {
            styles: MAX_STYLE_COUNT,
            graphemes: MAX_GRAPHEME_COUNT,
        }
    } else {
        CellLimits {
            styles: (style_base as usize)
                .checked_add(dictionary.appended_styles().len())
                .filter(|count| *count <= MAX_STYLE_COUNT)
                .ok_or_else(|| terminal_error("terminal patch style dictionary exceeds limit"))?,
            graphemes: (grapheme_base as usize)
                .checked_add(dictionary.appended_grapheme_lengths().len())
                .filter(|count| *count <= MAX_GRAPHEME_COUNT)
                .ok_or_else(|| {
                    terminal_error("terminal patch grapheme dictionary exceeds limit")
                })?,
        }
    };
    let changed_rows = if fields.contains(TerminalPatchFields::ROWS) {
        decode_spans(&mut reader, columns, rows, limits)?
    } else {
        TerminalPatchRows::default()
    };
    reader.finish("terminal patch has trailing bytes")?;
    if !exposed_rows_are_replaced(changed_rows.spans(), scroll, rows, columns) {
        return invalid("terminal patch does not replace newly exposed rows");
    }
    let patch = TerminalViewportPatch {
        base_generation,
        base_view_generation,
        generation,
        view_generation,
        dictionary_generation,
        columns,
        rows,
        scroll,
        changed_rows,
        style_base,
        grapheme_base,
        dictionary,
        fields,
        foreground: metadata.foreground,
        background: metadata.background,
        presentation: metadata.presentation,
        overlays: metadata.overlays,
        kitty_placements: metadata.kitty_placements,
        cursor: metadata.cursor,
        scrollbar: metadata.scrollbar,
        mode: metadata.mode,
        search: metadata.search,
        unseen_output: metadata.unseen_output,
        kitty_keyboard: metadata.kitty_keyboard,
        mouse_tracking: metadata.mouse_tracking,
        status: metadata.status,
    };
    Ok((pane, sequence, patch))
}

pub(crate) fn decode_history_frame(payload: &[u8]) -> Result<HistoryChunk, ProtocolError> {
    let mut reader = Reader::new(payload);
    if reader.u8()? != HISTORY_CHUNK {
        return invalid("unknown terminal history type");
    }
    let pane = PaneId(reader.varint()?);
    let sequence = reader.varint()?;
    let start = reader.u32("history start")?;
    let total = reader.u32("history total")?;
    let offset = reader.u32("history offset")?;
    let columns = reader.u16("columns")?;
    let count = reader.count(MAX_HISTORY_CHUNK_ROWS, "history row")?;
    if count * usize::from(columns) > MAX_GRID_CELLS {
        return invalid("history chunk exceeds its cell limit");
    }
    let styles = decode_styles(&mut reader)?;
    let (grapheme_offsets, grapheme_bytes) = decode_full_graphemes(&mut reader)?;
    let limits = CellLimits {
        styles: styles.len(),
        graphemes: grapheme_offsets.len() - 1,
    };
    let width = usize::from(columns);
    let mut rows = (0..count)
        .map(|_| vec![PackedCell::EMPTY; width])
        .collect::<Vec<_>>();
    decode_content_rows(&mut reader, &mut rows, count, width, limits)?;
    reader.finish("terminal history has trailing bytes")?;
    Ok(HistoryChunk {
        pane,
        sequence,
        start,
        total,
        offset,
        columns,
        rows,
        dictionary: TerminalDictionary::from_shared(
            styles.into(),
            grapheme_offsets.into(),
            grapheme_bytes.into(),
        ),
    })
}

trait RowTarget {
    fn row(&mut self, row: usize) -> &mut [PackedCell];
}

struct FlatRows<'a> {
    cells: &'a mut [PackedCell],
    width: usize,
}

impl RowTarget for FlatRows<'_> {
    fn row(&mut self, row: usize) -> &mut [PackedCell] {
        &mut self.cells[row * self.width..(row + 1) * self.width]
    }
}

impl RowTarget for Vec<Vec<PackedCell>> {
    fn row(&mut self, row: usize) -> &mut [PackedCell] {
        &mut self[row]
    }
}

trait CellSink {
    fn written(&self) -> usize;
    fn push(&mut self, cell: PackedCell);
    fn repeat(&mut self, cell: PackedCell, count: usize);
}

impl CellSink for Vec<PackedCell> {
    fn written(&self) -> usize {
        self.len()
    }

    fn push(&mut self, cell: PackedCell) {
        Vec::push(self, cell);
    }

    fn repeat(&mut self, cell: PackedCell, count: usize) {
        self.extend(std::iter::repeat_n(cell, count));
    }
}

struct SliceSink<'a> {
    cells: &'a mut [PackedCell],
    written: usize,
}

impl CellSink for SliceSink<'_> {
    fn written(&self) -> usize {
        self.written
    }

    fn push(&mut self, cell: PackedCell) {
        self.cells[self.written] = cell;
        self.written += 1;
    }

    fn repeat(&mut self, cell: PackedCell, count: usize) {
        self.cells[self.written..self.written + count].fill(cell);
        self.written += count;
    }
}

fn decode_content_rows(
    reader: &mut Reader<'_>,
    target: &mut impl RowTarget,
    rows: usize,
    columns: usize,
    limits: CellLimits,
) -> Result<(), ProtocolError> {
    let mut next = 0_usize;
    loop {
        let step = reader.varint()?;
        if step == 0 {
            return Ok(());
        }
        let row = usize::try_from(step - 1)
            .ok()
            .and_then(|gap| next.checked_add(gap))
            .filter(|row| *row < rows)
            .ok_or_else(|| terminal_error("terminal row is outside the viewport"))?;
        let head = reader.varint()?;
        if head & 1 != 0 {
            return invalid("full terminal row clears its tail");
        }
        let start = usize::try_from(head >> 1)
            .ok()
            .filter(|start| *start < columns)
            .ok_or_else(|| terminal_error("terminal row starts outside the viewport"))?;
        let mut sink = SliceSink {
            cells: &mut target.row(row)[start..],
            written: 0,
        };
        decode_runs(reader, columns - start, limits, &mut sink)?;
        if sink.written == 0 {
            return invalid("full terminal row carries no cells");
        }
        next = row + 1;
    }
}

fn decode_spans(
    reader: &mut Reader<'_>,
    columns: u16,
    rows: u16,
    limits: CellLimits,
) -> Result<TerminalPatchRows, ProtocolError> {
    let mut spans = TerminalPatchSpans::new();
    let mut cells = Vec::new();
    let mut next = 0_usize;
    loop {
        let step = reader.varint()?;
        if step == 0 {
            break;
        }
        let row = usize::try_from(step - 1)
            .ok()
            .and_then(|gap| next.checked_add(gap))
            .filter(|row| *row < usize::from(rows))
            .ok_or_else(|| terminal_error("terminal patch row is outside the viewport"))?;
        let head = reader.varint()?;
        let start = usize::try_from(head >> 1)
            .ok()
            .filter(|start| *start <= usize::from(columns))
            .ok_or_else(|| terminal_error("terminal patch span starts outside the viewport"))?;
        let before = cells.len();
        decode_runs(reader, usize::from(columns) - start, limits, &mut cells)?;
        spans.push(TerminalPatchSpan {
            row: row as u16,
            start: start as u16,
            len: (cells.len() - before) as u16,
            clear: head & 1 != 0,
        });
        next = row + 1;
    }
    if spans.is_empty() {
        return invalid("terminal patch rows section is empty");
    }
    Ok(TerminalPatchRows::from_spans(spans, cells))
}

fn decode_runs(
    reader: &mut Reader<'_>,
    room: usize,
    limits: CellLimits,
    cells: &mut impl CellSink,
) -> Result<(), ProtocolError> {
    let first = cells.written();
    let mut style = 0_u16;
    loop {
        let header = reader.varint()?;
        if header == 0 {
            return Ok(());
        }
        let count = usize::try_from(header >> RUN_COUNT_SHIFT)
            .ok()
            .filter(|count| *count != 0)
            .ok_or_else(|| terminal_error("terminal run is empty"))?;
        if header & RUN_STYLE != 0 {
            style = reader.u16("style id")?;
            if usize::from(style) >= limits.styles {
                return invalid("cell references a missing style");
            }
        }
        let kind = header & RUN_KIND_MASK;
        let width = if kind == RUN_WIDE {
            count.checked_mul(2)
        } else {
            Some(count)
        };
        if width.is_none_or(|width| cells.written() - first + width > room) {
            return invalid("terminal run overflows its row");
        }
        match kind {
            RUN_TEXT => {
                for _ in 0..count {
                    cells.push(PackedCell::from_raw(reader.scalar()?, style, 0));
                }
            }
            RUN_WIDE => {
                for _ in 0..count {
                    let glyph = reader.scalar()?;
                    if glyph == 0 {
                        return invalid("wide terminal cell has no glyph");
                    }
                    cells.push(PackedCell::from_raw(glyph, style, WIDE_FLAGS));
                    cells.push(PackedCell::from_raw(0, style, SPACER_TAIL_FLAGS));
                }
            }
            RUN_REPEAT => {
                let cell = decode_raw_cell(reader, style, limits)?;
                cells.repeat(cell, count);
            }
            _ => {
                for _ in 0..count {
                    cells.push(decode_raw_cell(reader, style, limits)?);
                }
            }
        }
    }
}

fn decode_raw_cell(
    reader: &mut Reader<'_>,
    style: u16,
    limits: CellLimits,
) -> Result<PackedCell, ProtocolError> {
    let code = reader.varint()?;
    let glyph = if code & 1 != 0 {
        let index = usize::try_from(code >> 1)
            .ok()
            .filter(|index| *index < limits.graphemes)
            .ok_or_else(|| terminal_error("cell references a missing grapheme"))?;
        GRAPHEME_TABLE_BIT | index as u32
    } else {
        u32::try_from(code >> 1)
            .ok()
            .filter(|glyph| *glyph == 0 || char::from_u32(*glyph).is_some())
            .ok_or_else(|| terminal_error("cell contains an invalid Unicode scalar"))?
    };
    let flags = reader.u16("cell flags")?;
    Ok(PackedCell::from_raw(glyph, style, flags))
}

fn decode_metadata(
    reader: &mut Reader<'_>,
    fields: TerminalPatchFields,
    columns: u16,
    rows: u16,
) -> Result<DecodedMetadata, ProtocolError> {
    let mut metadata = DecodedMetadata::default();
    if fields.contains(TerminalPatchFields::CURSOR_AT) {
        let column = reader.varint()?;
        let wide_tail = column & 1 != 0;
        let column = u16::try_from(column >> 1)
            .map_err(|_| terminal_error("cursor is outside the viewport"))?;
        let row = reader.u16("cursor row")?;
        let cursor = Cursor::new(
            column,
            row,
            false,
            false,
            wide_tail,
            CursorStyle::Bar,
            Color::default(),
        );
        check_cursor(cursor, columns, rows)?;
        metadata.cursor = Some(cursor);
    }
    if fields.contains(TerminalPatchFields::SCROLLBAR) {
        metadata.scrollbar = ScrollbarState {
            total: reader.u32("scrollbar total")?,
            offset: reader.u32("scrollbar offset")?,
            len: reader.u32("scrollbar length")?,
        };
        check_scrollbar(metadata.scrollbar)?;
    }
    if fields.contains(TerminalPatchFields::OVERLAYS) {
        let count = reader.count(MAX_OVERLAY_COUNT, "overlay")?;
        reader.preflight(count, 4)?;
        let mut overlays = Vec::with_capacity(count);
        for _ in 0..count {
            let overlay = OverlaySpan::from_raw(
                reader.u16("overlay row")?,
                reader.u16("overlay start")?,
                reader.u16("overlay end")?,
                reader.u16("overlay kind")?,
            );
            check_overlay(overlay, columns, rows)?;
            overlays.push(overlay);
        }
        metadata.overlays = overlays.into();
    }
    if fields.contains(TerminalPatchFields::CURSOR) {
        let flags = reader.u8()?;
        metadata.cursor = if flags & CURSOR_PRESENT == 0 {
            if flags != 0 {
                return invalid("cursor flags contain reserved bits");
            }
            None
        } else {
            if flags >> CURSOR_STYLE_SHIFT > 3 {
                return invalid("unknown cursor style");
            }
            let style = match flags >> CURSOR_STYLE_SHIFT {
                0 => CursorStyle::Bar,
                1 => CursorStyle::Block,
                2 => CursorStyle::Underline,
                _ => CursorStyle::BlockHollow,
            };
            let cursor = Cursor::new(
                reader.u16("cursor column")?,
                reader.u16("cursor row")?,
                flags & CURSOR_VISIBLE != 0,
                flags & CURSOR_BLINKING != 0,
                flags & CURSOR_WIDE_TAIL != 0,
                style,
                Color::from_packed(reader.rgb()?),
            );
            check_cursor(cursor, columns, rows)?;
            Some(cursor)
        };
    }
    if fields.contains(TerminalPatchFields::PRESENTATION) {
        let title = reader.string(MAX_TITLE_BYTES, "title")?;
        let working_directory =
            reader.optional_string(MAX_WORKING_DIRECTORY_BYTES, "working directory")?;
        let hovered_uri = reader.optional_string(MAX_HOVER_URI_BYTES, "hovered URI")?;
        validate_working_directory(working_directory)?;
        validate_hovered_uri(hovered_uri)?;
        metadata.presentation = Arc::new(TerminalPresentation::new(
            Arc::from(title),
            working_directory.map(Arc::from),
            hovered_uri.map(Arc::from),
        ));
    }
    if fields.contains(TerminalPatchFields::COLORS) {
        metadata.foreground = Color::from_packed(reader.rgb()?);
        metadata.background = Color::from_packed(reader.rgb()?);
    }
    if fields.contains(TerminalPatchFields::MODE) {
        metadata.mode = match reader.u8()? {
            0 => TerminalMode::Live,
            1 => TerminalMode::Copy {
                position: reader.u32("copy-mode position")?,
                total: reader.u32("copy-mode total")?,
                hide_position: reader.bool()?,
            },
            2 => TerminalMode::View {
                position: reader.u32("view-mode position")?,
                total: reader.u32("view-mode total")?,
            },
            _ => return invalid("unknown terminal interaction mode"),
        };
        check_mode(metadata.mode)?;
    }
    if fields.contains(TerminalPatchFields::SEARCH) {
        let flags = reader.u8()?;
        if flags & !(SEARCH_PRESENT | SEARCH_PENDING | SEARCH_INVALID_PATTERN) != 0
            || flags & SEARCH_PRESENT == 0 && flags != 0
        {
            return invalid("unknown search status flags");
        }
        metadata.search = if flags & SEARCH_PRESENT == 0 {
            None
        } else {
            let search =
                SearchStatus::new(reader.u32("search current")?, reader.u32("search total")?)
                    .with_pending(flags & SEARCH_PENDING != 0)
                    .with_invalid_pattern(flags & SEARCH_INVALID_PATTERN != 0);
            if search.current() > search.total {
                return invalid("search status is inconsistent");
            }
            Some(search)
        };
    }
    if fields.contains(TerminalPatchFields::UNSEEN) {
        metadata.unseen_output = reader.u32("unseen output count")?;
    }
    if fields.contains(TerminalPatchFields::INPUT_MODES) {
        let flags = reader.u8()?;
        if flags & !(INPUT_KITTY_KEYBOARD | INPUT_MOUSE_TRACKING) != 0 {
            return invalid("unknown terminal input mode flags");
        }
        metadata.kitty_keyboard = flags & INPUT_KITTY_KEYBOARD != 0;
        metadata.mouse_tracking = flags & INPUT_MOUSE_TRACKING != 0;
    }
    if fields.contains(TerminalPatchFields::STATUS) {
        metadata.status = decode_status(reader)?;
    }
    if fields.contains(TerminalPatchFields::KITTY) {
        let count = reader.count(MAX_KITTY_PLACEMENTS, "kitty placement")?;
        reader.preflight(count, KITTY_PLACEMENT_WIRE_BYTES)?;
        let placements = decode_kitty_placements(reader, count)?;
        validate_kitty_placements(&placements, columns, rows, metadata.scrollbar)?;
        metadata.kitty_placements = placements;
    }
    Ok(metadata)
}

fn decode_styles(reader: &mut Reader<'_>) -> Result<Vec<PackedStyle>, ProtocolError> {
    let count = reader.count(MAX_STYLE_COUNT, "style")?;
    reader.preflight(count, 7)?;
    let mut styles = Vec::with_capacity(count);
    for _ in 0..count {
        let flags = reader.u8()?;
        if flags & !STYLE_FLAGS_MASK != 0 {
            return invalid("packed style flags contain reserved bits");
        }
        let underline_kind = flags >> STYLE_UNDERLINE_KIND_SHIFT & STYLE_UNDERLINE_KIND_MASK;
        if underline_kind > 5 {
            return invalid("style contains an invalid packed value");
        }
        let foreground = reader.rgb()?;
        let background = reader.rgb()?;
        let underline_color = if flags & STYLE_UNDERLINE_COLOR != 0 {
            reader.rgb()?
        } else {
            NO_COLOR
        };
        let attributes = if flags & STYLE_ATTRIBUTES != 0 {
            reader.u16("style attributes")?
        } else {
            0
        };
        let classes = if flags & STYLE_CLASSES != 0 {
            reader.u32("style classes")?
        } else {
            0
        };
        let style = PackedStyle::from_raw(
            foreground,
            background,
            underline_color,
            attributes,
            underline_kind,
        );
        if style.attributes() != attributes {
            return invalid("style attributes contain reserved bits");
        }
        let Some(style) = style.with_class_word(classes) else {
            return invalid("packed style colour classes are invalid");
        };
        styles.push(style);
    }
    Ok(styles)
}

fn decode_full_graphemes(reader: &mut Reader<'_>) -> Result<(Vec<u32>, Vec<u8>), ProtocolError> {
    let count = reader.count(MAX_GRAPHEME_COUNT, "grapheme")?;
    reader.preflight(count, 1)?;
    let mut offsets = Vec::with_capacity(count + 1);
    offsets.push(0_u32);
    let mut total = 0_usize;
    for _ in 0..count {
        let length = reader.count(MAX_GRAPHEME_BYTES, "grapheme length")?;
        total = total
            .checked_add(length)
            .filter(|total| *total <= MAX_GRAPHEME_BYTES)
            .ok_or_else(|| terminal_error("grapheme arena exceeds limit"))?;
        offsets.push(total as u32);
    }
    let bytes = reader.bytes(total)?;
    for window in offsets.windows(2) {
        std::str::from_utf8(&bytes[window[0] as usize..window[1] as usize])
            .map_err(|_| terminal_error("grapheme is not valid UTF-8"))?;
    }
    Ok((offsets, bytes.to_vec()))
}

fn decode_appended_graphemes(
    reader: &mut Reader<'_>,
) -> Result<(Vec<u32>, Vec<u8>), ProtocolError> {
    let count = reader.count(MAX_GRAPHEME_COUNT, "appended grapheme")?;
    reader.preflight(count, 1)?;
    let mut lengths = Vec::with_capacity(count);
    let mut total = 0_usize;
    for _ in 0..count {
        let length = reader.count(MAX_GRAPHEME_BYTES, "grapheme length")?;
        total = total
            .checked_add(length)
            .filter(|total| *total <= MAX_GRAPHEME_BYTES)
            .ok_or_else(|| terminal_error("terminal patch grapheme arena exceeds limit"))?;
        lengths.push(length as u32);
    }
    let bytes = reader.bytes(total)?;
    let mut cursor = 0_usize;
    for length in &lengths {
        let end = cursor + *length as usize;
        std::str::from_utf8(&bytes[cursor..end])
            .map_err(|_| terminal_error("grapheme is not valid UTF-8"))?;
        cursor = end;
    }
    Ok((lengths, bytes.to_vec()))
}

fn decode_status(reader: &mut Reader<'_>) -> Result<SessionStatus, ProtocolError> {
    match reader.u8()? {
        0 => Ok(SessionStatus::Starting),
        1 => Ok(SessionStatus::Running),
        2 => {
            let code = reader.u32("exit code")?;
            let signal = reader.optional_string(MAX_STATUS_BYTES, "status string")?;
            Ok(SessionStatus::exited(code, signal.map(str::to_owned)))
        }
        3 => Ok(SessionStatus::failed(
            reader.string(MAX_STATUS_BYTES, "status string")?,
        )),
        _ => invalid("unknown terminal status"),
    }
}

fn decode_kitty_placements(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Arc<[KittyPlacement]>, ProtocolError> {
    let mut placements = Vec::with_capacity(count);
    for _ in 0..count {
        let image_id = reader.le_u32()?;
        let image_generation = reader.le_u64()?;
        let layer = match reader.u8()? {
            0 => KittyLayer::BelowBg,
            1 => KittyLayer::BelowText,
            2 => KittyLayer::AboveText,
            _ => return invalid("kitty placement has an unknown paint layer"),
        };
        let has_source_rect = reader.bool()?;
        if reader.bytes(2)? != [0, 0] {
            return invalid("kitty placement reserved field is nonzero");
        }
        let viewport_col = reader.le_u32()?.cast_signed();
        let viewport_row = reader.le_u32()?.cast_signed();
        let absolute_row = reader.le_u64()?;
        let cell_offset_x = reader.le_u32()?;
        let cell_offset_y = reader.le_u32()?;
        let grid_cols = reader.le_u32()?;
        let grid_rows = reader.le_u32()?;
        let pixel_width = reader.le_u32()?;
        let pixel_height = reader.le_u32()?;
        let source = (
            reader.le_u32()?,
            reader.le_u32()?,
            reader.le_u32()?,
            reader.le_u32()?,
        );
        if !has_source_rect && source != (0, 0, 0, 0) {
            return invalid("kitty placement empty source rectangle is nonzero");
        }
        placements.push(KittyPlacement {
            image_id,
            image_generation,
            layer,
            viewport_col,
            viewport_row,
            absolute_row,
            cell_offset_x,
            cell_offset_y,
            grid_cols,
            grid_rows,
            pixel_width,
            pixel_height,
            source_rect: has_source_rect.then_some(source),
        });
    }
    Ok(placements.into())
}

fn check_cursor(cursor: Cursor, columns: u16, rows: u16) -> Result<(), ProtocolError> {
    if cursor.row() >= rows || cursor.column() >= columns {
        return invalid("cursor is outside the viewport");
    }
    Ok(())
}

fn check_scrollbar(scrollbar: ScrollbarState) -> Result<(), ProtocolError> {
    if scrollbar.offset > scrollbar.total
        || scrollbar.len > scrollbar.total
        || scrollbar.offset.saturating_add(scrollbar.len) > scrollbar.total
    {
        return invalid("scrollbar range is inconsistent");
    }
    Ok(())
}

fn check_overlay(overlay: OverlaySpan, columns: u16, rows: u16) -> Result<(), ProtocolError> {
    if overlay.row >= rows || overlay.start > overlay.end || overlay.end > columns {
        return invalid("overlay span is outside the viewport");
    }
    if overlay.kind_and_flags() & 0xff > OverlayKind::CopyCursor as u16 {
        return invalid("overlay span has an unknown kind");
    }
    Ok(())
}

fn check_mode(mode: TerminalMode) -> Result<(), ProtocolError> {
    if let TerminalMode::Copy {
        position, total, ..
    }
    | TerminalMode::View { position, total } = mode
        && (total == 0 || position == 0 || position > total)
    {
        return invalid("copy-mode position is inconsistent");
    }
    Ok(())
}

pub(crate) fn validate_hovered_uri(uri: Option<&str>) -> Result<(), ProtocolError> {
    if uri.is_some_and(|uri| {
        uri.len() > MAX_HOVER_URI_BYTES
            || uri
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
    }) {
        return invalid("hovered URI is invalid");
    }
    Ok(())
}

pub(crate) fn validate_working_directory(
    working_directory: Option<&str>,
) -> Result<(), ProtocolError> {
    if working_directory.is_some_and(|working_directory| {
        working_directory.len() > MAX_WORKING_DIRECTORY_BYTES
            || working_directory.chars().any(char::is_control)
    }) {
        return invalid("working directory is invalid");
    }
    Ok(())
}

fn valid_style(style: PackedStyle) -> bool {
    style.foreground_raw() <= 0x00ff_ffff
        && style.background_raw() <= 0x00ff_ffff
        && (style.underline_color_raw() <= 0x00ff_ffff || style.underline_color_raw() == NO_COLOR)
        && style.underline_kind_raw() <= 5
}

fn validate_kitty_placements(
    placements: &[KittyPlacement],
    columns: u16,
    rows: u16,
    scrollbar: ScrollbarState,
) -> Result<(), ProtocolError> {
    if placements.len() > MAX_KITTY_PLACEMENTS {
        return invalid("kitty placement count exceeds its wire limit");
    }
    for placement in placements {
        if placement.image_id == 0
            || placement.image_generation == 0
            || placement.grid_cols == 0
            || placement.grid_rows == 0
            || placement.pixel_width == 0
            || placement.pixel_height == 0
        {
            return invalid("kitty placement has empty image or geometry metadata");
        }
        let right = i64::from(placement.viewport_col) + i64::from(placement.grid_cols);
        let bottom = i64::from(placement.viewport_row) + i64::from(placement.grid_rows);
        if right <= 0
            || i64::from(placement.viewport_col) >= i64::from(columns)
            || bottom <= 0
            || i64::from(placement.viewport_row) >= i64::from(rows)
        {
            return invalid("kitty placement is outside the viewport");
        }
        let expected_absolute = if placement.viewport_row < 0 {
            u64::from(scrollbar.offset)
                .saturating_sub(u64::from(placement.viewport_row.unsigned_abs()))
        } else {
            u64::from(scrollbar.offset)
                .saturating_add(u64::from(placement.viewport_row.cast_unsigned()))
        };
        if placement.absolute_row != expected_absolute {
            return invalid("kitty placement absolute row is inconsistent");
        }
        if let Some((x, y, width, height)) = placement.source_rect
            && (width == 0
                || height == 0
                || x.checked_add(width).is_none()
                || y.checked_add(height).is_none())
        {
            return invalid("kitty placement source rectangle is invalid");
        }
    }
    Ok(())
}

fn push_varint(output: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        output.push(value as u8 | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn push_zigzag(output: &mut Vec<u8>, value: u64) {
    let signed = value.cast_signed();
    push_varint(output, (signed << 1 ^ signed >> 63).cast_unsigned());
}

fn push_rgb(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes()[..3]);
}

fn push_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    push_varint(output, bytes.len() as u64);
    output.extend_from_slice(bytes);
}

fn push_optional_bytes(output: &mut Vec<u8>, bytes: Option<&[u8]>) {
    match bytes {
        None => output.push(0),
        Some(bytes) => {
            push_varint(output, bytes.len() as u64 + 1);
            output.extend_from_slice(bytes);
        }
    }
}

fn terminal_error(message: &str) -> ProtocolError {
    ProtocolError::InvalidTerminal(message.to_owned())
}

fn invalid<T>(message: &str) -> Result<T, ProtocolError> {
    Err(terminal_error(message))
}

pub(crate) struct Reader<'a> {
    remaining: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { remaining: bytes }
    }

    pub(crate) fn finish(&self, message: &str) -> Result<(), ProtocolError> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            invalid(message)
        }
    }

    fn preflight(&self, count: usize, minimum: usize) -> Result<(), ProtocolError> {
        if count.saturating_mul(minimum) > self.remaining.len() {
            return Err(ProtocolError::Truncated);
        }
        Ok(())
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], ProtocolError> {
        let Some((head, tail)) = self.remaining.split_at_checked(len) else {
            return Err(ProtocolError::Truncated);
        };
        self.remaining = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.bytes(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, ProtocolError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => invalid("invalid boolean value"),
        }
    }

    fn le_u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?
                .try_into()
                .map_err(|_| ProtocolError::Truncated)?,
        ))
    }

    fn le_u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(
            self.bytes(8)?
                .try_into()
                .map_err(|_| ProtocolError::Truncated)?,
        ))
    }

    fn rgb(&mut self) -> Result<u32, ProtocolError> {
        let bytes = self.bytes(3)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]))
    }

    pub(crate) fn varint(&mut self) -> Result<u64, ProtocolError> {
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            let bits = u64::from(byte & 0x7f);
            if shift == 63 && bits > 1 {
                return invalid("varint overflows u64");
            }
            value |= bits << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        invalid("varint overflows u64")
    }

    fn zigzag(&mut self) -> Result<u64, ProtocolError> {
        let value = self.varint()?;
        Ok(value >> 1 ^ (value & 1).wrapping_neg())
    }

    fn u16(&mut self, name: &str) -> Result<u16, ProtocolError> {
        u16::try_from(self.varint()?)
            .map_err(|_| ProtocolError::InvalidTerminal(format!("{name} exceeds u16")))
    }

    fn u32(&mut self, name: &str) -> Result<u32, ProtocolError> {
        u32::try_from(self.varint()?)
            .map_err(|_| ProtocolError::InvalidTerminal(format!("{name} exceeds u32")))
    }

    fn count(&mut self, max: usize, name: &str) -> Result<usize, ProtocolError> {
        let value = self.varint()?;
        usize::try_from(value)
            .ok()
            .filter(|value| *value <= max)
            .ok_or_else(|| {
                ProtocolError::InvalidTerminal(format!("{name} count {value} exceeds limit {max}"))
            })
    }

    fn fields(&mut self) -> Result<TerminalPatchFields, ProtocolError> {
        u16::try_from(self.varint()?)
            .ok()
            .and_then(TerminalPatchFields::from_bits)
            .ok_or_else(|| terminal_error("terminal frame carries unknown fields"))
    }

    fn scalar(&mut self) -> Result<u32, ProtocolError> {
        let first = *self.remaining.first().ok_or(ProtocolError::Truncated)?;
        if first < 0x80 {
            self.remaining = &self.remaining[1..];
            return Ok(u32::from(first));
        }
        let len = match first {
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return invalid("terminal glyph is not valid UTF-8"),
        };
        let bytes = self.bytes(len)?;
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| text.chars().next())
            .map(u32::from)
            .ok_or_else(|| terminal_error("terminal glyph is not valid UTF-8"))
    }

    fn string(&mut self, max: usize, name: &str) -> Result<&'a str, ProtocolError> {
        let len = self.count(max, name)?;
        std::str::from_utf8(self.bytes(len)?)
            .map_err(|_| ProtocolError::InvalidTerminal(format!("{name} is not valid UTF-8")))
    }

    fn optional_string(
        &mut self,
        max: usize,
        name: &str,
    ) -> Result<Option<&'a str>, ProtocolError> {
        let len = self.count(max.saturating_add(1), name)?;
        if len == 0 {
            return Ok(None);
        }
        std::str::from_utf8(self.bytes(len - 1)?)
            .map(Some)
            .map_err(|_| ProtocolError::InvalidTerminal(format!("{name} is not valid UTF-8")))
    }
}

struct ViewportBytes<'a>(&'a TerminalViewport);

impl serde::Serialize for ViewportBytes<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut bytes = Vec::new();
        encode_viewport_body(&mut bytes, self.0).map_err(serde::ser::Error::custom)?;
        serializer.serialize_bytes(&bytes)
    }
}

struct OwnedViewportBytes(TerminalViewport);

impl<'de> serde::Deserialize<'de> for OwnedViewportBytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_bytes(ViewportVisitor).map(Self)
    }
}

struct ViewportVisitor;

impl<'de> serde::de::Visitor<'de> for ViewportVisitor {
    type Value = TerminalViewport;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a packed terminal viewport")
    }

    fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
        let mut reader = Reader::new(bytes);
        let viewport = decode_viewport_body(&mut reader).map_err(E::custom)?;
        reader
            .finish("packed terminal viewport has trailing bytes")
            .map_err(E::custom)?;
        Ok(viewport)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(byte) = seq.next_element::<u8>()? {
            bytes.push(byte);
        }
        self.visit_bytes(&bytes)
    }
}

pub(crate) mod viewport_bytes {
    use zz_terminal::TerminalViewport;

    pub(crate) fn serialize<S: serde::Serializer>(
        viewport: &TerminalViewport,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&super::ViewportBytes(viewport), serializer)
    }

    pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<TerminalViewport, D::Error> {
        deserializer.deserialize_bytes(super::ViewportVisitor)
    }
}

pub(crate) mod optional_viewport_bytes {
    use zz_terminal::TerminalViewport;

    #[expect(
        clippy::ref_option,
        reason = "serde passes a `with` field by reference"
    )]
    pub(crate) fn serialize<S: serde::Serializer>(
        viewport: &Option<TerminalViewport>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match viewport {
            Some(viewport) => serializer.serialize_some(&super::ViewportBytes(viewport)),
            None => serializer.serialize_none(),
        }
    }

    pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<TerminalViewport>, D::Error> {
        <Option<super::OwnedViewportBytes> as serde::Deserialize>::deserialize(deserializer)
            .map(|viewport| viewport.map(|viewport| viewport.0))
    }
}

#[cfg(test)]
#[path = "pane_frame_tests.rs"]
mod tests;
