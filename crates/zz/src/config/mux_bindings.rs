use zz_protocol::{
    CommandInvocation, KeyBindingSnapshot, TmuxOption, canonical_command, command_spec,
    parse_tmux_command_options,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SplitPaneKind {
    Picker,
    Terminal,
    Browser,
}

impl SplitPaneKind {
    fn command(self) -> &'static str {
        match self {
            Self::Picker => "split-picker",
            Self::Terminal => "split-window",
            Self::Browser => "split-browser",
        }
    }
}

fn command_kind(command: &CommandInvocation) -> Option<SplitPaneKind> {
    match canonical_command(&command.name) {
        "split-picker" => Some(SplitPaneKind::Picker),
        "split-window" => Some(SplitPaneKind::Terminal),
        "split-browser" => Some(SplitPaneKind::Browser),
        _ => None,
    }
}

fn command_direction(command: &CommandInvocation) -> Option<SplitDirection> {
    command_kind(command)?;
    let parsed = parse_tmux_command_options(command_spec(&command.name)?, command).ok()?;
    Some(if parsed.options.contains(&TmuxOption::Flag("-h")) {
        SplitDirection::Horizontal
    } else {
        SplitDirection::Vertical
    })
}

pub(crate) fn preferred_split_binding(
    bindings: &[KeyBindingSnapshot],
    direction: SplitDirection,
) -> Option<&KeyBindingSnapshot> {
    let preferred = match direction {
        SplitDirection::Horizontal => ["|", "%"],
        SplitDirection::Vertical => ["-", "\""],
    };
    bindings
        .iter()
        .filter(|binding| {
            binding
                .commands
                .iter()
                .any(|command| command_direction(command) == Some(direction))
        })
        .min_by_key(|binding| {
            preferred
                .iter()
                .position(|key| *key == binding.key)
                .unwrap_or(preferred.len())
        })
}

pub(crate) fn split_binding_kind(binding: &KeyBindingSnapshot) -> Option<SplitPaneKind> {
    let [command] = binding.commands.as_slice() else {
        return None;
    };
    let kind = command_kind(command)?;
    let parsed = parse_tmux_command_options(command_spec(&command.name)?, command).ok()?;
    if (parsed.options.contains(&TmuxOption::Flag("-h"))
        && parsed.options.contains(&TmuxOption::Flag("-v")))
        || !parsed.positionals.is_empty()
        || parsed.options.iter().any(|option| {
            !matches!(option, TmuxOption::Flag("-h" | "-v"))
                && !matches!(
                    option,
                    TmuxOption::Value("-c", "#{pane_current_path}")
                        if kind != SplitPaneKind::Browser
                )
        })
    {
        return None;
    }
    Some(kind)
}

fn binding_line(binding: &KeyBindingSnapshot) -> String {
    let mut args = vec!["-T".to_owned(), "prefix".to_owned()];
    if binding.repeat {
        args.push("-r".to_owned());
    }
    if let Some(note) = &binding.note {
        args.extend(["-N".to_owned(), note.clone()]);
    }
    args.push(binding.key.clone());
    let body = binding
        .commands
        .iter()
        .map(zz_mux::format_command)
        .collect::<Vec<_>>()
        .join(" ; ");
    args.push(format!("{{ {body} }}"));
    let block = args.len() - 1;
    zz_mux::format_command(&CommandInvocation::new("bind-key", args).with_command_blocks([block]))
}

fn generated_split_line(line: &str) -> bool {
    let parsed = zz_mux::MuxEngine::parse_config_without_variable_expansion("settings", line);
    let [command] = parsed.commands.as_slice() else {
        return false;
    };
    if !parsed.diagnostics.is_empty()
        || command.name != "bind-key"
        || zz_mux::format_command(command) != line
    {
        return false;
    }
    let Ok(options) = parse_tmux_command_options(
        command_spec("bind-key").expect("bind-key is catalogued"),
        command,
    ) else {
        return false;
    };
    if !options.options.contains(&TmuxOption::Value("-T", "prefix"))
        || options.positionals.len() != 2
        || !command.argument_is_command_block(command.args.len() - 1)
    {
        return false;
    }
    let Some(body) = zz_mux::command_block_body(&options.positionals[1]) else {
        return false;
    };
    let body = zz_mux::MuxEngine::parse_config_without_variable_expansion("settings", body);
    let [split] = body.commands.as_slice() else {
        return false;
    };
    body.diagnostics.is_empty() && command_kind(split).is_some()
}

