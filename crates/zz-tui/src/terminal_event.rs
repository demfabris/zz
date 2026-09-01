//! Bounded decoding for the terminal input protocols enabled by `tty`.

use std::ops::BitOr;

const MAX_BUFFER_BYTES: usize = 4 * 1024 * 1024;
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Event {
    CellSize { width_px: u32, height_px: u32 },
    DeviceAttributes,
    FocusGained,
    FocusLost,
    Key(KeyEvent),
    KittyGraphicsResponse { image_id: u32, ok: bool },
    Mouse(MouseEvent),
    Paste(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
}

impl KeyEvent {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyCode {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    F(u8),
    Char(char),
    Esc,
    Unidentified,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const ALT: Self = Self(1 << 1);
    pub const CONTROL: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Default)]
pub(crate) struct EventParser {
    bytes: Vec<u8>,
    paste: Vec<u8>,
    in_paste: bool,
}

impl EventParser {
    pub fn push(&mut self, input: &[u8], output: &mut Vec<Event>) {
        let remaining = MAX_BUFFER_BYTES.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&input[..input.len().min(remaining)]);
        self.parse(output);
    }

    pub fn has_pending_escape(&self) -> bool {
        self.bytes.first() == Some(&0x1b)
    }

    pub fn flush_escape(&mut self, output: &mut Vec<Event>) {
        if self.has_pending_escape() {
            if self.bytes.starts_with(b"\x1b[?") || self.bytes.starts_with(b"\x1b_") {
                return;
            }
            self.bytes.remove(0);
            output.push(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
            self.parse(output);
        }
    }

    fn parse(&mut self, output: &mut Vec<Event>) {
        loop {
            if self.in_paste {
                let Some(end) = find_subslice(&self.bytes, PASTE_END) else {
                    let retain = PASTE_END.len().saturating_sub(1).min(self.bytes.len());
                    let drain = self.bytes.len().saturating_sub(retain);
                    let available = MAX_BUFFER_BYTES.saturating_sub(self.paste.len());
                    self.paste
                        .extend_from_slice(&self.bytes[..drain.min(available)]);
                    self.bytes.drain(..drain);
                    return;
                };
                let available = MAX_BUFFER_BYTES.saturating_sub(self.paste.len());
                self.paste
                    .extend_from_slice(&self.bytes[..end.min(available)]);
                self.bytes.drain(..end + PASTE_END.len());
                self.in_paste = false;
                output.push(Event::Paste(
                    String::from_utf8_lossy(&std::mem::take(&mut self.paste)).into_owned(),
                ));
                continue;
            }

            if self.bytes.starts_with(PASTE_START) {
                self.bytes.drain(..PASTE_START.len());
                self.in_paste = true;
                continue;
            }
            let Some(parsed) = parse_one(&self.bytes) else {
                return;
            };
            self.bytes.drain(..parsed.consumed);
            if let Some(event) = parsed.event {
                output.push(event);
            }
        }
    }
}

struct Parsed {
    consumed: usize,
    event: Option<Event>,
}

fn parse_one(bytes: &[u8]) -> Option<Parsed> {
    if bytes.starts_with(b"Gi=") {
        let terminator = find_subslice(bytes, b"\x1b\\")?;
        return Some(Parsed {
            consumed: terminator + 2,
            event: parse_kitty_graphics_response(&bytes[..terminator]),
        });
    }
    let first = *bytes.first()?;
    if first == 0x1b {
        return parse_escape(bytes);
    }
    if first < 0x20 || first == 0x7f {
        return Some(Parsed {
            consumed: 1,
            event: Some(Event::Key(control_key(first))),
        });
    }
    let width = utf8_width(first);
    if bytes.len() < width {
        return None;
    }
    let character = std::str::from_utf8(&bytes[..width])
        .ok()
        .and_then(|text| text.chars().next());
    Some(Parsed {
        consumed: width.max(1),
        event: character.map(|character| {
            let modifiers = if character.is_ascii_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            Event::Key(KeyEvent::new(KeyCode::Char(character), modifiers))
        }),
    })
}

fn parse_escape(bytes: &[u8]) -> Option<Parsed> {
    let second = *bytes.get(1)?;
    if second == b'_' {
        let terminator = find_subslice(&bytes[2..], b"\x1b\\")? + 2;
        let event = bytes.get(2..terminator).and_then(parse_application_command);
        return Some(Parsed {
            consumed: terminator + 2,
            event,
        });
    }
    if second == b'[' {
        let final_index = bytes
            .iter()
            .enumerate()
            .skip(2)
            .find_map(|(index, byte)| (0x40..=0x7e).contains(byte).then_some(index));
        let Some(final_index) = final_index else {
            return (bytes.len() > 64).then_some(Parsed {
                consumed: 1,
                event: Some(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))),
            });
        };
        let parameters = std::str::from_utf8(&bytes[2..final_index]).unwrap_or_default();
        return Some(Parsed {
            consumed: final_index + 1,
            event: parse_csi(parameters, bytes[final_index]),
        });
    }
    if second == b'O' {
        let final_byte = *bytes.get(2)?;
        return Some(Parsed {
            consumed: 3,
            event: ss3_key(final_byte).map(Event::Key),
        });
    }

    let parsed = parse_one(&bytes[1..])?;
    let event = match parsed.event {
        Some(Event::Key(mut key)) => {
            key.modifiers = key.modifiers | KeyModifiers::ALT;
            Some(Event::Key(key))
        }
        _ => Some(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))),
    };
    Some(Parsed {
        consumed: parsed.consumed + 1,
        event,
    })
}

