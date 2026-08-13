use std::sync::Arc;

/// tmux's default separator set: printable non-alphanumeric ASCII, no underscore.
pub const DEFAULT_WORD_SEPARATORS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~";

const WORD_WHITESPACE_CODEPOINTS: &[char] = &[
    '\0', '\t', '\n', '\u{b}', '\u{c}', '\r', ' ', '\u{85}', '\u{a0}', '\u{1680}', '\u{2000}',
    '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}',
    '\u{2009}', '\u{200a}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}',
];

/// Precompiled word-separator lookup shared by live selection and copy mode.
/// Boundaries always include NUL, tab, and space, as tmux does.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordSeparators {
    ascii_mask: [u64; 2],
    separator_codepoints: Arc<[char]>,
    boundary_codepoints: Arc<[char]>,
}

impl Default for WordSeparators {
    fn default() -> Self {
        Self::new(DEFAULT_WORD_SEPARATORS)
    }
}

impl WordSeparators {
    #[must_use]
    pub fn new(value: &str) -> Self {
        let mut ascii_mask = [0_u64; 2];
        let mut separator_codepoints = value
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<Vec<_>>();
        separator_codepoints.sort_unstable();
        separator_codepoints.dedup();
        for &character in &separator_codepoints {
            let codepoint = u32::from(character);
            if codepoint < 128 {
                let word = usize::from(codepoint >= 64);
                ascii_mask[word] |= 1_u64 << (codepoint % 64);
            }
        }

        let mut boundary_codepoints = separator_codepoints.clone();
        boundary_codepoints.extend_from_slice(WORD_WHITESPACE_CODEPOINTS);
        boundary_codepoints.sort_unstable();
        boundary_codepoints.dedup();
        Self {
            ascii_mask,
            separator_codepoints: separator_codepoints.into(),
            boundary_codepoints: boundary_codepoints.into(),
        }
    }

    /// Whether a non-whitespace codepoint is a configured separator.
    #[must_use]
    pub fn contains_separator(&self, character: char) -> bool {
        let codepoint = u32::from(character);
        if codepoint < 128 {
            let word = usize::from(codepoint >= 64);
            return self.ascii_mask[word] & (1_u64 << (codepoint % 64)) != 0;
        }
        self.separator_codepoints.binary_search(&character).is_ok()
    }

    /// The configured non-whitespace separators.
    #[must_use]
    pub fn separator_codepoints(&self) -> &[char] {
        &self.separator_codepoints
    }

    /// The precompiled boundary list libghostty selection expects.
    #[must_use]
    pub fn boundary_codepoints(&self) -> &[char] {
        &self.boundary_codepoints
    }

    /// Every `char::is_whitespace` codepoint, plus NUL for empty cells.
    #[must_use]
    pub const fn whitespace_codepoints() -> &'static [char] {
        WORD_WHITESPACE_CODEPOINTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_tmux_and_keeps_a_compact_cold_layout() {
        let separators = WordSeparators::default();
        assert!(separators.contains_separator('.'));
        assert!(separators.contains_separator('|'));
        assert!(!separators.contains_separator('_'));
        assert!(!separators.contains_separator('a'));
        assert!(separators.boundary_codepoints().contains(&' '));
        assert!(separators.boundary_codepoints().contains(&'\t'));
        assert!(separators.boundary_codepoints().contains(&'\0'));
        assert!(std::mem::size_of::<WordSeparators>() <= 48);
        assert_eq!(
            std::mem::align_of::<WordSeparators>(),
            std::mem::align_of::<u64>()
        );
    }

    #[test]
    fn empty_option_keeps_only_terminal_whitespace_boundaries() {
        let separators = WordSeparators::new("");
        assert!(!separators.contains_separator('.'));
        assert!(!separators.contains_separator(' '));
        assert!(separators.separator_codepoints().is_empty());
        assert_eq!(
            separators.boundary_codepoints(),
            WordSeparators::whitespace_codepoints()
        );
    }
}
