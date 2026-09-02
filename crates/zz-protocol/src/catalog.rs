//! Shared metadata for tmux-compatible and native `zz` commands.

use crate::message::{CommandInvocation, ServerError};

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
    /// Whether zz accepts a value only when it is attached to the flag.
    pub attached_value: bool,
    pub optional_value: bool,
    /// Whether the option is catalogued only so its value can be rejected.
    pub unsupported: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TmuxOption<'a> {
    Flag(&'static str),
    Value(&'static str, &'a str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmuxOptionParse<'a> {
    pub options: Vec<TmuxOption<'a>>,
    pub positionals: &'a [String],
}

impl CommandOptionSpec {
    const fn flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            value: None,
            description,
            completable: true,
            attached_value: false,
            optional_value: false,
            unsupported: false,
        }
    }

    const fn optional_value(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            value: None,
            description,
            completable: true,
            attached_value: true,
            optional_value: true,
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
            optional_value: false,
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
            optional_value: false,
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
            optional_value: false,
            unsupported: true,
        }
    }

    const fn unsupported_optional_value(name: &'static str) -> Self {
        Self {
            name,
            value: None,
            description: "unsupported tmux option",
            completable: false,
            attached_value: true,
            optional_value: true,
            unsupported: true,
        }
    }
}

/// Syntax, completion, and canonicalization metadata for one command.
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
    pub const TMUX_VERSION_OUTPUT: &'static str = "tmux 3.8-zz";
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

    #[must_use]
    pub fn pinned_tmux_usage(&self) -> &'static str {
        PINNED_TMUX_USAGE_OVERRIDES
            .iter()
            .find_map(|(name, usage)| (*name == self.name).then_some(*usage))
            .unwrap_or(self.usage)
    }

    #[must_use]
    pub fn uses_tmux_option_grammar(&self) -> bool {
        !NATIVE_COMMAND_NAMES.contains(&self.name)
    }

    #[must_use]
    pub fn positional_minimum(&self) -> usize {
        POSITIONAL_MINIMUMS
            .iter()
            .find_map(|(name, minimum)| (*name == self.name).then_some(*minimum))
            .unwrap_or(0)
    }

    pub fn validate_positional_minimum(&self, actual: usize) -> Result<(), ServerError> {
        let minimum = self.positional_minimum();
        if actual < minimum {
            return Err(ServerError::CommandParse(format!(
                "command {}: too few arguments (need at least {minimum})",
                self.name
            )));
        }
        Ok(())
    }

    #[must_use]
    pub const fn positional_maximum(&self) -> Option<usize> {
        if self.variadic.is_none() {
            Some(self.positionals.len())
        } else {
            None
        }
    }

    pub fn validate_positional_maximum(&self, actual: usize) -> Result<(), ServerError> {
        let Some(maximum) = self.positional_maximum() else {
            return Ok(());
        };
        if actual > maximum {
            return Err(ServerError::CommandParse(format!(
                "command {}: too many arguments (need at most {maximum})",
                self.name
            )));
        }
        Ok(())
    }
}

pub fn parse_tmux_options<'a>(
    spec: &CommandSpec,
    args: &'a [String],
) -> Result<TmuxOptionParse<'a>, ServerError> {
    parse_tmux_options_with_command(spec, args, None)
}

pub fn parse_tmux_command_options<'a>(
    spec: &CommandSpec,
    command: &'a CommandInvocation,
) -> Result<TmuxOptionParse<'a>, ServerError> {
    let has_custom_parser = COMMAND_ARGS_PARSE_SPECS
        .iter()
        .any(|args_parse| args_parse.name == spec.name);
    let typed =
        (!has_custom_parser || COMMAND_ARGS_PARSE_BEHAVES.contains(&spec.name)).then_some(command);
    let parsed = parse_tmux_options_with_command(spec, &command.args, typed)?;
    if let Some(command) = typed {
        validate_command_args_parse(spec, command, &parsed)?;
    }
    Ok(parsed)
}

fn parse_tmux_options_with_command<'a>(
    spec: &CommandSpec,
    args: &'a [String],
    command: Option<&CommandInvocation>,
) -> Result<TmuxOptionParse<'a>, ServerError> {
    let mut options = Vec::new();
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if !argument.starts_with('-') || argument == "-" {
            break;
        }
        index += 1;
        if argument == "--" {
            break;
        }

        for (offset, flag) in argument[1..].char_indices() {
            if flag == '?' {
                return Err(ServerError::CommandParse(format!(
                    "usage: {} {}",
                    spec.name,
                    spec.pinned_tmux_usage()
                )));
            }
            if !flag.is_ascii_alphanumeric() {
                return Err(ServerError::CommandParse(format!(
                    "command {}: invalid flag -{flag}",
                    spec.name
                )));
            }
            let option = spec
                .options
                .iter()
                .find(|option| {
                    let mut name = option.name.chars();
                    name.next() == Some('-') && name.next() == Some(flag) && name.next().is_none()
                })
                .ok_or_else(|| {
                    ServerError::CommandParse(format!(
                        "command {}: unknown flag -{flag}",
                        spec.name
                    ))
                })?;
            let value_start = offset + flag.len_utf8() + 1;
            let attached = &argument[value_start..];
            if option.optional_value {
                if !attached.is_empty() {
                    options.push(TmuxOption::Value(option.name, attached));
                } else if let Some(value) = args
                    .get(index)
                    .filter(|value| !optional_value_starts_option(value))
                {
                    if command.is_some_and(|command| command.argument_is_command_block(index)) {
                        return Err(ServerError::CommandParse(format!(
                            "command {}: {} argument must be a string",
                            spec.name, option.name
                        )));
                    }
                    options.push(TmuxOption::Value(option.name, value));
                    index += 1;
                } else {
                    options.push(TmuxOption::Flag(option.name));
                }
                break;
            }
            if option.value.is_some() || option.attached_value {
                let value = if attached.is_empty() {
                    let value = args.get(index).ok_or_else(|| {
                        ServerError::CommandParse(format!(
                            "command {}: {} expects an argument",
                            spec.name, option.name
                        ))
                    })?;
                    if command.is_some_and(|command| command.argument_is_command_block(index)) {
                        return Err(ServerError::CommandParse(format!(
                            "command {}: {} argument must be a string",
                            spec.name, option.name
                        )));
                    }
                    index += 1;
                    value.as_str()
                } else {
                    attached
                };
                options.push(TmuxOption::Value(option.name, value));
                break;
            }
            options.push(TmuxOption::Flag(option.name));
        }
    }
    Ok(TmuxOptionParse {
        options,
        positionals: &args[index..],
    })
}

fn validate_command_args_parse(
    spec: &CommandSpec,
    command: &CommandInvocation,
    parsed: &TmuxOptionParse<'_>,
) -> Result<(), ServerError> {
    #[derive(Clone, Copy)]
    enum DisplayMenuState {
        Name,
        Key,
        Action,
    }

    let args_parse = COMMAND_ARGS_PARSE_SPECS
        .iter()
        .find(|args_parse| args_parse.name == spec.name)
        .map(|args_parse| args_parse.rule);
    let run_shell_commands = args_parse == Some(CommandArgsParseRule::RunShellCommandFlag)
        && parsed
            .options
            .iter()
            .any(|option| matches!(option, TmuxOption::Flag("-C")));
    let set_hook_monitor = args_parse == Some(CommandArgsParseRule::SetHookMonitorOrValue)
        && parsed
            .options
            .iter()
            .any(|option| matches!(option, TmuxOption::Value("-B", _)));
    let start = command.args.len().saturating_sub(parsed.positionals.len());
    let mut display_menu_state = DisplayMenuState::Name;
    for (position, index) in (start..command.args.len()).enumerate() {
        let is_command_block = command.argument_is_command_block(index);
        let accepts_command_block = match args_parse {
            None => false,
            Some(CommandArgsParseRule::CommandsOrString) => true,
            Some(CommandArgsParseRule::DisplayMenuItems) => match display_menu_state {
                DisplayMenuState::Name => {
                    if !command.args[index].is_empty() {
                        display_menu_state = DisplayMenuState::Key;
                    }
                    false
                }
                DisplayMenuState::Key => {
                    display_menu_state = DisplayMenuState::Action;
                    false
                }
                DisplayMenuState::Action => {
                    display_menu_state = DisplayMenuState::Name;
                    true
                }
            },
            Some(CommandArgsParseRule::IfShellBranches) => matches!(position, 1 | 2),
            Some(CommandArgsParseRule::RunShellCommandFlag) => run_shell_commands,
            Some(CommandArgsParseRule::SetHookMonitorOrValue) => set_hook_monitor || position == 1,
            Some(CommandArgsParseRule::SetOptionValue) => position == 1,
        };
        if is_command_block && !accepts_command_block {
            return Err(ServerError::CommandParse(format!(
                "command {}: argument {} must be \"string\"",
                spec.name,
                position + 1
            )));
        }
    }
    Ok(())
}

