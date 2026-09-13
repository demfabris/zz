//! The terminal features a client's roster carries, and the colours they
//! decide. `tty-features.c` keeps the same table for the pin, and both halves
//! of this crate read it: the daemon to publish `client_termfeatures` and
//! `client_colours`, and a client to know what its own terminal takes.

#[cfg_attr(not(feature = "daemon"), allow(dead_code))]
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

/// `TTY_FEATURES_BASE_MODERN_XTERM`, the base every modern-xterm entry of
/// `tty_default_features` opens with.
macro_rules! modern_xterm_features {
    ($extra:literal) => {
        concat!(
            "256,RGB,bpaste,clipboard,mouse,strikethrough,title,",
            $extra
        )
    };
}

/// `tty_default_features`: the features each terminal the pin can name by a
/// reply carries. `tty_keys_device_attributes2` reads a secondary DA's first
/// parameter as a letter and `tty_keys_extended_device_attributes` reads an
/// XTVERSION reply's leading text, and both hand the name here. A name this
/// table does not carry adds nothing.
pub fn terminal_default_features(name: &str) -> &'static str {
    match name {
        "mintty" => modern_xterm_features!("ccolour,cstyle,extkeys,margins,overline,usstyle"),
        "tmux" => modern_xterm_features!(
            "ccolour,cstyle,extkeys,focus,overline,usstyle,hyperlinks,progressbar"
        ),
        "rxvt-unicode" => "256,bpaste,ccolour,cstyle,mouse,title,ignorefkeys",
        "iTerm2" => modern_xterm_features!(
            "cstyle,extkeys,margins,usstyle,sync,osc7,hyperlinks,progressbar"
        ),
        "foot" => modern_xterm_features!("ccolour,cstyle,extkeys,usstyle,sync,osc7,hyperlinks"),
        "WezTerm" => modern_xterm_features!("ccolour,cstyle,extkeys,focus,hyperlinks,usstyle"),
        "ghostty" => modern_xterm_features!(
            "ccolour,cstyle,extkeys,focus,overline,hyperlinks,osc7,sync,usstyle,progressbar"
        ),
        "XTerm" => modern_xterm_features!("ccolour,cstyle,extkeys,focus"),
        _ => "",
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

    /// `tty_default_features`'s own table, name for name, as tty-features.c
    /// spells it at the pin's d77c9dc6.
    #[test]
    fn the_named_terminals_carry_the_features_the_pin_gives_them() {
        let list =
            |name| terminal_features_list(terminal_feature_mask([terminal_default_features(name)]));
        assert_eq!(
            list("tmux"),
            "256,bpaste,ccolour,clipboard,hyperlinks,cstyle,extkeys,focus,mouse,overline,progressbar,RGB,strikethrough,title,usstyle"
        );
        assert_eq!(
            list("XTerm"),
            "256,bpaste,ccolour,clipboard,cstyle,extkeys,focus,mouse,RGB,strikethrough,title"
        );
        assert_eq!(
            list("rxvt-unicode"),
            "256,bpaste,ccolour,cstyle,ignorefkeys,mouse,title"
        );
        assert_eq!(terminal_default_features("Konsole"), "");
        assert_eq!(
            terminal_colour_count(
                "xterm",
                "",
                terminal_feature_mask([terminal_default_features("tmux")])
            ),
            16_777_216
        );
        assert_eq!(
            terminal_colour_count(
                "xterm",
                "",
                terminal_feature_mask([terminal_default_features("rxvt-unicode")])
            ),
            256
        );
    }

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
