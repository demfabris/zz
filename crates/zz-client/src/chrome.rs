use std::fmt;

use zz_protocol::{Binding, CommandInvocation, KeyTables, input_key_name};
use zz_terminal::{KeyInput, Modifiers};

/// The chrome table consulted for every key press.
pub const UI_TABLE: &str = "ui";
/// The chrome table consulted while the session tree has focus.
pub const SIDEBAR_TABLE: &str = "sidebar";
/// The chrome table consulted while a browser surface has focus.
pub const BROWSER_TABLE: &str = "browser";
/// The chrome table consulted while a terminal surface has focus.
pub const TERMINAL_TABLE: &str = "terminal";
/// Every table a chrome binding may name.
pub const CHROME_TABLES: [&str; 4] = [UI_TABLE, SIDEBAR_TABLE, BROWSER_TABLE, TERMINAL_TABLE];

const SELECT_TAB_PREFIX: &str = "browser-select-tab-";
const SELECT_TAB_NAMES: [&str; 8] = [
    "browser-select-tab-1",
    "browser-select-tab-2",
    "browser-select-tab-3",
    "browser-select-tab-4",
    "browser-select-tab-5",
    "browser-select-tab-6",
    "browser-select-tab-7",
    "browser-select-tab-8",
];

/// A chrome action resolved client-side before the skin applies its local or
/// protocol-backed effect. Skins switch on the action instead of inspecting
/// chords themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeAction {
    Detach,
    ToggleSidebar,
    ClosePane,
    OpenSettings,
    UiZoomIn,
    UiZoomOut,
    UiZoomReset,
    SidebarCancel,
    SidebarConfirm,
    SidebarRename,
    SidebarCommandPalette,
    SidebarSelectUp,
    SidebarSelectDown,
    SidebarSelectLeft,
    SidebarSelectRight,
    SidebarSelectFirst,
    SidebarSelectLast,
    TerminalFontIncrease,
    TerminalFontDecrease,
    TerminalSearch,
    TerminalCopy,
    TerminalSelectAll,
    TerminalClearHistory,
    TerminalPaste,
    BrowserZoomIn,
    BrowserZoomOut,
    BrowserZoomReset,
    BrowserDevTools,
    BrowserNewTab,
    BrowserNextTab,
    BrowserPreviousTab,
    /// Activate a tab by position. Named actions cover the `0..8` range that
    /// [`ChromeAction::from_name`] accepts.
    BrowserSelectTab(u8),
    BrowserSelectLastTab,
    BrowserBack,
    BrowserForward,
    BrowserReload,
    BrowserFocusAddress,
    BrowserElementSelector,
    BrowserUndo,
    BrowserRedo,
    BrowserCut,
    BrowserCopy,
    BrowserPaste,
    BrowserPasteAndMatchStyle,
    BrowserSelectAll,
}

