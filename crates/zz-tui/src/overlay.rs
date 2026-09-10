use unicode_width::UnicodeWidthChar as _;
use zz_daemon::InteractiveClient;
use zz_protocol::{
    DisplayPanesAction, DisplayPanesState, InputMessage, PaneIndicator, StyledSegment, TmuxColour,
    TmuxStyle,
};

use crate::{
    layout::Rect,
    state::Model,
    terminal_event::{Event, KeyEventKind, MouseEventKind},
    tty::TerminalSize,
};

const UNWRITTEN: char = '\u{0}';

pub(crate) fn close_display_panes_on_resize(
    model: &Model,
    client: &InteractiveClient,
    previous: TerminalSize,
) -> Result<(), String> {
    let resized = previous.columns != model.size.columns || previous.rows != model.size.rows;
    if resized && model.display_panes.is_some() {
        client
            .send_input(InputMessage::DisplayPanes {
                action: DisplayPanesAction::Dismiss,
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn dismiss_client_message(
    model: &Model,
    client: &InteractiveClient,
    event: &Event,
) -> Result<(), String> {
    let pressed = match event {
        Event::Key(key) => key.kind != KeyEventKind::Release,
        Event::Mouse(mouse) => matches!(mouse.kind, MouseEventKind::Down(_)),
        _ => false,
    };
    let owned_here = model.menu.is_some()
        || model.confirm.is_some()
        || model.popup.is_some()
        || model.display_panes.is_some();
    let from_daemon = model
        .client_message
        .as_ref()
        .is_some_and(|message| message.id.is_some());
    if pressed && owned_here && from_daemon {
        client
            .send_input(InputMessage::DismissClientMessage)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PromptView {
    pub text: String,
    pub cursor: u16,
}

pub(crate) fn prompt_view(prompt: &str, input: &str, index: u32, width: u16) -> PromptView {
    let area = usize::from(width);
    let mut text = String::new();
    let mut start = 0;
    for character in prompt.chars() {
        let cells = character.width().unwrap_or(0);
        if start + cells > area {
            break;
        }
        text.push(character);
        start += cells;
    }
    let left = area - start;
    if left == 0 {
        return PromptView {
            text,
            cursor: u16::try_from(start).unwrap_or(u16::MAX),
        };
    }
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    let before: usize = input
        .chars()
        .take(index)
        .map(|character| character.width().unwrap_or(0))
        .sum();
    let offset = if before >= left { before - left + 1 } else { 0 };
    let mut drawn = 0;
    let mut position = 0;
    for character in input.chars() {
        let cells = character.width().unwrap_or(0);
        if position >= offset {
            if drawn + cells > left {
                break;
            }
            text.push(character);
            drawn += cells;
        }
        position += cells;
    }
    PromptView {
        text,
        cursor: u16::try_from(start + before - offset).unwrap_or(u16::MAX),
    }
}

pub(crate) fn compose_over(
    expanded: &str,
    width: u16,
    base_style: &str,
    underlay: &str,
    underlay_style: &TmuxStyle,
) -> Vec<StyledSegment> {
    let marks = vec![UNWRITTEN.to_string(); usize::from(width)];
    let composed = zz_client::compose_status_row_over(expanded, &marks, base_style);
    let mut segments: Vec<StyledSegment> = Vec::new();
    let mut push = |text: &str, style: &TmuxStyle| match segments.last_mut() {
        Some(last) if last.style == *style => last.text.push_str(text),
        _ => segments.push(StyledSegment {
            text: text.to_owned(),
            style: style.clone(),
        }),
    };
    for segment in composed.segments {
        for character in segment.text.chars() {
            if character == UNWRITTEN {
                push(underlay, underlay_style);
            } else {
                push(character.encode_utf8(&mut [0; 4]), &segment.style);
            }
        }
    }
    segments
}

pub(crate) fn written_runs(
    expanded: &str,
    width: u16,
    base_style: &str,
) -> Vec<(u16, StyledSegment)> {
    let marks = vec![UNWRITTEN.to_string(); usize::from(width)];
    let composed = zz_client::compose_status_row_over(expanded, &marks, base_style);
    let mut runs: Vec<(u16, StyledSegment)> = Vec::new();
    let mut column = 0_u16;
    let mut open = false;
    for segment in composed.segments {
        for character in segment.text.chars() {
            if character == UNWRITTEN {
                open = false;
                column = column.saturating_add(1);
                continue;
            }
            let cells = u16::try_from(character.width().unwrap_or(0)).unwrap_or(0);
            match runs.last_mut() {
                Some((_, last)) if open && last.style == segment.style => {
                    last.text.push(character);
                }
                _ => runs.push((
                    column,
                    StyledSegment {
                        text: character.to_string(),
                        style: segment.style.clone(),
                    },
                )),
            }
            open = true;
            column = column.saturating_add(cells);
        }
    }
    runs
}

const CLOCK_DIGITS: [[[bool; 5]; 5]; 10] = {
    const O: bool = true;
    const X: bool = false;
    [
        [
            [O, O, O, O, O],
            [O, X, X, X, O],
            [O, X, X, X, O],
            [O, X, X, X, O],
            [O, O, O, O, O],
        ],
        [
            [X, X, X, X, O],
            [X, X, X, X, O],
            [X, X, X, X, O],
            [X, X, X, X, O],
            [X, X, X, X, O],
        ],
        [
            [O, O, O, O, O],
            [X, X, X, X, O],
            [O, O, O, O, O],
            [O, X, X, X, X],
            [O, O, O, O, O],
        ],
        [
            [O, O, O, O, O],
            [X, X, X, X, O],
            [O, O, O, O, O],
            [X, X, X, X, O],
            [O, O, O, O, O],
        ],
        [
            [O, X, X, X, O],
            [O, X, X, X, O],
            [O, O, O, O, O],
            [X, X, X, X, O],
            [X, X, X, X, O],
        ],
        [
            [O, O, O, O, O],
            [O, X, X, X, X],
            [O, O, O, O, O],
            [X, X, X, X, O],
            [O, O, O, O, O],
        ],
        [
            [O, O, O, O, O],
            [O, X, X, X, X],
            [O, O, O, O, O],
            [O, X, X, X, O],
            [O, O, O, O, O],
        ],
        [
            [O, O, O, O, O],
            [X, X, X, X, O],
            [X, X, X, X, O],
            [X, X, X, X, O],
            [X, X, X, X, O],
        ],
        [
            [O, O, O, O, O],
            [O, X, X, X, O],
            [O, O, O, O, O],
            [O, X, X, X, O],
            [O, O, O, O, O],
        ],
        [
            [O, O, O, O, O],
            [O, X, X, X, O],
            [O, O, O, O, O],
            [X, X, X, X, O],
            [O, O, O, O, O],
        ],
    ]
};

const THEME_RED: u8 = 6;
const THEME_BLUE: u8 = 7;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CellWrite {
    pub column: u16,
    pub row: u16,
    pub segment: StyledSegment,
}

pub(crate) fn display_panes_writes(
    state: &DisplayPanesState,
    indicator: &PaneIndicator,
    rect: Rect,
) -> Vec<CellWrite> {
    let colour = if indicator.active() {
        state.active_colour.unwrap_or(TmuxColour::Theme(THEME_RED))
    } else {
        state.colour.unwrap_or(TmuxColour::Theme(THEME_BLUE))
    };
    let foreground = TmuxStyle {
        fg: Some(colour),
        bg: Some(TmuxColour::Default),
        ..TmuxStyle::default()
    };
    let background = TmuxStyle {
        fg: Some(TmuxColour::Default),
        bg: Some(colour),
        ..TmuxStyle::default()
    };
    let digits = indicator.index.to_string();
    let length = u16::try_from(digits.len()).unwrap_or(u16::MAX);
    let (width, height) = (rect.width, rect.height);
    let mut writes = Vec::new();
    if width < length {
        return writes;
    }
    let letter = (10..35)
        .contains(&indicator.index)
        .then(|| char::from(b'a' + u8::try_from(indicator.index - 10).unwrap_or(0)));
    let letter_length = u16::from(letter.is_some());
    let centre_x = width / 2;
    let centre_y = height / 2;
    let put =
        |writes: &mut Vec<CellWrite>, column: u16, row: u16, text: String, style: &TmuxStyle| {
            if column < width && row < height {
                writes.push(CellWrite {
                    column: rect.x.saturating_add(column),
                    row: rect.y.saturating_add(row),
                    segment: StyledSegment {
                        text,
                        style: style.clone(),
                    },
                });
            }
        };
    if width < length.saturating_mul(6) || height < 5 {
        if width > length.saturating_add(letter_length) {
            let total = length + letter_length + 1;
            let column = centre_x.saturating_sub(total / 2);
            let mut text = digits;
            text.push(' ');
            if let Some(letter) = letter {
                text.push(letter);
            }
            put(&mut writes, column, centre_y, text, &foreground);
        } else {
            let column = centre_x.saturating_sub(length / 2);
            put(&mut writes, column, centre_y, digits, &foreground);
        }
        return writes;
    }
    let mut left = centre_x.saturating_sub(length.saturating_mul(3));
    let top = centre_y.saturating_sub(2);
    for digit in digits.bytes() {
        let glyph = &CLOCK_DIGITS[usize::from(digit - b'0')];
        for (line, cells) in glyph.iter().enumerate() {
            for (offset, lit) in cells.iter().enumerate() {
                if *lit {
                    put(
                        &mut writes,
                        left + u16::try_from(offset).unwrap_or(0),
                        top + u16::try_from(line).unwrap_or(0),
                        " ".to_owned(),
                        &background,
                    );
                }
            }
        }
        left = left.saturating_add(6);
    }
    if height <= 6 {
        return writes;
    }
    for (column, segment) in written_runs(&indicator.label, width, "") {
        let mut style = segment.style;
        if matches!(style.fg, None | Some(TmuxColour::Default)) {
            style.fg = Some(colour);
        }
        if style.bg.is_none() {
            style.bg = Some(TmuxColour::Default);
        }
        put(&mut writes, column, 0, segment.text, &style);
    }
    if let Some(letter) = letter {
        let column = (width / 2 + length * 3).saturating_sub(letter_length + 1);
        put(
            &mut writes,
            column,
            top + 5,
            letter.to_string(),
            &foreground,
        );
    }
    writes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_prompt_keeps_the_cursor_after_the_input() {
        let view = prompt_view(":", "list", 4, 80);
        assert_eq!(view.text, ":list");
        assert_eq!(view.cursor, 5);
    }

    #[test]
    fn a_long_prompt_scrolls_so_the_cursor_stays_on_the_last_column() {
        let input = "x".repeat(100) + "END";
        let view = prompt_view(":", &input, 103, 80);
        assert_eq!(view.cursor, 79);
        assert!(view.text.ends_with("END"));
        assert_eq!(view.text.chars().count(), 79);
    }

    #[test]
    fn a_long_prompt_with_the_cursor_home_draws_from_the_start() {
        let input = "y".repeat(100);
        let view = prompt_view(":", &input, 3, 80);
        assert_eq!(view.cursor, 4);
        assert_eq!(view.text.chars().count(), 80);
    }

    #[test]
    fn a_prompt_wider_than_the_area_puts_the_cursor_at_its_end() {
        let view = prompt_view("abcdef", "zz", 2, 4);
        assert_eq!(view.text, "abcd");
        assert_eq!(view.cursor, 4);
    }

    fn indicator(index: u32, active: bool) -> PaneIndicator {
        PaneIndicator {
            pane: zz_protocol::PaneId(u64::from(index)),
            index,
            select_key: b'0',
            flags: if active { PaneIndicator::ACTIVE } else { 0 },
            label: String::new(),
        }
    }

    fn state() -> DisplayPanesState {
        DisplayPanesState {
            window: zz_protocol::WindowId(0),
            duration_ms: 0,
            indicators: Vec::new(),
            colour: None,
            active_colour: Some(TmuxColour::Basic(1)),
        }
    }

    #[test]
    fn a_large_pane_gets_the_clock_digit_centred_in_the_active_colour() {
        let rect = Rect {
            x: 10,
            y: 0,
            width: 40,
            height: 23,
        };
        let writes = display_panes_writes(&state(), &indicator(1, true), rect);
        assert_eq!(writes.len(), 5);
        assert!(writes.iter().all(|write| write.column == 10 + 20 - 3 + 4));
        assert_eq!(writes[0].row, 11 - 2);
        assert_eq!(writes[0].segment.style.bg, Some(TmuxColour::Basic(1)));
    }

    #[test]
    fn a_small_pane_gets_the_plain_number_in_the_inactive_colour() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 3,
        };
        let writes = display_panes_writes(&state(), &indicator(0, false), rect);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].segment.text, "0 ");
        assert_eq!(
            writes[0].segment.style.fg,
            Some(TmuxColour::Theme(THEME_BLUE))
        );
    }
}