fn parse_csi(parameters: &str, final_byte: u8) -> Option<Event> {
    match (parameters, final_byte) {
        (parameters, b'c') if parameters.starts_with('?') => {
            return Some(Event::DeviceAttributes);
        }
        ("", b'I') => return Some(Event::FocusGained),
        ("", b'O') => return Some(Event::FocusLost),
        (parameters, b'M' | b'm') if parameters.starts_with('<') => {
            return parse_sgr_mouse(parameters, final_byte).map(Event::Mouse);
        }
        (_, b'u') => return parse_kitty_key(parameters).map(Event::Key),
        (parameters, b't') => return parse_cell_size(parameters),
        _ => {}
    }

    let mut fields = parameters.split(';');
    let first = fields.next().unwrap_or_default();
    // Kitty report-event-types puts `modifiers:event` in field two: 2 repeat, 3 release.
    let mut modifier_parts = fields.next().unwrap_or_default().split(':');
    let modifier = modifier_parts
        .next()
        .and_then(|field| field.parse::<u8>().ok())
        .map_or(KeyModifiers::NONE, legacy_modifiers);
    let kind = match modifier_parts
        .next()
        .and_then(|field| field.parse::<u8>().ok())
    {
        Some(2) => KeyEventKind::Repeat,
        Some(3) => KeyEventKind::Release,
        _ => KeyEventKind::Press,
    };
    let first = first.split(':').next().unwrap_or_default();
    let code = match final_byte {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'Z' => KeyCode::BackTab,
        b'~' => tilde_key(first.parse().ok()?)?,
        _ => return None,
    };
    let modifier = if matches!(code, KeyCode::BackTab) {
        modifier | KeyModifiers::SHIFT
    } else {
        modifier
    };
    Some(Event::Key(KeyEvent {
        code,
        modifiers: modifier,
        kind,
    }))
}

fn parse_application_command(bytes: &[u8]) -> Option<Event> {
    parse_kitty_graphics_response(bytes)
}

fn parse_kitty_graphics_response(bytes: &[u8]) -> Option<Event> {
    let start = find_subslice(bytes, b"Gi=")?;
    let response = bytes.get(start + 3..)?;
    let separator = response.iter().position(|byte| *byte == b';')?;
    let image_id = std::str::from_utf8(&response[..separator])
        .ok()?
        .parse::<u32>()
        .ok()?;
    let message = &response[separator + 1..];
    let message_end = message
        .iter()
        .position(|byte| *byte == 0x1b)
        .unwrap_or(message.len());
    Some(Event::KittyGraphicsResponse {
        image_id,
        ok: &message[..message_end] == b"OK",
    })
}