impl ChromeAction {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Detach => "detach",
            Self::ToggleSidebar => "toggle-sidebar",
            Self::ClosePane => "close-pane",
            Self::OpenSettings => "open-settings",
            Self::UiZoomIn => "ui-zoom-in",
            Self::UiZoomOut => "ui-zoom-out",
            Self::UiZoomReset => "ui-zoom-reset",
            Self::SidebarCancel => "sidebar-cancel",
            Self::SidebarConfirm => "sidebar-confirm",
            Self::SidebarRename => "sidebar-rename",
            Self::SidebarCommandPalette => "sidebar-command-palette",
            Self::SidebarSelectUp => "sidebar-select-up",
            Self::SidebarSelectDown => "sidebar-select-down",
            Self::SidebarSelectLeft => "sidebar-select-left",
            Self::SidebarSelectRight => "sidebar-select-right",
            Self::SidebarSelectFirst => "sidebar-select-first",
            Self::SidebarSelectLast => "sidebar-select-last",
            Self::TerminalFontIncrease => "terminal-font-increase",
            Self::TerminalFontDecrease => "terminal-font-decrease",
            Self::TerminalSearch => "terminal-search",
            Self::TerminalCopy => "terminal-copy",
            Self::TerminalSelectAll => "terminal-select-all",
            Self::TerminalClearHistory => "terminal-clear-history",
            Self::TerminalPaste => "terminal-paste",
            Self::BrowserZoomIn => "browser-zoom-in",
            Self::BrowserZoomOut => "browser-zoom-out",
            Self::BrowserZoomReset => "browser-zoom-reset",
            Self::BrowserDevTools => "browser-devtools",
            Self::BrowserNewTab => "browser-new-tab",
            Self::BrowserNextTab => "browser-next-tab",
            Self::BrowserPreviousTab => "browser-previous-tab",
            Self::BrowserSelectTab(index) if index < 8 => SELECT_TAB_NAMES[index as usize],
            Self::BrowserSelectTab(_) => "browser-select-tab",
            Self::BrowserSelectLastTab => "browser-select-last-tab",
            Self::BrowserBack => "browser-back",
            Self::BrowserForward => "browser-forward",
            Self::BrowserReload => "browser-reload",
            Self::BrowserFocusAddress => "browser-focus-address",
            Self::BrowserElementSelector => "browser-element-selector",
            Self::BrowserUndo => "browser-undo",
            Self::BrowserRedo => "browser-redo",
            Self::BrowserCut => "browser-cut",
            Self::BrowserCopy => "browser-copy",
            Self::BrowserPaste => "browser-paste",
            Self::BrowserPasteAndMatchStyle => "browser-paste-and-match-style",
            Self::BrowserSelectAll => "browser-select-all",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(position) = name.strip_prefix(SELECT_TAB_PREFIX) {
            let position = position.parse::<u8>().ok()?;
            return (1..=8)
                .contains(&position)
                .then(|| Self::BrowserSelectTab(position - 1));
        }
        Some(match name {
            "detach" => Self::Detach,
            "toggle-sidebar" => Self::ToggleSidebar,
            "close-pane" => Self::ClosePane,
            "open-settings" => Self::OpenSettings,
            "ui-zoom-in" => Self::UiZoomIn,
            "ui-zoom-out" => Self::UiZoomOut,
            "ui-zoom-reset" => Self::UiZoomReset,
            "sidebar-cancel" => Self::SidebarCancel,
            "sidebar-confirm" => Self::SidebarConfirm,
            "sidebar-rename" => Self::SidebarRename,
            "sidebar-command-palette" => Self::SidebarCommandPalette,
            "sidebar-select-up" => Self::SidebarSelectUp,
            "sidebar-select-down" => Self::SidebarSelectDown,
            "sidebar-select-left" => Self::SidebarSelectLeft,
            "sidebar-select-right" => Self::SidebarSelectRight,
            "sidebar-select-first" => Self::SidebarSelectFirst,
            "sidebar-select-last" => Self::SidebarSelectLast,
            "terminal-font-increase" => Self::TerminalFontIncrease,
            "terminal-font-decrease" => Self::TerminalFontDecrease,
            "terminal-search" => Self::TerminalSearch,
            "terminal-copy" => Self::TerminalCopy,
            "terminal-select-all" => Self::TerminalSelectAll,
            "terminal-clear-history" => Self::TerminalClearHistory,
            "terminal-paste" => Self::TerminalPaste,
            "browser-zoom-in" => Self::BrowserZoomIn,
            "browser-zoom-out" => Self::BrowserZoomOut,
            "browser-zoom-reset" => Self::BrowserZoomReset,
            "browser-devtools" => Self::BrowserDevTools,
            "browser-new-tab" => Self::BrowserNewTab,
            "browser-next-tab" => Self::BrowserNextTab,
            "browser-previous-tab" => Self::BrowserPreviousTab,
            "browser-select-last-tab" => Self::BrowserSelectLastTab,
            "browser-back" => Self::BrowserBack,
            "browser-forward" => Self::BrowserForward,
            "browser-reload" => Self::BrowserReload,
            "browser-focus-address" => Self::BrowserFocusAddress,
            "browser-element-selector" => Self::BrowserElementSelector,
            "browser-undo" => Self::BrowserUndo,
            "browser-redo" => Self::BrowserRedo,
            "browser-cut" => Self::BrowserCut,
            "browser-copy" => Self::BrowserCopy,
            "browser-paste" => Self::BrowserPaste,
            "browser-paste-and-match-style" => Self::BrowserPasteAndMatchStyle,
            "browser-select-all" => Self::BrowserSelectAll,
            _ => return None,
        })
    }
}

/// The named action was not one [`ChromeAction`] knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownChromeAction(pub String);

/// A chrome chord in the tmux `bind-key` spelling, extended with `D-` for
/// Command/Super and `S-` for Shift. The wire grammar folds both away — a pane
/// never receives them — so chrome, which never forwards a key, spells them out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChromeKey {
    pub command: bool,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub base: String,
}

impl ChromeKey {
    /// Parse a chord, accepting `D-`/`Cmd-`/`Super-`, `C-`/`Ctrl-`,
    /// `M-`/`Alt-` and `S-`/`Shift-` in any order.
    #[must_use]
    pub fn parse(spelling: &str) -> Option<Self> {
        let mut key = Self::default();
        let mut rest = spelling.trim();
        loop {
            if let Some(tail) = strip_modifier(rest, &["D-", "Cmd-", "Super-"]) {
                key.command = true;
                rest = tail;
            } else if let Some(tail) = strip_modifier(rest, &["C-", "Ctrl-"]) {
                key.control = true;
                rest = tail;
            } else if let Some(tail) = strip_modifier(rest, &["M-", "Alt-"]) {
                key.alt = true;
                rest = tail;
            } else if let Some(tail) = strip_modifier(rest, &["S-", "Shift-"]) {
                key.shift = true;
                rest = tail;
            } else {
                break;
            }
        }
        key.base = match rest {
            "Space" => " ".to_owned(),
            "" if spelling.ends_with(' ') => " ".to_owned(),
            "" => return None,
            base => base.to_owned(),
        };
        Some(key.normalized())
    }

