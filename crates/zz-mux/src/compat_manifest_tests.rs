use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use zz_protocol::{
    COMMAND_ARGS_PARSE_BEHAVES, COMMAND_ARGS_PARSE_SPECS, CommandArgsParseRule, CommandResolution,
    DAEMON_COMMAND_SPECS, KeyTables, NATIVE_COMMAND_NAMES, POSITIONAL_MINIMUMS, canonical_command,
    canonical_key, resolve_command,
};

use crate::{
    BEHAVES, COMMAND_SPECS, CopyActionCategory, TMUX_OPTION_CONSUMERS,
    command::{
        accepted_native_literal_format_context_scopes, format_key_command,
        missing_derived_format_context_families, missing_literal_format_context_scopes,
        mux_derived_format_context_families, mux_literal_format_context_scopes,
    },
    formats::{
        constant_format_variable_names, delegated_format_variable_names,
        direct_format_variable_names, format_modifier_names, format_variable_names,
    },
};

#[derive(Deserialize)]
struct Manifest {
    gaps: Vec<Gap>,
    closed: Vec<ClosedGap>,
}

#[derive(Deserialize)]
struct Gap {
    id: String,
    decision: String,
    status: String,
    items: Vec<String>,
}

#[derive(Deserialize)]
struct ClosedGap {
    id: String,
}

#[derive(Deserialize)]
struct Oracle {
    schema: usize,
    commands: Vec<OracleCommand>,
    args_parse: BTreeMap<String, String>,
    options: Vec<String>,
    formats: Vec<String>,
    format_contexts: OracleFormatContexts,
    format_modifiers: Vec<String>,
    hooks: Vec<String>,
    key_bindings: Vec<OracleKey>,
}

#[derive(Deserialize)]
struct OracleFormatContexts {
    literal_scopes: Vec<OracleLiteralFormatScope>,
    derived_families: Vec<OracleDerivedFormatFamily>,
    propagation: Vec<OracleFormatPropagation>,
}

#[derive(Deserialize)]
struct OracleLiteralFormatScope {
    path: String,
    function: String,
    names: Vec<String>,
}

#[derive(Deserialize)]
struct OracleDerivedFormatFamily {
    family: String,
    names: Vec<String>,
    patterns: Vec<String>,
    producers: Vec<OracleFormatProducer>,
}

#[derive(Deserialize)]
struct OracleFormatProducer {
    path: String,
    function: String,
}

#[derive(Deserialize)]
struct OracleFormatPropagation {
    path: String,
    function: String,
    callee: String,
}

