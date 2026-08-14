use zz_protocol::{Binding, CommandInvocation, KeyTables};
use zz_terminal::KeyInput;

/// A client-local chrome action: behavior that never crosses the wire because
/// it belongs to the skin (detach, sidebar focus, local browser zoom). Skins
/// switch on the resolved action and never inspect chords themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeAction {
    Detach,
    ToggleSidebar,
    BrowserZoomIn,
    BrowserZoomOut,
    BrowserZoomReset,
}

impl ChromeAction {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Detach => "detach",
            Self::ToggleSidebar => "toggle-sidebar",
            Self::BrowserZoomIn => "browser-zoom-in",
            Self::BrowserZoomOut => "browser-zoom-out",
            Self::BrowserZoomReset => "browser-zoom-reset",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "detach" => Self::Detach,
            "toggle-sidebar" => Self::ToggleSidebar,
            "browser-zoom-in" => Self::BrowserZoomIn,
            "browser-zoom-out" => Self::BrowserZoomOut,
            "browser-zoom-reset" => Self::BrowserZoomReset,
            _ => return None,
        })
    }
}

/// The named action was not one [`ChromeAction`] knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownChromeAction(pub String);

/// Default chrome bindings, as data. The `ui` table is consulted for every
/// key; `browser` only while a browser surface has focus.
const CHROME_DEFAULTS: &[(&str, &str, ChromeAction)] = &[
    ("ui", "C-\\", ChromeAction::Detach),
    ("ui", "M-s", ChromeAction::ToggleSidebar),
    ("ui", "M-S", ChromeAction::ToggleSidebar),
    ("browser", "C-=", ChromeAction::BrowserZoomIn),
    ("browser", "C-+", ChromeAction::BrowserZoomIn),
    ("browser", "C--", ChromeAction::BrowserZoomOut),
    ("browser", "C-_", ChromeAction::BrowserZoomOut),
    ("browser", "C-0", ChromeAction::BrowserZoomReset),
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
        let mut tables = KeyTables::empty();
        for (table, key, action) in CHROME_DEFAULTS {
            tables.bind(
                table,
                key,
                Binding {
                    commands: vec![CommandInvocation::new(action.name(), [] as [&str; 0])],
                    repeat: false,
                    note: None,
                },
            );
        }
        Self { tables }
    }
}

impl ChromeKeymap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve a key press against one chrome table.
    #[must_use]
    pub fn resolve(&self, table: &str, input: &KeyInput) -> Option<ChromeAction> {
        let binding = self.tables.resolve_input(table, input)?;
        let [command] = binding.commands.as_slice() else {
            return None;
        };
        if !command.args.is_empty() {
            return None;
        }
        ChromeAction::from_name(&command.name)
    }

    /// Bind `key` in `table` to a named chrome action, replacing any default.
    pub fn bind(
        &mut self,
        table: &str,
        key: &str,
        action: &str,
    ) -> Result<(), UnknownChromeAction> {
        let action = ChromeAction::from_name(action)
            .ok_or_else(|| UnknownChromeAction(action.to_owned()))?;
        self.tables.bind(
            table,
            key,
            Binding {
                commands: vec![CommandInvocation::new(action.name(), [] as [&str; 0])],
                repeat: false,
                note: None,
            },
        );
        Ok(())
    }

    /// Remove a chrome binding; true when one existed.
    pub fn unbind(&mut self, table: &str, key: &str) -> bool {
        self.tables.unbind(table, key)
    }

    /// Every chrome binding, flattened for help and settings surfaces in the
    /// same shape the daemon publishes its tables.
    #[must_use]
    pub fn bindings(&self) -> Vec<(String, String, ChromeAction)> {
        self.tables
            .list(None)
            .filter_map(|(table, key, binding)| {
                let [command] = binding.commands.as_slice() else {
                    return None;
                };
                ChromeAction::from_name(&command.name)
                    .map(|action| (table.to_owned(), key.to_owned(), action))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use zz_terminal::{KeyAction, KeyCode, Modifiers};

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
        assert_eq!(bindings.len(), CHROME_DEFAULTS.len());
    }
}
