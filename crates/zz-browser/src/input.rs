#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub window_zoom: f32,
    pub screen_x: i32,
    pub screen_y: i32,
    pub visible: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            scale_factor: 0.0,
            window_zoom: 1.0,
            screen_x: 0,
            screen_y: 0,
            visible: false,
        }
    }
}

impl Viewport {
    #[must_use]
    pub fn sanitized(self) -> Self {
        Self {
            scale_factor: if self.scale_factor.is_finite() {
                self.scale_factor.clamp(0.5, 8.0)
            } else {
                1.0
            },
            window_zoom: if self.window_zoom.is_finite() && self.window_zoom > 0.0 {
                self.window_zoom.clamp(0.25, 4.0)
            } else {
                1.0
            },
            ..self
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers(u8);

impl Modifiers {
    const SHIFT: u8 = 1 << 0;
    const CONTROL: u8 = 1 << 1;
    const ALT: u8 = 1 << 2;
    const PLATFORM: u8 = 1 << 3;
    const LEFT_MOUSE: u8 = 1 << 4;
    const MIDDLE_MOUSE: u8 = 1 << 5;
    const RIGHT_MOUSE: u8 = 1 << 6;
    const IS_REPEAT: u8 = 1 << 7;

    #[must_use]
    #[allow(
        clippy::fn_params_excessive_bools,
        reason = "the constructor is the explicit packing boundary for four platform flags"
    )]
    pub const fn new(shift: bool, control: bool, alt: bool, platform: bool) -> Self {
        let mut bits = 0;
        if shift {
            bits |= Self::SHIFT;
        }
        if control {
            bits |= Self::CONTROL;
        }
        if alt {
            bits |= Self::ALT;
        }
        if platform {
            bits |= Self::PLATFORM;
        }
        Self(bits)
    }

    #[must_use]
    pub const fn with_pointer_button(mut self, button: Option<PointerButton>) -> Self {
        self.0 &= !(Self::LEFT_MOUSE | Self::MIDDLE_MOUSE | Self::RIGHT_MOUSE);
        self.0 |= match button {
            Some(PointerButton::Left) => Self::LEFT_MOUSE,
            Some(PointerButton::Middle) => Self::MIDDLE_MOUSE,
            Some(PointerButton::Right) => Self::RIGHT_MOUSE,
            None => 0,
        };
        self
    }

    #[must_use]
    pub const fn with_repeat(mut self, enabled: bool) -> Self {
        if enabled {
            self.0 |= Self::IS_REPEAT;
        } else {
            self.0 &= !Self::IS_REPEAT;
        }
        self
    }

    #[must_use]
    pub const fn shift(self) -> bool {
        self.0 & Self::SHIFT != 0
    }

    #[must_use]
    pub const fn control(self) -> bool {
        self.0 & Self::CONTROL != 0
    }

    #[must_use]
    pub const fn alt(self) -> bool {
        self.0 & Self::ALT != 0
    }

    #[must_use]
    pub const fn platform(self) -> bool {
        self.0 & Self::PLATFORM != 0
    }

    #[must_use]
    pub const fn left_mouse(self) -> bool {
        self.0 & Self::LEFT_MOUSE != 0
    }

    #[must_use]
    pub const fn middle_mouse(self) -> bool {
        self.0 & Self::MIDDLE_MOUSE != 0
    }

    #[must_use]
    pub const fn right_mouse(self) -> bool {
        self.0 & Self::RIGHT_MOUSE != 0
    }

    #[must_use]
    pub const fn is_repeat(self) -> bool {
        self.0 & Self::IS_REPEAT != 0
    }

    pub fn set_control(&mut self, enabled: bool) {
        self.set(Self::CONTROL, enabled);
    }

    pub fn set_alt(&mut self, enabled: bool) {
        self.set(Self::ALT, enabled);
    }

    fn set(&mut self, mask: u8, enabled: bool) {
        if enabled {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
}

/// Editing commands routed to the focused frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteAndMatchStyle,
    SelectAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Move,
    Leave,
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    pub x: i32,
    pub y: i32,
    pub phase: PointerPhase,
    pub button: Option<PointerButton>,
    pub click_count: i32,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelEvent {
    pub x: i32,
    pub y: i32,
    pub delta_x: i32,
    pub delta_y: i32,
    pub precise: bool,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserKey {
    Character(char),
    Backspace,
    Tab,
    Enter,
    Escape,
    Space,
    PageUp,
    PageDown,
    End,
    Home,
    ArrowLeft,
    ArrowUp,
    ArrowRight,
    ArrowDown,
    Insert,
    Delete,
    Function(u8),
    Unidentified,
}

impl BrowserKey {
    /// Chromium's cross-platform virtual-key representation.
    #[must_use]
    pub fn windows_key_code(self) -> i32 {
        match self {
            Self::Backspace => 0x08,
            Self::Tab => 0x09,
            Self::Enter => 0x0d,
            Self::Escape => 0x1b,
            Self::Space => 0x20,
            Self::PageUp => 0x21,
            Self::PageDown => 0x22,
            Self::End => 0x23,
            Self::Home => 0x24,
            Self::ArrowLeft => 0x25,
            Self::ArrowUp => 0x26,
            Self::ArrowRight => 0x27,
            Self::ArrowDown => 0x28,
            Self::Insert => 0x2d,
            Self::Delete => 0x2e,
            Self::Function(number @ 1..=24) => 0x70 + i32::from(number) - 1,
            Self::Character(character) => {
                i32::try_from(u32::from(character.to_ascii_uppercase())).unwrap_or(0)
            }
            Self::Unidentified | Self::Function(_) => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyInput {
    pub action: KeyAction,
    pub key: BrowserKey,
    pub modifiers: Modifiers,
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn input_records_keep_their_packed_layout() {
        assert_eq!(size_of::<Modifiers>(), 1);
        assert_eq!(align_of::<Modifiers>(), align_of::<u8>());
        assert_eq!(size_of::<KeyInput>(), 12);
        assert_eq!(size_of::<PointerEvent>(), 16);
        assert_eq!(size_of::<WheelEvent>(), 20);
    }

    #[test]
    fn modifier_mask_tracks_keyboard_pointer_and_repeat_flags() {
        let modifiers = Modifiers::new(true, true, true, true)
            .with_pointer_button(Some(PointerButton::Middle))
            .with_repeat(true);
        assert!(modifiers.shift());
        assert!(modifiers.control());
        assert!(modifiers.alt());
        assert!(modifiers.platform());
        assert!(!modifiers.left_mouse());
        assert!(modifiers.middle_mouse());
        assert!(!modifiers.right_mouse());
        assert!(modifiers.is_repeat());
    }

    #[test]
    fn sanitizes_scale_factor() {
        assert!(
            (Viewport {
                scale_factor: f32::NAN,
                ..Viewport::default()
            }
            .sanitized()
            .scale_factor
                - 1.0)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (Viewport {
                scale_factor: 20.0,
                ..Viewport::default()
            }
            .sanitized()
            .scale_factor
                - 8.0)
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn maps_virtual_keys() {
        assert_eq!(BrowserKey::ArrowLeft.windows_key_code(), 0x25);
        assert_eq!(BrowserKey::Function(12).windows_key_code(), 0x7b);
        assert_eq!(BrowserKey::Character('a').windows_key_code(), 0x41);
    }
}
