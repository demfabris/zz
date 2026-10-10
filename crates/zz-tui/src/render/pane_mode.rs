use zz_protocol::{
    PaneMode, PanesModeArea, PanesModeBorder, PanesModeClear, StyledSegment, ThemeColours,
    TmuxAlign, TmuxAttributeState, TmuxColour, TmuxStyle, parse_styled_segments, parse_tmux_colour,
};

use super::{
    Renderer,
    chooser::{Grid, Paint, Trailing, acs_glyph, plain, segments_width},
};
use crate::{
    layout::{PaneRect, Rect},
    mode_view::resolved_style,
    state::Model,
};

const CLOCK_TABLE: [[[bool; 5]; 5]; 14] = {
    const O: bool = false;
    const X: bool = true;
    [
        [
            [X, X, X, X, X],
            [X, O, O, O, X],
            [X, O, O, O, X],
            [X, O, O, O, X],
            [X, X, X, X, X],
        ],
        [
            [O, O, O, O, X],
            [O, O, O, O, X],
            [O, O, O, O, X],
            [O, O, O, O, X],
            [O, O, O, O, X],
        ],
        [
            [X, X, X, X, X],
            [O, O, O, O, X],
            [X, X, X, X, X],
            [X, O, O, O, O],
            [X, X, X, X, X],
        ],
        [
            [X, X, X, X, X],
            [O, O, O, O, X],
            [X, X, X, X, X],
            [O, O, O, O, X],
            [X, X, X, X, X],
        ],
        [
            [X, O, O, O, X],
            [X, O, O, O, X],
            [X, X, X, X, X],
            [O, O, O, O, X],
            [O, O, O, O, X],
        ],
        [
            [X, X, X, X, X],
            [X, O, O, O, O],
            [X, X, X, X, X],
            [O, O, O, O, X],
            [X, X, X, X, X],
        ],
        [
            [X, X, X, X, X],
            [X, O, O, O, O],
            [X, X, X, X, X],
            [X, O, O, O, X],
            [X, X, X, X, X],
        ],
        [
            [X, X, X, X, X],
            [O, O, O, O, X],
            [O, O, O, O, X],
            [O, O, O, O, X],
            [O, O, O, O, X],
        ],
        [
            [X, X, X, X, X],
            [X, O, O, O, X],
            [X, X, X, X, X],
            [X, O, O, O, X],
            [X, X, X, X, X],
        ],
        [
            [X, X, X, X, X],
            [X, O, O, O, X],
            [X, X, X, X, X],
            [O, O, O, O, X],
            [X, X, X, X, X],
        ],
        [
            [O, O, O, O, O],
            [O, O, X, O, O],
            [O, O, O, O, O],
            [O, O, X, O, O],
            [O, O, O, O, O],
        ],
        [
            [X, X, X, X, X],
            [X, O, O, O, X],
            [X, X, X, X, X],
            [X, O, O, O, X],
            [X, O, O, O, X],
        ],
        [
            [X, X, X, X, X],
            [X, O, O, O, X],
            [X, X, X, X, X],
            [X, O, O, O, O],
            [X, O, O, O, O],
        ],
        [
            [X, O, O, O, X],
            [X, X, O, X, X],
            [X, O, X, O, X],
            [X, O, O, O, X],
            [X, O, O, O, X],
        ],
    ]
};

const GLYPH: u16 = 5;
const PITCH: u16 = 6;

pub(super) struct ModeSurface {
    grid: Grid,
    pub(super) cursor: (u16, u16),
    pub(super) cursor_visible: bool,
}

fn clock_index(character: char) -> Option<usize> {
    match character {
        '0'..='9' => Some(character as usize - '0' as usize),
        ':' => Some(10),
        'A' => Some(11),
        'P' => Some(12),
        'M' => Some(13),
        _ => None,
    }
}

fn solid(colour: TmuxColour) -> TmuxStyle {
    TmuxStyle {
        fg: Some(colour),
        bg: Some(colour),
        ..TmuxStyle::default()
    }
}

fn foreground(colour: TmuxColour) -> TmuxStyle {
    TmuxStyle {
        fg: Some(colour),
        bg: Some(TmuxColour::Default),
        ..TmuxStyle::default()
    }
}

