//! The shared status-row compositor: lays one daemon-expanded status row out
//! to a fixed display width the way tmux's `format_draw` does — alignment
//! sections, the window/pane/session list with focus-preserving truncation and
//! `<`/`>` markers, `fill=`, and the hit ranges mouse input routes through.

use std::ops::Range;

use unicode_width::UnicodeWidthChar as _;
use zz_protocol::{
    StyledSegment, TmuxAlign, TmuxAttributeState, TmuxColour, TmuxDefaultType, TmuxList, TmuxRange,
    TmuxStyle, apply_style, parse_style,
};

const LEFT: usize = 0;
const CENTRE: usize = 1;
const RIGHT: usize = 2;
const ABSOLUTE_CENTRE: usize = 3;
const LIST: usize = 4;
const LIST_LEFT: usize = 5;
const LIST_RIGHT: usize = 6;
const AFTER: usize = 7;
const TOTAL: usize = 8;

/// One clickable span of a composed row, in display columns of that row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusHitRange {
    pub columns: Range<u16>,
    pub target: TmuxRange,
}

/// One status row composed to a fixed width: styled runs whose display widths
/// sum to exactly that width, plus the hit ranges `format_draw` would report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposedStatusRow {
    pub segments: Vec<StyledSegment>,
    pub ranges: Vec<StatusHitRange>,
}

impl ComposedStatusRow {
    #[must_use]
    pub fn hit_target(&self, column: u16) -> Option<&TmuxRange> {
        self.ranges
            .iter()
            .find(|range| range.columns.contains(&column))
            .map(|range| &range.target)
    }
}

/// Composes one daemon-expanded status row. An empty or unparseable
/// `base_style` means the theme default, never an error.
#[must_use]
pub fn compose_status_row(expanded: &str, width: u16, base_style: &str) -> ComposedStatusRow {
    let base = parse_style(base_style).unwrap_or_default();
    compose(expanded, usize::from(width), &base, &[])
}

/// Composes one row over a per-column underlay whose length is the row width.
/// A column the format never writes keeps its underlay text instead of a
/// blank, the way `window_make_pane_status` fills the pane status screen with
/// border cells before `format_draw` writes `pane-border-format` over it.
#[must_use]
pub fn compose_status_row_over(
    expanded: &str,
    underlay: &[String],
    base_style: &str,
) -> ComposedStatusRow {
    let base = parse_style(base_style).unwrap_or_default();
    compose(expanded, underlay.len(), &base, underlay)
}

#[derive(Clone, Debug)]
enum Slot {
    Cell {
        text: String,
        width: usize,
        style: TmuxStyle,
    },
    Tail(TmuxStyle),
}

impl Slot {
    fn blank(style: TmuxStyle) -> Self {
        Self::Cell {
            text: " ".to_owned(),
            width: 1,
            style,
        }
    }

