use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use libghostty_vt::{
    Terminal,
    screen::{RowSemanticPrompt, Screen},
    terminal::PointCoordinate,
};

use parking_lot::Mutex;

use super::copy_grid::{CopyGrid, CopyRow};

use crate::{CellWidth, Color, PackedCell, PackedStyle, TerminalDictionary};

use super::{HistorySearchSnapshot, SelectionMode, WorkerError, color, reported_working_directory};

const ROW_WRAPPED: u8 = 1 << 0;
const ROW_WRAP_CONTINUATION: u8 = 1 << 1;
const SEMANTIC_OUTPUT: u8 = 0;
const SEMANTIC_INPUT: u8 = 1;
const SEMANTIC_PROMPT: u8 = 2;

static NEXT_MODE_REVISION_ID: AtomicU64 = AtomicU64::new(1);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ModeRowMeta {
    flags: u8,
    prompt: u8,
    reserved: u16,
}

impl ModeRowMeta {
    pub(super) fn new(wrapped: bool, continuation: bool, prompt: RowSemanticPrompt) -> Self {
        Self {
            flags: (u8::from(wrapped) * ROW_WRAPPED)
                | (u8::from(continuation) * ROW_WRAP_CONTINUATION),
            prompt: match prompt {
                RowSemanticPrompt::None => 0,
                RowSemanticPrompt::Prompt => 1,
                RowSemanticPrompt::Continuation => 2,
            },
            reserved: 0,
        }
    }

    pub(super) const fn wrapped(self) -> bool {
        self.flags & ROW_WRAPPED != 0
    }

    pub(super) const fn continuation(self) -> bool {
        self.flags & ROW_WRAP_CONTINUATION != 0
    }

    pub(super) const fn prompt(self) -> bool {
        self.prompt == 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ModeSelection {
    pub(super) anchor: PointCoordinate,
    pub(super) focus: PointCoordinate,
    pub(super) mode: SelectionMode,
    pub(super) rectangle: bool,
}

#[derive(Debug)]
pub(super) struct ModeRevision {
    pub(super) id: u64,
    pub(super) screen: Screen,
    pub(super) columns: u16,
    pub(super) viewport_rows: u16,
    pub(super) foreground: Color,
    pub(super) background: Color,
    palette: Box<[Color; 256]>,
    pub(super) title: Arc<str>,
    pub(super) working_directory: Option<Arc<str>>,
    pub(super) search: Arc<HistorySearchSnapshot>,
    grid: Mutex<CopyGrid>,
    total: u32,
    output_rows: std::sync::OnceLock<Vec<u64>>,
}

struct ModeReaderRow {
    index: u32,
    row: Arc<CopyRow>,
    dictionary: Arc<TerminalDictionary>,
}

pub(super) struct ModeRevisionReader<'revision> {
    revision: &'revision ModeRevision,
    cached: RefCell<Option<ModeReaderRow>>,
}

impl ModeRevisionReader<'_> {
    pub(super) fn columns(&self) -> u16 {
        self.revision.columns
    }

    pub(super) fn total_rows(&self) -> u32 {
        self.revision.total_rows()
    }

    fn with_row<T>(&self, row: u32, read: impl FnOnce(&CopyRow, &TerminalDictionary) -> T) -> T {
        let index = row.min(self.revision.total.saturating_sub(1));
        let mut cached = self.cached.borrow_mut();
        if cached.as_ref().is_none_or(|cached| cached.index != index) {
            let mut grid = self.revision.grid.lock();
            let row = grid.row(index).expect("frozen row");
            let dictionary = grid.dictionary();
            *cached = Some(ModeReaderRow {
                index,
                row,
                dictionary,
            });
        }
        let cached = cached.as_ref().expect("captured row");
        read(&cached.row, &cached.dictionary)
    }

    fn with_cell<T>(
        &self,
        point: PointCoordinate,
        read: impl FnOnce(PackedCell, &TerminalDictionary) -> T,
    ) -> T {
        let point = self.revision.clamp_point(point);
        self.with_row(point.y, |row, dictionary| {
            read(row.cells[usize::from(point.x)], dictionary)
        })
    }

    pub(super) fn row_meta(&self, row: u32) -> ModeRowMeta {
        self.with_row(row, |row, _| row.meta)
    }

