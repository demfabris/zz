#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChromeColor {
    Background,
    Foreground,
    Success,
    Warning,
    Danger,
}

impl ChromeColor {
    pub const ALL: [Self; 5] = [
        Self::Background,
        Self::Foreground,
        Self::Success,
        Self::Warning,
        Self::Danger,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Background => "chrome-background",
            Self::Foreground => "chrome-foreground",
            Self::Success => "chrome-success",
            Self::Warning => "chrome-warning",
            Self::Danger => "chrome-danger",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.as_str() == key)
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::Foreground => "Foreground",
            Self::Success => "Success",
            Self::Warning => "Warning",
            Self::Danger => "Danger",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Background => {
                "The window's base plane. Every panel, popover and hover state is this color, \
                 raised."
            }
            Self::Foreground => {
                "Default text, and the source of muted text, focus rings, links, selection and every edge."
            }
            Self::Success => "Something completed or is healthy.",
            Self::Warning => "Something needs attention but still works.",
            Self::Danger => "Something failed or is destructive.",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeModeSetting {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeModeSetting {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.as_str() == value.trim())
    }

    pub const fn pinned(self) -> Option<bool> {
        match self {
            Self::System => None,
            Self::Light => Some(false),
            Self::Dark => Some(true),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromePresetId {
    TokyoNight,
    Catppuccin,
    Gruvbox,
    Nord,
    Breeze,
    Adwaita,
    Ubuntu,
    RosePine,
    Ayu,
    Solarized,
    MacosClassic,
}

impl ChromePresetId {
    pub const ALL: [Self; 11] = [
        Self::TokyoNight,
        Self::Catppuccin,
        Self::Gruvbox,
        Self::Nord,
        Self::Breeze,
        Self::Adwaita,
        Self::Ubuntu,
        Self::RosePine,
        Self::Ayu,
        Self::Solarized,
        Self::MacosClassic,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TokyoNight => "tokyo-night",
            Self::Catppuccin => "catppuccin",
            Self::Gruvbox => "gruvbox",
            Self::Nord => "nord",
            Self::Breeze => "breeze",
            Self::Adwaita => "adwaita",
            Self::Ubuntu => "ubuntu",
            Self::RosePine => "rose-pine",
            Self::Ayu => "ayu",
            Self::Solarized => "solarized",
            Self::MacosClassic => "macos-classic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.as_str() == value.trim())
    }

    pub fn preset(self) -> &'static ChromePreset {
        CHROME_PRESETS
            .iter()
            .find(|preset| preset.id == self)
            .expect("every ChromePresetId has a built-in preset")
    }
}

pub struct ChromePreset {
    pub id: ChromePresetId,
    pub name: &'static str,
    pub light: [&'static str; ChromeColor::ALL.len()],
    pub dark: [&'static str; ChromeColor::ALL.len()],
}

impl ChromePreset {
    pub fn colors(&self, dark: bool) -> &[&'static str; ChromeColor::ALL.len()] {
        if dark { &self.dark } else { &self.light }
    }
}

pub const CHROME_PRESETS: [ChromePreset; 11] = [
    ChromePreset {
        id: ChromePresetId::TokyoNight,
        name: "Tokyo Night",
        light: ["#e1e2e7", "#3760bf", "#587539", "#8c6c3e", "#f52a65"],
        dark: ["#1a1b26", "#c0caf5", "#9ece6a", "#e0af68", "#f7768e"],
    },
    ChromePreset {
        id: ChromePresetId::Catppuccin,
        name: "Catppuccin",
        light: ["#eff1f5", "#4c4f69", "#40a02b", "#df8e1d", "#d20f39"],
        dark: ["#1e1e2e", "#cdd6f4", "#a6e3a1", "#f9e2af", "#f38ba8"],
    },
    ChromePreset {
        id: ChromePresetId::Gruvbox,
        name: "Gruvbox",
        light: ["#fbf1c7", "#3c3836", "#79740e", "#b57614", "#9d0006"],
        dark: ["#282828", "#ebdbb2", "#b8bb26", "#fabd2f", "#fb4934"],
    },
    ChromePreset {
        id: ChromePresetId::Nord,
        name: "Nord",
        light: ["#eceff4", "#2e3440", "#a3be8c", "#ebcb8b", "#bf616a"],
        dark: ["#2e3440", "#eceff4", "#a3be8c", "#ebcb8b", "#bf616a"],
    },
    ChromePreset {
        id: ChromePresetId::Breeze,
        name: "Breeze",
        light: ["#eff0f1", "#232629", "#27ae60", "#f67400", "#da4453"],
        dark: ["#202326", "#fcfcfc", "#27ae60", "#f67400", "#da4453"],
    },
    ChromePreset {
        id: ChromePresetId::Adwaita,
        name: "Adwaita",
        light: ["#fafafb", "#323237", "#007c3d", "#905400", "#c30000"],
        dark: ["#222226", "#ffffff", "#78e9ab", "#ffc252", "#ff938c"],
    },
    ChromePreset {
        id: ChromePresetId::Ubuntu,
        name: "Ubuntu",
        light: ["#fafafa", "#3d3d3d", "#109b26", "#f99b11", "#c7162b"],
        dark: ["#2c2c2c", "#f7f7f7", "#50c856", "#f99b11", "#ff5c5d"],
    },
    ChromePreset {
        id: ChromePresetId::RosePine,
        name: "Rosé Pine",
        light: ["#faf4ed", "#464261", "#286983", "#ea9d34", "#b4637a"],
        dark: ["#191724", "#e0def4", "#31748f", "#f6c177", "#eb6f92"],
    },
    ChromePreset {
        id: ChromePresetId::Ayu,
        name: "Ayu",
        light: ["#f8f9fa", "#5c6166", "#6cbf43", "#f29718", "#e65050"],
        dark: ["#0d1017", "#bfbdb6", "#70bf56", "#e6b450", "#d95757"],
    },
    ChromePreset {
        id: ChromePresetId::Solarized,
        name: "Solarized",
        light: ["#fdf6e3", "#586e75", "#859900", "#b58900", "#dc322f"],
        dark: ["#002b36", "#93a1a1", "#859900", "#b58900", "#dc322f"],
    },
    ChromePreset {
        id: ChromePresetId::MacosClassic,
        name: "macOS Classic",
        light: ["#ffffff", "#1a1a1a", "#036a07", "#9e7008", "#c5060b"],
        dark: ["#131313", "#caccca", "#62ba46", "#b0a878", "#d2602d"],
    },
];

/// What `app-icon` selects.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppIconSetting {
    /// Follow the OS appearance.
    #[default]
    Automatic,
    Light,
    Dark,
}

impl AppIconSetting {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::Light, Self::Dark];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|setting| setting.as_str() == value.trim())
    }
}

pub fn parse_hex(value: &str) -> Result<[f32; 4], String> {
    let digits = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("expected hexadecimal digits".to_owned());
    }
    // `#rgb` is the CSS shorthand: each digit doubles into a byte.
    let component = |index: usize, width: usize| -> f32 {
        let slice = &digits[index * width..(index + 1) * width];
        let byte = u8::from_str_radix(slice, 16).unwrap_or_default();
        f32::from(if width == 1 { byte * 17 } else { byte }) / 255.0
    };
    let (width, alpha) = match digits.len() {
        3 => (1, 1.0),
        6 => (2, 1.0),
        8 => (2, component(3, 2)),
        _ => return Err("expected #rgb, #rrggbb or #rrggbbaa".to_owned()),
    };
    Ok([
        component(0, width),
        component(1, width),
        component(2, width),
        alpha,
    ])
}