fn resolve(colour: &str, theme: &ThemeColours) -> TmuxColour {
    let parsed = parse_tmux_colour(colour).unwrap_or(TmuxColour::Default);
    match parsed {
        TmuxColour::Theme(index) => theme.slot(index).unwrap_or(TmuxColour::Default),
        other => other,
    }
}

fn cleared_to(style: &TmuxStyle) -> TmuxStyle {
    TmuxStyle {
        fg: Some(TmuxColour::Default),
        bg: style.bg.or(Some(TmuxColour::Default)),
        ..TmuxStyle::default()
    }
}

fn base_cell(style: &TmuxStyle) -> TmuxStyle {
    let mut style = style.clone();
    style.attributes.noattr = TmuxAttributeState::Unset;
    style
}

fn clock_surface(time: &str, colour: &str, rect: Rect, theme: &ThemeColours) -> ModeSurface {
    let colour = resolve(colour, theme);
    let length = u16::try_from(time.chars().count()).unwrap_or(u16::MAX);
    let mut grid = Grid::new(rect.width, rect.height);
    if rect.width < PITCH.saturating_mul(length) || rect.height < 6 {
        if rect.width < length || rect.height == 0 {
            return ModeSurface {
                grid,
                cursor: (0, 0),
                cursor_visible: false,
            };
        }
        let column = rect.width / 2 - length / 2;
        let row = rect.height / 2;
        let paint = Paint::Style(foreground(colour));
        let used = grid.text(column, row, time, &paint, rect.width - column);
        return ModeSurface {
            grid,
            cursor: (column.saturating_add(used), row),
            cursor_visible: false,
        };
    }
    let mut column = rect.width / 2 - 3 * length;
    let row = rect.height / 2 - 3;
    let paint = Paint::Style(solid(colour));
    let mut cursor = (column, row);
    for character in time.chars() {
        let Some(index) = clock_index(character) else {
            column = column.saturating_add(PITCH);
            continue;
        };
        for down in 0..GLYPH {
            for across in 0..GLYPH {
                let x = column.saturating_add(across);
                let y = row.saturating_add(down);
                cursor = (x, y);
                if CLOCK_TABLE[index][usize::from(down)][usize::from(across)] {
                    cursor = (x.saturating_add(1), y);
                    grid.text(x, y, "#", &paint, 1);
                }
            }
        }
        column = column.saturating_add(PITCH);
    }
    ModeSurface {
        grid,
        cursor,
        cursor_visible: false,
    }
}

struct SwitchView<'a> {
    rows: &'a [String],
    selected: u32,
    offset: u32,
    selection_style: &'a str,
    prompt: &'a str,
    prompt_style: &'a str,
    prompt_cursor: u16,
    matches: &'a [Vec<u16>],
    match_style: &'a str,
}

fn switch_surface(view: &SwitchView<'_>, rect: Rect, theme: &ThemeColours) -> ModeSurface {
    let mut grid = Grid::new(rect.width, rect.height);
    if rect.height <= 1 {
        return ModeSurface {
            grid,
            cursor: (0, 0),
            cursor_visible: false,
        };
    }
    let visible = rect.height - 1;
    let selection = base_cell(&resolved_style(view.selection_style, theme).unwrap_or_else(plain));
    let highlight = resolved_style(view.match_style, theme).map_or_else(plain, |style| TmuxStyle {
        fg: style.fg.or(Some(TmuxColour::Default)),
        bg: style.bg.or(Some(TmuxColour::Default)),
        attributes: style.attributes,
        ..TmuxStyle::default()
    });
    let base = plain();
    for index in 0..visible {
        let row_index = usize::from(index).saturating_add(view.offset as usize);
        let Some(row) = view.rows.get(row_index) else {
            break;
        };
        if u32::from(index).saturating_add(view.offset) == view.selected {
            grid.fill(0, index, rect.width, &Paint::Style(cleared_to(&selection)));
            grid.markup(0, index, rect.width, row, &selection, false);
        } else {
            grid.markup(0, index, rect.width, row, &base, false);
        }
        for column in view.matches.get(row_index).into_iter().flatten() {
            if *column < rect.width {
                grid.restyle(*column, index, &Paint::Style(highlight.clone()));
            }
        }
    }
    let prompt_row = rect.height - 1;
    let style = base_cell(&resolved_style(view.prompt_style, theme).unwrap_or_else(plain));
    grid.text(0, prompt_row, view.prompt, &Paint::Style(style), rect.width);
    ModeSurface {
        grid,
        cursor: (
            view.prompt_cursor.min(rect.width.saturating_sub(1)),
            prompt_row,
        ),
        cursor_visible: true,
    }
}

