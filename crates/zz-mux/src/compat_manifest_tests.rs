use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use zz_protocol::{
    COMMAND_ARGS_PARSE_BEHAVES, COMMAND_ARGS_PARSE_SPECS, CommandArgsParseRule, CommandResolution,
    DAEMON_COMMAND_SPECS, DAEMON_INVALID_FLAG_BEHAVES, KeyTables, NATIVE_COMMAND_NAMES,
    POSITIONAL_MAX_BEHAVES, canonical_command, canonical_key, resolve_command,
};

use crate::{
    BEHAVES, COMMAND_SPECS,
    command::{
        COMMAND_ITEM_CONTEXT_FORMATS, LIST_COMMAND_CONTEXT_FORMATS, LIST_KEY_CONTEXT_FORMATS,
        format_key_command,
    },
    formats::{constant_format_variable_names, format_variable_names},
};

#[derive(Deserialize)]
struct Manifest {
    gaps: Vec<Gap>,
}

#[derive(Deserialize)]
struct Gap {
    id: String,
    items: Vec<String>,
}

#[derive(Deserialize)]
struct Oracle {
    commands: Vec<OracleCommand>,
    args_parse: BTreeMap<String, String>,
    options: Vec<String>,
    formats: Vec<String>,
    format_contexts: BTreeMap<String, Vec<String>>,
    hooks: Vec<String>,
    key_bindings: Vec<OracleKey>,
}

#[derive(Deserialize)]
struct OracleCommand {
    name: String,
    aliases: Vec<String>,
    flags: BTreeMap<String, String>,
    min_args: usize,
    max_args: Option<usize>,
}

#[derive(Deserialize)]
struct OracleKey {
    table: String,
    key: String,
    repeat: bool,
    command: String,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    serde_json::from_str(
        &fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display())),
    )
    .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()))
}

fn inventory() -> (Oracle, BTreeMap<String, String>) {
    let root = root();
    let oracle = read_json(&root.join("compat/tmux-oracle.json"));
    let manifest: Manifest = read_json(&root.join("compat/tmux-gaps.json"));
    let mut items = BTreeMap::new();
    for gap in manifest.gaps {
        for item in gap.items {
            assert!(
                items.insert(item.clone(), gap.id.clone()).is_none(),
                "manifest item appears more than once: {item}"
            );
        }
    }
    (oracle, items)
}

fn specs() -> BTreeMap<&'static str, &'static zz_protocol::CommandSpec> {
    let mut specs = BTreeMap::new();
    for spec in COMMAND_SPECS.iter().chain(DAEMON_COMMAND_SPECS) {
        assert!(
            specs.insert(spec.name, spec).is_none(),
            "duplicate command spec: {}",
            spec.name
        );
    }
    specs
}

fn item_key(value: &str) -> String {
    let key = canonical_key(value);
    key.strip_suffix(' ')
        .map_or_else(|| key.clone(), |prefix| format!("{prefix}Space"))
}

fn args_parse_rule_name(rule: CommandArgsParseRule) -> &'static str {
    match rule {
        CommandArgsParseRule::CommandsOrString => "commands-or-string",
        CommandArgsParseRule::DisplayMenuItems => "display-menu-items",
        CommandArgsParseRule::IfShellBranches => "if-shell-branches",
        CommandArgsParseRule::RunShellCommandFlag => "run-shell-command-flag",
        CommandArgsParseRule::SetHookMonitorOrValue => "set-hook-monitor-or-value",
        CommandArgsParseRule::SetOptionValue => "set-option-value",
    }
}