    /// Fold the two spellings of a shifted letter together: bare Shift lives in
    /// the letter's case, the way the wire grammar spells it, and Shift beside
    /// another modifier lives in the `S-` prefix, because the case of a chorded
    /// letter is not something a keyboard reports.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        let commanded = self.command || self.control || self.alt;
        let mut characters = self.base.chars();
        let (Some(single), None) = (characters.next(), characters.next()) else {
            return self;
        };
        if self.shift && !commanded && single.is_ascii_alphabetic() {
            self.base = single.to_ascii_uppercase().to_string();
            self.shift = false;
        } else if commanded && single.is_ascii_uppercase() {
            self.base = single.to_ascii_lowercase().to_string();
            self.shift = true;
        }
        self
    }
}

impl fmt::Display for ChromeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.command {
            formatter.write_str("D-")?;
        }
        if self.control {
            formatter.write_str("C-")?;
        }
        if self.alt {
            formatter.write_str("M-")?;
        }
        if self.shift {
            formatter.write_str("S-")?;
        }
        formatter.write_str(&self.base)
    }
}

fn strip_modifier<'a>(value: &'a str, spellings: &[&str]) -> Option<&'a str> {
    spellings
        .iter()
        .find_map(|spelling| value.strip_prefix(spelling))
}

/// Which skin's chrome conventions the built-in bindings follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeProfile {
    /// The raw-terminal client, which no Command chord ever reaches.
    Tui,
    /// The desktop client where Control carries chrome.
    Desktop,
    /// The desktop client on Apple platforms, where Command carries chrome.
    DesktopApple,
}

impl ChromeProfile {
    /// The desktop profile for the platform this build targets.
    pub const DESKTOP: Self = if cfg!(any(target_os = "macos", target_os = "ios")) {
        Self::DesktopApple
    } else {
        Self::Desktop
    };

    const fn defaults(self) -> [&'static [ChromeDefault]; 2] {
        match self {
            Self::Tui => [TUI_DEFAULTS, TUI_SIDEBAR_DEFAULTS],
            Self::Desktop => [DESKTOP_DEFAULTS, DESKTOP_CONTROL_DEFAULTS],
            Self::DesktopApple => [DESKTOP_DEFAULTS, DESKTOP_COMMAND_DEFAULTS],
        }
    }
}

type ChromeDefault = (&'static str, &'static str, ChromeAction);

/// The raw-terminal client's chrome, whose chords all survive a PTY.
const TUI_DEFAULTS: &[ChromeDefault] = &[
    (UI_TABLE, "C-\\", ChromeAction::Detach),
    (UI_TABLE, "M-s", ChromeAction::ToggleSidebar),
    (UI_TABLE, "M-S", ChromeAction::ToggleSidebar),
    (BROWSER_TABLE, "C-=", ChromeAction::BrowserZoomIn),
    (BROWSER_TABLE, "C-+", ChromeAction::BrowserZoomIn),
    (BROWSER_TABLE, "C--", ChromeAction::BrowserZoomOut),
    (BROWSER_TABLE, "C-_", ChromeAction::BrowserZoomOut),
    (BROWSER_TABLE, "C-0", ChromeAction::BrowserZoomReset),
];

const TUI_SIDEBAR_DEFAULTS: &[ChromeDefault] = &[
    (SIDEBAR_TABLE, "Up", ChromeAction::SidebarSelectUp),
    (SIDEBAR_TABLE, "k", ChromeAction::SidebarSelectUp),
    (SIDEBAR_TABLE, "Down", ChromeAction::SidebarSelectDown),
    (SIDEBAR_TABLE, "j", ChromeAction::SidebarSelectDown),
    (SIDEBAR_TABLE, "Enter", ChromeAction::SidebarConfirm),
    (SIDEBAR_TABLE, "r", ChromeAction::SidebarRename),
    (SIDEBAR_TABLE, "Escape", ChromeAction::SidebarCancel),
    (SIDEBAR_TABLE, "q", ChromeAction::ToggleSidebar),
];

