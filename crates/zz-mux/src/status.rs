use std::time::Duration;

use crate::{parse_style, parse_tmux_colour};

pub use crate::formats::{
    FormatUniverse, StatusContext, StatusHooks, expand_format_values, expand_status,
};

pub const DEFAULT_STATUS_LEFT: &str = crate::tmux_options::STATUS_LEFT_DEFAULT;
pub const DEFAULT_STATUS_RIGHT: &str = crate::tmux_options::STATUS_RIGHT_DEFAULT;
pub const DEFAULT_STATUS_INTERVAL: Duration = Duration::from_secs(15);
pub const DEFAULT_STATUS_STYLE: &str = "bg=themegreen,fg=themeblack";
pub const DEFAULT_WINDOW_STATUS_FORMAT: &str = "#I:#W#{?window_flags,#{window_flags}, }";
pub const MAX_STATUS_FORMAT_BYTES: usize = 4 * 1024;
const MAX_STATUS_SIDE_LENGTH: u16 = i16::MAX as u16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusJustify {
    #[default]
    Left,
    Centre,
    Right,
    AbsoluteCentre,
}

impl StatusJustify {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Centre => "centre",
            Self::Right => "right",
            Self::AbsoluteCentre => "absolute-centre",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusPosition {
    Top,
    #[default]
    Bottom,
}

impl StatusPosition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusFormats {
    pub enabled: bool,
    pub lines: u8,
    pub interval: Duration,
    pub left: String,
    pub right: String,
    pub style: String,
    pub background: String,
    pub foreground: String,
    pub left_style: String,
    pub right_style: String,
    pub left_length: u16,
    pub right_length: u16,
    pub justify: StatusJustify,
    pub position: StatusPosition,
}

impl Default for StatusFormats {
    fn default() -> Self {
        Self {
            enabled: true,
            lines: 1,
            interval: DEFAULT_STATUS_INTERVAL,
            left: DEFAULT_STATUS_LEFT.to_owned(),
            right: DEFAULT_STATUS_RIGHT.to_owned(),
            style: DEFAULT_STATUS_STYLE.to_owned(),
            background: "default".to_owned(),
            foreground: "default".to_owned(),
            left_style: "default".to_owned(),
            right_style: "default".to_owned(),
            left_length: 10,
            right_length: 40,
            justify: StatusJustify::Left,
            position: StatusPosition::Bottom,
        }
    }
}

impl StatusFormats {
    #[must_use]
    pub fn lines_string(&self) -> String {
        match self.lines {
            0 => "off".to_owned(),
            1 => "on".to_owned(),
            lines => lines.to_string(),
        }
    }

    pub fn toggle_enabled_choice(&mut self) -> bool {
        let next = match self.lines {
            0 => 1,
            1 => 0,
            lines => lines,
        };
        let changed = self.lines != next;
        self.lines = next;
        self.enabled = next != 0;
        changed
    }

    #[must_use]
    pub fn format(&self, option: StatusOption) -> Option<&str> {
        match option {
            StatusOption::Left => Some(&self.left),
            StatusOption::Right => Some(&self.right),
            StatusOption::Style => Some(&self.style),
            StatusOption::LeftStyle => Some(&self.left_style),
            StatusOption::RightStyle => Some(&self.right_style),
            _ => None,
        }
    }

    #[must_use]
    pub fn value(&self, option: StatusOption) -> String {
        match option {
            StatusOption::Enabled => self.lines_string(),
            StatusOption::Interval => self.interval.as_secs().to_string(),
            StatusOption::Left => self.left.clone(),
            StatusOption::Right => self.right.clone(),
            StatusOption::Style => self.style.clone(),
            StatusOption::Background => self.background.clone(),
            StatusOption::Foreground => self.foreground.clone(),
            StatusOption::LeftStyle => self.left_style.clone(),
            StatusOption::RightStyle => self.right_style.clone(),
            StatusOption::LeftLength => self.left_length.to_string(),
            StatusOption::RightLength => self.right_length.to_string(),
            StatusOption::Justify => self.justify.as_str().to_owned(),
            StatusOption::Position => self.position.as_str().to_owned(),
        }
    }

