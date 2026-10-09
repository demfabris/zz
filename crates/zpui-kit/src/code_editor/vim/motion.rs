use std::ops::Range;

use sum_tree::Bias;

use crate::code_editor::{Rope, RopeExt as _};

use super::parser::Operator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MotionKind {
    Exclusive,
    Inclusive,
    Linewise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    FirstNonBlank,
    LineEnd,
    WordForward { big: bool },
    WordBackward { big: bool },
    WordEnd { big: bool },
    WordEndBackward { big: bool },
    Find(FindChar),
    RepeatFind { reverse: bool },
    FirstLine,
    LastLine,
    ParagraphForward,
    ParagraphBackward,
    HalfPage { down: bool },
    Page { down: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FindChar {
    pub target: char,
    pub till: bool,
    pub backward: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MotionTarget {
    pub offset: usize,
    pub kind: MotionKind,
    pub goal_column: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MotionContext {
    pub viewport_rows: usize,
    pub goal_column: Option<usize>,
    pub last_find: Option<FindChar>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum OperatorSpan {
    Charwise(Range<usize>),
    Linewise { first_row: usize, last_row: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LineSpan {
    pub content: Range<usize>,
    /// `content`, or the preceding newline when the span reaches the last line.
    pub delete: Range<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Blank,
    Word,
    Punct,
}

fn class(character: char, big: bool) -> Class {
    if character.is_whitespace() {
        Class::Blank
    } else if big || character.is_alphanumeric() || character == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

fn char_at(rope: &Rope, offset: usize) -> Option<char> {
    if offset >= rope.len() {
        return None;
    }
    rope.char_at(offset)
}

fn char_before(rope: &Rope, offset: usize) -> Option<char> {
    if offset == 0 {
        return None;
    }
    rope.chars_at(offset).reversed().next()
}

fn forward(rope: &Rope, offset: usize) -> usize {
    char_at(rope, offset).map_or(rope.len(), |character| offset + character.len_utf8())
}

fn backward(rope: &Rope, offset: usize) -> usize {
    char_before(rope, offset).map_or(0, |character| offset - character.len_utf8())
}

fn line_is_blank(rope: &Rope, row: usize) -> bool {
    rope.slice_line(row)
        .chars()
        .all(|character| character.is_whitespace())
}

pub(super) fn first_non_blank(rope: &Rope, row: usize) -> usize {
    let start = rope.line_start_offset(row);
    let line = rope.slice_line(row);
    let indent: usize = line
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .map(char::len_utf8)
        .sum();
    start + indent
}

fn starts_word(rope: &Rope, offset: usize, big: bool) -> bool {
    let Some(character) = char_at(rope, offset) else {
        return false;
    };
    if class(character, big) == Class::Blank {
        return character == '\n' && char_before(rope, offset).is_none_or(|before| before == '\n');
    }
    char_before(rope, offset).is_none_or(|before| class(before, big) != class(character, big))
}

fn ends_word(rope: &Rope, offset: usize, big: bool) -> bool {
    let Some(character) = char_at(rope, offset) else {
        return false;
    };
    if class(character, big) == Class::Blank {
        return character == '\n' && char_before(rope, offset).is_none_or(|before| before == '\n');
    }
    char_at(rope, forward(rope, offset))
        .is_none_or(|next| class(next, big) != class(character, big))
}

fn scan_forward(
    rope: &Rope,
    cursor: usize,
    count: usize,
    predicate: impl Fn(&Rope, usize) -> bool,
) -> usize {
    let mut offset = cursor;
    let mut found = offset;
    for _ in 0..count {
        loop {
            let next = forward(rope, offset);
            if next == offset {
                return rope.len();
            }
            offset = next;
            if predicate(rope, offset) {
                found = offset;
                break;
            }
            if offset >= rope.len() {
                return rope.len();
            }
        }
    }
    found
}

fn scan_backward(
    rope: &Rope,
    cursor: usize,
    count: usize,
    predicate: impl Fn(&Rope, usize) -> bool,
) -> usize {
    let mut offset = cursor;
    let mut found = 0;
    for _ in 0..count {
        loop {
            if offset == 0 {
                return 0;
            }
            offset = backward(rope, offset);
            if predicate(rope, offset) {
                found = offset;
                break;
            }
        }
    }
    found
}

fn horizontal(rope: &Rope, cursor: usize, count: usize, right: bool) -> usize {
    let row = rope.offset_to_point(cursor).row;
    let bound = if right {
        rope.line_end_offset(row)
    } else {
        rope.line_start_offset(row)
    };
    let mut offset = cursor;
    for _ in 0..count {
        if right && offset >= bound {
            break;
        }
        if !right && offset <= bound {
            break;
        }
        offset = if right {
            forward(rope, offset)
        } else {
            backward(rope, offset)
        };
    }
    offset
}

fn vertical(rope: &Rope, cursor: usize, rows: isize, context: &MotionContext) -> MotionTarget {
    let point = rope.offset_to_point(cursor);
    let goal = context.goal_column.unwrap_or(point.column);
    let last_row = rope.lines_len().saturating_sub(1);
    let row = point.row.saturating_add_signed(rows).min(last_row);
    let offset = rope.line_start_offset(row) + goal.min(rope.line_len(row));
    MotionTarget {
        offset: rope.clip_offset(offset, Bias::Left),
        kind: MotionKind::Linewise,
        goal_column: Some(goal),
    }
}

fn find_char(rope: &Rope, cursor: usize, find: FindChar, count: usize) -> Option<usize> {
    let row = rope.offset_to_point(cursor).row;
    let start = rope.line_start_offset(row);
    let end = rope.line_end_offset(row);
    let mut offset = cursor;
    for _ in 0..count {
        loop {
            if find.backward {
                if offset <= start {
                    return None;
                }
                offset = backward(rope, offset);
            } else {
                offset = forward(rope, offset);
                if offset >= end {
                    return None;
                }
            }
            if char_at(rope, offset) == Some(find.target) {
                break;
            }
        }
    }
    Some(if find.till {
        if find.backward {
            forward(rope, offset)
        } else {
            backward(rope, offset)
        }
    } else {
        offset
    })
}

fn paragraph_forward(rope: &Rope, row: usize, count: usize) -> usize {
    let last = rope.lines_len().saturating_sub(1);
    let mut row = row;
    for _ in 0..count {
        let Some(next) = (row + 1..=last).find(|candidate| {
            line_is_blank(rope, *candidate) && !line_is_blank(rope, candidate - 1)
        }) else {
            return rope.len();
        };
        row = next;
    }
    rope.line_start_offset(row)
}

fn paragraph_backward(rope: &Rope, row: usize, count: usize) -> usize {
    let last = rope.lines_len().saturating_sub(1);
    let mut row = row;
    for _ in 0..count {
        let Some(previous) = (0..row).rev().find(|candidate| {
            line_is_blank(rope, *candidate)
                && (*candidate == last || !line_is_blank(rope, candidate + 1))
        }) else {
            return 0;
        };
        row = previous;
    }
    rope.line_start_offset(row)
}

const fn exclusive(offset: usize) -> MotionTarget {
    MotionTarget {
        offset,
        kind: MotionKind::Exclusive,
        goal_column: None,
    }
}

const fn inclusive(offset: usize) -> MotionTarget {
    MotionTarget {
        offset,
        kind: MotionKind::Inclusive,
        goal_column: None,
    }
}

const fn linewise(offset: usize) -> MotionTarget {
    MotionTarget {
        offset,
        kind: MotionKind::Linewise,
        goal_column: None,
    }
}

pub(super) fn resolve_motion(
    rope: &Rope,
    cursor: usize,
    motion: Motion,
    count: Option<usize>,
    context: &MotionContext,
) -> Option<MotionTarget> {
    let repeat = count.unwrap_or(1).max(1);
    let point = rope.offset_to_point(cursor);
    let target = match motion {
        Motion::Left => exclusive(horizontal(rope, cursor, repeat, false)),
        Motion::Right => exclusive(horizontal(rope, cursor, repeat, true)),
        Motion::Up => vertical(rope, cursor, -(repeat as isize), context),
        Motion::Down => vertical(rope, cursor, repeat as isize, context),
        Motion::LineStart => exclusive(rope.line_start_offset(point.row)),
        Motion::FirstNonBlank => exclusive(first_non_blank(rope, point.row)),
        Motion::LineEnd => {
            let row = (point.row + repeat - 1).min(rope.lines_len().saturating_sub(1));
            let end = rope.line_end_offset(row);
            inclusive(backward(rope, end).max(rope.line_start_offset(row)))
        }
        Motion::WordForward { big } => exclusive(scan_forward(rope, cursor, repeat, |rope, at| {
            starts_word(rope, at, big)
        })),
        Motion::WordBackward { big } => {
            exclusive(scan_backward(rope, cursor, repeat, |rope, at| {
                starts_word(rope, at, big)
            }))
        }
        Motion::WordEnd { big } => inclusive(scan_forward(rope, cursor, repeat, |rope, at| {
            ends_word(rope, at, big)
        })),
        Motion::WordEndBackward { big } => {
            inclusive(scan_backward(rope, cursor, repeat, |rope, at| {
                ends_word(rope, at, big)
            }))
        }
        Motion::Find(find) => {
            let offset = find_char(rope, cursor, find, repeat)?;
            if find.backward {
                exclusive(offset)
            } else {
                inclusive(offset)
            }
        }
        Motion::RepeatFind { reverse } => {
            let mut find = context.last_find?;
            if reverse {
                find.backward = !find.backward;
            }
            let offset = find_char(rope, cursor, find, repeat)?;
            if find.backward {
                exclusive(offset)
            } else {
                inclusive(offset)
            }
        }
        Motion::FirstLine => {
            let row = repeat
                .saturating_sub(1)
                .min(rope.lines_len().saturating_sub(1));
            linewise(first_non_blank(rope, row))
        }
        Motion::LastLine => {
            let last = rope.lines_len().saturating_sub(1);
            let row = count.map_or(last, |line| line.saturating_sub(1).min(last));
            linewise(first_non_blank(rope, row))
        }
        Motion::ParagraphForward => exclusive(paragraph_forward(rope, point.row, repeat)),
        Motion::ParagraphBackward => exclusive(paragraph_backward(rope, point.row, repeat)),
        Motion::HalfPage { down } => {
            let rows = (context.viewport_rows / 2).max(1) * repeat;
            vertical(rope, cursor, signed_rows(rows, down), context)
        }
        Motion::Page { down } => {
            let rows = context.viewport_rows.max(1) * repeat;
            vertical(rope, cursor, signed_rows(rows, down), context)
        }
    };
    Some(target)
}

fn signed_rows(rows: usize, down: bool) -> isize {
    let rows = rows.min(isize::MAX as usize) as isize;
    if down { rows } else { -rows }
}

pub(super) fn line_span(rope: &Rope, first_row: usize, last_row: usize) -> LineSpan {
    let last_line = rope.lines_len().saturating_sub(1);
    let first_row = first_row.min(last_line);
    let last_row = last_row.min(last_line);
    let start = rope.line_start_offset(first_row);
    let end = rope.line_end_offset(last_row);
    let trailing_newline = end < rope.len();
    let content = start..if trailing_newline {
        forward(rope, end)
    } else {
        end
    };
    let delete = if trailing_newline || first_row == 0 {
        content.clone()
    } else {
        backward(rope, start)..end
    };
    LineSpan { content, delete }
}

pub(super) fn operator_span(
    rope: &Rope,
    cursor: usize,
    operator: Operator,
    motion: Motion,
    count: Option<usize>,
    context: &MotionContext,
) -> Option<OperatorSpan> {
    let on_blank = char_at(rope, cursor).is_none_or(char::is_whitespace);
    let motion = match (operator, motion) {
        (Operator::Change, Motion::WordForward { big }) if !on_blank => Motion::WordEnd { big },
        _ => motion,
    };
    let target = resolve_motion(rope, cursor, motion, count, context)?;
    let mut offset = target.offset;
    if matches!(motion, Motion::WordForward { .. }) {
        let line_end = rope.line_end_offset(rope.offset_to_point(cursor).row);
        if offset > line_end {
            offset = line_end.max(cursor);
        }
    }

    let (start, end) = (cursor.min(offset), cursor.max(offset));
    Some(match target.kind {
        MotionKind::Linewise => OperatorSpan::Linewise {
            first_row: rope.offset_to_point(start).row,
            last_row: rope.offset_to_point(end).row,
        },
        MotionKind::Exclusive => OperatorSpan::Charwise(start..end),
        MotionKind::Inclusive if char_at(rope, end) == Some('\n') => {
            OperatorSpan::Charwise(start..end)
        }
        MotionKind::Inclusive => OperatorSpan::Charwise(start..forward(rope, end)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(text: &str) -> Rope {
        Rope::from(text)
    }

    fn go(text: &str, cursor: usize, motion: Motion) -> usize {
        resolve_motion(&rope(text), cursor, motion, None, &MotionContext::default())
            .expect("motion resolves")
            .offset
    }

    fn go_n(text: &str, cursor: usize, motion: Motion, count: usize) -> usize {
        resolve_motion(
            &rope(text),
            cursor,
            motion,
            Some(count),
            &MotionContext::default(),
        )
        .expect("motion resolves")
        .offset
    }

    #[test]
    fn word_motions_split_words_from_punctuation_runs() {
        let text = "alpha, beta::gamma";
        let word = Motion::WordForward { big: false };
        let big = Motion::WordForward { big: true };
        assert_eq!(go(text, 0, word), 5, "w stops on the comma run");
        assert_eq!(go(text, 5, word), 7, "w skips the blank after it");
        assert_eq!(go(text, 7, word), 11, "w stops on ::");
        assert_eq!(go(text, 11, word), 13, "w leaves the punctuation run");
        assert_eq!(go(text, 0, big), 7, "W treats alpha, as one word");
        assert_eq!(go_n(text, 0, word, 3), 11, "counts compose");
    }

    #[test]
    fn word_motions_run_backwards_and_to_word_ends() {
        let text = "alpha beta gamma";
        assert_eq!(
            go(text, 12, Motion::WordBackward { big: false }),
            11,
            "b from inside a word lands on that word's start"
        );
        assert_eq!(go(text, 11, Motion::WordBackward { big: false }), 6);
        assert_eq!(go(text, 0, Motion::WordEnd { big: false }), 4);
        assert_eq!(go(text, 4, Motion::WordEnd { big: false }), 9);
        assert_eq!(go(text, 12, Motion::WordEndBackward { big: false }), 9);
        assert_eq!(go(text, 2, Motion::WordEndBackward { big: false }), 0);
    }

    #[test]
    fn word_end_at_buffer_end_stays_on_the_last_character() {
        let text = "one two";
        assert_eq!(go(text, 4, Motion::WordEnd { big: false }), 6);
        assert_eq!(go(text, 6, Motion::WordEnd { big: false }), text.len());
    }

    #[test]
    fn word_motions_treat_an_empty_line_as_a_word() {
        let text = "one\n\ntwo";
        assert_eq!(go(text, 0, Motion::WordForward { big: false }), 4);
        assert_eq!(go(text, 4, Motion::WordForward { big: false }), 5);
    }

    #[test]
    fn word_motions_step_over_unicode_by_whole_characters() {
        let text = "héllo wörld";
        assert_eq!(go(text, 0, Motion::WordForward { big: false }), 7);
        assert_eq!(go(text, 7, Motion::WordBackward { big: false }), 0);
        assert_eq!(go(text, 0, Motion::WordEnd { big: false }), 5);
    }

    #[test]
    fn line_motions_respect_indentation_and_line_bounds() {
        let text = "  indented\nnext";
        assert_eq!(go(text, 6, Motion::LineStart), 0);
        assert_eq!(go(text, 6, Motion::FirstNonBlank), 2);
        assert_eq!(go(text, 2, Motion::LineEnd), 9, "$ stops on the last char");
        assert_eq!(go(text, 2, Motion::Left), 1);
        assert_eq!(go_n(text, 2, Motion::Left, 9), 0, "h stops at line start");
        assert_eq!(go_n(text, 8, Motion::Right, 9), 10, "l stops at line end");
    }

    #[test]
    fn vertical_motions_carry_the_goal_column() {
        let text = "long line here\nab\nlong again";
        let context = MotionContext::default();
        let down = resolve_motion(&rope(text), 10, Motion::Down, None, &context).unwrap();
        assert_eq!(down.goal_column, Some(10));
        assert_eq!(down.offset, 17, "clamped to the short line");
        let context = MotionContext {
            goal_column: Some(10),
            ..MotionContext::default()
        };
        let down = resolve_motion(&rope(text), 17, Motion::Down, None, &context).unwrap();
        assert_eq!(down.offset, 28, "the goal column comes back");
    }

    #[test]
    fn find_and_repeat_stay_on_the_line() {
        let text = "a,b,c\nd,e";
        let find = |target, till, backward| {
            Motion::Find(FindChar {
                target,
                till,
                backward,
            })
        };
        assert_eq!(go(text, 0, find(',', false, false)), 1);
        assert_eq!(go_n(text, 0, find(',', false, false), 2), 3);
        assert_eq!(go(text, 0, find(',', true, false)), 0, "t stops short");
        assert_eq!(go(text, 4, find(',', false, true)), 3);
        assert_eq!(go(text, 4, find(',', true, true)), 4, "T stops short");
        assert!(
            resolve_motion(
                &rope(text),
                0,
                find(';', false, false),
                None,
                &MotionContext::default()
            )
            .is_none(),
            "a missing target aborts"
        );
        assert!(
            resolve_motion(
                &rope(text),
                0,
                find(',', false, false),
                None,
                &MotionContext::default()
            )
            .is_some()
        );

        let context = MotionContext {
            last_find: Some(FindChar {
                target: ',',
                till: false,
                backward: false,
            }),
            ..MotionContext::default()
        };
        let repeat = resolve_motion(
            &rope(text),
            1,
            Motion::RepeatFind { reverse: false },
            None,
            &context,
        )
        .unwrap();
        assert_eq!(repeat.offset, 3);
        let reverse = resolve_motion(
            &rope(text),
            3,
            Motion::RepeatFind { reverse: true },
            None,
            &context,
        )
        .unwrap();
        assert_eq!(reverse.offset, 1);
    }

    #[test]
    fn repeat_find_without_a_previous_find_fails() {
        assert!(
            resolve_motion(
                &rope("abc"),
                0,
                Motion::RepeatFind { reverse: false },
                None,
                &MotionContext::default()
            )
            .is_none()
        );
    }

    #[test]
    fn document_motions_land_on_the_first_non_blank() {
        let text = "one\n  two\nthree";
        assert_eq!(go(text, 12, Motion::FirstLine), 0);
        assert_eq!(go_n(text, 0, Motion::FirstLine, 2), 6);
        assert_eq!(go(text, 0, Motion::LastLine), 10);
        assert_eq!(go_n(text, 0, Motion::LastLine, 2), 6);
    }

    #[test]
    fn paragraph_motions_stop_on_blank_lines() {
        let text = "a\nb\n\nc\nd\n\ne";
        assert_eq!(go(text, 0, Motion::ParagraphForward), 4);
        assert_eq!(go(text, 4, Motion::ParagraphForward), 9);
        assert_eq!(go(text, 10, Motion::ParagraphForward), text.len());
        assert_eq!(go(text, 10, Motion::ParagraphBackward), 9);
        assert_eq!(go(text, 5, Motion::ParagraphBackward), 4);
        assert_eq!(go(text, 2, Motion::ParagraphBackward), 0);
    }

    #[test]
    fn page_motions_use_the_viewport_height() {
        let text = "0\n1\n2\n3\n4\n5\n6\n7\n8\n9";
        let context = MotionContext {
            viewport_rows: 4,
            ..MotionContext::default()
        };
        let half = resolve_motion(
            &rope(text),
            0,
            Motion::HalfPage { down: true },
            None,
            &context,
        )
        .unwrap()
        .offset;
        assert_eq!(half, 4, "half of four rows is two lines down");
        let page = resolve_motion(&rope(text), 0, Motion::Page { down: true }, None, &context)
            .unwrap()
            .offset;
        assert_eq!(page, 8);
    }

    #[test]
    fn exclusive_inclusive_and_linewise_spans_differ() {
        let text = "alpha beta\ngamma";
        let context = MotionContext::default();
        let span = |motion| operator_span(&rope(text), 0, Operator::Delete, motion, None, &context);
        assert_eq!(
            span(Motion::WordForward { big: false }),
            Some(OperatorSpan::Charwise(0..6)),
            "dw is exclusive"
        );
        assert_eq!(
            span(Motion::WordEnd { big: false }),
            Some(OperatorSpan::Charwise(0..5)),
            "de is inclusive"
        );
        assert_eq!(
            span(Motion::LineEnd),
            Some(OperatorSpan::Charwise(0..10)),
            "d$ is inclusive to the end of the line"
        );
        assert_eq!(
            span(Motion::Down),
            Some(OperatorSpan::Linewise {
                first_row: 0,
                last_row: 1
            }),
            "dj is linewise"
        );
    }

    #[test]
    fn delete_word_stops_at_the_end_of_the_line() {
        let text = "alpha beta\ngamma";
        let context = MotionContext::default();
        assert_eq!(
            operator_span(
                &rope(text),
                6,
                Operator::Delete,
                Motion::WordForward { big: false },
                None,
                &context
            ),
            Some(OperatorSpan::Charwise(6..10)),
            "dw on the last word of a line spares the newline"
        );
    }

    #[test]
    fn change_word_behaves_like_change_to_word_end() {
        let text = "alpha beta";
        let context = MotionContext::default();
        assert_eq!(
            operator_span(
                &rope(text),
                0,
                Operator::Change,
                Motion::WordForward { big: false },
                None,
                &context
            ),
            Some(OperatorSpan::Charwise(0..5)),
            "cw spares the trailing blank"
        );
        assert_eq!(
            operator_span(
                &rope(text),
                5,
                Operator::Change,
                Motion::WordForward { big: false },
                None,
                &context
            ),
            Some(OperatorSpan::Charwise(5..6)),
            "cw on a blank keeps w semantics"
        );
    }

    #[test]
    fn line_spans_swallow_the_right_newline() {
        let text = "one\ntwo\nthree";
        let first = line_span(&rope(text), 0, 0);
        assert_eq!(first.content, 0..4);
        assert_eq!(first.delete, 0..4);
        let last = line_span(&rope(text), 2, 2);
        assert_eq!(last.content, 8..13);
        assert_eq!(
            last.delete,
            7..13,
            "dd on the last line eats the newline before it"
        );
        let both = line_span(&rope(text), 0, 1);
        assert_eq!(both.content, 0..8);
    }

    #[test]
    fn a_single_line_buffer_deletes_to_nothing() {
        let span = line_span(&rope("only"), 0, 0);
        assert_eq!(span.content, 0..4);
        assert_eq!(span.delete, 0..4);
    }
}