/// Desktop chrome that spells the same chord on every platform.
const DESKTOP_DEFAULTS: &[ChromeDefault] = &[
    (SIDEBAR_TABLE, "Escape", ChromeAction::SidebarCancel),
    (SIDEBAR_TABLE, "q", ChromeAction::SidebarCancel),
    (SIDEBAR_TABLE, "Enter", ChromeAction::SidebarConfirm),
    (SIDEBAR_TABLE, "r", ChromeAction::SidebarRename),
    (SIDEBAR_TABLE, ":", ChromeAction::SidebarCommandPalette),
    (SIDEBAR_TABLE, "Down", ChromeAction::SidebarSelectDown),
    (SIDEBAR_TABLE, "j", ChromeAction::SidebarSelectDown),
    (SIDEBAR_TABLE, "Up", ChromeAction::SidebarSelectUp),
    (SIDEBAR_TABLE, "k", ChromeAction::SidebarSelectUp),
    (SIDEBAR_TABLE, "Left", ChromeAction::SidebarSelectLeft),
    (SIDEBAR_TABLE, "h", ChromeAction::SidebarSelectLeft),
    (SIDEBAR_TABLE, "Right", ChromeAction::SidebarSelectRight),
    (SIDEBAR_TABLE, "l", ChromeAction::SidebarSelectRight),
    (SIDEBAR_TABLE, "g", ChromeAction::SidebarSelectFirst),
    (SIDEBAR_TABLE, "Home", ChromeAction::SidebarSelectFirst),
    (SIDEBAR_TABLE, "G", ChromeAction::SidebarSelectLast),
    (SIDEBAR_TABLE, "End", ChromeAction::SidebarSelectLast),
    (TERMINAL_TABLE, "C-=", ChromeAction::TerminalFontIncrease),
    (TERMINAL_TABLE, "C-+", ChromeAction::TerminalFontIncrease),
    (TERMINAL_TABLE, "C--", ChromeAction::TerminalFontDecrease),
    (TERMINAL_TABLE, "C-S-f", ChromeAction::TerminalSearch),
    (TERMINAL_TABLE, "D-f", ChromeAction::TerminalSearch),
    (TERMINAL_TABLE, "C-S-c", ChromeAction::TerminalCopy),
    (TERMINAL_TABLE, "D-c", ChromeAction::TerminalCopy),
    (TERMINAL_TABLE, "C-S-a", ChromeAction::TerminalSelectAll),
    (TERMINAL_TABLE, "D-a", ChromeAction::TerminalSelectAll),
    (TERMINAL_TABLE, "C-S-k", ChromeAction::TerminalClearHistory),
    (TERMINAL_TABLE, "D-k", ChromeAction::TerminalClearHistory),
    (TERMINAL_TABLE, "C-S-v", ChromeAction::TerminalPaste),
    (TERMINAL_TABLE, "D-v", ChromeAction::TerminalPaste),
    (BROWSER_TABLE, "C-Tab", ChromeAction::BrowserNextTab),
    (BROWSER_TABLE, "C-S-Tab", ChromeAction::BrowserPreviousTab),
];

/// Desktop chrome on Apple platforms, where the browser conventions are
/// Safari's.
const DESKTOP_COMMAND_DEFAULTS: &[ChromeDefault] = &[
    (UI_TABLE, "D-=", ChromeAction::UiZoomIn),
    (UI_TABLE, "D-+", ChromeAction::UiZoomIn),
    (UI_TABLE, "D--", ChromeAction::UiZoomOut),
    (UI_TABLE, "D-0", ChromeAction::UiZoomReset),
    (UI_TABLE, "D-,", ChromeAction::OpenSettings),
    (BROWSER_TABLE, "D-z", ChromeAction::BrowserUndo),
    (BROWSER_TABLE, "D-S-z", ChromeAction::BrowserRedo),
    (BROWSER_TABLE, "D-x", ChromeAction::BrowserCut),
    (BROWSER_TABLE, "D-c", ChromeAction::BrowserCopy),
    (BROWSER_TABLE, "D-v", ChromeAction::BrowserPaste),
    (
        BROWSER_TABLE,
        "D-S-v",
        ChromeAction::BrowserPasteAndMatchStyle,
    ),
    (BROWSER_TABLE, "D-a", ChromeAction::BrowserSelectAll),
    (BROWSER_TABLE, "D-=", ChromeAction::BrowserZoomIn),
    (BROWSER_TABLE, "D-+", ChromeAction::BrowserZoomIn),
    (BROWSER_TABLE, "D--", ChromeAction::BrowserZoomOut),
    (BROWSER_TABLE, "D-0", ChromeAction::BrowserZoomReset),
    (BROWSER_TABLE, "D-M-i", ChromeAction::BrowserDevTools),
    (BROWSER_TABLE, "D-t", ChromeAction::BrowserNewTab),
    (BROWSER_TABLE, "D-M-Right", ChromeAction::BrowserNextTab),
    (BROWSER_TABLE, "D-M-Left", ChromeAction::BrowserPreviousTab),
    (BROWSER_TABLE, "D-S-]", ChromeAction::BrowserNextTab),
    (BROWSER_TABLE, "D-S-[", ChromeAction::BrowserPreviousTab),
    (BROWSER_TABLE, "D-9", ChromeAction::BrowserSelectLastTab),
    (BROWSER_TABLE, "D-l", ChromeAction::BrowserFocusAddress),
    (BROWSER_TABLE, "D-r", ChromeAction::BrowserReload),
    (BROWSER_TABLE, "D-[", ChromeAction::BrowserBack),
    (BROWSER_TABLE, "D-]", ChromeAction::BrowserForward),
    (BROWSER_TABLE, "D-1", ChromeAction::BrowserSelectTab(0)),
    (BROWSER_TABLE, "D-2", ChromeAction::BrowserSelectTab(1)),
    (BROWSER_TABLE, "D-3", ChromeAction::BrowserSelectTab(2)),
    (BROWSER_TABLE, "D-4", ChromeAction::BrowserSelectTab(3)),
    (BROWSER_TABLE, "D-5", ChromeAction::BrowserSelectTab(4)),
    (BROWSER_TABLE, "D-6", ChromeAction::BrowserSelectTab(5)),
    (BROWSER_TABLE, "D-7", ChromeAction::BrowserSelectTab(6)),
    (BROWSER_TABLE, "D-8", ChromeAction::BrowserSelectTab(7)),
    (BROWSER_TABLE, "D-S-c", ChromeAction::BrowserElementSelector),
];

