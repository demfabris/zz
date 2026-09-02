//! The per-client terminal `#{I/c:}` and `#{I/f:}` interrogate.
//!
//! tmux builds one `struct tty_term` per attached client from the terminfo
//! entry its `TERM` names, then writes more capabilities into it from whichever
//! features `tty_term_create` turned on. `tty_term_has_name` and
//! `tty_feature_present` read that object, so neither answer can come from a
//! compiled terminfo entry alone and neither can come from the feature list a
//! multiplexer publishes about its own renderers.
//!
//! The capability list arrives here the way `tty_term_read_list` produces it,
//! as `name=value` strings, because reading the terminfo database is I/O and
//! belongs to the daemon; everything after that is the pin's own arithmetic.
//! `TERM_NOAM` is left out of the flag set because the pin sets it on a resize
//! rather than in `tty_term_create`, so no fresh interrogate can see it.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::fnmatch;

const TERM_256_COLOURS: u32 = 0x1;
const TERM_DECSLRM: u32 = 0x4;
const TERM_DECFRA: u32 = 0x8;
const TERM_RGB_COLOURS: u32 = 0x10;
const TERM_VT100_LIKE: u32 = 0x20;
const TERM_SIXEL: u32 = 0x40;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CapabilityKind {
    Text,
    Number,
    Flag,
}

struct TtyFeature {
    name: &'static str,
    capabilities: &'static [&'static str],
    flags: u32,
}