    pub(super) fn cell(&self, point: PointCoordinate) -> PackedCell {
        self.with_cell(point, |cell, _| cell)
    }

    pub(super) fn first_char(&self, point: PointCoordinate) -> Option<char> {
        self.with_cell(point, cell_first_char)
    }

    pub(super) fn push_text(&self, point: PointCoordinate, output: &mut String) {
        self.with_cell(point, |cell, dictionary| {
            if let Some(text) = emitting_cell_text(cell, dictionary) {
                text.push(output);
            }
        });
    }

    fn with_cells<T>(
        &self,
        row: u32,
        read: impl FnOnce(&[PackedCell], ModeRowMeta, &TerminalDictionary) -> T,
    ) -> T {
        self.with_row(row, |row, dictionary| {
            read(&row.cells, row.meta, dictionary)
        })
    }
}

impl ModeRevision {
    pub(super) fn reader(&self) -> ModeRevisionReader<'_> {
        ModeRevisionReader {
            revision: self,
            cached: RefCell::new(None),
        }
    }

    pub(super) fn capture(terminal: &Terminal<'_, '_>) -> Result<Arc<Self>, WorkerError> {
        let snapshot = terminal.clone_screen()?;
        Self::from_snapshot(snapshot)
    }

    fn from_snapshot(
        snapshot: libghostty_vt::terminal::ScreenSnapshot,
    ) -> Result<Arc<Self>, WorkerError> {
        let screen = snapshot.active_screen()?;
        let columns = snapshot.cols()?.max(1);
        let viewport_rows = snapshot.rows()?.max(1);
        let total = u32::try_from(snapshot.total_rows()?)
            .map_err(|_| WorkerError::ViewportMetadataTooLarge)?
            .max(1);
        let foreground = color(
            snapshot
                .fg_color()?
                .unwrap_or(libghostty_vt::style::RgbColor {
                    r: 255,
                    g: 255,
                    b: 255,
                }),
        );
        let background = color(
            snapshot
                .bg_color()?
                .unwrap_or(libghostty_vt::style::RgbColor { r: 0, g: 0, b: 0 }),
        );
        let raw_palette = snapshot.color_palette()?.0;
        let palette = Box::new(raw_palette.map(color));
        let title = Arc::from(snapshot.title().unwrap_or("zz"));
        let working_directory = snapshot
            .pwd()
            .ok()
            .and_then(reported_working_directory)
            .map(Arc::from);
        let grid = CopyGrid::new(snapshot, foreground, background, &raw_palette, columns);
        let search = Arc::new(HistorySearchSnapshot {
            columns,
            terminal: Arc::clone(&grid.terminal),
            #[cfg(test)]
            search_gate: Mutex::new(()),
            total_rows: total,
        });
        Ok(Arc::new(Self {
            id: NEXT_MODE_REVISION_ID.fetch_add(1, Ordering::Relaxed).max(1),
            screen,
            columns,
            viewport_rows,
            foreground,
            background,
            palette,
            title,
            working_directory,
            search,
            grid: Mutex::new(grid),
            total,
            output_rows: std::sync::OnceLock::new(),
        }))
    }

    pub(super) fn stamp_output_rows(&self, rows: impl FnOnce() -> Vec<u64>) {
        let _ = self.output_rows.get_or_init(rows);
    }

    pub(super) fn output_rows(&self) -> &[u64] {
        self.output_rows.get().map_or(&[], Vec::as_slice)
    }

    pub(super) fn row_has_hyperlink(&self, row: u32) -> bool {
        let terminal = Arc::clone(&self.grid.lock().terminal);
        let snapshot = terminal.lock();
        libghostty_vt::terminal::GridRead::grid_ref(
            &*snapshot,
            libghostty_vt::terminal::Point::Screen(PointCoordinate { x: 0, y: row }),
        )
        .and_then(|grid| grid.row())
        .and_then(libghostty_vt::screen::Row::has_hyperlink)
        .unwrap_or(false)
    }

    pub(super) fn viewport_cells(&self, offset: u32) -> Arc<[PackedCell]> {
        let mut grid = self.grid.lock();
        grid.begin_viewport();
        let columns = usize::from(self.columns);
        let end = offset.saturating_add(u32::from(self.viewport_rows));
        let len = columns * usize::try_from(end - offset).expect("viewport row count");
        let mut cells: Arc<[PackedCell]> = std::iter::repeat_n(PackedCell::EMPTY, len).collect();
        let output = Arc::get_mut(&mut cells).expect("new viewport cells");
        for (index, row) in (offset..end).enumerate() {
            if row < self.total {
                let source = grid.row(row).expect("frozen row");
                let count = source.cells.len().min(columns);
                output[index * columns..index * columns + count]
                    .copy_from_slice(&source.cells[..count]);
            }
        }
        grid.end_viewport();
        cells
    }

    pub(super) fn dictionary_generation(&self) -> u32 {
        self.grid.lock().generation()
    }

    pub(super) fn shared_dictionary(&self) -> Arc<TerminalDictionary> {
        self.grid.lock().dictionary()
    }

    pub(super) fn with_appearance(
        &self,
        terminal: &mut Terminal<'_, '_>,
    ) -> Result<Arc<Self>, WorkerError> {
        let snapshot = self.grid.lock().terminal.lock().clone_screen()?;
        let mut snapshot = snapshot;
        snapshot.set_colors_from(terminal)?;
        let revision = Self::from_snapshot(snapshot)?;
        revision.stamp_output_rows(|| self.output_rows().to_vec());
        Ok(revision)
    }

    pub(super) fn resized(
        self: &Arc<Self>,
        columns: u16,
        rows: u16,
        cursor: PointCoordinate,
    ) -> Result<(Arc<Self>, PointCoordinate), WorkerError> {
        let mut snapshot = self.grid.lock().terminal.lock().clone_screen()?;
        let marks = logical_marks(&snapshot, self.output_rows());
        let cursor = snapshot
            .resize_anchored(columns, rows, self.clamp_point(cursor))?
            .unwrap_or(cursor);
        let output_rows = place_logical_marks(&snapshot, &marks);
        let revision = Self::from_snapshot(snapshot)?;
        revision.stamp_output_rows(|| output_rows);
        Ok((revision, cursor))
    }

    pub(super) fn matches_terminal_appearance(
        &self,
        terminal: &Terminal<'_, '_>,
    ) -> Result<bool, WorkerError> {
        let foreground = terminal.fg_color()?.map(color);
        let background = terminal.bg_color()?.map(color);
        let palette = terminal.color_palette()?.0.map(color);
        Ok(foreground == Some(self.foreground)
            && background == Some(self.background)
            && palette == *self.palette)
    }

    pub(super) fn total_rows(&self) -> u32 {
        self.total
    }

    pub(super) fn maximum_offset(&self) -> u32 {
        self.total_rows()
            .saturating_sub(u32::from(self.viewport_rows))
    }

    pub(super) fn clamp_point(&self, mut point: PointCoordinate) -> PointCoordinate {
        point.x = point.x.min(self.columns.saturating_sub(1));
        point.y = point.y.min(self.total_rows().saturating_sub(1));
        point
    }

    pub(super) fn cell(&self, point: PointCoordinate) -> PackedCell {
        let point = self.clamp_point(point);
        self.grid.lock().row(point.y).expect("frozen row").cells[usize::from(point.x)]
    }

    pub(super) fn first_char(&self, point: PointCoordinate) -> Option<char> {
        let cell = self.cell(point);
        if cell.glyph() & crate::GRAPHEME_TABLE_BIT == 0 {
            return char::from_u32(cell.glyph()).filter(|character| *character != '\0');
        }
        let index = usize::try_from(cell.glyph() & !crate::GRAPHEME_TABLE_BIT).ok()?;
        let dictionary = self.shared_dictionary();
        let start = usize::try_from(*dictionary.grapheme_offsets.get(index)?).ok()?;
        let end = usize::try_from(*dictionary.grapheme_offsets.get(index + 1)?).ok()?;
        std::str::from_utf8(dictionary.grapheme_bytes.get(start..end)?)
            .ok()?
            .chars()
            .next()
    }

    pub(super) fn cell_matches_text(&self, point: PointCoordinate, target: &str) -> bool {
        if target.is_empty() {
            return false;
        }
        let cell = self.cell(point);
        if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
            return false;
        }
        let glyph = cell.glyph();
        if glyph & crate::GRAPHEME_TABLE_BIT == 0 {
            let mut characters = target.chars();
            return char::from_u32(glyph).is_some_and(|glyph| characters.next() == Some(glyph))
                && characters.next().is_none();
        }
        let Ok(index) = usize::try_from(glyph & !crate::GRAPHEME_TABLE_BIT) else {
            return false;
        };
        let dictionary = self.shared_dictionary();
        let Some(start) = dictionary
            .grapheme_offsets
            .get(index)
            .and_then(|offset| usize::try_from(*offset).ok())
        else {
            return false;
        };
        let Some(end) = dictionary
            .grapheme_offsets
            .get(index + 1)
            .and_then(|offset| usize::try_from(*offset).ok())
        else {
            return false;
        };
        dictionary.grapheme_bytes.get(start..end) == Some(target.as_bytes())
    }

    pub(super) fn push_cell_text(&self, cell: PackedCell, output: &mut String) {
        let glyph = cell.glyph();
        if glyph == 0 || matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
            return;
        }
        if glyph & crate::GRAPHEME_TABLE_BIT == 0 {
            if let Some(character) = char::from_u32(glyph) {
                output.push(character);
            }
            return;
        }
        let Ok(index) = usize::try_from(glyph & !crate::GRAPHEME_TABLE_BIT) else {
            return;
        };
        let dictionary = self.shared_dictionary();
        let Some(start) = dictionary
            .grapheme_offsets
            .get(index)
            .and_then(|offset| usize::try_from(*offset).ok())
        else {
            return;
        };
        let Some(end) = dictionary
            .grapheme_offsets
            .get(index + 1)
            .and_then(|offset| usize::try_from(*offset).ok())
        else {
            return;
        };
        if let Some(text) = dictionary
            .grapheme_bytes
            .get(start..end)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        {
            output.push_str(text);
        }
    }

    pub(super) fn semantic(&self, point: PointCoordinate) -> u8 {
        let point = self.clamp_point(point);
        self.grid.lock().row(point.y).expect("frozen row").semantics[usize::from(point.x)]
    }

    pub(super) fn is_input(&self, point: PointCoordinate) -> bool {
        self.semantic(point) == SEMANTIC_INPUT
    }

    pub(super) fn is_output(&self, point: PointCoordinate) -> bool {
        self.semantic(point) == SEMANTIC_OUTPUT
    }

    pub(super) fn is_prompt(&self, point: PointCoordinate) -> bool {
        self.semantic(point) == SEMANTIC_PROMPT
    }

    pub(super) fn row(&self, row: u32) -> ModeRowMeta {
        self.grid
            .lock()
            .row(row.min(self.total.saturating_sub(1)))
            .expect("frozen row")
            .meta
    }

    /// `window_copy_get_selection`: the last row is trimmed to its own length
    /// and then, under emacs, stops one cell short of the bottom-right cell
    /// the cursor stands on. A dragged rectangle drops that column on every
    /// row instead, and only when the selection started left of the cursor.
    pub(super) fn format_selection(&self, selection: ModeSelection, mode_keys_vi: bool) -> String {
        let mut output = String::new();
        let (start, end) = ordered_points(selection.anchor, selection.focus);
        let (left, right) = if selection.rectangle {
            (
                selection.anchor.x.min(selection.focus.x),
                selection.anchor.x.max(selection.focus.x),
            )
        } else {
            (0, self.columns.saturating_sub(1))
        };
        let reader = self.reader();
        for row in start.y..=end.y {
            reader.with_cells(row, |cells, meta, dictionary| {
                let row_start = if selection.rectangle {
                    left
                } else if row == start.y {
                    start.x
                } else {
                    0
                };
                let row_end = if selection.rectangle {
                    right
                } else if row == end.y {
                    end.x
                } else {
                    self.columns.saturating_sub(1)
                };
                let line_end = if meta.wrapped() {
                    self.columns
                } else {
                    cells
                        .iter()
                        .rposition(|cell| {
                            cell_first_char(*cell, dictionary)
                                .is_some_and(|character| character != ' ')
                        })
                        .map_or(0, |column| u16::try_from(column + 1).expect("column"))
                };
                let drops_focus_cell = !mode_keys_vi
                    && if selection.rectangle {
                        selection.anchor.x < selection.focus.x
                    } else {
                        row == end.y
                    };
                let selected_end = if drops_focus_cell {
                    row_end.min(line_end)
                } else {
                    row_end.saturating_add(1).min(line_end)
                };
                if row_start < selected_end {
                    for cell in &cells[usize::from(row_start)..usize::from(selected_end)] {
                        if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
                            continue;
                        }
                        if cell.glyph() == 0 {
                            output.push(' ');
                        } else if let Some(text) = cell_text(*cell, dictionary) {
                            text.push(&mut output);
                        }
                    }
                }
                let has_line_break = !meta.wrapped() || selected_end < line_end;
                let keeps_final_break = mode_keys_vi && row == end.y && {
                    let line_length = cells
                        .iter()
                        .rposition(|cell| {
                            cell_first_char(*cell, dictionary)
                                .is_some_and(|character| !character.is_whitespace())
                        })
                        .map_or(0, |column| u16::try_from(column + 1).expect("column"));
                    let last_exclusive = if selection.rectangle {
                        right.saturating_add(1)
                    } else {
                        end.x.min(line_length).saturating_add(1)
                    };
                    last_exclusive > line_length
                };
                if has_line_break
                    && (row < end.y
                        || (row == end.y
                            && (selection.mode == SelectionMode::Line || keeps_final_break)))
                {
                    output.push('\n');
                }
            });
        }
        output
    }

    pub(super) fn capture_rows(
        &self,
        start: u32,
        end: u32,
        join_wrapped: bool,
        preserve_trailing: bool,
        escape_sequences: bool,
    ) -> String {
        if escape_sequences {
            return self.capture_rows_vt(start, end, join_wrapped, preserve_trailing);
        }
        let mut output = String::new();
        let reader = self.reader();
        for row in start..=end {
            reader.with_cells(row, |cells, meta, dictionary| {
                let line_start = output.len();
                for cell in cells {
                    if let Some(text) = emitting_cell_text(*cell, dictionary) {
                        text.push(&mut output);
                    }
                }
                if !preserve_trailing {
                    let trimmed_length = output[line_start..].trim_end().len();
                    output.truncate(line_start + trimmed_length);
                }
                if row < end && !(join_wrapped && meta.wrapped()) {
                    output.push('\n');
                }
            });
        }
        output
    }

    fn capture_rows_vt(
        &self,
        start: u32,
        end: u32,
        join_wrapped: bool,
        preserve_trailing: bool,
    ) -> String {
        let mut output = String::new();
        let reader = self.reader();
        for row in start..=end {
            reader.with_cells(row, |cells, meta, dictionary| {
                let mut active_style = None;
                let last_column = if preserve_trailing {
                    cells.len().checked_sub(1)
                } else {
                    cells.iter().rposition(|cell| {
                        cell_first_char(*cell, dictionary)
                            .is_some_and(|character| !character.is_whitespace())
                    })
                };
                for cell in cells.iter().take(last_column.unwrap_or(0) + 1) {
                    let Some(text) = emitting_cell_text(*cell, dictionary) else {
                        continue;
                    };
                    if active_style != Some(cell.style_id()) {
                        if let Some(style) = dictionary.styles.get(usize::from(cell.style_id())) {
                            push_sgr(&mut output, *style);
                        }
                        active_style = Some(cell.style_id());
                    }
                    text.push(&mut output);
                }
                if active_style.is_some() {
                    output.push_str("\x1b[0m");
                }
                if row < end && !(join_wrapped && meta.wrapped()) {
                    output.push('\n');
                }
            });
        }
        output
    }
}

