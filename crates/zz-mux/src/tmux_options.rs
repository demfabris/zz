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
pub(crate) const MESSAGE_COMMAND_STYLE_DEFAULT: &str = "bg=themeblack,fg=themeyellow,#{?#{m/r:(^|#,)IS(PANE|MODE)($|#,),#{prompt_flags}},,fill=themeblack}";
pub(crate) const MESSAGE_FORMAT_DEFAULT: &str =
    "#[#{?#{command_prompt},#{E:message-command-style},#{E:message-style}}]#{message}";
pub(crate) const MESSAGE_STYLE_DEFAULT: &str = "bg=themeyellow,fg=themeblack,#{?#{m/r:(^|#,)IS(PANE|MODE)($|#,),#{prompt_flags}},,fill=themeyellow}";
pub(crate) const PANE_SCROLLBARS_STYLE_DEFAULT: &str =
    "bg=themedarkgrey,fg=themelightgrey,width=1,pad=0";
const COMMAND_ALIAS_DEFAULTS: &[&str] = &[
    "split-pane=split-window",
    "splitp=split-window",
    "server-info=show-messages -JT",
    "info=show-messages -JT",
    "choose-window=choose-tree -w",
    "choose-session=choose-tree -s",
];
const TERMINAL_FEATURE_DEFAULTS: &[&str] = &[
    "xterm*:clipboard:ccolour:cstyle:focus:title",
    "screen*:title",
    "rxvt*:ignorefkeys",
];
const TERMINAL_OVERRIDE_DEFAULTS: &[&str] = &["linux*:AX@"];
const UPDATE_ENVIRONMENT_DEFAULTS: &[&str] = &[
    "DISPLAY",
    "KRB5CCNAME",
    "MSYSTEM",
    "SSH_ASKPASS",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "SSH_CONNECTION",
    "WAYLAND_DISPLAY",
    "WINDOWID",
    "XAUTHORITY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_TYPE",
];
const STATUS_FORMAT_DEFAULTS: &[&str] = &[
    concat!(
        "#[align=left range=left #{E:status-left-style}]",
        "#[push-default]",
        "#{T;=/#{status-left-length}:status-left}",
        "#[pop-default]",
        "#[norange default]",
        "#[list=on align=#{status-justify}]",
        "#[list=left-marker]<#[list=right-marker]>#[list=on]",
        "#{W:",
        "#[range=window|#{window_index} ",
        "#{E:window-status-style}",
        "#{?#{&&:#{window_last_flag},",
        "#{!=:#{E:window-status-last-style},default}}, ",
        "#{E:window-status-last-style},",
        "}",
        "#{?#{&&:#{window_bell_flag},",
        "#{!=:#{E:window-status-bell-style},default}}, ",
        "#{E:window-status-bell-style},",
        "#{?#{&&:#{||:#{window_activity_flag},",
        "#{window_silence_flag}},",
        "#{!=:",
        "#{E:window-status-activity-style},",
        "default}}, ",
        "#{E:window-status-activity-style},",
        "}",
        "}",
        "]",
        "#[push-default]",
        "#{T:window-status-format}",
        "#[pop-default]",
        "#[norange default]",
        "#{?loop_last_flag,,#{E:window-status-separator}}",
        ",",
        "#[range=window|#{window_index} list=focus ",
        "#{?#{!=:#{E:window-status-current-style},default},",
        "#{E:window-status-current-style},",
        "#{E:window-status-style}",
        "}",
        "#{?#{&&:#{window_last_flag},",
        "#{!=:#{E:window-status-last-style},default}}, ",
        "#{E:window-status-last-style},",
        "}",
        "#{?#{&&:#{window_bell_flag},",
        "#{!=:#{E:window-status-bell-style},default}}, ",
        "#{E:window-status-bell-style},",
        "#{?#{&&:#{||:#{window_activity_flag},",
        "#{window_silence_flag}},",
        "#{!=:",
        "#{E:window-status-activity-style},",
        "default}}, ",
        "#{E:window-status-activity-style},",
        "}",
        "}",
        "]",
        "#[push-default]",
        "#{T:window-status-current-format}",
        "#[pop-default]",
        "#[norange list=on default]",
        "#{?loop_last_flag,,#{E:window-status-separator}}",
        "}",
        "#[nolist align=right range=right #{E:status-right-style}]",
        "#[push-default]",
        "#{T;=/#{status-right-length}:status-right}",
        "#[pop-default]",
        "#[norange default]",
    ),
    concat!(
        "#[align=left]#{R: ,#{n:#{session_name}}}P: ",
        "#[norange default]",
        "#[list=on align=#{status-justify}]",
        "#[list=left-marker]<#[list=right-marker]>#[list=on]",
        "#{P:",
        "#[range=pane|#{pane_id} ",
        "#{E:pane-status-style}",
        "]",
        "#[push-default]",
        "#{T:window-pane-status-format}",
        "#[pop-default]",
        "#[norange list=on default]  ",
        ",",
        "#[range=pane|#{pane_id} list=focus ",
        "#{?#{!=:#{E:pane-status-current-style},default},",
        "#{E:pane-status-current-style},",
        "#{E:pane-status-style}",
        "}",
        "]",
        "#[push-default]",
        "#{T:window-pane-current-status-format}",
        "#[pop-default]",
        "#[norange list=on default] ",
        "}",
    ),
    concat!(
        "#[align=left]#{R: ,#{n:#{session_name}}}S: ",
        "#[norange default]",
        "#[list=on align=#{status-justify}]",
        "#[list=left-marker]<#[list=right-marker]>#[list=on]",
        "#{S:",
        "#[range=session|#{session_id} ",
        "#{E:session-status-style}",
        "]",
        "#[push-default]",
        "#S#{session_alert}",
        "#[pop-default]",
        "#[norange list=on default]  ",
        ",",
        "#[range=session|#{session_id} list=focus ",
        "#{?#{!=:#{E:session-status-current-style},default},",
        "#{E:session-status-current-style},",
        "#{E:session-status-style}",
        "}",
        "]",
        "#[push-default]",
        "#S*#{session_alert}",
        "#[pop-default]",
        "#[norange list=on default] ",
        "}",
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TmuxArrayValue {
    String,
    Colour,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TmuxArray {
    pub(crate) defaults: &'static [&'static str],
    pub(crate) separators: &'static str,
    pub(crate) value: TmuxArrayValue,
}

pub(crate) fn tmux_stored_array(name: &str) -> Option<TmuxArray> {
    let array = match name {
        "command-alias" => TmuxArray {
            defaults: COMMAND_ALIAS_DEFAULTS,
            separators: ",",
            value: TmuxArrayValue::String,
        },
        "codepoint-widths" | "user-keys" => TmuxArray {
            defaults: &[],
            separators: ",",
            value: TmuxArrayValue::String,
        },
        "terminal-overrides" => TmuxArray {
            defaults: TERMINAL_OVERRIDE_DEFAULTS,
            separators: ",",
            value: TmuxArrayValue::String,
        },
        "terminal-features" => TmuxArray {
            defaults: TERMINAL_FEATURE_DEFAULTS,
            separators: ",",
            value: TmuxArrayValue::String,
        },
        "status-format" => TmuxArray {
            defaults: STATUS_FORMAT_DEFAULTS,
            separators: " ,",
            value: TmuxArrayValue::String,
        },
        "pane-colours" => TmuxArray {
            defaults: &[],
            separators: " ,",
            value: TmuxArrayValue::Colour,
        },
        "update-environment" => TmuxArray {
            defaults: UPDATE_ENVIRONMENT_DEFAULTS,
            separators: " ,",
            value: TmuxArrayValue::String,
        },
        _ => return None,
    };
    Some(array)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TmuxStoredScalarKind {
    String,
    Style,
    Colour,
    Flag,
    Choice(&'static [&'static str]),
    Key,
}

impl TmuxStoredScalarKind {
    pub(crate) const fn is_string(self) -> bool {
        matches!(self, Self::String | Self::Style | Self::Colour)
    }

    pub(crate) const fn append_separator(self) -> Option<&'static str> {
        match self {
            Self::String | Self::Colour => Some(""),
            Self::Style => Some(","),
            Self::Flag | Self::Choice(_) | Self::Key => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TmuxStoredScalar {
    pub(crate) default: &'static str,
    pub(crate) kind: TmuxStoredScalarKind,
}

impl TmuxStoredScalar {
    pub(crate) const fn option_default(self) -> TmuxOptionDefault {
        if self.kind.is_string() {
            TmuxOptionDefault::String(self.default)
        } else {
            TmuxOptionDefault::Scalar(self.default)
        }
    }
}

pub(crate) fn tmux_stored_scalar(name: &str) -> Option<TmuxStoredScalar> {
    use TmuxStoredScalarKind::{Choice, Colour, Flag, Key, String, Style};

    let (default, kind) = match name {
        "theme" => ("detect", Choice(&["detect", "terminal", "light", "dark"])),
        "dark-theme-black" => ("#{?#{e|>=:#{client_colours},256},gray5,black}", Colour),
        "dark-theme-white" => ("#{?#{e|>=:#{client_colours},256},gray90,white}", Colour),
        "dark-theme-light-grey" => ("#{?#{e|>=:#{client_colours},256},gray70,white}", Colour),
        "dark-theme-dark-grey" => ("#{?#{e|>=:#{client_colours},256},gray15,black}", Colour),
        "dark-theme-green" => (
            "#{?#{e|>=:#{client_colours},256},yellowgreen,green}",
            Colour,
        ),
        "dark-theme-yellow" | "light-theme-yellow" => (
            "#{?#{e|>=:#{client_colours},256},darkgoldenrod,yellow}",
            Colour,
        ),
        "dark-theme-red" => ("#{?#{e|>=:#{client_colours},256},indianred,red}", Colour),
        "dark-theme-blue" => ("#{?#{e|>=:#{client_colours},256},skyblue3,blue}", Colour),
        "dark-theme-cyan" => ("#{?#{e|>=:#{client_colours},256},cadetblue,cyan}", Colour),
        "dark-theme-magenta" => (
            "#{?#{e|>=:#{client_colours},256},mediumpurple,magenta}",
            Colour,
        ),
        "light-theme-black" => ("#{?#{e|>=:#{client_colours},256},gray10,black}", Colour),
        "light-theme-white" => ("#{?#{e|>=:#{client_colours},256},gray95,white}", Colour),
        "light-theme-light-grey" => ("#{?#{e|>=:#{client_colours},256},gray80,white}", Colour),
        "light-theme-dark-grey" => ("#{?#{e|>=:#{client_colours},256},gray45,black}", Colour),
        "light-theme-green" => ("#{?#{e|>=:#{client_colours},256},seagreen,green}", Colour),
        "light-theme-red" => ("#{?#{e|>=:#{client_colours},256},indianred4,red}", Colour),
        "light-theme-blue" => ("#{?#{e|>=:#{client_colours},256},steelblue,blue}", Colour),
        "light-theme-cyan" => ("#{?#{e|>=:#{client_colours},256},darkcyan,cyan}", Colour),
        "light-theme-magenta" => ("#{?#{e|>=:#{client_colours},256},purple4,magenta}", Colour),
        "exit-empty" => ("on", Flag),
        // prefix2 and the set-titles pair are storage-only until C3 owns behavior and wire.
        "exit-unattached" | "focus-follows-mouse" | "set-titles" => ("off", Flag),
        "destroy-unattached" => ("off", Choice(&["off", "on", "keep-last", "keep-group"])),
        "detach-on-destroy" => (
            "on",
            Choice(&["off", "on", "no-detached", "previous", "next"]),
        ),
        "display-panes-active-colour" => ("themered", Colour),
        "display-panes-colour" => ("themeblue", Colour),
        "display-panes-format" => ("#[align=right]#{pane_width}x#{pane_height}", String),
        "prefix2" => ("None", Key),
        "set-titles-string" => ("#S:#I:#W - \"#T\" #{session_alerts}", String),
        "status-keys" => ("emacs", Choice(&["emacs", "vi"])),
        "visual-activity" | "visual-silence" => ("off", Choice(&["off", "on", "both"])),
        "copy-mode-match-style" => ("bg=themecyan,fg=themeblack", Style),
        "copy-mode-current-match-style" => ("bg=thememagenta,fg=themeblack", Style),
        "copy-mode-mark-style" => ("bg=themeyellow,fg=themeblack", Style),
        "copy-mode-position-format" => (
            concat!(
                "#[align=right]#{t/p:top_line_time}",
                "#{?#{e|>:#{top_line_time},0}, ,}",
                "[#{copy_position}/#{copy_position_limit}]",
                "#{?search_timed_out, (timed out),",
                "#{?search_count, (#{search_count}",
                "#{?search_count_partial,+,} results),}}",
            ),
            String,
        ),
        "copy-mode-position-style" | "copy-mode-selection-style" | "tree-mode-selection-style" => {
            ("#{E:mode-style}", Style)
        }
        "copy-mode-current-line-number-style" => ("fg=themeyellow", Style),
        "copy-mode-line-number-style" => ("fg=themelightgrey,dim", Style),
        "copy-mode-line-numbers" => (
            "off",
            Choice(&["off", "default", "absolute", "relative", "hybrid"]),
        ),
        "mode-style" => ("noattr,bg=themeyellow,fg=themeblack", Style),
        "pane-active-border-style" => (
            concat!(
                "fg=#{?pane_marked,thememagenta,",
                "#{?synchronize-panes,themered,",
                "#{?pane_in_mode,themeyellow,themegreen}}}",
            ),
            Style,
        ),
        "pane-border-format" => (
            concat!(
                "#{?pane_active,#[reverse],}#{pane_index}#[default] \"#{pane_title}\"",
                "#{?#{mouse},#[align=right]#[range=control|7]",
                "[#{?#{pane_floating_flag},t,f}]#[norange]",
                "#[range=control|8][#{?#{window_zoomed_flag},u,z}]#[norange]",
                "#[range=control|9][x]#[norange],}",
            ),
            String,
        ),
        "pane-border-status" => (
            "off",
            Choice(&["off", "top", "bottom", "top-floating", "bottom-floating"]),
        ),
        "pane-border-style" => ("fg=themelightgrey", Style),
        "pane-status-current-style" | "session-status-current-style" => ("underscore", Style),
        "pane-status-style" | "session-status-style" | "window-active-style" | "window-style" => {
            ("default", Style)
        }
        "remain-on-exit-format" => (
            concat!(
                "Pane is dead (#{?#{!=:#{pane_dead_status},},",
                "status #{pane_dead_status},}",
                "#{?#{!=:#{pane_dead_signal},},signal #{pane_dead_signal},}, ",
                "#{t:pane_dead_time})",
            ),
            String,
        ),
        "switch-mode-match-style" => ("bg=cyan fg=black", Style),
        "tree-mode-border-style" => ("bg=themedarkgrey,fg=themelightgrey", Style),
        "tree-mode-preview-format" => (
            "#{?pane_format,#{pane_index}:#{pane_title},#{window_index}:#{window_name}}",
            String,
        ),
        "tree-mode-preview-style" => (
            concat!(
                "fg=#{?#{||:#{&&:#{pane_format},#{pane_active}},",
                "#{&&:#{window_format},#{window_active}}},themered,themeblue}",
            ),
            Style,
        ),
        "window-pane-current-status-format" | "window-pane-status-format" => {
            ("#P:[#T]#{?pane_flags,#{pane_flags}, }", String)
        }
        _ => return None,
    };
    Some(TmuxStoredScalar { default, kind })
}
pub(crate) const HOOK_NAMES: &[&str] = &[
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
    "client-active",
    "client-attached",
    "client-detached",
    "client-focus-in",
    "client-focus-out",
    "client-resized",
    "client-session-changed",
    "client-light-theme",
    "client-dark-theme",
    "command-error",
    "pane-died",
    "pane-exited",
    "pane-focus-in",
    "pane-focus-out",
    "pane-mode-changed",
    "pane-set-clipboard",
    "pane-title-changed",
    "session-closed",
    "session-created",
    "session-renamed",
    "session-window-changed",
    "window-layout-changed",
    "window-linked",
    "window-pane-changed",
    "window-renamed",
    "window-resized",
    "window-unlinked",
];

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

const OPTION_TABLE_ORDER: &[&str] = &[
    "backspace",
    "buffer-limit",
    "command-alias",
    "codepoint-widths",
    "copy-command",
    "cursor-colour",
    "cursor-style",
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
    "menu-style",
    "menu-selected-style",
    "menu-border-style",
    "menu-border-lines",
    "message-limit",
    "prefix-timeout",
    "prompt-history-limit",
    "set-clipboard",
    "terminal-overrides",
    "terminal-features",
    "theme",
    "dark-theme-black",
    "dark-theme-white",
    "dark-theme-light-grey",
    "dark-theme-dark-grey",
    "dark-theme-green",
    "dark-theme-yellow",
    "dark-theme-red",
    "dark-theme-blue",
    "dark-theme-cyan",
    "dark-theme-magenta",
    "light-theme-black",
    "light-theme-white",
    "light-theme-light-grey",
    "light-theme-dark-grey",
    "light-theme-green",
    "light-theme-yellow",
    "light-theme-red",
    "light-theme-blue",
    "light-theme-cyan",
    "light-theme-magenta",
    "user-keys",
    "variation-selector-always-wide",
    "activity-action",
    "assume-paste-time",
    "base-index",
    "bell-action",
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
    "renumber-windows",
    "repeat-time",
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
    "pane-status-current-style",
    "pane-status-style",
    "prompt-cursor-colour",
    "prompt-command-cursor-colour",
    "prompt-cursor-style",
    "prompt-command-cursor-style",
    "session-status-current-style",
    "session-status-style",
    "update-environment",
    "visual-activity",
    "visual-bell",
    "visual-silence",
    "word-separators",
    "aggressive-resize",
    "allow-passthrough",
    "allow-rename",
    "allow-set-title",
    "alternate-screen",
    "automatic-rename",
    "automatic-rename-format",
    "clock-mode-colour",
    "clock-mode-style",
    "copy-mode-match-style",
    "copy-mode-current-match-style",
    "copy-mode-mark-style",
    "copy-mode-position-format",
    "copy-mode-position-style",
    "copy-mode-selection-style",
    "copy-mode-current-line-number-style",
    "copy-mode-line-number-style",
    "copy-mode-line-numbers",
    "fill-character",
    "main-pane-height",
    "main-pane-width",
    "mode-keys",
    "mode-style",
    "monitor-activity",
    "monitor-bell",
    "monitor-silence",
    "other-pane-height",
    "other-pane-width",
    "pane-active-border-style",
    "pane-base-index",
    "pane-border-format",
    "pane-border-indicators",
    "pane-border-lines",
    "pane-border-status",
    "pane-border-style",
    "pane-colours",
    "pane-scrollbars",
    "pane-scrollbars-timeout",
    "pane-scrollbars-style",
    "pane-scrollbars-position",
    "popup-style",
    "popup-border-style",
    "popup-border-lines",
    "remain-on-exit",
    "remain-on-exit-format",
    "scroll-on-clear",
    "switch-mode-match-style",
    "synchronize-panes",
    "tiled-layout-max-columns",
    "tree-mode-border-style",
    "tree-mode-preview-format",
    "tree-mode-preview-style",
    "tree-mode-selection-style",
    "window-active-style",
    "window-pane-current-status-format",
    "window-pane-status-format",
    "window-size",
    "window-style",
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

pub(crate) fn tmux_option_table_order(name: &str) -> usize {
    OPTION_TABLE_ORDER
        .iter()
        .position(|candidate| *candidate == name)
        .unwrap_or(usize::MAX)
}

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
        "backspace" => TmuxOptionDefault::Scalar("C-?"),
        "default-client-command" => TmuxOptionDefault::Scalar("new-session"),
        "default-terminal" => TmuxOptionDefault::String("tmux-256color"),
        "editor" => TmuxOptionDefault::String("/usr/bin/vi"),
        "escape-time" => TmuxOptionDefault::Scalar("10"),
        "extended-keys-format" => TmuxOptionDefault::Scalar("xterm"),
        "get-clipboard" => TmuxOptionDefault::Scalar("buffer"),
        "input-buffer-size" => TmuxOptionDefault::Scalar("1048576"),
        "prompt-history-limit" => TmuxOptionDefault::Scalar("100"),
        "activity-action" | "silence-action" => TmuxOptionDefault::Scalar("other"),
        "assume-paste-time" => TmuxOptionDefault::Scalar("1"),
        "base-index"
        | "initial-repeat-time"
        | "lock-after-time"
        | "message-line"
        | "monitor-silence"
        | "pane-base-index"
        | "prefix-timeout"
        | "tiled-layout-max-columns" => TmuxOptionDefault::Scalar("0"),
        "bell-action" => TmuxOptionDefault::Scalar("any"),
        "buffer-limit" => TmuxOptionDefault::Scalar("50"),
        "copy-command"
        | "cursor-colour"
        | "default-command"
        | "fill-character"
        | "history-file"
        | "prompt-command-cursor-colour"
        | "prompt-cursor-colour" => TmuxOptionDefault::String(""),
        "default-size" => TmuxOptionDefault::String("80x24"),
        "default-shell" => TmuxOptionDefault::String("/bin/sh"),
        "display-panes-time" | "message-limit" => TmuxOptionDefault::Scalar("1000"),
        "history-limit" => TmuxOptionDefault::Scalar("2000"),
        "display-time" => TmuxOptionDefault::Scalar("750"),
        "key-table" => TmuxOptionDefault::String("root"),
        "mode-keys" => TmuxOptionDefault::Scalar("emacs"),
        "main-pane-height" => TmuxOptionDefault::String("24"),
        "main-pane-width" => TmuxOptionDefault::String("80"),
        "other-pane-height" | "other-pane-width" => TmuxOptionDefault::String("0"),
        "window-size" => TmuxOptionDefault::Scalar("latest"),
        "lock-command" => TmuxOptionDefault::String("lock -np"),
        "message-command-style" => TmuxOptionDefault::String(MESSAGE_COMMAND_STYLE_DEFAULT),
        "message-format" => TmuxOptionDefault::String(MESSAGE_FORMAT_DEFAULT),
        "message-style" => TmuxOptionDefault::String(MESSAGE_STYLE_DEFAULT),
        "menu-border-lines" | "pane-border-lines" | "popup-border-lines" => {
            TmuxOptionDefault::Scalar("single")
        }
        "menu-border-style" | "popup-border-style" => {
            TmuxOptionDefault::String("bg=themedarkgrey,fg=themelightgrey")
        }
        "menu-selected-style" => TmuxOptionDefault::String("bg=themeyellow,fg=themeblack"),
        "menu-style" | "popup-style" => TmuxOptionDefault::String("bg=themedarkgrey,fg=themewhite"),
        "aggressive-resize" | "allow-passthrough" | "allow-rename" | "extended-keys"
        | "focus-events" | "monitor-activity" | "pane-scrollbars" | "remain-on-exit"
        | "renumber-windows" | "synchronize-panes" | "visual-bell" => {
            TmuxOptionDefault::Scalar("off")
        }
        "cursor-style" => TmuxOptionDefault::Scalar("default"),
        "prompt-cursor-style" | "prompt-command-cursor-style" => {
            TmuxOptionDefault::Scalar("default")
        }
        "prefix" => TmuxOptionDefault::Scalar("C-b"),
        "pane-scrollbars-timeout" | "repeat-time" => TmuxOptionDefault::Scalar("500"),
        "set-clipboard" => TmuxOptionDefault::Scalar("external"),
        "allow-set-title"
        | "alternate-screen"
        | "automatic-rename"
        | "mouse"
        | "monitor-bell"
        | "scroll-on-clear"
        | "status"
        | "variation-selector-always-wide"
        | "wrap-search"
        | "xterm-keys" => TmuxOptionDefault::Scalar("on"),
        "status-interval" => TmuxOptionDefault::Scalar("15"),
        "status-left" => TmuxOptionDefault::String(STATUS_LEFT_DEFAULT),
        "status-right" => TmuxOptionDefault::String(STATUS_RIGHT_DEFAULT),
        "update-environment" => TmuxOptionDefault::Array(UPDATE_ENVIRONMENT_DEFAULT),
        "word-separators" => TmuxOptionDefault::String("!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~"),
        "automatic-rename-format" => TmuxOptionDefault::String(
            "#{?pane_in_mode,[tmux],#{pane_current_command}}#{?pane_dead,[dead],}",
        ),
        "clock-mode-colour" => TmuxOptionDefault::String("themeblue"),
        "clock-mode-style" => TmuxOptionDefault::Scalar("24"),
        "pane-border-indicators" => TmuxOptionDefault::Scalar("colour"),
        "pane-scrollbars-position" => TmuxOptionDefault::Scalar("right"),
        "pane-scrollbars-style" => TmuxOptionDefault::String(PANE_SCROLLBARS_STYLE_DEFAULT),
        _ => return tmux_stored_scalar(name).map(TmuxStoredScalar::option_default),
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
    ) || tmux_option_is_hook(name)
}

pub(crate) fn tmux_option_is_hook(name: &str) -> bool {
    HOOK_NAMES.contains(&name)
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
        assert_eq!(
            tmux_options()
                .filter(|option| tmux_stored_array(option.name).is_some())
                .map(|option| option.name)
                .collect::<BTreeSet<_>>(),
            [
                "command-alias",
                "codepoint-widths",
                "pane-colours",
                "status-format",
                "terminal-features",
                "terminal-overrides",
                "update-environment",
                "user-keys",
            ]
            .into_iter()
            .collect()
        );
        let hooks = tmux_options()
            .filter(|option| tmux_option_is_hook(option.name))
            .map(|option| option.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(hooks.len(), 68);
        assert_eq!(hooks, HOOK_NAMES.iter().copied().collect());
    }

    #[test]
    fn listing_order_covers_every_non_hook_table_option_once() {
        assert_eq!(OPTION_TABLE_ORDER.len(), 180);
        assert_eq!(
            OPTION_TABLE_ORDER.iter().copied().collect::<BTreeSet<_>>(),
            tmux_options()
                .filter(|option| !tmux_option_is_hook(option.name))
                .map(|option| option.name)
                .collect()
        );
    }

    #[test]
    fn every_named_option_has_storage_metadata() {
        let named = tmux_options()
            .filter(|option| !tmux_option_is_hook(option.name))
            .collect::<Vec<_>>();
        assert_eq!(named.len(), 180);
        assert!(named.iter().all(|option| {
            if option.is_array {
                tmux_stored_array(option.name).is_some()
            } else {
                option.default.is_some()
                    || matches!(
                        option.name,
                        "status-bg"
                            | "status-fg"
                            | "status-justify"
                            | "status-left-length"
                            | "status-left-style"
                            | "status-position"
                            | "status-right-length"
                            | "status-right-style"
                            | "status-style"
                            | "window-status-format"
                            | "window-status-current-format"
                            | "window-status-separator"
                            | "window-status-style"
                            | "window-status-current-style"
                            | "window-status-last-style"
                            | "window-status-bell-style"
                            | "window-status-activity-style"
                    )
            }
        }));
    }

    #[test]
    fn implemented_options_carry_their_pin_defaults_and_types() {
        let implemented = tmux_options()
            .filter_map(|option| option.default.map(|default| (option.name, default)))
            .collect::<std::collections::BTreeMap<_, _>>();
        for (name, expected) in [
            ("backspace", TmuxOptionDefault::Scalar("C-?")),
            (
                "default-client-command",
                TmuxOptionDefault::Scalar("new-session"),
            ),
            ("editor", TmuxOptionDefault::String("/usr/bin/vi")),
            ("extended-keys", TmuxOptionDefault::Scalar("off")),
            ("extended-keys-format", TmuxOptionDefault::Scalar("xterm")),
            ("get-clipboard", TmuxOptionDefault::Scalar("buffer")),
            ("input-buffer-size", TmuxOptionDefault::Scalar("1048576")),
            (
                "variation-selector-always-wide",
                TmuxOptionDefault::Scalar("on"),
            ),
            ("assume-paste-time", TmuxOptionDefault::Scalar("1")),
            (
                "message-command-style",
                TmuxOptionDefault::String(MESSAGE_COMMAND_STYLE_DEFAULT),
            ),
            (
                "message-format",
                TmuxOptionDefault::String(MESSAGE_FORMAT_DEFAULT),
            ),
            ("message-line", TmuxOptionDefault::Scalar("0")),
            (
                "message-style",
                TmuxOptionDefault::String(MESSAGE_STYLE_DEFAULT),
            ),
            (
                "prompt-command-cursor-colour",
                TmuxOptionDefault::String(""),
            ),
            (
                "prompt-command-cursor-style",
                TmuxOptionDefault::Scalar("default"),
            ),
            ("prompt-cursor-colour", TmuxOptionDefault::String("")),
            ("prompt-cursor-style", TmuxOptionDefault::Scalar("default")),
            ("clock-mode-colour", TmuxOptionDefault::String("themeblue")),
            ("clock-mode-style", TmuxOptionDefault::Scalar("24")),
            ("fill-character", TmuxOptionDefault::String("")),
            (
                "pane-border-indicators",
                TmuxOptionDefault::Scalar("colour"),
            ),
            ("pane-border-lines", TmuxOptionDefault::Scalar("single")),
            ("pane-scrollbars", TmuxOptionDefault::Scalar("off")),
            (
                "pane-scrollbars-position",
                TmuxOptionDefault::Scalar("right"),
            ),
            (
                "pane-scrollbars-style",
                TmuxOptionDefault::String(PANE_SCROLLBARS_STYLE_DEFAULT),
            ),
            ("pane-scrollbars-timeout", TmuxOptionDefault::Scalar("500")),
            ("xterm-keys", TmuxOptionDefault::Scalar("on")),
        ] {
            assert_eq!(implemented.get(name), Some(&expected), "{name}");
        }
        assert_eq!(
            implemented.get("update-environment"),
            Some(&TmuxOptionDefault::Array(UPDATE_ENVIRONMENT_DEFAULT))
        );
    }
}
