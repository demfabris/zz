use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, MutexGuard, PoisonError},
};

use gpui::SharedString;

use super::{Language, languages};

/// Parser and query data for one language. A `None` grammar is never parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageConfig {
    pub name: SharedString,
    pub language: Option<tree_sitter::Language>,
    pub injection_languages: Vec<SharedString>,
    pub highlights: SharedString,
    pub injections: SharedString,
    pub locals: SharedString,
}

impl LanguageConfig {
    pub fn new(
        name: impl Into<SharedString>,
        language: tree_sitter::Language,
        injection_languages: Vec<SharedString>,
        highlights: &str,
        injections: &str,
        locals: &str,
    ) -> Self {
        Self {
            name: name.into(),
            language: Some(language),
            injection_languages,
            highlights: highlights.to_string().into(),
            injections: injections.to_string().into(),
            locals: locals.to_string().into(),
        }
    }

    pub fn plain(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            language: None,
            injection_languages: Vec::new(),
            highlights: SharedString::default(),
            injections: SharedString::default(),
            locals: SharedString::default(),
        }
    }
}

/// Process-wide registry for the built-in syntax grammars.
pub struct LanguageRegistry {
    languages: Mutex<HashMap<SharedString, LanguageConfig>>,
}

impl LanguageRegistry {
    /// The registry, pre-populated with the built-in grammars.
    pub fn singleton() -> &'static Self {
        static INSTANCE: LazyLock<LanguageRegistry> = LazyLock::new(|| LanguageRegistry {
            languages: Mutex::new(
                languages::Language::all()
                    .map(|language| (language.name().into(), language.config()))
                    .collect(),
            ),
        });
        &INSTANCE
    }

    /// Register or replace a language configuration.
    pub fn register(&self, lang: &str, config: &LanguageConfig) {
        self.entries().insert(lang.into(), config.clone());
    }

    /// Look up a language by canonical name or supported short alias.
    pub fn language(&self, name: &str) -> Option<LanguageConfig> {
        let entries = self.entries();
        entries.get(name).cloned().or_else(|| {
            Language::from_name(name).and_then(|language| entries.get(language.name()).cloned())
        })
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
    fn registry_contains_the_supported_grammar_set() {
        let registry = LanguageRegistry::singleton();
        for language in ["json", "markdown", "rust", "tmux", "toml"] {
            assert!(registry.language(language).is_some(), "{language}");
        }
        assert!(registry.language("javascript").is_none());
    }

    #[test]
    fn aliases_resolve_to_canonical_languages() {
        let registry = LanguageRegistry::singleton();
        assert_eq!(registry.language("rs").unwrap().name.as_ref(), "rust");
        assert_eq!(registry.language("md").unwrap().name.as_ref(), "markdown");
    }
}
