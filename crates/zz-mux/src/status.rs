use std::time::Duration;

pub use crate::formats::{
    FormatUniverse, StatusContext, StatusHooks, expand_format_values, expand_status,
};

pub const DEFAULT_STATUS_LEFT: &str = crate::tmux_options::STATUS_LEFT_DEFAULT;
pub const DEFAULT_STATUS_RIGHT: &str = crate::tmux_options::STATUS_RIGHT_DEFAULT;
pub const DEFAULT_STATUS_INTERVAL: Duration = Duration::from_secs(15);
pub const MAX_STATUS_FORMAT_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusFormats {
    pub enabled: bool,
    pub lines: u8,
    pub interval: Duration,
    pub left: String,
    pub right: String,
}

impl Default for StatusFormats {
    fn default() -> Self {
        Self {
            enabled: true,
            lines: 1,
            interval: DEFAULT_STATUS_INTERVAL,
            left: DEFAULT_STATUS_LEFT.to_owned(),
            right: DEFAULT_STATUS_RIGHT.to_owned(),
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
            StatusOption::Left => Some(self.left.as_str()),
            StatusOption::Right => Some(self.right.as_str()),
            StatusOption::Enabled | StatusOption::Interval => None,
        }
    }

    pub fn set(&mut self, option: StatusOption, value: Option<&str>) -> Result<bool, &'static str> {
        let defaults = Self::default();
        match option {
            StatusOption::Enabled => {
                let next = match value {
                    Some(value) => parse_enabled(value)?,
                    None => defaults.lines,
                };
                let changed = std::mem::replace(&mut self.lines, next) != next;
                self.enabled = next != 0;
                Ok(changed)
            }
            StatusOption::Interval => {
                let next = match value {
                    Some(value) => parse_interval(value)?,
                    None => defaults.interval,
                };
                Ok(std::mem::replace(&mut self.interval, next) != next)
            }
            StatusOption::Left | StatusOption::Right => {
                let next = match value {
                    Some(value) => parse_format(value)?,
                    None => match option {
                        StatusOption::Right => defaults.right,
                        _ => defaults.left,
                    },
                };
                let slot = match option {
                    StatusOption::Right => &mut self.right,
                    _ => &mut self.left,
                };
                Ok(std::mem::replace(slot, next) != *slot)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusOption {
    Enabled,
    Interval,
    Left,
    Right,
}

impl StatusOption {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "status" => Self::Enabled,
            "status-interval" => Self::Interval,
            "status-left" => Self::Left,
            "status-right" => Self::Right,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "status",
            Self::Interval => "status-interval",
            Self::Left => "status-left",
            Self::Right => "status-right",
        }
    }
}

fn parse_enabled(value: &str) -> Result<u8, &'static str> {
    match value {
        "on" | "1" => Ok(1),
        "off" | "0" => Ok(0),
        "2" => Ok(2),
        "3" => Ok(3),
        "4" => Ok(4),
        "5" => Ok(5),
        _ => Err("status expects on, off, or a line count in 1..5"),
    }
}

fn parse_interval(value: &str) -> Result<Duration, &'static str> {
    value
        .parse::<u32>()
        .map(|seconds| Duration::from_secs(u64::from(seconds)))
        .map_err(|_| "status-interval expects a whole number of seconds")
}

fn parse_format(value: &str) -> Result<String, &'static str> {
    if value.len() > MAX_STATUS_FORMAT_BYTES {
        return Err("status format exceeds the supported length");
    }
    Ok(value.to_owned())
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
    fn option_names_round_trip() {
        for option in [
            StatusOption::Enabled,
            StatusOption::Interval,
            StatusOption::Left,
            StatusOption::Right,
        ] {
            assert_eq!(StatusOption::from_name(option.as_str()), Some(option));
        }
        assert_eq!(StatusOption::from_name("status-justify"), None);
    }
}
