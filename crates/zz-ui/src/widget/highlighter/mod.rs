//! Syntax palettes plus an optional tree-sitter engine.

#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

#[cfg(feature = "tree-sitter")]
mod highlighter;
#[cfg(feature = "tree-sitter")]
mod languages;
mod palette;
#[cfg(feature = "tree-sitter")]
mod registry;
#[cfg(not(feature = "tree-sitter"))]
mod syntax;
mod theme;

#[cfg(feature = "tree-sitter")]
pub use highlighter::SyntaxHighlighter;
#[cfg(feature = "tree-sitter")]
pub use languages::Language;
#[cfg(feature = "tree-sitter")]
pub use registry::{LanguageConfig, LanguageRegistry};
#[cfg(not(feature = "tree-sitter"))]
pub use syntax::{LanguageConfig, LanguageRegistry, SyntaxHighlighter};
pub use theme::{
    FontStyle, FontWeightContent, HighlightTheme, HighlightThemeStyle, SyntaxColors, ThemeStyle,
};
