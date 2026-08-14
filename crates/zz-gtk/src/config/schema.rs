//! What `zz/config` can say, as one table.
//!
//! The desktop spells every key out twice — a typed field on its config struct
//! and a hand-written settings row — which is why its two config files run to
//! nearly 7000 lines. Here a key is one [`Setting`] row and the whole settings
//! surface is generated from the table, so adding a key is adding a row.
//!
//! Key strings, defaults, ranges and value grammar are the desktop's; both
//! clients read and write the same file and must agree on all of them.

use zz_protocol::MuxOptionKey;
use zz_terminal::{
    AppearanceColor, AppearanceConfigKey, CellHeightAdjustment, Color, CursorBlinkPolicy,
    CursorStyle, TerminalAppearance,
};

/// Which surface resolves a key's effective value.
///
/// Client keys are resolved here, from the file. Daemon keys are transported
/// verbatim as `SetConfigOverrides` entries and read back from what the daemon
/// publishes — the client never decides what an appearance or mux value means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Client,
    Appearance(AppearanceConfigKey),
    Mux(MuxOptionKey),
}

/// How much of a key zz-gtk actually renders. Every key is still written, so a
/// setting this client cannot honour still reaches the desktop through the
/// shared file; the badge just refuses to pretend otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    Honored,
    /// Written and honoured by the desktop, not yet read by this client.
    Unwired(&'static str),
    /// Cannot mean anything here, whatever the file says.
    Inapplicable(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: &'static str,
    pub title: &'static str,
}

const fn choice(value: &'static str, title: &'static str) -> Choice {
    Choice { value, title }
}

/// The control a key gets, and the value grammar behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Toggle {
        default: bool,
    },
    /// `default` is ignored for daemon-owned keys: those read their effective
    /// value back from the daemon rather than from a constant restated here.
    Number {
        default: f32,
        min: f64,
        max: f64,
        step: f64,
        digits: u32,
    },
    Choice {
        default: &'static str,
        options: &'static [Choice],
    },
    /// `#rrggbb` or `#rrggbbaa`; cleared means "remove the line".
    Color,
    Text {
        placeholder: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Page {
    Interface,
    Panes,
    Terminal,
    Multiplexer,
    System,
    About,
}

impl Page {
    pub const ALL: [Self; 6] = [
        Self::Interface,
        Self::Panes,
        Self::Terminal,
        Self::Multiplexer,
        Self::System,
        Self::About,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Interface => "Interface",
            Self::Panes => "Panes",
            Self::Terminal => "Terminal",
            Self::Multiplexer => "Multiplexer",
            Self::System => "System",
            Self::About => "About",
        }
    }

    pub const fn icon(self) -> &'static str {
        match self {
            Self::Interface => "applications-graphics-symbolic",
            Self::Panes => "view-grid-symbolic",
            Self::Terminal => "utilities-terminal-symbolic",
            Self::Multiplexer => "view-list-symbolic",
            Self::System => "emblem-system-symbolic",
            Self::About => "help-about-symbolic",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Interface => "interface",
            Self::Panes => "panes",
            Self::Terminal => "terminal",
            Self::Multiplexer => "multiplexer",
            Self::System => "system",
            Self::About => "about",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setting {
    pub key: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub page: Page,
    pub group: &'static str,
    pub owner: Owner,
    pub support: Support,
    pub kind: Kind,
}

impl Setting {
    pub const fn is_daemon_owned(&self) -> bool {
        !matches!(self.owner, Owner::Client)
    }
}

const THEME_MODES: &[Choice] = &[
    choice("system", "System"),
    choice("light", "Light"),
    choice("dark", "Dark"),
];

const CURSOR_STYLES: &[Choice] = &[
    choice("block", "Block"),
    choice("bar", "Bar"),
    choice("underline", "Underline"),
    choice("block_hollow", "Hollow block"),
];

const CURSOR_BLINK: &[Choice] = &[
    choice("terminal", "Follow the program"),
    choice("true", "Always blink"),
    choice("false", "Never blink"),
];

const MODE_KEYS: &[Choice] = &[choice("emacs", "Emacs"), choice("vi", "Vi")];

const SET_CLIPBOARD: &[Choice] = &[
    choice("external", "External"),
    choice("on", "On"),
    choice("off", "Off"),
];

const ON_OFF: &[Choice] = &[choice("off", "Off"), choice("on", "On")];

/// The corner-radius family the desktop resolves as a chrome preset. zz-gtk
/// takes its chrome from the GNOME stylesheet, so the key is written for the
/// desktop's benefit and left unread here.
const CHROME_PRESETS: &[Choice] = &[
    choice("tokyo-night", "Tokyo Night"),
    choice("catppuccin", "Catppuccin"),
    choice("gruvbox", "Gruvbox"),
    choice("nord", "Nord"),
    choice("breeze", "Breeze"),
    choice("adwaita", "Adwaita"),
    choice("ubuntu", "Ubuntu"),
    choice("rose-pine", "Rosé Pine"),
    choice("ayu", "Ayu"),
    choice("solarized", "Solarized"),
    choice("macos-classic", "macOS Classic"),
];

const PANE_GEOMETRY_NOTE: &str = "the grid draws a fixed two-pixel divider.";

pub const SETTINGS: &[Setting] = &[
    Setting {
        key: "theme-mode",
        title: "Theme",
        description: "Follow the desktop's light/dark preference, or pin one.",
        page: Page::Interface,
        group: "Appearance",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Choice {
            default: "system",
            options: THEME_MODES,
        },
    },
    Setting {
        key: "animations",
        title: "Animations",
        description: "Transitions and motion across the window.",
        page: Page::Interface,
        group: "Appearance",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Toggle { default: true },
    },
    Setting {
        key: "chrome-background",
        title: "Background",
        description: "The window's base plane; every panel and popover derives from it.",
        page: Page::Interface,
        group: "Chrome colors",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "chrome-foreground",
        title: "Foreground",
        description: "Default text, and the source of muted text and focus rings.",
        page: Page::Interface,
        group: "Chrome colors",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "chrome-border",
        title: "Border",
        description: "Every edge: panel borders, dividers, input outlines.",
        page: Page::Interface,
        group: "Chrome colors",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "chrome-success",
        title: "Success",
        description: "Something completed or is healthy.",
        page: Page::Interface,
        group: "Chrome colors",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "chrome-warning",
        title: "Warning",
        description: "Something needs attention but still works.",
        page: Page::Interface,
        group: "Chrome colors",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "chrome-danger",
        title: "Danger",
        description: "Something failed or is destructive.",
        page: Page::Interface,
        group: "Chrome colors",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "chrome-preset",
        title: "Chrome preset",
        description: "A paired light/dark chrome family. Choosing one clears every explicit \
                      chrome color.",
        page: Page::Interface,
        group: "Chrome colors",
        owner: Owner::Client,
        support: Support::Unwired(
            "chrome comes from the GNOME stylesheet; the colors above override it.",
        ),
        kind: Kind::Choice {
            default: "",
            options: CHROME_PRESETS,
        },
    },
    Setting {
        key: "widget-corner-radius",
        title: "Widget corner radius",
        description: "Corner rounding of buttons, inputs and popovers.",
        page: Page::Interface,
        group: "Chrome colors",
        owner: Owner::Client,
        support: Support::Unwired("GTK widget radii come from the platform stylesheet."),
        kind: Kind::Number {
            default: 6.0,
            min: 0.0,
            max: 24.0,
            step: 1.0,
            digits: 1,
        },
    },
    Setting {
        key: "use-system-titlebar",
        title: "System titlebar",
        description: "Let the desktop draw the window frame instead of the app.",
        page: Page::Interface,
        group: "Window",
        owner: Owner::Client,
        support: Support::Inapplicable("the toolkit and compositor own the window frame."),
        kind: Kind::Toggle { default: false },
    },
    Setting {
        key: "window-corner-radius",
        title: "Window corner radius",
        description: "Rounding of the app-drawn window frame.",
        page: Page::Interface,
        group: "Window",
        owner: Owner::Client,
        support: Support::Inapplicable("the compositor owns the window shape."),
        kind: Kind::Number {
            default: 13.5,
            min: 0.0,
            max: 32.0,
            step: 0.5,
            digits: 1,
        },
    },
    Setting {
        key: "window-background-blur",
        title: "Background blur",
        description: "Translucent window background behind the panes.",
        page: Page::Interface,
        group: "Window",
        owner: Owner::Client,
        support: Support::Inapplicable("no blurred surface exists in this shell."),
        kind: Kind::Toggle { default: false },
    },
    Setting {
        key: "pane-gaps",
        title: "Gapped panes",
        description: "Separate panes with a gap instead of a hairline divider.",
        page: Page::Panes,
        group: "Layout",
        owner: Owner::Client,
        support: Support::Unwired(PANE_GEOMETRY_NOTE),
        kind: Kind::Toggle { default: false },
    },
    Setting {
        key: "pane-margin",
        title: "Gap size",
        description: "Space between neighbouring panes when gaps are on.",
        page: Page::Panes,
        group: "Layout",
        owner: Owner::Client,
        support: Support::Unwired(PANE_GEOMETRY_NOTE),
        kind: Kind::Number {
            default: 6.0,
            min: 0.0,
            max: 32.0,
            step: 1.0,
            digits: 1,
        },
    },
    Setting {
        key: "pane-corner-radius",
        title: "Pane corner radius",
        description: "Corner rounding of each pane when gaps are on.",
        page: Page::Panes,
        group: "Layout",
        owner: Owner::Client,
        support: Support::Unwired(PANE_GEOMETRY_NOTE),
        kind: Kind::Number {
            default: 13.5,
            min: 0.0,
            max: 32.0,
            step: 0.5,
            digits: 1,
        },
    },
    Setting {
        key: "pane-border-width",
        title: "Pane border width",
        description: "Outline drawn around each pane when gaps are on.",
        page: Page::Panes,
        group: "Layout",
        owner: Owner::Client,
        support: Support::Unwired(PANE_GEOMETRY_NOTE),
        kind: Kind::Number {
            default: 1.0,
            min: 0.0,
            max: 8.0,
            step: 0.5,
            digits: 1,
        },
    },
    Setting {
        key: "font-family",
        title: "Font",
        description: "Monospace family the daemon resolves for every terminal pane.",
        page: Page::Terminal,
        group: "Font",
        owner: Owner::Appearance(AppearanceConfigKey::FontFamily),
        support: Support::Honored,
        kind: Kind::Text {
            placeholder: "the daemon's default monospace",
        },
    },
    Setting {
        key: "font-size",
        title: "Font size",
        description: "Base point size. The zoom chord adds a client-local offset on top.",
        page: Page::Terminal,
        group: "Font",
        owner: Owner::Appearance(AppearanceConfigKey::FontSize),
        support: Support::Honored,
        kind: Kind::Number {
            default: 13.0,
            min: 4.0,
            max: 72.0,
            step: 0.5,
            digits: 1,
        },
    },
    Setting {
        key: "zz-font-weight",
        title: "Font weight",
        description: "Weight of regular text; 700 and above renders bold.",
        page: Page::Terminal,
        group: "Font",
        owner: Owner::Appearance(AppearanceConfigKey::ZzFontWeight),
        support: Support::Honored,
        kind: Kind::Number {
            default: 400.0,
            min: 100.0,
            max: 900.0,
            step: 100.0,
            digits: 0,
        },
    },
    Setting {
        key: "adjust-cell-height",
        title: "Cell height",
        description: "Extra line height, in pixels or as a percentage such as `10%`.",
        page: Page::Terminal,
        group: "Font",
        owner: Owner::Appearance(AppearanceConfigKey::AdjustCellHeight),
        support: Support::Honored,
        kind: Kind::Text { placeholder: "0" },
    },
    Setting {
        key: "theme",
        title: "Ghostty theme",
        description: "A Ghostty theme name; the daemon flattens it to concrete colors.",
        page: Page::Terminal,
        group: "Colors",
        owner: Owner::Appearance(AppearanceConfigKey::Theme),
        support: Support::Honored,
        kind: Kind::Text {
            placeholder: "no theme file",
        },
    },
    Setting {
        key: "background",
        title: "Background",
        description: "Terminal background color.",
        page: Page::Terminal,
        group: "Colors",
        owner: Owner::Appearance(AppearanceConfigKey::Background),
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "foreground",
        title: "Foreground",
        description: "Default text color.",
        page: Page::Terminal,
        group: "Colors",
        owner: Owner::Appearance(AppearanceConfigKey::Foreground),
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "minimum-contrast",
        title: "Minimum contrast",
        description: "Contrast ratio the daemon enforces between text and its background.",
        page: Page::Terminal,
        group: "Colors",
        owner: Owner::Appearance(AppearanceConfigKey::MinimumContrast),
        support: Support::Honored,
        kind: Kind::Number {
            default: 1.0,
            min: 1.0,
            max: 21.0,
            step: 0.1,
            digits: 2,
        },
    },
    Setting {
        key: "background-opacity",
        title: "Background opacity",
        description: "Terminal background alpha.",
        page: Page::Terminal,
        group: "Colors",
        owner: Owner::Appearance(AppearanceConfigKey::BackgroundOpacity),
        support: Support::Unwired("the pane paints an opaque background."),
        kind: Kind::Number {
            default: 1.0,
            min: 0.0,
            max: 1.0,
            step: 0.05,
            digits: 2,
        },
    },
    Setting {
        key: "cursor-color",
        title: "Cursor color",
        description: "Color of the text cursor.",
        page: Page::Terminal,
        group: "Cursor",
        owner: Owner::Appearance(AppearanceConfigKey::CursorColor),
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "cursor-style",
        title: "Cursor style",
        description: "Shape the cursor takes when the program does not choose one.",
        page: Page::Terminal,
        group: "Cursor",
        owner: Owner::Appearance(AppearanceConfigKey::CursorStyle),
        support: Support::Honored,
        kind: Kind::Choice {
            default: "block",
            options: CURSOR_STYLES,
        },
    },
    Setting {
        key: "cursor-style-blink",
        title: "Cursor blink",
        description: "Whether the cursor blinks, or defers to the running program.",
        page: Page::Terminal,
        group: "Cursor",
        owner: Owner::Appearance(AppearanceConfigKey::CursorStyleBlink),
        support: Support::Honored,
        kind: Kind::Choice {
            default: "terminal",
            options: CURSOR_BLINK,
        },
    },
    Setting {
        key: "zz-cursor-blink-interval-ms",
        title: "Blink interval",
        description: "Milliseconds between cursor blinks.",
        page: Page::Terminal,
        group: "Cursor",
        owner: Owner::Appearance(AppearanceConfigKey::ZzCursorBlinkIntervalMs),
        support: Support::Honored,
        kind: Kind::Number {
            default: 600.0,
            min: 0.0,
            max: 5000.0,
            step: 50.0,
            digits: 0,
        },
    },
    Setting {
        key: "zz-copy-cursor-color",
        title: "Copy-mode cursor",
        description: "Cursor color while copy mode is active.",
        page: Page::Terminal,
        group: "Cursor",
        owner: Owner::Appearance(AppearanceConfigKey::ZzCopyCursorColor),
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "selection-background",
        title: "Selection background",
        description: "Highlight behind selected text.",
        page: Page::Terminal,
        group: "Selection and search",
        owner: Owner::Appearance(AppearanceConfigKey::SelectionBackground),
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "selection-foreground",
        title: "Selection text",
        description: "Text color inside the selection.",
        page: Page::Terminal,
        group: "Selection and search",
        owner: Owner::Appearance(AppearanceConfigKey::SelectionForeground),
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "zz-rounded-selection",
        title: "Rounded selection",
        description: "Round the corners of the selection highlight.",
        page: Page::Terminal,
        group: "Selection and search",
        owner: Owner::Appearance(AppearanceConfigKey::ZzRoundedSelection),
        support: Support::Unwired("the pane fills selection rectangles square."),
        kind: Kind::Toggle { default: false },
    },
    Setting {
        key: "zz-search-match-color",
        title: "Search match",
        description: "Highlight on every search hit.",
        page: Page::Terminal,
        group: "Selection and search",
        owner: Owner::Appearance(AppearanceConfigKey::ZzSearchMatchColor),
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "zz-search-current-color",
        title: "Current match",
        description: "Highlight on the hit the search cursor is on.",
        page: Page::Terminal,
        group: "Selection and search",
        owner: Owner::Appearance(AppearanceConfigKey::ZzSearchCurrentColor),
        support: Support::Honored,
        kind: Kind::Color,
    },
    Setting {
        key: "zz-link-color",
        title: "Link color",
        description: "Color of hovered hyperlinks.",
        page: Page::Terminal,
        group: "Selection and search",
        owner: Owner::Appearance(AppearanceConfigKey::ZzLinkColor),
        support: Support::Unwired("the pane paints no link decorations yet."),
        kind: Kind::Color,
    },
    Setting {
        key: "window-padding-x",
        title: "Horizontal padding",
        description: "Left and right pane padding, as `left,right`.",
        page: Page::Terminal,
        group: "Padding",
        owner: Owner::Appearance(AppearanceConfigKey::WindowPaddingX),
        support: Support::Unwired("the pane pads its grid from the widget's own margins."),
        kind: Kind::Text { placeholder: "0,0" },
    },
    Setting {
        key: "window-padding-y",
        title: "Vertical padding",
        description: "Top and bottom pane padding, as `top,bottom`.",
        page: Page::Terminal,
        group: "Padding",
        owner: Owner::Appearance(AppearanceConfigKey::WindowPaddingY),
        support: Support::Unwired("the pane pads its grid from the widget's own margins."),
        kind: Kind::Text { placeholder: "0,0" },
    },
    Setting {
        key: "prefix",
        title: "Prefix key",
        description: "The chord every mux binding hangs off, in tmux spelling.",
        page: Page::Multiplexer,
        group: "Keys",
        owner: Owner::Mux(MuxOptionKey::Prefix),
        support: Support::Honored,
        kind: Kind::Text { placeholder: "C-b" },
    },
    Setting {
        key: "mode-keys",
        title: "Mode keys",
        description: "Motion grammar inside copy mode.",
        page: Page::Multiplexer,
        group: "Keys",
        owner: Owner::Mux(MuxOptionKey::ModeKeys),
        support: Support::Honored,
        kind: Kind::Choice {
            default: "emacs",
            options: MODE_KEYS,
        },
    },
    Setting {
        key: "word-separators",
        title: "Word separators",
        description: "Characters that end a word for double-click and copy-mode motions.",
        page: Page::Multiplexer,
        group: "Keys",
        owner: Owner::Mux(MuxOptionKey::WordSeparators),
        support: Support::Honored,
        kind: Kind::Text {
            placeholder: "the daemon's default set",
        },
    },
    Setting {
        key: "history-limit",
        title: "History limit",
        description: "Scrollback lines the daemon keeps per pane.",
        page: Page::Multiplexer,
        group: "History",
        owner: Owner::Mux(MuxOptionKey::HistoryLimit),
        support: Support::Honored,
        kind: Kind::Number {
            default: 2000.0,
            min: 0.0,
            max: 1_000_000.0,
            step: 500.0,
            digits: 0,
        },
    },
    Setting {
        key: "history-trickle",
        title: "History trickle",
        description: "Lines the daemon backfills per scrollback request.",
        page: Page::Multiplexer,
        group: "History",
        owner: Owner::Mux(MuxOptionKey::HistoryTrickle),
        support: Support::Honored,
        kind: Kind::Number {
            default: 2000.0,
            min: 0.0,
            max: 100_000.0,
            step: 500.0,
            digits: 0,
        },
    },
    Setting {
        key: "buffer-limit",
        title: "Buffer limit",
        description: "Paste buffers the daemon retains.",
        page: Page::Multiplexer,
        group: "History",
        owner: Owner::Mux(MuxOptionKey::BufferLimit),
        support: Support::Honored,
        kind: Kind::Number {
            default: 50.0,
            min: 1.0,
            max: 1000.0,
            step: 1.0,
            digits: 0,
        },
    },
    Setting {
        key: "set-clipboard",
        title: "Set clipboard",
        description: "Whether programs may write the system clipboard through OSC 52.",
        page: Page::Multiplexer,
        group: "Clipboard",
        owner: Owner::Mux(MuxOptionKey::SetClipboard),
        support: Support::Honored,
        kind: Kind::Choice {
            default: "external",
            options: SET_CLIPBOARD,
        },
    },
    Setting {
        key: "copy-command",
        title: "Copy command",
        description: "External command a copy is piped through; empty uses the system clipboard.",
        page: Page::Multiplexer,
        group: "Clipboard",
        owner: Owner::Mux(MuxOptionKey::CopyCommand),
        support: Support::Honored,
        kind: Kind::Text {
            placeholder: "the system clipboard",
        },
    },
    Setting {
        key: "synchronize-panes",
        title: "Synchronize panes",
        description: "Send every keystroke to all panes of a window.",
        page: Page::Multiplexer,
        group: "Clipboard",
        owner: Owner::Mux(MuxOptionKey::SynchronizePanes),
        support: Support::Honored,
        kind: Kind::Choice {
            default: "off",
            options: ON_OFF,
        },
    },
    Setting {
        key: "quit-daemon-on-exit",
        title: "Quit the daemon on exit",
        description: "Stop the daemon when this window closes instead of leaving sessions running.",
        page: Page::System,
        group: "Daemon",
        owner: Owner::Client,
        support: Support::Honored,
        kind: Kind::Toggle { default: false },
    },
    Setting {
        key: "auto-restart-stale-daemon",
        title: "Restart a stale daemon",
        description: "Replace a daemon left running from an older build.",
        page: Page::System,
        group: "Daemon",
        owner: Owner::Client,
        support: Support::Inapplicable("this client never spawns a daemon; it dials a socket."),
        kind: Kind::Toggle { default: false },
    },
    Setting {
        key: "tray",
        title: "Tray icon",
        description: "Keep a status icon and close to the tray.",
        page: Page::System,
        group: "Daemon",
        owner: Owner::Client,
        support: Support::Inapplicable("this client publishes no StatusNotifierItem."),
        kind: Kind::Toggle { default: true },
    },
    Setting {
        key: "show-fps",
        title: "Frame counter",
        description: "Overlay a frames-per-second badge on each pane.",
        page: Page::System,
        group: "Daemon",
        owner: Owner::Client,
        support: Support::Inapplicable("GTK owns the frame clock."),
        kind: Kind::Toggle { default: false },
    },
    Setting {
        key: "experimental-agent-pane",
        title: "Agent panes",
        description: "Let the daemon create ACP agent panes.",
        page: Page::System,
        group: "Experimental",
        owner: Owner::Mux(MuxOptionKey::ExperimentalAgentPane),
        support: Support::Unwired("this client renders a placeholder for agent panes."),
        kind: Kind::Toggle { default: false },
    },
    Setting {
        key: "experimental-editor-pane",
        title: "Editor panes",
        description: "Let the daemon create editor panes.",
        page: Page::System,
        group: "Experimental",
        owner: Owner::Mux(MuxOptionKey::ExperimentalEditorPane),
        support: Support::Unwired("this client renders a placeholder for editor panes."),
        kind: Kind::Toggle { default: false },
    },
];

pub fn for_page(page: Page) -> impl Iterator<Item = &'static Setting> {
    SETTINGS.iter().filter(move |setting| setting.page == page)
}

/// The groups a page renders, in table order and without duplicates.
#[must_use]
pub fn groups(page: Page) -> Vec<&'static str> {
    let mut groups: Vec<&'static str> = Vec::new();
    for setting in for_page(page) {
        if !groups.contains(&setting.group) {
            groups.push(setting.group);
        }
    }
    groups
}