#[derive(Deserialize)]
struct OracleCommand {
    name: String,
    aliases: Vec<String>,
    usage: String,
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

const STRUCTURALLY_MATCHING_SHARED_BINDINGS_BY_TABLE: &[(&str, usize)] =
    &[("copy-mode", 61), ("copy-mode-vi", 72), ("prefix", 32)];

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
        if spec.positional_minimum() != command.min_args {
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
}

#[test]
fn scoped_format_contexts_and_modifiers_match_the_pinned_oracle() {
    let (oracle, items) = inventory();
    assert_eq!(oracle.schema, 5);

    let mut upstream_literals = BTreeSet::new();
    let mut upstream_scopes = BTreeSet::new();
    let mut upstream_names = BTreeSet::new();
    for scope in &oracle.format_contexts.literal_scopes {
        assert!(
            upstream_scopes.insert((scope.path.clone(), scope.function.clone())),
            "duplicate oracle literal context scope: {}:{}",
            scope.path,
            scope.function
        );
        let names = scope.names.iter().cloned().collect::<BTreeSet<_>>();
        assert_eq!(
            names.len(),
            scope.names.len(),
            "duplicate oracle literal context name in {}:{}",
            scope.path,
            scope.function
        );
        for name in names {
            upstream_names.insert(name.clone());
            assert!(
                upstream_literals.insert((scope.path.clone(), scope.function.clone(), name)),
                "duplicate oracle literal context tuple"
            );
        }
    }
    assert_eq!(upstream_scopes.len(), 31);
    assert_eq!(upstream_literals.len(), 153);
    assert_eq!(upstream_names.len(), 108);

    let mut mux_literals = BTreeSet::new();
    for (path, function, names) in mux_literal_format_context_scopes() {
        for &name in names {
            assert!(
                mux_literals.insert((path.to_owned(), function.to_owned(), name.to_owned(),)),
                "duplicate mux literal context: {path}:{function}:{name}"
            );
        }
    }
    assert_eq!(mux_literals.len(), 82);
    assert!(mux_literals.is_subset(&upstream_literals));

    let mut accepted_native_literals = BTreeSet::new();
    for (path, function, names) in accepted_native_literal_format_context_scopes() {
        for &name in names {
            assert!(
                accepted_native_literals.insert((
                    path.to_owned(),
                    function.to_owned(),
                    name.to_owned(),
                )),
                "duplicate accepted-native literal context: {path}:{function}:{name}"
            );
        }
    }
    assert_eq!(accepted_native_literals.len(), 39);
    assert!(accepted_native_literals.is_subset(&upstream_literals));
    assert!(mux_literals.is_disjoint(&accepted_native_literals));

    let mut missing_literals = BTreeSet::new();
    for (path, function, names) in missing_literal_format_context_scopes() {
        for &name in names {
            assert!(
                missing_literals.insert((path.to_owned(), function.to_owned(), name.to_owned(),)),
                "duplicate missing literal context: {path}:{function}:{name}"
            );
        }
    }
    assert_eq!(missing_literals.len(), 0);
    assert!(missing_literals.is_subset(&upstream_literals));
    assert!(mux_literals.is_disjoint(&missing_literals));
    assert!(accepted_native_literals.is_disjoint(&missing_literals));

    let mut classified_literals = mux_literals.clone();
    classified_literals.extend(accepted_native_literals.iter().cloned());
    classified_literals.extend(missing_literals.iter().cloned());
    let delegated_literals = upstream_literals
        .difference(&classified_literals)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(delegated_literals.len(), 32);
    assert_eq!(
        classified_literals
            .union(&delegated_literals)
            .cloned()
            .collect::<BTreeSet<_>>(),
        upstream_literals
    );

    let mut upstream_families = BTreeMap::new();
    let mut family_producers = BTreeSet::new();
    for family in &oracle.format_contexts.derived_families {
        let names = family.names.iter().cloned().collect::<BTreeSet<_>>();
        let patterns = family.patterns.iter().cloned().collect::<BTreeSet<_>>();
        assert_eq!(names.len(), family.names.len(), "{} names", family.family);
        assert_eq!(
            patterns.len(),
            family.patterns.len(),
            "{} patterns",
            family.family
        );
        assert!(
            !names.is_empty() || !patterns.is_empty(),
            "{}",
            family.family
        );
        assert!(
            upstream_families
                .insert(family.family.clone(), (names, patterns))
                .is_none(),
            "duplicate oracle derived format family: {}",
            family.family
        );
        assert!(!family.producers.is_empty(), "{}", family.family);
        for producer in &family.producers {
            assert!(
                family_producers.insert((
                    family.family.clone(),
                    producer.path.clone(),
                    producer.function.clone(),
                )),
                "duplicate oracle derived format producer"
            );
        }
    }
    assert_eq!(upstream_families.len(), 10);

    let mut mux_families = BTreeSet::new();
    for (family, names, patterns) in mux_derived_format_context_families() {
        let registration = (
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<BTreeSet<_>>(),
            patterns
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect::<BTreeSet<_>>(),
        );
        assert_eq!(
            upstream_families.get(family),
            Some(&registration),
            "mux derived format family differs from the oracle: {family}"
        );
        assert!(
            mux_families.insert(family),
            "duplicate mux derived format family: {family}"
        );
    }
    assert_eq!(mux_families.len(), 9);

    let mut missing_families = BTreeSet::new();
    for (family, names, patterns) in missing_derived_format_context_families() {
        let registration = (
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<BTreeSet<_>>(),
            patterns
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect::<BTreeSet<_>>(),
        );
        assert_eq!(
            upstream_families.get(family),
            Some(&registration),
            "missing derived format family differs from the oracle: {family}"
        );
        assert!(
            missing_families.insert(family),
            "duplicate missing derived format family: {family}"
        );
    }
    assert!(missing_families.is_empty());
    assert!(mux_families.is_disjoint(&missing_families));
    let classified_families = mux_families
        .union(&missing_families)
        .copied()
        .collect::<BTreeSet<_>>();
    let upstream_family_names = upstream_families
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let delegated_families = upstream_family_names
        .difference(&classified_families)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(delegated_families, BTreeSet::from(["run-shell-position"]));

    let propagation = oracle
        .format_contexts
        .propagation
        .iter()
        .map(|entry| {
            (
                entry.path.as_str(),
                entry.function.as_str(),
                entry.callee.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(propagation.len(), 5);
    assert_eq!(propagation.len(), oracle.format_contexts.propagation.len());
    assert!(
        propagation
            .iter()
            .all(|(_, _, callee)| matches!(*callee, "format_add" | "format_merge"))
    );

    let upstream_modifiers = oracle
        .format_modifiers
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(upstream_modifiers.len(), 36);
    assert_eq!(upstream_modifiers.len(), oracle.format_modifiers.len());
    let implemented_modifiers = format_modifier_names().collect::<BTreeSet<_>>();
    assert_eq!(implemented_modifiers.len(), 36);
    let missing_modifiers = upstream_modifiers
        .difference(&implemented_modifiers)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(missing_modifiers, BTreeSet::new());
    assert!(implemented_modifiers.is_subset(&upstream_modifiers));

    let missing_modifier_items = BTreeMap::<&str, (&str, &str)>::new();
    assert_eq!(
        missing_modifier_items
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        missing_modifiers
    );

    assert!(
        items.keys().all(|item| !item.starts_with("context-format:")
            && !item.starts_with("native-context-format:")),
        "registration-only context partitions must not mint runtime tuple items"
    );

    let manifest: Manifest = read_json(&root().join("compat/tmux-gaps.json"));
    let groups = manifest
        .gaps
        .iter()
        .map(|gap| {
            (
                gap.id.as_str(),
                (gap.decision.as_str(), gap.status.as_str()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let item = "semantic:native-format-context-producers";
    let owner = "formats.native-typed-context-producers";
    assert_eq!(
        items.get(item).map(String::as_str),
        Some(owner),
        "wrong manifest owner for {item}"
    );
    assert_eq!(
        groups.get(owner).copied(),
        Some(("native", "accepted")),
        "wrong manifest decision or status for {owner}"
    );
    for (item, owner) in missing_modifier_items.values() {
        assert_eq!(
            items.get(*item).map(String::as_str),
            Some(*owner),
            "wrong manifest owner for {item}"
        );
        assert_eq!(
            groups.get(*owner).copied(),
            Some(("adopt", "open")),
            "wrong manifest decision or status for {owner}"
        );
    }
    let closed = manifest
        .closed
        .iter()
        .map(|gap| gap.id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(
        closed.contains("tracker.format-vocabulary-registration"),
        "format vocabulary registration tracker must be closed"
    );
    assert!(
        closed.contains("formats.repeat-modifier"),
        "implemented repeat modifier tracker must be closed"
    );
    assert!(
        closed.contains("formats.context-producer-fidelity"),
        "implemented context producer tracker must be closed"
    );
    assert!(
        [
            "semantic:tracker-format-modifier-vocabulary",
            "semantic:tracker-open-context-format-vocabulary",
            "semantic:format-modifier-repeat",
            "semantic:format-context-producer-fidelity",
        ]
        .iter()
        .all(|item| !items.contains_key(*item)),
        "closed registration items must not remain active"
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
    assert_eq!(expected.len(), 24);
    assert!(!expected.contains("display-panes"));
    for name in &expected {
        let spec = zz_protocol::catalog_command_spec(name).expect("daemon command spec");
        assert!(spec.uses_tmux_option_grammar(), "{name}");
        assert!(
            !upstream[name].flags.contains_key("-0"),
            "daemon invalid-flag probe is now valid for {name}"
        );
    }

    let tracked = items.contains_key("semantic:tracker-daemon-invalid-flag-runtime");
    assert!(!tracked, "daemon invalid-flag runtime remains tracked");
}

#[test]
fn command_flag_fixture_matches_the_pin() {
    let (oracle, _) = inventory();
    let specs = specs();
    let mut expected = String::new();
    let mut rows = 0;
    let mut aliases = 0;
    let mut required = 0;

    for command in &oracle.commands {
        if !specs.contains_key(command.name.as_str()) {
            continue;
        }
        assert!(command.aliases.len() <= 1, "{}", command.name);
        let alias = command.aliases.first().map_or("-", String::as_str);
        aliases += usize::from(alias != "-");
        let required_option = command
            .flags
            .iter()
            .find_map(|(name, shape)| (shape == "required").then_some(name.as_str()))
            .unwrap_or("-");
        required += usize::from(required_option != "-");
        let usage = if command.usage.is_empty() {
            "@EMPTY@"
        } else {
            &command.usage
        };
        writeln!(
            expected,
            "{}\t{alias}\t{required_option}\t{usage}",
            command.name
        )
        .expect("write command flag fixture row");
        rows += 1;
    }

    assert_eq!((rows, aliases, required), (83, 74, 79));
    assert_eq!(
        fs::read_to_string(root().join("compat/scenarios/smoke/fixtures/command-flag-errors.tsv"))
            .expect("command flag fixture corpus"),
        expected
    );
}

#[test]
fn positional_maximum_runtime_inventory_matches_the_pin() {
    let (oracle, items) = inventory();
    let specs = specs();
    let mut implemented = BTreeSet::new();
    let mut unimplemented = BTreeSet::new();

    for command in oracle
        .commands
        .iter()
        .filter(|command| command.max_args.is_some())
    {
        if let Some(spec) = specs.get(command.name.as_str()) {
            assert!(
                !NATIVE_COMMAND_NAMES.contains(&spec.name),
                "upstream maximum resolved to a native command: {}",
                command.name
            );
            assert_eq!(
                spec.positional_maximum(),
                command.max_args,
                "{}",
                command.name
            );
            assert!(
                !items.contains_key(&format!("positional-max:{}", command.name)),
                "verified positional maximum remains tracked: {}",
                command.name
            );
            implemented.insert(command.name.as_str());
        } else {
            assert!(
                matches!(
                    resolve_command(&command.name),
                    CommandResolution::Unimplemented(name) if name == command.name
                ),
                "finite upstream maximum is neither implemented nor explicitly unsupported: {}",
                command.name
            );
            unimplemented.insert(command.name.as_str());
        }
    }

    assert_eq!(implemented.len(), 72);
    assert_eq!(unimplemented.len(), 8);
    assert_eq!(implemented.len() + unimplemented.len(), 80);
}

#[test]
fn positional_minimum_runtime_inventory_matches_the_pin() {
    let (oracle, items) = inventory();
    let upstream = oracle
        .commands
        .iter()
        .map(|command| (command.name.as_str(), command))
        .collect::<BTreeMap<_, _>>();
    let specs = specs();
    let behaves = POSITIONAL_MINIMUMS
        .iter()
        .copied()
        .collect::<BTreeMap<_, _>>();
    let expected = BTreeMap::from([
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
    ]);

    assert_eq!(
        behaves.len(),
        POSITIONAL_MINIMUMS.len(),
        "positional minimum behavior inventory contains duplicates"
    );
    assert_eq!(
        behaves, expected,
        "positional minimum behavior inventory does not match the closed roster"
    );
    for (name, minimum) in behaves {
        let command = upstream.get(name).unwrap_or_else(|| {
            panic!("positional minimum inventory names a native command: {name}")
        });
        let spec = specs.get(name).unwrap_or_else(|| {
            panic!("positional minimum inventory names an absent command: {name}")
        });
        assert_eq!(minimum, command.min_args, "{name}");
        assert_eq!(spec.positional_minimum(), minimum, "{name}");
        assert!(
            !items.contains_key(&format!("positional-min:{name}")),
            "verified positional minimum remains tracked: {name}"
        );
    }
}

#[test]
fn tmux_option_consumer_partition_matches_pinned_inventory() {
    let (oracle, items) = inventory();
    let manifest: Manifest = read_json(&root().join("compat/tmux-gaps.json"));
    let oracle_options = oracle
        .options
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let catalog = crate::tmux_options::tmux_options()
        .filter(|option| !crate::tmux_options::tmux_option_is_hook(option.name))
        .map(|option| option.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(oracle.options.len(), 180, "pinned option count changed");
    assert_eq!(
        oracle_options.len(),
        180,
        "pinned options contain duplicates"
    );
    assert_eq!(catalog.len(), 180, "live option catalog count changed");
    assert_eq!(
        catalog, oracle_options,
        "live option catalog differs from the pin"
    );

    let consumers = TMUX_OPTION_CONSUMERS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(TMUX_OPTION_CONSUMERS.len(), 118);
    assert_eq!(
        consumers.len(),
        118,
        "option consumer roster contains duplicates"
    );
    assert!(
        consumers.is_subset(&catalog),
        "option consumer roster contains names outside the catalog"
    );
    let mut scope_counts = [0; 4];
    for name in TMUX_OPTION_CONSUMERS {
        let option = crate::tmux_options::exact_tmux_option(name)
            .unwrap_or_else(|| panic!("consumer option is not exact: {name}"));
        let index = match option.scope {
            crate::tmux_options::TmuxOptionScope::Server => 0,
            crate::tmux_options::TmuxOptionScope::Session => 1,
            crate::tmux_options::TmuxOptionScope::Window => 2,
            crate::tmux_options::TmuxOptionScope::WindowPane => 3,
        };
        scope_counts[index] += 1;
    }
    assert_eq!(scope_counts, [15, 44, 41, 18]);

    let tracked = items
        .keys()
        .filter_map(|item| item.strip_prefix("option:"))
        .collect::<BTreeSet<_>>();
    assert_eq!(tracked.len(), 62, "active option gap count changed");
    assert!(
        consumers.is_disjoint(&tracked),
        "consumed and tracked option names overlap"
    );
    assert_eq!(
        consumers.union(&tracked).copied().collect::<BTreeSet<_>>(),
        catalog,
        "consumed and tracked option names do not partition the catalog"
    );

    assert!(
        !items.contains_key("semantic:tracker-option-consumer-registration"),
        "closed option consumer registration remains tracked"
    );
    assert!(
        manifest
            .gaps
            .iter()
            .all(|gap| gap.id != "tracker.semantic-coverage"),
        "closed option consumer registration group remains active"
    );
    assert!(
        manifest
            .closed
            .iter()
            .any(|gap| gap.id == "tracker.semantic-coverage"),
        "option consumer registration group is not closed"
    );
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
    let direct_formats = direct_format_variable_names().collect::<BTreeSet<_>>();
    let delegated_formats = delegated_format_variable_names().collect::<BTreeSet<_>>();
    assert_eq!(formats.len(), 198, "pinned global format count changed");
    assert_eq!(constant_formats.len(), 51, "tracked format count changed");
    assert_eq!(direct_formats.len(), 99, "direct format count changed");
    assert_eq!(
        delegated_formats.len(),
        48,
        "delegated format count changed"
    );
    assert!(
        constant_formats.is_disjoint(&direct_formats),
        "tracked and direct format registrations overlap"
    );
    assert!(
        constant_formats.is_disjoint(&delegated_formats),
        "tracked and delegated format registrations overlap"
    );
    assert!(
        direct_formats.is_disjoint(&delegated_formats),
        "direct and delegated format registrations overlap"
    );
    let nonconstant_formats = direct_formats
        .union(&delegated_formats)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        nonconstant_formats.len(),
        147,
        "nonconstant format registration count changed"
    );
    let tracked_formats = items
        .keys()
        .filter_map(|item| item.strip_prefix("format:"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tracked_formats, constant_formats,
        "constant-backed format variables and tracked format gaps differ"
    );
    let classified_formats = nonconstant_formats
        .union(&tracked_formats)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        classified_formats, formats,
        "nonconstant behavior registrations and tracked format gaps do not partition the pin"
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
    let missing_keys = upstream_keys
        .difference(&zz_keys)
        .cloned()
        .collect::<BTreeSet<_>>();
    let untracked_keys = missing_keys
        .iter()
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

    let shared_keys = upstream_keys
        .intersection(&zz_keys)
        .cloned()
        .collect::<BTreeSet<_>>();
    let structurally_matching_bindings = shared_keys
        .iter()
        .filter(|(table, key)| !divergent_bindings.contains(&format!("binding:{table}:{key}")))
        .cloned()
        .collect::<BTreeSet<_>>();
    let matching_by_table = structurally_matching_bindings.iter().fold(
        BTreeMap::<&str, usize>::new(),
        |mut counts, (table, _)| {
            *counts.entry(table).or_default() += 1;
            counts
        },
    );
    assert_eq!(
        oracle.key_bindings.len(),
        303,
        "pinned binding count changed"
    );
    assert_eq!(zz_keys.len(), 295, "zz default binding count changed");
    assert_eq!(
        shared_keys.len(),
        210,
        "shared default binding count changed"
    );
    assert_eq!(
        missing_keys.len(),
        93,
        "missing default binding count changed"
    );
    assert_eq!(
        native_keys.len(),
        85,
        "native default binding count changed"
    );
    assert_eq!(
        divergent_bindings.len(),
        45,
        "divergent shared binding count changed"
    );
    assert_eq!(
        structurally_matching_bindings.len(),
        165,
        "structurally matching shared binding count changed"
    );
    assert_eq!(
        matching_by_table,
        STRUCTURALLY_MATCHING_SHARED_BINDINGS_BY_TABLE
            .iter()
            .copied()
            .collect(),
        "structurally matching shared binding tables changed"
    );
}

#[test]
fn stock_copy_mode_action_keys_render_the_pinned_binding() {
    let (oracle, _) = inventory();
    let key_tables = KeyTables::default();
    let zz = key_tables
        .list(None)
        .map(|(table, key, binding)| {
            (
                (table, item_key(key)),
                (binding.repeat, format_key_command(binding)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (table, key) in [
        ("copy-mode-vi", "P"),
        ("copy-mode", "P"),
        ("copy-mode", "C-M-b"),
        ("copy-mode", "C-l"),
        ("copy-mode", "M-l"),
    ] {
        let pinned = oracle
            .key_bindings
            .iter()
            .find(|binding| binding.table == table && item_key(&binding.key) == key)
            .unwrap_or_else(|| panic!("the pin binds {table} {key}"));
        assert_eq!(
            zz.get(&(table, key.to_owned())),
            Some(&(pinned.repeat, pinned.command.clone())),
            "{table} {key}"
        );
    }
}

#[test]
fn missing_copy_actions_keep_their_behavior_item_open() {
    let manifest: Manifest = read_json(&root().join("compat/tmux-gaps.json"));
    let open = manifest
        .gaps
        .iter()
        .find(|gap| gap.id == "copy-mode.action-fidelity")
        .map(|group| {
            group
                .items
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut missing = BTreeSet::new();
    for entry in crate::copy_actions::missing_copy_mode_actions() {
        missing.insert(entry.category);
    }
    assert!(
        missing
            .iter()
            .all(|category| *category == CopyActionCategory::Vocabulary),
        "a behavior category regained an unmapped pinned action: {missing:?}"
    );
    for (category, item) in [
        (
            CopyActionCategory::CursorGeometry,
            "semantic:copy-mode-cursor-geometry",
        ),
        (
            CopyActionCategory::LogicalLineAndModeKeys,
            "semantic:copy-mode-logical-line-and-mode-keys",
        ),
        (CopyActionCategory::GotoLine, "semantic:copy-mode-goto-line"),
        (
            CopyActionCategory::SelectionLifecycle,
            "semantic:copy-mode-selection-lifecycle",
        ),
        (
            CopyActionCategory::JumpPagePrompt,
            "semantic:copy-mode-jump-page-prompt-actions",
        ),
        (
            CopyActionCategory::CopyFormatAndDestination,
            "semantic:copy-mode-copy-format-and-destination",
        ),
    ] {
        assert!(
            !missing.contains(&category) || open.contains(item),
            "{item} was closed while {category:?} still has unmapped pinned actions"
        );
    }
}
