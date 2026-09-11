#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChromeColor {
    Background,
    Foreground,
    Accent,
}

impl ChromeColor {
    pub const ALL: [Self; 3] = [Self::Background, Self::Foreground, Self::Accent];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Background => "chrome-background",
            Self::Foreground => "chrome-foreground",
            Self::Accent => "chrome-accent",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.as_str() == key)
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::Foreground => "Foreground",
            Self::Accent => "Accent",
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
            Self::Accent => {
                "The one chromatic emphasis: a checked switch, the selected tile ring, the active pane border, the send button."
            }
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
pub struct ChromePresetId(&'static str);

impl ChromePresetId {
    pub fn parse(value: &str) -> Option<Self> {
        CHROME_PRESETS
            .iter()
            .find(|preset| preset.id.as_str() == value.trim())
            .map(|preset| preset.id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }

    pub fn preset(self) -> &'static ChromePreset {
        CHROME_PRESETS
            .iter()
            .find(|preset| preset.id == self)
            .expect("every ChromePresetId has a built-in preset")
    }

    pub fn dark(self) -> bool {
        self.preset().dark
    }
}

pub struct ChromePreset {
    pub id: ChromePresetId,
    pub name: &'static str,
    pub dark: bool,
    pub background: &'static str,
    pub foreground: &'static str,
    pub accent: &'static str,
    pub success: &'static str,
    pub warning: &'static str,
    pub danger: &'static str,
}

pub fn chrome_presets(dark: bool) -> impl Iterator<Item = &'static ChromePreset> {
    CHROME_PRESETS
        .iter()
        .filter(move |preset| preset.dark == dark)
}