/// A boolean as the file spells it. The desktop accepts only these two words.
#[must_use]
pub const fn boolean(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// A number as the file spells it: Rust's `f32` display, so `6.0` writes as
/// `6` — matching what the desktop's settings surface produces byte for byte.
#[must_use]
pub fn number(value: f64) -> String {
    (value as f32).to_string()
}

/// A daemon-owned appearance key's effective value, spelled the way the file
/// would spell it. This is the desktop's `appearance_config_values` narrowed to
/// the single-valued keys this client exposes; cumulative keys (`palette`,
/// `font-feature`) have no single-line form and are not offered.
#[must_use]
pub fn appearance_display(
    appearance: &TerminalAppearance,
    key: AppearanceConfigKey,
) -> Option<String> {
    let value = match key {
        AppearanceConfigKey::Background => rgb(appearance.background),
        AppearanceConfigKey::Foreground => rgb(appearance.foreground),
        AppearanceConfigKey::CursorColor => rgb(appearance.cursor_color),
        AppearanceConfigKey::SelectionForeground => rgb(appearance.selection_foreground),
        AppearanceConfigKey::SelectionBackground => rgba(appearance.selection_background),
        AppearanceConfigKey::ZzSearchMatchColor => rgba(appearance.search_match_color),
        AppearanceConfigKey::ZzSearchCurrentColor => rgba(appearance.search_current_color),
        AppearanceConfigKey::ZzCopyCursorColor => rgba(appearance.copy_cursor_color),
        AppearanceConfigKey::ZzLinkColor => rgb(appearance.link_color),
        AppearanceConfigKey::FontFamily => appearance.font_families.first().cloned()?,
        AppearanceConfigKey::FontSize => appearance.font_size_points.to_string(),
        AppearanceConfigKey::ZzFontWeight => appearance.font_weight.to_string(),
        AppearanceConfigKey::MinimumContrast => appearance.minimum_contrast.to_string(),
        AppearanceConfigKey::BackgroundOpacity => appearance.background_opacity.to_string(),
        AppearanceConfigKey::ZzCursorBlinkIntervalMs => {
            appearance.cursor_blink_interval_ms.to_string()
        }
        AppearanceConfigKey::ZzRoundedSelection => appearance.rounded_selection.to_string(),
        AppearanceConfigKey::AdjustCellHeight => match appearance.cell_height_adjustment {
            CellHeightAdjustment::None => String::new(),
            CellHeightAdjustment::Pixels(value) => value.to_string(),
            CellHeightAdjustment::Percent(value) => format!("{value}%"),
        },
        AppearanceConfigKey::CursorStyle => match appearance.cursor_style {
            CursorStyle::Bar => "bar",
            CursorStyle::Block => "block",
            CursorStyle::Underline => "underline",
            CursorStyle::BlockHollow => "block_hollow",
        }
        .to_owned(),
        AppearanceConfigKey::CursorStyleBlink => match appearance.cursor_blink_policy {
            CursorBlinkPolicy::Off => "false",
            CursorBlinkPolicy::On => "true",
            CursorBlinkPolicy::Terminal => "terminal",
        }
        .to_owned(),
        AppearanceConfigKey::WindowPaddingX => {
            format!("{},{}", appearance.padding_left, appearance.padding_right)
        }
        AppearanceConfigKey::WindowPaddingY => {
            format!("{},{}", appearance.padding_top, appearance.padding_bottom)
        }
        AppearanceConfigKey::Theme
        | AppearanceConfigKey::Palette
        | AppearanceConfigKey::FontFamilyBold
        | AppearanceConfigKey::FontFamilyItalic
        | AppearanceConfigKey::FontFamilyBoldItalic
        | AppearanceConfigKey::FontFeature
        | AppearanceConfigKey::FontSyntheticStyle
        | AppearanceConfigKey::FontThicken
        | AppearanceConfigKey::FontThickenStrength => return None,
    };
    Some(value)
}

fn rgb(color: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn rgba(color: AppearanceColor) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color.r, color.g, color.b, color.a
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_unique_and_classified() {
        let mut seen: Vec<&str> = Vec::new();
        for setting in SETTINGS {
            assert!(
                !seen.contains(&setting.key),
                "{} appears twice in the table",
                setting.key
            );
            seen.push(setting.key);
            match setting.owner {
                Owner::Client => assert!(
                    AppearanceConfigKey::from_config_key(setting.key).is_none()
                        && MuxOptionKey::from_config_key(setting.key).is_none(),
                    "{} is claimed as client-local but the daemon owns it",
                    setting.key
                ),
                Owner::Appearance(key) => assert_eq!(key.as_str(), setting.key),
                Owner::Mux(key) => assert_eq!(key.as_str(), setting.key),
            }
        }
    }

    #[test]
    fn every_daemon_key_the_table_offers_has_a_readable_effective_value() {
        let appearance = TerminalAppearance::default();
        for setting in SETTINGS {
            if let Owner::Appearance(key) = setting.owner {
                assert!(
                    appearance_display(&appearance, key).is_some() || key.as_str() == "theme",
                    "{} has no single-line effective value",
                    setting.key
                );
            }
        }
    }

    #[test]
    fn numbers_are_spelled_the_way_the_desktop_spells_them() {
        assert_eq!(number(6.0), "6");
        assert_eq!(number(13.5), "13.5");
    }
}
