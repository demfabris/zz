#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TmuxOptionScope {
    Server,
    Session,
    Window,
    WindowPane,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TmuxOption {
    pub(crate) name: &'static str,
    pub(crate) scope: TmuxOptionScope,
    pub(crate) default: Option<TmuxOptionDefault>,
    pub(crate) is_array: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TmuxOptionDefault {
    Array(&'static str),
    String(&'static str),
    Scalar(&'static str),
}

impl TmuxOptionDefault {
    pub(crate) const fn value(self) -> &'static str {
        match self {
            Self::Array(value) | Self::String(value) | Self::Scalar(value) => value,
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(self, Self::String(_))
    }
}

pub(crate) const STATUS_LEFT_DEFAULT: &str = "[#{session_name}] ";
pub(crate) const STATUS_RIGHT_DEFAULT: &str = "#{?window_bigger,[#{window_offset_x}#,#{window_offset_y}] ,}\"#{=21:pane_title}\" %H:%M %d-%b-%y";
pub(crate) const UPDATE_ENVIRONMENT_DEFAULT: &str = "DISPLAY KRB5CCNAME MSYSTEM SSH_ASKPASS SSH_AUTH_SOCK SSH_AGENT_PID SSH_CONNECTION WAYLAND_DISPLAY WINDOWID XAUTHORITY XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE";

const SERVER_OPTIONS: &[&str] = &[
    "backspace",
    "buffer-limit",
    "codepoint-widths",
    "command-alias",
    "copy-command",
    "dark-theme-black",
    "dark-theme-blue",
    "dark-theme-cyan",
    "dark-theme-dark-grey",
    "dark-theme-green",
    "dark-theme-light-grey",
    "dark-theme-magenta",
    "dark-theme-red",
    "dark-theme-white",
    "dark-theme-yellow",
    "default-client-command",
    "default-terminal",
    "editor",
    "escape-time",
    "exit-empty",
    "exit-unattached",
    "extended-keys",
    "extended-keys-format",
    "focus-events",
    "get-clipboard",
    "history-file",
    "input-buffer-size",
    "light-theme-black",
    "light-theme-blue",
    "light-theme-cyan",
    "light-theme-dark-grey",
    "light-theme-green",
    "light-theme-light-grey",
    "light-theme-magenta",
    "light-theme-red",
    "light-theme-white",
    "light-theme-yellow",
    "message-limit",
    "prefix-timeout",
    "prompt-history-limit",
    "set-clipboard",
    "terminal-features",
    "terminal-overrides",
    "theme",
    "user-keys",
    "variation-selector-always-wide",
];

const SESSION_OPTIONS: &[&str] = &[
    "activity-action",
    "after-bind-key",
    "after-capture-pane",
    "after-copy-mode",
    "after-display-message",
    "after-display-panes",
    "after-kill-pane",
    "after-list-buffers",
    "after-list-clients",
    "after-list-keys",
    "after-list-panes",
    "after-list-sessions",
    "after-list-windows",
    "after-load-buffer",
    "after-lock-server",
    "after-new-session",
    "after-new-window",
    "after-paste-buffer",
    "after-pipe-pane",
    "after-queue",
    "after-refresh-client",
    "after-rename-session",
    "after-rename-window",
    "after-resize-pane",
    "after-resize-window",
    "after-save-buffer",
    "after-select-layout",
    "after-select-pane",
    "after-select-window",
    "after-send-keys",
    "after-set-buffer",
    "after-set-environment",
    "after-set-hook",
    "after-set-option",
    "after-show-environment",
    "after-show-messages",
    "after-show-options",
    "after-split-window",
    "after-unbind-key",
    "alert-activity",
    "alert-bell",
    "alert-silence",
    "assume-paste-time",
    "base-index",
    "bell-action",
    "client-active",
    "client-attached",
    "client-dark-theme",
    "client-detached",
    "client-focus-in",
    "client-focus-out",
    "client-light-theme",
    "client-resized",
    "client-session-changed",
    "command-error",
    "default-command",
    "default-shell",
    "default-size",
    "destroy-unattached",
    "detach-on-destroy",
    "display-panes-active-colour",
    "display-panes-colour",
    "display-panes-format",
    "display-panes-time",
    "display-time",
    "focus-follows-mouse",
    "history-limit",
    "initial-repeat-time",
    "key-table",
    "lock-after-time",
    "lock-command",
    "message-command-style",
    "message-format",
    "message-line",
    "message-style",
    "mouse",
    "prefix",
    "prefix2",
    "prompt-command-cursor-colour",
    "prompt-command-cursor-style",
    "prompt-cursor-colour",
    "prompt-cursor-style",
    "renumber-windows",
    "repeat-time",
    "session-closed",
    "session-created",
    "session-renamed",
    "session-window-changed",
    "set-titles",
    "set-titles-string",
    "silence-action",
    "status",
    "status-bg",
    "status-fg",
    "status-format",
    "status-interval",
    "status-justify",
    "status-keys",
    "status-left",
    "status-left-length",
    "status-left-style",
    "status-position",
    "status-right",
    "status-right-length",
    "status-right-style",
    "status-style",
    "update-environment",
    "visual-activity",
    "visual-bell",
    "visual-silence",
    "window-linked",
    "window-unlinked",
    "word-separators",
];

const WINDOW_OPTIONS: &[&str] = &[
    "aggressive-resize",
    "automatic-rename",
    "automatic-rename-format",
    "clock-mode-colour",
    "clock-mode-style",
    "copy-mode-current-line-number-style",
    "copy-mode-current-match-style",
    "copy-mode-line-number-style",
    "copy-mode-line-numbers",
    "copy-mode-mark-style",
    "copy-mode-match-style",
    "copy-mode-position-style",
    "copy-mode-selection-style",
    "fill-character",
    "main-pane-height",
    "main-pane-width",
    "menu-border-lines",
    "menu-border-style",
    "menu-selected-style",
    "menu-style",
    "mode-keys",
    "mode-style",
    "monitor-activity",
    "monitor-bell",
    "monitor-silence",
    "other-pane-height",
    "other-pane-width",
    "pane-base-index",
    "pane-border-indicators",
    "pane-scrollbars",
    "pane-scrollbars-position",
    "pane-scrollbars-timeout",
    "pane-status-current-style",
    "pane-status-style",
    "popup-border-lines",
    "popup-border-style",
    "popup-style",
    "session-status-current-style",
    "session-status-style",
    "tiled-layout-max-columns",
    "tree-mode-border-style",
    "tree-mode-preview-style",
    "tree-mode-selection-style",
    "window-layout-changed",
    "window-pane-changed",
    "window-pane-current-status-format",
    "window-pane-status-format",
    "window-renamed",
    "window-resized",
    "window-size",
    "window-status-activity-style",
    "window-status-bell-style",
    "window-status-current-format",
    "window-status-current-style",
    "window-status-format",
    "window-status-last-style",
    "window-status-separator",
    "window-status-style",
    "wrap-search",
    "xterm-keys",
];

const WINDOW_PANE_OPTIONS: &[&str] = &[
    "allow-passthrough",
    "allow-rename",
    "allow-set-title",
    "alternate-screen",
    "copy-mode-position-format",
    "cursor-colour",
    "cursor-style",
    "pane-active-border-style",
    "pane-border-format",
    "pane-border-lines",
    "pane-border-status",
    "pane-border-style",
    "pane-colours",
    "pane-died",
    "pane-exited",
    "pane-focus-in",
    "pane-focus-out",
    "pane-mode-changed",
    "pane-scrollbars-style",
    "pane-set-clipboard",
    "pane-title-changed",
    "remain-on-exit",
    "remain-on-exit-format",
    "scroll-on-clear",
    "switch-mode-match-style",
    "synchronize-panes",
    "tree-mode-preview-format",
    "window-active-style",
    "window-style",
];

const ALIASES: &[(&str, &str)] = &[
    ("display-panes-color", "display-panes-colour"),
    ("display-panes-active-color", "display-panes-active-colour"),
    ("clock-mode-color", "clock-mode-colour"),
    ("cursor-color", "cursor-colour"),
    ("prompt-cursor-color", "prompt-cursor-colour"),
    (
        "prompt-command-cursor-color",
        "prompt-command-cursor-colour",
    ),
    ("pane-colors", "pane-colours"),
];

pub(crate) fn tmux_options() -> impl Iterator<Item = TmuxOption> {
    [
        (TmuxOptionScope::Server, SERVER_OPTIONS),
        (TmuxOptionScope::Session, SESSION_OPTIONS),
        (TmuxOptionScope::Window, WINDOW_OPTIONS),
        (TmuxOptionScope::WindowPane, WINDOW_PANE_OPTIONS),
    ]
    .into_iter()
    .flat_map(|(scope, names)| {
        names.iter().copied().map(move |name| TmuxOption {
            name,
            scope,
            default: tmux_option_default(name),
            is_array: tmux_option_is_array(name),
        })
    })
}

fn tmux_option_default(name: &str) -> Option<TmuxOptionDefault> {
    Some(match name {
        "default-terminal" => TmuxOptionDefault::String("tmux-256color"),
        "escape-time" => TmuxOptionDefault::Scalar("10"),
        "base-index" | "initial-repeat-time" | "pane-base-index" => TmuxOptionDefault::Scalar("0"),
        "buffer-limit" => TmuxOptionDefault::Scalar("50"),
        "copy-command" | "default-command" => TmuxOptionDefault::String(""),
        "default-shell" => TmuxOptionDefault::String("/bin/sh"),
        "history-limit" => TmuxOptionDefault::Scalar("2000"),
        "display-time" => TmuxOptionDefault::Scalar("750"),
        "message-limit" => TmuxOptionDefault::Scalar("1000"),
        "mode-keys" => TmuxOptionDefault::Scalar("emacs"),
        "aggressive-resize" | "renumber-windows" | "synchronize-panes" | "remain-on-exit" => {
            TmuxOptionDefault::Scalar("off")
        }
        "prefix" => TmuxOptionDefault::Scalar("C-b"),
        "repeat-time" => TmuxOptionDefault::Scalar("500"),
        "set-clipboard" => TmuxOptionDefault::Scalar("external"),
        "status" | "automatic-rename" | "mouse" => TmuxOptionDefault::Scalar("on"),
        "status-interval" => TmuxOptionDefault::Scalar("15"),
        "status-left" => TmuxOptionDefault::String(STATUS_LEFT_DEFAULT),
        "status-right" => TmuxOptionDefault::String(STATUS_RIGHT_DEFAULT),
        "update-environment" => TmuxOptionDefault::Array(UPDATE_ENVIRONMENT_DEFAULT),
        "word-separators" => TmuxOptionDefault::String("!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~"),
        "automatic-rename-format" => TmuxOptionDefault::String(
            "#{?pane_in_mode,[tmux],#{pane_current_command}}#{?pane_dead,[dead],}",
        ),
        _ => return None,
    })
}

fn tmux_option_is_array(name: &str) -> bool {
    matches!(
        name,
        "command-alias"
            | "codepoint-widths"
            | "terminal-overrides"
            | "terminal-features"
            | "user-keys"
            | "status-format"
            | "update-environment"
            | "pane-colours"
            | "alert-activity"
            | "alert-bell"
            | "alert-silence"
            | "command-error"
            | "pane-died"
            | "pane-exited"
            | "pane-focus-in"
            | "pane-focus-out"
            | "pane-mode-changed"
            | "pane-set-clipboard"
            | "pane-title-changed"
            | "session-closed"
            | "session-created"
            | "session-renamed"
            | "session-window-changed"
            | "window-layout-changed"
            | "window-linked"
            | "window-pane-changed"
            | "window-renamed"
            | "window-resized"
            | "window-unlinked"
    ) || name.starts_with("after-")
        || name.starts_with("client-")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedTmuxOption<'a> {
    pub(crate) name: &'a str,
    pub(crate) index: Option<String>,
}

pub(crate) fn parse_tmux_option(input: &str) -> Result<ParsedTmuxOption<'_>, ()> {
    if input.is_empty() {
        return Err(());
    }
    let Some(open) = input.find('[') else {
        return Ok(ParsedTmuxOption {
            name: input,
            index: None,
        });
    };
    let rest = &input[open + 1..];
    let Some(close) = rest.find(']') else {
        return Err(());
    };
    if close == 0 || open + close + 2 != input.len() {
        return Err(());
    }
    let raw = &rest[..close];
    let index = if raw.bytes().all(|byte| byte.is_ascii_digit()) {
        raw.parse::<u32>().map_err(|_| ())?.to_string()
    } else {
        raw.to_owned()
    };
    Ok(ParsedTmuxOption {
        name: &input[..open],
        index: Some(index),
    })
}

pub(crate) fn match_tmux_option(input: &str) -> Result<Option<TmuxOption>, ()> {
    let input = ALIASES
        .iter()
        .find_map(|(alias, name)| (*alias == input).then_some(*name))
        .unwrap_or(input);
    if let Some(exact) = tmux_options().find(|option| option.name == input) {
        return Ok(Some(exact));
    }
    let mut matches = tmux_options().filter(|option| option.name.starts_with(input));
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(());
    }
    Ok(Some(first))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn catalog_is_complete_and_unique() {
        let options = tmux_options().collect::<Vec<_>>();
        assert_eq!(options.len(), 248);
        assert_eq!(
            options
                .iter()
                .map(|option| option.name)
                .collect::<BTreeSet<_>>()
                .len(),
            options.len()
        );
    }

    #[test]
    fn exact_names_win_and_prefixes_must_be_unique() {
        assert_eq!(match_tmux_option("prefix").unwrap().unwrap().name, "prefix");
        assert_eq!(
            match_tmux_option("base-ind").unwrap().unwrap().name,
            "base-index"
        );
        assert!(match_tmux_option("status-l").is_err());
        assert_eq!(match_tmux_option("not-an-option"), Ok(None));
    }

    #[test]
    fn indexed_spellings_follow_the_pin_grammar() {
        assert_eq!(
            parse_tmux_option("status-format[000]").unwrap(),
            ParsedTmuxOption {
                name: "status-format",
                index: Some("0".to_owned()),
            }
        );
        assert_eq!(
            parse_tmux_option("@plain[key]").unwrap(),
            ParsedTmuxOption {
                name: "@plain",
                index: Some("key".to_owned()),
            }
        );
        assert_eq!(
            parse_tmux_option("name]tail").unwrap(),
            ParsedTmuxOption {
                name: "name]tail",
                index: None,
            }
        );
        for invalid in ["", "name[]", "name[0", "name[0]tail", "name[4294967296]"] {
            assert!(parse_tmux_option(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn array_metadata_matches_the_pin_table() {
        let arrays = tmux_options()
            .filter(|option| option.is_array)
            .map(|option| option.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(arrays.len(), 76);
        for name in [
            "command-alias",
            "codepoint-widths",
            "terminal-overrides",
            "terminal-features",
            "user-keys",
            "status-format",
            "update-environment",
            "pane-colours",
        ] {
            assert!(arrays.contains(name), "{name}");
        }
    }

    #[test]
    fn implemented_options_carry_their_pin_defaults_and_types() {
        let implemented = tmux_options()
            .filter_map(|option| option.default.map(|default| (option.name, default)))
            .collect::<Vec<_>>();
        assert_eq!(
            implemented,
            vec![
                ("buffer-limit", TmuxOptionDefault::Scalar("50")),
                ("copy-command", TmuxOptionDefault::String("")),
                (
                    "default-terminal",
                    TmuxOptionDefault::String("tmux-256color")
                ),
                ("escape-time", TmuxOptionDefault::Scalar("10")),
                ("message-limit", TmuxOptionDefault::Scalar("1000")),
                ("set-clipboard", TmuxOptionDefault::Scalar("external")),
                ("base-index", TmuxOptionDefault::Scalar("0")),
                ("default-command", TmuxOptionDefault::String("")),
                ("default-shell", TmuxOptionDefault::String("/bin/sh")),
                ("display-time", TmuxOptionDefault::Scalar("750")),
                ("history-limit", TmuxOptionDefault::Scalar("2000")),
                ("initial-repeat-time", TmuxOptionDefault::Scalar("0")),
                ("mouse", TmuxOptionDefault::Scalar("on")),
                ("prefix", TmuxOptionDefault::Scalar("C-b")),
                ("renumber-windows", TmuxOptionDefault::Scalar("off")),
                ("repeat-time", TmuxOptionDefault::Scalar("500")),
                ("status", TmuxOptionDefault::Scalar("on")),
                ("status-interval", TmuxOptionDefault::Scalar("15")),
                (
                    "status-left",
                    TmuxOptionDefault::String("[#{session_name}] ")
                ),
                (
                    "status-right",
                    TmuxOptionDefault::String(STATUS_RIGHT_DEFAULT),
                ),
                (
                    "update-environment",
                    TmuxOptionDefault::Array(UPDATE_ENVIRONMENT_DEFAULT),
                ),
                (
                    "word-separators",
                    TmuxOptionDefault::String("!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~"),
                ),
                ("aggressive-resize", TmuxOptionDefault::Scalar("off")),
                ("automatic-rename", TmuxOptionDefault::Scalar("on")),
                (
                    "automatic-rename-format",
                    TmuxOptionDefault::String(
                        "#{?pane_in_mode,[tmux],#{pane_current_command}}#{?pane_dead,[dead],}"
                    ),
                ),
                ("mode-keys", TmuxOptionDefault::Scalar("emacs")),
                ("pane-base-index", TmuxOptionDefault::Scalar("0")),
                ("remain-on-exit", TmuxOptionDefault::Scalar("off")),
                ("synchronize-panes", TmuxOptionDefault::Scalar("off")),
            ]
        );
    }
}