fn parse_cell_size(parameters: &str) -> Option<Event> {
    let mut fields = parameters.split(';');
    if fields.next()? != "6" {
        return None;
    }
    let height_px = fields.next()?.parse::<u32>().ok()?;
    let width_px = fields.next()?.parse::<u32>().ok()?;
    (width_px > 0 && height_px > 0).then_some(Event::CellSize {
        width_px,
        height_px,
    })
}

fn parse_kitty_key(parameters: &str) -> Option<KeyEvent> {
    let mut fields = parameters.split(';');
    let mut codepoints = fields.next()?.split(':');
    let codepoint = codepoints.next()?.parse::<u32>().ok()?;
    let shifted_codepoint = codepoints
        .next()
        .and_then(|value| value.parse::<u32>().ok());
    let modifier_field = fields.next().unwrap_or("1");
    let mut modifier_parts = modifier_field.split(':');
    let modifiers = modifier_parts
        .next()
        .and_then(|field| field.parse::<u8>().ok())
        .map_or(KeyModifiers::NONE, legacy_modifiers);
    let kind = match modifier_parts
        .next()
        .and_then(|field| field.parse::<u8>().ok())
    {
        Some(2) => KeyEventKind::Repeat,
        Some(3) => KeyEventKind::Release,
        _ => KeyEventKind::Press,
    };
    Some(KeyEvent {
        code: kitty_key(if modifiers.contains(KeyModifiers::SHIFT) {
            shifted_codepoint.unwrap_or(codepoint)
        } else {
            codepoint
        }),
        modifiers,
        kind,
    })
}

fn kitty_key(codepoint: u32) -> KeyCode {
    match codepoint {
        27 | 57_344 => KeyCode::Esc,
        13 | 57_345 => KeyCode::Enter,
        9 | 57_346 => KeyCode::Tab,
        8 | 127 | 57_347 => KeyCode::Backspace,
        57_348 => KeyCode::Insert,
        57_349 => KeyCode::Delete,
        57_350 => KeyCode::Left,
        57_351 => KeyCode::Right,
        57_352 => KeyCode::Up,
        57_353 => KeyCode::Down,
        57_354 => KeyCode::PageUp,
        57_355 => KeyCode::PageDown,
        57_356 => KeyCode::Home,
        57_357 => KeyCode::End,
        57_364..=57_427 => KeyCode::F(u8::try_from(codepoint - 57_363).unwrap_or(u8::MAX)),
        _ => char::from_u32(codepoint).map_or(KeyCode::Unidentified, KeyCode::Char),
    }
}

fn parse_sgr_mouse(parameters: &str, final_byte: u8) -> Option<MouseEvent> {
    let mut fields = parameters.trim_start_matches('<').split(';');
    let encoded = fields.next()?.parse::<u16>().ok()?;
    let column = fields.next()?.parse::<u16>().ok()?.saturating_sub(1);
    let row = fields.next()?.parse::<u16>().ok()?.saturating_sub(1);
    let button = match encoded & 0b11 {
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        _ => MouseButton::Left,
    };
    let kind = if encoded & 64 != 0 {
        match encoded & 0b11 {
            0 => MouseEventKind::ScrollUp,
            1 => MouseEventKind::ScrollDown,
            2 => MouseEventKind::ScrollLeft,
            _ => MouseEventKind::ScrollRight,
        }
    } else if final_byte == b'm' {
        MouseEventKind::Up(button)
    } else if encoded & 32 != 0 {
        if encoded & 0b11 == 3 {
            MouseEventKind::Moved
        } else {
            MouseEventKind::Drag(button)
        }
    } else {
        MouseEventKind::Down(button)
    };
    let mut modifiers = KeyModifiers::NONE;
    if encoded & 4 != 0 {
        modifiers = modifiers | KeyModifiers::SHIFT;
    }
    if encoded & 8 != 0 {
        modifiers = modifiers | KeyModifiers::ALT;
    }
    if encoded & 16 != 0 {
        modifiers = modifiers | KeyModifiers::CONTROL;
    }
    Some(MouseEvent {
        kind,
        column,
        row,
        modifiers,
    })
}