const CELL_BORDERS: [char; 13] = [
    ' ', 'x', 'q', 'l', 'k', 'm', 'j', 'w', 'v', 't', 'u', 'n', '~',
];

struct PanesView<'a> {
    areas: &'a [PanesModeArea],
    borders: &'a [PanesModeBorder],
    border_style: &'a str,
    copy: bool,
    format: bool,
    clears: &'a [PanesModeClear],
}

fn panes_label(grid: &mut Grid, area: &PanesModeArea, base: &TmuxStyle) -> Option<(u16, u16)> {
    if area.label.is_empty() || area.width == 0 {
        return None;
    }
    let mut sections: [Vec<StyledSegment>; 4] = Default::default();
    for segment in parse_styled_segments(&area.label) {
        let index = match segment.style.align {
            Some(TmuxAlign::Centre) => 1,
            Some(TmuxAlign::Right) => 2,
            Some(TmuxAlign::AbsoluteCentre) => 3,
            _ => 0,
        };
        sections[index].push(segment);
    }
    let available = area.width;
    let [left, centre, right, absolute] = sections.each_ref().map(|section| {
        u16::try_from(segments_width(section))
            .unwrap_or(u16::MAX)
            .min(available)
    });
    let middle = left + available.saturating_sub(right).saturating_sub(left) / 2;
    let columns = [
        0,
        middle.saturating_sub(centre / 2),
        available - right,
        (available - absolute) / 2,
    ];
    for ((section, column), width) in sections
        .iter()
        .zip(columns)
        .zip([left, centre, right, absolute])
    {
        if width > 0 {
            grid.segments(area.x + column, area.y, width, section, base, false);
        }
    }
    Some((area.x, area.y))
}

fn panes_number(
    grid: &mut Grid,
    area: &PanesModeArea,
    format: bool,
    theme: &ThemeColours,
) -> (u16, u16) {
    let colour = resolve(&area.colour, theme);
    let text = foreground(colour);
    let digits = area.number.to_string();
    let length = u16::try_from(digits.len()).unwrap_or(u16::MAX);
    let letter = (area.number > 9 && area.number < 35)
        .then(|| char::from(b'a' + u8::try_from(area.number - 10).unwrap_or(0)).to_string());
    let (x, y, sx, sy) = (area.x, area.y, area.width, area.height);
    if sx < length {
        return (x, y);
    }
    let mut width = length.saturating_mul(PITCH).saturating_sub(1);
    if sx < width || sy < if format { 7 } else { 5 } {
        width = length;
        if letter.is_some() && sx >= length + 2 {
            width += 2;
        }
        let column = x + (sx - width) / 2;
        let row = y + sy / 2;
        let paint = Paint::Style(text.clone());
        let mut cursor = (column + grid.text(column, row, &digits, &paint, sx), row);
        if width > length
            && let Some(letter) = &letter
        {
            cursor.0 += grid.text(cursor.0, row, &format!(" {letter}"), &paint, sx);
        }
        if format && sy > 1 {
            return panes_label(grid, area, &text).unwrap_or(cursor);
        }
        return cursor;
    }
    let mut column = x + (sx - width) / 2;
    let row = y + (sy - GLYPH) / 2;
    let paint = Paint::Style(TmuxStyle {
        fg: Some(TmuxColour::Default),
        bg: Some(colour),
        ..TmuxStyle::default()
    });
    let mut cursor = (x, y);
    for character in digits.chars() {
        let Some(index) = clock_index(character) else {
            continue;
        };
        for down in 0..GLYPH {
            for across in 0..GLYPH {
                if CLOCK_TABLE[index][usize::from(down)][usize::from(across)] {
                    grid.text(column + across, row + down, " ", &paint, 1);
                    cursor = (column + across + 1, row + down);
                }
            }
        }
        column += PITCH;
    }
    if sy <= 6 {
        return cursor;
    }
    if let Some(label) = panes_label(grid, area, &text) {
        cursor = label;
    }
    if let Some(letter) = &letter {
        grid.text(column - 2, row + GLYPH, letter, &Paint::Style(text), 1);
        cursor = (column - 1, row + GLYPH);
    }
    cursor
}

