use crate::{TmuxColour, parse_tmux_colour};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TmuxAttributeState {
    Off,
    On,
    #[default]
    Unset,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TmuxAttributes {
    pub acs: TmuxAttributeState,
    pub bold: TmuxAttributeState,
    pub dim: TmuxAttributeState,
    pub underscore: TmuxAttributeState,
    pub blink: TmuxAttributeState,
    pub reverse: TmuxAttributeState,
    pub hidden: TmuxAttributeState,
    pub italics: TmuxAttributeState,
    pub strikethrough: TmuxAttributeState,
    pub double_underscore: TmuxAttributeState,
    pub curly_underscore: TmuxAttributeState,
    pub dotted_underscore: TmuxAttributeState,
    pub dashed_underscore: TmuxAttributeState,
    pub overline: TmuxAttributeState,
    pub noattr: TmuxAttributeState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmuxAlign {
    Default,
    Left,
    Centre,
    Right,
    AbsoluteCentre,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmuxList {
    Off,
    On,
    Focus,
    LeftMarker,
    RightMarker,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TmuxRange {
    None,
    Left,
    Right,
    Control(u8),
    Pane(u32),
    Window(u32),
    Session(u32),
    User(String),
    Other {
        kind: String,
        argument: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmuxWidth {
    Cells(u32),
    Percentage(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmuxDefaultType {
    Push,
    Pop,
    Set,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TmuxStyle {
    pub fg: Option<TmuxColour>,
    pub bg: Option<TmuxColour>,
    pub us: Option<TmuxColour>,
    pub attributes: TmuxAttributes,
    pub align: Option<TmuxAlign>,
    pub fill: Option<TmuxColour>,
    pub list: Option<TmuxList>,
    pub range: Option<TmuxRange>,
    pub width: Option<TmuxWidth>,
    pub pad: Option<u32>,
    pub default_type: Option<TmuxDefaultType>,
    pub dim_percentage: Option<u8>,
    pub ignore: Option<bool>,
    pub link: Option<String>,
    pub reset: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSegment {
    pub text: String,
    pub style: TmuxStyle,
}

pub fn parse_style(value: &str) -> Option<TmuxStyle> {
    let mut style = TmuxStyle::default();
    for token in value
        .split([' ', ',', '\n'])
        .filter(|token| !token.is_empty())
    {
        if token.len() > 255 || !parse_token(&mut style, token) {
            return None;
        }
    }
    Some(style)
}

pub fn valid_style(value: &str) -> bool {
    parse_style(value).is_some()
}

pub fn parse_styled_segments(value: &str) -> Vec<StyledSegment> {
    let bytes = value.as_bytes();
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut current = TmuxStyle::default();
    let mut base = TmuxStyle::default();
    let mut current_default = base.clone();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'#' {
            let character = value[index..]
                .chars()
                .next()
                .expect("index is on a character boundary");
            text.push(character);
            index += character.len_utf8();
            continue;
        }
        let start = index;
        while bytes.get(index) == Some(&b'#') {
            index += 1;
        }
        let hashes = index - start;
        if bytes.get(index) != Some(&b'[') {
            text.extend(std::iter::repeat_n('#', hashes.div_ceil(2)));
            continue;
        }
        text.extend(std::iter::repeat_n('#', hashes / 2));
        if hashes % 2 == 0 {
            text.push('[');
            index += 1;
            continue;
        }
        let marker_start = index + 1;
        let Some(relative_end) = value[marker_start..].find(']') else {
            break;
        };
        let marker_end = marker_start + relative_end;
        if !text.is_empty() {
            segments.push(StyledSegment {
                text: std::mem::take(&mut text),
                style: current.clone(),
            });
        }
        if let Some(delta) = parse_style(&value[marker_start..marker_end]) {
            let saved = current.clone();
            apply_style(&mut current, &delta, &current_default);
            match delta.default_type {
                Some(TmuxDefaultType::Push) => current_default = saved,
                Some(TmuxDefaultType::Pop) => current_default.clone_from(&base),
                Some(TmuxDefaultType::Set) => {
                    base.clone_from(&saved);
                    current_default = saved;
                }
                None => {}
            }
            current.default_type = None;
        }
        index = marker_end + 1;
    }
    if !text.is_empty() {
        segments.push(StyledSegment {
            text,
            style: current,
        });
    }
    segments
}

fn parse_token(style: &mut TmuxStyle, token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "default" => {
            style.fg = None;
            style.bg = None;
            style.us = None;
            style.attributes = TmuxAttributes::default();
            style.link = Some(String::new());
            style.reset = true;
            return true;
        }
        "none" => {
            set_all_attributes(&mut style.attributes, TmuxAttributeState::Off);
            return true;
        }
        "ignore" => {
            style.ignore = Some(true);
            return true;
        }
        "noignore" => {
            style.ignore = Some(false);
            return true;
        }
        "push-default" => {
            style.default_type = Some(TmuxDefaultType::Push);
            return true;
        }
        "pop-default" => {
            style.default_type = Some(TmuxDefaultType::Pop);
            return true;
        }
        "set-default" => {
            style.default_type = Some(TmuxDefaultType::Set);
            return true;
        }
        "nolist" => {
            style.list = Some(TmuxList::Off);
            return true;
        }
        "norange" => {
            style.range = Some(TmuxRange::None);
            return true;
        }
        "noalign" => {
            style.align = Some(TmuxAlign::Default);
            return true;
        }
        "nolink" => {
            style.link = Some(String::new());
            return true;
        }
        "noattr" => {
            style.attributes.noattr = TmuxAttributeState::On;
            return true;
        }
        _ => {}
    }
    if let Some(value) = lower.strip_prefix("fg=") {
        let Some(colour) = parse_tmux_colour(value) else {
            return false;
        };
        style.fg = Some(colour);
        return true;
    }
    if let Some(value) = lower.strip_prefix("bg=") {
        let Some(colour) = parse_tmux_colour(value) else {
            return false;
        };
        style.bg = Some(colour);
        return true;
    }
    if let Some(value) = lower.strip_prefix("us=") {
        let Some(colour) = parse_tmux_colour(value) else {
            return false;
        };
        style.us = Some(colour);
        return true;
    }
    if let Some(value) = lower.strip_prefix("fill=") {
        let Some(colour) = parse_tmux_colour(value) else {
            return false;
        };
        style.fill = Some(colour);
        return true;
    }
    if let Some(value) = lower.strip_prefix("align=") {
        style.align = Some(match value {
            "left" => TmuxAlign::Left,
            "centre" => TmuxAlign::Centre,
            "right" => TmuxAlign::Right,
            "absolute-centre" => TmuxAlign::AbsoluteCentre,
            _ => return false,
        });
        return true;
    }
    if let Some(value) = lower.strip_prefix("list=") {
        style.list = Some(match value {
            "on" => TmuxList::On,
            "focus" => TmuxList::Focus,
            "left-marker" => TmuxList::LeftMarker,
            "right-marker" => TmuxList::RightMarker,
            _ => return false,
        });
        return true;
    }
    if let Some(value) = lower.strip_prefix("range=") {
        let Some(range) = parse_range(value) else {
            return false;
        };
        style.range = Some(range);
        return true;
    }
    if let Some(value) = lower.strip_prefix("dim=") {
        let Some(percentage) = parse_percentage(value) else {
            return false;
        };
        style.dim_percentage = Some(percentage);
        return true;
    }
    if let Some(value) = lower.strip_prefix("width=") {
        let width = if let Some(value) = value.strip_suffix('%') {
            let Some(percentage) = parse_percentage(value) else {
                return false;
            };
            TmuxWidth::Percentage(percentage)
        } else {
            let Some(cells) = parse_unsigned(value) else {
                return false;
            };
            TmuxWidth::Cells(cells)
        };
        style.width = Some(width);
        return true;
    }
    if let Some(value) = lower.strip_prefix("pad=") {
        let Some(pad) = parse_unsigned(value) else {
            return false;
        };
        style.pad = Some(pad);
        return true;
    }
    if lower.starts_with("link=") {
        style.link = Some(token[5..].to_owned());
        return true;
    }
    if let Some(attributes) = lower.strip_prefix("no") {
        return parse_attributes(&mut style.attributes, attributes, TmuxAttributeState::Off);
    }
    parse_attributes(&mut style.attributes, &lower, TmuxAttributeState::On)
}

fn parse_range(value: &str) -> Option<TmuxRange> {
    let (kind, argument) = value
        .split_once('|')
        .map_or((value, None), |(kind, argument)| (kind, Some(argument)));
    if argument == Some("") {
        return None;
    }
    Some(match (kind, argument) {
        ("left", None) => TmuxRange::Left,
        ("right", None) => TmuxRange::Right,
        ("control", Some(argument)) => {
            TmuxRange::Control(argument.parse::<u8>().ok().filter(|value| *value <= 9)?)
        }
        ("pane", Some(argument)) => TmuxRange::Pane(parse_unsigned(argument.strip_prefix('%')?)?),
        ("window", Some(argument)) => TmuxRange::Window(parse_unsigned(argument)?),
        ("session", Some(argument)) => {
            TmuxRange::Session(parse_unsigned(argument.strip_prefix('$')?)?)
        }
        ("user", Some(argument)) => TmuxRange::User(argument.to_owned()),
        ("left" | "right", Some(_))
        | ("control" | "pane" | "window" | "session" | "user", None) => return None,
        (kind, argument) if !kind.is_empty() => TmuxRange::Other {
            kind: kind.to_owned(),
            argument: argument.map(str::to_owned),
        },
        _ => return None,
    })
}

fn parse_percentage(value: &str) -> Option<u8> {
    value
        .strip_suffix('%')
        .unwrap_or(value)
        .parse::<u8>()
        .ok()
        .filter(|value| *value <= 100)
}

fn parse_unsigned(value: &str) -> Option<u32> {
    (!value.is_empty()).then(|| value.parse().ok()).flatten()
}

fn parse_attributes(
    attributes: &mut TmuxAttributes,
    value: &str,
    state: TmuxAttributeState,
) -> bool {
    if matches!(value, "default" | "none") {
        return true;
    }
    value
        .split('|')
        .all(|attribute| set_attribute(attributes, attribute, state))
}

fn set_attribute(attributes: &mut TmuxAttributes, name: &str, state: TmuxAttributeState) -> bool {
    let slot = match name {
        "acs" => &mut attributes.acs,
        "bright" | "bold" => &mut attributes.bold,
        "dim" => &mut attributes.dim,
        "underscore" => &mut attributes.underscore,
        "blink" => &mut attributes.blink,
        "reverse" => &mut attributes.reverse,
        "hidden" => &mut attributes.hidden,
        "italics" => &mut attributes.italics,
        "strikethrough" => &mut attributes.strikethrough,
        "double-underscore" => &mut attributes.double_underscore,
        "curly-underscore" => &mut attributes.curly_underscore,
        "dotted-underscore" => &mut attributes.dotted_underscore,
        "dashed-underscore" => &mut attributes.dashed_underscore,
        "overline" => &mut attributes.overline,
        _ => return false,
    };
    *slot = state;
    true
}

fn set_all_attributes(attributes: &mut TmuxAttributes, state: TmuxAttributeState) {
    attributes.acs = state;
    attributes.bold = state;
    attributes.dim = state;
    attributes.underscore = state;
    attributes.blink = state;
    attributes.reverse = state;
    attributes.hidden = state;
    attributes.italics = state;
    attributes.strikethrough = state;
    attributes.double_underscore = state;
    attributes.curly_underscore = state;
    attributes.dotted_underscore = state;
    attributes.dashed_underscore = state;
    attributes.overline = state;
    attributes.noattr = state;
}

fn apply_style(current: &mut TmuxStyle, delta: &TmuxStyle, base: &TmuxStyle) {
    if delta.reset {
        current.fg = base.fg;
        current.bg = base.bg;
        current.us = base.us;
        current.attributes.clone_from(&base.attributes);
        current.link = None;
    }
    apply_colour(&mut current.fg, delta.fg, base.fg);
    apply_colour(&mut current.bg, delta.bg, base.bg);
    apply_colour(&mut current.us, delta.us, base.us);
    apply_attributes(&mut current.attributes, &delta.attributes);
    if let Some(align) = delta.align {
        current.align = (align != TmuxAlign::Default).then_some(align);
    }
    if let Some(fill) = delta.fill {
        current.fill = Some(fill);
    }
    if let Some(list) = delta.list {
        current.list = Some(list);
    }
    if let Some(range) = &delta.range {
        current.range = (range != &TmuxRange::None).then(|| range.clone());
    }
    if let Some(width) = delta.width {
        current.width = Some(width);
    }
    if let Some(pad) = delta.pad {
        current.pad = Some(pad);
    }
    if let Some(default_type) = delta.default_type {
        current.default_type = Some(default_type);
    }
    if let Some(dim) = delta.dim_percentage {
        current.dim_percentage = Some(dim);
    }
    if let Some(ignore) = delta.ignore {
        current.ignore = Some(ignore);
    }
    if let Some(link) = &delta.link {
        current.link = (!link.is_empty()).then(|| link.clone());
    }
}

fn apply_colour(
    current: &mut Option<TmuxColour>,
    delta: Option<TmuxColour>,
    base: Option<TmuxColour>,
) {
    if let Some(delta) = delta {
        *current = if delta == TmuxColour::Default {
            base
        } else {
            Some(delta)
        };
    }
}

fn apply_attributes(current: &mut TmuxAttributes, delta: &TmuxAttributes) {
    apply_attribute(&mut current.acs, delta.acs);
    apply_attribute(&mut current.bold, delta.bold);
    apply_attribute(&mut current.dim, delta.dim);
    apply_attribute(&mut current.underscore, delta.underscore);
    apply_attribute(&mut current.blink, delta.blink);
    apply_attribute(&mut current.reverse, delta.reverse);
    apply_attribute(&mut current.hidden, delta.hidden);
    apply_attribute(&mut current.italics, delta.italics);
    apply_attribute(&mut current.strikethrough, delta.strikethrough);
    apply_attribute(&mut current.double_underscore, delta.double_underscore);
    apply_attribute(&mut current.curly_underscore, delta.curly_underscore);
    apply_attribute(&mut current.dotted_underscore, delta.dotted_underscore);
    apply_attribute(&mut current.dashed_underscore, delta.dashed_underscore);
    apply_attribute(&mut current.overline, delta.overline);
    apply_attribute(&mut current.noattr, delta.noattr);
}

fn apply_attribute(current: &mut TmuxAttributeState, delta: TmuxAttributeState) {
    if delta != TmuxAttributeState::Unset {
        *current = delta;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_pinned_style_tokens_and_attributes() {
        for value in [
            "default",
            "none",
            "ignore",
            "noignore",
            "push-default",
            "pop-default",
            "set-default",
            "nolist",
            "norange",
            "noalign",
            "nolink",
            "noattr",
            "fg=red",
            "bg=colour123",
            "us=#123456",
            "fill=terminal",
            "align=left",
            "align=centre",
            "align=right",
            "align=absolute-centre",
            "list=on",
            "list=focus",
            "list=left-marker",
            "list=right-marker",
            "range=left",
            "range=right",
            "range=control|9",
            "range=pane|%12",
            "range=window|12",
            "range=session|$12",
            "range=user|owner",
            "range=custom",
            "range=custom|value",
            "dim=50%",
            "width=80%",
            "width=80",
            "pad=2",
            "link=https://example.com",
            "LINK=https://example.com/CaseSensitive",
            "nodefault",
            "nobold|underscore",
            "acs|bright|bold|dim|underscore|blink|reverse|hidden|italics|strikethrough|double-underscore|curly-underscore|dotted-underscore|dashed-underscore|overline",
        ] {
            assert!(parse_style(value).is_some(), "{value}");
        }
    }

    #[test]
    fn parsed_styles_carry_typed_values_and_explicit_resets() {
        let style = parse_style(
            "fg=red,bg=blue,us=green,bold,nodim,align=right,fill=yellow,list=focus,range=pane|%12,width=80%,pad=2,dim=50%,ignore,link=https://example.com,push-default",
        )
        .expect("valid style");
        assert_eq!(style.fg, Some(TmuxColour::Basic(1)));
        assert_eq!(style.bg, Some(TmuxColour::Basic(4)));
        assert_eq!(style.us, Some(TmuxColour::Basic(2)));
        assert_eq!(style.attributes.bold, TmuxAttributeState::On);
        assert_eq!(style.attributes.dim, TmuxAttributeState::Off);
        assert_eq!(style.align, Some(TmuxAlign::Right));
        assert_eq!(style.fill, Some(TmuxColour::Basic(3)));
        assert_eq!(style.list, Some(TmuxList::Focus));
        assert_eq!(style.range, Some(TmuxRange::Pane(12)));
        assert_eq!(style.width, Some(TmuxWidth::Percentage(80)));
        assert_eq!(style.pad, Some(2));
        assert_eq!(style.dim_percentage, Some(50));
        assert_eq!(style.ignore, Some(true));
        assert_eq!(style.link.as_deref(), Some("https://example.com"));
        assert_eq!(style.default_type, Some(TmuxDefaultType::Push));

        assert!(parse_style("default").expect("default").reset);
        assert_eq!(
            parse_style("noalign").expect("noalign").align,
            Some(TmuxAlign::Default)
        );
        assert_eq!(
            parse_style("norange").expect("norange").range,
            Some(TmuxRange::None)
        );
    }

    #[test]
    fn rejects_invalid_pinned_values() {
        for value in [
            "fg=colour256",
            "fg=#zzzzzz",
            "bogus",
            "fg=",
            "align=middle",
            "fill=",
            "us=",
            "list=no",
            "list=bogus",
            "range=control|10",
            "range=pane|12",
            "range=session|12",
            "range=user|",
            "dim=101",
            "width=-1",
            "pad=-1",
            "hyperlink",
            "nohyperlink",
            "bold|noitalics",
        ] {
            assert!(parse_style(value).is_none(), "{value}");
        }
    }

    #[test]
    fn styled_segments_are_cumulative_and_honor_resets_and_escaping() {
        let segments = parse_styled_segments(
            "plain#[fg=red,bold]hot#[italics]both#[none]plain##[fg=blue]literal#[default]end",
        );
        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>(),
            ["plain", "hot", "both", "plain#[fg=blue]literal", "end"]
        );
        assert_eq!(segments[1].style.fg, Some(TmuxColour::Basic(1)));
        assert_eq!(segments[1].style.attributes.bold, TmuxAttributeState::On);
        assert_eq!(segments[2].style.attributes.italics, TmuxAttributeState::On);
        assert_eq!(segments[3].style.attributes.bold, TmuxAttributeState::Off);
        assert_eq!(
            segments[3].style.attributes.italics,
            TmuxAttributeState::Off
        );
        assert_eq!(segments[4].style, TmuxStyle::default());

        let segments = parse_styled_segments("#[align=right]right#[noalign]default");
        assert_eq!(segments[0].style.align, Some(TmuxAlign::Right));
        assert_eq!(segments[1].style.align, None);
    }

    #[test]
    fn default_stack_matches_format_draw() {
        let segments = parse_styled_segments(
            "#[fg=red]red#[push-default,fg=blue]blue#[default]red#[pop-default]#[default]base",
        );
        assert_eq!(segments[0].style.fg, Some(TmuxColour::Basic(1)));
        assert_eq!(segments[1].style.fg, Some(TmuxColour::Basic(4)));
        assert_eq!(segments[2].style.fg, Some(TmuxColour::Basic(1)));
        assert_eq!(segments[3].style.fg, None);
    }
}
