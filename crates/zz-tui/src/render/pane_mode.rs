use zz_protocol::{
    PaneMode, ThemeColours, TmuxAttributeState, TmuxColour, TmuxStyle, parse_tmux_colour,
};

use super::{
    Renderer,
    chooser::{Grid, Paint, Trailing, plain},
};
use crate::{layout::Rect, mode_view::resolved_style, state::Model};

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

fn switch_surface(
    rows: &[String],
    selected: u32,
    offset: u32,
    selection_style: &str,
    prompt: &str,
    prompt_style: &str,
    rect: Rect,
    theme: &ThemeColours,
) -> ModeSurface {
    let mut grid = Grid::new(rect.width, rect.height);
    if rect.height <= 1 {
        return ModeSurface {
            grid,
            cursor: (0, 0),
            cursor_visible: false,
        };
    }
    let visible = rect.height - 1;
    let selection = base_cell(&resolved_style(selection_style, theme).unwrap_or_else(plain));
    let base = plain();
    for index in 0..visible {
        let Some(row) = rows.get(usize::from(index).saturating_add(offset as usize)) else {
            break;
        };
        if u32::from(index).saturating_add(offset) == selected {
            grid.fill(0, index, rect.width, &Paint::Style(cleared_to(&selection)));
            grid.markup(0, index, rect.width, row, &selection, false);
        } else {
            let used = grid.markup(0, index, rect.width, row, &base, false);
            grid.extend_last_cell(used, index);
        }
    }
    let prompt_row = rect.height - 1;
    let style = base_cell(&resolved_style(prompt_style, theme).unwrap_or_else(plain));
    let used = grid.markup(0, prompt_row, rect.width, prompt, &style, false);
    ModeSurface {
        grid,
        cursor: (used.min(rect.width.saturating_sub(1)), prompt_row),
        cursor_visible: true,
    }
}

pub(super) fn surface(mode: &PaneMode, rect: Rect, theme: &ThemeColours) -> ModeSurface {
    match mode {
        PaneMode::Customize {
            state,
            presentation,
            offset,
        } => {
            let (grid, (x, y, cursor_visible)) =
                super::chooser::customize_surface(state, presentation, *offset, rect);
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
        } => switch_surface(
            rows,
            *selected,
            *offset,
            selection_style,
            prompt,
            prompt_style,
            rect,
            theme,
        ),
    }
}

impl Renderer {
    pub(super) fn paint_pane_mode(&mut self, mode: &PaneMode, rect: Rect, model: &Model) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let reaches_edge = rect.x.saturating_add(rect.width) >= model.size.columns;
        surface(mode, rect, &model.status.theme).grid.emit_into(
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
    fn a_switch_row_keeps_its_dim_runs_over_the_selection_style() {
        let mode = PaneMode::Switch {
            rows: vec!["cli #[dim]2 windows#[default] attached #[dim]win#[default]".to_owned()],
            selected: 0,
            offset: 0,
            selection_style: "noattr,bg=themeyellow,fg=themeblack".to_owned(),
            prompt: "(search) ".to_owned(),
            prompt_style: "bg=themeyellow,fg=themeblack".to_owned(),
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
