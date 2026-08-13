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
}

impl CommandOptionSpec {
    const fn flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            value: None,
            description,
            completable: true,
        }
    }

    const fn value(name: &'static str, value: CommandValueKind, description: &'static str) -> Self {
        Self {
            name,
            value: Some(value),
            description,
            completable: true,
        }
    }

    const fn unsupported_value(name: &'static str) -> Self {
        Self {
            name,
            value: Some(CommandValueKind::FreeForm),
            description: "unsupported tmux option",
            completable: false,
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

/// Every tmux-compatible command currently executable by [`crate::MuxEngine`].
pub static COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "new-session",
        aliases: &["new"],
        description: "Create a new session",
        options: &[
            CommandOptionSpec::flag("-d", "do not attach"),
            CommandOptionSpec::value("-s", FreeForm, "session name"),
            CommandOptionSpec::value("-n", FreeForm, "initial window name"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "list-sessions",
        aliases: &["ls"],
        description: "List sessions",
        options: &[CommandOptionSpec::value("-F", FreeForm, "output format")],
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
        options: &[CommandOptionSpec::value("-t", Session, "target session")],
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
        ],
        positionals: &[Session],
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
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "new-window",
        aliases: &["neww"],
        description: "Create a terminal window",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::value("-n", FreeForm, "window name"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "new-browser",
        aliases: &[],
        description: "Create a browser window",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::value("-n", FreeForm, "window name"),
            CommandOptionSpec::value("-p", FreeForm, "browser profile"),
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
        ],
        positionals: &[Window],
        variadic: None,
    },
    CommandSpec {
        name: "next-window",
        aliases: &["next"],
        description: "Select the next window",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "previous-window",
        aliases: &["previous"],
        description: "Select the previous window",
        options: &[],
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
        options: &[CommandOptionSpec::value("-t", Window, "target window")],
        positionals: &[Window],
        variadic: None,
    },
    CommandSpec {
        name: "new-pane",
        aliases: &[],
        description: "Split a pane and choose what it becomes",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-p", FreeForm, "split percentage"),
            CommandOptionSpec::value("-c", FreeForm, "terminal working directory source"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
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
            CommandOptionSpec::value("-p", FreeForm, "split percentage"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
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
        options: &[CommandOptionSpec::value("-t", Pane, "target pane")],
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
            CommandOptionSpec::value("-t", Window, "destination window or session"),
            CommandOptionSpec::flag("-d", "do not select the new window"),
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
            CommandOptionSpec::unsupported_value("-D"),
            CommandOptionSpec::unsupported_value("-L"),
            CommandOptionSpec::unsupported_value("-P"),
            CommandOptionSpec::unsupported_value("-R"),
            CommandOptionSpec::unsupported_value("-U"),
            CommandOptionSpec::unsupported_value("-X"),
            CommandOptionSpec::unsupported_value("-Y"),
            CommandOptionSpec::unsupported_value("-l"),
            CommandOptionSpec::unsupported_value("-z"),
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
            CommandOptionSpec::unsupported_value("-D"),
            CommandOptionSpec::unsupported_value("-L"),
            CommandOptionSpec::unsupported_value("-P"),
            CommandOptionSpec::unsupported_value("-R"),
            CommandOptionSpec::unsupported_value("-U"),
            CommandOptionSpec::unsupported_value("-X"),
            CommandOptionSpec::unsupported_value("-Y"),
            CommandOptionSpec::unsupported_value("-l"),
            CommandOptionSpec::unsupported_value("-z"),
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
            CommandOptionSpec::flag("-D", "resize downward"),
            CommandOptionSpec::flag("-L", "resize left"),
            CommandOptionSpec::flag("-R", "resize right"),
            CommandOptionSpec::flag("-U", "resize upward"),
            CommandOptionSpec::flag("-Z", "toggle zoom"),
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
        options: &[CommandOptionSpec::value("-t", Pane, "target pane")],
        positionals: &[Pane],
        variadic: None,
    },
    CommandSpec {
        name: "send-keys",
        aliases: &["send"],
        description: "Send keys or a copy-mode command",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-X", "copy-mode command"),
            CommandOptionSpec::flag("-C", "do not copy to the system clipboard"),
            CommandOptionSpec::flag("-P", "do not create a paste buffer"),
            CommandOptionSpec::flag("-l", "literal text"),
            CommandOptionSpec::flag("-o", "operate on command output"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "send-prefix",
        aliases: &[],
        description: "Send the prefix key to a pane",
        options: &[CommandOptionSpec::value("-t", Pane, "target pane")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "copy-mode",
        aliases: &[],
        description: "Enter copy mode",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-u", "scroll one page up"),
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
            CommandOptionSpec::flag("-b", "prompt from bottom"),
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
            CommandOptionSpec::flag("-Z", "preserve zoom"),
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
            CommandOptionSpec::flag("-Z", "preserve zoom"),
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
            CommandOptionSpec::flag("-b", "block until selection"),
            CommandOptionSpec::unsupported_value("-t"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "clear-history",
        aliases: &["clearhist"],
        description: "Clear terminal history",
        options: &[CommandOptionSpec::value("-t", Pane, "target pane")],
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
            CommandOptionSpec::flag("-n", "root table"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "list-keys",
        aliases: &["lsk"],
        description: "List key bindings",
        options: &[CommandOptionSpec::value("-T", KeyTable, "key table")],
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
        ],
        positionals: &[SetOption, Boolean],
        variadic: None,
    },
    CommandSpec {
        name: "source-file",
        aliases: &["source"],
        description: "Load a tmux configuration file",
        options: &[],
        positionals: &[FreeForm],
        variadic: None,
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
            "new-pane",
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