/// Desktop chrome everywhere else, where the browser conventions are Chrome's.
const DESKTOP_CONTROL_DEFAULTS: &[ChromeDefault] = &[
    (UI_TABLE, "C-=", ChromeAction::UiZoomIn),
    (UI_TABLE, "C-+", ChromeAction::UiZoomIn),
    (UI_TABLE, "C--", ChromeAction::UiZoomOut),
    (UI_TABLE, "C-0", ChromeAction::UiZoomReset),
    (UI_TABLE, "C-,", ChromeAction::OpenSettings),
    (BROWSER_TABLE, "C-z", ChromeAction::BrowserUndo),
    (BROWSER_TABLE, "C-y", ChromeAction::BrowserRedo),
    (BROWSER_TABLE, "C-S-z", ChromeAction::BrowserRedo),
    (BROWSER_TABLE, "C-x", ChromeAction::BrowserCut),
    (BROWSER_TABLE, "C-c", ChromeAction::BrowserCopy),
    (BROWSER_TABLE, "C-v", ChromeAction::BrowserPaste),
    (
        BROWSER_TABLE,
        "C-S-v",
        ChromeAction::BrowserPasteAndMatchStyle,
    ),
    (BROWSER_TABLE, "C-a", ChromeAction::BrowserSelectAll),
    (BROWSER_TABLE, "C-=", ChromeAction::BrowserZoomIn),
    (BROWSER_TABLE, "C-+", ChromeAction::BrowserZoomIn),
    (BROWSER_TABLE, "C--", ChromeAction::BrowserZoomOut),
    (BROWSER_TABLE, "C-0", ChromeAction::BrowserZoomReset),
    (BROWSER_TABLE, "C-S-i", ChromeAction::BrowserDevTools),
    (BROWSER_TABLE, "C-t", ChromeAction::BrowserNewTab),
    (BROWSER_TABLE, "C-w", ChromeAction::ClosePane),
    (BROWSER_TABLE, "C-NPage", ChromeAction::BrowserNextTab),
    (BROWSER_TABLE, "C-PPage", ChromeAction::BrowserPreviousTab),
    (BROWSER_TABLE, "C-9", ChromeAction::BrowserSelectLastTab),
    (BROWSER_TABLE, "C-l", ChromeAction::BrowserFocusAddress),
    (BROWSER_TABLE, "C-r", ChromeAction::BrowserReload),
    (BROWSER_TABLE, "F5", ChromeAction::BrowserReload),
    (BROWSER_TABLE, "M-Left", ChromeAction::BrowserBack),
    (BROWSER_TABLE, "M-Right", ChromeAction::BrowserForward),
    (BROWSER_TABLE, "C-1", ChromeAction::BrowserSelectTab(0)),
    (BROWSER_TABLE, "C-2", ChromeAction::BrowserSelectTab(1)),
    (BROWSER_TABLE, "C-3", ChromeAction::BrowserSelectTab(2)),
    (BROWSER_TABLE, "C-4", ChromeAction::BrowserSelectTab(3)),
    (BROWSER_TABLE, "C-5", ChromeAction::BrowserSelectTab(4)),
    (BROWSER_TABLE, "C-6", ChromeAction::BrowserSelectTab(5)),
    (BROWSER_TABLE, "C-7", ChromeAction::BrowserSelectTab(6)),
    (BROWSER_TABLE, "C-8", ChromeAction::BrowserSelectTab(7)),
    (BROWSER_TABLE, "C-S-c", ChromeAction::BrowserElementSelector),
];

