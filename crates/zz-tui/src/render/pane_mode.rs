use zz_protocol::{PaneMode, ThemeColours, TmuxColour, TmuxStyle, parse_tmux_colour};

use super::{Renderer, write_cursor_position, write_tmux_sgr};
use crate::{layout::Rect, state::Model};

/// `window_clock_table`: five rows of five columns per glyph, digits first,
/// then `:`, `A`, `P` and `M`.
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

/// One cell the mode paints, in the pane's own coordinates.
struct ModeCell {
    column: u16,
    row: u16,
    glyph: &'static str,
    style: TmuxStyle,
}

/// What a client draws for a pane holding a server-owned mode: the cells over
/// a cleared pane, and where the pin's own writer left the cursor.
pub(super) struct ModeSurface {
    cells: Vec<ModeCell>,
    /// `s->cx`/`s->cy` after `screen_write_stop`, in the pane's coordinates.
    pub(super) cursor: (u16, u16),
}

/// The index of a character in `window_clock_table`, or `None` for one
/// `window_clock_draw_screen` skips while still advancing a glyph.
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

/// `style_parse_colour` on the `clock-mode-colour` option, with a theme slot
/// resolved through the theme the daemon published.
fn resolve(colour: &str, theme: &ThemeColours) -> TmuxColour {
    let parsed = parse_tmux_colour(colour).unwrap_or(TmuxColour::Default);
    match parsed {
        TmuxColour::Theme(index) => theme.slot(index).unwrap_or(TmuxColour::Default),
        other => other,
    }
}

/// `window_clock_draw_screen`. The clock is centred in the pane and drawn from
/// `window_clock_table` with the foreground and the background both the
/// resolved colour, which is what turns a `#` into a block. A pane too small
/// for the big face falls back to the plain string on one row.
fn clock_surface(time: &str, colour: &str, rect: Rect, theme: &ThemeColours) -> ModeSurface {
    let colour = resolve(colour, theme);
    let length = u16::try_from(time.chars().count()).unwrap_or(u16::MAX);
    if rect.width < PITCH.saturating_mul(length) || rect.height < 6 {
        if rect.width < length || rect.height == 0 {
            return ModeSurface {
                cells: Vec::new(),
                cursor: (0, 0),
            };
        }
        let column = rect.width / 2 - length / 2;
        let row = rect.height / 2;
        let style = foreground(colour);
        let cells = time
            .chars()
            .enumerate()
            .map(|(offset, character)| ModeCell {
                column: column.saturating_add(u16::try_from(offset).unwrap_or(u16::MAX)),
                row,
                glyph: character_glyph(character),
                style: style.clone(),
            })
            .collect();
        return ModeSurface {
            cells,
            cursor: (column.saturating_add(length), row),
        };
    }
    let mut column = rect.width / 2 - 3 * length;
    let row = rect.height / 2 - 3;
    let style = solid(colour);
    let mut cells = Vec::new();
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
                // `screen_write_cursormove` runs for every cell of the glyph
                // and `screen_write_putc` only for the set ones, so the cursor
                // lands past the last set cell of the last glyph.
                cursor = (x, y);
                if CLOCK_TABLE[index][usize::from(down)][usize::from(across)] {
                    cursor = (x.saturating_add(1), y);
                    cells.push(ModeCell {
                        column: x,
                        row: y,
                        glyph: "#",
                        style: style.clone(),
                    });
                }
            }
        }
        column = column.saturating_add(PITCH);
    }
    ModeSurface { cells, cursor }
}

fn character_glyph(character: char) -> &'static str {
    match character {
        '0' => "0",
        '1' => "1",
        '2' => "2",
        '3' => "3",
        '4' => "4",
        '5' => "5",
        '6' => "6",
        '7' => "7",
        '8' => "8",
        '9' => "9",
        ':' => ":",
        'A' => "A",
        'P' => "P",
        'M' => "M",
        _ => " ",
    }
}

/// The surface a pane's mode draws over its rect, or `None` when the pane
/// holds no server-owned mode.
pub(super) fn surface(mode: &PaneMode, rect: Rect, theme: &ThemeColours) -> ModeSurface {
    match mode {
        PaneMode::Clock { time, colour } => clock_surface(time, colour, rect, theme),
    }
}

impl Renderer {
    /// Draws a pane's server-owned mode over its rect. `window_pane_set_mode`
    /// gives the mode its own screen, so the pane's own content is gone for as
    /// long as the mode is up and the rect is cleared to the default cell the
    /// way `screen_write_clearscreen(&ctx, 8)` clears it.
    pub(super) fn paint_pane_mode(&mut self, mode: &PaneMode, rect: Rect, model: &Model) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let blank = " ".repeat(usize::from(rect.width));
        let cleared = TmuxStyle {
            fg: Some(TmuxColour::Default),
            bg: Some(TmuxColour::Default),
            ..TmuxStyle::default()
        };
        for row in 0..rect.height {
            write_cursor_position(&mut self.output, rect.x, rect.y.saturating_add(row));
            write_tmux_sgr(
                &mut self.output,
                &cleared,
                model.appearance.foreground,
                model.appearance.background,
                &model.appearance,
            );
            self.output.extend_from_slice(blank.as_bytes());
        }
        for cell in surface(mode, rect, &model.status.theme).cells {
            write_cursor_position(
                &mut self.output,
                rect.x.saturating_add(cell.column),
                rect.y.saturating_add(cell.row),
            );
            write_tmux_sgr(
                &mut self.output,
                &cell.style,
                model.appearance.foreground,
                model.appearance.background,
                &model.appearance,
            );
            self.output.extend_from_slice(cell.glyph.as_bytes());
        }
        self.output.extend_from_slice(b"\x1b[0m");
    }
}
