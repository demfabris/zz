//! Built-in tree-sitter grammars. `Plain` has none, so nothing parses it.

use gpui::SharedString;

use super::LanguageConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Plain,
    Json,
    Markdown,
    MarkdownInline,
    Rust,
    Tmux,
    Toml,
}

impl From<Language> for SharedString {
    fn from(language: Language) -> Self {
        language.name().into()
    }
}

impl Language {
    pub fn all() -> impl Iterator<Item = Self> {
        [
            Self::Plain,
            Self::Json,
            Self::Markdown,
            Self::MarkdownInline,
            Self::Rust,
            Self::Tmux,
            Self::Toml,
        ]
        .into_iter()
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Plain => "text",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::MarkdownInline => "markdown_inline",
            Self::Rust => "rust",
            Self::Tmux => "tmux",
            Self::Toml => "toml",
        }
    }

    pub fn from_str(name: &str) -> Self {
        Self::from_name(name).unwrap_or(Self::Plain)
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "text" | "plain" | "plaintext" => Some(Self::Plain),
            "json" | "jsonc" => Some(Self::Json),
            "markdown" | "md" | "mdx" => Some(Self::Markdown),
            "markdown_inline" | "markdown-inline" => Some(Self::MarkdownInline),
            "rust" | "rs" => Some(Self::Rust),
            "tmux" | "tmux.conf" => Some(Self::Tmux),
            "toml" => Some(Self::Toml),
            _ => None,
        }
    }

    fn injection_languages(self) -> Vec<SharedString> {
        match self {
            Self::Markdown => ["markdown_inline", "rust", "json", "toml"]
                .into_iter()
                .map(Into::into)
                .collect(),
            Self::Rust => vec!["rust".into()],
            _ => Vec::new(),
        }
    }

    pub(super) fn config(self) -> LanguageConfig {
        let (language, highlights, injections, locals) = match self {
            Self::Plain => return LanguageConfig::plain(self.name()),
            Self::Json => (
                tree_sitter_json::LANGUAGE,
                include_str!("languages/json/highlights.scm"),
                "",
                "",
            ),
            Self::Markdown => (
                tree_sitter_md::LANGUAGE,
                include_str!("languages/markdown/highlights.scm"),
                include_str!("languages/markdown/injections.scm"),
                "",
            ),
            Self::MarkdownInline => (
                tree_sitter_md::INLINE_LANGUAGE,
                include_str!("languages/markdown_inline/highlights.scm"),
                "",
                "",
            ),
            Self::Rust => (
                tree_sitter_rust::LANGUAGE,
                include_str!("languages/rust/highlights.scm"),
                include_str!("languages/rust/injections.scm"),
                "",
            ),
            Self::Tmux => (
                tree_sitter_tmux::LANGUAGE,
                tree_sitter_tmux::HIGHLIGHTS_QUERY,
                "",
                "",
            ),
            Self::Toml => (
                tree_sitter_toml_ng::LANGUAGE,
                tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
                "",
                "",
            ),
        };

        LanguageConfig::new(
            self.name(),
            language.into(),
            self.injection_languages(),
            highlights,
            injections,
            locals,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_and_unknown_language_are_stable() {
        assert_eq!(Language::from_str("rs"), Language::Rust);
        assert_eq!(Language::from_str("md"), Language::Markdown);
        assert_eq!(Language::from_str("unknown"), Language::Plain);
    }
}
