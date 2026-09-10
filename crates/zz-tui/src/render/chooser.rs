use std::collections::HashMap;

use unicode_width::UnicodeWidthChar as _;
use zz_protocol::{
    ChooseBufferState, ChooseTreeState, ChooserPresentation, ChooserPreview, ChooserPreviewSize,
    ChooserPreviewTile, ThemeColours, TmuxColour, TmuxStyle, apply_style, parse_style,
    parse_styled_segments,
};
use zz_terminal::{
    CellWidth, Color, Glyph, PackedCell, PackedStyle, TerminalAppearance, TerminalViewport,
    UnderlineStyle,
};

use super::{Renderer, write_cursor_position, write_sgr, write_tmux_sgr};
use crate::state::Model;

const HELP_START: &[(&str, &str)] = &[
    ("      Up, k", "Move cursor up"),
    ("    Down, j", "Move cursor down"),
    ("          g", "Go to top"),
    ("          G", "Go to bottom"),
    (" PPage, C-b", "Page up"),
    (" NPage, C-f", "Page down"),
    ("    Left, h", "Collapse %1"),
    ("   Right, l", "Expand %1"),
    ("        M--", "Collapse all %1s"),
    ("        M-+", "Expand all %1s"),
    ("          t", "Toggle %1 tag"),
    ("          T", "Untag all %1s"),
    ("        C-t", "Tag all %1s"),
    ("        C-s", "Search forward"),
    ("          n", "Repeat search forward"),
    ("          N", "Repeat search backward"),
    ("          f", "Filter %1s"),
    ("          O", "Change sort order"),
    ("          r", "Reverse sort order"),
    ("          v", "Toggle preview"),
];
const HELP_TREE: &[(&str, &str)] = &[
    ("      Enter", "Choose selected item"),
    ("       S-Up", "Swap current and previous window"),
    ("     S-Down", "Swap current and next window"),
    ("          x", "Kill selected item"),
    ("          X", "Kill tagged items"),
    ("          <", "Scroll previews left"),
    ("          >", "Scroll previews right"),
    ("          m", "Set the marked pane"),
    ("          M", "Clear the marked pane"),
    ("          i", "Toggle session, window and pane information"),
    ("          :", "Run a command for each tagged item"),
    ("          f", "Enter a format"),
    ("          H", "Jump to the starting pane"),
];
const HELP_BUFFER: &[(&str, &str)] = &[
    ("      Enter", "Paste selected %1"),
    ("          p", "Paste selected %1"),
    ("          P", "Paste tagged %1s"),
    ("          d", "Delete selected %1"),
    ("          D", "Delete tagged %1s"),
    ("          e", "Open %1 in editor"),
    ("          f", "Enter a filter"),
];
const HELP_END: &[(&str, &str)] = &[("  q, Escape", "Exit mode")];
const HELP_DEFAULT_WIDTH: u16 = 39;
const HELP_TREE_WIDTH: u16 = 51;

pub(super) enum ModeTreePaint {
    Painted((u16, u16, bool)),
    Pending,
    Unavailable,
}

#[derive(Default)]
pub(super) struct ModeTree {
    offset: usize,
    open: bool,
    cursor: Option<(u16, u16, bool)>,
}

#[derive(Clone, PartialEq)]
enum Paint {
    Style(TmuxStyle),
    Pane {
        style: PackedStyle,
        reverse: bool,
        foreground: Color,
        background: Color,
    },
}

#[derive(Clone)]
struct Cell {
    glyph: String,
    width: u8,
    paint: Paint,
}