    fn style(&self) -> &TmuxStyle {
        match self {
            Self::Cell { style, .. } | Self::Tail(style) => style,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Screen {
    slots: Vec<Slot>,
}

impl Screen {
    fn cx(&self) -> usize {
        self.slots.len()
    }

    fn push(&mut self, text: &str, width: usize, style: &TmuxStyle) {
        self.slots.push(Slot::Cell {
            text: text.to_owned(),
            width,
            style: style.clone(),
        });
        for _ in 1..width {
            self.slots.push(Slot::Tail(style.clone()));
        }
    }

    fn push_zero_width(&mut self, character: char) {
        if let Some(Slot::Cell { text, .. }) = self
            .slots
            .iter_mut()
            .rev()
            .find(|slot| matches!(slot, Slot::Cell { .. }))
        {
            text.push(character);
        }
    }
}

#[derive(Clone, Debug)]
struct PendingRange {
    screen: usize,
    start: usize,
    end: usize,
    target: TmuxRange,
    placed: Option<Range<usize>>,
}

struct Walk {
    screens: [Screen; TOTAL],
    ranges: Vec<PendingRange>,
    list_align: usize,
    focus_start: isize,
    focus_end: isize,
    fill: Option<TmuxColour>,
    aborted: bool,
}

const fn align_index(align: Option<TmuxAlign>) -> usize {
    match align {
        None | Some(TmuxAlign::Default) => 0,
        Some(TmuxAlign::Left) => 1,
        Some(TmuxAlign::Centre) => 2,
        Some(TmuxAlign::Right) => 3,
        Some(TmuxAlign::AbsoluteCentre) => 4,
    }
}

fn range_matches(open: &TmuxRange, current: Option<&TmuxRange>) -> bool {
    match (open, current) {
        (TmuxRange::Left, Some(TmuxRange::Left))
        | (TmuxRange::Right, Some(TmuxRange::Right))
        | (TmuxRange::Control(_), Some(TmuxRange::Control(_))) => true,
        (TmuxRange::Pane(open), Some(TmuxRange::Pane(current)))
        | (TmuxRange::Window(open), Some(TmuxRange::Window(current)))
        | (TmuxRange::Session(open), Some(TmuxRange::Session(current))) => open == current,
        (TmuxRange::User(open), Some(TmuxRange::User(current))) => open == current,
        (
            TmuxRange::Other { kind, argument },
            Some(TmuxRange::Other {
                kind: current_kind,
                argument: current_argument,
            }),
        ) => kind == current_kind && argument == current_argument,
        _ => false,
    }
}

fn walk(expanded: &str) -> Walk {
    let bytes = expanded.as_bytes();
    let mut walked = Walk {
        screens: Default::default(),
        ranges: Vec::new(),
        list_align: 0,
        focus_start: -1,
        focus_end: -1,
        fill: None,
        aborted: false,
    };
    let mut map = [LEFT, LEFT, CENTRE, RIGHT, ABSOLUTE_CENTRE];
    let mut current = LEFT;
    let mut list_state: i8 = -1;
    let mut style = TmuxStyle::default();
    let mut base = TmuxStyle::default();
    let mut current_default = TmuxStyle::default();
    let mut open_range: Option<PendingRange> = None;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' && index + 1 < bytes.len() && bytes[index + 1] != b'[' {
            let mut hashes = 1;
            while bytes.get(index + hashes) == Some(&b'#') {
                hashes += 1;
            }
            let even = hashes % 2 == 0;
            if bytes.get(index + hashes) != Some(&b'[') {
                index += hashes;
                let count = if even { hashes / 2 } else { hashes / 2 + 1 };
                for _ in 0..count {
                    walked.screens[current].push("#", 1, &style);
                }
                continue;
            }
            if even {
                index += hashes + 1;
            } else {
                index += hashes - 1;
            }
            if style.ignore == Some(true) {
                continue;
            }
            for _ in 0..hashes / 2 {
                walked.screens[current].push("#", 1, &style);
            }
            if even {
                walked.screens[current].push("[", 1, &style);
            }
            continue;
        }
        if bytes[index] != b'#' || bytes.get(index + 1) != Some(&b'[') || style.ignore == Some(true)
        {
            let character = expanded[index..]
                .chars()
                .next()
                .expect("walk index is on a character boundary");
            index += character.len_utf8();
            if character.is_ascii() {
                if matches!(character, ' '..='~') {
                    walked.screens[current].push(character.encode_utf8(&mut [0; 4]), 1, &style);
                }
                continue;
            }
            match character.width().unwrap_or(0) {
                0 => walked.screens[current].push_zero_width(character),
                width => {
                    walked.screens[current].push(character.encode_utf8(&mut [0; 4]), width, &style);
                }
            }
            continue;
        }
        let marker_start = index + 2;
        let Some(relative_end) = expanded[marker_start..].find(']') else {
            walked.aborted = true;
            walked.ranges.clear();
            return walked;
        };
        let marker_end = marker_start + relative_end;
        index = marker_end + 1;
        let Some(delta) = parse_style(&expanded[marker_start..marker_end]) else {
            continue;
        };
        let saved = style.clone();
        apply_style(&mut style, &delta, &current_default);
        match delta.default_type {
            Some(TmuxDefaultType::Push) => current_default = saved,
            Some(TmuxDefaultType::Pop) => current_default.clone_from(&base),
            Some(TmuxDefaultType::Set) => {
                base.clone_from(&saved);
                current_default = saved;
            }
            None => {}
        }
        style.default_type = None;
        if let Some(fill) = style.fill {
            walked.fill = Some(fill);
        }
        match style.list.unwrap_or(TmuxList::Off) {
            TmuxList::On => {
                if list_state != 0 {
                    open_range = None;
                    list_state = 0;
                    walked.list_align = align_index(style.align);
                }
                if walked.focus_start != -1 && walked.focus_end == -1 {
                    walked.focus_end = to_isize(walked.screens[LIST].cx());
                }
                current = LIST;
            }
            TmuxList::Focus => {
                if list_state == 0 && walked.focus_start == -1 {
                    walked.focus_start = to_isize(walked.screens[LIST].cx());
                }
            }
            TmuxList::Off => {
                if list_state == 0 {
                    open_range = None;
                    if walked.focus_start != -1 && walked.focus_end == -1 {
                        walked.focus_end = to_isize(walked.screens[LIST].cx());
                    }
                    map[walked.list_align] = AFTER;
                    if walked.list_align == 1 {
                        map[0] = AFTER;
                    }
                    list_state = 1;
                }
                current = map[align_index(style.align)];
            }
            TmuxList::LeftMarker => {
                if list_state == 0 && walked.screens[LIST_LEFT].cx() == 0 {
                    open_range = None;
                    if walked.focus_start != -1 && walked.focus_end == -1 {
                        walked.focus_start = -1;
                        walked.focus_end = -1;
                    }
                    current = LIST_LEFT;
                }
            }
            TmuxList::RightMarker => {
                if list_state == 0 && walked.screens[LIST_RIGHT].cx() == 0 {
                    open_range = None;
                    if walked.focus_start != -1 && walked.focus_end == -1 {
                        walked.focus_start = -1;
                        walked.focus_end = -1;
                    }
                    current = LIST_RIGHT;
                }
            }
        }
        if let Some(open) = open_range.take() {
            if range_matches(&open.target, style.range.as_ref()) {
                open_range = Some(open);
            } else if walked.screens[current].cx() != open.start {
                walked.ranges.push(PendingRange {
                    end: walked.screens[current].cx() + 1,
                    ..open
                });
            }
        }
        if open_range.is_none()
            && let Some(target) = style.range.clone()
        {
            open_range = Some(PendingRange {
                screen: current,
                start: walked.screens[current].cx(),
                end: 0,
                target,
                placed: None,
            });
        }
    }
    walked
}

fn to_isize(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

/// A centre offset whose subtraction underflowed: tmux's `u_int` wraps and
/// `screen_write_cursormove` clamps to the last column, so the section is
/// clipped at the right edge and never overlaps the left.
fn clamped_offset(offset: Option<usize>, available: usize) -> usize {
    offset.unwrap_or_else(|| available.saturating_sub(1))
}

struct Draw {
    out: Vec<Option<Slot>>,
    screens: [Screen; TOTAL],
    ranges: Vec<PendingRange>,
    focus_start: isize,
    focus_end: isize,
}

impl Draw {
    fn cx(&self, screen: usize) -> usize {
        self.screens[screen].cx()
    }

    fn put(&mut self, screen: usize, offset: usize, start: usize, width: usize) {
        self.copy(screen, offset, start, width);
        for range in &mut self.ranges {
            if range.screen != screen || range.placed.is_some() {
                continue;
            }
            let clipped_start = range.start.max(start);
            let clipped_end = range.end.min(start + width);
            if clipped_start < clipped_end {
                range.placed = Some(clipped_start - start + offset..clipped_end - start + offset);
            }
        }
    }

    fn copy(&mut self, screen: usize, offset: usize, start: usize, width: usize) {
        let source = &self.screens[screen].slots;
        let mut column = 0;
        while column < width {
            let target = offset + column;
            if target >= self.out.len() {
                break;
            }
            let Some(slot) = source.get(start + column) else {
                break;
            };
            match slot {
                Slot::Cell {
                    width: cell_width, ..
                } if *cell_width > 1 => {
                    if column + cell_width <= width
                        && target + cell_width <= self.out.len()
                        && start + column + cell_width <= source.len()
                    {
                        for extra in 0..*cell_width {
                            self.out[target + extra] = Some(source[start + column + extra].clone());
                        }
                        column += cell_width;
                    } else {
                        self.out[target] = Some(Slot::blank(slot.style().clone()));
                        column += 1;
                    }
                }
                Slot::Cell { .. } => {
                    self.out[target] = Some(slot.clone());
                    column += 1;
                }
                Slot::Tail(style) => {
                    self.out[target] = Some(Slot::blank(style.clone()));
                    column += 1;
                }
            }
        }
    }

    fn append_after(&mut self, screen: usize, width: usize) {
        let appended: Vec<Slot> = self.screens[AFTER].slots[..width.min(self.cx(AFTER))].to_vec();
        self.screens[screen].slots.extend(appended);
    }

    fn focus(&self, unset: usize) -> (usize, usize) {
        if self.focus_start == -1 || self.focus_end == -1 {
            (unset, unset)
        } else {
            let start = usize::try_from(self.focus_start).unwrap_or(0);
            let end = usize::try_from(self.focus_end).unwrap_or(0).max(start);
            (start, end)
        }
    }

    fn put_list(&mut self, mut offset: usize, mut width: usize, focus: (usize, usize)) {
        let list_cx = self.cx(LIST);
        if width >= list_cx {
            self.put(LIST, offset, 0, width);
            return;
        }
        let (focus_start, focus_end) = focus;
        let focus_centre = focus_start + (focus_end - focus_start) / 2;
        let mut start = focus_centre.saturating_sub(width / 2);
        if start + width > list_cx {
            start = list_cx - width;
        }
        let left_cx = self.cx(LIST_LEFT);
        if start != 0 && width > left_cx {
            self.copy(LIST_LEFT, offset, 0, left_cx);
            offset += left_cx;
            start += left_cx;
            width -= left_cx;
        }
        let right_cx = self.cx(LIST_RIGHT);
        if start + width < list_cx && width > right_cx {
            self.copy(LIST_RIGHT, offset + width - right_cx, 0, right_cx);
            width -= right_cx;
        }
        self.put(LIST, offset, start, width);
    }
}

fn compose(
    expanded: &str,
    available: usize,
    base: &TmuxStyle,
    underlay: &[String],
) -> ComposedStatusRow {
    if available == 0 {
        return ComposedStatusRow::default();
    }
    let walked = walk(expanded);
    let aborted = walked.aborted;
    let list_align = walked.list_align;
    let fill = walked.fill;
    let mut draw = Draw {
        out: vec![None; available],
        screens: walked.screens,
        ranges: walked.ranges,
        focus_start: walked.focus_start,
        focus_end: walked.focus_end,
    };
    if !aborted {
        match list_align {
            1 => arrange_left(&mut draw, available),
            2 => arrange_centre(&mut draw, available),
            3 => arrange_right(&mut draw, available),
            4 => arrange_absolute_centre(&mut draw, available),
            _ => arrange_none(&mut draw, available),
        }
    }
    finish(draw, base, fill, underlay)
}

fn arrange_none(draw: &mut Draw, available: usize) {
    let mut left = draw.cx(LEFT);
    let mut centre = draw.cx(CENTRE);
    let mut right = draw.cx(RIGHT);
    let mut abs_centre = draw.cx(ABSOLUTE_CENTRE);
    while left + centre + right > available {
        if centre > 0 {
            centre -= 1;
        } else if right > 0 {
            right -= 1;
        } else {
            left -= 1;
        }
    }
    draw.put(LEFT, 0, 0, left);
    draw.put(RIGHT, available - right, draw.cx(RIGHT) - right, right);
    draw.put(
        CENTRE,
        (left + ((available - right) - left) / 2).saturating_sub(centre / 2),
        draw.cx(CENTRE) / 2 - centre / 2,
        centre,
    );
    if abs_centre > available {
        abs_centre = available;
    }
    draw.put(ABSOLUTE_CENTRE, (available - abs_centre) / 2, 0, abs_centre);
}

fn arrange_left(draw: &mut Draw, available: usize) {
    let mut left = draw.cx(LEFT);
    let mut centre = draw.cx(CENTRE);
    let mut right = draw.cx(RIGHT);
    let mut abs_centre = draw.cx(ABSOLUTE_CENTRE);
    let mut list = draw.cx(LIST);
    let mut after = draw.cx(AFTER);
    while left + centre + right + list + after > available {
        if centre > 0 {
            centre -= 1;
        } else if list > 0 {
            list -= 1;
        } else if right > 0 {
            right -= 1;
        } else if after > 0 {
            after -= 1;
        } else {
            left -= 1;
        }
    }
    if list == 0 {
        draw.append_after(LEFT, after);
        arrange_none(draw, available);
        return;
    }
    draw.put(LEFT, 0, 0, left);
    draw.put(RIGHT, available - right, draw.cx(RIGHT) - right, right);
    draw.put(AFTER, left + list, 0, after);
    draw.put(
        CENTRE,
        ((left + list + after) + ((available - right) - (left + list + after)) / 2)
            .saturating_sub(centre / 2),
        draw.cx(CENTRE) / 2 - centre / 2,
        centre,
    );
    let focus = draw.focus(0);
    draw.put_list(left, list, focus);
    if abs_centre > available {
        abs_centre = available;
    }
    draw.put(ABSOLUTE_CENTRE, (available - abs_centre) / 2, 0, abs_centre);
}

fn arrange_centre(draw: &mut Draw, available: usize) {
    let mut left = draw.cx(LEFT);
    let mut centre = draw.cx(CENTRE);
    let mut right = draw.cx(RIGHT);
    let mut abs_centre = draw.cx(ABSOLUTE_CENTRE);
    let mut list = draw.cx(LIST);
    let mut after = draw.cx(AFTER);
    while left + centre + right + list + after > available {
        if list > 0 {
            list -= 1;
        } else if after > 0 {
            after -= 1;
        } else if centre > 0 {
            centre -= 1;
        } else if right > 0 {
            right -= 1;
        } else {
            left -= 1;
        }
    }
    if list == 0 {
        draw.append_after(CENTRE, after);
        arrange_none(draw, available);
        return;
    }
    draw.put(LEFT, 0, 0, left);
    draw.put(RIGHT, available - right, draw.cx(RIGHT) - right, right);
    let middle = left + ((available - right) - left) / 2;
    draw.put(
        CENTRE,
        clamped_offset(middle.checked_sub(list / 2 + centre), available),
        0,
        centre,
    );
    draw.put(AFTER, (middle + list).saturating_sub(list / 2), 0, after);
    let focus = draw.focus(draw.cx(LIST) / 2);
    draw.put_list(middle.saturating_sub(list / 2), list, focus);
    if abs_centre > available {
        abs_centre = available;
    }
    draw.put(ABSOLUTE_CENTRE, (available - abs_centre) / 2, 0, abs_centre);
}

fn arrange_right(draw: &mut Draw, available: usize) {
    let mut left = draw.cx(LEFT);
    let mut centre = draw.cx(CENTRE);
    let mut right = draw.cx(RIGHT);
    let mut abs_centre = draw.cx(ABSOLUTE_CENTRE);
    let mut list = draw.cx(LIST);
    let mut after = draw.cx(AFTER);
    while left + centre + right + list + after > available {
        if centre > 0 {
            centre -= 1;
        } else if list > 0 {
            list -= 1;
        } else if right > 0 {
            right -= 1;
        } else if after > 0 {
            after -= 1;
        } else {
            left -= 1;
        }
    }
    if list == 0 {
        draw.append_after(RIGHT, after);
        arrange_none(draw, available);
        return;
    }
    draw.put(LEFT, 0, 0, left);
    draw.put(AFTER, available - after, draw.cx(AFTER) - after, after);
    let inner = available - right - list - after;
    draw.put(RIGHT, inner, 0, right);
    draw.put(
        CENTRE,
        (left + (inner - left) / 2).saturating_sub(centre / 2),
        draw.cx(CENTRE) / 2 - centre / 2,
        centre,
    );
    let focus = draw.focus(0);
    draw.put_list(available - list - after, list, focus);
    if abs_centre > available {
        abs_centre = available;
    }
    draw.put(ABSOLUTE_CENTRE, (available - abs_centre) / 2, 0, abs_centre);
}

fn arrange_absolute_centre(draw: &mut Draw, available: usize) {
    let mut left = draw.cx(LEFT);
    let mut centre = draw.cx(CENTRE);
    let mut right = draw.cx(RIGHT);
    let mut abs_centre = draw.cx(ABSOLUTE_CENTRE);
    let mut list = draw.cx(LIST);
    let mut after = draw.cx(AFTER);
    while left + centre + right > available {
        if centre > 0 {
            centre -= 1;
        } else if right > 0 {
            right -= 1;
        } else {
            left -= 1;
        }
    }
    while list + after + abs_centre > available {
        if list > 0 {
            list -= 1;
        } else if after > 0 {
            after -= 1;
        } else {
            abs_centre -= 1;
        }
    }
    draw.put(LEFT, 0, 0, left);
    draw.put(RIGHT, available - right, draw.cx(RIGHT) - right, right);
    let middle = left + ((available - right) - left) / 2;
    draw.put(
        CENTRE,
        clamped_offset(middle.checked_sub(centre), available),
        0,
        centre,
    );
    let focus = draw.focus(draw.cx(LIST) / 2);
    let mut offset = (available - list - abs_centre) / 2;
    draw.put(ABSOLUTE_CENTRE, offset, 0, abs_centre);
    offset += abs_centre;
    draw.put_list(offset, list, focus);
    offset += list;
    draw.put(AFTER, offset, 0, after);
}

fn merged_style(base: &TmuxStyle, cell: &TmuxStyle) -> TmuxStyle {
    let mut merged = TmuxStyle {
        fg: cell.fg.or(base.fg),
        bg: cell.bg.or(base.bg),
        us: cell.us.or(base.us),
        attributes: base.attributes.clone(),
        dim_percentage: cell.dim_percentage.or(base.dim_percentage),
        link: cell.link.clone().or_else(|| base.link.clone()),
        ..TmuxStyle::default()
    };
    let deltas = &cell.attributes;
    for (slot, delta) in [
        (&mut merged.attributes.acs, deltas.acs),
        (&mut merged.attributes.bold, deltas.bold),
        (&mut merged.attributes.dim, deltas.dim),
        (&mut merged.attributes.underscore, deltas.underscore),
        (&mut merged.attributes.blink, deltas.blink),
        (&mut merged.attributes.reverse, deltas.reverse),
        (&mut merged.attributes.hidden, deltas.hidden),
        (&mut merged.attributes.italics, deltas.italics),
        (&mut merged.attributes.strikethrough, deltas.strikethrough),
        (
            &mut merged.attributes.double_underscore,
            deltas.double_underscore,
        ),
        (
            &mut merged.attributes.curly_underscore,
            deltas.curly_underscore,
        ),
        (
            &mut merged.attributes.dotted_underscore,
            deltas.dotted_underscore,
        ),
        (
            &mut merged.attributes.dashed_underscore,
            deltas.dashed_underscore,
        ),
        (&mut merged.attributes.overline, deltas.overline),
        (&mut merged.attributes.noattr, deltas.noattr),
    ] {
        if delta != TmuxAttributeState::Unset {
            *slot = delta;
        }
    }
    merged
}

fn finish(
    draw: Draw,
    base: &TmuxStyle,
    fill: Option<TmuxColour>,
    underlay: &[String],
) -> ComposedStatusRow {
    let uncovered = fill.map_or_else(
        || merged_style(base, &TmuxStyle::default()),
        |fill| TmuxStyle {
            bg: Some(fill),
            ..TmuxStyle::default()
        },
    );
    let mut segments: Vec<StyledSegment> = Vec::new();
    let mut push = |text: &str, style: TmuxStyle| {
        if let Some(last) = segments.last_mut().filter(|last| last.style == style) {
            last.text.push_str(text);
        } else {
            segments.push(StyledSegment {
                text: text.to_owned(),
                style,
            });
        }
    };
    for (column, slot) in draw.out.iter().enumerate() {
        match slot {
            None => push(
                underlay.get(column).map_or(" ", String::as_str),
                uncovered.clone(),
            ),
            Some(Slot::Cell { text, style, .. }) => push(text, merged_style(base, style)),
            Some(Slot::Tail(_)) => {}
        }
    }
    let ranges = draw
        .ranges
        .into_iter()
        .filter_map(|range| {
            let placed = range.placed?;
            Some(StatusHitRange {
                columns: u16::try_from(placed.start).unwrap_or(u16::MAX)
                    ..u16::try_from(placed.end).unwrap_or(u16::MAX),
                target: range.target,
            })
        })
        .collect();
    ComposedStatusRow { segments, ranges }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_text(row: &ComposedStatusRow) -> String {
        row.segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    fn default_shaped_row(windows: &str) -> String {
        format!(
            "#[align=left range=left]L#[norange default]\
             #[list=on align=left]\
             #[list=left-marker]<#[list=right-marker]>#[list=on]\
             {windows}\
             #[nolist align=right range=right]R#[norange default]"
        )
    }

    fn window_item(index: u64, label: &str, focus: bool) -> String {
        if focus {
            format!("#[range=window|{index} list=focus]{label}#[norange list=on default] ")
        } else {
            format!("#[range=window|{index}]{label}#[norange default] ")
        }
    }

    #[test]
    fn default_row_places_left_list_and_right() {
        let windows = format!(
            "{}{}",
            window_item(0, "0:sh", false),
            window_item(1, "1:vim*", true)
        );
        let row = compose_status_row(&default_shaped_row(&windows), 40, "");
        let text = plain_text(&row);
        assert_eq!(text.chars().count(), 40);
        assert!(text.starts_with("L0:sh 1:vim*"), "{text:?}");
        assert!(text.ends_with('R'), "{text:?}");
        let windows: Vec<_> = row
            .ranges
            .iter()
            .filter(|range| matches!(range.target, TmuxRange::Window(_)))
            .collect();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].columns, 1..6);
        assert_eq!(windows[0].target, TmuxRange::Window(0));
        assert_eq!(windows[1].columns, 6..13);
        assert_eq!(windows[1].target, TmuxRange::Window(1));
        assert_eq!(row.hit_target(3), Some(&TmuxRange::Window(0)));
        assert_eq!(row.hit_target(12), Some(&TmuxRange::Window(1)));
        assert_eq!(row.hit_target(20), None);
        assert!(
            row.ranges
                .iter()
                .any(|range| range.target == TmuxRange::Left && range.columns == (0..1))
        );
        assert!(
            row.ranges
                .iter()
                .any(|range| range.target == TmuxRange::Right && range.columns == (39..40))
        );
    }

    #[test]
    fn truncated_list_keeps_the_focus_visible_and_paints_markers() {
        let mut windows = String::new();
        for index in 0..8 {
            windows.push_str(&window_item(index, &format!("{index}:shell"), index == 7));
        }
        let row = compose_status_row(&default_shaped_row(&windows), 30, "");
        let text = plain_text(&row);
        assert_eq!(text.chars().count(), 30);
        assert!(
            text.contains("7:shell"),
            "focused window stays visible: {text:?}"
        );
        assert!(text.contains('<'), "left marker paints: {text:?}");
        assert!(
            row.ranges
                .iter()
                .any(|range| range.target == TmuxRange::Window(7)),
            "{:?}",
            row.ranges
        );
        assert!(
            !row.ranges
                .iter()
                .any(|range| range.target == TmuxRange::Window(0)),
            "clipped-away windows expose no hit range"
        );
    }

    #[test]
    fn alignment_sections_land_left_centre_and_right() {
        let row = compose_status_row("AA#[align=centre]CC#[align=right]RR", 20, "");
        assert_eq!(plain_text(&row), "AA       CC       RR");
    }

    #[test]
    fn absolute_centre_lands_in_the_middle_of_the_full_width() {
        let row = compose_status_row("#[align=absolute-centre]XX", 21, "");
        assert_eq!(
            plain_text(&row),
            format!("{}XX{}", " ".repeat(9), " ".repeat(10))
        );
    }

    #[test]
    fn fill_paints_uncovered_columns() {
        let row = compose_status_row("#[fill=red]ok", 5, "");
        assert_eq!(plain_text(&row), "ok   ");
        let blank = &row.segments.last().expect("blank segment").style;
        assert_eq!(blank.bg, Some(TmuxColour::Basic(1)));
    }

    #[test]
    fn fill_cells_take_only_the_fill_background_never_base_fg_or_attributes() {
        let row = compose_status_row("#[fill=red]ok", 5, "fg=white,bg=green,bold");
        assert_eq!(plain_text(&row), "ok   ");
        let drawn = &row.segments[0].style;
        assert_eq!(drawn.fg, Some(TmuxColour::Basic(7)));
        assert_eq!(drawn.bg, Some(TmuxColour::Basic(2)));
        assert_eq!(drawn.attributes.bold, TmuxAttributeState::On);
        let blank = &row.segments.last().expect("fill segment").style;
        assert_eq!(blank.bg, Some(TmuxColour::Basic(1)));
        assert_eq!(blank.fg, None, "fill cells keep the default foreground");
        assert_eq!(
            blank.attributes.bold,
            TmuxAttributeState::Unset,
            "fill cells keep default attributes"
        );
    }

    #[test]
    fn an_underflowing_centre_offset_clips_at_the_right_edge_like_the_pin() {
        let row = compose_status_row(
            "#[align=centre]CC#[list=on align=centre]12345678#[nolist]",
            10,
            "",
        );
        assert_eq!(plain_text(&row), " 12345678C");
    }

    #[test]
    fn base_style_backs_blank_and_default_cells() {
        let row = compose_status_row("hi", 4, "bg=green,fg=black,bold");
        assert_eq!(plain_text(&row), "hi  ");
        for segment in &row.segments {
            assert_eq!(segment.style.bg, Some(TmuxColour::Basic(2)));
            assert_eq!(segment.style.fg, Some(TmuxColour::Basic(0)));
            assert_eq!(segment.style.attributes.bold, TmuxAttributeState::On);
        }

        let styled = compose_status_row("#[fg=red]hi", 4, "bg=green,fg=black");
        assert_eq!(styled.segments[0].style.fg, Some(TmuxColour::Basic(1)));
        assert_eq!(styled.segments[0].style.bg, Some(TmuxColour::Basic(2)));
    }

    #[test]
    fn an_unparseable_base_style_means_theme_default() {
        let row = compose_status_row("hi", 4, "bg=#{@broken}");
        assert_eq!(plain_text(&row), "hi  ");
        for segment in &row.segments {
            assert_eq!(segment.style.bg, None);
            assert_eq!(segment.style.fg, None);
        }
    }

    #[test]
    fn wide_glyphs_cut_at_a_copy_boundary_become_spaces() {
        let row = compose_status_row("界界", 3, "");
        let text = plain_text(&row);
        assert_eq!(text, "界 ");
        assert_eq!(
            text.chars()
                .map(|character| character.width().unwrap_or(0))
                .sum::<usize>(),
            3
        );
    }

    #[test]
    fn escaped_hashes_render_literally_and_never_open_a_style() {
        let row = compose_status_row("a##[fg=red]b", 14, "");
        assert_eq!(plain_text(&row), "a#[fg=red]b   ");
        assert!(
            row.segments
                .iter()
                .all(|segment| segment.style.fg.is_none())
        );
    }

    #[test]
    fn an_unterminated_marker_draws_nothing_like_format_draw() {
        let row = compose_status_row("visible#[fg=red", 10, "");
        assert_eq!(plain_text(&row), " ".repeat(10));
        assert!(row.ranges.is_empty());
    }

    #[test]
    fn pane_and_session_ranges_carry_wire_sized_ids() {
        let row = compose_status_row(
            "#[range=pane|%18446744073709551615]P#[norange] #[range=session|$7]S#[norange] end",
            20,
            "",
        );
        assert_eq!(row.hit_target(0), Some(&TmuxRange::Pane(u64::MAX)));
        assert_eq!(row.hit_target(2), Some(&TmuxRange::Session(7)));
    }

    #[test]
    fn a_range_left_open_at_the_end_of_the_row_is_discarded() {
        let row = compose_status_row("#[range=window|3]never closed", 30, "");
        assert!(row.ranges.is_empty());
    }

    #[test]
    fn push_and_pop_default_restore_the_marker_scoped_style() {
        let row = compose_status_row(
            "#[fg=red]r#[push-default fg=blue]b#[default]r#[pop-default]#[default]d",
            4,
            "",
        );
        assert_eq!(row.segments[0].style.fg, Some(TmuxColour::Basic(1)));
        assert_eq!(row.segments[1].style.fg, Some(TmuxColour::Basic(4)));
        assert_eq!(row.segments[2].style.fg, Some(TmuxColour::Basic(1)));
        assert_eq!(row.segments[3].style.fg, None);
    }

    #[test]
    fn zero_width_output_is_empty() {
        assert_eq!(
            compose_status_row("anything", 0, ""),
            ComposedStatusRow::default()
        );
    }

    #[test]
    fn blank_rows_compose_to_base_styled_spaces() {
        let row = compose_status_row("", 6, "bg=blue");
        assert_eq!(plain_text(&row), "      ");
        assert_eq!(row.segments.len(), 1);
        assert_eq!(row.segments[0].style.bg, Some(TmuxColour::Basic(4)));
    }
}