enum ModeCellText<'text> {
    Scalar(char),
    Grapheme(&'text str),
}

impl ModeCellText<'_> {
    fn push(self, output: &mut String) {
        match self {
            Self::Scalar(character) => output.push(character),
            Self::Grapheme(text) => output.push_str(text),
        }
    }
}

fn cell_text(cell: PackedCell, dictionary: &TerminalDictionary) -> Option<ModeCellText<'_>> {
    let glyph = cell.glyph();
    if glyph == 0 {
        return None;
    }
    if glyph & crate::GRAPHEME_TABLE_BIT == 0 {
        return char::from_u32(glyph).map(ModeCellText::Scalar);
    }
    let index = usize::try_from(glyph & !crate::GRAPHEME_TABLE_BIT).ok()?;
    let offsets = dictionary.grapheme_offsets.get(index..=index + 1)?;
    let start = usize::try_from(offsets[0]).ok()?;
    let end = usize::try_from(offsets[1]).ok()?;
    let text = std::str::from_utf8(dictionary.grapheme_bytes.get(start..end)?).ok()?;
    (!text.is_empty()).then_some(ModeCellText::Grapheme(text))
}

fn emitting_cell_text(
    cell: PackedCell,
    dictionary: &TerminalDictionary,
) -> Option<ModeCellText<'_>> {
    if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
        return None;
    }
    cell_text(cell, dictionary)
}

