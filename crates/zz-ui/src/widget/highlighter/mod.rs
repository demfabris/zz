//! Syntax palettes plus an optional tree-sitter engine.

#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

#[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
mod highlighter;
#[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
mod languages;
mod palette;
#[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
mod registry;
#[cfg(any(not(feature = "tree-sitter"), target_family = "wasm"))]
mod syntax;
mod theme;

#[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
pub use highlighter::SyntaxHighlighter;
#[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
pub use languages::Language;
#[cfg(all(feature = "tree-sitter", not(target_family = "wasm")))]
pub use registry::{LanguageConfig, LanguageRegistry};
#[cfg(any(not(feature = "tree-sitter"), target_family = "wasm"))]
pub use syntax::{LanguageConfig, LanguageRegistry, SyntaxHighlighter};
pub use theme::{
    FontStyle, FontWeightContent, HighlightTheme, HighlightThemeStyle, SyntaxColors, ThemeStyle,
};