/// tty-term.c `tty_term_codes`: the 233 capability names tmux reads for a
/// client terminal, with the type it reads each one as. `tty_term_has_name`
/// answers 0 for every name outside this table, whatever the terminfo entry
/// carries.
const TTY_TERM_CODES: [(&str, CapabilityKind); 233] = [
    ("acsc", CapabilityKind::Text),
    ("am", CapabilityKind::Flag),
    ("AX", CapabilityKind::Flag),
    ("bce", CapabilityKind::Flag),
    ("bel", CapabilityKind::Text),
    ("Bidi", CapabilityKind::Text),
    ("blink", CapabilityKind::Text),
    ("bold", CapabilityKind::Text),
    ("civis", CapabilityKind::Text),
    ("clear", CapabilityKind::Text),
    ("Clmg", CapabilityKind::Text),
    ("Cmg", CapabilityKind::Text),
    ("cnorm", CapabilityKind::Text),
    ("colors", CapabilityKind::Number),
    ("Cr", CapabilityKind::Text),
    ("csr", CapabilityKind::Text),
    ("Cs", CapabilityKind::Text),
    ("cub1", CapabilityKind::Text),
    ("cub", CapabilityKind::Text),
    ("cud1", CapabilityKind::Text),
    ("cud", CapabilityKind::Text),
    ("cuf1", CapabilityKind::Text),
    ("cuf", CapabilityKind::Text),
    ("cup", CapabilityKind::Text),
    ("cuu1", CapabilityKind::Text),
    ("cuu", CapabilityKind::Text),
    ("cvvis", CapabilityKind::Text),
    ("dch1", CapabilityKind::Text),
    ("dch", CapabilityKind::Text),
    ("dim", CapabilityKind::Text),
    ("dl1", CapabilityKind::Text),
    ("dl", CapabilityKind::Text),
    ("Dseks", CapabilityKind::Text),
    ("Dsfcs", CapabilityKind::Text),
    ("Dsbp", CapabilityKind::Text),
    ("Dsmg", CapabilityKind::Text),
    ("E3", CapabilityKind::Text),
    ("ech", CapabilityKind::Text),
    ("ed", CapabilityKind::Text),
    ("el1", CapabilityKind::Text),
    ("el", CapabilityKind::Text),
    ("enacs", CapabilityKind::Text),
    ("Enbp", CapabilityKind::Text),
    ("Eneks", CapabilityKind::Text),
    ("Enfcs", CapabilityKind::Text),
    ("Enmg", CapabilityKind::Text),
    ("fsl", CapabilityKind::Text),
    ("Hls", CapabilityKind::Text),
    ("home", CapabilityKind::Text),
    ("hpa", CapabilityKind::Text),
    ("ich1", CapabilityKind::Text),
    ("ich", CapabilityKind::Text),
    ("il1", CapabilityKind::Text),
    ("il", CapabilityKind::Text),
    ("indn", CapabilityKind::Text),
    ("invis", CapabilityKind::Text),
    ("kcbt", CapabilityKind::Text),
    ("kcub1", CapabilityKind::Text),
    ("kcud1", CapabilityKind::Text),
    ("kcuf1", CapabilityKind::Text),
    ("kcuu1", CapabilityKind::Text),
    ("kDC", CapabilityKind::Text),
    ("kDC3", CapabilityKind::Text),
    ("kDC4", CapabilityKind::Text),
    ("kDC5", CapabilityKind::Text),
    ("kDC6", CapabilityKind::Text),
    ("kDC7", CapabilityKind::Text),
    ("kdch1", CapabilityKind::Text),
    ("kDN", CapabilityKind::Text),
    ("kDN3", CapabilityKind::Text),
    ("kDN4", CapabilityKind::Text),
    ("kDN5", CapabilityKind::Text),
    ("kDN6", CapabilityKind::Text),
    ("kDN7", CapabilityKind::Text),
    ("kEND", CapabilityKind::Text),
    ("kEND3", CapabilityKind::Text),
    ("kEND4", CapabilityKind::Text),
    ("kEND5", CapabilityKind::Text),
    ("kEND6", CapabilityKind::Text),
    ("kEND7", CapabilityKind::Text),
    ("kend", CapabilityKind::Text),
    ("kf10", CapabilityKind::Text),
    ("kf11", CapabilityKind::Text),
    ("kf12", CapabilityKind::Text),
    ("kf13", CapabilityKind::Text),
    ("kf14", CapabilityKind::Text),
    ("kf15", CapabilityKind::Text),
    ("kf16", CapabilityKind::Text),
    ("kf17", CapabilityKind::Text),
    ("kf18", CapabilityKind::Text),
    ("kf19", CapabilityKind::Text),
    ("kf1", CapabilityKind::Text),
    ("kf20", CapabilityKind::Text),
    ("kf21", CapabilityKind::Text),
    ("kf22", CapabilityKind::Text),
    ("kf23", CapabilityKind::Text),
    ("kf24", CapabilityKind::Text),
    ("kf25", CapabilityKind::Text),
    ("kf26", CapabilityKind::Text),
    ("kf27", CapabilityKind::Text),
    ("kf28", CapabilityKind::Text),
    ("kf29", CapabilityKind::Text),
    ("kf2", CapabilityKind::Text),
    ("kf30", CapabilityKind::Text),
    ("kf31", CapabilityKind::Text),
    ("kf32", CapabilityKind::Text),
    ("kf33", CapabilityKind::Text),
    ("kf34", CapabilityKind::Text),
    ("kf35", CapabilityKind::Text),
    ("kf36", CapabilityKind::Text),
    ("kf37", CapabilityKind::Text),
    ("kf38", CapabilityKind::Text),
    ("kf39", CapabilityKind::Text),
    ("kf3", CapabilityKind::Text),
    ("kf40", CapabilityKind::Text),
    ("kf41", CapabilityKind::Text),
    ("kf42", CapabilityKind::Text),
    ("kf43", CapabilityKind::Text),
    ("kf44", CapabilityKind::Text),
    ("kf45", CapabilityKind::Text),
    ("kf46", CapabilityKind::Text),
    ("kf47", CapabilityKind::Text),
    ("kf48", CapabilityKind::Text),
    ("kf49", CapabilityKind::Text),
    ("kf4", CapabilityKind::Text),
    ("kf50", CapabilityKind::Text),
    ("kf51", CapabilityKind::Text),
    ("kf52", CapabilityKind::Text),
    ("kf53", CapabilityKind::Text),
    ("kf54", CapabilityKind::Text),
    ("kf55", CapabilityKind::Text),
    ("kf56", CapabilityKind::Text),
    ("kf57", CapabilityKind::Text),
    ("kf58", CapabilityKind::Text),
    ("kf59", CapabilityKind::Text),
    ("kf5", CapabilityKind::Text),
    ("kf60", CapabilityKind::Text),
    ("kf61", CapabilityKind::Text),
    ("kf62", CapabilityKind::Text),
    ("kf63", CapabilityKind::Text),
    ("kf6", CapabilityKind::Text),
    ("kf7", CapabilityKind::Text),
    ("kf8", CapabilityKind::Text),
    ("kf9", CapabilityKind::Text),
    ("kHOM", CapabilityKind::Text),
    ("kHOM3", CapabilityKind::Text),
    ("kHOM4", CapabilityKind::Text),
    ("kHOM5", CapabilityKind::Text),
    ("kHOM6", CapabilityKind::Text),
    ("kHOM7", CapabilityKind::Text),
    ("khome", CapabilityKind::Text),
    ("kIC", CapabilityKind::Text),
    ("kIC3", CapabilityKind::Text),
    ("kIC4", CapabilityKind::Text),
    ("kIC5", CapabilityKind::Text),
    ("kIC6", CapabilityKind::Text),
    ("kIC7", CapabilityKind::Text),
    ("kich1", CapabilityKind::Text),
    ("kind", CapabilityKind::Text),
    ("kLFT", CapabilityKind::Text),
    ("kLFT3", CapabilityKind::Text),
    ("kLFT4", CapabilityKind::Text),
    ("kLFT5", CapabilityKind::Text),
    ("kLFT6", CapabilityKind::Text),
    ("kLFT7", CapabilityKind::Text),
    ("kmous", CapabilityKind::Text),
    ("knp", CapabilityKind::Text),
    ("kNXT", CapabilityKind::Text),
    ("kNXT3", CapabilityKind::Text),
    ("kNXT4", CapabilityKind::Text),
    ("kNXT5", CapabilityKind::Text),
    ("kNXT6", CapabilityKind::Text),
    ("kNXT7", CapabilityKind::Text),
    ("kpp", CapabilityKind::Text),
    ("kPRV", CapabilityKind::Text),
    ("kPRV3", CapabilityKind::Text),
    ("kPRV4", CapabilityKind::Text),
    ("kPRV5", CapabilityKind::Text),
    ("kPRV6", CapabilityKind::Text),
    ("kPRV7", CapabilityKind::Text),
    ("kRIT", CapabilityKind::Text),
    ("kRIT3", CapabilityKind::Text),
    ("kRIT4", CapabilityKind::Text),
    ("kRIT5", CapabilityKind::Text),
    ("kRIT6", CapabilityKind::Text),
    ("kRIT7", CapabilityKind::Text),
    ("kri", CapabilityKind::Text),
    ("kUP", CapabilityKind::Text),
    ("kUP3", CapabilityKind::Text),
    ("kUP4", CapabilityKind::Text),
    ("kUP5", CapabilityKind::Text),
    ("kUP6", CapabilityKind::Text),
    ("kUP7", CapabilityKind::Text),
    ("Ms", CapabilityKind::Text),
    ("Nobr", CapabilityKind::Text),
    ("ol", CapabilityKind::Text),
    ("op", CapabilityKind::Text),
    ("Rect", CapabilityKind::Text),
    ("rev", CapabilityKind::Text),
    ("RGB", CapabilityKind::Flag),
    ("rin", CapabilityKind::Text),
    ("ri", CapabilityKind::Text),
    ("rmacs", CapabilityKind::Text),
    ("rmcup", CapabilityKind::Text),
    ("rmkx", CapabilityKind::Text),
    ("setab", CapabilityKind::Text),
    ("setaf", CapabilityKind::Text),
    ("setal", CapabilityKind::Text),
    ("setrgbb", CapabilityKind::Text),
    ("setrgbf", CapabilityKind::Text),
    ("Setulc", CapabilityKind::Text),
    ("Setulc1", CapabilityKind::Text),
    ("Se", CapabilityKind::Text),
    ("Sxl", CapabilityKind::Flag),
    ("sgr0", CapabilityKind::Text),
    ("sitm", CapabilityKind::Text),
    ("smacs", CapabilityKind::Text),
    ("smcup", CapabilityKind::Text),
    ("smkx", CapabilityKind::Text),
    ("Smol", CapabilityKind::Text),
    ("smso", CapabilityKind::Text),
    ("Smulx", CapabilityKind::Text),
    ("smul", CapabilityKind::Text),
    ("smxx", CapabilityKind::Text),
    ("Spb", CapabilityKind::Text),
    ("Ss", CapabilityKind::Text),
    ("Swd", CapabilityKind::Text),
    ("Sync", CapabilityKind::Text),
    ("Tc", CapabilityKind::Flag),
    ("tsl", CapabilityKind::Text),
    ("U8", CapabilityKind::Number),
    ("vpa", CapabilityKind::Text),
    ("XT", CapabilityKind::Flag),
];

