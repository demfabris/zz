use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use libghostty_vt::{
    RenderState, Terminal,
    render::{CellIterator, RowIterator},
    screen::{CellSemanticContent, CellWide, RowSemanticPrompt, Screen},
    terminal::{PointCoordinate, ScrollViewport},
};

use crate::{CellWidth, Color, PackedCell, PackedStyle, TerminalDictionary};

use super::{
    HistorySearchRow, HistorySearchSnapshot, MAX_SEARCH_SNAPSHOT_BYTES, SearchCellOffset,
    SelectionMode, ViewportDictionary, WorkerError, color, reported_working_directory,
    resolve_style_color, style_attributes, underline_style,
};

const MAX_MODE_REVISION_BYTES: usize = 128 * 1024 * 1024;
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
    fn new(wrapped: bool, continuation: bool, prompt: RowSemanticPrompt) -> Self {
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
    pub(super) cells: Vec<PackedCell>,
    pub(super) dictionary: Arc<TerminalDictionary>,
    pub(super) rows: Vec<ModeRowMeta>,
    semantics: Vec<u8>,
    pub(super) search: Arc<HistorySearchSnapshot>,
}

impl ModeRevision {
    pub(super) fn capture(terminal: &mut Terminal<'_, '_>) -> Result<Arc<Self>, WorkerError> {
        let screen = terminal.active_screen()?;
        let columns = terminal.cols()?.max(1);
        let viewport_rows = terminal.rows()?.max(1);
        let total_rows = terminal.total_rows()?.max(1);
        let cell_count = total_rows
            .checked_mul(usize::from(columns))
            .ok_or(WorkerError::ModeRevisionTooLarge)?;
        let base_bytes = cell_count
            .checked_mul(std::mem::size_of::<PackedCell>() + std::mem::size_of::<u8>())
            .and_then(|bytes| bytes.checked_add(total_rows * std::mem::size_of::<ModeRowMeta>()))
            .ok_or(WorkerError::ModeRevisionTooLarge)?;
        if base_bytes > MAX_MODE_REVISION_BYTES {
            return Err(WorkerError::ModeRevisionTooLarge);
        }

        let saved_offset = terminal.scrollbar()?.offset;
        let title: Arc<str> = Arc::from(terminal.title().unwrap_or("zz"));
        let working_directory = terminal
            .pwd()
            .ok()
            .and_then(reported_working_directory)
            .map(Arc::from);
        let mut render_state = RenderState::new()?;
        let mut row_iterator = RowIterator::new()?;
        let mut cell_iterator = CellIterator::new()?;
        let mut dictionary = ViewportDictionary::default();
        let mut cells = vec![PackedCell::EMPTY; cell_count];
        let mut semantics = vec![SEMANTIC_OUTPUT; cell_count];
        let mut row_meta = vec![ModeRowMeta::default(); total_rows];
        let mut search_text = String::with_capacity(cell_count.min(MAX_SEARCH_SNAPSHOT_BYTES / 2));
        let mut search_rows = Vec::with_capacity(total_rows);
        let mut search_offsets = Vec::with_capacity(cell_count.min(MAX_SEARCH_SNAPSHOT_BYTES / 16));
        let mut grapheme_scratch = String::with_capacity(8);
        let mut captured_until = 0_usize;
        let mut foreground;
        let mut background;
        let mut palette = Box::new([Color::rgb(0, 0, 0); 256]);

        terminal.scroll_viewport(ScrollViewport::Top);
        loop {
            let scrollbar = terminal.scrollbar()?;
            let page_offset = usize::try_from(scrollbar.offset).unwrap_or(usize::MAX);
            let maximum = scrollbar.total.saturating_sub(scrollbar.len);
            let snapshot = render_state.update(terminal)?;
            let colors = snapshot.colors()?;
            foreground = color(colors.foreground);
            background = color(colors.background);
            *palette = colors.palette.map(color);
            dictionary.ensure_default(
                PackedStyle::new(foreground, background, None, 0, crate::UnderlineStyle::None),
                &colors.palette,
            );
            let mut rows = row_iterator.update(&snapshot)?;
            let mut viewport_row = 0_usize;
            while let Some(row) = rows.next() {
                let absolute_row = page_offset.saturating_add(viewport_row);
                viewport_row += 1;
                if absolute_row < captured_until || absolute_row >= total_rows {
                    continue;
                }

                let raw_row = row.raw_row()?;
                row_meta[absolute_row] = ModeRowMeta::new(
                    raw_row.is_wrapped()?,
                    raw_row.is_wrap_continuation()?,
                    raw_row.semantic_prompt()?,
                );
                let text_start = u32::try_from(search_text.len())
                    .map_err(|_| WorkerError::SearchSnapshotTooLarge)?;
                let offset_start = u32::try_from(search_offsets.len())
                    .map_err(|_| WorkerError::SearchSnapshotTooLarge)?;
                let row_text_start = search_text.len();
                let row_start = absolute_row.saturating_mul(usize::from(columns));
                let mut page_cells = cell_iterator.update(row)?;
                let mut column = 0_u16;
                while let Some(cell) = page_cells.next() {
                    if column >= columns {
                        break;
                    }
                    let index = row_start.saturating_add(usize::from(column));
                    let raw_style = cell.style()?;
                    let mut cell_foreground = color(cell.fg_color()?.unwrap_or(colors.foreground));
                    let mut cell_background = color(cell.bg_color()?.unwrap_or(colors.background));
                    if raw_style.inverse {
                        std::mem::swap(&mut cell_foreground, &mut cell_background);
                    }
                    grapheme_scratch.clear();
                    cell.graphemes_utf8(&mut grapheme_scratch)?;
                    let raw_cell = cell.raw_cell()?;
                    let wide = raw_cell.wide()?;
                    let width = match wide {
                        CellWide::Narrow => CellWidth::Narrow,
                        CellWide::Wide => CellWidth::Wide,
                        CellWide::SpacerTail => CellWidth::SpacerTail,
                        CellWide::SpacerHead => CellWidth::SpacerHead,
                    };
                    let style = PackedStyle::new(
                        cell_foreground,
                        cell_background,
                        resolve_style_color(raw_style.underline_color, &colors.palette),
                        style_attributes(
                            &raw_style,
                            matches!(
                                if raw_style.inverse {
                                    raw_style.bg_color
                                } else {
                                    raw_style.fg_color
                                },
                                libghostty_vt::style::StyleColor::Rgb(_)
                            ),
                            raw_cell.has_hyperlink()?,
                        ),
                        underline_style(raw_style.underline),
                    );
                    cells[index] = PackedCell::new(
                        dictionary.encode_glyph(&grapheme_scratch),
                        dictionary.intern_style(style),
                        width,
                    );
                    semantics[index] = match raw_cell.semantic_content()? {
                        CellSemanticContent::Output => SEMANTIC_OUTPUT,
                        CellSemanticContent::Input => SEMANTIC_INPUT,
                        CellSemanticContent::Prompt => SEMANTIC_PROMPT,
                    };
                    if !matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
                        let start = u32::try_from(search_text.len() - row_text_start)
                            .map_err(|_| WorkerError::SearchSnapshotTooLarge)?;
                        search_text.push_str(&grapheme_scratch);
                        let end = u32::try_from(search_text.len() - row_text_start)
                            .map_err(|_| WorkerError::SearchSnapshotTooLarge)?;
                        if end > start {
                            search_offsets.push(SearchCellOffset {
                                start,
                                end,
                                column,
                                width: u16::from(matches!(wide, CellWide::Wide)) + 1,
                            });
                        }
                    }
                    column = column.saturating_add(1);
                }
                search_rows.push(HistorySearchRow {
                    text_start,
                    text_end: u32::try_from(search_text.len())
                        .map_err(|_| WorkerError::SearchSnapshotTooLarge)?,
                    offset_start,
                    offset_end: u32::try_from(search_offsets.len())
                        .map_err(|_| WorkerError::SearchSnapshotTooLarge)?,
                });
                captured_until = absolute_row.saturating_add(1);
                let search_bytes = search_text
                    .len()
                    .saturating_add(search_offsets.len() * std::mem::size_of::<SearchCellOffset>())
                    .saturating_add(search_rows.len() * std::mem::size_of::<HistorySearchRow>());
                if base_bytes.saturating_add(search_bytes) > MAX_MODE_REVISION_BYTES
                    || search_bytes > MAX_SEARCH_SNAPSHOT_BYTES
                {
                    terminal.scroll_viewport(ScrollViewport::Top);
                    terminal.scroll_viewport(ScrollViewport::Delta(super::saturating_isize(
                        i64::try_from(saved_offset).unwrap_or(i64::MAX),
                    )));
                    return Err(WorkerError::ModeRevisionTooLarge);
                }
            }
            if scrollbar.offset >= maximum || captured_until >= total_rows {
                break;
            }
            let next = scrollbar.offset.saturating_add(scrollbar.len).min(maximum);
            let delta = next.saturating_sub(scrollbar.offset);
            terminal.scroll_viewport(ScrollViewport::Delta(super::saturating_isize(
                i64::try_from(delta).unwrap_or(i64::MAX),
            )));
        }
        terminal.scroll_viewport(ScrollViewport::Top);
        terminal.scroll_viewport(ScrollViewport::Delta(super::saturating_isize(
            i64::try_from(saved_offset).unwrap_or(i64::MAX),
        )));
        let shared_dictionary = dictionary.shared_dictionary();
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
            cells,
            dictionary: shared_dictionary,
            rows: row_meta,
            semantics,
            search: Arc::new(HistorySearchSnapshot {
                columns,
                text: search_text,
                rows: search_rows,
                offsets: search_offsets,
            }),
        }))
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
        u32::try_from(self.rows.len()).unwrap_or(u32::MAX).max(1)
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
        let index = usize::try_from(point.y)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::from(self.columns))
            .saturating_add(usize::from(point.x));
        self.cells.get(index).copied().unwrap_or(PackedCell::EMPTY)
    }

    pub(super) fn first_char(&self, point: PointCoordinate) -> Option<char> {
        let cell = self.cell(point);
        if cell.glyph() & crate::GRAPHEME_TABLE_BIT == 0 {
            return char::from_u32(cell.glyph()).filter(|character| *character != '\0');
        }
        let index = usize::try_from(cell.glyph() & !crate::GRAPHEME_TABLE_BIT).ok()?;
        let start = usize::try_from(*self.dictionary.grapheme_offsets.get(index)?).ok()?;
        let end = usize::try_from(*self.dictionary.grapheme_offsets.get(index + 1)?).ok()?;
        std::str::from_utf8(self.dictionary.grapheme_bytes.get(start..end)?)
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
        let Some(start) = self
            .dictionary
            .grapheme_offsets
            .get(index)
            .and_then(|offset| usize::try_from(*offset).ok())
        else {
            return false;
        };
        let Some(end) = self
            .dictionary
            .grapheme_offsets
            .get(index + 1)
            .and_then(|offset| usize::try_from(*offset).ok())
        else {
            return false;
        };
        self.dictionary.grapheme_bytes.get(start..end) == Some(target.as_bytes())
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
        let Some(start) = self
            .dictionary
            .grapheme_offsets
            .get(index)
            .and_then(|offset| usize::try_from(*offset).ok())
        else {
            return;
        };
        let Some(end) = self
            .dictionary
            .grapheme_offsets
            .get(index + 1)
            .and_then(|offset| usize::try_from(*offset).ok())
        else {
            return;
        };
        if let Some(text) = self
            .dictionary
            .grapheme_bytes
            .get(start..end)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        {
            output.push_str(text);
        }
    }

    pub(super) fn semantic(&self, point: PointCoordinate) -> u8 {
        let point = self.clamp_point(point);
        let index = usize::try_from(point.y)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::from(self.columns))
            .saturating_add(usize::from(point.x));
        self.semantics
            .get(index)
            .copied()
            .unwrap_or(SEMANTIC_OUTPUT)
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
        usize::try_from(row)
            .ok()
            .and_then(|row| self.rows.get(row))
            .copied()
            .unwrap_or_default()
    }

    pub(super) fn selection_row_end(&self, row: u32) -> u16 {
        if self.row(row).wrapped() {
            return self.columns;
        }
        (0..self.columns)
            .rev()
            .find(|column| {
                self.first_char(PointCoordinate { x: *column, y: row })
                    .is_some_and(|character| character != ' ')
            })
            .map_or(0, |column| column.saturating_add(1))
    }

    fn push_selection_cell_text(&self, cell: PackedCell, output: &mut String) {
        if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
            return;
        }
        if cell.glyph() == 0 {
            output.push(' ');
        } else {
            self.push_cell_text(cell, output);
        }
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
        for row in start.y..=end.y {
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
            let line_end = self.selection_row_end(row);
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
                for column in row_start..selected_end {
                    self.push_selection_cell_text(
                        self.cell(PointCoordinate { x: column, y: row }),
                        &mut output,
                    );
                }
            }
            let has_line_break = !self.row(row).wrapped() || selected_end < line_end;
            if has_line_break
                && (row < end.y || (row == end.y && selection.mode == SelectionMode::Line))
            {
                output.push('\n');
            }
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
        for row in start..=end {
            let mut line = String::new();
            for column in 0..self.columns {
                self.push_cell_text(self.cell(PointCoordinate { x: column, y: row }), &mut line);
            }
            if preserve_trailing {
                output.push_str(&line);
            } else {
                output.push_str(line.trim_end());
            }
            if row < end && !(join_wrapped && self.row(row).wrapped()) {
                output.push('\n');
            }
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
        for row in start..=end {
            let mut line = String::new();
            let mut active_style = None;
            let last_column = if preserve_trailing {
                self.columns.checked_sub(1)
            } else {
                (0..self.columns).rev().find(|column| {
                    self.first_char(PointCoordinate { x: *column, y: row })
                        .is_some_and(|character| !character.is_whitespace())
                })
            };
            for column in 0..=last_column.unwrap_or(0) {
                let cell = self.cell(PointCoordinate { x: column, y: row });
                if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
                    continue;
                }
                let mut text = String::new();
                self.push_cell_text(cell, &mut text);
                if text.is_empty() {
                    continue;
                }
                if active_style != Some(cell.style_id()) {
                    if let Some(style) = self
                        .dictionary
                        .styles
                        .get(usize::from(cell.style_id()))
                        .copied()
                    {
                        push_sgr(&mut line, style);
                    }
                    active_style = Some(cell.style_id());
                }
                line.push_str(&text);
            }
            if active_style.is_some() {
                line.push_str("\x1b[0m");
            }
            output.push_str(&line);
            if row < end && !(join_wrapped && self.row(row).wrapped()) {
                output.push('\n');
            }
        }
        output
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