struct Grid {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

fn plain() -> TmuxStyle {
    TmuxStyle {
        fg: Some(TmuxColour::Default),
        bg: Some(TmuxColour::Default),
        ..TmuxStyle::default()
    }
}

fn layered(delta: &str, base: &TmuxStyle) -> TmuxStyle {
    let mut style = base.clone();
    if let Some(delta) = parse_style(delta) {
        apply_style(&mut style, &delta, base);
    }
    style
}

fn theme(name: &str) -> Option<TmuxColour> {
    parse_style(&format!("fg={name}")).and_then(|style| style.fg)
}

fn resolved(style: &TmuxStyle, theme: &ThemeColours) -> TmuxStyle {
    let mut style = style.clone();
    for slot in [&mut style.fg, &mut style.bg, &mut style.us] {
        if let Some(TmuxColour::Theme(index)) = slot
            && let Some(colour) = theme.slot(*index)
        {
            *slot = Some(colour);
        }
    }
    style
}

fn text_width(text: &str) -> usize {
    text.chars()
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

fn markup_width(markup: &str) -> usize {
    parse_styled_segments(markup)
        .iter()
        .map(|segment| text_width(&segment.text))
        .sum()
}

fn narrow(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

impl Grid {
    fn new(width: u16, height: u16) -> Self {
        let blank = Cell {
            glyph: " ".to_owned(),
            width: 1,
            paint: Paint::Style(plain()),
        };
        Self {
            width,
            height,
            cells: vec![blank; usize::from(width) * usize::from(height)],
        }
    }

    fn index(&self, x: u16, y: u16) -> Option<usize> {
        (x < self.width && y < self.height)
            .then(|| usize::from(y) * usize::from(self.width) + usize::from(x))
    }

    fn blank_at(&mut self, x: u16, y: u16) {
        if let Some(index) = self.index(x, y) {
            " ".clone_into(&mut self.cells[index].glyph);
            self.cells[index].width = 1;
        }
    }

    fn set(&mut self, x: u16, y: u16, glyph: &str, width: u8, paint: &Paint) {
        let Some(index) = self.index(x, y) else {
            return;
        };
        if self.cells[index].width == 0 && x > 0 {
            self.blank_at(x - 1, y);
        }
        if self.cells[index].width == 2 {
            self.blank_at(x + 1, y);
        }
        if width == 2 && x + 1 >= self.width {
            self.cells[index] = Cell {
                glyph: " ".to_owned(),
                width: 1,
                paint: paint.clone(),
            };
            return;
        }
        self.cells[index] = Cell {
            glyph: glyph.to_owned(),
            width,
            paint: paint.clone(),
        };
        if width == 2
            && let Some(tail) = self.index(x + 1, y)
        {
            if self.cells[tail].width == 2 {
                self.blank_at(x + 2, y);
            }
            self.cells[tail] = Cell {
                glyph: String::new(),
                width: 0,
                paint: paint.clone(),
            };
        }
    }

    fn text(&mut self, x: u16, y: u16, text: &str, paint: &Paint, limit: u16) -> u16 {
        let mut used = 0_u16;
        for character in text.chars() {
            let Some(width) = character.width().filter(|width| *width > 0) else {
                continue;
            };
            let width = narrow(width);
            if used.saturating_add(width) > limit {
                break;
            }
            let mut buffer = [0; 4];
            self.set(
                x.saturating_add(used),
                y,
                character.encode_utf8(&mut buffer),
                u8::try_from(width).unwrap_or(1),
                paint,
            );
            used += width;
        }
        used
    }

    fn fill(&mut self, x: u16, y: u16, count: u16, paint: &Paint) {
        for offset in 0..count {
            self.set(x.saturating_add(offset), y, " ", 1, paint);
        }
    }

    fn markup(
        &mut self,
        x: u16,
        y: u16,
        limit: u16,
        markup: &str,
        base: &TmuxStyle,
        default_colours: bool,
    ) -> u16 {
        let mut used = 0_u16;
        for segment in parse_styled_segments(markup) {
            if used >= limit {
                break;
            }
            let mut style = base.clone();
            apply_style(&mut style, &segment.style, base);
            if default_colours {
                style.fg = base.fg;
                style.bg = base.bg;
            }
            used += self.text(
                x.saturating_add(used),
                y,
                &segment.text,
                &Paint::Style(style),
                limit - used,
            );
        }
        used
    }

    fn vline(&mut self, x: u16, y: u16, count: u16, paint: &Paint) {
        for row in 0..count {
            self.set(x, y.saturating_add(row), "│", 1, paint);
        }
    }

    fn frame(&mut self, x: u16, y: u16, width: u16, height: u16, paint: &Paint) {
        if width < 2 || height < 2 {
            return;
        }
        let right = x + width - 1;
        let bottom = y + height - 1;
        self.set(x, y, "┌", 1, paint);
        self.set(right, y, "┐", 1, paint);
        self.set(x, bottom, "└", 1, paint);
        self.set(right, bottom, "┘", 1, paint);
        for column in x + 1..right {
            self.set(column, y, "─", 1, paint);
            self.set(column, bottom, "─", 1, paint);
        }
        for row in y + 1..bottom {
            self.set(x, row, "│", 1, paint);
            self.set(right, row, "│", 1, paint);
        }
    }

    fn preview(&mut self, x: u16, y: u16, nx: u16, ny: u16, viewport: &TerminalViewport) {
        let cursor = viewport.cursor.filter(|cursor| cursor.visible());
        let (px, py) = cursor.map_or((0, 0), |cursor| {
            (
                preview_origin(cursor.column(), nx, viewport.columns),
                preview_origin(cursor.row(), ny, viewport.rows),
            )
        });
        let default_style = viewport.styles().first().copied().unwrap_or_else(|| {
            PackedStyle::new(
                viewport.foreground,
                viewport.background,
                None,
                0,
                UnderlineStyle::None,
            )
        });
        let paint = |cell: PackedCell, reverse: bool| Paint::Pane {
            style: viewport.style(cell).unwrap_or(default_style),
            reverse,
            foreground: viewport.foreground,
            background: viewport.background,
        };
        for row in 0..ny {
            let source_row = py + row;
            if source_row >= viewport.rows {
                break;
            }
            for column in 0..nx {
                let source_column = px + column;
                if source_column >= viewport.columns {
                    break;
                }
                let cell = viewport
                    .cell(source_row, source_column)
                    .unwrap_or(PackedCell::EMPTY);
                if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
                    continue;
                }
                let (glyph, width) = glyph_of(viewport, cell);
                self.set(x + column, y + row, &glyph, width, &paint(cell, false));
            }
        }
        if let Some(cursor) = cursor {
            let (column, row) = (cursor.column(), cursor.row());
            if column >= px && row >= py && column - px < nx && row - py < ny {
                let cell = viewport.cell(row, column).unwrap_or(PackedCell::EMPTY);
                let (glyph, width) = glyph_of(viewport, cell);
                self.set(
                    x + column - px,
                    y + row - py,
                    &glyph,
                    width,
                    &paint(cell, true),
                );
            }
        }
    }

    fn settle_blank_runs(&mut self) {
        let width = usize::from(self.width);
        for row in 0..usize::from(self.height) {
            let line = &mut self.cells[row * width..(row + 1) * width];
            let mut start = 0;
            while start < width {
                let Some(first) = blank_style(&line[start]).cloned() else {
                    start += 1;
                    continue;
                };
                let mut end = start + 1;
                while end < width
                    && blank_style(&line[end]).is_some_and(|style| {
                        style.bg == first.bg && style.attributes == first.attributes
                    })
                {
                    end += 1;
                }
                let last = line[end - 1].paint.clone();
                for cell in &mut line[start..end] {
                    cell.paint = last.clone();
                }
                start = end;
            }
        }
    }

    fn emit(
        &self,
        output: &mut Vec<u8>,
        x: u16,
        y: u16,
        theme: &ThemeColours,
        appearance: &TerminalAppearance,
    ) {
        let width = usize::from(self.width);
        for row in 0..self.height {
            write_cursor_position(output, x, y + row);
            let line = &self.cells[usize::from(row) * width..(usize::from(row) + 1) * width];
            let used = line
                .iter()
                .rposition(|cell| !erasable(cell))
                .map_or(0, |index| index + 1);
            let mut current: Option<&Paint> = None;
            for cell in &line[..used] {
                if cell.width == 0 {
                    continue;
                }
                if current != Some(&cell.paint) {
                    match &cell.paint {
                        Paint::Style(style) => write_tmux_sgr(
                            output,
                            &resolved(style, theme),
                            appearance.foreground,
                            appearance.background,
                            appearance,
                        ),
                        Paint::Pane {
                            style,
                            reverse,
                            foreground,
                            background,
                        } => write_sgr(output, *style, *reverse, *foreground, *background),
                    }
                    current = Some(&cell.paint);
                }
                output.extend_from_slice(cell.glyph.as_bytes());
            }
            output.extend_from_slice(b"\x1b[0m");
            if used < width {
                output.extend_from_slice(b"\x1b[K");
            }
        }
    }
}

fn blank_style(cell: &Cell) -> Option<&TmuxStyle> {
    match &cell.paint {
        Paint::Style(style) if cell.width == 1 && cell.glyph == " " => Some(style),
        _ => None,
    }
}

fn erasable(cell: &Cell) -> bool {
    blank_style(cell).is_some_and(|style| {
        matches!(style.bg, None | Some(TmuxColour::Default))
            && style.attributes == plain().attributes
    })
}

fn preview_origin(cursor: u16, span: u16, size: u16) -> u16 {
    let mut origin = cursor.saturating_sub(span / 3);
    if origin + span > size {
        origin = size.saturating_sub(span);
    }
    origin
}

fn glyph_of(viewport: &TerminalViewport, cell: PackedCell) -> (String, u8) {
    let width = if cell.width() == CellWidth::Wide {
        2
    } else {
        1
    };
    let glyph = match viewport.glyph(cell) {
        Glyph::Empty => " ".to_owned(),
        Glyph::Scalar(character) => character.to_string(),
        Glyph::Grapheme(grapheme) => grapheme.to_owned(),
    };
    (glyph, width)
}

struct Line<'a> {
    depth: usize,
    key: &'a str,
    children: bool,
    expanded: bool,
    tagged: bool,
    name: &'a str,
    text: &'a str,
    align: bool,
    last: bool,
    parent_last: bool,
    flat: bool,
}

fn tree_lines<'a>(
    state: &'a ChooseTreeState,
    presentation: &'a ChooserPresentation,
) -> Option<Vec<Line<'a>>> {
    (state.items.len() == presentation.rows.len()).then(|| {
        state
            .items
            .iter()
            .zip(&presentation.rows)
            .map(|(item, row)| Line {
                depth: usize::from(item.depth),
                key: &item.key,
                children: item.has_children(),
                expanded: item.expanded(),
                tagged: item.tagged(),
                name: &row.name,
                text: &row.text,
                align: row.align,
                last: false,
                parent_last: false,
                flat: true,
            })
            .collect()
    })
}