fn optional_value_starts_option(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.first() == Some(&b'-')
        && bytes
            .get(1)
            .is_some_and(|byte| *byte == b'-' || byte.is_ascii_alphabetic())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandArgsParseRule {
    CommandsOrString,
    DisplayMenuItems,
    IfShellBranches,
    RunShellCommandFlag,
    SetHookMonitorOrValue,
    SetOptionValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandArgsParseSpec {
    pub name: &'static str,
    pub rule: CommandArgsParseRule,
}

pub static COMMAND_ARGS_PARSE_SPECS: &[CommandArgsParseSpec] = &[
    CommandArgsParseSpec {
        name: "bind-key",
        rule: CommandArgsParseRule::CommandsOrString,
    },
    CommandArgsParseSpec {
        name: "choose-buffer",
        rule: CommandArgsParseRule::CommandsOrString,
    },
    CommandArgsParseSpec {
        name: "choose-tree",
        rule: CommandArgsParseRule::CommandsOrString,
    },
    CommandArgsParseSpec {
        name: "command-prompt",
        rule: CommandArgsParseRule::CommandsOrString,
    },
    CommandArgsParseSpec {
        name: "confirm-before",
        rule: CommandArgsParseRule::CommandsOrString,
    },
    CommandArgsParseSpec {
        name: "display-menu",
        rule: CommandArgsParseRule::DisplayMenuItems,
    },
    CommandArgsParseSpec {
        name: "display-panes",
        rule: CommandArgsParseRule::CommandsOrString,
    },
    CommandArgsParseSpec {
        name: "if-shell",
        rule: CommandArgsParseRule::IfShellBranches,
    },
    CommandArgsParseSpec {
        name: "run-shell",
        rule: CommandArgsParseRule::RunShellCommandFlag,
    },
    CommandArgsParseSpec {
        name: "set-hook",
        rule: CommandArgsParseRule::SetHookMonitorOrValue,
    },
    CommandArgsParseSpec {
        name: "set-option",
        rule: CommandArgsParseRule::SetOptionValue,
    },
    CommandArgsParseSpec {
        name: "set-window-option",
        rule: CommandArgsParseRule::SetOptionValue,
    },
];

pub static COMMAND_ARGS_PARSE_BEHAVES: &[&str] = &[
    "bind-key",
    "choose-buffer",
    "choose-tree",
    "command-prompt",
    "confirm-before",
    "display-menu",
    "display-panes",
    "if-shell",
    "run-shell",
    "set-hook",
    "set-option",
    "set-window-option",
];

use CommandValueKind::{
    Boolean, FreeForm, KeyTable, Layout, Pane, PaneKind, Session, SetOption, Window,
};

static PINNED_TMUX_USAGE_OVERRIDES: &[(&str, &str)] = &[
    (
        "break-pane",
        "[-abdPW] [-F format] [-n window-name] [-s src-pane] [-t dst-window] [-x width] [-y height] [-X x-position] [-Y y-position]",
    ),
    (
        "capture-pane",
        "[-aCeFHJLMNpPqRT] [-b buffer-name] [-E end-line] [-S start-line] [-t target-pane]",
    ),
    (
        "choose-buffer",
        "[-kNrZ] [-F format] [-f filter] [-K key-format] [-O sort-order] [-t target-pane] [template]",
    ),
    (
        "choose-tree",
        "[-GhkNrswZ] [-F format] [-f filter] [-K key-format] [-O sort-order] [-t target-pane] [template]",
    ),
    ("clear-history", "[-H] [-t target-pane]"),
    (
        "command-prompt",
        "[-1CbeFiklNP] [-I inputs] [-p prompts] [-t target-client] [-T prompt-type] [template]",
    ),
    ("copy-mode", "[-dekHMqSu] [-s src-pane] [-t target-pane]"),
    (
        "display-message",
        "[-aCIlNpv] [-c target-client] [-d delay] [-F format] [-t target-pane] [message]",
    ),
    (
        "join-pane",
        "[-bdfhv] [-l size] [-s src-pane] [-t dst-pane]",
    ),
    ("kill-session", "[-aCg] [-f filter] [-t target-session]"),
    ("list-buffers", "[-F format] [-f filter] [-O order]"),
    (
        "list-clients",
        "[-F format] [-f filter] [-O order][-t target-session]",
    ),
    (
        "list-panes",
        "[-asr] [-F format] [-f filter] [-O order][-t target-window]",
    ),
    (
        "list-windows",
        "[-ar] [-F format] [-f filter] [-O order][-t target-session]",
    ),
    ("load-buffer", "[-b buffer-name] [-t target-client] path"),
    (
        "move-pane",
        "[-bdfhMv] [-D lines] [-l size] [-L columns] [-P position] [-R columns] [-s src-pane] [-t dst-pane] [-U lines] [-X x-position] [-Y y-position] [-z z-index]",
    ),
    (
        "refresh-client",
        "[-cDlLRSU] [-A pane:state] [-B name:what:format] [-C XxY] [-f flags] [-r pane:report] [-t target-client] [adjustment]",
    ),
    (
        "resize-pane",
        "[-MTZ] [-D lines] [-L columns] [-R columns] [-U lines] [-x width] [-y height] [-t target-pane]",
    ),
    ("select-pane", "[-DdeLlMmRUZ] [-T title] [-t target-pane]"),
    (
        "send-keys",
        "[-FHKlMRX] [-c target-client] [-N repeat-count] [-t target-pane] [key ...]",
    ),
    ("show-messages", "[-JT] [-t target-client]"),
    (
        "split-window",
        "[-bdefhIklPvWZ] [-c start-directory] [-e environment] [-F format] [-l size] [-m message] [-p percentage] [-s style] [-S active-border-style] [-R inactive-border-style] [-T title] [-t target-pane] [shell-command [argument ...]]",
    ),
];

pub static DAEMON_COMMAND_NAMES: &[&str] = &[
    "capture-pane",
    "capturep",
    "run-shell",
    "run",
    "if-shell",
    "if",
    "agent-send",
    "send-last-output",
    "show-last-output",
    "send-text",
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
    "clear-prompt-history",
    "clearphist",
    "show-prompt-history",
    "showphist",
    "refresh-client",
    "refresh",
    "switch-client",
    "switchc",
    "wait-for",
    "wait",
    "pipe-pane",
    "pipep",
    "display-popup",
    "popup",
    "display-menu",
    "menu",
    "confirm-before",
    "confirm",
    "lock-client",
    "lockc",
    "lock-server",
    "lock",
    "lock-session",
    "locks",
];

pub static POSITIONAL_MINIMUMS: &[(&str, usize)] = &[
    ("bind-key", 1),
    ("confirm-before", 1),
    ("display-menu", 1),
    ("find-window", 1),
    ("if-shell", 2),
    ("load-buffer", 1),
    ("rename-session", 1),
    ("rename-window", 1),
    ("save-buffer", 1),
    ("set-environment", 1),
    ("set-option", 1),
    ("set-window-option", 1),
    ("source-file", 1),
    ("wait-for", 1),
];

pub static UNIMPLEMENTED_TMUX_COMMANDS: &[&str] = &[
    "new-pane",
    "newp",
    "server-access",
    "customize-mode",
    "choose-client",
    "clock-mode",
    "suspend-client",
    "suspendc",
    "link-window",
    "linkw",
    "unlink-window",
    "unlinkw",
    "switch-mode",
];

static UNIMPLEMENTED_TMUX_COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "choose-client",
        aliases: &[],
        description: "Unsupported tmux command",
        usage: "[-hikNrZ] [-F format] [-f filter] [-K key-format] [-O sort-order] [-t target-pane] [template]",
        options: &[
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_value("-K"),
            CommandOptionSpec::unsupported_flag("-N"),
            CommandOptionSpec::unsupported_value("-O"),
            CommandOptionSpec::unsupported_flag("-Z"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_flag("-h"),
            CommandOptionSpec::unsupported_flag("-i"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_flag("-r"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::unsupported_flag("-y"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "clock-mode",
        aliases: &[],
        description: "Unsupported tmux command",
        usage: "[-t target-pane]",
        options: &[CommandOptionSpec::unsupported_value("-t")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "customize-mode",
        aliases: &[],
        description: "Unsupported tmux command",
        usage: "[-kNZ] [-F format] [-f filter] [-t target-pane]",
        options: &[
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_flag("-N"),
            CommandOptionSpec::unsupported_flag("-Z"),
            CommandOptionSpec::unsupported_value("-f"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::unsupported_flag("-y"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "link-window",
        aliases: &["linkw"],
        description: "Unsupported tmux command",
        usage: "[-abdk] [-s src-window] [-t dst-window]",
        options: &[
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::unsupported_flag("-b"),
            CommandOptionSpec::unsupported_flag("-d"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_value("-s"),
            CommandOptionSpec::unsupported_value("-t"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "new-pane",
        aliases: &["newp"],
        description: "Unsupported tmux command",
        usage: "[-bdefhIklPvWZ] [-B border-lines] [-c start-directory] [-e environment] [-F format] [-l size] [-m message] [-p percentage] [-s style] [-S active-border-style] [-R inactive-border-style] [-T title] [-x width] [-y height] [-X x-position] [-Y y-position] [-t target-pane] [shell-command [argument ...]]",
        options: &[
            CommandOptionSpec::unsupported_value("-B"),
            CommandOptionSpec::unsupported_flag("-E"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_flag("-I"),
            CommandOptionSpec::unsupported_flag("-L"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_value("-R"),
            CommandOptionSpec::unsupported_value("-S"),
            CommandOptionSpec::unsupported_value("-T"),
            CommandOptionSpec::unsupported_flag("-W"),
            CommandOptionSpec::unsupported_value("-X"),
            CommandOptionSpec::unsupported_value("-Y"),
            CommandOptionSpec::unsupported_flag("-Z"),
            CommandOptionSpec::unsupported_flag("-b"),
            CommandOptionSpec::unsupported_value("-c"),
            CommandOptionSpec::unsupported_flag("-d"),
            CommandOptionSpec::unsupported_value("-e"),
            CommandOptionSpec::unsupported_flag("-f"),
            CommandOptionSpec::unsupported_flag("-h"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_value("-l"),
            CommandOptionSpec::unsupported_value("-m"),
            CommandOptionSpec::unsupported_value("-p"),
            CommandOptionSpec::unsupported_value("-s"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::unsupported_flag("-v"),
            CommandOptionSpec::unsupported_value("-x"),
            CommandOptionSpec::unsupported_value("-y"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "server-access",
        aliases: &[],
        description: "Unsupported tmux command",
        usage: "[-adglrw] [-t target-pane] [user|group]",
        options: &[
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::unsupported_flag("-d"),
            CommandOptionSpec::unsupported_flag("-g"),
            CommandOptionSpec::unsupported_flag("-l"),
            CommandOptionSpec::unsupported_flag("-r"),
            CommandOptionSpec::unsupported_flag("-w"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "suspend-client",
        aliases: &["suspendc"],
        description: "Unsupported tmux command",
        usage: "[-t target-client]",
        options: &[CommandOptionSpec::unsupported_value("-t")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "switch-mode",
        aliases: &[],
        description: "Unsupported tmux command",
        usage: "[-kswZ] [-F format] [-t target-pane] [command]",
        options: &[
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::unsupported_flag("-Z"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_flag("-s"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::unsupported_flag("-w"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "unlink-window",
        aliases: &["unlinkw"],
        description: "Unsupported tmux command",
        usage: "[-k] [-t target-window]",
        options: &[
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::unsupported_value("-t"),
        ],
        positionals: &[],
        variadic: None,
    },
];

pub static NATIVE_COMMAND_NAMES: &[&str] = &[
    "agent-send",
    "capture-browser",
    "copy-mode-search-prompt",
    "debug-marker",
    "focus-sidebar",
    "new-browser",
    "reload-config",
    "restart-agent-pane",
    "select-pane-kind",
    "send-last-output",
    "send-text",
    "set-agent-provider",
    "set-agent-session",
    "set-browser-profile",
    "set-browser-tabs",
    "set-browser-url",
    "set-editor-path",
    "show-last-output",
    "split-browser",
    "split-picker",
    "tools",
];

pub static DAEMON_COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "capture-pane",
        aliases: &["capturep"],
        description: "Capture the contents of a pane",
        usage: "[-aeJMNpqT] [-b buffer-name] [-E end-line] [-S start-line] [-t target-pane]",
        options: &[
            CommandOptionSpec::flag("-a", "capture the alternate screen"),
            CommandOptionSpec::flag("-e", "include escape sequences"),
            CommandOptionSpec::flag("-J", "join wrapped lines"),
            CommandOptionSpec::flag("-M", "trailing spaces"),
            CommandOptionSpec::flag("-N", "preserve trailing spaces"),
            CommandOptionSpec::flag("-p", "print to stdout"),
            CommandOptionSpec::flag("-q", "quiet"),
            CommandOptionSpec::flag("-T", "trim trailing positions"),
            CommandOptionSpec::value("-b", FreeForm, "buffer name"),
            CommandOptionSpec::value("-E", FreeForm, "end line"),
            CommandOptionSpec::value("-S", FreeForm, "start line"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::unsupported_flag("-C"),
            CommandOptionSpec::unsupported_flag("-F"),
            CommandOptionSpec::unsupported_flag("-H"),
            CommandOptionSpec::unsupported_flag("-L"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_flag("-R"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "agent-send",
        aliases: &[],
        description: "Send text to an agent pane",
        usage: "[-t target-pane] [--target target-pane] [--submit] [--wait] [--timeout seconds] [--context context] [text ...]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("--target", Pane, "target pane"),
            CommandOptionSpec::flag(
                "--submit",
                "submit the text instead of filling the composer",
            ),
            CommandOptionSpec::flag("--wait", "submit, then print the turn's reply"),
            CommandOptionSpec::value("--timeout", FreeForm, "seconds to wait for the reply"),
            CommandOptionSpec::value("--context", FreeForm, "file and optional line range"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "send-last-output",
        aliases: &[],
        description: "Send a terminal's last command and output to an agent pane",
        usage: "[-t target-pane]",
        options: &[CommandOptionSpec::value("-t", Pane, "target terminal pane")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "show-last-output",
        aliases: &[],
        description: "Print a terminal's last command and output",
        usage: "[-t target-pane]",
        options: &[CommandOptionSpec::value("-t", Pane, "target terminal pane")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "send-text",
        aliases: &[],
        description: "Paste text into a terminal pane, wait for it to land, then press Enter",
        usage: "[-t target-pane] [--target target-pane] [--no-enter] [--timeout milliseconds] text ...",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target terminal pane"),
            CommandOptionSpec::value("--target", Pane, "target terminal pane"),
            CommandOptionSpec::flag("--no-enter", "leave the text in the composer"),
            CommandOptionSpec::value("--timeout", FreeForm, "milliseconds to wait for the echo"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "capture-browser",
        aliases: &[],
        description: "Save a browser pane's current frame as a PNG",
        usage: "[-t target-pane] [-o output-path]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target browser pane"),
            CommandOptionSpec::value("-o", FreeForm, "absolute output path"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "debug-marker",
        aliases: &[],
        description: "Write a marker to the daemon log",
        usage: "[note ...]",
        options: &[],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "tools",
        aliases: &[],
        description: "Show commands for controlling a zz workspace",
        usage: "[argument ...]",
        options: &[],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "clear-prompt-history",
        aliases: &["clearphist"],
        description: "Clear command prompt history",
        usage: "[-T prompt-type]",
        options: &[CommandOptionSpec::value(
            "-T",
            FreeForm,
            "prompt type: command or search",
        )],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "show-prompt-history",
        aliases: &["showphist"],
        description: "Show command prompt history",
        usage: "[-T prompt-type]",
        options: &[CommandOptionSpec::value(
            "-T",
            FreeForm,
            "prompt type: command or search",
        )],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "if-shell",
        aliases: &["if"],
        description: "Run a command when a shell command succeeds",
        usage: "[-bF] [-t target-pane] shell-command command [command]",
        options: &[
            CommandOptionSpec::flag("-b", "run in the background"),
            CommandOptionSpec::flag("-F", "evaluate as a format"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
        ],
        positionals: &[FreeForm, FreeForm, FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "run-shell",
        aliases: &["run"],
        description: "Run a shell command without a pane",
        usage: "[-bCE] [-c start-directory] [-d delay] [-t target-pane] [shell-command [argument ...]]",
        options: &[
            CommandOptionSpec::flag("-b", "run in the background"),
            CommandOptionSpec::flag("-C", "run as a tmux command"),
            CommandOptionSpec::flag("-E", "show stderr"),
            CommandOptionSpec::value("-c", FreeForm, "start directory"),
            CommandOptionSpec::value("-d", FreeForm, "delay in seconds"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec {
                name: "-s",
                value: Some(FreeForm),
                description: "accepted and ignored like the pin",
                completable: false,
                attached_value: false,
                optional_value: false,
                unsupported: false,
            },
        ],
        positionals: &[FreeForm],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "delete-buffer",
        aliases: &["deleteb"],
        description: "Delete a paste buffer",
        usage: "[-b buffer-name]",
        options: &[CommandOptionSpec::value("-b", FreeForm, "buffer name")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "list-buffers",
        aliases: &["lsb"],
        description: "List paste buffers",
        usage: "[-r] [-F format] [-f filter] [-O order]",
        options: &[
            CommandOptionSpec::value("-F", FreeForm, "format"),
            CommandOptionSpec::value("-f", FreeForm, "filter"),
            CommandOptionSpec::value("-O", FreeForm, "sort order"),
            CommandOptionSpec::flag("-r", "reverse sort order"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "load-buffer",
        aliases: &["loadb"],
        description: "Load a file into a paste buffer",
        usage: "[-w] [-b buffer-name] [-t target-client] path",
        options: &[
            CommandOptionSpec::value("-b", FreeForm, "buffer name"),
            CommandOptionSpec::value("-t", FreeForm, "target client"),
            CommandOptionSpec::flag("-w", "write the buffer to the client clipboard"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "paste-buffer",
        aliases: &["pasteb"],
        description: "Insert the contents of a paste buffer into a pane",
        usage: "[-dprS] [-s separator] [-b buffer-name] [-t target-pane]",
        options: &[
            CommandOptionSpec::flag("-d", "delete the buffer after pasting"),
            CommandOptionSpec::flag("-p", "bracketed paste"),
            CommandOptionSpec::flag("-r", "do not translate line endings"),
            CommandOptionSpec::flag("-S", "paste the separator"),
            CommandOptionSpec::value("-s", FreeForm, "separator"),
            CommandOptionSpec::value("-b", FreeForm, "buffer name"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "save-buffer",
        aliases: &["saveb"],
        description: "Save a paste buffer to a file",
        usage: "[-a] [-b buffer-name] path",
        options: &[
            CommandOptionSpec::flag("-a", "append to the file"),
            CommandOptionSpec::value("-b", FreeForm, "buffer name"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "set-buffer",
        aliases: &["setb"],
        description: "Set the contents of a paste buffer",
        usage: "[-aw] [-b buffer-name] [-n new-buffer-name] [-t target-client] [data]",
        options: &[
            CommandOptionSpec::flag("-a", "append to the buffer"),
            CommandOptionSpec::value("-b", FreeForm, "buffer name"),
            CommandOptionSpec::value("-n", FreeForm, "rename the source buffer"),
            CommandOptionSpec::value("-t", FreeForm, "target client"),
            CommandOptionSpec::flag("-w", "write the buffer to the client clipboard"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "show-buffer",
        aliases: &["showb"],
        description: "Display the contents of a paste buffer",
        usage: "[-b buffer-name]",
        options: &[CommandOptionSpec::value("-b", FreeForm, "buffer name")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "wait-for",
        aliases: &["wait"],
        description: "Block or wake a client on a named channel",
        usage: "[-L|-S|-U] channel",
        options: &[
            CommandOptionSpec::flag("-L", "lock the channel"),
            CommandOptionSpec::flag("-S", "signal the channel"),
            CommandOptionSpec::flag("-U", "unlock the channel"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "pipe-pane",
        aliases: &["pipep"],
        description: "Pipe pane input or output to a shell command",
        usage: "[-IOo] [-t target-pane] [shell-command]",
        options: &[
            CommandOptionSpec::flag("-I", "pipe input to the pane"),
            CommandOptionSpec::flag("-O", "pipe output from the pane"),
            CommandOptionSpec::flag("-o", "toggle an existing pipe"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "display-popup",
        aliases: &["popup"],
        description: "Display a popup running a shell command",
        usage: "[-BCEkN] [-b border-lines] [-c target-client] [-d start-directory] [-e environment] [-h height] [-s style] [-S border-style] [-t target-pane] [-T title] [-w width] [-x position] [-y position] [shell-command [argument ...]]",
        options: &[
            CommandOptionSpec::flag("-B", "no border"),
            CommandOptionSpec::flag("-C", "close any popup"),
            CommandOptionSpec::flag("-E", "close when the command exits"),
            CommandOptionSpec::flag("-k", "kill the popup on close"),
            CommandOptionSpec::flag("-N", "no fallback shell"),
            CommandOptionSpec::value("-b", FreeForm, "border lines"),
            CommandOptionSpec::value("-c", FreeForm, "target client"),
            CommandOptionSpec::value("-d", FreeForm, "start directory"),
            CommandOptionSpec::value("-e", FreeForm, "environment"),
            CommandOptionSpec::value("-h", FreeForm, "height"),
            CommandOptionSpec::value("-s", FreeForm, "style"),
            CommandOptionSpec::value("-S", FreeForm, "border style"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-T", FreeForm, "title"),
            CommandOptionSpec::value("-w", FreeForm, "width"),
            CommandOptionSpec::value("-x", FreeForm, "x position"),
            CommandOptionSpec::value("-y", FreeForm, "y position"),
        ],
        positionals: &[FreeForm],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "display-menu",
        aliases: &["menu"],
        description: "Display a menu on a client",
        usage: "[-MO] [-b border-lines] [-c target-client] [-C starting-choice] [-H selected-style] [-s style] [-S border-style] [-t target-pane] [-T title] [-x position] [-y position] name [key] [command] ...",
        options: &[
            CommandOptionSpec::flag("-M", "mouse-only menu"),
            CommandOptionSpec::flag("-O", "stay open"),
            CommandOptionSpec::value("-b", FreeForm, "border lines"),
            CommandOptionSpec::value("-c", FreeForm, "target client"),
            CommandOptionSpec::value("-C", FreeForm, "starting choice"),
            CommandOptionSpec::value("-H", FreeForm, "selected style"),
            CommandOptionSpec::value("-s", FreeForm, "style"),
            CommandOptionSpec::value("-S", FreeForm, "border style"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-T", FreeForm, "title"),
            CommandOptionSpec::value("-x", FreeForm, "x position"),
            CommandOptionSpec::value("-y", FreeForm, "y position"),
        ],
        positionals: &[FreeForm],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "confirm-before",
        aliases: &["confirm"],
        description: "Ask for confirmation before running a command",
        usage: "[-by] [-c confirm-key] [-p prompt] [-t target-client] command",
        options: &[
            CommandOptionSpec::flag("-b", "run in the background"),
            CommandOptionSpec::flag("-y", "default to yes"),
            CommandOptionSpec::value("-c", FreeForm, "confirm key"),
            CommandOptionSpec::value("-p", FreeForm, "prompt"),
            CommandOptionSpec::value("-t", FreeForm, "target client"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "lock-client",
        aliases: &["lockc"],
        description: "Lock a client",
        usage: "[-t target-client]",
        options: &[CommandOptionSpec::value("-t", FreeForm, "target client")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "lock-server",
        aliases: &["lock"],
        description: "Lock every client",
        usage: "",
        options: &[],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "lock-session",
        aliases: &["locks"],
        description: "Lock every client attached to a session",
        usage: "[-t target-session]",
        options: &[CommandOptionSpec::value("-t", Session, "target session")],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "switch-client",
        aliases: &["switchc"],
        description: "Switch an attached client to another session",
        usage: "[-ElnprZ] [-c target-client] [-t target-session] [-T key-table] [-O order]",
        options: &[
            CommandOptionSpec::flag("-E", "do not update the session environment"),
            CommandOptionSpec::flag("-l", "switch to the last session"),
            CommandOptionSpec::flag("-n", "switch to the next session"),
            CommandOptionSpec::flag("-p", "switch to the previous session"),
            CommandOptionSpec::flag("-r", "toggle read-only mode"),
            CommandOptionSpec::flag("-Z", "preserve zoom when selecting a pane"),
            CommandOptionSpec::value("-c", FreeForm, "target client"),
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::value("-T", KeyTable, "key table"),
            CommandOptionSpec::value("-O", FreeForm, "session sort order"),
            CommandOptionSpec {
                name: "-F",
                value: None,
                description: "accepted and ignored like the pin",
                completable: false,
                attached_value: false,
                optional_value: false,
                unsupported: false,
            },
        ],
        positionals: &[],
        variadic: None,
    },
];

/// Every tmux-compatible command currently executable by the mux engine.
pub static COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "new-session",
        aliases: &["new"],
        description: "Create a new session",
        usage: "[-AdDEPX] [-c start-directory] [-e environment] [-F format] [-f flags] [-n window-name] [-s session-name] [-t target-session] [-x width] [-y height] [shell-command [argument ...]]",
        options: &[
            CommandOptionSpec::flag("-d", "do not attach"),
            CommandOptionSpec::flag("-A", "attach to the named session when it exists"),
            CommandOptionSpec::flag("-D", "with -A, detach other clients"),
            CommandOptionSpec::flag("-E", "do not apply update-environment when creating"),
            CommandOptionSpec::flag("-P", "print information about the new session"),
            CommandOptionSpec::flag(
                "-X",
                "with -A, detach other clients and hang up their parents",
            ),
            CommandOptionSpec::value("-s", FreeForm, "session name"),
            CommandOptionSpec::value("-n", FreeForm, "initial window name"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
            CommandOptionSpec::unsupported_value("-t"),
            CommandOptionSpec::value("-e", FreeForm, "session environment"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::value("-f", FreeForm, "client flags"),
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
        usage: "[-r] [-F format] [-f filter] [-O order]",
        options: &[
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::value("-f", FreeForm, "filter"),
            CommandOptionSpec::value("-O", FreeForm, "sort order"),
            CommandOptionSpec::flag("-r", "reverse sort order"),
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
        usage: "[-aC] [-f filter] [-t target-session]",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::flag("-a", "kill every other session"),
            CommandOptionSpec::flag("-C", "clear alerts in the session instead of killing"),
            CommandOptionSpec::unsupported_flag("-g"),
            CommandOptionSpec::value("-f", FreeForm, "filter sessions killed by -a"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "attach-session",
        aliases: &["attach"],
        description: "Attach to a session",
        usage: "[-dErx] [-c working-directory] [-f flags] [-t target-session]",
        options: &[
            CommandOptionSpec::flag("-d", "detach other clients"),
            CommandOptionSpec::flag("-E", "do not update the session environment"),
            CommandOptionSpec::flag("-r", "attach read-only"),
            CommandOptionSpec::value("-c", FreeForm, "working directory for the session"),
            CommandOptionSpec::value("-t", Pane, "target session, window, or pane"),
            CommandOptionSpec::value("-f", FreeForm, "client flags"),
            CommandOptionSpec::flag("-x", "detach other clients and hang up their parents"),
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
        usage: "[-aP] [-E shell-command] [-s target-session] [-t target-client]",
        options: &[
            CommandOptionSpec::flag("-a", "detach every other client"),
            CommandOptionSpec::value("-s", Session, "detach every client on the session"),
            CommandOptionSpec::value("-t", FreeForm, "target client"),
            CommandOptionSpec::value("-E", FreeForm, "replace the client with a shell command"),
            CommandOptionSpec::flag("-P", "send SIGHUP to the client's parent process"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "list-clients",
        aliases: &["lsc"],
        description: "List attached clients",
        usage: "[-r] [-F format] [-f filter] [-O order] [-t target-session]",
        options: &[
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::value("-f", FreeForm, "filter"),
            CommandOptionSpec::value("-O", FreeForm, "sort order"),
            CommandOptionSpec::flag("-r", "reverse sort order"),
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
        usage: "[-abdEkPS] [-c start-directory] [-e environment] [-F format] [-n window-name] [-t target-window] [shell-command [argument ...]]",
        options: &[
            CommandOptionSpec::value("-t", Window, "destination session or window index"),
            CommandOptionSpec::value("-n", FreeForm, "window name"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
            CommandOptionSpec::flag("-d", "do not select the new window"),
            CommandOptionSpec::flag("-a", "insert after the target window"),
            CommandOptionSpec::flag("-b", "insert before the target window"),
            CommandOptionSpec::flag("-k", "replace the window at the target index"),
            CommandOptionSpec::flag("-P", "print information about the new window"),
            CommandOptionSpec::value("-F", FreeForm, "print format"),
            CommandOptionSpec::flag("-S", "select an existing window with the same name"),
            CommandOptionSpec::flag("-E", "create an empty pane"),
            CommandOptionSpec::value("-e", FreeForm, "pane environment"),
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
        usage: "[-ar] [-F format] [-f filter] [-O order] [-t target-session]",
        options: &[
            CommandOptionSpec::value("-t", Session, "target session"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::flag("-a", "list windows from every session"),
            CommandOptionSpec::value("-f", FreeForm, "filter"),
            CommandOptionSpec::value("-O", FreeForm, "sort order"),
            CommandOptionSpec::flag("-r", "reverse sort order"),
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
        usage: "[-a] [-f filter] [-t target-window]",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-a", "kill every other window in the session"),
            CommandOptionSpec::value("-f", FreeForm, "filter windows killed by -a"),
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
        usage: "[-bdEfhkPvZ] [-c start-directory] [-e environment] [-F format] [-l size] [-m message] [-p percentage] [-R inactive-border-style] [-s style] [-S active-border-style] [-T title] [-t target-pane] [shell-command [argument ...]]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-l", FreeForm, "new pane size in cells or percent"),
            CommandOptionSpec::value("-p", FreeForm, "split percentage"),
            CommandOptionSpec::value("-c", FreeForm, "start in the current pane path"),
            CommandOptionSpec::flag("-b", "new pane goes left or above"),
            CommandOptionSpec::flag("-d", "keep focus on the current pane"),
            CommandOptionSpec::flag("-f", "span the full window"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-P", "print information about the new pane"),
            CommandOptionSpec::value("-F", FreeForm, "print format"),
            CommandOptionSpec::flag("-v", "vertical split"),
            CommandOptionSpec::value("-e", FreeForm, "pane environment"),
            CommandOptionSpec::flag("-E", "create an empty pane"),
            CommandOptionSpec::unsupported_flag("-I"),
            CommandOptionSpec::flag("-k", "retain the new pane until a key is pressed"),
            CommandOptionSpec::value("-m", FreeForm, "retained pane message"),
            CommandOptionSpec::value("-R", FreeForm, "inactive pane border style"),
            CommandOptionSpec::value("-s", FreeForm, "pane style"),
            CommandOptionSpec::value("-S", FreeForm, "active pane border style"),
            CommandOptionSpec::value("-T", FreeForm, "pane title"),
            CommandOptionSpec::unsupported_flag("-W"),
            CommandOptionSpec::flag("-Z", "zoom the active pane after splitting"),
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
        usage: "[-abdP] [-F format] [-n window-name] [-s src-pane] [-t dst-window]",
        options: &[
            CommandOptionSpec::value("-n", FreeForm, "new window name"),
            CommandOptionSpec::value("-s", Pane, "source pane"),
            CommandOptionSpec::value("-t", Window, "destination session or window index"),
            CommandOptionSpec::flag("-a", "insert after the destination window"),
            CommandOptionSpec::flag("-b", "insert before the destination window"),
            CommandOptionSpec::flag("-d", "do not select the new window"),
            CommandOptionSpec::flag("-P", "print information about the new window"),
            CommandOptionSpec::value("-F", FreeForm, "print format"),
            CommandOptionSpec::unsupported_flag("-W"),
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
        usage: "[-bdfhv] [-l size] [-p percentage] [-s src-pane] [-t dst-pane]",
        options: &[
            CommandOptionSpec::value("-l", FreeForm, "new pane size in cells or percent"),
            CommandOptionSpec::value("-p", FreeForm, "split percentage"),
            CommandOptionSpec::value("-s", Pane, "source pane"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-b", "place before target"),
            CommandOptionSpec::flag("-d", "do not select moved pane"),
            CommandOptionSpec::flag("-f", "fill target space"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "move-pane",
        aliases: &["movep"],
        description: "Move a pane into another window",
        usage: "[-bdfhv] [-l size] [-s src-pane] [-t dst-pane]",
        options: &[
            CommandOptionSpec::value("-l", FreeForm, "new pane size in cells or percent"),
            CommandOptionSpec::value("-s", Pane, "source pane"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-b", "place before target"),
            CommandOptionSpec::flag("-d", "do not select moved pane"),
            CommandOptionSpec::flag("-f", "fill target space"),
            CommandOptionSpec::flag("-h", "horizontal split"),
            CommandOptionSpec::flag("-v", "vertical split"),
            CommandOptionSpec::unsupported_optional_value("-D"),
            CommandOptionSpec::unsupported_optional_value("-L"),
            CommandOptionSpec::unsupported_value("-P"),
            CommandOptionSpec::unsupported_optional_value("-R"),
            CommandOptionSpec::unsupported_optional_value("-U"),
            CommandOptionSpec::unsupported_value("-X"),
            CommandOptionSpec::unsupported_value("-Y"),
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
        usage: "[-DdegLlMmRUZ] [-P style] [-T title] [-t target-pane]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-T", FreeForm, "pane title"),
            CommandOptionSpec::value("-P", FreeForm, "pane style"),
            CommandOptionSpec::flag("-D", "pane below"),
            CommandOptionSpec::flag("-L", "pane to the left"),
            CommandOptionSpec::flag("-M", "clear the marked pane"),
            CommandOptionSpec::flag("-R", "pane to the right"),
            CommandOptionSpec::flag("-U", "pane above"),
            CommandOptionSpec::flag("-Z", "preserve zoom"),
            CommandOptionSpec::flag("-d", "disable input to the pane"),
            CommandOptionSpec::flag("-e", "enable input to the pane"),
            CommandOptionSpec::flag("-g", "print the pane style"),
            CommandOptionSpec::flag("-l", "last pane"),
            CommandOptionSpec::flag("-m", "mark the pane"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "last-pane",
        aliases: &["lastp"],
        description: "Select the last pane",
        usage: "[-deZ] [-t target-window]",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-d", "disable input to the last pane"),
            CommandOptionSpec::flag("-e", "enable input to the last pane"),
            CommandOptionSpec::flag("-Z", "preserve zoom"),
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
        usage: "[-asr] [-F format] [-f filter] [-O order] [-t target-window]",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::value("-f", FreeForm, "filter"),
            CommandOptionSpec::value("-O", FreeForm, "sort order"),
            CommandOptionSpec::flag("-a", "list panes from every session"),
            CommandOptionSpec::flag("-r", "reverse sort order"),
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
            CommandOptionSpec::optional_value("-D", "resize downward by an optional amount"),
            CommandOptionSpec::optional_value("-L", "resize left by an optional amount"),
            CommandOptionSpec::optional_value("-R", "resize right by an optional amount"),
            CommandOptionSpec::optional_value("-U", "resize upward by an optional amount"),
            CommandOptionSpec::flag("-Z", "toggle zoom"),
            CommandOptionSpec::unsupported_flag("-M"),
            CommandOptionSpec::unsupported_flag("-T"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "resize-window",
        aliases: &["resizew"],
        description: "Resize a window and select manual sizing",
        usage: "[-aADLRU] [-x width] [-y height] [-t target-window] [adjustment]",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::value("-x", FreeForm, "width in cells"),
            CommandOptionSpec::value("-y", FreeForm, "height in cells"),
            CommandOptionSpec::flag("-D", "increase height"),
            CommandOptionSpec::flag("-L", "decrease width"),
            CommandOptionSpec::flag("-R", "increase width"),
            CommandOptionSpec::flag("-U", "decrease height"),
            CommandOptionSpec::flag("-a", "resize to the smallest attached client"),
            CommandOptionSpec::flag("-A", "resize to the largest attached client"),
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
        usage: "[-a] [-f filter] [-t target-pane]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-a", "kill every other pane in the window"),
            CommandOptionSpec::value("-f", FreeForm, "filter panes killed by -a"),
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
        usage: "[-FHlRX] [-c target-client] [-N repeat-count] [-t target-pane] [key ...]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::value("-N", FreeForm, "repeat count"),
            CommandOptionSpec::flag("-X", "copy-mode command"),
            CommandOptionSpec::flag("-H", "keys are hexadecimal character codes"),
            CommandOptionSpec::flag("-F", "compatibility no-op"),
            CommandOptionSpec::flag("-l", "literal text"),
            CommandOptionSpec::value("-c", FreeForm, "target client"),
            CommandOptionSpec::unsupported_flag("-K"),
            CommandOptionSpec::unsupported_flag("-M"),
            CommandOptionSpec::flag("-R", "reset the pane terminal state"),
        ],
        positionals: &[],
        variadic: Some(FreeForm),
    },
    CommandSpec {
        name: "send-prefix",
        aliases: &[],
        description: "Send the prefix key to a pane",
        usage: "[-2] [-t target-pane]",
        options: &[
            CommandOptionSpec::flag("-2", "send the secondary prefix key"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
        ],
        positionals: &[],
        variadic: None,
    },
    CommandSpec {
        name: "copy-mode",
        aliases: &[],
        description: "Enter copy mode",
        usage: "[-deHMqu] [-t target-pane]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-d", "scroll one page down"),
            CommandOptionSpec::flag("-u", "scroll one page up"),
            CommandOptionSpec::flag("-e", "exit copy mode at the bottom of history"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::flag("-H", "hide the copy position indicator"),
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
        usage: "[-1CbeiklN] [-I inputs] [-p prompts] [-T prompt-type] [template]",
        options: &[
            CommandOptionSpec::value("-I", FreeForm, "initial input"),
            CommandOptionSpec::value("-p", FreeForm, "prompt label"),
            CommandOptionSpec::value("-T", FreeForm, "prompt type: command or search"),
            CommandOptionSpec::flag("-1", "submit the first key pressed"),
            CommandOptionSpec::flag("-b", "background prompt, always on in zz"),
            CommandOptionSpec::flag("-C", "keep publishing terminal frames while open"),
            CommandOptionSpec::flag("-e", "exit when backspace empties the prompt"),
            CommandOptionSpec::flag("-i", "run the template on every edit"),
            CommandOptionSpec::flag("-k", "submit the name of the first key pressed"),
            CommandOptionSpec::flag("-l", "one prompt and one input, commas kept literal"),
            CommandOptionSpec::flag("-N", "collect digits and pass the first non-digit on"),
            CommandOptionSpec::unsupported_flag("-F"),
            CommandOptionSpec::unsupported_flag("-P"),
            CommandOptionSpec::unsupported_value("-t"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "choose-tree",
        aliases: &[],
        description: "Choose a session, window, or pane",
        usage: "[-NrswZ] [-f filter] [-K key-format] [-O order] [-t target-pane] [template]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-s", "show sessions"),
            CommandOptionSpec::flag("-w", "show windows"),
            CommandOptionSpec::flag("-Z", "zoom the chooser, always full window in zz"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::value("-f", FreeForm, "filter"),
            CommandOptionSpec::unsupported_flag("-G"),
            CommandOptionSpec::unsupported_flag("-h"),
            CommandOptionSpec::value("-K", FreeForm, "per-row shortcut key format"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::flag("-N", "disable the preview, already zz's only layout"),
            CommandOptionSpec::value("-O", FreeForm, "sort order"),
            CommandOptionSpec::flag("-r", "reverse sort order"),
            CommandOptionSpec::unsupported_flag("-y"),
        ],
        positionals: &[FreeForm],
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
        usage: "[-NrZ] [-f filter] [-K key-format] [-O order] [-t target-pane] [template]",
        options: &[
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-Z", "zoom the chooser, always full window in zz"),
            CommandOptionSpec::unsupported_value("-F"),
            CommandOptionSpec::value("-f", FreeForm, "filter"),
            CommandOptionSpec::value("-K", FreeForm, "per-row shortcut key format"),
            CommandOptionSpec::unsupported_flag("-k"),
            CommandOptionSpec::flag("-N", "disable the preview, already zz's only layout"),
            CommandOptionSpec::value("-O", FreeForm, "sort order"),
            CommandOptionSpec::flag("-r", "reverse sort order"),
            CommandOptionSpec::unsupported_flag("-y"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "display-message",
        aliases: &["display"],
        description: "Display or print a formatted message",
        usage: "[-ClNp] [-c target-client] [-d delay] [-F format] [-t target-pane] [message]",
        options: &[
            CommandOptionSpec::flag("-p", "print the message"),
            CommandOptionSpec::flag("-C", "keep terminal updates flowing while it shows"),
            CommandOptionSpec::value("-c", FreeForm, "destination client"),
            CommandOptionSpec::value("-d", FreeForm, "milliseconds to show the message"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::flag("-l", "do not expand the message template"),
            CommandOptionSpec::flag("-N", "ignore key presses while the message shows"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::unsupported_flag("-a"),
            CommandOptionSpec::unsupported_flag("-I"),
            CommandOptionSpec::unsupported_flag("-v"),
        ],
        positionals: &[FreeForm],
        variadic: None,
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
        usage: "[-bN] [-d duration] [-t target-client] [template]",
        options: &[
            CommandOptionSpec::value("-d", FreeForm, "duration in milliseconds"),
            CommandOptionSpec::flag("-b", "do not block other commands, always on in zz"),
            CommandOptionSpec::flag("-N", "disable pane selection"),
            CommandOptionSpec::value("-t", FreeForm, "target client"),
        ],
        positionals: &[FreeForm],
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
        usage: "[-anq] [-T key-table] key",
        options: &[
            CommandOptionSpec::value("-T", KeyTable, "key table"),
            CommandOptionSpec::flag("-a", "remove every binding in the key table"),
            CommandOptionSpec::flag("-n", "root table"),
            CommandOptionSpec::flag("-q", "suppress errors"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "list-keys",
        aliases: &["lsk"],
        description: "List key bindings",
        usage: "[-1aNr] [-F format] [-O order] [-P prefix-string][-T key-table] [key]",
        options: &[
            CommandOptionSpec::flag("-1", "show only the first binding"),
            CommandOptionSpec::value("-F", FreeForm, "output format"),
            CommandOptionSpec::value("-O", FreeForm, "sort order"),
            CommandOptionSpec::value("-P", FreeForm, "displayed prefix string"),
            CommandOptionSpec::value("-T", KeyTable, "key table"),
            CommandOptionSpec::flag("-a", "include bindings without notes"),
            CommandOptionSpec::flag("-N", "list key notes"),
            CommandOptionSpec::flag("-r", "reverse the sort order"),
        ],
        positionals: &[FreeForm],
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
        name: "set-hook",
        aliases: &[],
        description: "Set or immediately run a hook",
        usage: "[-agpRuw] [-B name:what:format] [-t target-pane] [hook] [command]",
        options: &[
            CommandOptionSpec::value("-B", FreeForm, "format monitor"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-a", "append"),
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::flag("-p", "pane scope"),
            CommandOptionSpec::flag("-R", "run immediately"),
            CommandOptionSpec::flag("-u", "unset"),
            CommandOptionSpec::flag("-w", "window scope"),
        ],
        positionals: &[FreeForm, FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "show-hooks",
        aliases: &[],
        description: "Show hooks",
        usage: "[-Bgpw] [-t target-pane] [hook]",
        options: &[
            CommandOptionSpec::flag("-B", "show format monitors"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::flag("-p", "pane scope"),
            CommandOptionSpec::flag("-w", "window scope"),
        ],
        positionals: &[FreeForm],
        variadic: None,
    },
    CommandSpec {
        name: "set-option",
        aliases: &["set"],
        description: "Set a server, session, window, or pane option",
        usage: "[-aFgopqsuUw] [-t target-pane] option [value]",
        options: &[
            CommandOptionSpec::value("-t", FreeForm, "target"),
            CommandOptionSpec::flag("-a", "append"),
            CommandOptionSpec::flag("-F", "expand the option value as a format"),
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::flag("-o", "set only if unset"),
            CommandOptionSpec::flag("-p", "pane scope"),
            CommandOptionSpec::flag("-q", "quiet"),
            CommandOptionSpec::flag("-s", "server scope"),
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
        usage: "[-aFgoqu] [-t target-window] option [value]",
        options: &[
            CommandOptionSpec::value("-t", Window, "target window"),
            CommandOptionSpec::flag("-a", "append"),
            CommandOptionSpec::flag("-F", "expand the option value as a format"),
            CommandOptionSpec::flag("-g", "global scope"),
            CommandOptionSpec::flag("-o", "set only if unset"),
            CommandOptionSpec::flag("-q", "quiet"),
            CommandOptionSpec::flag("-u", "unset"),
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
        usage: "[-Fnqv] [-t target-pane] path ...",
        options: &[
            CommandOptionSpec::flag("-F", "expand formats in file paths"),
            CommandOptionSpec::flag("-n", "parse without applying commands"),
            CommandOptionSpec::flag("-q", "do not report a missing file"),
            CommandOptionSpec::value("-t", Pane, "target pane"),
            CommandOptionSpec::flag("-v", "print parsed commands"),
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

#[must_use]
pub fn catalog_command_spec(name: &str) -> Option<&'static CommandSpec> {
    command_spec(name).or_else(|| {
        DAEMON_COMMAND_SPECS
            .iter()
            .find(|spec| spec.name == name || spec.aliases.contains(&name))
    })
}

#[must_use]
pub fn unimplemented_tmux_command_spec(name: &str) -> Option<&'static CommandSpec> {
    UNIMPLEMENTED_TMUX_COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
}

pub fn command_specs() -> impl Iterator<Item = &'static CommandSpec> {
    COMMAND_SPECS.iter().chain(DAEMON_COMMAND_SPECS)
}

/// Resolve a known alias while preserving unknown input for structured errors.
#[must_use]
pub fn canonical_command(command: &str) -> &str {
    match resolve_command(command) {
        CommandResolution::Canonical(name) | CommandResolution::Unimplemented(name) => name,
        CommandResolution::Ambiguous(_) | CommandResolution::Unknown => command,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResolution {
    Canonical(&'static str),
    Unimplemented(&'static str),
    Ambiguous(String),
    Unknown,
}

#[must_use]
pub fn resolve_command(command: &str) -> CommandResolution {
    if command.is_empty() {
        return CommandResolution::Unknown;
    }
    for (name, aliases, implemented) in command_table() {
        if name == command || aliases.contains(&command) {
            return resolved(name, implemented);
        }
    }
    prefix_resolution(command, false)
        .or_else(|| prefix_resolution(command, true))
        .unwrap_or(CommandResolution::Unknown)
}

fn prefix_resolution(command: &str, native: bool) -> Option<CommandResolution> {
    let matches = command_table()
        .filter(|(name, _, _)| NATIVE_COMMAND_NAMES.contains(name) == native)
        .filter(|(name, _, _)| name.starts_with(command))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => None,
        [(name, _, implemented)] => Some(resolved(name, *implemented)),
        _ => Some(CommandResolution::Ambiguous(format!(
            "ambiguous command: {command}, could be: {}",
            matches
                .iter()
                .map(|(name, _, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn resolved(name: &'static str, implemented: bool) -> CommandResolution {
    if implemented {
        CommandResolution::Canonical(name)
    } else {
        CommandResolution::Unimplemented(name)
    }
}

fn command_table() -> impl Iterator<Item = (&'static str, &'static [&'static str], bool)> {
    static TABLE: std::sync::OnceLock<Vec<(&'static str, &'static [&'static str], bool)>> =
        std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mut entries: Vec<(&'static str, &'static [&'static str], bool)> = COMMAND_SPECS
                .iter()
                .chain(DAEMON_COMMAND_SPECS)
                .map(|spec| (spec.name, spec.aliases, true))
                .chain(
                    UNIMPLEMENTED_TMUX_COMMAND_SPECS
                        .iter()
                        .map(|spec| (spec.name, spec.aliases, false)),
                )
                .collect();
            entries.sort_unstable_by_key(|(name, _, _)| *name);
            entries
        })
        .iter()
        .copied()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use serde::Deserialize;

    use super::*;

    #[test]
    fn unimplemented_table_matches_the_flat_list() {
        let mut flat: Vec<&str> = UNIMPLEMENTED_TMUX_COMMANDS.to_vec();
        flat.sort_unstable();
        let mut structured: Vec<&str> = UNIMPLEMENTED_TMUX_COMMAND_SPECS
            .iter()
            .flat_map(|spec| std::iter::once(spec.name).chain(spec.aliases.iter().copied()))
            .collect();
        structured.sort_unstable();
        assert_eq!(flat, structured);
    }

    #[test]
    fn command_resolution_follows_the_pin_contract() {
        assert_eq!(
            resolve_command("show-option"),
            CommandResolution::Canonical("show-options")
        );
        assert_eq!(
            resolve_command("show-options"),
            CommandResolution::Canonical("show-options")
        );
        assert_eq!(
            resolve_command("kill-s"),
            CommandResolution::Ambiguous(
                "ambiguous command: kill-s, could be: kill-server, kill-session".to_owned()
            )
        );
        assert_eq!(
            resolve_command("switch-c"),
            CommandResolution::Canonical("switch-client")
        );
        assert_eq!(
            resolve_command("switchc"),
            CommandResolution::Canonical("switch-client")
        );
        assert_eq!(
            resolve_command("capture-pan"),
            CommandResolution::Canonical("capture-pane")
        );
        assert_eq!(
            resolve_command("list-buf"),
            CommandResolution::Canonical("list-buffers")
        );
        assert_eq!(resolve_command("wibble"), CommandResolution::Unknown);
        assert_eq!(resolve_command(""), CommandResolution::Unknown);
        assert_eq!(canonical_command("show-option"), "show-options");
        assert_eq!(canonical_command("kill-s"), "kill-s");
        for (spelling, canonical) in [
            ("attach", "attach-session"),
            ("capturep", "capture-pane"),
            ("setb", "set-buffer"),
            ("splitw", "split-window"),
        ] {
            assert_eq!(
                resolve_command(spelling),
                CommandResolution::Canonical(canonical)
            );
        }
        for (spelling, canonical) in [
            ("agent-s", "agent-send"),
            ("capture-b", "capture-browser"),
            ("copy-mode-s", "copy-mode-search-prompt"),
            ("debug-m", "debug-marker"),
            ("select-pane-k", "select-pane-kind"),
            ("send-last", "send-last-output"),
            ("tool", "tools"),
        ] {
            assert_eq!(
                resolve_command(spelling),
                CommandResolution::Canonical(canonical)
            );
            assert_eq!(catalog_command_spec(canonical).unwrap().name, canonical);
        }
        for canonical in [
            "capture-browser",
            "copy-mode-search-prompt",
            "select-pane-kind",
            "split-browser",
            "split-picker",
        ] {
            assert_eq!(
                resolve_command(canonical),
                CommandResolution::Canonical(canonical)
            );
        }
    }

    #[test]
    fn native_names_do_not_change_the_affected_tmux_prefixes() {
        for (prefix, canonical) in [
            ("a", "attach-session"),
            ("ca", "capture-pane"),
            ("cap", "capture-pane"),
            ("capt", "capture-pane"),
            ("captu", "capture-pane"),
            ("captur", "capture-pane"),
            ("capture", "capture-pane"),
            ("capture-", "capture-pane"),
            ("cop", "copy-mode"),
            ("copy", "copy-mode"),
            ("copy-", "copy-mode"),
            ("copy-m", "copy-mode"),
            ("copy-mo", "copy-mode"),
            ("copy-mod", "copy-mode"),
            ("f", "find-window"),
            ("select-p", "select-pane"),
            ("select-pa", "select-pane"),
            ("select-pan", "select-pane"),
            ("set-b", "set-buffer"),
            ("set-e", "set-environment"),
            ("sp", "split-window"),
            ("spl", "split-window"),
            ("spli", "split-window"),
            ("split", "split-window"),
            ("split-", "split-window"),
        ] {
            assert_eq!(
                resolve_command(prefix),
                CommandResolution::Canonical(canonical),
                "{prefix}"
            );
        }
    }

    fn usage_options(usage: &str) -> BTreeMap<String, bool> {
        let mut options = BTreeMap::new();
        for usage_option in usage.split("[-").skip(1) {
            let (body, _) = usage_option
                .split_once(']')
                .expect("usage option has a closing bracket");
            let mut tokens = body.split_ascii_whitespace();
            let flags = tokens.next().expect("usage option has a flag");
            let takes_value = tokens.next().is_some();
            assert!(tokens.next().is_none(), "usage option has one value label");
            let names = if flags.starts_with('-') {
                vec![format!("-{flags}")]
            } else if flags.contains('|') {
                flags
                    .split('|')
                    .map(|flag| format!("-{}", flag.trim_start_matches('-')))
                    .collect()
            } else {
                flags.chars().map(|flag| format!("-{flag}")).collect()
            };
            if takes_value {
                assert_eq!(names.len(), 1, "valued usage option is not grouped");
            }
            for name in names {
                assert!(
                    options.insert(name.clone(), takes_value).is_none(),
                    "duplicate usage option {name}"
                );
            }
        }
        options
    }

    fn accepted_options(spec: &CommandSpec) -> BTreeMap<String, bool> {
        spec.options
            .iter()
            .filter(|option| !option.unsupported)
            .map(|option| {
                (
                    option.name.to_owned(),
                    option.value.is_some() || option.attached_value,
                )
            })
            .collect()
    }

    #[derive(Debug, Deserialize)]
    struct Oracle {
        commands: Vec<OracleCommand>,
    }

    #[derive(Debug, Deserialize)]
    struct OracleCommand {
        name: String,
        aliases: Vec<String>,
        usage: String,
        flags: BTreeMap<String, String>,
        min_args: usize,
        max_args: Option<usize>,
    }

    fn oracle_commands() -> Vec<OracleCommand> {
        serde_json::from_str::<Oracle>(include_str!("../../../compat/tmux-oracle.json"))
            .expect("pinned tmux oracle")
            .commands
    }

    fn catalog_flag_shapes(spec: &CommandSpec) -> BTreeMap<String, String> {
        spec.options
            .iter()
            .map(|option| {
                let arity = if option.optional_value {
                    "optional"
                } else if option.value.is_some() || option.attached_value {
                    "required"
                } else {
                    "none"
                };
                (option.name.to_owned(), arity.to_owned())
            })
            .collect()
    }

    fn owned(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect()
    }

    #[test]
    fn tmux_option_catalog_matches_the_pinned_oracle() {
        let oracle = oracle_commands();
        assert_eq!(oracle.len(), 92);
        let upstream = oracle
            .iter()
            .map(|command| command.name.as_str())
            .collect::<BTreeSet<_>>();
        let mut implemented = 0;
        let mut aliases = 0;
        let mut flag_shapes = BTreeMap::<&str, usize>::new();
        let mut supported = 0;
        let mut unsupported = 0;
        let mut usage_overrides = BTreeSet::new();
        for command in &oracle {
            let Some(spec) = catalog_command_spec(&command.name) else {
                continue;
            };
            implemented += 1;
            aliases += command.aliases.len();
            for arity in command.flags.values() {
                *flag_shapes.entry(arity).or_default() += 1;
            }
            for option in spec.options {
                if option.unsupported {
                    unsupported += 1;
                } else {
                    supported += 1;
                }
            }
            assert!(spec.uses_tmux_option_grammar(), "{}", spec.name);
            assert_eq!(spec.aliases, command.aliases, "aliases for {}", spec.name);
            assert_eq!(
                spec.pinned_tmux_usage(),
                command.usage,
                "usage for {}",
                spec.name
            );
            assert_eq!(
                catalog_flag_shapes(spec),
                command.flags,
                "flags for {}",
                spec.name
            );
            if spec.usage != command.usage {
                usage_overrides.insert(spec.name);
            }
        }
        assert_eq!(implemented, 83);
        assert_eq!(aliases, 74);
        assert_eq!(flag_shapes.values().sum::<usize>(), 503);
        assert_eq!(
            flag_shapes,
            BTreeMap::from([("none", 280), ("optional", 8), ("required", 215)])
        );
        assert_eq!((supported, unsupported), (453, 50));
        assert_eq!(usage_overrides.len(), 22);
        assert_eq!(
            usage_overrides,
            PINNED_TMUX_USAGE_OVERRIDES
                .iter()
                .map(|(name, _)| *name)
                .collect()
        );
        for spec in command_specs().filter(|spec| spec.uses_tmux_option_grammar()) {
            assert!(upstream.contains(spec.name), "{}", spec.name);
        }
    }

    #[test]
    fn unimplemented_tmux_syntax_matches_the_pinned_oracle() {
        let oracle = oracle_commands()
            .into_iter()
            .map(|command| (command.name.clone(), command))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(UNIMPLEMENTED_TMUX_COMMAND_SPECS.len(), 9);
        for spec in UNIMPLEMENTED_TMUX_COMMAND_SPECS {
            let command = &oracle[spec.name];
            assert_eq!(spec.aliases, command.aliases, "aliases for {}", spec.name);
            assert_eq!(spec.usage, command.usage, "usage for {}", spec.name);
            assert_eq!(
                catalog_flag_shapes(spec),
                command.flags,
                "flags for {}",
                spec.name
            );
            assert_eq!(
                spec.positional_minimum(),
                command.min_args,
                "minimum arguments for {}",
                spec.name
            );
            assert_eq!(
                spec.positional_maximum(),
                command.max_args,
                "maximum arguments for {}",
                spec.name
            );
        }
    }

    #[test]
    fn tmux_option_diagnostics_use_the_canonical_command() {
        let spec = command_spec("display").expect("display-message alias");
        for (arguments, expected) in [
            (
                vec!["-?"],
                format!("usage: display-message {}", spec.pinned_tmux_usage()),
            ),
            (
                vec!["-Q"],
                "command display-message: unknown flag -Q".to_owned(),
            ),
            (
                vec!["-@"],
                "command display-message: invalid flag -@".to_owned(),
            ),
            (
                vec!["--bogus"],
                "command display-message: invalid flag --".to_owned(),
            ),
        ] {
            let arguments = owned(&arguments);
            assert_eq!(
                parse_tmux_options(spec, &arguments)
                    .expect_err("tmux option diagnostic")
                    .tmux_message(),
                expected
            );
        }

        let spec = command_spec("kill-server").expect("kill-server");
        let arguments = owned(&["-?"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments)
                .expect_err("usage")
                .tmux_message(),
            "usage: kill-server "
        );

        let spec = command_spec("attach-session").expect("attach-session");
        let arguments = owned(&["-xQ"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments)
                .expect_err("later unknown flag")
                .tmux_message(),
            "command attach-session: unknown flag -Q"
        );

        let spec = catalog_command_spec("detach-client").expect("detach-client");
        let arguments = owned(&["-E"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments)
                .expect_err("missing unsupported value")
                .tmux_message(),
            "command detach-client: -E expects an argument"
        );
    }

    #[test]
    fn tmux_required_values_accept_attached_and_flag_looking_arguments() {
        let spec = command_spec("list-keys").expect("list-keys");
        let arguments = owned(&["-rOname", "tail"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments).expect("attached value"),
            TmuxOptionParse {
                options: vec![TmuxOption::Flag("-r"), TmuxOption::Value("-O", "name")],
                positionals: &arguments[1..],
            }
        );

        let arguments = owned(&["-O", "-?"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments).expect("separated value"),
            TmuxOptionParse {
                options: vec![TmuxOption::Value("-O", "-?")],
                positionals: &[],
            }
        );

        let arguments = owned(&["-O"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments)
                .expect_err("missing value")
                .tmux_message(),
            "command list-keys: -O expects an argument"
        );
    }

    #[test]
    fn tmux_optional_values_follow_the_pinned_lookahead() {
        let spec = command_spec("resize-pane").expect("resize-pane");
        for value in ["-?", "-@", "-2", "-"] {
            let arguments = owned(&["-D", value]);
            assert_eq!(
                parse_tmux_options(spec, &arguments).expect("optional value"),
                TmuxOptionParse {
                    options: vec![TmuxOption::Value("-D", value)],
                    positionals: &[],
                }
            );
        }

        let arguments = owned(&["-D10"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments).expect("attached optional value"),
            TmuxOptionParse {
                options: vec![TmuxOption::Value("-D", "10")],
                positionals: &[],
            }
        );

        let arguments = owned(&["-D", "-Z"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments).expect("next option"),
            TmuxOptionParse {
                options: vec![TmuxOption::Flag("-D"), TmuxOption::Flag("-Z")],
                positionals: &[],
            }
        );

        let arguments = owned(&["-D", "--bogus"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments)
                .expect_err("invalid long option")
                .tmux_message(),
            "command resize-pane: invalid flag --"
        );

        let arguments = owned(&["-D"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments).expect("missing optional value"),
            TmuxOptionParse {
                options: vec![TmuxOption::Flag("-D")],
                positionals: &[],
            }
        );
    }

    #[test]
    fn if_shell_args_parse_distinguishes_conditions_branches_and_option_values() {
        let spec = catalog_command_spec("if-shell").expect("if-shell");

        let condition = CommandInvocation::new(
            "if-shell",
            [
                "-F",
                "{ display-message condition }",
                "display-message branch",
            ],
        )
        .with_command_blocks([1]);
        assert_eq!(
            parse_tmux_command_options(spec, &condition)
                .expect_err("typed condition")
                .tmux_message(),
            "command if-shell: argument 1 must be \"string\""
        );

        let branches = CommandInvocation::new(
            "if-shell",
            [
                "-F",
                "1",
                "{ display-message true }",
                "{ display-message false }",
            ],
        )
        .with_command_blocks([2, 3]);
        let parsed = parse_tmux_command_options(spec, &branches).expect("typed branches");
        assert_eq!(parsed.positionals, &branches.args[1..]);

        let extra = CommandInvocation::new(
            "if-shell",
            ["1", "display-message true", "display-message false", "{}"],
        )
        .with_command_blocks([3]);
        assert_eq!(
            parse_tmux_command_options(spec, &extra)
                .expect_err("typed extra positional")
                .tmux_message(),
            "command if-shell: argument 4 must be \"string\""
        );

        let target = CommandInvocation::new(
            "if-shell",
            [
                "-t",
                "{ display-message target }",
                "1",
                "display-message branch",
            ],
        )
        .with_command_blocks([1]);
        assert_eq!(
            parse_tmux_command_options(spec, &target)
                .expect_err("typed option value")
                .tmux_message(),
            "command if-shell: -t argument must be a string"
        );

        let quoted = CommandInvocation::new("if-shell", ["-F", "1", "{ display-message quoted }"]);
        assert!(parse_tmux_command_options(spec, &quoted).is_ok());
    }

    #[test]
    fn run_shell_args_parse_switches_every_positional_with_leading_command_mode() {
        let spec = catalog_command_spec("run-shell").expect("run-shell");

        for (command, expected) in [
            (
                CommandInvocation::new("run-shell", ["{ display-message first }"])
                    .with_command_blocks([0]),
                "command run-shell: argument 1 must be \"string\"",
            ),
            (
                CommandInvocation::new("run-shell", ["printf first", "{ display-message second }"])
                    .with_command_blocks([1]),
                "command run-shell: argument 2 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "run-shell",
                    ["printf first", "-C", "{ display-message third }"],
                )
                .with_command_blocks([2]),
                "command run-shell: argument 3 must be \"string\"",
            ),
        ] {
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed shell argument")
                    .tmux_message(),
                expected
            );
        }

        for command in [
            CommandInvocation::new(
                "run-shell",
                [
                    "-C",
                    "{ display-message first }",
                    "{ display-message ignored }",
                ],
            )
            .with_command_blocks([1, 2]),
            CommandInvocation::new("run-shell", ["-bC", "{ display-message first }"])
                .with_command_blocks([1]),
            CommandInvocation::new("run-shell", ["-C", "{ display-message quoted }"]),
        ] {
            parse_tmux_command_options(spec, &command).expect("command-mode argument");
        }

        for command in [
            CommandInvocation::new("run-shell", ["-Cd0", "{ display-message first }"])
                .with_command_blocks([1]),
            CommandInvocation::new("run-shell", ["-C", "--", "{ display-message first }"])
                .with_command_blocks([2]),
        ] {
            parse_tmux_command_options(spec, &command).expect("leading command-mode flag");
        }

        for (command, expected) in [
            (
                CommandInvocation::new("run-shell", ["-d0C", "{ display-message first }"])
                    .with_command_blocks([1]),
                "command run-shell: argument 1 must be \"string\"",
            ),
            (
                CommandInvocation::new("run-shell", ["-cC", "{ display-message first }"])
                    .with_command_blocks([1]),
                "command run-shell: argument 1 must be \"string\"",
            ),
            (
                CommandInvocation::new("run-shell", ["--", "-C", "{ display-message second }"])
                    .with_command_blocks([2]),
                "command run-shell: argument 2 must be \"string\"",
            ),
            (
                CommandInvocation::new("run-shell", ["-", "-C", "{ display-message third }"])
                    .with_command_blocks([2]),
                "command run-shell: argument 3 must be \"string\"",
            ),
        ] {
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("command-mode boundary")
                    .tmux_message(),
                expected
            );
        }

        for option in ["-c", "-d", "-s", "-t"] {
            let command = CommandInvocation::new(
                "run-shell",
                [
                    "-C",
                    option,
                    "{ display-message option }",
                    "display-message branch",
                ],
            )
            .with_command_blocks([2]);
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed option value")
                    .tmux_message(),
                format!("command run-shell: {option} argument must be a string")
            );
        }

        let quoted = CommandInvocation::new("run-shell", ["{ display-message shell text }"]);
        assert!(parse_tmux_command_options(spec, &quoted).is_ok());
    }

    #[test]
    fn set_option_args_parse_accepts_only_the_value_as_a_command_block() {
        for name in ["set-option", "set-window-option"] {
            let spec = catalog_command_spec(name).expect("set option command");

            for command in [
                CommandInvocation::new(name, ["-g", "@typed", "{ display-message value }"])
                    .with_command_blocks([2]),
                CommandInvocation::new(name, ["--", "@typed", "{}"]).with_command_blocks([2]),
            ] {
                parse_tmux_command_options(spec, &command).expect("typed option value");
            }

            for (command, expected) in [
                (
                    CommandInvocation::new(name, ["-g", "{ display-message name }", "value"])
                        .with_command_blocks([1]),
                    format!("command {name}: argument 1 must be \"string\""),
                ),
                (
                    CommandInvocation::new(
                        name,
                        ["-t", "{ display-message target }", "@typed", "value"],
                    )
                    .with_command_blocks([1]),
                    format!("command {name}: -t argument must be a string"),
                ),
                (
                    CommandInvocation::new(name, ["@typed", "value", "{ display-message extra }"])
                        .with_command_blocks([2]),
                    format!("command {name}: argument 3 must be \"string\""),
                ),
                (
                    CommandInvocation::new(name, ["@typed", "-g", "{ display-message late }"])
                        .with_command_blocks([2]),
                    format!("command {name}: argument 3 must be \"string\""),
                ),
            ] {
                assert_eq!(
                    parse_tmux_command_options(spec, &command)
                        .expect_err("rejected command block")
                        .tmux_message(),
                    expected
                );
            }

            let quoted =
                CommandInvocation::new(name, ["-g", "@quoted", "{ display-message quoted }"]);
            parse_tmux_command_options(spec, &quoted).expect("quoted braces");
        }
    }

    #[test]
    fn bind_key_args_parse_accepts_every_positional_as_command_or_string() {
        let spec = catalog_command_spec("bind-key").expect("bind-key");

        for command in [
            CommandInvocation::new(
                "bind-key",
                ["{ display-message key }", "display-message action"],
            )
            .with_command_blocks([0]),
            CommandInvocation::new("bind-key", ["F1", "{ display-message action }"])
                .with_command_blocks([1]),
            CommandInvocation::new(
                "bind-key",
                ["F1", "display-message", "{ display-message argument }"],
            )
            .with_command_blocks([2]),
            CommandInvocation::new("bind-key", ["--", "F1", "{}"]).with_command_blocks([2]),
            CommandInvocation::new("bind-key", ["F1", "-T", "{}"]).with_command_blocks([2]),
            CommandInvocation::new("bind-key", ["F1", "{ display-message quoted }"]),
        ] {
            parse_tmux_command_options(spec, &command).expect("command-or-string positional");
        }

        for option in ["-T", "-N"] {
            let command = CommandInvocation::new(
                "bind-key",
                [
                    option,
                    "{ display-message option }",
                    "F1",
                    "display-message",
                ],
            )
            .with_command_blocks([1]);
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed option value")
                    .tmux_message(),
                format!("command bind-key: {option} argument must be a string")
            );
        }
    }

    #[test]
    fn confirm_before_args_parse_accepts_only_the_command_as_a_block() {
        let spec = catalog_command_spec("confirm-before").expect("confirm-before");

        for command in [
            CommandInvocation::new("confirm-before", ["{ display-message action }"])
                .with_command_blocks([0]),
            CommandInvocation::new(
                "confirm-before",
                ["-by", "--", "{ display-message action }"],
            )
            .with_command_blocks([2]),
            CommandInvocation::new("confirm-before", ["{ display-message quoted }"]),
        ] {
            parse_tmux_command_options(spec, &command).expect("command-or-string positional");
        }

        for option in ["-c", "-p", "-t"] {
            let command = CommandInvocation::new(
                "confirm-before",
                [
                    option,
                    "{ display-message option }",
                    "display-message action",
                ],
            )
            .with_command_blocks([1]);
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed option value")
                    .tmux_message(),
                format!("command confirm-before: {option} argument must be a string")
            );
        }
    }

    #[test]
    fn command_prompt_args_parse_accepts_only_the_template_as_a_block() {
        let spec = catalog_command_spec("command-prompt").expect("command-prompt");

        for command in [
            CommandInvocation::new("command-prompt", ["{ display-message action }"])
                .with_command_blocks([0]),
            CommandInvocation::new("command-prompt", ["-b", "--", "{ display-message action }"])
                .with_command_blocks([2]),
            CommandInvocation::new("command-prompt", ["{ display-message quoted }"]),
        ] {
            parse_tmux_command_options(spec, &command).expect("command-or-string positional");
        }

        for option in ["-I", "-p", "-t", "-T"] {
            let command = CommandInvocation::new(
                "command-prompt",
                [
                    option,
                    "{ display-message option }",
                    "display-message action",
                ],
            )
            .with_command_blocks([1]);
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed option value")
                    .tmux_message(),
                format!("command command-prompt: {option} argument must be a string")
            );
        }
    }

    #[test]
    fn display_menu_args_parse_tracks_names_keys_actions_and_separators() {
        let spec = catalog_command_spec("display-menu").expect("display-menu");

        for command in [
            CommandInvocation::new("display-menu", ["One", "o", "display-message one"]),
            CommandInvocation::new("display-menu", ["One", "o", "display-message one", "Two"]),
            CommandInvocation::new(
                "display-menu",
                ["One", "o", "display-message one", "Two", "t"],
            ),
            CommandInvocation::new(
                "display-menu",
                [
                    "One",
                    "o",
                    "{ display-message one }",
                    "",
                    "Two",
                    "t",
                    "{ display-message two }",
                ],
            )
            .with_command_blocks([2, 6]),
            CommandInvocation::new(
                "display-menu",
                ["", "", "One", "o", "{ display-message one }"],
            )
            .with_command_blocks([4]),
            CommandInvocation::new("display-menu", ["", "One", "o", "{ display-message one }"])
                .with_command_blocks([3]),
            CommandInvocation::new(
                "display-menu",
                [
                    "One",
                    "o",
                    "display-message one",
                    "Two",
                    "t",
                    "{ display-message two }",
                ],
            )
            .with_command_blocks([5]),
        ] {
            parse_tmux_command_options(spec, &command).expect("display menu items");
        }

        for (command, expected) in [
            (
                CommandInvocation::new(
                    "display-menu",
                    ["{ display-message name }", "n", "display-message action"],
                )
                .with_command_blocks([0]),
                "command display-menu: argument 1 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "display-menu",
                    ["One", "{ display-message key }", "display-message action"],
                )
                .with_command_blocks([1]),
                "command display-menu: argument 2 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "display-menu",
                    [
                        "",
                        "{ display-message name }",
                        "n",
                        "display-message action",
                    ],
                )
                .with_command_blocks([1]),
                "command display-menu: argument 2 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "display-menu",
                    [
                        "One",
                        "o",
                        "display-message one",
                        "{ display-message name }",
                        "t",
                        "display-message two",
                    ],
                )
                .with_command_blocks([3]),
                "command display-menu: argument 4 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "display-menu",
                    [
                        "One",
                        "o",
                        "display-message one",
                        "Two",
                        "{ display-message key }",
                        "display-message two",
                    ],
                )
                .with_command_blocks([4]),
                "command display-menu: argument 5 must be \"string\"",
            ),
        ] {
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed display menu string")
                    .tmux_message(),
                expected
            );
        }

        for option in ["-b", "-c", "-C", "-H", "-s", "-S", "-t", "-T", "-x", "-y"] {
            let command = CommandInvocation::new(
                "display-menu",
                [
                    option,
                    "{ display-message option }",
                    "One",
                    "o",
                    "display-message action",
                ],
            )
            .with_command_blocks([1]);
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed option value")
                    .tmux_message(),
                format!("command display-menu: {option} argument must be a string")
            );
        }
    }

    #[test]
    fn display_panes_args_parse_accepts_a_command_or_string_template() {
        let spec = catalog_command_spec("display-panes").expect("display-panes");

        for command in [
            CommandInvocation::new("display-panes", ["{ display-message action }"])
                .with_command_blocks([0]),
            CommandInvocation::new("display-panes", ["-bN", "{ display-message action }"])
                .with_command_blocks([1]),
            CommandInvocation::new("display-panes", ["--", "{ display-message action }"])
                .with_command_blocks([1]),
            CommandInvocation::new("display-panes", ["{ display-message quoted }"]),
        ] {
            parse_tmux_command_options(spec, &command).expect("command-or-string template");
        }

        for option in ["-d", "-t"] {
            let command =
                CommandInvocation::new("display-panes", [option, "{ display-message option }"])
                    .with_command_blocks([1]);
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed option value")
                    .tmux_message(),
                format!("command display-panes: {option} argument must be a string")
            );
        }
    }

    #[test]
    fn chooser_args_parse_accepts_a_command_or_string_template() {
        for (name, flags) in [("choose-buffer", "-kNrZ"), ("choose-tree", "-GhkNrswZ")] {
            let spec = catalog_command_spec(name).expect("chooser");
            for command in [
                CommandInvocation::new(name, ["{ display-message action }"])
                    .with_command_blocks([0]),
                CommandInvocation::new(name, [flags, "{ display-message action }"])
                    .with_command_blocks([1]),
                CommandInvocation::new(name, ["--", "{ display-message action }"])
                    .with_command_blocks([1]),
                CommandInvocation::new(name, ["{ display-message quoted }"]),
            ] {
                parse_tmux_command_options(spec, &command).expect("command-or-string template");
            }

            for option in ["-F", "-f", "-K", "-O", "-t"] {
                let command = CommandInvocation::new(name, [option, "{ display-message option }"])
                    .with_command_blocks([1]);
                assert_eq!(
                    parse_tmux_command_options(spec, &command)
                        .expect_err("typed option value")
                        .tmux_message(),
                    format!("command {name}: {option} argument must be a string")
                );
            }
        }
    }

    #[test]
    fn set_hook_args_parse_switches_positionals_for_format_monitors() {
        let spec = catalog_command_spec("set-hook").expect("set-hook");

        for command in [
            CommandInvocation::new(
                "set-hook",
                ["-g", "after-new-window", "{ display-message action }"],
            )
            .with_command_blocks([2]),
            CommandInvocation::new(
                "set-hook",
                [
                    "-B",
                    "@monitor:window:#{window_name}",
                    "{ display-message first }",
                    "{ display-message second }",
                    "{ display-message third }",
                ],
            )
            .with_command_blocks([2, 3, 4]),
            CommandInvocation::new(
                "set-hook",
                ["-g", "after-new-window", "{ display-message quoted }"],
            ),
        ] {
            parse_tmux_command_options(spec, &command).expect("command-or-string positional");
        }

        for (command, expected) in [
            (
                CommandInvocation::new(
                    "set-hook",
                    ["-g", "{ display-message name }", "display-message action"],
                )
                .with_command_blocks([1]),
                "command set-hook: argument 1 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "set-hook",
                    [
                        "after-new-window",
                        "display-message action",
                        "{ display-message extra }",
                    ],
                )
                .with_command_blocks([2]),
                "command set-hook: argument 3 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "set-hook",
                    ["after-new-window", "-B", "{ display-message late }"],
                )
                .with_command_blocks([2]),
                "command set-hook: argument 3 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "set-hook",
                    [
                        "-t",
                        "{ display-message target }",
                        "after-new-window",
                        "display-message action",
                    ],
                )
                .with_command_blocks([1]),
                "command set-hook: -t argument must be a string",
            ),
            (
                CommandInvocation::new(
                    "set-hook",
                    ["-B", "{ display-message monitor }", "after-new-window"],
                )
                .with_command_blocks([1]),
                "command set-hook: -B argument must be a string",
            ),
        ] {
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("rejected command block")
                    .tmux_message(),
                expected
            );
        }
    }

    #[test]
    fn default_args_parse_requires_string_positionals_and_option_values() {
        let spec = catalog_command_spec("display-message").expect("display-message");
        for (command, expected) in [
            (
                CommandInvocation::new("display-message", ["{ display-message value }"])
                    .with_command_blocks([0]),
                "command display-message: argument 1 must be \"string\"",
            ),
            (
                CommandInvocation::new(
                    "display-message",
                    ["-t", "{ display-message target }", "value"],
                )
                .with_command_blocks([1]),
                "command display-message: -t argument must be a string",
            ),
        ] {
            assert_eq!(
                parse_tmux_command_options(spec, &command)
                    .expect_err("typed string position")
                    .tmux_message(),
                expected
            );
        }

        parse_tmux_command_options(
            spec,
            &CommandInvocation::new("display-message", ["{ display-message quoted }"]),
        )
        .expect("quoted braces");
    }

    #[test]
    fn tmux_option_scanning_stops_at_the_first_positional_or_boundary() {
        let spec = catalog_command_spec("run-shell").expect("run-shell");
        let arguments = owned(&["echo", "-?"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments).expect("positional boundary"),
            TmuxOptionParse {
                options: Vec::new(),
                positionals: &arguments,
            }
        );

        let spec = command_spec("kill-server").expect("kill-server");
        let arguments = owned(&["--", "-?"]);
        assert_eq!(
            parse_tmux_options(spec, &arguments).expect("explicit boundary"),
            TmuxOptionParse {
                options: Vec::new(),
                positionals: &arguments[1..],
            }
        );
        assert!(
            !catalog_command_spec("agent-send")
                .expect("agent-send")
                .uses_tmux_option_grammar()
        );
    }

    #[test]
    fn catalog_names_aliases_and_options_are_unique() {
        let mut names = BTreeSet::new();
        for spec in command_specs() {
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

        for spec in command_specs() {
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
            let expected = if spec.name == "new-session" {
                spec.options
                    .iter()
                    .map(|option| {
                        (
                            option.name.to_owned(),
                            option.value.is_some() || option.attached_value,
                        )
                    })
                    .collect()
            } else {
                accepted_options(spec)
            };
            assert_eq!(
                usage_options(spec.usage),
                expected,
                "usage drift for {}",
                spec.name
            );
        }

        for spec in DAEMON_COMMAND_SPECS {
            let advertised = spec
                .options
                .iter()
                .filter(|option| option.completable)
                .map(|option| (option.name.to_owned(), option.value.is_some()))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(
                usage_options(spec.usage),
                advertised,
                "usage drift for {}",
                spec.name
            );
        }
    }

    #[test]
    fn split_window_spawn_style_and_title_options_are_supported_values() {
        let spec = command_spec("split-window").expect("split-window catalog entry");
        for name in ["-R", "-s", "-S", "-T"] {
            let option = spec.option(name).expect("split-window option");
            assert_eq!(
                (
                    option.value,
                    option.completable,
                    option.attached_value,
                    option.optional_value,
                    option.unsupported,
                ),
                (Some(CommandValueKind::FreeForm), true, false, false, false),
                "{name}"
            );
        }
    }

    #[test]
    fn unknown_commands_remain_available_for_structured_errors() {
        assert_eq!(canonical_command("future-command"), "future-command");
        assert!(command_spec("future-command").is_none());
    }

    #[test]
    fn send_keys_outer_options_match_the_tmux_pin() {
        let spec = command_spec("send-keys").expect("send-keys catalog entry");
        let options = spec
            .options
            .iter()
            .map(|option| (option.name, option.value.is_some()))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            options,
            BTreeSet::from([
                ("-F", false),
                ("-H", false),
                ("-K", false),
                ("-M", false),
                ("-N", true),
                ("-R", false),
                ("-X", false),
                ("-c", true),
                ("-l", false),
                ("-t", true),
            ])
        );
    }

    #[test]
    fn attach_session_working_directory_is_supported_and_advertised() {
        let spec = command_spec("attach-session").unwrap();
        let working_directory = spec
            .options
            .iter()
            .find(|option| option.name == "-c")
            .unwrap();

        assert_eq!(working_directory.value, Some(CommandValueKind::FreeForm));
        assert!(!working_directory.unsupported);
        assert!(working_directory.completable);
        assert_eq!(
            spec.options
                .iter()
                .find(|option| option.name == "-t")
                .unwrap()
                .value,
            Some(CommandValueKind::Pane)
        );
        assert_eq!(
            spec.usage,
            "[-dErx] [-c working-directory] [-f flags] [-t target-session]"
        );
        assert!(
            spec.options
                .iter()
                .any(|option| option.name == "-E" && !option.unsupported)
        );
    }

    #[test]
    fn attaching_commands_accept_client_flags() {
        for name in ["attach-session", "new-session"] {
            let flags = command_spec(name)
                .unwrap()
                .options
                .iter()
                .find(|option| option.name == "-f")
                .unwrap();
            assert_eq!(flags.value, Some(CommandValueKind::FreeForm));
            assert!(!flags.unsupported);
            assert!(flags.completable);
        }
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
        for name in DAEMON_COMMAND_NAMES {
            let spec = catalog_command_spec(name)
                .unwrap_or_else(|| panic!("daemon command {name} has no shared spec"));
            assert_eq!(canonical_command(name), spec.name);
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
            assert!(!UNIMPLEMENTED_TMUX_COMMANDS.contains(&name));
            assert!(command_spec(name).is_some());
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
            "resize-window",
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
            "set-hook",
            "show-hooks",
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
