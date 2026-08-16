//! Shared metadata for tmux-compatible commands implemented by `zz`.

/// The kind of value accepted by an option or positional argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandValueKind {
    /// Arbitrary user-provided text. No value completions are produced.
    FreeForm,
    Session,
    Window,
    Pane,
    Layout,
    PaneKind,
    KeyTable,
    SetOption,
    Boolean,
}

/// One flag or value option accepted by a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandOptionSpec {
    pub name: &'static str,
    pub value: Option<CommandValueKind>,
    pub description: &'static str,
    /// Whether the option is part of the implemented completion surface.
    pub completable: bool,
    /// tmux's `X::` optional-argument form: the option takes a value only when
    /// it is attached (`-R10`); a bare `-R` stays a flag and never consumes the
    /// next argument.
    pub attached_value: bool,
    /// Whether the option is catalogued only so its value can be rejected.
    pub unsupported: bool,
}

impl CommandOptionSpec {
    const fn flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            value: None,
            description,
            completable: true,
            attached_value: false,
            unsupported: false,
        }
    }

    const fn attached_flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            value: None,
            description,
            completable: true,
            attached_value: true,
            unsupported: false,
        }
    }

    const fn value(name: &'static str, value: CommandValueKind, description: &'static str) -> Self {
        Self {
            name,
            value: Some(value),
            description,
            completable: true,
            attached_value: false,
            unsupported: false,
        }
    }

    const fn unsupported_value(name: &'static str) -> Self {
        Self {
            name,
            value: Some(CommandValueKind::FreeForm),
            description: "unsupported tmux option",
            completable: false,
            attached_value: false,
            unsupported: true,
        }
    }

    const fn unsupported_flag(name: &'static str) -> Self {
        Self {
            name,
            value: None,
            description: "unsupported tmux flag",
            completable: false,
            attached_value: false,
            unsupported: true,
        }
    }

    const fn unsupported_attached(name: &'static str) -> Self {
        Self {
            name,
            value: None,
            description: "unsupported tmux option",
            completable: false,
            attached_value: true,
            unsupported: true,
        }
    }
}

/// Completion and canonicalization metadata for one implemented command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub options: &'static [CommandOptionSpec],
    /// Kinds for fixed positional arguments, in order.
    pub positionals: &'static [CommandValueKind],
    /// Kind accepted by all remaining positional arguments.
    pub variadic: Option<CommandValueKind>,
}

impl CommandSpec {
    pub const DAEMON_COMMAND_NAMES: &'static [&'static str] = crate::catalog::DAEMON_COMMAND_NAMES;
    pub const UNIMPLEMENTED_TMUX_COMMANDS: &'static [&'static str] =
        crate::catalog::UNIMPLEMENTED_TMUX_COMMANDS;

    #[must_use]
    pub fn positional_kind(&self, index: usize) -> Option<CommandValueKind> {
        self.positionals.get(index).copied().or(self.variadic)
    }

    #[must_use]
    pub fn option(&self, name: &str) -> Option<&CommandOptionSpec> {
        self.options.iter().find(|option| option.name == name)
    }
}

use CommandValueKind::{
    Boolean, FreeForm, KeyTable, Layout, Pane, PaneKind, Session, SetOption, Window,
};

pub static DAEMON_COMMAND_NAMES: &[&str] = &[
    "capture-pane",
    "capturep",
    "agent-send",
    "send-last-output",
    "capture-browser",
    "debug-marker",
    "tools",
    "set-buffer",
    "setb",
    "show-buffer",
    "showb",
    "list-buffers",
    "lsb",
    "load-buffer",
    "loadb",
    "save-buffer",
    "saveb",
    "delete-buffer",
    "deleteb",
    "paste-buffer",
    "pasteb",
];

