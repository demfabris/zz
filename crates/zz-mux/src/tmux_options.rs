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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TmuxOptionDefault {
    String(&'static str),
    Scalar(&'static str),
}

impl TmuxOptionDefault {
    pub(crate) const fn value(self) -> &'static str {
        match self {
            Self::String(value) | Self::Scalar(value) => value,
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(self, Self::String(_))
    }
}

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
        })
    })
}

fn tmux_option_default(name: &str) -> Option<TmuxOptionDefault> {
    Some(match name {
        "base-index" | "pane-base-index" => TmuxOptionDefault::Scalar("0"),
        "buffer-limit" => TmuxOptionDefault::Scalar("50"),
        "copy-command" => TmuxOptionDefault::String(""),
        "history-limit" => TmuxOptionDefault::Scalar("2000"),
        "mode-keys" => TmuxOptionDefault::Scalar("emacs"),
        "prefix" => TmuxOptionDefault::Scalar("C-b"),
        "renumber-windows" | "synchronize-panes" => TmuxOptionDefault::Scalar("off"),
        "set-clipboard" => TmuxOptionDefault::Scalar("external"),
        "status" => TmuxOptionDefault::Scalar("on"),
        "status-interval" => TmuxOptionDefault::Scalar("15"),
        "status-left" => TmuxOptionDefault::String("[#{session_name}] "),
        "status-right" => TmuxOptionDefault::String(
            "#{?window_bigger,[#{window_offset_x}#,#{window_offset_y}] ,}\"#{=21:pane_title}\" %H:%M %d-%b-%y",
        ),
        "word-separators" => TmuxOptionDefault::String("!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~"),
        _ => return None,
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
    fn implemented_options_carry_their_pin_defaults_and_types() {
        let implemented = tmux_options()
            .filter_map(|option| option.default.map(|default| (option.name, default)))
            .collect::<Vec<_>>();
        assert_eq!(
            implemented,
            vec![
                ("buffer-limit", TmuxOptionDefault::Scalar("50")),
                ("copy-command", TmuxOptionDefault::String("")),
                ("set-clipboard", TmuxOptionDefault::Scalar("external")),
                ("base-index", TmuxOptionDefault::Scalar("0")),
                ("history-limit", TmuxOptionDefault::Scalar("2000")),
                ("prefix", TmuxOptionDefault::Scalar("C-b")),
                ("renumber-windows", TmuxOptionDefault::Scalar("off")),
                ("status", TmuxOptionDefault::Scalar("on")),
                ("status-interval", TmuxOptionDefault::Scalar("15")),
                (
                    "status-left",
                    TmuxOptionDefault::String("[#{session_name}] ")
                ),
                (
                    "status-right",
                    TmuxOptionDefault::String(
                        "#{?window_bigger,[#{window_offset_x}#,#{window_offset_y}] ,}\"#{=21:pane_title}\" %H:%M %d-%b-%y",
                    ),
                ),
                (
                    "word-separators",
                    TmuxOptionDefault::String("!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~"),
                ),
                ("mode-keys", TmuxOptionDefault::Scalar("emacs")),
                ("pane-base-index", TmuxOptionDefault::Scalar("0")),
                ("synchronize-panes", TmuxOptionDefault::Scalar("off")),
            ]
        );
    }
}
