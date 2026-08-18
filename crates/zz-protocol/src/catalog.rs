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
    pub usage: &'static str,
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
    "run-shell",
    "run",
    "if-shell",
    "if",
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
    "list-clients",
    "lsc",
    "show-messages",
    "showmsgs",
    "refresh-client",
    "refresh",
    "wait-for",
    "wait",
    "pipe-pane",
    "pipep",
];

pub static UNIMPLEMENTED_TMUX_COMMANDS: &[&str] = &[
    "new-pane",
    "newp",
    "set-hook",
    "show-hooks",
    "lock-client",
    "lockc",
    "lock-server",
    "lock",
    "lock-session",
    "locks",
    "server-access",
    "display-popup",
    "popup",
    "display-menu",
    "menu",
    "confirm-before",
    "confirm",
    "customize-mode",
    "choose-client",
    "clock-mode",
    "suspend-client",
    "suspendc",
    "switch-client",
    "switchc",
    "link-window",
    "linkw",
    "unlink-window",
    "unlinkw",
    "resize-window",
    "resizew",
    "clear-prompt-history",
    "clearphist",
    "show-prompt-history",
    "showphist",
    "switch-mode",
];

pub static DAEMON_COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "capture-pane",
        aliases: &["capturep"],
        description: "Capture the contents of a pane",
        usage: "[-aeJMNpqT] [-b buffer-name] [-E end-line] [-S start-line] [-t target-pane]",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "if-shell",
        aliases: &["if"],
        description: "Run a command when a shell command succeeds",
        usage: "[-bF] [-t target-pane] shell-command command [command]",
        options: &[],
        positionals: &[FreeForm, FreeForm, FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "run-shell",
        aliases: &["run"],
        description: "Run a shell command without a pane",
        usage: "[-bCE] [-c start-directory] [-d delay] [-t target-pane] [shell-command [argument ...]]",
        options: &[],
        positionals: &[FreeForm],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "delete-buffer",
        aliases: &["deleteb"],
        description: "Delete a paste buffer",
        usage: "[-b buffer-name]",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "list-buffers",
        aliases: &["lsb"],
        description: "List paste buffers",
        usage: "[-F format]",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "load-buffer",
        aliases: &["loadb"],
        description: "Load a file into a paste buffer",
        usage: "[-b buffer-name] path",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "paste-buffer",
        aliases: &["pasteb"],
        description: "Insert the contents of a paste buffer into a pane",
        usage: "[-dprS] [-s separator] [-b buffer-name] [-t target-pane]",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "save-buffer",
        aliases: &["saveb"],
        description: "Save a paste buffer to a file",
        usage: "[-a] [-b buffer-name] path",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "set-buffer",
        aliases: &["setb"],
        description: "Set the contents of a paste buffer",
        usage: "[-a] [-b buffer-name] [data]",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "show-buffer",
        aliases: &["showb"],
        description: "Display the contents of a paste buffer",
        usage: "[-b buffer-name]",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "wait-for",
        aliases: &["wait"],
        description: "Block or wake a client on a named channel",
        usage: "[-L|-S|-U] channel",
        options: &[],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "pipe-pane",
        aliases: &["pipep"],
        description: "Pipe pane input or output to a shell command",
        usage: "[-IOo] [-t target-pane] [shell-command]",
        options: &[],
        positionals: &[FreeForm],
        variadic: None,
    },
];

#[cfg(test)]
const DAEMON_COMMAND_ACCEPTED_OPTIONS: &[(&str, &str, &str)] = &[
    ("capture-pane", "aeJMNpqT", "bESt"),
    ("if-shell", "bF", "t"),
    ("run-shell", "bCE", "cdt"),
    ("delete-buffer", "", "b"),
    ("list-buffers", "", "F"),
    ("load-buffer", "", "b"),
    ("paste-buffer", "dprS", "sbt"),
    ("save-buffer", "a", "b"),
    ("set-buffer", "a", "b"),
    ("show-buffer", "", "b"),
    ("wait-for", "LSU", ""),
    ("pipe-pane", "IOo", "t"),
];

