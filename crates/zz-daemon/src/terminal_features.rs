//! The terminal features a client's roster carries, and the colours they
//! decide. `tty-features.c` keeps the same table for the pin, and both halves
//! of this crate read it: the daemon to publish `client_termfeatures` and
//! `client_colours`, and a client to know what its own terminal takes.

pub(crate) const TERMINAL_FEATURES: [&str; 21] = [
    "256",
    "bpaste",
    "ccolour",
    "clipboard",
    "hyperlinks",
    "cstyle",
    "extkeys",
    "focus",
    "ignorefkeys",
    "margins",
    "mouse",
    "osc7",
    "overline",
    "progressbar",
    "rectfill",
    "RGB",
    "sixel",
    "strikethrough",
    "sync",
    "title",
    "usstyle",
];

pub(crate) fn terminal_feature_bit(name: &str) -> Option<u32> {
    TERMINAL_FEATURES
        .iter()
        .position(|feature| feature.eq_ignore_ascii_case(name))
        .map(|index| 1 << index)
}


pub(crate) fn terminal_features_list(features: u32) -> String {
    TERMINAL_FEATURES
        .iter()
        .enumerate()
        .filter(|(index, _)| features & (1 << index) != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(",")
}


/// `tty_term_create` over `tty_default_features`, then `tty_check_fg`'s own
/// question: `TERM_RGBCOLOURS` from a `RGB` feature or a truecolor
/// `COLORTERM`, `TERM_256COLOURS` from a `256` feature or a `*-256color`
/// terminal, and otherwise the terminal's `colors` number, which terminfo
/// gives as 8 for `xterm`, `screen` and their kin and 16 for `*-16color`.
pub fn terminal_colour_count(term: &str, colour_term: &str, requested: u32) -> u32 {
    let colour_term = colour_term.to_ascii_lowercase();
    let term = term.to_ascii_lowercase();
    let has = |name| terminal_feature_bit(name).is_some_and(|bit| requested & bit != 0);
    if has("RGB")
        || matches!(colour_term.as_str(), "truecolor" | "24bit")
        || term.contains("truecolor")
    {
        16_777_216
    } else if has("256") || term.contains("256color") {
        256
    } else if term.contains("16color") {
        16
    } else if term == "dumb" {
        2
    } else {
        8
    }
}

/// The mask a client's feature specs carry, for the hello's capabilities and
/// for a caller holding the raw specs `-T` and `-2` left behind alike.
pub fn terminal_feature_mask<'a>(specs: impl IntoIterator<Item = &'a str>) -> u32 {
    let mut features = 0;
    for spec in specs {
        for name in spec.split([':', ',']) {
            let Some(bit) = terminal_feature_bit(name) else {
                break;
            };
            features |= bit;
        }
    }
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `tty_term_create` reads `colors` out of terminfo, where `xterm`,
    /// `screen` and their kin carry 8 and only a `*-16color` entry carries 16,
    /// and `tty_add_features` raises it from `COLORTERM` and from what `-2`
    /// and `-T` requested.
    #[test]
    fn a_terminal_takes_the_colours_its_name_and_its_features_give_it() {
        let bare = |term| terminal_colour_count(term, "", 0);
        assert_eq!(bare("xterm"), 8);
        assert_eq!(bare("screen"), 8);
        assert_eq!(bare("xterm-16color"), 16);
        assert_eq!(bare("xterm-256color"), 256);
        assert_eq!(bare("dumb"), 2);
        assert_eq!(terminal_colour_count("xterm", "truecolor", 0), 16_777_216);
        assert_eq!(terminal_colour_count("xterm", "24bit", 0), 16_777_216);
        let requested = |spec: &str| terminal_feature_mask([spec]);
        assert_eq!(terminal_colour_count("xterm", "", requested("256")), 256);
        assert_eq!(
            terminal_colour_count("xterm", "", requested("RGB")),
            16_777_216
        );
        assert_eq!(
            terminal_colour_count("xterm", "", requested("sixel:RGB")),
            16_777_216
        );
        assert_eq!(terminal_colour_count("xterm", "", requested("nope")), 8);
    }
}