fn control_key(byte: u8) -> KeyEvent {
    match byte {
        0 => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
        8 | 127 => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        9 => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        10 | 13 => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        1..=26 => KeyEvent::new(
            KeyCode::Char(char::from(b'a' + byte - 1)),
            KeyModifiers::CONTROL,
        ),
        28 => KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::CONTROL),
        29 => KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL),
        30 => KeyEvent::new(KeyCode::Char('^'), KeyModifiers::CONTROL),
        31 => KeyEvent::new(KeyCode::Char('_'), KeyModifiers::CONTROL),
        _ => KeyEvent::new(KeyCode::Unidentified, KeyModifiers::NONE),
    }
}

fn ss3_key(final_byte: u8) -> Option<KeyEvent> {
    let code = match final_byte {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'R' => KeyCode::F(3),
        b'S' => KeyCode::F(4),
        _ => return None,
    };
    Some(KeyEvent::new(code, KeyModifiers::NONE))
}

fn tilde_key(number: u16) -> Option<KeyCode> {
    match number {
        1 | 7 => Some(KeyCode::Home),
        2 => Some(KeyCode::Insert),
        3 => Some(KeyCode::Delete),
        4 | 8 => Some(KeyCode::End),
        5 => Some(KeyCode::PageUp),
        6 => Some(KeyCode::PageDown),
        11..=15 => Some(KeyCode::F(u8::try_from(number - 10).ok()?)),
        17..=21 => Some(KeyCode::F(u8::try_from(number - 11).ok()?)),
        23..=24 => Some(KeyCode::F(u8::try_from(number - 12).ok()?)),
        _ => None,
    }
}

fn legacy_modifiers(value: u8) -> KeyModifiers {
    let bits = value.saturating_sub(1);
    let mut modifiers = KeyModifiers::NONE;
    if bits & 1 != 0 {
        modifiers = modifiers | KeyModifiers::SHIFT;
    }
    if bits & 2 != 0 {
        modifiers = modifiers | KeyModifiers::ALT;
    }
    if bits & 4 != 0 {
        modifiers = modifiers | KeyModifiers::CONTROL;
    }
    if bits & 8 != 0 {
        modifiers = modifiers | KeyModifiers::SUPER;
    }
    modifiers
}