/// The client-local half of the binding story: the same [`KeyTables`] data
/// model and resolution semantics the daemon uses for pane input, instantiated
/// over chrome tables that never cross the wire. Defaults are data; overrides
/// arrive through [`ChromeKeymap::bind`]/[`ChromeKeymap::unbind`].
#[derive(Debug)]
pub struct ChromeKeymap {
    tables: KeyTables,
}

impl Default for ChromeKeymap {
    fn default() -> Self {
        Self::for_profile(ChromeProfile::Tui)
    }
}

impl ChromeKeymap {
    /// The raw-terminal client's keymap. Desktop skins want
    /// [`ChromeKeymap::for_profile`] with [`ChromeProfile::DESKTOP`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn for_profile(profile: ChromeProfile) -> Self {
        let mut tables = KeyTables::empty();
        for (table, key, action) in profile.defaults().into_iter().flatten() {
            bind_action(&mut tables, table, key, *action);
        }
        Self { tables }
    }

    /// Resolve a key press against one chrome table.
    #[must_use]
    pub fn resolve(&self, table: &str, input: &KeyInput) -> Option<ChromeAction> {
        binding_action(self.binding_for(table, input)?)
    }

    /// Chrome names a press under its most specific spelling first, so a
    /// binding written without `S-` still catches a chord the user shifts.
    fn binding_for(&self, table: &str, input: &KeyInput) -> Option<&Binding> {
        if input.modifiers.shift()
            && let Some(binding) = self
                .tables
                .get(table, &pressed_key(input, true).to_string())
        {
            return Some(binding);
        }
        if input.modifiers.platform() {
            return self
                .tables
                .get(table, &pressed_key(input, false).to_string());
        }
        self.tables.resolve_input(table, input)
    }

    /// The action a chord is bound to right now, for surfaces that dispatched
    /// on a chord and must not act on a binding the user has since replaced.
    #[must_use]
    pub fn action_for(&self, table: &str, key: &str) -> Option<ChromeAction> {
        let key = ChromeKey::parse(key)?;
        binding_action(self.tables.get(table, &key.to_string())?)
    }

    /// Bind `key` in `table` to a named chrome action, replacing any default.
    ///
    /// # Errors
    /// Returns the offending name when it is not a chrome action.
    pub fn bind(
        &mut self,
        table: &str,
        key: &str,
        action: &str,
    ) -> Result<(), UnknownChromeAction> {
        let action = ChromeAction::from_name(action)
            .ok_or_else(|| UnknownChromeAction(action.to_owned()))?;
        bind_action(&mut self.tables, table, key, action);
        Ok(())
    }

    /// Remove a chrome binding; true when one existed.
    pub fn unbind(&mut self, table: &str, key: &str) -> bool {
        self.tables.unbind(table, &canonical_chrome_key(key))
    }

    /// One table's bindings, for the skins that turn chrome data into their own
    /// binding model.
    #[must_use]
    pub fn table_bindings(&self, table: &str) -> Vec<(ChromeKey, ChromeAction)> {
        self.tables
            .list(Some(table))
            .filter_map(|(_, key, binding)| {
                Some((ChromeKey::parse(key)?, binding_action(binding)?))
            })
            .collect()
    }

    /// Every chrome binding, flattened for help and settings surfaces in the
    /// same shape the daemon publishes its tables.
    #[must_use]
    pub fn bindings(&self) -> Vec<(String, String, ChromeAction)> {
        self.tables
            .list(None)
            .filter_map(|(table, key, binding)| {
                binding_action(binding).map(|action| (table.to_owned(), key.to_owned(), action))
            })
            .collect()
    }
}

fn bind_action(tables: &mut KeyTables, table: &str, key: &str, action: ChromeAction) {
    tables.bind(
        table,
        &canonical_chrome_key(key),
        Binding {
            commands: vec![CommandInvocation::new(action.name(), [] as [&str; 0])],
            repeat: false,
            note: None,
        },
    );
}

fn canonical_chrome_key(key: &str) -> String {
    ChromeKey::parse(key).map_or_else(|| key.to_owned(), |key| key.to_string())
}

fn binding_action(binding: &Binding) -> Option<ChromeAction> {
    let [command] = binding.commands.as_slice() else {
        return None;
    };
    if !command.args.is_empty() {
        return None;
    }
    ChromeAction::from_name(&command.name)
}

fn pressed_key(input: &KeyInput, shift: bool) -> ChromeKey {
    ChromeKey {
        command: input.modifiers.platform(),
        control: input.modifiers.control(),
        alt: input.modifiers.alt(),
        shift,
        base: base_name(input),
    }
    .normalized()
}

