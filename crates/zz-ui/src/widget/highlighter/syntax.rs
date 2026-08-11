//! Inert highlighter and language registry used when `tree-sitter` is off.

use std::{
    collections::HashMap,
    ops::Range,
    sync::{LazyLock, Mutex, MutexGuard, PoisonError},
};

use gpui::{HighlightStyle, SharedString};

use super::theme::HighlightTheme;

#[derive(Debug)]
pub struct SyntaxHighlighter {
    language: SharedString,
}

impl SyntaxHighlighter {
    pub fn new(language: impl AsRef<str>) -> Self {
        Self {
            language: SharedString::new(language),
        }
    }

    pub fn language(&self) -> &SharedString {
        &self.language
    }

    /// No-op: there is no parser to feed.
    pub fn update(&mut self, _text: &str) {}

    /// Always empty, so code renders unstyled.
    pub fn styles(
        &self,
        _range: &Range<usize>,
        _theme: &HighlightTheme,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        Vec::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageConfig {
    pub name: SharedString,
}

#[derive(Debug, Default)]
pub struct LanguageRegistry {
    languages: Mutex<HashMap<SharedString, LanguageConfig>>,
}

impl LanguageRegistry {
    pub fn singleton() -> &'static LanguageRegistry {
        static INSTANCE: LazyLock<LanguageRegistry> = LazyLock::new(LanguageRegistry::default);
        &INSTANCE
    }

    /// Registers `config` under `lang`, replacing any previous entry.
    pub fn register(&self, lang: &str, config: &LanguageConfig) {
        self.entries()
            .insert(SharedString::new(lang), config.clone());
    }

    pub fn language(&self, name: &str) -> Option<LanguageConfig> {
        self.entries().get(name).cloned()
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<SharedString, LanguageConfig>> {
        self.languages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighting_is_inert() {
        let mut highlighter = SyntaxHighlighter::new("rust");
        highlighter.update("fn main() { let x = 1; }");

        assert!(
            highlighter
                .styles(&(0..24), &HighlightTheme::default_light())
                .is_empty()
        );
    }

    #[test]
    fn registry_round_trips_a_registration() {
        let registry = LanguageRegistry::default();
        assert!(registry.language("rust").is_none());

        let config = LanguageConfig {
            name: "Rust".into(),
        };
        registry.register("rust", &config);

        assert_eq!(registry.language("rust"), Some(config));
        assert!(registry.language("zig").is_none());
    }
}