/// tty-features.c `tty_features`, in declaration order. Each row is the
/// feature name, the capabilities `tty_apply_features` writes into the term
/// when the feature is on, and the term flags it sets.
const TTY_FEATURES: [TtyFeature; 21] = [
    TtyFeature {
        name: "256",
        capabilities: &["AX", "setab", "setaf"],
        flags: TERM_256_COLOURS,
    },
    TtyFeature {
        name: "bpaste",
        capabilities: &["Enbp", "Dsbp"],
        flags: 0,
    },
    TtyFeature {
        name: "ccolour",
        capabilities: &["Cs", "Cr"],
        flags: 0,
    },
    TtyFeature {
        name: "clipboard",
        capabilities: &["Ms"],
        flags: 0,
    },
    TtyFeature {
        name: "hyperlinks",
        capabilities: &["Hls"],
        flags: 0,
    },
    TtyFeature {
        name: "cstyle",
        capabilities: &["Ss", "Se"],
        flags: 0,
    },
    TtyFeature {
        name: "extkeys",
        capabilities: &["Eneks", "Dseks"],
        flags: 0,
    },
    TtyFeature {
        name: "focus",
        capabilities: &["Enfcs", "Dsfcs"],
        flags: 0,
    },
    TtyFeature {
        name: "ignorefkeys",
        capabilities: &[
            "kf0@", "kf1@", "kf2@", "kf3@", "kf4@", "kf5@", "kf6@", "kf7@", "kf8@", "kf9@",
            "kf10@", "kf11@", "kf12@", "kf13@", "kf14@", "kf15@", "kf16@", "kf17@", "kf18@",
            "kf19@", "kf20@", "kf21@", "kf22@", "kf23@", "kf24@", "kf25@", "kf26@", "kf27@",
            "kf28@", "kf29@", "kf30@", "kf31@", "kf32@", "kf33@", "kf34@", "kf35@", "kf36@",
            "kf37@", "kf38@", "kf39@", "kf40@", "kf41@", "kf42@", "kf43@", "kf44@", "kf45@",
            "kf46@", "kf47@", "kf48@", "kf49@", "kf50@", "kf51@", "kf52@", "kf53@", "kf54@",
            "kf55@", "kf56@", "kf57@", "kf58@", "kf59@", "kf60@", "kf61@", "kf62@", "kf63@",
        ],
        flags: 0,
    },
    TtyFeature {
        name: "margins",
        capabilities: &["Enmg", "Dsmg", "Clmg", "Cmg"],
        flags: TERM_DECSLRM,
    },
    TtyFeature {
        name: "mouse",
        capabilities: &["kmous"],
        flags: 0,
    },
    TtyFeature {
        name: "osc7",
        capabilities: &["Swd", "fsl"],
        flags: 0,
    },
    TtyFeature {
        name: "overline",
        capabilities: &["Smol"],
        flags: 0,
    },
    TtyFeature {
        name: "progressbar",
        capabilities: &["Spb"],
        flags: 0,
    },
    TtyFeature {
        name: "rectfill",
        capabilities: &["Rect"],
        flags: TERM_DECFRA,
    },
    TtyFeature {
        name: "RGB",
        capabilities: &["AX", "setrgbf", "setrgbb", "setab", "setaf"],
        flags: TERM_256_COLOURS | TERM_RGB_COLOURS,
    },
    TtyFeature {
        name: "sixel",
        capabilities: &["Sxl"],
        flags: TERM_SIXEL,
    },
    TtyFeature {
        name: "strikethrough",
        capabilities: &["smxx"],
        flags: 0,
    },
    TtyFeature {
        name: "sync",
        capabilities: &["Sync"],
        flags: 0,
    },
    TtyFeature {
        name: "title",
        capabilities: &["tsl", "fsl"],
        flags: 0,
    },
    TtyFeature {
        name: "usstyle",
        capabilities: &["Smulx", "Setulc", "Setulc1", "ol"],
        flags: 0,
    },
];