#[test]
fn command_and_flag_gaps_match_the_pinned_oracle() {
    let (oracle, items) = inventory();
    let specs = specs();
    let mut upstream = BTreeMap::new();
    let mut upstream_spellings = BTreeMap::new();
    for command in &oracle.commands {
        assert!(
            upstream.insert(command.name.as_str(), command).is_none(),
            "duplicate oracle command: {}",
            command.name
        );
        for spelling in std::iter::once(&command.name).chain(&command.aliases) {
            assert!(
                upstream_spellings
                    .insert(spelling.as_str(), command.name.as_str())
                    .is_none(),
                "duplicate oracle command spelling: {spelling}"
            );
        }
    }
    let mut unclassified_extensions = Vec::new();

    for command in &oracle.commands {
        if let Some(spec) = specs.get(command.name.as_str()) {
            for alias in &command.aliases {
                assert!(
                    spec.aliases.contains(&alias.as_str()),
                    "{} is missing upstream alias {alias}",
                    command.name
                );
            }
            for flag in command.flags.keys() {
                let implemented = spec.option(flag).is_some_and(|option| !option.unsupported);
                assert!(
                    implemented || items.contains_key(&format!("flag:{}:{flag}", command.name)),
                    "upstream flag {} {flag} is neither implemented nor tracked",
                    command.name
                );
            }
        } else {
            let item = format!("command:{}", command.name);
            assert!(
                items.contains_key(&item),
                "upstream command {} is neither implemented nor tracked",
                command.name
            );
            for spelling in std::iter::once(&command.name).chain(&command.aliases) {
                match resolve_command(spelling) {
                    CommandResolution::Unimplemented(name) => assert_eq!(name, command.name),
                    resolution => panic!(
                        "tracked command spelling {spelling} must remain recognized, got {resolution:?}"
                    ),
                }
            }
        }
    }

    for (name, spec) in &specs {
        let Some(command) = upstream.get(name) else {
            assert!(
                !upstream_spellings.contains_key(name),
                "native command name collides with an upstream spelling: {name}"
            );
            if !items.contains_key(&format!("native-command:{name}")) {
                unclassified_extensions.push(format!("native-command:{name}"));
            }
            continue;
        };
        let aliases = spec.aliases.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            aliases.len(),
            spec.aliases.len(),
            "duplicate alias on {name}"
        );
        for alias in spec.aliases {
            if command.aliases.iter().any(|candidate| candidate == alias) {
                continue;
            }
            assert!(
                !upstream_spellings.contains_key(alias),
                "zz alias {alias} for {name} collides with an upstream spelling"
            );
            let item = format!("native-alias:{name}:{alias}");
            if !items.contains_key(&item) {
                unclassified_extensions.push(item);
            }
        }
        for option in spec.options {
            if !command.flags.contains_key(option.name) {
                let item = format!("extension-flag:{name}:{}", option.name);
                if !items.contains_key(&item) {
                    unclassified_extensions.push(item);
                }
            }
        }
        for option in spec.options.iter().filter(|option| option.unsupported) {
            if command.flags.contains_key(option.name) {
                let item = format!("flag:{name}:{}", option.name);
                assert!(
                    items.contains_key(&item),
                    "catalogued unsupported upstream flag {name} {} is not tracked",
                    option.name
                );
            }
        }
    }
    assert!(
        unclassified_extensions.is_empty(),
        "unclassified zz catalog extensions: {unclassified_extensions:?}"
    );

    for item in items.keys().filter(|item| item.starts_with("command:")) {
        let name = item.strip_prefix("command:").unwrap();
        assert!(upstream.contains_key(name), "stale command item: {item}");
        assert!(
            !specs.contains_key(name),
            "implemented command has a stale item: {item}"
        );
    }
    for item in items.keys().filter(|item| item.starts_with("flag:")) {
        let mut fields = item.split(':');
        let _ = fields.next();
        let name = fields.next().unwrap();
        let flag = fields.next().unwrap();
        let command = upstream
            .get(name)
            .unwrap_or_else(|| panic!("stale flag item names a non-upstream command: {item}"));
        let spec = specs
            .get(name)
            .unwrap_or_else(|| panic!("flag item duplicates an unimplemented command gap: {item}"));
        assert!(
            command.flags.contains_key(flag),
            "stale flag item is absent from the oracle: {item}"
        );
        assert!(
            spec.option(flag).is_none_or(|option| option.unsupported),
            "implemented flag has a stale item: {item}"
        );
    }
    for item in items
        .keys()
        .filter(|item| item.starts_with("native-command:"))
    {
        let name = item.strip_prefix("native-command:").unwrap();
        assert!(
            specs.contains_key(name),
            "stale native command item: {item}"
        );
        assert!(
            !upstream.contains_key(name),
            "upstream command has a stale native item: {item}"
        );
    }
    for item in items
        .keys()
        .filter(|item| item.starts_with("native-alias:"))
    {
        let mut fields = item.split(':');
        let _ = fields.next();
        let name = fields.next().unwrap();
        let alias = fields.next().unwrap();
        let spec = specs
            .get(name)
            .unwrap_or_else(|| panic!("stale native alias names an unknown command: {item}"));
        assert!(
            spec.aliases.contains(&alias),
            "stale native alias item: {item}"
        );
        let command = upstream
            .get(name)
            .unwrap_or_else(|| panic!("native command aliases do not need separate items: {item}"));
        assert!(
            !command.aliases.iter().any(|candidate| candidate == alias),
            "upstream alias has a stale native item: {item}"
        );
    }
    for item in items
        .keys()
        .filter(|item| item.starts_with("extension-flag:"))
    {
        let mut fields = item.split(':');
        let _ = fields.next();
        let name = fields.next().unwrap();
        let flag = fields.next().unwrap();
        let spec = specs
            .get(name)
            .unwrap_or_else(|| panic!("stale extension flag names an unknown command: {item}"));
        let command = upstream
            .get(name)
            .unwrap_or_else(|| panic!("extension flag belongs to a native command: {item}"));
        assert!(
            spec.option(flag).is_some(),
            "stale extension flag item: {item}"
        );
        assert!(
            !command.flags.contains_key(flag),
            "upstream flag has a stale extension item: {item}"
        );
    }

    let mut flag_arity_mismatches = BTreeSet::new();
    let mut positional_min_mismatches = BTreeSet::new();
    let mut positional_max_mismatches = BTreeSet::new();
    for command in &oracle.commands {
        let Some(spec) = specs.get(command.name.as_str()) else {
            continue;
        };
        for (flag, expected) in &command.flags {
            let Some(option) = spec.option(flag) else {
                continue;
            };
            if option.unsupported {
                continue;
            }
            let actual = if option.optional_value {
                "optional"
            } else if option.attached_value {
                "attached-only"
            } else if option.value.is_some() {
                "required"
            } else {
                "none"
            };
            if actual != expected {
                flag_arity_mismatches.insert(format!("flag-arity:{}:{flag}", command.name));
            }
        }
        if command.min_args != 0 {
            positional_min_mismatches.insert(format!("positional-min:{}", command.name));
        }
        let maximum = spec.positional_maximum();
        if maximum != command.max_args {
            positional_max_mismatches.insert(format!("positional-max:{}", command.name));
        }
    }
    let tracked_flag_arity = items
        .keys()
        .filter(|item| item.starts_with("flag-arity:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let tracked_positional_min = items
        .keys()
        .filter(|item| item.starts_with("positional-min:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let tracked_positional_max = items
        .keys()
        .filter(|item| item.starts_with("positional-max:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tracked_flag_arity, flag_arity_mismatches,
        "implemented flag arity mismatches and tracked items differ"
    );
    assert_eq!(
        tracked_positional_min, positional_min_mismatches,
        "required positional bounds and tracked minimum items differ"
    );
    assert_eq!(
        tracked_positional_max, positional_max_mismatches,
        "positional maximum metadata and tracked items differ"
    );

    let upstream_names = upstream.keys().copied().collect::<BTreeSet<_>>();
    let native_names = specs
        .keys()
        .copied()
        .filter(|name| !upstream.contains_key(name))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        NATIVE_COMMAND_NAMES
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        native_names,
        "native command roster differs from catalog minus the pinned oracle"
    );
    assert_eq!(
        NATIVE_COMMAND_NAMES.len(),
        native_names.len(),
        "native command roster contains duplicates"
    );
    assert!(
        items
            .keys()
            .all(|item| !item.starts_with("prefix-collision:")),
        "resolved native-prefix collisions must not remain tracked"
    );
    let assert_resolves_to = |spelling: &str, expected: &str| {
        let resolution = resolve_command(spelling);
        if specs.contains_key(expected) {
            assert!(
                matches!(resolution, CommandResolution::Canonical(actual) if actual == expected),
                "{spelling} should resolve to implemented {expected}, got {resolution:?}"
            );
        } else {
            assert!(
                matches!(resolution, CommandResolution::Unimplemented(actual) if actual == expected),
                "{spelling} should resolve to unimplemented {expected}, got {resolution:?}"
            );
        }
    };
    for upstream_name in &upstream_names {
        for end in 1..upstream_name.len() {
            let prefix = &upstream_name[..end];
            if let Some(expected) = upstream_spellings.get(prefix) {
                assert_resolves_to(prefix, expected);
                continue;
            }
            let matches = upstream_names
                .iter()
                .copied()
                .filter(|name| name.starts_with(prefix))
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [name] => assert_resolves_to(prefix, name),
                _ => assert_eq!(
                    resolve_command(prefix),
                    CommandResolution::Ambiguous(format!(
                        "ambiguous command: {prefix}, could be: {}",
                        matches.join(", ")
                    )),
                    "upstream prefix {prefix}"
                ),
            }
            if matches.len() == 1 {
                assert_eq!(matches[0], *upstream_name);
            }
        }
    }

    let supported_contexts = BTreeMap::from([
        (
            "command-item",
            COMMAND_ITEM_CONTEXT_FORMATS.iter().copied().collect(),
        ),
        (
            "list-commands",
            LIST_COMMAND_CONTEXT_FORMATS.iter().copied().collect(),
        ),
        (
            "list-keys",
            LIST_KEY_CONTEXT_FORMATS.iter().copied().collect(),
        ),
    ]);
    let mut missing_contexts = BTreeSet::new();
    let mut native_contexts = BTreeSet::new();
    for (scope, upstream_names) in &oracle.format_contexts {
        let upstream_names = upstream_names
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let supported_names = supported_contexts
            .get(scope.as_str())
            .unwrap_or_else(|| panic!("unrecognized oracle format context: {scope}"));
        missing_contexts.extend(
            upstream_names
                .difference(supported_names)
                .map(|name| format!("context-format:{scope}:{name}")),
        );
        native_contexts.extend(
            supported_names
                .difference(&upstream_names)
                .map(|name| format!("native-context-format:{scope}:{name}")),
        );
    }
    assert_eq!(
        supported_contexts.keys().copied().collect::<BTreeSet<_>>(),
        oracle.format_contexts.keys().map(String::as_str).collect(),
        "zz and oracle selected format context scopes differ"
    );
    let tracked_missing_contexts = items
        .keys()
        .filter(|item| item.starts_with("context-format:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let tracked_native_contexts = items
        .keys()
        .filter(|item| item.starts_with("native-context-format:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tracked_missing_contexts, missing_contexts,
        "missing selected context formats and tracked items differ"
    );
    assert_eq!(
        tracked_native_contexts, native_contexts,
        "native selected context formats and tracked items differ"
    );
}

#[test]
fn args_parse_gaps_match_the_pinned_oracle() {
    let (oracle, items) = inventory();
    let specs = specs();
    let mut sidecar = BTreeMap::new();
    let mut sidecar_order = Vec::new();
    for spec in COMMAND_ARGS_PARSE_SPECS {
        assert!(
            sidecar
                .insert(spec.name, args_parse_rule_name(spec.rule))
                .is_none(),
            "duplicate args_parse sidecar command: {}",
            spec.name
        );
        sidecar_order.push(spec.name);
    }
    assert_eq!(
        sidecar_order,
        sidecar_order
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>(),
        "args_parse sidecar must be sorted and unique"
    );

    let implemented = oracle
        .args_parse
        .keys()
        .filter(|name| specs.contains_key(name.as_str()))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        sidecar.keys().copied().collect::<BTreeSet<_>>(),
        implemented,
        "args_parse sidecar commands differ from the implemented pinned callback inventory"
    );
    for (name, rule) in &oracle.args_parse {
        if let Some(sidecar_rule) = sidecar.get(name.as_str()) {
            assert_eq!(
                *sidecar_rule, rule,
                "args_parse rule differs for implemented command {name}"
            );
            assert!(
                !items.contains_key(&format!("command:{name}")),
                "implemented callback command has a command item: {name}"
            );
        } else {
            assert!(
                items.contains_key(&format!("command:{name}")),
                "unimplemented callback command lacks its command item: {name}"
            );
            assert!(
                !items.contains_key(&format!("args-parse:{name}")),
                "unimplemented callback command has a separate args-parse item: {name}"
            );
        }
    }

    let behaves = COMMAND_ARGS_PARSE_BEHAVES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        behaves.len(),
        COMMAND_ARGS_PARSE_BEHAVES.len(),
        "args_parse BEHAVES contains duplicates"
    );
    assert!(
        behaves.is_subset(&implemented),
        "args_parse BEHAVES contains a command outside the sidecar"
    );
    let expected_items = implemented
        .difference(&behaves)
        .map(|name| format!("args-parse:{name}"))
        .collect::<BTreeSet<_>>();
    let tracked_items = items
        .keys()
        .filter(|item| item.starts_with("args-parse:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tracked_items, expected_items,
        "unverified args_parse rules and tracked items differ"
    );
}

#[test]
fn daemon_invalid_flag_runtime_inventory_matches_the_pin() {
    let (oracle, items) = inventory();
    let upstream = oracle
        .commands
        .iter()
        .map(|command| (command.name.as_str(), command))
        .collect::<BTreeMap<_, _>>();
    let expected = zz_protocol::CommandSpec::DAEMON_COMMAND_NAMES
        .iter()
        .map(|name| canonical_command(name))
        .filter(|name| upstream.contains_key(name))
        .collect::<BTreeSet<_>>();
    let behaves = DAEMON_INVALID_FLAG_BEHAVES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    assert_eq!(
        behaves.len(),
        DAEMON_INVALID_FLAG_BEHAVES.len(),
        "daemon invalid-flag behavior inventory contains duplicates"
    );
    assert!(
        behaves.is_subset(&expected),
        "daemon invalid-flag behavior inventory contains a native or non-daemon command"
    );
    for name in &behaves {
        assert!(
            !upstream[name].flags.contains_key("-G"),
            "daemon invalid-flag fixture flag is now valid for {name}"
        );
    }

    let tracked = items.contains_key("semantic:tracker-daemon-invalid-flag-runtime");
    assert_eq!(
        tracked,
        behaves != expected,
        "daemon invalid-flag runtime inventory and tracker item disagree"
    );
}

#[test]
fn positional_maximum_runtime_inventory_matches_the_pin() {
    let (oracle, items) = inventory();
    let upstream = oracle
        .commands
        .iter()
        .map(|command| (command.name.as_str(), command))
        .collect::<BTreeMap<_, _>>();
    let specs = specs();
    let behaves = POSITIONAL_MAX_BEHAVES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let expected = [
        "choose-buffer",
        "choose-tree",
        "display-message",
        "display-panes",
        "load-buffer",
        "save-buffer",
        "select-pane",
        "set-buffer",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();

    assert_eq!(
        behaves.len(),
        POSITIONAL_MAX_BEHAVES.len(),
        "positional maximum behavior inventory contains duplicates"
    );
    assert_eq!(
        behaves, expected,
        "positional maximum behavior inventory does not match the closed roster"
    );
    for name in behaves {
        let command = upstream.get(name).unwrap_or_else(|| {
            panic!("positional maximum inventory names a native command: {name}")
        });
        let spec = specs.get(name).unwrap_or_else(|| {
            panic!("positional maximum inventory names an absent command: {name}")
        });
        assert_eq!(spec.positional_maximum(), command.max_args, "{name}");
        assert!(
            !items.contains_key(&format!("positional-max:{name}")),
            "verified positional maximum remains tracked: {name}"
        );
    }
}

#[test]
fn option_format_hook_and_default_key_items_match_pinned_inventories() {
    let (oracle, items) = inventory();
    let behaves = BEHAVES.iter().copied().collect::<BTreeSet<_>>();
    let options = crate::tmux_options::tmux_options()
        .filter(|option| !crate::tmux_options::tmux_option_is_hook(option.name))
        .map(|option| option.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        options,
        oracle.options.iter().map(String::as_str).collect(),
        "zz option names differ from the pinned oracle"
    );
    for option in options.difference(&behaves) {
        assert!(
            items.contains_key(&format!("option:{option}")),
            "storage-only option is not tracked: {option}"
        );
    }
    for item in items.keys().filter(|item| item.starts_with("option:")) {
        let option = item.strip_prefix("option:").unwrap();
        assert!(options.contains(option), "stale option item: {item}");
        assert!(
            !behaves.contains(option),
            "behaving option has a stale item: {item}"
        );
    }

    let formats = format_variable_names().collect::<BTreeSet<_>>();
    assert_eq!(
        formats,
        oracle.formats.iter().map(String::as_str).collect(),
        "zz format names differ from the pinned oracle"
    );
    let constant_formats = constant_format_variable_names().collect::<BTreeSet<_>>();
    let tracked_formats = items
        .keys()
        .filter_map(|item| item.strip_prefix("format:"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tracked_formats, constant_formats,
        "constant-backed format variables and tracked format gaps differ"
    );
    for item in items.keys().filter(|item| item.starts_with("format:")) {
        let format = item.strip_prefix("format:").unwrap();
        assert!(formats.contains(format), "stale format item: {item}");
    }
    let hooks = crate::tmux_options::HOOK_NAMES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        hooks,
        oracle.hooks.iter().map(String::as_str).collect(),
        "zz hook names differ from the pinned oracle"
    );
    for item in items.keys().filter(|item| item.starts_with("hook:")) {
        let hook = item.strip_prefix("hook:").unwrap();
        assert!(hooks.contains(hook), "stale hook item: {item}");
    }

    let upstream_keys = oracle
        .key_bindings
        .iter()
        .map(|binding| (binding.table.as_str(), item_key(&binding.key)))
        .collect::<BTreeSet<_>>();
    let key_tables = KeyTables::default();
    let zz_keys = key_tables
        .list(None)
        .map(|(table, key, _)| (table, item_key(key)))
        .collect::<BTreeSet<_>>();
    let untracked_keys = upstream_keys
        .difference(&zz_keys)
        .map(|(table, key)| format!("key:{table}:{key}"))
        .filter(|item| !items.contains_key(item))
        .collect::<Vec<_>>();
    assert!(
        untracked_keys.is_empty(),
        "missing upstream default keys are not tracked: {untracked_keys:?}"
    );
    for item in items.keys().filter(|item| item.starts_with("key:")) {
        let mut fields = item.splitn(3, ':');
        let _ = fields.next();
        let table = fields.next().unwrap();
        let key = fields.next().unwrap();
        let binding = (table, key.to_owned());
        assert!(
            upstream_keys.contains(&binding),
            "stale default key item: {item}"
        );
        assert!(
            !zz_keys.contains(&binding),
            "implemented default key has a stale item: {item}"
        );
    }
    let native_keys = zz_keys
        .difference(&upstream_keys)
        .map(|(table, key)| format!("native-key:{table}:{key}"))
        .collect::<BTreeSet<_>>();
    let tracked_native_keys = items
        .keys()
        .filter(|item| item.starts_with("native-key:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tracked_native_keys, native_keys,
        "zz-only default keys and tracked native key items differ"
    );

    let upstream_bindings = oracle
        .key_bindings
        .iter()
        .map(|binding| {
            (
                (binding.table.as_str(), item_key(&binding.key)),
                (binding.repeat, binding.command.as_str()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let zz_bindings = key_tables
        .list(None)
        .map(|(table, key, binding)| {
            (
                (table, item_key(key)),
                (binding.repeat, format_key_command(binding)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let divergent_bindings = upstream_bindings
        .iter()
        .filter_map(|((table, key), (repeat, command))| {
            zz_bindings.get(&(table, key.clone())).and_then(|zz| {
                (zz.0 != *repeat || zz.1 != *command).then(|| format!("binding:{table}:{key}"))
            })
        })
        .collect::<BTreeSet<_>>();
    let tracked_bindings = items
        .keys()
        .filter(|item| item.starts_with("binding:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tracked_bindings, divergent_bindings,
        "divergent shared default bindings and tracked binding items differ"
    );
}
