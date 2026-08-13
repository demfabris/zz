use std::ops::Range;

use crate::code_editor::{Rope, RopeExt as _};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObjectKind {
    Word { big: bool },
    Quote(char),
    Bracket { open: char, close: char },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TextObject {
    pub kind: ObjectKind,
    pub around: bool,
}

impl TextObject {
    pub(super) fn from_key(key: char, around: bool) -> Option<Self> {
        let kind = match key {
            'w' => ObjectKind::Word { big: false },
            'W' => ObjectKind::Word { big: true },
            '"' | '\'' | '`' => ObjectKind::Quote(key),
            '(' | ')' | 'b' => ObjectKind::Bracket {
                open: '(',
                close: ')',
            },
            '[' | ']' => ObjectKind::Bracket {
                open: '[',
                close: ']',
            },
            '{' | '}' | 'B' => ObjectKind::Bracket {
                open: '{',
                close: '}',
            },
            _ => return None,
        };
        Some(Self { kind, around })
    }
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

fn word_run(rope: &Rope, cursor: usize, big: bool) -> Option<Range<usize>> {
    let character = char_at(rope, cursor)?;
    if character == '\n' {
        return None;
    }
    let wanted = class(character, big);
    let mut start = cursor;
    while let Some(previous) = char_before(rope, start) {
        if previous == '\n' || class(previous, big) != wanted {
            break;
        }
        start = backward(rope, start);
    }
    let mut end = forward(rope, cursor);
    while let Some(next) = char_at(rope, end) {
        if next == '\n' || class(next, big) != wanted {
            break;
        }
        end = forward(rope, end);
    }
    Some(start..end)
}

fn word_object(rope: &Rope, cursor: usize, big: bool, around: bool) -> Option<Range<usize>> {
    let run = word_run(rope, cursor, big)?;
    if !around {
        return Some(run);
    }
    let mut end = run.end;
    while let Some(next) = char_at(rope, end) {
        if next == '\n' || !next.is_whitespace() {
            break;
        }
        end = forward(rope, end);
    }
    if end > run.end {
        return Some(run.start..end);
    }
    let mut start = run.start;
    while let Some(previous) = char_before(rope, start) {
        if previous == '\n' || !previous.is_whitespace() {
            break;
        }
        start = backward(rope, start);
    }
    Some(start..run.end)
}

fn quote_object(rope: &Rope, cursor: usize, delimiter: char, around: bool) -> Option<Range<usize>> {
    let row = rope.offset_to_point(cursor).row;
    let line_end = rope.line_end_offset(row);
    let mut delimiters = Vec::new();
    let mut offset = rope.line_start_offset(row);
    while offset < line_end {
        if char_at(rope, offset) == Some(delimiter) {
            delimiters.push(offset);
        }
        offset = forward(rope, offset);
    }

    let (open, close) = delimiters
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .find(|(_, close)| cursor <= *close)?;
    if !around {
        return Some(forward(rope, open)..close);
    }
    let mut end = forward(rope, close);
    while let Some(next) = char_at(rope, end) {
        if next == '\n' || !next.is_whitespace() {
            break;
        }
        end = forward(rope, end);
    }
    Some(open..end)
}

fn enclosing_pair(rope: &Rope, cursor: usize, open: char, close: char) -> Option<(usize, usize)> {
    let start = if char_at(rope, cursor) == Some(open) {
        cursor
    } else {
        let mut depth = 0usize;
        let mut offset = cursor;
        loop {
            if offset == 0 {
                return None;
            }
            offset = backward(rope, offset);
            match char_at(rope, offset) {
                Some(character) if character == close => depth += 1,
                Some(character) if character == open => {
                    if depth == 0 {
                        break offset;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    };

    let mut depth = 0usize;
    let mut offset = forward(rope, start);
    let end = loop {
        if offset >= rope.len() {
            return None;
        }
        match char_at(rope, offset) {
            Some(character) if character == open => depth += 1,
            Some(character) if character == close => {
                if depth == 0 {
                    break offset;
                }
                depth -= 1;
            }
            _ => {}
        }
        offset = forward(rope, offset);
    };
    (cursor <= end).then_some((start, end))
}

pub(super) fn resolve_object(
    rope: &Rope,
    cursor: usize,
    object: TextObject,
) -> Option<Range<usize>> {
    match object.kind {
        ObjectKind::Word { big } => word_object(rope, cursor, big, object.around),
        ObjectKind::Quote(delimiter) => quote_object(rope, cursor, delimiter, object.around),
        ObjectKind::Bracket { open, close } => {
            let (start, end) = enclosing_pair(rope, cursor, open, close)?;
            Some(if object.around {
                start..forward(rope, end)
            } else {
                forward(rope, start)..end
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(text: &str, cursor: usize, key: char, around: bool) -> Option<String> {
        let rope = Rope::from(text);
        let object = TextObject::from_key(key, around)?;
        let range = resolve_object(&rope, cursor, object)?;
        Some(rope.slice(range).to_string())
    }

    #[test]
    fn word_objects_take_the_run_under_the_cursor() {
        let text = "alpha beta gamma";
        assert_eq!(resolve(text, 7, 'w', false).as_deref(), Some("beta"));
        assert_eq!(resolve(text, 7, 'w', true).as_deref(), Some("beta "));
        assert_eq!(resolve(text, 13, 'w', true).as_deref(), Some(" gamma"));
    }

    #[test]
    fn word_objects_stop_at_punctuation_but_big_ones_do_not() {
        let text = "foo.bar baz";
        assert_eq!(resolve(text, 0, 'w', false).as_deref(), Some("foo"));
        assert_eq!(resolve(text, 3, 'w', false).as_deref(), Some("."));
        assert_eq!(resolve(text, 0, 'W', false).as_deref(), Some("foo.bar"));
    }

    #[test]
    fn a_word_object_on_whitespace_selects_the_blank_run() {
        let text = "one   two";
        assert_eq!(resolve(text, 4, 'w', false).as_deref(), Some("   "));
    }

    #[test]
    fn word_objects_never_cross_a_line_break() {
        let text = "one\ntwo";
        assert_eq!(resolve(text, 0, 'w', true).as_deref(), Some("one"));
        assert_eq!(resolve(text, 3, 'w', false), None, "the newline is nothing");
    }

    #[test]
    fn quote_objects_are_line_scoped_and_paired_from_the_line_start() {
        let text = "say \"hello there\" now\n\"other\"";
        assert_eq!(resolve(text, 8, '"', false).as_deref(), Some("hello there"));
        assert_eq!(
            resolve(text, 8, '"', true).as_deref(),
            Some("\"hello there\" ")
        );
        assert_eq!(
            resolve(text, 0, '"', false).as_deref(),
            Some("hello there"),
            "before the pair still finds it"
        );
        assert_eq!(
            resolve(text, 24, '"', false).as_deref(),
            Some("other"),
            "the next line has its own pair"
        );
    }

    #[test]
    fn quote_objects_fail_when_the_line_has_no_pair() {
        assert_eq!(resolve("one \" two", 0, '"', false), None);
    }

    #[test]
    fn bracket_objects_are_nesting_aware() {
        let text = "f(a, g(b, c), d)";
        assert_eq!(resolve(text, 10, '(', false).as_deref(), Some("b, c"));
        assert_eq!(resolve(text, 10, ')', true).as_deref(), Some("(b, c)"));
        assert_eq!(
            resolve(text, 3, 'b', false).as_deref(),
            Some("a, g(b, c), d"),
            "outside the inner pair the outer one wins"
        );
        assert_eq!(
            resolve(text, 6, '(', false).as_deref(),
            Some("b, c"),
            "sitting on the open bracket selects its own pair"
        );
        assert_eq!(
            resolve(text, 11, '(', false).as_deref(),
            Some("b, c"),
            "sitting on the close bracket selects its own pair"
        );
    }

    #[test]
    fn bracket_objects_span_lines_and_handle_the_curly_aliases() {
        let text = "fn main() {\n    body\n}";
        assert_eq!(
            resolve(text, 16, 'B', false).as_deref(),
            Some("\n    body\n")
        );
        assert_eq!(
            resolve(text, 16, '}', true).as_deref(),
            Some("{\n    body\n}")
        );
    }

    #[test]
    fn an_empty_pair_resolves_to_an_empty_range() {
        assert_eq!(resolve("f()", 1, '(', false).as_deref(), Some(""));
    }

    #[test]
    fn brackets_outside_any_pair_resolve_to_nothing() {
        assert_eq!(resolve("no brackets here", 3, '(', false), None);
        assert_eq!(resolve("(closed) after", 12, '(', false), None);
    }

    #[test]
    fn unknown_object_keys_are_rejected() {
        assert!(TextObject::from_key('z', false).is_none());
        assert!(TextObject::from_key('p', true).is_none());
    }
}