    pub fn set(&mut self, option: StatusOption, value: Option<&str>) -> Result<bool, String> {
        let defaults = Self::default();
        match option {
            StatusOption::Enabled => {
                let next = value.map_or(Ok(defaults.lines), parse_enabled)?;
                let changed = std::mem::replace(&mut self.lines, next) != next;
                self.enabled = next != 0;
                Ok(changed)
            }
            StatusOption::Interval => {
                let next = value.map_or(Ok(defaults.interval), parse_interval)?;
                Ok(std::mem::replace(&mut self.interval, next) != next)
            }
            StatusOption::Left | StatusOption::Right => {
                let next = value.map_or_else(
                    || {
                        Ok(if option == StatusOption::Right {
                            defaults.right
                        } else {
                            defaults.left
                        })
                    },
                    parse_format,
                )?;
                let slot = if option == StatusOption::Right {
                    &mut self.right
                } else {
                    &mut self.left
                };
                Ok(std::mem::replace(slot, next.clone()) != next)
            }
            StatusOption::Style | StatusOption::LeftStyle | StatusOption::RightStyle => {
                let next = value.map_or_else(|| Ok(defaults.value(option)), parse_style_option)?;
                let slot = match option {
                    StatusOption::Style => &mut self.style,
                    StatusOption::LeftStyle => &mut self.left_style,
                    StatusOption::RightStyle => &mut self.right_style,
                    _ => unreachable!(),
                };
                Ok(std::mem::replace(slot, next.clone()) != next)
            }
            StatusOption::Background | StatusOption::Foreground => {
                let next = value.map_or_else(
                    || Ok(defaults.value(option)),
                    |value| {
                        parse_tmux_colour(value)
                            .is_some()
                            .then(|| value.to_owned())
                            .ok_or_else(|| format!("bad colour: {value}"))
                    },
                )?;
                let slot = if option == StatusOption::Background {
                    &mut self.background
                } else {
                    &mut self.foreground
                };
                Ok(std::mem::replace(slot, next.clone()) != next)
            }
            StatusOption::LeftLength | StatusOption::RightLength => {
                let next = value.map_or_else(
                    || {
                        Ok(if option == StatusOption::LeftLength {
                            defaults.left_length
                        } else {
                            defaults.right_length
                        })
                    },
                    parse_side_length,
                )?;
                let slot = if option == StatusOption::LeftLength {
                    &mut self.left_length
                } else {
                    &mut self.right_length
                };
                Ok(std::mem::replace(slot, next) != next)
            }
            StatusOption::Justify => {
                let next = value.map_or(Ok(defaults.justify), parse_justify)?;
                Ok(std::mem::replace(&mut self.justify, next) != next)
            }
            StatusOption::Position => {
                let next = value.map_or(Ok(defaults.position), parse_position)?;
                Ok(std::mem::replace(&mut self.position, next) != next)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatusOption {
    Enabled,
    Background,
    Foreground,
    Interval,
    Justify,
    Left,
    LeftLength,
    LeftStyle,
    Position,
    Right,
    RightLength,
    RightStyle,
    Style,
}

impl StatusOption {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "status" => Self::Enabled,
            "status-bg" => Self::Background,
            "status-fg" => Self::Foreground,
            "status-interval" => Self::Interval,
            "status-justify" => Self::Justify,
            "status-left" => Self::Left,
            "status-left-length" => Self::LeftLength,
            "status-left-style" => Self::LeftStyle,
            "status-position" => Self::Position,
            "status-right" => Self::Right,
            "status-right-length" => Self::RightLength,
            "status-right-style" => Self::RightStyle,
            "status-style" => Self::Style,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "status",
            Self::Background => "status-bg",
            Self::Foreground => "status-fg",
            Self::Interval => "status-interval",
            Self::Justify => "status-justify",
            Self::Left => "status-left",
            Self::LeftLength => "status-left-length",
            Self::LeftStyle => "status-left-style",
            Self::Position => "status-position",
            Self::Right => "status-right",
            Self::RightLength => "status-right-length",
            Self::RightStyle => "status-right-style",
            Self::Style => "status-style",
        }
    }

    pub const fn is_string(self) -> bool {
        matches!(self, Self::Left | Self::Right) || self.is_style()
    }

    pub const fn is_style(self) -> bool {
        matches!(self, Self::LeftStyle | Self::RightStyle | Self::Style)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowStatusFormats {
    pub format: String,
    pub current_format: String,
    pub separator: String,
    pub style: String,
    pub current_style: String,
    pub last_style: String,
    pub bell_style: String,
    pub activity_style: String,
}

impl Default for WindowStatusFormats {
    fn default() -> Self {
        Self {
            format: DEFAULT_WINDOW_STATUS_FORMAT.to_owned(),
            current_format: DEFAULT_WINDOW_STATUS_FORMAT.to_owned(),
            separator: " ".to_owned(),
            style: "default".to_owned(),
            current_style: "underscore".to_owned(),
            last_style: "default".to_owned(),
            bell_style: "reverse".to_owned(),
            activity_style: "reverse".to_owned(),
        }
    }
}

impl WindowStatusFormats {
    pub fn value(&self, option: WindowStatusOption) -> &str {
        match option {
            WindowStatusOption::Format => &self.format,
            WindowStatusOption::CurrentFormat => &self.current_format,
            WindowStatusOption::Separator => &self.separator,
            WindowStatusOption::Style => &self.style,
            WindowStatusOption::CurrentStyle => &self.current_style,
            WindowStatusOption::LastStyle => &self.last_style,
            WindowStatusOption::BellStyle => &self.bell_style,
            WindowStatusOption::ActivityStyle => &self.activity_style,
        }
    }

    pub fn set(&mut self, option: WindowStatusOption, value: Option<&str>) -> Result<bool, String> {
        let defaults = Self::default();
        let next = value.map_or_else(
            || Ok(defaults.value(option).to_owned()),
            |value| {
                if option.is_style() {
                    parse_style_option(value)
                } else if value.len() > MAX_STATUS_FORMAT_BYTES {
                    Err("status format exceeds the supported length".to_owned())
                } else {
                    Ok(value.to_owned())
                }
            },
        )?;
        let slot = match option {
            WindowStatusOption::Format => &mut self.format,
            WindowStatusOption::CurrentFormat => &mut self.current_format,
            WindowStatusOption::Separator => &mut self.separator,
            WindowStatusOption::Style => &mut self.style,
            WindowStatusOption::CurrentStyle => &mut self.current_style,
            WindowStatusOption::LastStyle => &mut self.last_style,
            WindowStatusOption::BellStyle => &mut self.bell_style,
            WindowStatusOption::ActivityStyle => &mut self.activity_style,
        };
        Ok(std::mem::replace(slot, next.clone()) != next)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowStatusOption {
    Format,
    CurrentFormat,
    Separator,
    Style,
    CurrentStyle,
    LastStyle,
    BellStyle,
    ActivityStyle,
}

impl WindowStatusOption {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "window-status-format" => Self::Format,
            "window-status-current-format" => Self::CurrentFormat,
            "window-status-separator" => Self::Separator,
            "window-status-style" => Self::Style,
            "window-status-current-style" => Self::CurrentStyle,
            "window-status-last-style" => Self::LastStyle,
            "window-status-bell-style" => Self::BellStyle,
            "window-status-activity-style" => Self::ActivityStyle,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Format => "window-status-format",
            Self::CurrentFormat => "window-status-current-format",
            Self::Separator => "window-status-separator",
            Self::Style => "window-status-style",
            Self::CurrentStyle => "window-status-current-style",
            Self::LastStyle => "window-status-last-style",
            Self::BellStyle => "window-status-bell-style",
            Self::ActivityStyle => "window-status-activity-style",
        }
    }

    pub const fn is_style(self) -> bool {
        !matches!(self, Self::Format | Self::CurrentFormat | Self::Separator)
    }
}

fn parse_style_option(value: &str) -> Result<String, String> {
    if value.contains("#{") || parse_style(value).is_some() {
        Ok(value.to_owned())
    } else {
        Err(format!("invalid style: {value}"))
    }
}

fn parse_enabled(value: &str) -> Result<u8, String> {
    match value {
        "on" | "1" => Ok(1),
        "off" | "0" => Ok(0),
        "2" => Ok(2),
        "3" => Ok(3),
        "4" => Ok(4),
        "5" => Ok(5),
        _ => Err("status expects on, off, or a line count in 1..5".to_owned()),
    }
}

fn parse_interval(value: &str) -> Result<Duration, String> {
    value
        .parse::<u32>()
        .map(|seconds| Duration::from_secs(u64::from(seconds)))
        .map_err(|_| "status-interval expects a whole number of seconds".to_owned())
}

fn parse_format(value: &str) -> Result<String, String> {
    if value.len() > MAX_STATUS_FORMAT_BYTES {
        return Err("status format exceeds the supported length".to_owned());
    }
    Ok(value.to_owned())
}

fn parse_side_length(value: &str) -> Result<u16, String> {
    match value.parse::<i128>() {
        Ok(number) if number < 0 => Err(format!("value is too small: {value}")),
        Ok(number) if number > i128::from(MAX_STATUS_SIDE_LENGTH) => {
            Err(format!("value is too large: {value}"))
        }
        Ok(number) => Ok(u16::try_from(number).expect("bounded status length fits u16")),
        Err(_) => Err(format!("value is invalid: {value}")),
    }
}

fn parse_justify(value: &str) -> Result<StatusJustify, String> {
    match value {
        "left" => Ok(StatusJustify::Left),
        "centre" => Ok(StatusJustify::Centre),
        "right" => Ok(StatusJustify::Right),
        "absolute-centre" => Ok(StatusJustify::AbsoluteCentre),
        _ => Err(format!("unknown value: {value}")),
    }
}

fn parse_position(value: &str) -> Result<StatusPosition, String> {
    match value {
        "top" => Ok(StatusPosition::Top),
        "bottom" => Ok(StatusPosition::Bottom),
        _ => Err(format!("unknown value: {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_values_parse_like_tmux() {
        let mut formats = StatusFormats::default();
        assert_eq!(formats.set(StatusOption::Enabled, Some("off")), Ok(true));
        assert!(!formats.enabled);
        assert_eq!(formats.lines, 0);
        assert_eq!(formats.set(StatusOption::Enabled, Some("2")), Ok(true));
        assert!(formats.enabled);
        assert_eq!(formats.lines, 2);
        assert!(!formats.toggle_enabled_choice());
        assert_eq!(formats.lines_string(), "2");
        assert_eq!(formats.set(StatusOption::Enabled, Some("on")), Ok(true));
        assert_eq!(formats.lines, 1);
        assert!(formats.toggle_enabled_choice());
        assert_eq!(formats.lines_string(), "off");
        assert!(
            formats
                .set(StatusOption::Enabled, Some("sometimes"))
                .is_err()
        );
        assert!(!formats.enabled);
        assert_eq!(formats.set(StatusOption::Interval, Some("5")), Ok(true));
        assert_eq!(formats.interval, Duration::from_secs(5));
        assert_eq!(formats.set(StatusOption::Interval, Some("0")), Ok(true));
        assert!(formats.set(StatusOption::Interval, Some("-1")).is_err());
        assert!(
            formats
                .set(
                    StatusOption::Left,
                    Some(&"x".repeat(MAX_STATUS_FORMAT_BYTES + 1))
                )
                .is_err()
        );
        assert_eq!(formats.set(StatusOption::Right, Some("%H:%M")), Ok(true));
        assert_eq!(formats.format(StatusOption::Right), Some("%H:%M"));
        assert_eq!(formats.set(StatusOption::Right, None), Ok(true));
        assert_eq!(formats.right, DEFAULT_STATUS_RIGHT);
        assert_eq!(formats.set(StatusOption::Interval, None), Ok(true));
        assert_eq!(formats.interval, DEFAULT_STATUS_INTERVAL);
    }

    #[test]
    fn status_family_defaults_and_rejections_match_the_pin() {
        let formats = StatusFormats::default();
        assert_eq!(
            formats.value(StatusOption::Style),
            "bg=themegreen,fg=themeblack"
        );
        assert_eq!(formats.value(StatusOption::Background), "default");
        assert_eq!(formats.value(StatusOption::Foreground), "default");
        assert_eq!(formats.value(StatusOption::LeftStyle), "default");
        assert_eq!(formats.value(StatusOption::RightStyle), "default");
        assert_eq!(formats.value(StatusOption::LeftLength), "10");
        assert_eq!(formats.value(StatusOption::RightLength), "40");
        assert_eq!(formats.value(StatusOption::Justify), "left");
        assert_eq!(formats.value(StatusOption::Position), "bottom");

        let mut formats = formats;
        assert_eq!(
            formats.set(StatusOption::LeftLength, Some("-1")),
            Err("value is too small: -1".to_owned())
        );
        assert_eq!(
            formats.set(StatusOption::RightLength, Some("32768")),
            Err("value is too large: 32768".to_owned())
        );
        assert_eq!(
            formats.set(StatusOption::Justify, Some("middle")),
            Err("unknown value: middle".to_owned())
        );
        assert_eq!(
            formats.set(StatusOption::Style, Some("fg=nope")),
            Err("invalid style: fg=nope".to_owned())
        );
        assert_eq!(
            formats.set(StatusOption::Foreground, Some("nope")),
            Err("bad colour: nope".to_owned())
        );
    }

    #[test]
    fn window_status_defaults_match_the_pin() {
        let formats = WindowStatusFormats::default();
        assert_eq!(formats.format, DEFAULT_WINDOW_STATUS_FORMAT);
        assert_eq!(formats.current_format, DEFAULT_WINDOW_STATUS_FORMAT);
        assert_eq!(formats.separator, " ");
        assert_eq!(formats.style, "default");
        assert_eq!(formats.current_style, "underscore");
        assert_eq!(formats.last_style, "default");
        assert_eq!(formats.bell_style, "reverse");
        assert_eq!(formats.activity_style, "reverse");
    }

    #[test]
    fn status_style_options_defer_dynamic_values() {
        let dynamic = "fg=#{?client_prefix,red,green}";
        let mut status = StatusFormats::default();
        for option in [
            StatusOption::Style,
            StatusOption::LeftStyle,
            StatusOption::RightStyle,
        ] {
            assert_eq!(status.set(option, Some(dynamic)), Ok(true));
            assert_eq!(status.value(option), dynamic);
        }

        let mut window = WindowStatusFormats::default();
        for option in [
            WindowStatusOption::Style,
            WindowStatusOption::CurrentStyle,
            WindowStatusOption::LastStyle,
            WindowStatusOption::BellStyle,
            WindowStatusOption::ActivityStyle,
        ] {
            assert_eq!(window.set(option, Some(dynamic)), Ok(true));
            assert_eq!(window.value(option), dynamic);
        }
    }

    #[test]
    fn option_names_round_trip() {
        for option in [
            StatusOption::Enabled,
            StatusOption::Background,
            StatusOption::Foreground,
            StatusOption::Interval,
            StatusOption::Justify,
            StatusOption::Left,
            StatusOption::LeftLength,
            StatusOption::LeftStyle,
            StatusOption::Position,
            StatusOption::Right,
            StatusOption::RightLength,
            StatusOption::RightStyle,
            StatusOption::Style,
        ] {
            assert_eq!(StatusOption::from_name(option.as_str()), Some(option));
        }
    }
}