pub static CHROME_PRESETS: [ChromePreset; 34] = [
    ChromePreset {
        id: ChromePresetId("tokyo-night"),
        name: "Tokyo Night",
        dark: true,
        background: "#1a1b26",
        foreground: "#c0caf5",
        accent: "#7aa2f7",
        success: "#9ece6a",
        warning: "#e0af68",
        danger: "#f7768e",
    },
    ChromePreset {
        id: ChromePresetId("tokyo-night-storm"),
        name: "Tokyo Night Storm",
        dark: true,
        background: "#24283b",
        foreground: "#c0caf5",
        accent: "#7aa2f7",
        success: "#9ece6a",
        warning: "#e0af68",
        danger: "#f7768e",
    },
    ChromePreset {
        id: ChromePresetId("catppuccin-mocha"),
        name: "Catppuccin Mocha",
        dark: true,
        background: "#1e1e2e",
        foreground: "#cdd6f4",
        accent: "#cba6f7",
        success: "#a6e3a1",
        warning: "#f9e2af",
        danger: "#f38ba8",
    },
    ChromePreset {
        id: ChromePresetId("catppuccin-macchiato"),
        name: "Catppuccin Macchiato",
        dark: true,
        background: "#24273a",
        foreground: "#cad3f5",
        accent: "#c6a0f6",
        success: "#a6da95",
        warning: "#eed49f",
        danger: "#ed8796",
    },
    ChromePreset {
        id: ChromePresetId("catppuccin-frappe"),
        name: "Catppuccin Frappé",
        dark: true,
        background: "#303446",
        foreground: "#c6d0f5",
        accent: "#ca9ee6",
        success: "#a6d189",
        warning: "#e5c890",
        danger: "#e78284",
    },
    ChromePreset {
        id: ChromePresetId("gruvbox-dark"),
        name: "Gruvbox",
        dark: true,
        background: "#282828",
        foreground: "#ebdbb2",
        accent: "#fe8019",
        success: "#b8bb26",
        warning: "#fabd2f",
        danger: "#fb4934",
    },
    ChromePreset {
        id: ChromePresetId("nord"),
        name: "Nord",
        dark: true,
        background: "#2e3440",
        foreground: "#eceff4",
        accent: "#88c0d0",
        success: "#a3be8c",
        warning: "#ebcb8b",
        danger: "#c9767c",
    },
    ChromePreset {
        id: ChromePresetId("dracula"),
        name: "Dracula",
        dark: true,
        background: "#282a36",
        foreground: "#f8f8f2",
        accent: "#bd93f9",
        success: "#50fa7b",
        warning: "#f1fa8c",
        danger: "#ff5555",
    },
    ChromePreset {
        id: ChromePresetId("one-dark"),
        name: "One Dark",
        dark: true,
        background: "#282c34",
        foreground: "#b2b9c5",
        accent: "#61afef",
        success: "#98c379",
        warning: "#e5c07b",
        danger: "#e06c75",
    },
    ChromePreset {
        id: ChromePresetId("github-dark"),
        name: "GitHub Dark",
        dark: true,
        background: "#0d1117",
        foreground: "#f0f6fc",
        accent: "#58a6ff",
        success: "#3fb950",
        warning: "#d29922",
        danger: "#f85149",
    },
    ChromePreset {
        id: ChromePresetId("everforest-dark"),
        name: "Everforest",
        dark: true,
        background: "#2d353b",
        foreground: "#d3c6aa",
        accent: "#7fbbb3",
        success: "#a7c080",
        warning: "#dbbc7f",
        danger: "#e67e80",
    },
    ChromePreset {
        id: ChromePresetId("rose-pine"),
        name: "Rosé Pine",
        dark: true,
        background: "#191724",
        foreground: "#e0def4",
        accent: "#ebbcba",
        success: "#9ccfd8",
        warning: "#f6c177",
        danger: "#eb6f92",
    },
    ChromePreset {
        id: ChromePresetId("rose-pine-moon"),
        name: "Rosé Pine Moon",
        dark: true,
        background: "#232136",
        foreground: "#e0def4",
        accent: "#ea9a97",
        success: "#9ccfd8",
        warning: "#f6c177",
        danger: "#eb6f92",
    },
    ChromePreset {
        id: ChromePresetId("solarized-dark"),
        name: "Solarized",
        dark: true,
        background: "#002b36",
        foreground: "#a9b4b4",
        accent: "#268bd2",
        success: "#859900",
        warning: "#b58900",
        danger: "#e14941",
    },
    ChromePreset {
        id: ChromePresetId("ayu-dark"),
        name: "Ayu Dark",
        dark: true,
        background: "#0b0e14",
        foreground: "#bfbdb6",
        accent: "#59c2ff",
        success: "#7fd962",
        warning: "#e6b450",
        danger: "#d95757",
    },
    ChromePreset {
        id: ChromePresetId("breeze-dark"),
        name: "Breeze Dark",
        dark: true,
        background: "#202326",
        foreground: "#fcfcfc",
        accent: "#3daee9",
        success: "#27ae60",
        warning: "#f67400",
        danger: "#da4453",
    },
    ChromePreset {
        id: ChromePresetId("adwaita-dark"),
        name: "Adwaita Dark",
        dark: true,
        background: "#222226",
        foreground: "#ffffff",
        accent: "#78aeed",
        success: "#78e9ab",
        warning: "#ffc252",
        danger: "#ff938c",
    },
    ChromePreset {
        id: ChromePresetId("ubuntu-dark"),
        name: "Ubuntu Dark",
        dark: true,
        background: "#2c2c2c",
        foreground: "#f7f7f7",
        accent: "#e95420",
        success: "#50c856",
        warning: "#f99b11",
        danger: "#ff5c5d",
    },
    ChromePreset {
        id: ChromePresetId("ubuntu-terminal"),
        name: "Ubuntu Terminal",
        dark: true,
        background: "#300a24",
        foreground: "#eeeeec",
        accent: "#e95420",
        success: "#4e9a06",
        warning: "#c4a000",
        danger: "#ef2929",
    },
    ChromePreset {
        id: ChromePresetId("macos-classic-dark"),
        name: "macOS Classic",
        dark: true,
        background: "#131313",
        foreground: "#caccca",
        accent: "#0a84ff",
        success: "#62ba46",
        warning: "#b0a878",
        danger: "#d2602d",
    },
    ChromePreset {
        id: ChromePresetId("tokyo-night-day"),
        name: "Tokyo Night Day",
        dark: false,
        background: "#e1e2e7",
        foreground: "#25448b",
        accent: "#276ece",
        success: "#587539",
        warning: "#8a6a3d",
        danger: "#d52357",
    },
    ChromePreset {
        id: ChromePresetId("catppuccin-latte"),
        name: "Catppuccin Latte",
        dark: false,
        background: "#eff1f5",
        foreground: "#4c4f69",
        accent: "#8839ef",
        success: "#358823",
        warning: "#a86a13",
        danger: "#d20f39",
    },
    ChromePreset {
        id: ChromePresetId("gruvbox-light"),
        name: "Gruvbox",
        dark: false,
        background: "#fbf1c7",
        foreground: "#3c3836",
        accent: "#af3a03",
        success: "#79740e",
        warning: "#a66c11",
        danger: "#9d0006",
    },
    ChromePreset {
        id: ChromePresetId("alucard"),
        name: "Alucard",
        dark: false,
        background: "#fffbeb",
        foreground: "#1f1f1f",
        accent: "#644ac9",
        success: "#14710a",
        warning: "#846e15",
        danger: "#cb3a2a",
    },
    ChromePreset {
        id: ChromePresetId("one-light"),
        name: "One Light",
        dark: false,
        background: "#fafafa",
        foreground: "#383a42",
        accent: "#4078f2",
        success: "#468e45",
        warning: "#aa7401",
        danger: "#d85145",
    },
    ChromePreset {
        id: ChromePresetId("github-light"),
        name: "GitHub Light",
        dark: false,
        background: "#ffffff",
        foreground: "#1f2328",
        accent: "#0969da",
        success: "#1a7f37",
        warning: "#9a6700",
        danger: "#d1242f",
    },
    ChromePreset {
        id: ChromePresetId("everforest-light"),
        name: "Everforest",
        dark: false,
        background: "#fdf6e3",
        foreground: "#4a555c",
        accent: "#3384b0",
        success: "#728301",
        warning: "#a37400",
        danger: "#d84946",
    },
    ChromePreset {
        id: ChromePresetId("rose-pine-dawn"),
        name: "Rosé Pine Dawn",
        dark: false,
        background: "#faf4ed",
        foreground: "#464261",
        accent: "#ad6864",
        success: "#4c848e",
        warning: "#a86f22",
        danger: "#b4637a",
    },
    ChromePreset {
        id: ChromePresetId("solarized-light"),
        name: "Solarized",
        dark: false,
        background: "#fdf6e3",
        foreground: "#44565b",
        accent: "#2381c4",
        success: "#738400",
        warning: "#9d7600",
        danger: "#dc322f",
    },
    ChromePreset {
        id: ChromePresetId("ayu-light"),
        name: "Ayu Light",
        dark: false,
        background: "#f8f9fa",
        foreground: "#505559",
        accent: "#2e83bf",
        success: "#4e8d2f",
        warning: "#ab701f",
        danger: "#da4b4b",
    },
    ChromePreset {
        id: ChromePresetId("breeze-light"),
        name: "Breeze",
        dark: false,
        background: "#eff0f1",
        foreground: "#232629",
        accent: "#2a7eaa",
        success: "#1d894a",
        warning: "#c05900",
        danger: "#d44251",
    },
    ChromePreset {
        id: ChromePresetId("adwaita-light"),
        name: "Adwaita",
        dark: false,
        background: "#fafafb",
        foreground: "#323237",
        accent: "#327fdb",
        success: "#007c3d",
        warning: "#905400",
        danger: "#c30000",
    },
    ChromePreset {
        id: ChromePresetId("ubuntu-light"),
        name: "Ubuntu",
        dark: false,
        background: "#fafafa",
        foreground: "#3d3d3d",
        accent: "#dd4f1e",
        success: "#0f9323",
        warning: "#b36e09",
        danger: "#c7162b",
    },
    ChromePreset {
        id: ChromePresetId("macos-classic-light"),
        name: "macOS Classic",
        dark: false,
        background: "#ffffff",
        foreground: "#1a1a1a",
        accent: "#007aff",
        success: "#036a07",
        warning: "#9e7008",
        danger: "#c5060b",
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