/// `tigetflag` answers 0, not -1, for a standard boolean the entry omits, so
/// `tty_term_read_list` stores it anyway and `tty_term_has` then answers 1.
/// Only these two of the seven flag capabilities are standard booleans; the
/// rest are tmux extensions that reach the term only from the entry's own
/// extended section.
const STANDARD_BOOLEAN_CAPABILITIES: [&str; 2] = ["am", "bce"];

/// One client's `struct tty_term`, reduced to the questions `#{I}` asks:
/// `tty_term_has_name` for `I/c:` and `tty_feature_present` for `I/f:`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TtyTerm {
    codes: BTreeMap<&'static str, CodeValue>,
    features: BTreeSet<String>,
    flags: u32,
}

/// One `struct tty_code`: the value `tty_term_string`, `tty_term_number` and
/// `tty_term_flag` read, present only while the code's type is not
/// `TTYCODE_NONE`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum CodeValue {
    Text(String),
    Number(i32),
    Flag(bool),
}

impl TtyTerm {
    /// `tty_term_create` for a client whose terminfo entry produced `entries`,
    /// each a `name=value` string the way `tty_term_read_list` writes them,
    /// with the global `terminal-features` and `terminal-overrides` arrays.
    #[must_use]
    pub fn create(
        term_name: &str,
        entries: &[String],
        colour_term: Option<&str>,
        terminal_features: &[String],
        terminal_overrides: &[String],
    ) -> Self {
        let mut term = Self::default();
        for entry in entries {
            let Some((name, value)) = entry.split_once('=') else {
                continue;
            };
            let Some((known, kind)) = code_entry(name) else {
                continue;
            };
            let code = match kind {
                CapabilityKind::Text => Some(CodeValue::Text(strip_padding(value))),
                CapabilityKind::Number => strtonum(value).map(CodeValue::Number),
                CapabilityKind::Flag => Some(CodeValue::Flag(value.starts_with('1'))),
            };
            match code {
                Some(code) => {
                    term.codes.insert(known, code);
                }
                None => {
                    term.codes.remove(known);
                }
            }
        }
        for name in STANDARD_BOOLEAN_CAPABILITIES {
            term.codes.entry(name).or_insert(CodeValue::Flag(false));
        }

        let mut requested = BTreeSet::new();
        for value in terminal_features {
            let mut offset = 0;
            let Some(first) = override_next(value, &mut offset) else {
                continue;
            };
            if fnmatch(&first, term_name) {
                add_features(&mut requested, &value[offset..], ':');
            }
        }
        if let Some(colour_term) = colour_term {
            if colour_term.eq_ignore_ascii_case("truecolor")
                || colour_term.eq_ignore_ascii_case("24bit")
            {
                add_features(&mut requested, "RGB", ',');
            } else if colour_term.contains("256") {
                add_features(&mut requested, "256", ',');
            }
        }

        term.apply_overrides(term_name, terminal_overrides);

        if term.flag("XT") || term.string("clear").starts_with("\u{1b}[") {
            term.flags |= TERM_VT100_LIKE;
            add_features(&mut requested, "bpaste,focus,title", ',');
        }
        if (term.flag("Tc") || term.has("RGB")) && (!term.has("setrgbf") || !term.has("setrgbb")) {
            add_features(&mut requested, "RGB", ',');
        }

        if !requested.is_empty() {
            for feature in &TTY_FEATURES {
                if !requested.contains(feature.name) {
                    continue;
                }
                for capability in feature.capabilities {
                    term.apply(capability);
                }
                term.flags |= feature.flags;
            }
            term.apply_overrides(term_name, terminal_overrides);
        }

        term.features = TTY_FEATURES
            .iter()
            .map(|feature| feature.name)
            .filter(|name| term.feature_present(name, &requested))
            .map(str::to_owned)
            .collect();
        term
    }

    /// `tty_term_has_name`.
    #[must_use]
    pub fn has_capability(&self, name: &str) -> bool {
        self.has(name)
    }

    /// `tty_feature_present`.
    #[must_use]
    pub fn has_feature(&self, name: &str) -> bool {
        self.features.contains(name)
    }

    /// The features the pin would list for this client, which is what its own
    /// `client_termfeatures` answers. zz publishes its renderer roster under
    /// that name instead, so this exists only to check the port.
    #[cfg(test)]
    fn feature_names(&self) -> impl Iterator<Item = &str> {
        self.features.iter().map(String::as_str)
    }