pub static UNIMPLEMENTED_TMUX_COMMANDS: &[&str] = &[
    "new-pane",
    "newp",
    "run-shell",
    "run",
    "if-shell",
    "if",
    "set-hook",
    "show-hooks",
    "wait-for",
    "wait",
    "pipe-pane",
    "pipep",
    "lock-client",
    "lockc",
    "lock-server",
    "lock",
    "lock-session",
    "locks",
    "server-access",
    "start-server",
    "start",
    "display-popup",
    "popup",
    "display-menu",
    "menu",
    "confirm-before",
    "confirm",
    "customize-mode",
    "choose-client",
    "clock-mode",
    "refresh-client",
    "refresh",
    "suspend-client",
    "suspendc",
    "switch-client",
    "switchc",
    "show-options",
    "show",
    "show-window-options",
    "showw",
    "move-window",
    "movew",
    "swap-window",
    "swapw",
    "set-environment",
    "setenv",
    "show-environment",
    "showenv",
    "respawn-pane",
    "respawnp",
    "respawn-window",
    "respawnw",
    "find-window",
    "findw",
    "list-clients",
    "lsc",
    "list-commands",
    "lscm",
    "link-window",
    "linkw",
    "unlink-window",
    "unlinkw",
    "resize-window",
    "resizew",
    "show-messages",
    "showmsgs",
    "clear-prompt-history",
    "clearphist",
    "show-prompt-history",
    "showphist",
    "switch-mode",
];