/// The press's key name with every modifier stripped, so the chrome spelling
/// owns the modifiers the wire fold would have erased.
fn base_name(input: &KeyInput) -> String {
    let bare = KeyInput {
        action: input.action,
        key: input.key,
        modifiers: Modifiers::new(false, false, false, false),
        text: None,
        unshifted_codepoint: input.unshifted_codepoint,
    };
    let name = input_key_name(&bare);
    if name.is_empty() {
        return input.text.as_deref().unwrap_or_default().to_owned();
    }
    name.into_string()
}

#[cfg(test)]
mod tests {
    use zz_terminal::{KeyAction, KeyCode};

    use super::*;

    fn chord(key: KeyCode, control: bool, alt: bool, text: Option<&str>) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key,
            modifiers: Modifiers::new(false, control, alt, false),
            text: text.map(|text| text.to_owned().into_boxed_str()),
            unshifted_codepoint: None,
        }
    }

    fn press(key: KeyCode, modifiers: Modifiers) -> KeyInput {
        let character = match key {
            KeyCode::Character(character) => Some(character),
            _ => None,
        };
        KeyInput {
            action: KeyAction::Press,
            key,
            modifiers,
            text: character.map(|character| character.to_string().into_boxed_str()),
            unshifted_codepoint: character,
        }
    }

    #[test]
    fn defaults_resolve_the_stock_chrome_chords() {
        let keymap = ChromeKeymap::new();
        assert_eq!(
            keymap.resolve("ui", &chord(KeyCode::Character('\\'), true, false, None)),
            Some(ChromeAction::Detach)
        );
        assert_eq!(
            keymap.resolve("ui", &chord(KeyCode::Character('s'), false, true, None)),
            Some(ChromeAction::ToggleSidebar)
        );
        assert_eq!(
            keymap.resolve(
                "browser",
                &chord(KeyCode::Character('0'), true, false, None)
            ),
            Some(ChromeAction::BrowserZoomReset)
        );
        assert_eq!(
            keymap.resolve(
                "ui",
                &chord(KeyCode::Character('s'), false, false, Some("s"))
            ),
            None
        );
    }

    #[test]
    fn overrides_rebind_and_unbind() {
        let mut keymap = ChromeKeymap::new();
        keymap.bind("ui", "C-d", "detach").expect("known action");
        assert_eq!(
            keymap.resolve("ui", &chord(KeyCode::Character('d'), true, false, None)),
            Some(ChromeAction::Detach)
        );
        assert!(keymap.unbind("ui", "C-\\"));
        assert_eq!(
            keymap.resolve("ui", &chord(KeyCode::Character('\\'), true, false, None)),
            None
        );
        assert_eq!(
            keymap.bind("ui", "x", "no-such-action"),
            Err(UnknownChromeAction("no-such-action".to_owned()))
        );
    }

    #[test]
    fn bindings_flatten_for_help_surfaces() {
        let bindings = ChromeKeymap::new().bindings();
        assert!(bindings.contains(&("ui".to_owned(), "C-\\".to_owned(), ChromeAction::Detach)));
        assert_eq!(
            bindings.len(),
            TUI_DEFAULTS.len() + TUI_SIDEBAR_DEFAULTS.len()
        );
    }

    #[test]
    fn every_action_round_trips_through_its_name() {
        for profile in [
            ChromeProfile::Tui,
            ChromeProfile::Desktop,
            ChromeProfile::DesktopApple,
        ] {
            for (_, _, action) in ChromeKeymap::for_profile(profile).bindings() {
                assert_eq!(ChromeAction::from_name(action.name()), Some(action));
            }
        }
        assert_eq!(
            ChromeAction::from_name("browser-select-tab-8"),
            Some(ChromeAction::BrowserSelectTab(7))
        );
        assert_eq!(ChromeAction::from_name("browser-select-tab-9"), None);
        assert_eq!(ChromeAction::from_name("browser-select-tab-0"), None);
        assert_eq!(
            ChromeAction::BrowserSelectTab(8).name(),
            "browser-select-tab"
        );
        assert_eq!(
            ChromeAction::BrowserSelectTab(u8::MAX).name(),
            "browser-select-tab"
        );
    }

    #[test]
    fn chrome_keys_canonicalize_their_modifier_spellings() {
        for (spelling, canonical) in [
            ("Cmd-Alt-i", "D-M-i"),
            ("Ctrl-Shift-f", "C-S-f"),
            ("C-F", "C-S-f"),
            ("S-g", "G"),
            ("Super-t", "D-t"),
            ("Space", " "),
            ("C-Space", "C- "),
            ("F5", "F5"),
        ] {
            let key = ChromeKey::parse(spelling).expect("valid chrome key");
            assert_eq!(key.to_string(), canonical, "{spelling}");
        }
        assert_eq!(ChromeKey::parse(""), None);
        assert_eq!(ChromeKey::parse("C-"), None);
    }

    #[test]
    fn a_shifted_chord_falls_back_to_the_unshifted_spelling() {
        let keymap = ChromeKeymap::for_profile(ChromeProfile::Desktop);
        let shifted = Modifiers::new(true, true, false, false);
        assert_eq!(
            keymap.resolve("terminal", &press(KeyCode::Character('='), shifted)),
            Some(ChromeAction::TerminalFontIncrease)
        );
        let command_shift = Modifiers::new(true, false, false, true);
        assert_eq!(
            keymap.resolve("terminal", &press(KeyCode::Character('f'), command_shift)),
            Some(ChromeAction::TerminalSearch)
        );
    }

    #[test]
    fn terminal_chrome_never_claims_an_unshifted_control_chord() {
        let keymap = ChromeKeymap::for_profile(ChromeProfile::Desktop);
        let control = Modifiers::new(false, true, false, false);
        for character in ['f', 'c', 'a', 'k', 'v'] {
            assert_eq!(
                keymap.resolve("terminal", &press(KeyCode::Character(character), control)),
                None,
                "C-{character} belongs to the pane"
            );
        }
        let control_shift = Modifiers::new(true, true, false, false);
        assert_eq!(
            keymap.resolve("terminal", &press(KeyCode::Character('c'), control_shift)),
            Some(ChromeAction::TerminalCopy)
        );
    }

    #[test]
    fn desktop_browser_chords_resolve_per_platform() {
        let command = Modifiers::new(false, false, false, true);
        let control = Modifiers::new(false, true, false, false);
        let apple = ChromeKeymap::for_profile(ChromeProfile::DesktopApple);
        let other = ChromeKeymap::for_profile(ChromeProfile::Desktop);

        for (character, action) in [
            ('t', ChromeAction::BrowserNewTab),
            ('l', ChromeAction::BrowserFocusAddress),
            ('r', ChromeAction::BrowserReload),
            ('0', ChromeAction::BrowserZoomReset),
            ('9', ChromeAction::BrowserSelectLastTab),
            ('3', ChromeAction::BrowserSelectTab(2)),
        ] {
            assert_eq!(
                apple.resolve("browser", &press(KeyCode::Character(character), command)),
                Some(action),
                "cmd-{character}"
            );
            assert_eq!(
                other.resolve("browser", &press(KeyCode::Character(character), control)),
                Some(action),
                "ctrl-{character}"
            );
        }

        assert_eq!(
            apple.resolve("browser", &press(KeyCode::Character('['), command)),
            Some(ChromeAction::BrowserBack)
        );
        assert_eq!(
            other.resolve(
                "browser",
                &press(
                    KeyCode::ArrowLeft,
                    Modifiers::new(false, false, true, false)
                )
            ),
            Some(ChromeAction::BrowserBack)
        );
        assert_eq!(
            other.resolve(
                "browser",
                &press(KeyCode::Function(5), Modifiers::default())
            ),
            Some(ChromeAction::BrowserReload)
        );
        assert_eq!(
            other.resolve("browser", &press(KeyCode::PageDown, control)),
            Some(ChromeAction::BrowserNextTab)
        );
        assert_eq!(
            apple.resolve("browser", &press(KeyCode::Character('w'), command)),
            None,
            "cmd-w stays with the native menu"
        );
    }

    #[test]
    fn the_sidebar_tells_a_shifted_letter_from_its_bare_press() {
        let keymap = ChromeKeymap::for_profile(ChromeProfile::Desktop);
        assert_eq!(
            keymap.resolve(
                "sidebar",
                &press(KeyCode::Character('g'), Modifiers::default())
            ),
            Some(ChromeAction::SidebarSelectFirst)
        );
        assert_eq!(
            keymap.resolve(
                "sidebar",
                &press(
                    KeyCode::Character('g'),
                    Modifiers::new(true, false, false, false)
                )
            ),
            Some(ChromeAction::SidebarSelectLast)
        );
    }

    #[test]
    fn a_rebound_chord_reports_the_action_it_now_carries() {
        let mut keymap = ChromeKeymap::for_profile(ChromeProfile::Desktop);
        assert_eq!(
            keymap.action_for("browser", "Ctrl-Shift-c"),
            Some(ChromeAction::BrowserElementSelector)
        );
        keymap
            .bind("browser", "C-S-c", "browser-devtools")
            .expect("known action");
        assert_eq!(
            keymap.action_for("browser", "C-S-c"),
            Some(ChromeAction::BrowserDevTools)
        );
        assert!(keymap.unbind("browser", "Ctrl-Shift-c"));
        assert_eq!(keymap.action_for("browser", "C-S-c"), None);
    }
}