fn utf8_width(first: u8) -> usize {
    match first {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 1,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|candidate| candidate == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kitty::FILE_PROBE_IMAGE_ID;

    fn parse(bytes: &[u8]) -> Vec<Event> {
        let mut parser = EventParser::default();
        let mut events = Vec::new();
        parser.push(bytes, &mut events);
        events
    }

    #[test]
    fn parses_text_control_and_legacy_keys() {
        let events = parse(b"A\x1b[1;5D\x1bOP\x1c");
        assert_eq!(events.len(), 4);
        assert!(matches!(
            events[0],
            Event::Key(KeyEvent {
                code: KeyCode::Char('A'),
                modifiers: KeyModifiers::SHIFT,
                ..
            })
        ));
        assert!(matches!(
            events[1],
            Event::Key(KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::CONTROL,
                ..
            })
        ));
        assert!(matches!(
            events[2],
            Event::Key(KeyEvent {
                code: KeyCode::F(1),
                ..
            })
        ));
        assert!(matches!(
            events[3],
            Event::Key(KeyEvent {
                code: KeyCode::Char('\\'),
                modifiers: KeyModifiers::CONTROL,
                ..
            })
        ));
    }

    #[test]
    fn legacy_functional_keys_honor_kitty_event_types() {
        let events = parse(b"\x1b[A\x1b[1;1:2A\x1b[1;1:3A\x1b[1;5:3C");
        assert_eq!(events.len(), 4);
        assert!(matches!(
            events[0],
            Event::Key(KeyEvent {
                code: KeyCode::Up,
                kind: KeyEventKind::Press,
                ..
            })
        ));
        assert!(matches!(
            events[1],
            Event::Key(KeyEvent {
                code: KeyCode::Up,
                kind: KeyEventKind::Repeat,
                ..
            })
        ));
        assert!(matches!(
            events[2],
            Event::Key(KeyEvent {
                code: KeyCode::Up,
                kind: KeyEventKind::Release,
                ..
            })
        ));
        assert!(matches!(
            events[3],
            Event::Key(KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Release,
            })
        ));
    }

    #[test]
    fn parses_kitty_release_and_sgr_mouse() {
        let events = parse(b"\x1b[57352;5:3u\x1b[<20;17;9M");
        assert!(matches!(
            events[0],
            Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Release,
            })
        ));
        assert_eq!(
            events[1],
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 16,
                row: 8,
                modifiers: KeyModifiers::SHIFT | KeyModifiers::CONTROL,
            })
        );
    }

    #[test]
    fn bracketed_paste_can_span_reads() {
        let mut parser = EventParser::default();
        let mut events = Vec::new();
        parser.push(b"\x1b[200~hello\x1b[20", &mut events);
        assert!(events.is_empty());
        parser.push(b"1~", &mut events);
        assert_eq!(events, vec![Event::Paste("hello".to_owned())]);
    }

    #[test]
    fn bracketed_paste_swallows_control_bytes_the_pin_would_time() {
        let mut parser = EventParser::default();
        let mut events = Vec::new();
        parser.push(b"\x1b[200~a\x02b\x1b[201~\x02", &mut events);
        assert_eq!(
            events,
            vec![
                Event::Paste("a\u{2}b".to_owned()),
                Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
            ],
            "pinned tmux only times key arrivals for assume-paste-time when the client terminal lacks bracketed paste; the raw TUI always arms it, so a pasted C-b never reaches the key path"
        );
    }

    #[test]
    fn parses_terminal_cell_pixel_report() {
        assert_eq!(
            parse(b"\x1b[6;18;9t"),
            vec![Event::CellSize {
                width_px: 9,
                height_px: 18,
            }]
        );
    }

    #[test]
    fn kitty_response_and_device_attributes_can_span_reads() {
        let mut parser = EventParser::default();
        let mut events = Vec::new();
        parser.push(b"\x1b_Gi=42;O", &mut events);
        assert!(events.is_empty());
        parser.flush_escape(&mut events);
        assert!(events.is_empty());
        parser.push(b"K\x1b\\\x1b[?1;", &mut events);
        assert_eq!(
            events,
            vec![Event::KittyGraphicsResponse {
                image_id: 42,
                ok: true,
            }]
        );
        parser.push(b"2c", &mut events);
        assert_eq!(events.last(), Some(&Event::DeviceAttributes));
    }

    #[test]
    fn file_probe_response_accepts_a_relay_stripped_prefix_and_rejects_errors() {
        let prefixed = format!("\x1b_Gi={FILE_PROBE_IMAGE_ID};OK\x1b\\");
        let stripped = format!("Gi={FILE_PROBE_IMAGE_ID};OK\x1b\\");
        let error = format!("Gi={FILE_PROBE_IMAGE_ID};ENOENT\x1b\\");
        let expected = Event::KittyGraphicsResponse {
            image_id: FILE_PROBE_IMAGE_ID,
            ok: true,
        };
        assert_eq!(parse(prefixed.as_bytes()), vec![expected.clone()]);
        assert_eq!(parse(stripped.as_bytes()), vec![expected]);
        assert_eq!(
            parse(error.as_bytes()),
            vec![Event::KittyGraphicsResponse {
                image_id: FILE_PROBE_IMAGE_ID,
                ok: false,
            }]
        );
    }

    #[test]
    fn non_kitty_application_commands_are_swallowed() {
        assert_eq!(
            parse(b"\x1b_not-kitty;garbage\x1b\\x"),
            vec![Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::NONE,
            ))]
        );
    }

    #[test]
    fn lone_escape_is_flushed_without_losing_following_input() {
        let mut parser = EventParser::default();
        let mut events = Vec::new();
        parser.push(b"\x1b", &mut events);
        assert!(parser.has_pending_escape());
        parser.flush_escape(&mut events);
        assert!(matches!(
            events.as_slice(),
            [Event::Key(KeyEvent {
                code: KeyCode::Esc,
                ..
            })]
        ));
    }
}