pub(crate) fn update_split_binding(
    source: &str,
    bindings: &[KeyBindingSnapshot],
    original: Option<&KeyBindingSnapshot>,
    direction: SplitDirection,
    key: &str,
    kind: SplitPaneKind,
) -> Result<String, String> {
    if key.chars().any(char::is_control) {
        return Err("Enter one tmux key, such as -, |, or C-s.".to_owned());
    }
    let key = zz_mux::parse_tmux_key(key.trim())
        .filter(|key| key != "None" && key != "Any")
        .ok_or_else(|| "Enter one tmux key, such as -, |, or C-s.".to_owned())?;
    if bindings.iter().any(|binding| {
        zz_mux::parse_tmux_key(&binding.key).as_ref() == Some(&key)
            && original.is_none_or(|original| binding.key != original.key)
    }) {
        return Err(format!("Prefix + {key} already has a binding."));
    }
    let old_kind = original
        .map(|binding| {
            split_binding_kind(binding)
                .ok_or_else(|| "Edit this custom binding in the configuration editor.".to_owned())
        })
        .transpose()?;
    let command = if old_kind == Some(kind) {
        original.expect("an existing kind has a binding").commands[0].clone()
    } else {
        let flag = match direction {
            SplitDirection::Horizontal => "-h",
            SplitDirection::Vertical => "-v",
        };
        CommandInvocation::new(kind.command(), [flag])
    };
    let updated = KeyBindingSnapshot {
        key,
        commands: vec![command],
        repeat: original.is_some_and(|binding| binding.repeat),
        note: original.and_then(|binding| binding.note.clone()),
    };
    let new_line = binding_line(&updated);
    let mut result = source.to_owned();
    if let Some(original) = original {
        let old_line = binding_line(original);
        let mut offset = source.len();
        for raw_line in source.split_inclusive('\n').rev() {
            offset -= raw_line.len();
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            if !generated_split_line(line) {
                break;
            }
            if line == old_line {
                result.replace_range(offset..offset + raw_line.len(), "");
                break;
            }
        }
        if original.key != updated.key {
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(&zz_mux::format_command(&CommandInvocation::new(
                "unbind-key",
                ["-T", "prefix", original.key.as_str()],
            )));
            result.push('\n');
        }
    }
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&new_line);
    result.push('\n');
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(key: &str, name: &str, args: &[&str]) -> KeyBindingSnapshot {
        KeyBindingSnapshot {
            key: key.to_owned(),
            commands: vec![CommandInvocation::new(name, args.iter().copied())],
            repeat: false,
            note: None,
        }
    }

    fn apply(source: &str, binding: &KeyBindingSnapshot, key: &str, kind: SplitPaneKind) -> String {
        update_split_binding(
            source,
            std::slice::from_ref(binding),
            Some(binding),
            SplitDirection::Vertical,
            key,
            kind,
        )
        .unwrap()
    }

    #[test]
    fn recognizes_inherited_directory_and_rejects_custom_commands() {
        let mut current = binding("-", "split-window", &["-v", "-c", "#{pane_current_path}"]);
        assert_eq!(split_binding_kind(&current), Some(SplitPaneKind::Terminal));
        current.commands[0].args.push("htop".into());
        assert_eq!(split_binding_kind(&current), None);
        current = binding("-", "split-window", &["-v", "-c", "/tmp"]);
        assert_eq!(split_binding_kind(&current), None);
        current = binding("-", "split-window", &["-dv"]);
        assert_eq!(split_binding_kind(&current), None);
        for name in ["split-window", "split-picker", "split-browser"] {
            current = binding("-", name, &["-hv"]);
            assert_eq!(split_binding_kind(&current), None);
            current = binding("-", name, &["-h", "-v"]);
            assert_eq!(split_binding_kind(&current), None);
        }
        current
            .commands
            .push(CommandInvocation::new("select-pane", ["-L"]));
        assert_eq!(split_binding_kind(&current), None);
    }

    #[test]
    fn prefers_user_split_keys_and_reads_clustered_direction() {
        let bindings = [
            binding("%", "split-picker", &["-h"]),
            binding("|", "split-window", &["-dh"]),
        ];
        assert_eq!(
            preferred_split_binding(&bindings, SplitDirection::Horizontal)
                .unwrap()
                .key,
            "|"
        );
    }

    #[test]
    fn preserves_source_arguments_and_metadata_on_key_change() {
        let mut current = binding("-", "split-window", &["-v", "-c", "#{pane_current_path}"]);
        current.repeat = true;
        current.note = Some("split 'here'; $HOME".to_owned());
        let source = "# keep this\nset -g prefix C-a\n";
        let updated = apply(source, &current, "'", SplitPaneKind::Terminal);
        assert!(updated.starts_with(source));
        let parsed = zz_mux::MuxEngine::parse_config_without_variable_expansion("test", &updated);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert_eq!(parsed.commands.len(), 3);
        assert_eq!(parsed.commands[1].name, "unbind-key");
        let mut engine = zz_mux::MuxEngine::default();
        let mut context = zz_mux::ExecutionContext::default();
        for command in &parsed.commands {
            engine.execute(&mut context, command).unwrap();
        }
        let saved = engine.keys.get("prefix", "'").unwrap();
        assert!(saved.repeat);
        assert_eq!(saved.note, current.note);
        assert_eq!(saved.commands[0].args, current.commands[0].args);
        assert!(engine.keys.get("prefix", "-").is_none());
    }

    #[test]
    fn replaces_its_final_override_on_repeated_changes() {
        let current = binding("-", "split-window", &["-v"]);
        let source = "# original\nbind - split-window -v\n";
        let first = apply(source, &current, "-", SplitPaneKind::Picker);
        let picker = binding("-", "split-picker", &["-v"]);
        let second = apply(&first, &picker, "-", SplitPaneKind::Browser);
        assert!(second.starts_with(source));
        assert_eq!(first.lines().count(), second.lines().count());
        assert!(!second.contains("split-picker"));
    }

    #[test]
    fn alternating_directions_replace_only_trailing_generated_overrides() {
        let vertical = binding("-", "split-window", &["-v"]);
        let horizontal = binding("|", "split-window", &["-h"]);
        let bindings = [vertical.clone(), horizontal.clone()];
        let source = "# original\nbind - split-window -v\n";
        let first = apply(source, &vertical, "-", SplitPaneKind::Picker);
        let second = update_split_binding(
            &first,
            &bindings,
            Some(&horizontal),
            SplitDirection::Horizontal,
            "|",
            SplitPaneKind::Picker,
        )
        .unwrap();
        let picker = binding("-", "split-picker", &["-v"]);
        let third = apply(&second, &picker, "-", SplitPaneKind::Browser);
        assert!(third.starts_with(source));
        assert_eq!(second.lines().count(), third.lines().count());
        let commented = format!("{second}# user note\n");
        let fourth = apply(&commented, &picker, "-", SplitPaneKind::Browser);
        assert!(fourth.starts_with(&commented));
    }

    #[test]
    fn rejects_collisions_and_invalid_keys() {
        let current = binding("-", "split-window", &["-v"]);
        let bindings = [current.clone(), binding("s", "choose-tree", &[])];
        for key in ["s", "bad-key-name", "x\nbind q kill-server", "Any", "None"] {
            assert!(
                update_split_binding(
                    "",
                    &bindings,
                    Some(&current),
                    SplitDirection::Vertical,
                    key,
                    SplitPaneKind::Picker,
                )
                .is_err()
            );
        }
    }
}