    fn has(&self, name: &str) -> bool {
        self.codes.contains_key(name)
    }

    fn flag(&self, name: &str) -> bool {
        matches!(self.codes.get(name), Some(CodeValue::Flag(true)))
    }

    fn string(&self, name: &str) -> &str {
        match self.codes.get(name) {
            Some(CodeValue::Text(value)) => value,
            _ => "",
        }
    }

    /// `tty_term_apply`: one `:`-separated capability list, where `name@`
    /// removes the code, `name=value` sets it and a bare `name` sets a flag;
    /// names outside `tty_term_codes` are ignored.
    fn apply(&mut self, capabilities: &str) {
        let mut offset = 0;
        while let Some(item) = override_next(capabilities, &mut offset) {
            if item.is_empty() {
                continue;
            }
            let (name, value) = if let Some((name, value)) = item.split_once('=') {
                (name, Some(unvis(value)))
            } else if let Some(name) = item.strip_suffix('@') {
                (name, None)
            } else {
                (item.as_str(), Some(String::new()))
            };
            let Some((known, kind)) = code_entry(name) else {
                continue;
            };
            let Some(value) = value else {
                self.codes.remove(known);
                continue;
            };
            match kind {
                CapabilityKind::Text => {
                    self.codes.insert(known, CodeValue::Text(value));
                }
                CapabilityKind::Number => {
                    if let Some(number) = strtonum(&value) {
                        self.codes.insert(known, CodeValue::Number(number));
                    }
                }
                CapabilityKind::Flag => {
                    self.codes.insert(known, CodeValue::Flag(true));
                }
            }
        }
    }

    /// `tty_term_apply_overrides`: every `terminal-overrides` row whose first
    /// item matches the terminal name, then the term flags the capabilities
    /// decide. `TERM_NOAM` is left out because the pin sets it on a resize
    /// rather than here, so no fresh interrogate can see it.
    fn apply_overrides(&mut self, term_name: &str, terminal_overrides: &[String]) {
        for row in terminal_overrides {
            let mut offset = 0;
            let Some(first) = override_next(row, &mut offset) else {
                continue;
            };
            if fnmatch(&first, term_name) {
                self.apply(&row[offset..]);
            }
        }
        for (flag, present) in [
            (TERM_RGB_COLOURS, self.has("setrgbf") && self.has("setrgbb")),
            (TERM_DECSLRM, self.has("Cmg") && self.has("Clmg")),
            (TERM_DECFRA, self.has("Rect")),
        ] {
            if present {
                self.flags |= flag;
            } else {
                self.flags &= !flag;
            }
        }
    }

    /// `tty_feature_present`: the feature bit, else every capability the
    /// feature names being on the term with the feature's own term flags
    /// already set. `ignorefkeys` is excluded by name because its capabilities
    /// are cancels.
    fn feature_present(&self, name: &str, requested: &BTreeSet<String>) -> bool {
        let Some(feature) = TTY_FEATURES.iter().find(|feature| feature.name == name) else {
            return false;
        };
        if requested.contains(name) {
            return true;
        }
        if name == "ignorefkeys" {
            return false;
        }
        if feature.flags != 0 && (self.flags & feature.flags) != feature.flags {
            return false;
        }
        feature.capabilities.iter().all(|capability| {
            let name = capability
                .split_once('=')
                .map_or(*capability, |(name, _)| name);
            self.has(name)
        })
    }
}

fn code_entry(name: &str) -> Option<(&'static str, CapabilityKind)> {
    TTY_TERM_CODES
        .iter()
        .find(|(known, _)| *known == name)
        .copied()
}

/// `strtonum(value, 0, INT_MAX, ...)`: a decimal number in range, else nothing.
fn strtonum(value: &str) -> Option<i32> {
    value
        .trim_start()
        .parse::<i32>()
        .ok()
        .filter(|number| *number >= 0)
}

/// `tty_term_strip`: the `$<n>` padding a terminfo string carries.
fn strip_padding(value: &str) -> String {
    if !value.contains('$') {
        return value.to_owned();
    }
    let mut stripped = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find("$<") {
        stripped.push_str(&rest[..index]);
        rest = &rest[index..];
        match rest.find('>') {
            Some(close) => rest = &rest[close + 1..],
            None => return stripped,
        }
    }
    stripped.push_str(rest);
    stripped
}

/// `tty_term_override_next`: the next `:`-separated item of an override or
/// feature row, with `::` standing for a literal colon. `offset` moves past
/// the separator so the caller can hand the remainder on.
fn override_next(row: &str, offset: &mut usize) -> Option<String> {
    let bytes = row.as_bytes();
    let mut at = *offset;
    if at >= bytes.len() {
        return None;
    }
    let mut value = Vec::new();
    while at < bytes.len() {
        if bytes[at] == b':' {
            if bytes.get(at + 1) == Some(&b':') {
                value.push(b':');
                at += 2;
            } else {
                break;
            }
        } else {
            value.push(bytes[at]);
            at += 1;
        }
    }
    *offset = if at < bytes.len() { at + 1 } else { at };
    Some(String::from_utf8_lossy(&value).into_owned())
}

