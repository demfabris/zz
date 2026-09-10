use std::cmp::Ordering;

use unicode_width::UnicodeWidthChar as _;
use zz_protocol::{
    ModePresentation, PaneId, StyledSegment, ThemeColours, TmuxColour, TmuxStyle, parse_style,
};
use zz_terminal::{OVERLAY_RECTANGLE, OverlayKind, TerminalMode, TerminalViewport};

use crate::state::Model;

const UNTOUCHED: char = '\u{e000}';

pub(crate) fn presentation<'a>(
    model: &'a Model,
    pane: PaneId,
    viewport: &TerminalViewport,
) -> Option<&'a ModePresentation> {
    let view = match viewport.mode {
        TerminalMode::Copy {
            hide_position: false,
            ..
        } => false,
        TerminalMode::View { .. } => true,
        _ => return None,
    };
    model
        .status
        .modes
        .iter()
        .find(|mode| mode.pane == pane && mode.view == view)
}

pub(crate) fn resolved_style(value: &str, theme: &ThemeColours) -> Option<TmuxStyle> {
    if value.is_empty() {
        return None;
    }
    let mut style = parse_style(value)?;
    for slot in [&mut style.fg, &mut style.bg, &mut style.us] {
        if let Some(TmuxColour::Theme(index)) = slot
            && let Some(resolved) = theme.slot(*index)
        {
            *slot = Some(resolved);
        }
    }
    Some(style)
}

fn grounded(mut style: TmuxStyle) -> TmuxStyle {
    style.fg = Some(style.fg.unwrap_or(TmuxColour::Default));
    style.bg = Some(style.bg.unwrap_or(TmuxColour::Default));
    style
}

pub(crate) fn position_runs(mode: &ModePresentation, width: u16) -> Vec<(u16, Vec<StyledSegment>)> {
    if mode.position.is_empty() || width == 0 {
        return Vec::new();
    }
    let underlay = vec![UNTOUCHED.to_string(); usize::from(width)];
    let composed =
        zz_client::compose_status_row_over(&mode.position, &underlay, &mode.position_style);
    let mut runs: Vec<(u16, Vec<StyledSegment>)> = Vec::new();
    let mut column = 0_u16;
    let mut open = false;
    for segment in composed.segments {
        let style = grounded(segment.style);
        for character in segment.text.chars() {
            if character == UNTOUCHED {
                open = false;
                column = column.saturating_add(1);
                continue;
            }
            if !open {
                runs.push((column, Vec::new()));
                open = true;
            }
            let Some((_, run)) = runs.last_mut() else {
                continue;
            };
            match run.last_mut().filter(|last| last.style == style) {
                Some(last) => last.text.push(character),
                None => run.push(StyledSegment {
                    text: character.to_string(),
                    style: style.clone(),
                }),
            }
            let advance = u16::try_from(character.width().unwrap_or(0)).unwrap_or(0);
            column = column.saturating_add(advance);
        }
    }
    runs
}

pub(crate) fn message_style(model: &Model, command: bool) -> TmuxStyle {
    let value = if command {
        &model.status.message_command_style
    } else {
        &model.status.message_style
    };
    grounded(parse_style(value).unwrap_or_default())
}

pub(crate) fn message_front(text: &str, width: u16, style: &TmuxStyle) -> Vec<StyledSegment> {
    let width = usize::from(width);
    let mut used = 0_usize;
    let visible = text
        .chars()
        .take_while(|character| {
            let next = used.saturating_add(character.width().unwrap_or(0));
            if next > width {
                false
            } else {
                used = next;
                true
            }
        })
        .collect::<String>();
    if visible.is_empty() {
        return Vec::new();
    }
    vec![StyledSegment {
        text: visible,
        style: style.clone(),
    }]
}

fn cells_of(segments: &[StyledSegment], width: usize, cells: &mut [(String, TmuxStyle)]) {
    let mut column = 0_usize;
    for segment in segments {
        for character in segment.text.chars() {
            let advance = character.width().unwrap_or(0);
            if advance == 0 {
                if let Some((text, _)) = column.checked_sub(1).and_then(|last| cells.get_mut(last)) {
                    text.push(character);
                }
                continue;
            }
            if column + advance > width {
                return;
            }
            cells[column] = (character.to_string(), segment.style.clone());
            for tail in 1..advance {
                cells[column + tail] = (String::new(), segment.style.clone());
            }
            column += advance;
        }
    }
}