/// Every tmux-compatible command currently executable by the mux engine.
pub static COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "new-session",
        aliases: &["new"],
        description: "Create a new session",
        options: &[
            CommandOptionSpec::flag("-d", "do not attach"),
            CommandOptionSpec::flag("-A", "attach to the named session when it exists"),
            CommandOptionSpec::flag("-D", "with -A, detach other clients"),
            CommandOptionSpec::unsupported_flag("-E"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_flag("-X"),
            CommandOptionSpec::value("-s", FreeForm, "session name"),
            CommandOptionSpec::value("-n", FreeForm, "initial window name"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::unsupported_value("-e"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_value("-x"),
            CommandOptionSpec::unsupported_value("-y"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "list-sessions",
        aliases: &["ls"],
        description: "List sessions",
        options: &[
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_flag("-r"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "rename-session",
        aliases: &["rename"],
        description: "Rename a session",
        options: &[CommandOptionSpec::value("-t", Session, "target session")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "kill-session",
        aliases: &[],
        description: "Destroy a session",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::flag("-a", "kill every other session"),
            CommandOptionSpec::flag("-C", "clear alerts in the session instead of killing"),
            CommandOptionSpec::unsupported_flag("-g"),
            CommandOptionSpec::unsupported_value("-f"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "attach-session",
        aliases: &["attach"],
        description: "Attach to a session",
        options: &[
            CommandOptionSpec::flag("-d", "detach other clients"),
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::unsupported_value("-c"),
            CommandOptionSpec::unsupported_flag("-E"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_flag("-r"),
            CommandOptionSpec::unsupported_flag("-x"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "has-session",
        aliases: &["has"],
        description: "Check whether a session exists",
        options: &[CommandOptionSpec::value("-t", Session, "target session")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "detach-client",
        aliases: &["detach"],
        description: "Detach the current client",
        options: &[
            CommandOptionSpec::flag("-a", "detach every other client"),
            CommandOptionSpec::value("-s", Session, "detach every client on the session"),
            CommandOptionSpec::unsupported_value("-E"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::unsupported_flag("-P"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "new-window",
        aliases: &["neww"],
        description: "Create a terminal window",
        options: &[
            CommandOptionSpec::value("-t", Window, "destination session or window index"),
            CommandOptionSpec::value("-n", FreeForm, "window name"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
            CommandOptionSpec::flag("-d", "do not select the new window"),
            CommandOptionSpec::flag("-a", "insert after the target window"),
            CommandOptionSpec::flag("-k", "replace the window at the target index"),
            CommandOptionSpec::flag("-S", "select an existing window with the same name"),
            CommandOptionSpec::unsupported_flag("-b"),
            CommandOptionSpec::unsupported_flag("-E"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_value("-e"),
            CommandOptionSpec::unsupported_value("-F"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "new-browser",
        aliases: &[],
        description: "Create a browser window",
        options: &[
            CommandOptionSpec::value("-t", Window, "destination session or window index"),
            CommandOptionSpec::value("-n", FreeForm, "window name"),
            CommandOptionSpec::value("-p", FreeForm, "browser profile"),
            CommandOptionSpec::flag("-d", "do not select the new window"),
            CommandOptionSpec::flag("-a", "insert after the target window"),
            CommandOptionSpec::flag("-k", "replace the window at the target index"),
            CommandOptionSpec::flag("-S", "select an existing window with the same name"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "list-windows",
        aliases: &["lsw"],
        description: "List windows",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_flag("-r"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "rename-window",
        aliases: &["renamew"],
        description: "Rename a window",
        options: &[CommandOptionSpec::value("-t", Window, "target window")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "select-window",
        aliases: &["selectw"],
        description: "Select a window",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-l", "select the last window"),
            CommandOptionSpec::flag("-n", "select the next window"),
            CommandOptionSpec::flag("-p", "select the previous window"),
            CommandOptionSpec::flag("-T", "select the last window when already current"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "next-window",
        aliases: &["next"],
        description: "Select the next window",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::flag("-a", "select the next window with an alert"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "previous-window",
        aliases: &["prev"],
        description: "Select the previous window",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::flag("-a", "select the previous window with an alert"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "last-window",
        aliases: &["last"],
        description: "Select the previously current window",
        options: &[CommandOptionSpec::value("-t", Session, "target session")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "kill-window",
        aliases: &["killw"],
        description: "Destroy a window",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-a", "kill every other window in the session"),
            CommandOptionSpec::unsupported_value("-f"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "split-picker",
        aliases: &[],
        description: "Split a pane and choose what it becomes",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-l", FreeForm, "new pane size in cells or percent"),
            CommandOptionSpec::value("-p", FreeForm, "split percentage"),
            CommandOptionSpec::value("-c", FreeForm, "terminal working directory source"),
            CommandOptionSpec::flag("-b", "new pane goes left or above"),
            CommandOptionSpec::flag("-d", "keep focus on the current pane"),
            CommandOptionSpec::flag("-f", "span the full window"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
            CommandOptionSpec::unsupported_value("-B"),
            CommandOptionSpec::unsupported_value("-e"),
            CommandOptionSpec::unsupported_flag("-E"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_flag("-I"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_flag("-L"),
            CommandOptionSpec::unsupported_value("-m"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_value("-R"),
            CommandOptionSpec::unsupported_value("-s"),
            CommandOptionSpec::unsupported_value("-S"),
            CommandOptionSpec::unsupported_value("-T"),
            CommandOptionSpec::unsupported_flag("-W"),
            CommandOptionSpec::unsupported_value("-x"),
            CommandOptionSpec::unsupported_value("-X"),
            CommandOptionSpec::unsupported_value("-y"),
            CommandOptionSpec::unsupported_value("-Y"),
            CommandOptionSpec::unsupported_flag("-Z"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "split-window",
        aliases: &["splitw"],
        description: "Split a pane with a terminal",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-l", FreeForm, "new pane size in cells or percent"),
            CommandOptionSpec::value("-p", FreeForm, "split percentage"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
            CommandOptionSpec::flag("-b", "new pane goes left or above"),
            CommandOptionSpec::flag("-d", "keep focus on the current pane"),
            CommandOptionSpec::flag("-f", "span the full window"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
            CommandOptionSpec::unsupported_value("-e"),
            CommandOptionSpec::unsupported_flag("-E"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_flag("-I"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_value("-m"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_value("-R"),
            CommandOptionSpec::unsupported_value("-s"),
            CommandOptionSpec::unsupported_value("-S"),
            CommandOptionSpec::unsupported_value("-T"),
            CommandOptionSpec::unsupported_flag("-W"),
            CommandOptionSpec::unsupported_flag("-Z"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "split-browser",
        aliases: &[],
        description: "Split a pane with a browser",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-p", FreeForm, "browser profile"),
            CommandOptionSpec::flag("-b", "new pane goes left or above"),
            CommandOptionSpec::flag("-d", "keep focus on the current pane"),
            CommandOptionSpec::flag("-f", "span the full window"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "select-pane-kind",
        aliases: &[],
        description: "Materialize a pending pane as a terminal, browser, agent, or editor",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-c", FreeForm, "agent working directory"),
        ],
        positionals: &[PaneKind],
        variadic: None,
    },
    CommandSpec {
        name: "break-pane",
        aliases: &["breakp"],
        description: "Move a pane into a new window",
        options: &[
            CommandOptionSpec::value("-n", FreeForm, "new window name"),
            CommandOptionSpec::value("-s", Pane, "source pane"),
            CommandOptionSpec::value("-t", Window, "destination session or window index"),
            CommandOptionSpec::flag("-d", "do not select the new window"),
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::unsupported_flag("-b"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_flag("-W"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_value("-x"),
            CommandOptionSpec::unsupported_value("-y"),
            CommandOptionSpec::unsupported_value("-X"),
            CommandOptionSpec::unsupported_value("-Y"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "join-pane",
        aliases: &["joinp"],
        description: "Join a pane into another window",
        options: &[
            CommandOptionSpec::value("-p", FreeForm, "split percentage"),
            CommandOptionSpec::value("-s", Pane, "source pane"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-b", "place before target"),
            CommandOptionSpec::flag("-d", "do not select moved pane"),
            CommandOptionSpec::flag("-f", "fill target space"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
            CommandOptionSpec::unsupported_value("-l"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "move-pane",
        aliases: &["movep"],
        description: "Move a pane into another window",
        options: &[
            CommandOptionSpec::value("-p", FreeForm, "split percentage"),
            CommandOptionSpec::value("-s", Pane, "source pane"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-b", "place before target"),
            CommandOptionSpec::flag("-d", "do not select moved pane"),
            CommandOptionSpec::flag("-f", "fill target space"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
            CommandOptionSpec::unsupported_attached("-D"),
            CommandOptionSpec::unsupported_attached("-L"),
            CommandOptionSpec::unsupported_value("-P"),
            CommandOptionSpec::unsupported_attached("-R"),
            CommandOptionSpec::unsupported_attached("-U"),
            CommandOptionSpec::unsupported_value("-X"),
            CommandOptionSpec::unsupported_value("-Y"),
            CommandOptionSpec::unsupported_value("-l"),
            CommandOptionSpec::unsupported_value("-z"),
            CommandOptionSpec::unsupported_flag("-M"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "set-browser-url",
        aliases: &[],
        description: "Navigate a browser pane",
        options: &[CommandOptionSpec::value("-t", Pane, "target browser pane")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "set-browser-tabs",
        aliases: &[],
        description: "Replace a browser pane's tab list",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target browser pane"),
            CommandOptionSpec::value("-a", FreeForm, "active tab index"),
        ],
        positionals: &[FreeForm],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "set-browser-profile",
        aliases: &[],
        description: "Switch a browser pane to another profile",
        options: &[CommandOptionSpec::value("-t", Pane, "target browser pane")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "set-agent-session",
        aliases: &[],
        description: "Persist the opaque ACP session ID for an agent pane",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target agent pane"),
            CommandOptionSpec::value(
                "-c",
                FreeForm,
                "working directory used to create or restore the ACP session",
            ),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "set-agent-provider",
        aliases: &[],
        description: "Select the ACP provider for an agent pane",
        options: &[CommandOptionSpec::value("-t", Pane, "target agent pane")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "restart-agent-pane",
        aliases: &[],
        description: "Restart an agent pane's ACP adapter",
        options: &[CommandOptionSpec::value("-t", Pane, "target agent pane")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "set-editor-path",
        aliases: &[],
        description: "Persist or clear the absolute file path for an editor pane",
        options: &[CommandOptionSpec::value("-t", Pane, "target editor pane")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "select-pane",
        aliases: &["selectp"],
        description: "Select a pane",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-T", FreeForm, "pane title"),
            CommandOptionSpec::flag("-D", "pane below"),
            CommandOptionSpec::flag("-L", "pane to the left"),
            CommandOptionSpec::flag("-R", "pane to the right"),
            CommandOptionSpec::flag("-U", "pane above"),
            CommandOptionSpec::flag("-Z", "preserve zoom"),
            CommandOptionSpec::flag("-l", "last pane"),
            CommandOptionSpec::unsupported_flag("-d"),
            CommandOptionSpec::unsupported_flag("-e"),
            CommandOptionSpec::unsupported_flag("-g"),
            CommandOptionSpec::unsupported_flag("-M"),
            CommandOptionSpec::unsupported_flag("-m"),
            CommandOptionSpec::unsupported_value("-P"),
        ],
        positionals: &[Pane],
        variadic: None,
    },
    CommandSpec {
        name: "last-pane",
        aliases: &["lastp"],
        description: "Select the last pane",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-Z", "preserve zoom"),
            CommandOptionSpec::unsupported_flag("-d"),
            CommandOptionSpec::unsupported_flag("-e"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "swap-pane",
        aliases: &["swapp"],
        description: "Swap two panes",
        options: &[
            CommandOptionSpec::value("-s", Pane, "source pane"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-d", "do not select source"),
            CommandOptionSpec::flag("-D", "swap with next pane"),
            CommandOptionSpec::flag("-U", "swap with previous pane"),
            CommandOptionSpec::flag("-Z", "preserve zoom"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "list-panes",
        aliases: &["lsp"],
        description: "List panes",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_flag("-r"),
            CommandOptionSpec::unsupported_flag("-s"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "resize-pane",
        aliases: &["resizep"],
        description: "Resize or zoom a pane",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-x", FreeForm, "width in cells or percent"),
            CommandOptionSpec::value("-y", FreeForm, "height in cells or percent"),
            CommandOptionSpec::attached_flag("-D", "resize downward, optionally by attached cells"),
            CommandOptionSpec::attached_flag("-L", "resize left, optionally by attached cells"),
            CommandOptionSpec::attached_flag("-R", "resize right, optionally by attached cells"),
            CommandOptionSpec::attached_flag("-U", "resize upward, optionally by attached cells"),
            CommandOptionSpec::flag("-Z", "toggle zoom"),
            CommandOptionSpec::unsupported_flag("-M"),
            CommandOptionSpec::unsupported_flag("-T"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "select-layout",
        aliases: &["selectl"],
        description: "Select a pane layout",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-E", "spread current pane"),
            CommandOptionSpec::flag("-n", "next layout"),
            CommandOptionSpec::flag("-o", "previous layout"),
            CommandOptionSpec::flag("-p", "previous layout"),
        ],
        positionals: &[Layout],
        variadic: None,
    },
    CommandSpec {
        name: "next-layout",
        aliases: &["nextl"],
        description: "Select the next layout",
        options: &[CommandOptionSpec::value("-t", Window, "target window")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "previous-layout",
        aliases: &["prevl"],
        description: "Select the previous layout",
        options: &[CommandOptionSpec::value("-t", Window, "target window")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "rotate-window",
        aliases: &["rotatew"],
        description: "Rotate panes in a window",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-D", "rotate downward"),
            CommandOptionSpec::flag("-U", "rotate upward"),
            CommandOptionSpec::flag("-Z", "preserve zoom"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "kill-pane",
        aliases: &["killp"],
        description: "Destroy a pane",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-a", "kill every other pane in the window"),
            CommandOptionSpec::unsupported_value("-f"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "send-keys",
        aliases: &["send"],
        description: "Send keys or a copy-mode command",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-N", FreeForm, "repeat count"),
            CommandOptionSpec::flag("-X", "copy-mode command"),
            CommandOptionSpec::flag("-C", "do not copy to the system clipboard"),
            CommandOptionSpec::flag("-H", "keys are hexadecimal character codes"),
            CommandOptionSpec::flag("-P", "do not create a paste buffer"),
            CommandOptionSpec::flag("-l", "literal text"),
            CommandOptionSpec::flag("-o", "operate on command output"),
            CommandOptionSpec::unsupported_value("-c"),
            CommandOptionSpec::unsupported_flag("-F"),
            CommandOptionSpec::unsupported_flag("-K"),
            CommandOptionSpec::unsupported_flag("-M"),
            CommandOptionSpec::unsupported_flag("-R"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "send-prefix",
        aliases: &[],
        description: "Send the prefix key to a pane",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::unsupported_flag("-2"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "copy-mode",
        aliases: &[],
        description: "Enter copy mode",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-d", "scroll one page down"),
            CommandOptionSpec::flag("-u", "scroll one page up"),
            CommandOptionSpec::flag("-e", "exit copy mode at the bottom of history"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_flag("-H"),
            CommandOptionSpec::flag("-M", "mouse-drag entry; no-op without a mouse event"),
            CommandOptionSpec::flag("-q", "cancel copy mode"),
            CommandOptionSpec::unsupported_flag("-S"),
            CommandOptionSpec::unsupported_value("-s"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "copy-mode-search-prompt",
        aliases: &[],
        description: "Open the copy-mode search prompt",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-b", "search backward"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "command-prompt",
        aliases: &[],
        description: "Open a command prompt",
        options: &[
            CommandOptionSpec::value("-I", FreeForm, "initial input"),
            CommandOptionSpec::value("-p", FreeForm, "prompt label"),
            CommandOptionSpec::flag("-b", "background prompt, always on in zz"),
            CommandOptionSpec::unsupported_flag("-1"),
            CommandOptionSpec::unsupported_flag("-C"),
            CommandOptionSpec::unsupported_flag("-e"),
            CommandOptionSpec::unsupported_flag("-F"),
            CommandOptionSpec::unsupported_flag("-i"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_flag("-l"),
            CommandOptionSpec::unsupported_flag("-N"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::unsupported_value("-T"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "choose-tree",
        aliases: &[],
        description: "Choose a pane, or focus the session sidebar",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-s", "focus the session sidebar"),
            CommandOptionSpec::flag("-w", "focus the session sidebar"),
            CommandOptionSpec::flag("-Z", "zoom the chooser, always full window in zz"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_flag("-G"),
            CommandOptionSpec::unsupported_flag("-h"),
            CommandOptionSpec::unsupported_value("-K"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_flag("-N"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_flag("-r"),
            CommandOptionSpec::unsupported_flag("-y"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "focus-sidebar",
        aliases: &[],
        description: "Focus the workspace sidebar",
        options: &[CommandOptionSpec::value("-t", Pane, "target pane")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "choose-buffer",
        aliases: &[],
        description: "Choose a paste buffer",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-Z", "zoom the chooser, always full window in zz"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_value("-K"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_flag("-N"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_flag("-r"),
            CommandOptionSpec::unsupported_flag("-y"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "display-message",
        aliases: &["display"],
        description: "Display or print a formatted message",
        options: &[
            CommandOptionSpec::flag("-p", "print the message"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::unsupported_flag("-C"),
            CommandOptionSpec::unsupported_value("-c"),
            CommandOptionSpec::unsupported_value("-d"),
            CommandOptionSpec::unsupported_flag("-l"),
            CommandOptionSpec::unsupported_flag("-I"),
            CommandOptionSpec::unsupported_flag("-N"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_flag("-v"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "display-panes",
        aliases: &["displayp"],
        description: "Display pane numbers",
        options: &[
            CommandOptionSpec::value("-d", FreeForm, "duration in milliseconds"),
            CommandOptionSpec::flag("-b", "do not block other commands, always on in zz"),
            CommandOptionSpec::unsupported_flag("-N"),
            CommandOptionSpec::unsupported_value("-t"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "clear-history",
        aliases: &["clearhist"],
        description: "Clear terminal history",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::unsupported_flag("-H"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "bind-key",
        aliases: &["bind"],
        description: "Bind a key to a command",
        options: &[
            CommandOptionSpec::value("-T", KeyTable, "key table"),
            CommandOptionSpec::value("-N", FreeForm, "binding note"),
            CommandOptionSpec::flag("-n", "root table"),
            CommandOptionSpec::flag("-r", "repeatable binding"),
        ],
        positionals: &[FreeForm, FreeForm],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "unbind-key",
        aliases: &["unbind"],
        description: "Remove a key binding",
        options: &[
            CommandOptionSpec::value("-T", KeyTable, "key table"),
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::flag("-n", "root table"),
            CommandOptionSpec::unsupported_flag("-q"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "list-keys",
        aliases: &["lsk"],
        description: "List key bindings",
        options: &[
            CommandOptionSpec::value("-T", KeyTable, "key table"),
            CommandOptionSpec::unsupported_flag("-1"),
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_flag("-N"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_value("-P"),
            CommandOptionSpec::unsupported_flag("-r"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "set-option",
        aliases: &["set"],
        description: "Set a server, session, window, or pane option",
        options: &[
            CommandOptionSpec::value("-t", FreeForm, "target"),
            CommandOptionSpec::flag("-a", "append"),
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::flag("-o", "set only if unset"),
            CommandOptionSpec::flag("-p", "pane scope"),
            CommandOptionSpec::flag("-q", "quiet"),
            CommandOptionSpec::flag("-u", "unset"),
            CommandOptionSpec::flag("-U", "unset pane overrides"),
            CommandOptionSpec::flag("-w", "window scope"),
            CommandOptionSpec::unsupported_flag("-F"),
            CommandOptionSpec::unsupported_flag("-s"),
        ],
        positionals: &[SetOption, Boolean],
        variadic: None,
    },
    CommandSpec {
        name: "set-window-option",
        aliases: &["setw"],
        description: "Set a window option",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-a", "append"),
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::flag("-o", "set only if unset"),
            CommandOptionSpec::flag("-q", "quiet"),
            CommandOptionSpec::flag("-u", "unset"),
            CommandOptionSpec::unsupported_flag("-F"),
        ],
        positionals: &[SetOption, Boolean],
        variadic: None,
    },
    CommandSpec {
        name: "source-file",
        aliases: &["source"],
        description: "Load a tmux configuration file",
        options: &[
            CommandOptionSpec::flag("-q", "do not report a missing file"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::unsupported_flag("-F"),
            CommandOptionSpec::unsupported_flag("-n"),
            CommandOptionSpec::unsupported_flag("-v"),
        ],
        positionals: &[FreeForm],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "reload-config",
        aliases: &[],
        description: "Reload tmux and Ghostty configuration",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "kill-server",
        aliases: &[],
        description: "Stop the zz daemon",
        options: &[],
        positionals: &[],
        variadic: None,
    },
];

/// Look up a command by canonical name or alias.
#[must_use]
pub fn command_spec(name: &str) -> Option<&'static CommandSpec> {
    COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
}

/// Resolve a known alias while preserving unknown input for structured errors.
#[must_use]
pub fn canonical_command(command: &str) -> &str {
    command_spec(command).map_or(command, |spec| spec.name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn catalog_names_aliases_and_options_are_unique() {
        let mut names = BTreeSet::new();
        for spec in COMMAND_SPECS {
            assert!(names.insert(spec.name), "duplicate command {}", spec.name);
            let mut options = BTreeSet::new();
            for option in spec.options {
                assert!(
                    options.insert(option.name),
                    "duplicate option {} for {}",
                    option.name,
                    spec.name
                );
            }
        }

        for spec in COMMAND_SPECS {
            for alias in spec.aliases {
                assert!(names.insert(*alias), "duplicate command alias {alias}");
                assert_eq!(canonical_command(alias), spec.name);
            }
            assert_eq!(canonical_command(spec.name), spec.name);
        }
    }

    #[test]
    fn unknown_commands_remain_available_for_structured_errors() {
        assert_eq!(canonical_command("future-command"), "future-command");
        assert!(command_spec("future-command").is_none());
    }

    #[test]
    fn external_command_name_lists_are_unique_and_disjoint() {
        let mut names = BTreeSet::new();
        for name in DAEMON_COMMAND_NAMES {
            assert!(names.insert(*name), "duplicate daemon command {name}");
        }
        assert_eq!(names.len(), DAEMON_COMMAND_NAMES.len());
        for name in UNIMPLEMENTED_TMUX_COMMANDS {
            assert!(
                names.insert(*name),
                "duplicate or overlapping unimplemented command {name}"
            );
            assert!(command_spec(name).is_none());
        }
        assert_eq!(
            CommandSpec::DAEMON_COMMAND_NAMES,
            crate::catalog::DAEMON_COMMAND_NAMES
        );
        assert_eq!(
            CommandSpec::UNIMPLEMENTED_TMUX_COMMANDS,
            crate::catalog::UNIMPLEMENTED_TMUX_COMMANDS
        );
    }

    #[test]
    fn catalog_covers_every_executable_command_arm() {
        let executable = BTreeSet::from([
            "new-session",
            "list-sessions",
            "rename-session",
            "kill-session",
            "attach-session",
            "has-session",
            "detach-client",
            "new-window",
            "new-browser",
            "list-windows",
            "rename-window",
            "select-window",
            "next-window",
            "previous-window",
            "last-window",
            "kill-window",
            "split-picker",
            "split-window",
            "split-browser",
            "select-pane-kind",
            "break-pane",
            "join-pane",
            "move-pane",
            "set-browser-url",
            "set-browser-tabs",
            "set-browser-profile",
            "set-agent-session",
            "set-agent-provider",
            "restart-agent-pane",
            "set-editor-path",
            "select-pane",
            "last-pane",
            "swap-pane",
            "list-panes",
            "resize-pane",
            "select-layout",
            "next-layout",
            "previous-layout",
            "rotate-window",
            "kill-pane",
            "send-keys",
            "send-prefix",
            "copy-mode",
            "copy-mode-search-prompt",
            "command-prompt",
            "focus-sidebar",
            "choose-tree",
            "choose-buffer",
            "display-message",
            "display-panes",
            "clear-history",
            "bind-key",
            "unbind-key",
            "list-keys",
            "set-option",
            "set-window-option",
            "source-file",
            "reload-config",
            "kill-server",
        ]);
        let catalog = COMMAND_SPECS
            .iter()
            .map(|spec| spec.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(catalog, executable);
    }
}