/// `strunvis` over an override value, the way `tty_term_apply` decodes it;
/// a sequence unvis rejects leaves the value as written, as the pin does.
fn unvis(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let Some(&next) = bytes.get(index) else {
            return value.to_owned();
        };
        index += 1;
        let decoded = match next {
            b'\\' => b'\\',
            b'n' => b'\n',
            b'r' => b'\r',
            b'b' => 0x08,
            b'a' => 0x07,
            b'v' => 0x0b,
            b't' => b'\t',
            b'f' => 0x0c,
            b's' => b' ',
            b'E' => 0x1b,
            b'\n' | b'$' => continue,
            b'0'..=b'7' => {
                let mut number = u32::from(next - b'0');
                for _ in 0..2 {
                    match bytes.get(index) {
                        Some(digit @ b'0'..=b'7') => {
                            number = number * 8 + u32::from(digit - b'0');
                            index += 1;
                        }
                        _ => break,
                    }
                }
                (number & 0xff) as u8
            }
            b'^' => {
                let Some(&control) = bytes.get(index) else {
                    return value.to_owned();
                };
                index += 1;
                if control == b'?' {
                    0x7f
                } else {
                    control & 0x1f
                }
            }
            b'M' => {
                let meta = match bytes.get(index) {
                    Some(b'-') => bytes.get(index + 1).copied().map(|c| c | 0x80),
                    Some(b'^') => bytes
                        .get(index + 1)
                        .copied()
                        .map(|c| if c == b'?' { 0xff } else { (c & 0x1f) | 0x80 }),
                    _ => None,
                };
                let Some(meta) = meta else {
                    return value.to_owned();
                };
                index += 2;
                meta
            }
            _ => return value.to_owned(),
        };
        out.push(decoded);
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `tty_add_features`: a case-insensitive name match against the feature table,
/// stopping at the first name the table does not carry.
fn add_features(requested: &mut BTreeSet<String>, value: &str, separator: char) {
    for name in value.split(separator) {
        let Some(feature) = TTY_FEATURES
            .iter()
            .find(|feature| feature.name.eq_ignore_ascii_case(name))
        else {
            break;
        };
        requested.insert(feature.name.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `name=value` string `tty_term_read_list` produces for
    /// TERM=xterm-256color on the box the pin was measured on, keeping only the
    /// values the build actually reads: `clear`, whose CSI start decides
    /// `TERM_VT100LIKE`, and the numbers, which have to parse.
    const XTERM_256COLOR: [&str; 199] = [
        "acsc=x",
        "am=1",
        "AX=1",
        "bce=1",
        "bel=x",
        "blink=x",
        "bold=x",
        "civis=x",
        "clear=\u{1b}[H\u{1b}[2J",
        "cnorm=x",
        "colors=256",
        "Cr=x",
        "csr=x",
        "Cs=x",
        "cub1=x",
        "cub=x",
        "cud1=x",
        "cud=x",
        "cuf1=x",
        "cuf=x",
        "cup=x",
        "cuu1=x",
        "cuu=x",
        "cvvis=x",
        "dch1=x",
        "dch=x",
        "dim=x",
        "dl1=x",
        "dl=x",
        "E3=x",
        "ech=x",
        "ed=x",
        "el1=x",
        "el=x",
        "home=x",
        "hpa=x",
        "ich=x",
        "il1=x",
        "il=x",
        "indn=x",
        "invis=x",
        "kcbt=x",
        "kcub1=x",
        "kcud1=x",
        "kcuf1=x",
        "kcuu1=x",
        "kDC=x",
        "kDC3=x",
        "kDC4=x",
        "kDC5=x",
        "kDC6=x",
        "kDC7=x",
        "kdch1=x",
        "kDN=x",
        "kDN3=x",
        "kDN4=x",
        "kDN5=x",
        "kDN6=x",
        "kDN7=x",
        "kEND=x",
        "kEND3=x",
        "kEND4=x",
        "kEND5=x",
        "kEND6=x",
        "kEND7=x",
        "kend=x",
        "kf10=x",
        "kf11=x",
        "kf12=x",
        "kf13=x",
        "kf14=x",
        "kf15=x",
        "kf16=x",
        "kf17=x",
        "kf18=x",
        "kf19=x",
        "kf1=x",
        "kf20=x",
        "kf21=x",
        "kf22=x",
        "kf23=x",
        "kf24=x",
        "kf25=x",
        "kf26=x",
        "kf27=x",
        "kf28=x",
        "kf29=x",
        "kf2=x",
        "kf30=x",
        "kf31=x",
        "kf32=x",
        "kf33=x",
        "kf34=x",
        "kf35=x",
        "kf36=x",
        "kf37=x",
        "kf38=x",
        "kf39=x",
        "kf3=x",
        "kf40=x",
        "kf41=x",
        "kf42=x",
        "kf43=x",
        "kf44=x",
        "kf45=x",
        "kf46=x",
        "kf47=x",
        "kf48=x",
        "kf49=x",
        "kf4=x",
        "kf50=x",
        "kf51=x",
        "kf52=x",
        "kf53=x",
        "kf54=x",
        "kf55=x",
        "kf56=x",
        "kf57=x",
        "kf58=x",
        "kf59=x",
        "kf5=x",
        "kf60=x",
        "kf61=x",
        "kf62=x",
        "kf63=x",
        "kf6=x",
        "kf7=x",
        "kf8=x",
        "kf9=x",
        "kHOM=x",
        "kHOM3=x",
        "kHOM4=x",
        "kHOM5=x",
        "kHOM6=x",
        "kHOM7=x",
        "khome=x",
        "kIC=x",
        "kIC3=x",
        "kIC4=x",
        "kIC5=x",
        "kIC6=x",
        "kIC7=x",
        "kich1=x",
        "kind=x",
        "kLFT=x",
        "kLFT3=x",
        "kLFT4=x",
        "kLFT5=x",
        "kLFT6=x",
        "kLFT7=x",
        "kmous=x",
        "knp=x",
        "kNXT=x",
        "kNXT3=x",
        "kNXT4=x",
        "kNXT5=x",
        "kNXT6=x",
        "kNXT7=x",
        "kpp=x",
        "kPRV=x",
        "kPRV3=x",
        "kPRV4=x",
        "kPRV5=x",
        "kPRV6=x",
        "kPRV7=x",
        "kRIT=x",
        "kRIT3=x",
        "kRIT4=x",
        "kRIT5=x",
        "kRIT6=x",
        "kRIT7=x",
        "kri=x",
        "kUP=x",
        "kUP3=x",
        "kUP4=x",
        "kUP5=x",
        "kUP6=x",
        "kUP7=x",
        "Ms=x",
        "op=x",
        "rev=x",
        "rin=x",
        "ri=x",
        "rmacs=x",
        "rmcup=x",
        "rmkx=x",
        "setab=x",
        "setaf=x",
        "Se=x",
        "sgr0=x",
        "sitm=x",
        "smacs=x",
        "smcup=x",
        "smkx=x",
        "smso=x",
        "smul=x",
        "Ss=x",
        "vpa=x",
        "XT=1",
    ];

    fn xterm_256color_entries() -> Vec<String> {
        XTERM_256COLOR
            .iter()
            .map(|entry| (*entry).to_owned())
            .collect()
    }

    fn stock_terminal_features() -> Vec<String> {
        [
            "xterm*:clipboard:ccolour:cstyle:focus:title",
            "screen*:title",
            "rxvt*:ignorefkeys",
        ]
        .iter()
        .map(|value| (*value).to_owned())
        .collect()
    }

    fn stock_terminal_overrides() -> Vec<String> {
        vec!["linux*:AX@".to_owned()]
    }

    /// The default `terminal-features` and `terminal-overrides` arrays reach
    /// the interrogate through the engine accessors, so the stock lists these
    /// tests hard-code are the ones `tty_term_create` sees.
    #[test]
    fn the_stock_terminal_features_option_reaches_the_interrogate() {
        let engine = crate::MuxEngine::default();
        assert_eq!(engine.terminal_features_option(), stock_terminal_features());
    }

    #[test]
    fn the_stock_terminal_overrides_option_reaches_the_interrogate() {
        let engine = crate::MuxEngine::default();
        assert_eq!(
            engine.terminal_overrides_option(),
            stock_terminal_overrides()
        );
    }

    fn entries(list: &[&str]) -> Vec<String> {
        list.iter().map(|entry| (*entry).to_owned()).collect()
    }

    /// Measured against pinned tmux on 2026-09-02 with COLORTERM unset and a
    /// pty client per TERM: screen-256color and tmux-256color carry no XT but
    /// a clear starting with CSI, so `tty_term_create` still adds bpaste, focus
    /// and title; linux gets title the same way and loses AX to the stock
    /// `linux*:AX@` override; rxvt loses kf1 to the `ignorefkeys` cancels.
    #[test]
    fn a_vt100_like_clear_adds_the_three_features_without_xt() {
        let screen = TtyTerm::create(
            "screen-256color",
            &entries(&["clear=\u{1b}[H\u{1b}[J", "cup=x", "kf1=x"]),
            None,
            &stock_terminal_features(),
            &stock_terminal_overrides(),
        );
        assert!(screen.has_feature("bpaste"));
        assert!(screen.has_feature("focus"));
        assert!(screen.has_feature("title"));
        assert!(screen.has_capability("Enfcs"));
        assert!(screen.has_capability("kf1"));
        assert!(!screen.has_capability("XT"));

        let linux = TtyTerm::create(
            "linux",
            &entries(&["clear=\u{1b}[H\u{1b}[J", "cup=x", "AX=1", "kf1=x"]),
            None,
            &stock_terminal_features(),
            &stock_terminal_overrides(),
        );
        assert!(linux.has_feature("title"));
        assert!(!linux.has_capability("AX"));
        assert!(linux.has_capability("kf1"));

        let rxvt = TtyTerm::create(
            "rxvt",
            &entries(&["clear=\u{1b}[H\u{1b}[2J", "cup=x", "kf1=x", "kf63=x"]),
            None,
            &stock_terminal_features(),
            &stock_terminal_overrides(),
        );
        assert!(!rxvt.has_capability("kf1"));
        assert!(!rxvt.has_capability("kf63"));
        assert!(rxvt.has_feature("ignorefkeys"));
    }

    /// Measured on the pin with `set -g terminal-overrides 'xterm*:Tc'`:
    /// `#{I/c:Tc}` answers 1, and because Tc is a flag the RGB feature follows
    /// with setrgbf and setrgbb written in. The `Ss=\E[2 q:smcup@` row is read
    /// off `tty_term_apply`: the string lands and the cancel removes the code.
    #[test]
    fn terminal_overrides_apply_after_the_entry_and_after_the_features() {
        let term = TtyTerm::create(
            "xterm-256color",
            &xterm_256color_entries(),
            None,
            &stock_terminal_features(),
            &entries(&["linux*:AX@", "xterm*:Tc", "xterm*:Ss=\\E[2 q:smcup@"]),
        );
        assert!(term.has_capability("Tc"));
        assert!(term.has_feature("RGB"));
        assert!(term.has_capability("setrgbf"));
        assert!(term.has_capability("setrgbb"));
        assert!(term.has_capability("Ss"));
        assert_eq!(term.string("Ss"), "\u{1b}[2 q");
        assert!(!term.has_capability("smcup"));
        assert!(term.has_capability("AX"));

        let removed = TtyTerm::create(
            "xterm-256color",
            &xterm_256color_entries(),
            Some("truecolor"),
            &stock_terminal_features(),
            &entries(&["xterm*:setrgbf@"]),
        );
        assert!(!removed.has_capability("setrgbf"));
        assert!(removed.has_feature("RGB"));
        assert_eq!(removed.flags & TERM_RGB_COLOURS, 0);
    }

    #[test]
    fn override_rows_split_on_single_colons_and_unvis_their_values() {
        let mut offset = 0;
        assert_eq!(
            override_next("xterm*:Ss=\\E[2 q::x:smcup@", &mut offset).as_deref(),
            Some("xterm*")
        );
        assert_eq!(
            override_next("xterm*:Ss=\\E[2 q::x:smcup@", &mut offset).as_deref(),
            Some("Ss=\\E[2 q:x")
        );
        assert_eq!(
            override_next("xterm*:Ss=\\E[2 q::x:smcup@", &mut offset).as_deref(),
            Some("smcup@")
        );
        assert_eq!(
            override_next("xterm*:Ss=\\E[2 q::x:smcup@", &mut offset),
            None
        );
        assert_eq!(unvis("\\E[2 q\\\\\\s\\007\\^?"), "\u{1b}[2 q\\ \u{7}\u{7f}");
        assert_eq!(unvis("\\q"), "\\q");
        assert_eq!(strip_padding("\u{1b}[H$<5>\u{1b}[J"), "\u{1b}[H\u{1b}[J");
        assert_eq!(strtonum("256"), Some(256));
        assert_eq!(strtonum("0x100"), None);
        assert_eq!(strtonum("-1"), None);
    }

    /// Measured against pinned tmux on 2026-09-02 with an attached 80x24 pty
    /// client on TERM=xterm-256color and COLORTERM=truecolor, one
    /// `display-message -p -c <client>` per name.
    #[test]
    fn the_interrogate_matches_the_pinned_client_terminal() {
        let term = TtyTerm::create(
            "xterm-256color",
            &xterm_256color_entries(),
            Some("truecolor"),
            &stock_terminal_features(),
            &stock_terminal_overrides(),
        );
        for (name, present) in [
            ("am", true),
            ("AX", true),
            ("bce", true),
            ("RGB", false),
            ("Sxl", false),
            ("Tc", false),
            ("XT", true),
            ("colors", true),
            ("U8", false),
            ("smcup", true),
            ("Ss", true),
            ("Se", true),
            ("Sync", false),
            ("nosuchcap", false),
            ("hs", false),
            ("kmous", true),
            ("Ms", true),
            ("Cs", true),
            ("Cr", true),
            ("Enbp", true),
            ("Dsbp", true),
            ("Smulx", false),
            ("Setulc", false),
            ("ol", false),
            ("smxx", false),
            ("Smol", false),
            ("Hls", false),
            ("setrgbf", true),
            ("setrgbb", true),
            ("setab", true),
            ("setaf", true),
            ("tsl", true),
            ("fsl", true),
            ("Swd", false),
            ("Rect", false),
            ("Spb", false),
            ("Clmg", false),
            ("Cmg", false),
            ("Enmg", false),
            ("Dsmg", false),
            ("Eneks", false),
            ("Dseks", false),
            ("Enfcs", true),
            ("Dsfcs", true),
            ("kf0", false),
            ("kf1", true),
            ("kf63", true),
            ("acsc", true),
            ("clear", true),
            ("cup", true),
        ] {
            assert_eq!(term.has_capability(name), present, "I/c:{name}");
        }
        for (name, present) in [
            ("256", true),
            ("bpaste", true),
            ("ccolour", true),
            ("clipboard", true),
            ("hyperlinks", false),
            ("cstyle", true),
            ("extkeys", false),
            ("focus", true),
            ("ignorefkeys", false),
            ("margins", false),
            ("mouse", true),
            ("osc7", false),
            ("overline", false),
            ("progressbar", false),
            ("rectfill", false),
            ("RGB", true),
            ("sixel", false),
            ("strikethrough", false),
            ("sync", false),
            ("title", true),
            ("usstyle", false),
            ("nosuchfeature", false),
        ] {
            assert_eq!(term.has_feature(name), present, "I/f:{name}");
        }
        assert_eq!(
            term.feature_names().collect::<Vec<_>>(),
            [
                "256",
                "RGB",
                "bpaste",
                "ccolour",
                "clipboard",
                "cstyle",
                "focus",
                "mouse",
                "title"
            ]
        );
    }
}