fn panes_clear(grid: &mut Grid, view: &PanesView<'_>, before: usize, rect: Rect) {
    let blank = Paint::Style(plain());
    for clear in view.clears {
        if usize::try_from(clear.before).unwrap_or(usize::MAX) != before || clear.x >= rect.width {
            continue;
        }
        let width = clear.width.min(rect.width - clear.x);
        for row in clear.y..clear.y.saturating_add(clear.height).min(rect.height) {
            grid.fill(clear.x, row, width, &blank);
        }
    }
}

fn panes_surface(view: &PanesView<'_>, rect: Rect, theme: &ThemeColours) -> ModeSurface {
    let mut grid = Grid::new(rect.width, rect.height);
    let mut cursor = (0, 0);
    for (index, area) in view.areas.iter().enumerate() {
        panes_clear(&mut grid, view, index, rect);
        if area.x >= rect.width || area.y >= rect.height {
            continue;
        }
        let width = area.width.min(rect.width - area.x);
        let height = area.height.min(rect.height - area.y);
        if let Some(viewport) = &area.viewport {
            if view.copy {
                grid.copy(area.x, area.y, width, height, viewport);
            } else {
                grid.preview(area.x, area.y, width, height, viewport);
            }
        }
        let clipped = PanesModeArea {
            pane: area.pane,
            number: area.number,
            x: area.x,
            y: area.y,
            width,
            height,
            colour: area.colour.clone(),
            label: area.label.clone(),
            viewport: None,
        };
        cursor = panes_number(&mut grid, &clipped, view.format, theme);
    }
    panes_clear(&mut grid, view, view.areas.len(), rect);
    let border = Paint::Style(resolved_style(view.border_style, theme).unwrap_or_else(plain));
    for cell in view.borders {
        let glyph = acs_glyph(CELL_BORDERS[usize::from(cell.cell).min(12)]);
        grid.text(cell.x, cell.y, &glyph.to_string(), &border, 1);
        cursor = (cell.x + 1, cell.y);
    }
    ModeSurface {
        grid,
        cursor,
        cursor_visible: false,
    }
}

pub(super) fn surface(mode: &PaneMode, rect: Rect, theme: &ThemeColours) -> ModeSurface {
    match mode {
        PaneMode::Panes {
            areas,
            borders,
            border_style,
            copy,
            format,
            clears,
        } => panes_surface(
            &PanesView {
                areas,
                borders,
                border_style,
                copy: *copy,
                format: *format,
                clears,
            },
            rect,
            theme,
        ),
        PaneMode::Customize {
            state,
            presentation,
            offset,
            prompt,
            prompt_cursor,
            prompt_top,
        } => {
            let prompt =
                (!prompt.is_empty()).then_some((prompt.as_str(), *prompt_cursor, *prompt_top));
            let (grid, (x, y, cursor_visible)) =
                super::chooser::customize_surface(state, presentation, *offset, prompt, rect);
            ModeSurface {
                grid,
                cursor: (x, y),
                cursor_visible,
            }
        }
        PaneMode::Clock { time, colour } => clock_surface(time, colour, rect, theme),
        PaneMode::Switch {
            rows,
            selected,
            offset,
            selection_style,
            prompt,
            prompt_style,
            prompt_cursor,
            matches,
            match_style,
            ..
        } => switch_surface(
            &SwitchView {
                rows,
                selected: *selected,
                offset: *offset,
                selection_style,
                prompt,
                prompt_style,
                prompt_cursor: *prompt_cursor,
                matches,
                match_style,
            },
            rect,
            theme,
        ),
    }
}