pub(crate) fn over_underlay(
    front: &[StyledSegment],
    fill: Option<TmuxColour>,
    underlay: &[StyledSegment],
    width: u16,
) -> Vec<StyledSegment> {
    let width = usize::from(width);
    let blank = grounded(TmuxStyle::default());
    let mut cells = vec![(" ".to_owned(), blank); width];
    match fill {
        Some(fill) => {
            let filled = TmuxStyle {
                fg: Some(TmuxColour::Default),
                bg: Some(fill),
                ..TmuxStyle::default()
            };
            for cell in &mut cells {
                cell.1 = filled.clone();
            }
        }
        None => cells_of(underlay, width, &mut cells),
    }
    let mut drawn = vec![(String::new(), TmuxStyle::default()); width];
    cells_of(front, width, &mut drawn);
    let front_width = drawn
        .iter()
        .rposition(|(text, _)| !text.is_empty())
        .map_or(0, |last| last + 1);
    for (column, cell) in drawn.into_iter().enumerate().take(front_width) {
        cells[column] = cell;
    }
    if let Some(cell) = cells.get_mut(front_width)
        && cell.0.is_empty()
    {
        " ".clone_into(&mut cell.0);
    }
    let mut segments: Vec<StyledSegment> = Vec::new();
    for (text, style) in cells {
        if text.is_empty() {
            continue;
        }
        match segments.last_mut().filter(|last| last.style == style) {
            Some(last) => last.text.push_str(&text),
            None => segments.push(StyledSegment { text, style }),
        }
    }
    segments
}