/// Every tmux-compatible command currently executable by the mux engine.
pub static COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "new-session",
        aliases: &["new"],
        description: "Create a new session",
        usage: "[-AdD] [-c start-directory] [-n window-name] [-s session-name] [-x width] [-y height] [shell-command [argument ...]]",
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
            CommandOptionSpec::value("-x", FreeForm, "initial width"),
            CommandOptionSpec::value("-y", FreeForm, "initial height"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "list-sessions",
        aliases: &["ls"],
        description: "List sessions",
        usage: "[-F format]",
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
        usage: "[-t target-session] new-name",
        options: &[CommandOptionSpec::value("-t", Session, "target session")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "kill-session",
        aliases: &[],
        description: "Destroy a session",
        usage: "[-aC] [-t target-session]",
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
        usage: "[-d] [-t target-session]",
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
        usage: "[-t target-session]",
        options: &[CommandOptionSpec::value("-t", Session, "target session")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "detach-client",
        aliases: &["detach"],
        description: "Detach the current client",
        usage: "[-a] [-s target-session]",
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
        name: "list-clients",
        aliases: &["lsc"],
        description: "List attached clients",
        usage: "[-F format] [-t target-session]",
        options: &[
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_flag("-r"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "refresh-client",
        aliases: &["refresh"],
        description: "Refresh the current client",
        usage: "[-cDlLRSU] [-A pane:state] [-B name:what:format] [-C XxY] [-F flags] [-f flags] [-r pane:report] [-t target-client] [adjustment]",
        options: &[
            CommandOptionSpec::flag("-c", "return to the previous client size"),
            CommandOptionSpec::flag("-D", "disable client size updates"),
            CommandOptionSpec::flag("-l", "request clipboard data"),
            CommandOptionSpec::flag("-L", "request clipboard data and enable OSC 52"),
            CommandOptionSpec::flag("-R", "redraw the client"),
            CommandOptionSpec::flag("-S", "refresh client status"),
            CommandOptionSpec::flag("-U", "update client size"),
            CommandOptionSpec::value("-A", FreeForm, "pane state"),
            CommandOptionSpec::value("-B", FreeForm, "format subscription"),
            CommandOptionSpec::value("-C", FreeForm, "client size"),
            CommandOptionSpec::value("-F", FreeForm, "client flags"),
            CommandOptionSpec::value("-f", FreeForm, "client flags"),
            CommandOptionSpec::value("-r", FreeForm, "pane report"),
            CommandOptionSpec::value("-t", FreeForm, "target client"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "new-window",
        aliases: &["neww"],
        description: "Create a terminal window",
        usage: "[-adkS] [-c start-directory] [-n window-name] [-t target-window] [shell-command [argument ...]]",
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
        usage: "[-adkS] [-n window-name] [-p profile] [-t target-window] [url]",
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
        usage: "[-a] [-F format] [-t target-session]",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::flag("-a", "list windows from every session"),
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
        usage: "[-t target-window] new-name",
        options: &[CommandOptionSpec::value("-t", Window, "target window")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "select-window",
        aliases: &["selectw"],
        description: "Select a window",
        usage: "[-lnpT] [-t target-window]",
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
        usage: "[-a] [-t target-session]",
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
        usage: "[-a] [-t target-session]",
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
        usage: "[-t target-session]",
        options: &[CommandOptionSpec::value("-t", Session, "target session")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "kill-window",
        aliases: &["killw"],
        description: "Destroy a window",
        usage: "[-a] [-t target-window]",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-a", "kill every other window in the session"),
            CommandOptionSpec::unsupported_value("-f"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "move-window",
        aliases: &["movew"],
        description: "Move a window to another index or session",
        usage: "[-abdkr] [-s src-window] [-t dst-window]",
        options: &[
            CommandOptionSpec::flag("-a", "insert after the destination window"),
            CommandOptionSpec::flag("-b", "insert before the destination window"),
            CommandOptionSpec::flag("-d", "do not select the moved window"),
            CommandOptionSpec::flag("-k", "replace an occupied destination"),
            CommandOptionSpec::flag("-r", "renumber the target session"),
            CommandOptionSpec::value("-s", Window, "source window"),
            CommandOptionSpec::value("-t", Window, "destination window"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "swap-window",
        aliases: &["swapw"],
        description: "Exchange two window slots",
        usage: "[-d] [-s src-window] [-t dst-window]",
        options: &[
            CommandOptionSpec::flag("-d", "select the swapped destination slots"),
            CommandOptionSpec::value("-s", Window, "source window"),
            CommandOptionSpec::value("-t", Window, "destination window"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "find-window",
        aliases: &["findw"],
        description: "Find windows from an attached client",
        usage: "[-CiNrTZ] [-t target-pane] match-string",
        options: &[
            CommandOptionSpec::flag("-C", "match pane contents"),
            CommandOptionSpec::flag("-i", "ignore case"),
            CommandOptionSpec::flag("-N", "match window names"),
            CommandOptionSpec::flag("-r", "use a regular expression"),
            CommandOptionSpec::flag("-T", "match pane titles"),
            CommandOptionSpec::flag("-Z", "zoom the chooser"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "split-picker",
        aliases: &[],
        description: "Split a pane and choose what it becomes",
        usage: "[-bdfhv] [-c start-directory] [-l size] [-p percentage] [-t target-pane]",
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
        usage: "[-bdfhv] [-c start-directory] [-l size] [-p percentage] [-t target-pane] [shell-command [argument ...]]",
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
        usage: "[-bdfhv] [-p profile] [-t target-pane] [url]",
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
        usage: "[-c start-directory] [-t target-pane] pane-kind",
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
        usage: "[-d] [-n window-name] [-s src-pane] [-t dst-window]",
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
        usage: "[-bdfhv] [-p percentage] [-s src-pane] [-t dst-pane]",
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
        usage: "[-bdfhv] [-p percentage] [-s src-pane] [-t dst-pane]",
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
        usage: "[-t target-pane] url",
        options: &[CommandOptionSpec::value("-t", Pane, "target browser pane")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "set-browser-tabs",
        aliases: &[],
        description: "Replace a browser pane's tab list",
        usage: "[-a active-tab-index] [-t target-pane] url ...",
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
        usage: "[-t target-pane] profile",
        options: &[CommandOptionSpec::value("-t", Pane, "target browser pane")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "set-agent-session",
        aliases: &[],
        description: "Persist the opaque ACP session ID for an agent pane",
        usage: "[-c start-directory] [-t target-pane] session-id",
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
        usage: "[-t target-pane] provider",
        options: &[CommandOptionSpec::value("-t", Pane, "target agent pane")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "restart-agent-pane",
        aliases: &[],
        description: "Restart an agent pane's ACP adapter",
        usage: "[-t target-pane]",
        options: &[CommandOptionSpec::value("-t", Pane, "target agent pane")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "set-editor-path",
        aliases: &[],
        description: "Persist or clear the absolute file path for an editor pane",
        usage: "[-t target-pane] [path]",
        options: &[CommandOptionSpec::value("-t", Pane, "target editor pane")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "select-pane",
        aliases: &["selectp"],
        description: "Select a pane",
        usage: "[-DLlRUZ] [-T title] [-t target-pane]",
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
        usage: "[-Z] [-t target-window]",
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
        usage: "[-dDUZ] [-s src-pane] [-t dst-pane]",
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
        usage: "[-as] [-F format] [-t target-window]",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::flag("-a", "list panes from every session"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_flag("-r"),
            CommandOptionSpec::flag("-s", "list panes from every window in the target session"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "resize-pane",
        aliases: &["resizep"],
        description: "Resize or zoom a pane",
        usage: "[-Z] [-D lines] [-L columns] [-R columns] [-U lines] [-x width] [-y height] [-t target-pane]",
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
        usage: "[-Enop] [-t target-pane] [layout-name]",
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
        usage: "[-t target-window]",
        options: &[CommandOptionSpec::value("-t", Window, "target window")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "previous-layout",
        aliases: &["prevl"],
        description: "Select the previous layout",
        usage: "[-t target-window]",
        options: &[CommandOptionSpec::value("-t", Window, "target window")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "rotate-window",
        aliases: &["rotatew"],
        description: "Rotate panes in a window",
        usage: "[-DUZ] [-t target-window]",
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
        usage: "[-a] [-t target-pane]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-a", "kill every other pane in the window"),
            CommandOptionSpec::unsupported_value("-f"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "respawn-pane",
        aliases: &["respawnp"],
        description: "Restart a terminal pane in place",
        usage: "[-Ek] [-c start-directory] [-e environment] [-t target-pane] [shell-command [argument ...]]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-c", FreeForm, "start directory"),
            CommandOptionSpec::value("-e", FreeForm, "environment override"),
            CommandOptionSpec::flag("-E", "create an empty pane"),
            CommandOptionSpec::flag("-k", "replace an active pane"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "respawn-window",
        aliases: &["respawnw"],
        description: "Restart a terminal window in place",
        usage: "[-Ek] [-c start-directory] [-e environment] [-t target-window] [shell-command [argument ...]]",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::value("-c", FreeForm, "start directory"),
            CommandOptionSpec::value("-e", FreeForm, "environment override"),
            CommandOptionSpec::flag("-E", "create an empty pane"),
            CommandOptionSpec::flag("-k", "replace an active window"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "send-keys",
        aliases: &["send"],
        description: "Send keys or a copy-mode command",
        usage: "[-CHPloX] [-N repeat-count] [-t target-pane] [key ...]",
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
        usage: "[-t target-pane]",
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
        usage: "[-deMqu] [-t target-pane]",
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
        usage: "[-b] [-t target-pane]",
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
        usage: "[-b] [-I inputs] [-p prompts] [template]",
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
        usage: "[-swZ] [-t target-pane]",
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
        usage: "[-t target-pane]",
        options: &[CommandOptionSpec::value("-t", Pane, "target pane")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "choose-buffer",
        aliases: &[],
        description: "Choose a paste buffer",
        usage: "[-Z] [-t target-pane]",
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
        usage: "[-p] [-t target-pane] [message]",
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
        name: "show-messages",
        aliases: &["showmsgs"],
        description: "Show the daemon message log",
        usage: "",
        options: &[
            CommandOptionSpec::unsupported_flag("-J"),
            CommandOptionSpec::unsupported_flag("-T"),
            CommandOptionSpec::unsupported_value("-t"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "display-panes",
        aliases: &["displayp"],
        description: "Display pane numbers",
        usage: "[-b] [-d duration]",
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
        usage: "[-t target-pane]",
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
        usage: "[-nr] [-T key-table] [-N note] key [command [argument ...]]",
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
        usage: "[-n] [-T key-table] key",
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
        usage: "[-T key-table]",
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
        name: "list-commands",
        aliases: &["lscm"],
        description: "List implemented commands",
        usage: "[-F format] [command]",
        options: &[CommandOptionSpec::value("-F", FreeForm, "output format")],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "set-option",
        aliases: &["set"],
        description: "Set a server, session, window, or pane option",
        usage: "[-agopqsuUw] [-t target-pane] option [value]",
        options: &[
            CommandOptionSpec::value("-t", FreeForm, "target"),
            CommandOptionSpec::flag("-a", "append"),
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::flag("-o", "set only if unset"),
            CommandOptionSpec::flag("-p", "pane scope"),
            CommandOptionSpec::flag("-q", "quiet"),
            CommandOptionSpec::flag("-s", "server scope"),
            CommandOptionSpec::flag("-u", "unset"),
            CommandOptionSpec::flag("-U", "unset pane overrides"),
            CommandOptionSpec::flag("-w", "window scope"),
            CommandOptionSpec::unsupported_flag("-F"),
        ],
        positionals: &[SetOption, Boolean],
        variadic: None,
    },
    CommandSpec {
        name: "set-window-option",
        aliases: &["setw"],
        description: "Set a window option",
        usage: "[-agoqu] [-t target-window] option [value]",
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
        name: "show-options",
        aliases: &["show"],
        description: "Show server, session, window, or pane options",
        usage: "[-AgHpqsvw] [-t target-pane] [option]",
        options: &[
            CommandOptionSpec::flag("-A", "include inherited values"),
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::flag("-H", "include hooks"),
            CommandOptionSpec::flag("-p", "pane scope"),
            CommandOptionSpec::flag("-q", "quiet"),
            CommandOptionSpec::flag("-s", "server scope"),
            CommandOptionSpec::value("-t", FreeForm, "target"),
            CommandOptionSpec::flag("-v", "show value only"),
            CommandOptionSpec::flag("-w", "window scope"),
        ],
        positionals: &[SetOption],
        variadic: None,
    },
    CommandSpec {
        name: "show-window-options",
        aliases: &["showw"],
        description: "Show window options",
        usage: "[-gv] [-t target-window] [option]",
        options: &[
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-v", "show value only"),
        ],
        positionals: &[SetOption],
        variadic: None,
    },
    CommandSpec {
        name: "set-environment",
        aliases: &["setenv"],
        description: "Set a session environment variable",
        usage: "[-Fhgru] [-t target-session] variable [value]",
        options: &[
            CommandOptionSpec::flag("-F", "expand formats in value"),
            CommandOptionSpec::flag("-g", "global environment"),
            CommandOptionSpec::flag("-h", "hide from child processes"),
            CommandOptionSpec::flag("-r", "store an unset marker"),
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::flag("-u", "remove the stored variable"),
        ],
        positionals: &[FreeForm, FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "show-environment",
        aliases: &["showenv"],
        description: "Show session environment variables",
        usage: "[-hgs] [-t target-session] [variable]",
        options: &[
            CommandOptionSpec::flag("-g", "global environment"),
            CommandOptionSpec::flag("-h", "show hidden variables"),
            CommandOptionSpec::flag("-s", "emit shell commands"),
            CommandOptionSpec::value("-t", Session, "target session"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "source-file",
        aliases: &["source"],
        description: "Load a tmux configuration file",
        usage: "[-q] path ...",
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
        usage: "",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "start-server",
        aliases: &["start"],
        description: "Ensure the zz daemon is running",
        usage: "",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "kill-server",
        aliases: &[],
        description: "Stop the zz daemon",
        usage: "",
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
    command_spec(command)
        .or_else(|| {
            DAEMON_COMMAND_SPECS
                .iter()
                .find(|spec| spec.name == command || spec.aliases.contains(&command))
        })
        .map_or(command, |spec| spec.name)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    fn usage_options(usage: &str) -> BTreeMap<char, bool> {
        let mut options = BTreeMap::new();
        for usage_option in usage.split("[-").skip(1) {
            let (body, _) = usage_option
                .split_once(']')
                .expect("usage option has a closing bracket");
            let mut tokens = body.split_ascii_whitespace();
            let flags = tokens.next().expect("usage option has a flag");
            let takes_value = tokens.next().is_some();
            assert!(tokens.next().is_none(), "usage option has one value label");
            if takes_value {
                assert_eq!(flags.len(), 1, "valued usage option is not grouped");
            }
            for flag in flags.chars().filter(|flag| !matches!(flag, '|' | '-')) {
                assert!(
                    options.insert(flag, takes_value).is_none(),
                    "duplicate usage option -{flag}"
                );
            }
        }
        options
    }

    fn accepted_options(spec: &CommandSpec) -> BTreeMap<char, bool> {
        spec.options
            .iter()
            .filter(|option| !option.unsupported)
            .map(|option| {
                let bytes = option.name.as_bytes();
                assert_eq!(bytes.len(), 2, "{} has a non-short option", spec.name);
                assert_eq!(bytes[0], b'-', "{} has an invalid option", spec.name);
                (
                    char::from(bytes[1]),
                    option.value.is_some() || option.attached_value,
                )
            })
            .collect()
    }

    fn encoded_daemon_options(flags: &str, values: &str) -> BTreeMap<char, bool> {
        flags
            .chars()
            .map(|flag| (flag, false))
            .chain(values.chars().map(|flag| (flag, true)))
            .collect()
    }

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
    fn every_usage_string_matches_its_accepted_options() {
        for spec in COMMAND_SPECS {
            assert_eq!(
                usage_options(spec.usage),
                accepted_options(spec),
                "usage drift for {}",
                spec.name
            );
        }

        assert_eq!(
            DAEMON_COMMAND_ACCEPTED_OPTIONS.len(),
            DAEMON_COMMAND_SPECS.len()
        );
        for spec in DAEMON_COMMAND_SPECS {
            let (_, flags, values) = DAEMON_COMMAND_ACCEPTED_OPTIONS
                .iter()
                .find(|(name, _, _)| *name == spec.name)
                .unwrap_or_else(|| panic!("missing daemon option set for {}", spec.name));
            assert_eq!(
                usage_options(spec.usage),
                encoded_daemon_options(flags, values),
                "usage drift for {}",
                spec.name
            );
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
        for spec in DAEMON_COMMAND_SPECS {
            assert_eq!(canonical_command(spec.name), spec.name);
            for alias in spec.aliases {
                assert_eq!(canonical_command(alias), spec.name);
            }
        }
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
        for name in [
            "run-shell",
            "run",
            "if-shell",
            "if",
            "wait-for",
            "wait",
            "pipe-pane",
            "pipep",
        ] {
            assert!(DAEMON_COMMAND_NAMES.contains(&name));
            assert!(!UNIMPLEMENTED_TMUX_COMMANDS.contains(&name));
        }
        for name in ["set-hook", "show-hooks"] {
            assert!(UNIMPLEMENTED_TMUX_COMMANDS.contains(&name));
        }
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
            "list-clients",
            "refresh-client",
            "new-window",
            "new-browser",
            "list-windows",
            "rename-window",
            "select-window",
            "next-window",
            "previous-window",
            "last-window",
            "kill-window",
            "respawn-window",
            "move-window",
            "swap-window",
            "find-window",
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
            "respawn-pane",
            "send-keys",
            "send-prefix",
            "copy-mode",
            "copy-mode-search-prompt",
            "command-prompt",
            "focus-sidebar",
            "choose-tree",
            "choose-buffer",
            "display-message",
            "show-messages",
            "display-panes",
            "clear-history",
            "bind-key",
            "unbind-key",
            "list-keys",
            "list-commands",
            "set-option",
            "set-window-option",
            "show-options",
            "show-window-options",
            "set-environment",
            "show-environment",
            "source-file",
            "reload-config",
            "start-server",
            "kill-server",
        ]);
        let catalog = COMMAND_SPECS
            .iter()
            .map(|spec| spec.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(catalog, executable);
    }
}