impl Renderer {
    pub(super) fn paint_pane_mode(&mut self, mode: &PaneMode, entry: &PaneRect, model: &Model) {
        let rect = entry.content();
        let source = entry.source;
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let reaches_edge = rect.x.saturating_add(rect.width) >= model.size.columns;
        let whole = entry.mode_rect();
        let grid = surface(mode, whole, &model.status.theme).grid;
        let grid = if (whole.width, whole.height) == (rect.width, rect.height) {
            grid
        } else {
            grid.cropped(source.0, source.1, rect.width, rect.height)
        };
        grid.emit_into(
            &mut self.output,
            rect.x,
            rect.y,
            &model.status.theme,
            &model.appearance,
            Trailing::Pane { reaches_edge },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Rect;
    use zz_protocol::{PaneMode, ThemeColours};
    use zz_terminal::TerminalAppearance;

    #[test]
    fn the_panes_mode_draws_digits_labels_and_borders_and_parks_the_cursor_like_the_pin() {
        let area = |pane, number, x, width, colour: &str, label: &str| zz_protocol::PanesModeArea {
            pane: zz_protocol::PaneId(pane),
            number,
            x,
            y: 0,
            width,
            height: 23,
            colour: colour.to_owned(),
            label: label.to_owned(),
            viewport: None,
        };
        let mode = PaneMode::Panes {
            areas: vec![
                area(0, 0, 0, 40, "red", "#[align=right]40x23"),
                area(1, 11, 41, 39, "blue", "#[align=right]39x23"),
            ],
            borders: (0..23)
                .map(|y| zz_protocol::PanesModeBorder { x: 40, y, cell: 1 })
                .collect(),
            border_style: "bg=colour235,fg=colour250".to_owned(),
            copy: true,
            format: true,
            clears: Vec::new(),
        };
        let rect = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 23,
        };
        let theme = ThemeColours::default();
        let drawn = surface(&mode, rect, &theme);
        assert_eq!(drawn.cursor, (41, 22));
        assert!(!drawn.cursor_visible);
        let mut output = Vec::new();
        drawn.grid.emit_into(
            &mut output,
            0,
            0,
            &theme,
            &TerminalAppearance::default(),
            Trailing::Pane { reaches_edge: true },
        );
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("40x23"), "{text}");
        assert!(text.contains("39x23"), "{text}");
        assert!(text.contains('\u{2502}'), "{text}");
        assert!(
            text.contains('b'),
            "the letter under pane 11 is missing: {text}"
        );

        let single = PaneMode::Panes {
            areas: vec![area(0, 0, 0, 80, "red", "")],
            borders: Vec::new(),
            border_style: String::new(),
            copy: true,
            format: true,
            clears: Vec::new(),
        };
        assert_eq!(surface(&single, rect, &theme).cursor, (42, 13));
    }

    #[test]
    fn a_panes_label_places_each_aligned_section_like_format_draw() {
        let area = zz_protocol::PanesModeArea {
            pane: zz_protocol::PaneId(0),
            number: 0,
            x: 0,
            y: 0,
            width: 20,
            height: 10,
            colour: "red".to_owned(),
            label: "#[align=left]L#[align=centre]C#[align=right]R".to_owned(),
            viewport: None,
        };
        let mut grid = Grid::new(20, 10);
        panes_label(&mut grid, &area, &plain());
        assert_eq!(
            super::super::chooser::cell_text(&grid, 0),
            "L         C        R"
        );
    }

    #[test]
    fn a_switch_row_keeps_its_dim_runs_over_the_selection_style() {
        let mode = PaneMode::Switch {
            rows: vec!["cli #[dim]2 windows#[default] attached #[dim]win#[default]".to_owned()],
            selected: 0,
            offset: 0,
            selection_style: "noattr,bg=themeyellow,fg=themeblack".to_owned(),
            prompt: "(search) ".to_owned(),
            prompt_style: "bg=themeyellow,fg=themeblack".to_owned(),
            prompt_cursor: 9,
            matches: Vec::new(),
            match_style: String::new(),
            prompt_shape: zz_protocol::PromptCursor::default(),
        };
        let rect = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 4,
        };
        let theme = ThemeColours::default();
        let appearance = TerminalAppearance::default();
        let mut output = Vec::new();
        surface(&mode, rect, &theme).grid.emit_into(
            &mut output,
            0,
            0,
            &theme,
            &appearance,
            Trailing::Pane { reaches_edge: true },
        );
        let text = String::from_utf8_lossy(&output).replace('\u{1b}', "ESC");
        assert!(text.contains("ESC[2m"), "the row lost its dim runs: {text}");
        assert!(
            text.contains("ESC[38;2;13;13;13mESC[48;2;184;134;11mcli "),
            "the row lost the selection style: {text}"
        );
    }
}