fn cell_first_char(cell: PackedCell, dictionary: &TerminalDictionary) -> Option<char> {
    match cell_text(cell, dictionary)? {
        ModeCellText::Scalar(character) => Some(character),
        ModeCellText::Grapheme(text) => text.chars().next(),
    }
}

pub(super) fn push_sgr(output: &mut String, style: PackedStyle) {
    use std::fmt::Write as _;

    output.push_str("\x1b[0");
    if style.bold() {
        output.push_str(";1");
    }
    if style.faint() {
        output.push_str(";2");
    }
    if style.italic() {
        output.push_str(";3");
    }
    match style.underline() {
        crate::UnderlineStyle::None => {}
        crate::UnderlineStyle::Single => output.push_str(";4"),
        crate::UnderlineStyle::Double => output.push_str(";21"),
        crate::UnderlineStyle::Curly => output.push_str(";4:3"),
        crate::UnderlineStyle::Dotted => output.push_str(";4:4"),
        crate::UnderlineStyle::Dashed => output.push_str(";4:5"),
    }
    if style.blink() {
        output.push_str(";5");
    }
    if style.invisible() {
        output.push_str(";8");
    }
    if style.strikethrough() {
        output.push_str(";9");
    }
    if style.overline() {
        output.push_str(";53");
    }
    let foreground = style.foreground();
    let background = style.background();
    let _ = write!(
        output,
        ";38;2;{};{};{};48;2;{};{};{}",
        foreground.r, foreground.g, foreground.b, background.r, background.g, background.b,
    );
    if let Some(underline) = style.underline_color() {
        let _ = write!(
            output,
            ";58;2;{};{};{}",
            underline.r, underline.g, underline.b
        );
    }
    output.push('m');
}

