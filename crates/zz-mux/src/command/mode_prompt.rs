use unicode_width::UnicodeWidthChar as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeKey {
    Char(char),
    Ctrl(char),
    Meta(char),
    Keypad(char),
    Up,
    Down,
    Left,
    Right,
    CtrlLeft,
    CtrlRight,
    ShiftUp,
    ShiftDown,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    F1,
    Other,
}

const CONTROL_NAMES: [&str; 32] = [
    "[NUL]", "[SOH]", "[STX]", "[ETX]", "[EOT]", "[ENQ]", "[ASC]", "[BEL]", "[BS]", "Tab", "[LF]",
    "[VT]", "[FF]", "Enter", "[SO]", "[SI]", "[DLE]", "[DC1]", "[DC2]", "[DC3]", "[DC4]", "[NAK]",
    "[SYN]", "[ETB]", "[CAN]", "[EM]", "[SUB]", "Escape", "[FS]", "[GS]", "[RS]", "[US]",
];

impl ModeKey {
    #[must_use]
    pub fn parse(name: &str) -> Self {
        let mut chars = name.chars();
        if let (Some(character), None) = (chars.next(), chars.next()) {
            return Self::Char(character);
        }
        let mut base = name;
        let (mut control, mut meta, mut shift) = (false, false, false);
        if let Some(rest) = base.strip_prefix('^')
            && !rest.is_empty()
        {
            control = true;
            base = rest;
        }
        loop {
            let bytes = base.as_bytes();
            if bytes.len() > 2 && bytes[1] == b'-' {
                match bytes[0] {
                    b'C' | b'c' => control = true,
                    b'M' | b'm' => meta = true,
                    b'S' | b's' => shift = true,
                    _ => break,
                }
                base = &base[2..];
            } else {
                break;
            }
        }
        let mut chars = base.chars();
        if let (Some(character), None) = (chars.next(), chars.next()) {
            return match (control, meta) {
                (false, false) => Self::Char(character),
                (true, false) => Self::Ctrl(character.to_ascii_lowercase()),
                (false, true) => Self::Meta(character),
                (true, true) => Self::Other,
            };
        }
        let named = match base {
            "Up" => Self::Up,
            "Down" => Self::Down,
            "Left" => Self::Left,
            "Right" => Self::Right,
            "Home" => Self::Home,
            "End" => Self::End,
            "PPage" | "PageUp" | "PgUp" => Self::PageUp,
            "NPage" | "PageDown" | "PgDn" => Self::PageDown,
            "BSpace" => Self::Backspace,
            "DC" | "Delete" => Self::Delete,
            "F1" => Self::F1,
            "Space" => Self::Char(' '),
            "KPEnter" => Self::Keypad('\r'),
            "KP/" => Self::Keypad('/'),
            "KP*" => Self::Keypad('*'),
            "KP-" => Self::Keypad('-'),
            "KP+" => Self::Keypad('+'),
            "KP." => Self::Keypad('.'),
            _ => base
                .strip_prefix("KP")
                .and_then(|digit| digit.parse::<u8>().ok())
                .filter(|digit| *digit < 10)
                .map_or_else(
                    || {
                        CONTROL_NAMES
                            .iter()
                            .position(|control| *control == base)
                            .and_then(|code| char::from_u32(code as u32))
                            .map_or(Self::Other, Self::Char)
                    },
                    |digit| Self::Keypad(char::from(b'0' + digit)),
                ),
        };
        match (named, control, meta, shift) {
            (named, false, false, false) => named,
            (Self::Left, true, false, false) => Self::CtrlLeft,
            (Self::Right, true, false, false) => Self::CtrlRight,
            (Self::Up, false, false, true) => Self::ShiftUp,
            (Self::Down, false, false, true) => Self::ShiftDown,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub const fn is_cancel(self) -> bool {
        matches!(self, Self::Char('\u{1b}') | Self::Ctrl('[' | 'c' | 'g'))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptOutcome {
    NotHandled,
    Move,
    Handled,
    Changed(char),
    Done,
    Cancelled,
    Closed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModePrompt {
    pub label: String,
    buffer: Vec<char>,
    index: usize,
    last: String,
    incremental: bool,
    edit_arrows: bool,
    single: bool,
    quote_next: bool,
    word_separators: String,
    copied: Option<Vec<char>>,
}

impl ModePrompt {
    #[must_use]
    pub fn new(label: impl Into<String>, input: &str, word_separators: &str) -> Self {
        let buffer = input.chars().collect::<Vec<_>>();
        Self {
            label: label.into(),
            index: buffer.len(),
            buffer,
            word_separators: word_separators.to_owned(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn incremental(label: impl Into<String>, input: &str, word_separators: &str) -> Self {
        Self {
            label: label.into(),
            last: input.to_owned(),
            incremental: true,
            edit_arrows: true,
            word_separators: word_separators.to_owned(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn single(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            single: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn input(&self) -> String {
        self.buffer.iter().collect()
    }

    pub fn key(&mut self, key: ModeKey) -> PromptOutcome {
        let key = match key {
            ModeKey::Keypad(character) => ModeKey::Char(character),
            key => key,
        };
        if self.single || self.quote_next {
            let character = match key {
                ModeKey::Backspace => '\u{7f}',
                ModeKey::Char(character) | ModeKey::Meta(character) => character,
                ModeKey::Ctrl(character) if character.is_ascii() => {
                    char::from(character as u8 & 0x1f)
                }
                _ => return PromptOutcome::Handled,
            };
            self.quote_next = false;
            return self.append(character);
        }
        if self.incremental {
            match key {
                ModeKey::Up | ModeKey::Down | ModeKey::PageUp | ModeKey::PageDown => {
                    return PromptOutcome::Move;
                }
                ModeKey::Left | ModeKey::Right if !self.edit_arrows => {
                    return PromptOutcome::Move;
                }
                _ => {}
            }
        }
        let size = self.buffer.len();
        let mut prefix = '=';
        match key {
            ModeKey::Left | ModeKey::Ctrl('b') => self.index = self.index.saturating_sub(1),
            ModeKey::Right | ModeKey::Ctrl('f') => self.index = (self.index + 1).min(size),
            ModeKey::Home | ModeKey::Ctrl('a') => self.index = 0,
            ModeKey::End | ModeKey::Ctrl('e') => self.index = size,
            ModeKey::Char('\t') | ModeKey::Up | ModeKey::Down | ModeKey::Ctrl('p' | 'n') => {}
            ModeKey::Backspace | ModeKey::Ctrl('h') => {
                if self.index == 0 {
                    return PromptOutcome::Handled;
                }
                self.index -= 1;
                self.buffer.remove(self.index);
                return self.changed(prefix);
            }
            ModeKey::Delete | ModeKey::Ctrl('d') => {
                if self.index == size {
                    return PromptOutcome::Handled;
                }
                self.buffer.remove(self.index);
                return self.changed(prefix);
            }
            ModeKey::Ctrl('u') => {
                self.buffer.clear();
                self.index = 0;
                return self.changed(prefix);
            }
            ModeKey::Ctrl('k') => {
                if self.index >= size {
                    return PromptOutcome::Handled;
                }
                self.buffer.truncate(self.index);
                return self.changed(prefix);
            }
            ModeKey::Ctrl('w') => {
                let start = self.backward_word(&self.word_separators.clone());
                self.copied = Some(self.buffer[start..self.index].to_vec());
                self.buffer.drain(start..self.index);
                self.index = start;
                return self.changed(prefix);
            }
            ModeKey::CtrlRight | ModeKey::Meta('f') => {
                self.forward_word();
                return self.changed(prefix);
            }
            ModeKey::CtrlLeft | ModeKey::Meta('b') => {
                self.index = self.backward_word(&self.word_separators.clone());
                return self.changed(prefix);
            }
            ModeKey::Ctrl('y') => {
                let Some(copied) = self.copied.clone() else {
                    return PromptOutcome::Handled;
                };
                let count = copied.len();
                self.buffer.splice(self.index..self.index, copied);
                self.index += count;
                return self.changed(prefix);
            }
            ModeKey::Ctrl('t') => {
                let mut index = self.index;
                if index < size {
                    index += 1;
                }
                if index < 2 {
                    return PromptOutcome::Handled;
                }
                self.buffer.swap(index - 2, index - 1);
                self.index = index;
                return self.changed(prefix);
            }
            ModeKey::Char('\r' | '\n') => return PromptOutcome::Done,
            key if key.is_cancel() => return PromptOutcome::Cancelled,
            ModeKey::Ctrl(direction @ ('r' | 's')) => {
                if !self.incremental {
                    return PromptOutcome::Handled;
                }
                if self.buffer.is_empty() {
                    self.buffer = self.last.chars().collect();
                    self.index = self.buffer.len();
                } else {
                    prefix = if direction == 'r' { '-' } else { '+' };
                }
                return self.changed(prefix);
            }
            ModeKey::Ctrl('v') => self.quote_next = true,
            ModeKey::Char(character) => return self.append(character),
            _ => {}
        }
        PromptOutcome::Handled
    }

    fn append(&mut self, character: char) -> PromptOutcome {
        self.buffer.insert(self.index, character);
        self.index += 1;
        if self.single {
            return if self.buffer.len() == 1 {
                PromptOutcome::Done
            } else {
                PromptOutcome::Closed
            };
        }
        self.changed('=')
    }

    fn changed(&self, prefix: char) -> PromptOutcome {
        if self.incremental {
            PromptOutcome::Changed(prefix)
        } else {
            PromptOutcome::Handled
        }
    }

    fn separator(&self, index: usize, separators: &str) -> bool {
        self.buffer
            .get(index)
            .is_some_and(|character| character.is_ascii() && separators.contains(*character))
    }

    fn space(&self, index: usize) -> bool {
        self.buffer.get(index) == Some(&' ')
    }

    fn backward_word(&self, separators: &str) -> usize {
        let mut index = self.index;
        while index != 0 {
            index -= 1;
            if !self.space(index) {
                break;
            }
        }
        let word_is_separators = self.separator(index, separators);
        while index != 0 {
            index -= 1;
            if self.space(index) || word_is_separators != self.separator(index, separators) {
                index += 1;
                break;
            }
        }
        index
    }

    fn forward_word(&mut self) {
        let size = self.buffer.len();
        let mut index = self.index;
        while index != size && self.space(index) {
            index += 1;
        }
        if index == size {
            self.index = index;
            return;
        }
        let separators = self.word_separators.clone();
        let word_is_separators = self.separator(index, &separators) && !self.space(index);
        loop {
            index += 1;
            if self.space(index)
                || index == size
                || word_is_separators != self.separator(index, &separators)
            {
                break;
            }
        }
        self.index = index;
    }

    #[must_use]
    pub fn draw(&self, width: u16) -> (String, u16) {
        let width = usize::from(width);
        let mut text = String::new();
        let mut start = 0;
        for character in self.label.chars() {
            let cell = character.width().unwrap_or(0);
            if start + cell > width {
                break;
            }
            start += cell;
            text.push(character);
        }
        let left = width - start;
        if left == 0 {
            return (text, u16::try_from(start).unwrap_or(u16::MAX));
        }
        let cell = |character: &char| {
            if (*character as u32) < 0x20 || *character == '\u{7f}' {
                2
            } else {
                character.width().unwrap_or(0)
            }
        };
        let cursor = self.buffer[..self.index].iter().map(cell).sum::<usize>();
        let mut visible =
            self.buffer.iter().map(cell).sum::<usize>() + usize::from(self.quote_next);
        let offset = if cursor >= left {
            visible = left;
            cursor - left + 1
        } else {
            0
        };
        visible = visible.min(left);
        let mut used = 0;
        for character in &self.buffer {
            let size = cell(character);
            if used < offset {
                used += size;
                continue;
            }
            if used >= offset + visible {
                break;
            }
            used += size;
            if used > offset + visible {
                break;
            }
            if size == 2 && ((*character as u32) < 0x20 || *character == '\u{7f}') {
                text.push('^');
                text.push(if *character == '\u{7f}' {
                    '?'
                } else {
                    char::from(*character as u8 | 0x40)
                });
            } else {
                text.push(*character);
            }
        }
        (
            text,
            u16::try_from(start + cursor - offset).unwrap_or(u16::MAX),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_value_scrolls_so_the_cursor_stays_on_the_row() {
        let value = "x".repeat(200);
        let prompt = ModePrompt::new("(status-format[+], global) ", &value, "");
        let (text, cursor) = prompt.draw(80);
        assert_eq!(cursor, 79);
        assert_eq!(text.chars().count(), 79);
        assert!(text.starts_with("(status-format[+], global) xx"));
    }

    #[test]
    fn keys_parse_into_pin_key_codes() {
        assert_eq!(ModeKey::parse("C-n"), ModeKey::Ctrl('n'));
        assert_eq!(ModeKey::parse("M-f"), ModeKey::Meta('f'));
        assert_eq!(ModeKey::parse("Enter"), ModeKey::Char('\r'));
        assert_eq!(ModeKey::parse("[ETX]"), ModeKey::Char('\u{3}'));
        assert_eq!(ModeKey::parse("Space"), ModeKey::Char(' '));
        assert_eq!(ModeKey::parse("KP5"), ModeKey::Keypad('5'));
        assert_eq!(ModeKey::parse("C-Left"), ModeKey::CtrlLeft);
        assert_eq!(ModeKey::parse("M--"), ModeKey::Meta('-'));
    }

    #[test]
    fn an_incremental_prompt_reports_edits_and_hands_movement_back() {
        let mut prompt = ModePrompt::incremental("(search) ", "", " ");
        assert_eq!(prompt.key(ModeKey::Char('z')), PromptOutcome::Changed('='));
        assert_eq!(prompt.key(ModeKey::Down), PromptOutcome::Move);
        assert_eq!(prompt.key(ModeKey::Home), PromptOutcome::Handled);
        assert_eq!(prompt.draw(80), ("(search) z".to_owned(), 9));
        assert_eq!(prompt.key(ModeKey::Backspace), PromptOutcome::Handled);
        assert_eq!(prompt.key(ModeKey::End), PromptOutcome::Handled);
        assert_eq!(prompt.key(ModeKey::Backspace), PromptOutcome::Changed('='));
        assert_eq!(prompt.input(), "");
    }
}