fn buffer_lines<'a>(
    state: &'a ChooseBufferState,
    presentation: &'a ChooserPresentation,
) -> Option<Vec<Line<'a>>> {
    (state.items.len() == presentation.rows.len()).then(|| {
        state
            .items
            .iter()
            .zip(&presentation.rows)
            .map(|(item, row)| Line {
                depth: 0,
                key: &item.key,
                children: false,
                expanded: false,
                tagged: item.tagged,
                name: &row.name,
                text: &row.text,
                align: row.align,
                last: false,
                parent_last: false,
                flat: true,
            })
            .collect()
    })
}

fn link_lines(lines: &mut [Line<'_>]) {
    let count = lines.len();
    let mut parents = vec![None; count];
    let mut stack: Vec<usize> = Vec::new();
    for index in 0..count {
        let depth = lines[index].depth;
        stack.truncate(depth);
        parents[index] = depth
            .checked_sub(1)
            .and_then(|level| stack.get(level).copied());
        stack.push(index);
    }
    for index in 0..count {
        let depth = lines[index].depth;
        lines[index].last = lines[index + 1..]
            .iter()
            .find(|line| line.depth <= depth)
            .is_none_or(|line| line.depth < depth);
    }
    let mut siblings_with_children: HashMap<(Option<usize>, usize), bool> = HashMap::new();
    for index in 0..count {
        let entry = siblings_with_children
            .entry((parents[index], lines[index].depth))
            .or_default();
        *entry |= lines[index].children;
    }
    for index in 0..count {
        lines[index].parent_last = parents[index].is_some_and(|parent| lines[parent].last);
        lines[index].flat = !siblings_with_children
            .get(&(parents[index], lines[index].depth))
            .copied()
            .unwrap_or(false);
    }
}

fn list_height(size: ChooserPreviewSize, rows: usize, lines: usize) -> usize {
    let mut height = match size {
        ChooserPreviewSize::Normal => {
            let mut height = (rows / 3) * 2;
            if height > lines {
                height = rows / 2;
            }
            if height < 10 {
                height = rows;
            }
            height
        }
        ChooserPreviewSize::Big => (rows / 4).min(lines).max(2),
        ChooserPreviewSize::Off => rows,
    };
    if rows.saturating_sub(height) < 2 {
        height = rows;
    }
    height
}

struct Colours {
    grey: Option<TmuxColour>,
    red: Option<TmuxColour>,
    green: Option<TmuxColour>,
    cyan: Option<TmuxColour>,
}

fn prefix_pieces(
    line: &Line<'_>,
    key_width: usize,
    colours: &Colours,
) -> Vec<(String, Option<TmuxColour>)> {
    let mut pieces = Vec::new();
    let key = if line.key.is_empty() {
        String::new()
    } else {
        format!("({})", line.key)
    };
    let pad = key_width.saturating_sub(text_width(&key));
    pieces.push((format!("{key}{}", " ".repeat(pad)), colours.grey));
    if line.depth > 0 {
        let unit = if line.parent_last { "    " } else { "│   " };
        pieces.push((unit.repeat(line.depth - 1), colours.grey));
        let branch = if line.last { "└─> " } else { "├─> " };
        pieces.push((branch.to_owned(), colours.grey));
    }
    if line.children {
        if line.expanded {
            pieces.push(("-".to_owned(), colours.red));
        } else {
            pieces.push(("+".to_owned(), colours.green));
        }
        pieces.push((" ".to_owned(), colours.grey));
    } else if !line.flat {
        pieces.push(("  ".to_owned(), colours.grey));
    }
    pieces
}

fn tile_row(
    grid: &mut Grid,
    origin: (u16, u16),
    size: (u16, u16),
    tiles: &[ChooserPreviewTile],
    current: usize,
    border: &TmuxStyle,
) {
    let (cx, cy) = origin;
    let (sx, sy) = (usize::from(size.0), size.1);
    let total = tiles.len();
    if total == 0 {
        return;
    }
    let visible = if sx / total < 24 {
        (sx / 24).max(1)
    } else {
        total
    };
    let current = current.min(total - 1);
    let (start, end) = if current < visible {
        (0, visible)
    } else if current >= total - visible {
        (total - visible, total)
    } else {
        let start = current - visible / 2;
        (start, start + visible)
    };
    let mut left = start != 0;
    let mut right = end != total;
    if ((left && right) && sx <= 6) || ((left || right) && sx <= 3) {
        left = false;
        right = false;
    }
    let span = if left && right {
        sx - 6
    } else if left || right {
        sx - 3
    } else {
        sx
    };
    let each = span / visible;
    let remaining = span - visible * each;
    if each == 0 {
        return;
    }
    let gutter = Paint::Style(border.clone());
    if left {
        grid.vline(cx + 2, cy, sy, &gutter);
        grid.text(cx, cy + sy / 2, "<", &gutter, 1);
    }
    if right {
        let edge = cx + narrow(sx);
        grid.vline(edge - 3, cy, sy, &gutter);
        grid.text(edge - 1, cy + sy / 2, ">", &gutter, 1);
    }
    for (slot, index) in (start..end).enumerate() {
        let tile = &tiles[index];
        let tile_border = layered(&tile.border_style, &plain());
        let mut label_style = layered(&tile.label_style, &plain());
        label_style.bg = tile_border.bg;
        let offset = narrow(if left { 3 + slot * each } else { slot * each });
        let width = narrow(if index == end - 1 {
            each + remaining
        } else {
            each - 1
        });
        if let Some(viewport) = &tile.viewport {
            grid.preview(cx + offset, cy, width, sy, viewport);
        }
        if !tile.label.is_empty() {
            label(
                grid,
                (cx + offset, cy),
                (width, sy),
                &tile_border,
                &label_style,
                &tile.label,
            );
        }
        if index != end - 1 {
            grid.vline(
                cx + offset + width,
                cy,
                sy,
                &Paint::Style(tile_border.clone()),
            );
        }
    }
}

fn label(
    grid: &mut Grid,
    origin: (u16, u16),
    size: (u16, u16),
    border: &TmuxStyle,
    style: &TmuxStyle,
    text: &str,
) {
    let (px, py) = origin;
    let (sx, sy) = size;
    if sx < 5 || sy < 3 {
        return;
    }
    let width = narrow(markup_width(text)).min(sx - 4);
    if width == 0 {
        return;
    }
    let ox = (sx - width).div_ceil(2);
    let oy = sy.div_ceil(2);
    grid.frame(
        px + ox - 2,
        py + oy - 1,
        width + 4,
        3,
        &Paint::Style(border.clone()),
    );
    let clear = TmuxStyle {
        bg: border.bg,
        ..plain()
    };
    grid.fill(px + ox - 1, py + oy, width + 2, &Paint::Style(clear));
    grid.markup(px + ox, py + oy, width, text, style, false);
}

fn help(grid: &mut Grid, tree: bool, border: &TmuxStyle, colours: &Colours) {
    let (width, item, lines) = if tree {
        (HELP_TREE_WIDTH, "item", HELP_TREE)
    } else {
        (HELP_DEFAULT_WIDTH, "buffer", HELP_BUFFER)
    };
    let count = narrow(HELP_START.len() + lines.len() + HELP_END.len());
    let (box_width, box_height) = (width + 2, count + 2);
    if grid.width < box_width || grid.height < box_height {
        return;
    }
    let x = (grid.width - box_width) / 2;
    let y = (grid.height - box_height) / 2;
    grid.frame(x, y, box_width, box_height, &Paint::Style(border.clone()));
    let key_style = TmuxStyle {
        fg: colours.grey,
        ..plain()
    };
    let separator = layered("", border);
    for (index, (key, text)) in HELP_START.iter().chain(lines).chain(HELP_END).enumerate() {
        let row = y + 1 + narrow(index);
        grid.fill(x + 1, row, width, &Paint::Style(plain()));
        let mut used = grid.text(
            x + 1,
            row,
            &format!("{key} "),
            &Paint::Style(key_style.clone()),
            width,
        );
        used += grid.text(
            x + 1 + used,
            row,
            "│",
            &Paint::Style(separator.clone()),
            width - used,
        );
        grid.text(
            x + 1 + used,
            row,
            &format!(" {}", text.replace("%1", item)),
            &Paint::Style(plain()),
            width - used,
        );
    }
}

impl Renderer {
    pub(super) fn paint_mode_tree(&mut self, model: &Model) -> ModeTreePaint {
        let Some(presentation) = model.chooser_presentation.as_ref() else {
            self.mode_tree.open = false;
            return ModeTreePaint::Unavailable;
        };
        let selected = model
            .choose_tree
            .as_ref()
            .map(|state| state.selected)
            .or_else(|| model.choose_buffer.as_ref().map(|state| state.selected));
        if self.mode_tree.open && selected != Some(presentation.selected) {
            return ModeTreePaint::Pending;
        }
        let cursor = self.draw_mode_tree(model);
        self.mode_tree.open = cursor.is_some();
        self.mode_tree.cursor = cursor;
        cursor.map_or(ModeTreePaint::Unavailable, ModeTreePaint::Painted)
    }

    pub(super) fn restore_mode_tree_cursor_now(&mut self) {
        self.place_mode_tree_cursor(self.mode_tree.cursor);
    }

    pub(super) fn place_mode_tree_cursor(&mut self, cursor: Option<(u16, u16, bool)>) {
        match cursor {
            Some((x, y, visible)) => {
                write_cursor_position(&mut self.output, x, y);
                self.output
                    .extend_from_slice(if visible { b"\x1b[?25h" } else { b"\x1b[?25l" });
            }
            None => self.hide_cursor(),
        }
    }

    pub(super) fn restore_mode_tree_cursor(&mut self, model: &Model) {
        let open = model.choose_tree.is_some() || model.choose_buffer.is_some();
        let cursor = if open { self.mode_tree.cursor } else { None };
        self.place_mode_tree_cursor(cursor);
    }

    fn draw_mode_tree(&mut self, model: &Model) -> Option<(u16, u16, bool)> {
        let presentation = model.chooser_presentation.as_ref()?;
        let (mut lines, selected, show_help, prompt, no_matches, tree) =
            if let Some(state) = model.choose_tree.as_ref() {
                let prompt = if state.prompt.is_empty() {
                    state
                        .search
                        .as_ref()
                        .map(|search| format!("(search) {}", search.query))
                } else {
                    Some(state.prompt.clone())
                };
                (
                    tree_lines(state, presentation)?,
                    state.selected,
                    state.help,
                    prompt,
                    state.filter_no_matches,
                    true,
                )
            } else {
                let state = model.choose_buffer.as_ref()?;
                (
                    buffer_lines(state, presentation)?,
                    state.selected,
                    state.help,
                    state
                        .search
                        .as_ref()
                        .map(|search| format!("(search) {}", search.query)),
                    state.filter_no_matches,
                    false,
                )
            };
        let status_rows = model.status_block_rows();
        let sx = model.size.columns;
        let sy = model.size.rows.saturating_sub(status_rows);
        let top = if model.status_top() { status_rows } else { 0 };
        if sx == 0 || sy == 0 || lines.is_empty() {
            return None;
        }
        link_lines(&mut lines);
        let count = lines.len();
        let current = usize::try_from(selected)
            .unwrap_or(usize::MAX)
            .min(count - 1);
        let height = list_height(presentation.preview_size, usize::from(sy), count);
        if !self.mode_tree.open {
            self.mode_tree.offset = 0;
        }
        if current < self.mode_tree.offset {
            self.mode_tree.offset = current;
        } else if current >= self.mode_tree.offset + height {
            self.mode_tree.offset = current + 1 - height;
        }
        let offset = self.mode_tree.offset;
        let colours = Colours {
            grey: theme("themelightgrey"),
            red: theme("themered"),
            green: theme("themegreen"),
            cyan: theme("themecyan"),
        };
        let selection = layered(&presentation.selection_style, &plain());
        let border = layered(&presentation.border_style, &plain());
        let key_width = lines
            .iter()
            .filter(|line| !line.key.is_empty())
            .map(|line| line.key.len() + 3)
            .max()
            .unwrap_or(0);
        let mut align_widths: HashMap<usize, usize> = HashMap::new();
        for line in lines.iter().filter(|line| line.align) {
            let width = align_widths.entry(line.depth).or_default();
            *width = (*width).max(line.name.len());
        }
        let mut grid = Grid::new(sx, sy);
        for (row, index) in (offset..count.min(offset + height)).enumerate() {
            let line = &lines[index];
            let y = narrow(row);
            let chosen = index == current;
            let mut unselected = plain();
            let mut chosen_style = selection.clone();
            if line.tagged {
                unselected.fg = colours.cyan;
                chosen_style.fg = colours.cyan;
            }
            if chosen {
                let fill = TmuxStyle {
                    bg: chosen_style.bg,
                    ..plain()
                };
                grid.fill(0, y, sx, &Paint::Style(fill));
            }
            let pieces = prefix_pieces(line, key_width, &colours);
            let prefix_width =
                narrow(pieces.iter().map(|(text, _)| text_width(text)).sum()).min(sx);
            let mut x = 0;
            for (text, fg) in &pieces {
                let style = if chosen {
                    chosen_style.clone()
                } else {
                    TmuxStyle { fg: *fg, ..plain() }
                };
                x += grid.text(x, y, text, &Paint::Style(style), prefix_width - x);
            }
            let left = sx - prefix_width;
            if left == 0 {
                continue;
            }
            let name = if line.align {
                let width = align_widths.get(&line.depth).copied().unwrap_or(0);
                format!("{:>width$}", line.name)
            } else {
                line.name.to_owned()
            };
            let tag = if line.tagged { "*" } else { "" };
            let head = format!("{name}{tag}#[fg=themelightgrey]: #[default]");
            let base = if chosen { &chosen_style } else { &unselected };
            let head_width = narrow(markup_width(&head)).min(left);
            grid.markup(prefix_width, y, left, &head, base, chosen);
            let width = prefix_width + head_width;
            if width < sx {
                grid.markup(width, y, sx - width, line.text, base, chosen);
            }
        }
        let rows = usize::from(sy);
        if presentation.preview_size != ChooserPreviewSize::Off
            && !(rows <= 4 || height < 2 || rows - height <= 4 || sx <= 4)
        {
            let box_top = narrow(height);
            let box_paint = Paint::Style(border.clone());
            grid.frame(0, box_top, sx, sy - box_top, &box_paint);
            let name = lines[current].name;
            let view = if presentation.view.is_empty() {
                String::new()
            } else {
                format!(" (view: {})", presentation.view)
            };
            let title = if presentation.sort.is_empty() {
                format!(" {name}")
            } else {
                format!(" {name} (sort: {}){view}", presentation.sort)
            };
            if usize::from(sx) - 2 >= title.len() {
                let used = grid.text(1, box_top, &title, &box_paint, sx - 1);
                let state = if no_matches { "no matches" } else { "active" };
                let tail = if presentation.filter
                    && usize::from(sx) - 2 >= title.len() + 10 + state.len() + 2
                {
                    format!(" (filter: {state}) ")
                } else {
                    " ".to_owned()
                };
                grid.text(1 + used, box_top, &tail, &box_paint, sx - 1 - used);
            }
            let (box_x, box_y) = (sx - 4, sy - box_top - 2);
            if box_x != 0 && box_y != 0 {
                match &presentation.preview {
                    Some(ChooserPreview::Tiles { tiles, current }) => tile_row(
                        &mut grid,
                        (2, box_top + 1),
                        (box_x, box_y),
                        tiles,
                        usize::try_from(*current).unwrap_or(0),
                        &border,
                    ),
                    Some(ChooserPreview::Screen { viewport }) => {
                        grid.preview(2, box_top + 1, box_x, box_y, viewport);
                    }
                    Some(ChooserPreview::Text { lines }) => {
                        for (index, line) in lines.iter().take(usize::from(box_y)).enumerate() {
                            if !line.is_empty() {
                                grid.text(
                                    2,
                                    box_top + 1 + narrow(index),
                                    line,
                                    &Paint::Style(plain()),
                                    box_x,
                                );
                            }
                        }
                    }
                    None => {}
                }
            }
        }
        if show_help {
            help(&mut grid, tree, &border, &colours);
        }
        let cursor = if let Some(prompt) = prompt {
            let row = if model.status_top() { 0 } else { sy - 1 };
            let style = layered(&presentation.prompt_style, &plain());
            let used = grid.text(0, row, &prompt, &Paint::Style(style), sx);
            (used.min(sx - 1), top + row, true)
        } else {
            (0, top + narrow(current - offset), false)
        };
        grid.settle_blank_runs();
        grid.emit(
            &mut self.output,
            0,
            top,
            &model.status.theme,
            &model.appearance,
        );
        self.painted.clear();
        self.headers.clear();
        self.picker_cards.clear();
        self.sidebar_rows.clear();
        self.status_rows.clear();
        self.status_geometry = None;
        self.paint_status_block(model, true);
        Some(cursor)
    }
}