fn ordered_points(
    first: PointCoordinate,
    second: PointCoordinate,
) -> (PointCoordinate, PointCoordinate) {
    if (first.y, first.x) <= (second.y, second.x) {
        (first, second)
    } else {
        (second, first)
    }
}

fn continuation_rows(snapshot: &libghostty_vt::terminal::ScreenSnapshot) -> Vec<bool> {
    let total = u32::try_from(snapshot.total_rows().unwrap_or(0)).unwrap_or(u32::MAX);
    (0..total)
        .map(|row| {
            libghostty_vt::terminal::GridRead::grid_ref(
                snapshot,
                libghostty_vt::terminal::Point::Screen(PointCoordinate { x: 0, y: row }),
            )
            .and_then(|grid| grid.row())
            .and_then(libghostty_vt::screen::Row::is_wrap_continuation)
            .unwrap_or(false)
        })
        .collect()
}

fn logical_marks(
    snapshot: &libghostty_vt::terminal::ScreenSnapshot,
    rows: &[u64],
) -> Vec<(usize, usize)> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut line = 0;
    let mut offset = 0;
    let mut marks = Vec::with_capacity(rows.len());
    for (row, continuation) in continuation_rows(snapshot).into_iter().enumerate() {
        if row > 0 && !continuation {
            line += 1;
            offset = 0;
        } else if row > 0 {
            offset += 1;
        }
        if rows.binary_search(&(row as u64)).is_ok() {
            marks.push((line, offset));
        }
    }
    marks
}

fn place_logical_marks(
    snapshot: &libghostty_vt::terminal::ScreenSnapshot,
    marks: &[(usize, usize)],
) -> Vec<u64> {
    if marks.is_empty() {
        return Vec::new();
    }
    let mut starts = Vec::new();
    let continuations = continuation_rows(snapshot);
    for (row, continuation) in continuations.iter().enumerate() {
        if row == 0 || !continuation {
            starts.push(row);
        }
    }
    let mut rows = marks
        .iter()
        .filter_map(|(line, offset)| {
            let start = *starts.get(*line)?;
            let end = starts.get(line + 1).copied().unwrap_or(continuations.len());
            Some((start + offset).min(end.saturating_sub(1)) as u64)
        })
        .collect::<Vec<_>>();
    rows.dedup();
    rows
}
