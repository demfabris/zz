use serde::{Deserialize, Deserializer, Serialize};

/// A physical/logical key understood by the terminal encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyCode {
    Character(char),
    Backspace,
    Enter,
    Tab,
    Escape,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Function(u8),
    Unidentified,
}

/// The phase of a key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyAction {
    Press,
    Repeat,
    Release,
}

/// Keyboard modifiers carried with a key event in one byte.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Modifiers(u8);

impl Modifiers {
    const SHIFT: u8 = 1 << 0;
    const CONTROL: u8 = 1 << 1;
    const ALT: u8 = 1 << 2;
    const PLATFORM: u8 = 1 << 3;
    const ALL: u8 = Self::SHIFT | Self::CONTROL | Self::ALT | Self::PLATFORM;

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
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub(crate) const fn from_packed_bits(bits: u8) -> Self {
        Self(bits & Self::ALL)
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
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

impl<'de> Deserialize<'de> for Modifiers {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bits = u8::deserialize(deserializer)?;
        Self::from_bits(bits)
            .ok_or_else(|| serde::de::Error::custom("terminal modifiers contain reserved bits"))
    }
}

/// Renderer-independent input passed to libghostty's key encoder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyInput {
    pub action: KeyAction,
    pub key: KeyCode,
    pub modifiers: Modifiers,
    pub text: Option<Box<str>>,
    pub unshifted_codepoint: Option<char>,
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn input_records_keep_their_packed_layout() {
        assert_eq!(size_of::<Modifiers>(), 1);
        assert_eq!(align_of::<Modifiers>(), align_of::<u8>());
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size_of::<KeyInput>(), 32);
    }

    #[test]
    fn modifier_mask_round_trips_every_supported_flag() {
        let modifiers = Modifiers::new(true, true, true, true);
        assert!(modifiers.shift());
        assert!(modifiers.control());
        assert!(modifiers.alt());
        assert!(modifiers.platform());
        assert_eq!(Modifiers::from_bits(modifiers.bits()), Some(modifiers));
        assert_eq!(Modifiers::from_bits(0xf0), None);

        let mut parsed = Modifiers::default();
        parsed.set_control(true);
        parsed.set_alt(true);
        assert!(parsed.control());
        assert!(parsed.alt());
    }
}