pub(crate) fn emacs_selection_trim(viewport: &TerminalViewport) -> Option<(u16, u16)> {
    let bottom = viewport
        .overlays
        .iter()
        .filter(|overlay| overlay.kind() == OverlayKind::Selection)
        .max_by_key(|overlay| overlay.row)?;
    if bottom.flags() & OVERLAY_RECTANGLE != 0 || bottom.end <= bottom.start {
        return None;
    }
    if bottom.row.saturating_add(1) >= viewport.rows && bottom.end >= viewport.columns {
        return None;
    }
    let cursor = viewport
        .overlays
        .iter()
        .find(|overlay| overlay.kind() == OverlayKind::CopyCursor)?;
    let end = match cursor.row.cmp(&bottom.row) {
        Ordering::Equal if cursor.start.saturating_add(1) == bottom.end => {
            if cursor.start == 0 {
                return None;
            }
            cursor.start
        }
        Ordering::Equal if cursor.start == bottom.start => bottom.end - 1,
        Ordering::Less => bottom.end - 1,
        Ordering::Equal | Ordering::Greater => return None,
    };
    Some((bottom.row, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(position: &str, style: &str) -> ModePresentation {
        ModePresentation {
            pane: PaneId(1),
            view: false,
            position: position.to_owned(),
            position_style: style.to_owned(),
            selection_style: String::new(),
            vi_keys: false,
        }
    }

    #[test]
    fn a_right_aligned_position_draws_only_its_own_cells() {
        let runs = position_runs(
            &mode("#[align=right][0/45]", "bg=themeyellow,fg=themeblack"),
            20,
        );
        assert_eq!(runs.len(), 1);
        let (column, segments) = &runs[0];
        assert_eq!(*column, 14);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "[0/45]");
        assert_eq!(segments[0].style.bg, Some(TmuxColour::Theme(5)));
        assert_eq!(segments[0].style.fg, Some(TmuxColour::Theme(0)));
    }

    #[test]
    fn an_empty_position_draws_nothing() {
        assert!(position_runs(&mode("", "bg=red"), 20).is_empty());
        assert!(position_runs(&mode("[0/0]", ""), 0).is_empty());
    }

    fn plain(segments: &[StyledSegment]) -> String {
        segments.iter().map(|segment| segment.text.as_str()).collect()
    }

    #[test]
    fn a_filled_message_clears_the_row_to_the_fill_with_the_default_foreground() {
        let style = grounded(parse_style("bg=themeyellow,fg=themeblack,fill=themeyellow").expect("style"));
        let front = message_front("hello", 8, &style);
        let under = vec![StyledSegment {
            text: "STATUSROW".to_owned(),
            style: TmuxStyle::default(),
        }];
        let segments = over_underlay(&front, style.fill, &under, 8);
        assert_eq!(plain(&segments), "hello   ");
        assert_eq!(segments[0].style, style);
        assert_eq!(segments[1].style.fg, Some(TmuxColour::Default));
        assert_eq!(segments[1].style.bg, Some(TmuxColour::Theme(5)));
    }

    #[test]
    fn an_unfilled_message_keeps_the_row_underneath_past_its_text() {
        let style = grounded(parse_style("bg=red,fg=white").expect("style"));
        let front = message_front("ab", 6, &style);
        let under = vec![StyledSegment {
            text: "STATUS".to_owned(),
            style: TmuxStyle::default(),
        }];
        assert_eq!(plain(&over_underlay(&front, None, &under, 6)), "abATUS");
        let virtual_row = over_underlay(&front, None, &[], 4);
        assert_eq!(plain(&virtual_row), "ab  ");
        assert_eq!(virtual_row[1].style.bg, Some(TmuxColour::Default));
        assert_eq!(plain(&message_front("toolong", 3, &style)), "too");
    }

    #[test]
    fn a_selection_style_resolves_through_the_published_theme() {
        let theme = ThemeColours::default();
        let style = resolved_style("bg=themeyellow,fg=themeblack", &theme).expect("style");
        assert_eq!(style.bg, theme.slot(5));
        assert_eq!(style.fg, theme.slot(0));
        assert!(resolved_style("", &theme).is_none());
    }

    fn selection_viewport(overlays: Vec<zz_terminal::OverlaySpan>) -> TerminalViewport {
        let mut viewport = TerminalViewport::blank(80, 24, zz_terminal::SessionStatus::Running);
        viewport.overlays = std::sync::Arc::from(overlays);
        viewport
    }

    #[test]
    fn emacs_drops_the_bottom_right_cell_the_way_screen_check_selection_does() {
        use zz_terminal::OverlaySpan;
        let downward = selection_viewport(vec![
            OverlaySpan::new(3, 5, 80, OverlayKind::Selection),
            OverlaySpan::new(4, 0, 8, OverlayKind::Selection),
            OverlaySpan::new(4, 7, 8, OverlayKind::CopyCursor),
        ]);
        assert_eq!(emacs_selection_trim(&downward), Some((4, 7)));
        let upward = selection_viewport(vec![
            OverlaySpan::new(3, 2, 80, OverlayKind::Selection),
            OverlaySpan::new(4, 0, 6, OverlayKind::Selection),
            OverlaySpan::new(3, 2, 3, OverlayKind::CopyCursor),
        ]);
        assert_eq!(emacs_selection_trim(&upward), Some((4, 5)));
        let leftward = selection_viewport(vec![
            OverlaySpan::new(4, 2, 7, OverlayKind::Selection),
            OverlaySpan::new(4, 2, 3, OverlayKind::CopyCursor),
        ]);
        assert_eq!(emacs_selection_trim(&leftward), Some((4, 6)));
        let column_zero = selection_viewport(vec![
            OverlaySpan::new(3, 5, 80, OverlayKind::Selection),
            OverlaySpan::new(4, 0, 1, OverlayKind::Selection),
            OverlaySpan::new(4, 0, 1, OverlayKind::CopyCursor),
        ]);
        assert_eq!(emacs_selection_trim(&column_zero), None);
        let rectangle = selection_viewport(vec![
            OverlaySpan::with_flags(4, 2, 7, OverlayKind::Selection, OVERLAY_RECTANGLE),
            OverlaySpan::new(4, 6, 7, OverlayKind::CopyCursor),
        ]);
        assert_eq!(emacs_selection_trim(&rectangle), None);
    }
}
