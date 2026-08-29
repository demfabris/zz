#!/usr/bin/env python3

import argparse
import json
import re
import sys
from collections import Counter
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "compat" / "tmux-gaps.json"
ORACLE = ROOT / "compat" / "tmux-oracle.json"
REPORT = ROOT / "knowledge" / "tmux" / "gaps.md"
SCENARIOS = ROOT / "compat" / "scenarios"
FETCH_TMUX = ROOT / "compat" / "fetch-tmux.sh"
PIN_DOCS = [
    ROOT / "knowledge" / "playbooks" / "compat-harness.md",
    ROOT / "knowledge" / "tmux" / "tmux-compat.md",
    ROOT / "third_party" / "tmux-reference" / "UPSTREAM.md",
]
DECISIONS = {"adopt", "native", "park", "never"}
STATUSES = {"open", "blocked", "accepted"}
PRIORITIES = {"now", "next", "later", "none"}
EASE = {"easy", "medium", "hard", "hardest", "none"}
OWNERS = {"client", "daemon", "gui", "mux", "protocol", "terminal"}
IMPACTS = {"admin", "daily", "gui", "remote", "scripts"}
GAP_FIELDS = {
    "id",
    "title",
    "decision",
    "status",
    "priority",
    "ease",
    "impact",
    "owner",
    "items",
    "evidence",
    "acceptance",
    "depends_on",
    "reason",
}
CLOSED_FIELDS = {"id", "title", "closed_on", "evidence", "resolution"}
DIFFERENTIAL_FIELDS = {"scenario", "gap", "topo", "geo", "fmt", "out", "warn"}
ITEM_PATTERNS = [
    re.compile(r"^command:[a-z0-9][a-z0-9-]*$"),
    re.compile(r"^flag:[a-z0-9][a-z0-9-]*:-[A-Za-z0-9]$"),
    re.compile(r"^flag-arity:[a-z0-9][a-z0-9-]*:-[A-Za-z0-9]$"),
    re.compile(r"^positional-(?:min|max):[a-z0-9][a-z0-9-]*$"),
    re.compile(r"^option:[a-z0-9][a-z0-9-]*$"),
    re.compile(r"^format:[a-z0-9][a-z0-9_]*$"),
    re.compile(r"^(?:native-)?context-format:[a-z0-9][a-z0-9-]*:[a-z0-9][a-z0-9_]*$"),
    re.compile(r"^hook:[a-z0-9][a-z0-9-]*$"),
    re.compile(r"^key:[a-z0-9][a-z0-9-]*:.+$"),
    re.compile(r"^binding:[a-z0-9][a-z0-9-]*:.+$"),
    re.compile(r"^native-key:[a-z0-9][a-z0-9-]*:.+$"),
    re.compile(
        r"^prefix-collision:[a-z0-9][a-z0-9-]*:[a-z0-9][a-z0-9-]*:[a-z0-9][a-z0-9-]*$"
    ),
    re.compile(r"^native-command:[a-z0-9][a-z0-9-]*$"),
    re.compile(r"^native-alias:[a-z0-9][a-z0-9-]*:[a-z0-9][a-z0-9-]*$"),
    re.compile(r"^extension-flag:[a-z0-9][a-z0-9-]*:-[A-Za-z0-9]$"),
    re.compile(r"^args-parse:[a-z0-9][a-z0-9-]*$"),
    re.compile(r"^(semantic|presentation|protocol):[a-z0-9][a-z0-9._-]*$"),
]


def iso_date(value):
    if not isinstance(value, str) or re.fullmatch(r"20[0-9]{2}-[0-9]{2}-[0-9]{2}", value) is None:
        return False
    try:
        return date.fromisoformat(value).isoformat() == value
    except ValueError:
        return False


def load_json(path, errors):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        errors.append(f"missing {path.relative_to(ROOT)}")
    except (OSError, json.JSONDecodeError) as error:
        errors.append(f"cannot read {path.relative_to(ROOT)}: {error}")
    return None


def string_list(value, location, errors, allow_empty=True):
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        errors.append(f"{location} must be an array of strings")
        return []
    if not allow_empty and not value:
        errors.append(f"{location} must not be empty")
    for item in value:
        if not item or item != item.strip() or "\n" in item:
            errors.append(f"{location} contains an empty or non-normalized value: {item!r}")
    duplicates = sorted(item for item, count in Counter(value).items() if count > 1)
    if duplicates:
        errors.append(f"{location} contains duplicates: {', '.join(duplicates)}")
    return value


def check_path_reference(value, location, errors):
    match = re.match(r"^(resource|scenario|file):(.+)$", value)
    if match is None:
        return
    raw = re.sub(r":\d+(?::\d+)?$", "", match.group(2).split("#", 1)[0])
    path = Path(raw)
    if path.is_absolute() or ".." in path.parts:
        errors.append(f"{location} has an unsafe path: {value}")
        return
    if not (ROOT / path).exists():
        errors.append(f"{location} references a missing path: {value}")


def validate_manifest(manifest, oracle, include_report):
    errors = []
    if not isinstance(manifest, dict):
        return ["compat/tmux-gaps.json must contain an object"]
    expected_root = {"schema", "updated_on", "pin", "gaps", "closed", "known_differentials"}
    if set(manifest) != expected_root:
        errors.append(
            "compat/tmux-gaps.json fields must be "
            + ", ".join(sorted(expected_root))
            + f"; got {', '.join(sorted(manifest))}"
        )
    if manifest.get("schema") != 3:
        errors.append("compat/tmux-gaps.json schema must be 3")
    updated_on = manifest.get("updated_on")
    if not iso_date(updated_on):
        errors.append("compat/tmux-gaps.json updated_on must be a valid YYYY-MM-DD date")
    pin = manifest.get("pin")
    if not isinstance(pin, str) or re.fullmatch(r"[0-9a-f]{40}", pin) is None:
        errors.append("compat/tmux-gaps.json pin must be a full lowercase commit hash")
    gaps = manifest.get("gaps")
    if not isinstance(gaps, list):
        errors.append("compat/tmux-gaps.json gaps must be an array")
        gaps = []
    ids = set()
    items = {}
    ordered_ids = []
    gap_by_id = {}
    dependency_lists = {}
    for index, gap in enumerate(gaps):
        location = f"gaps[{index}]"
        if not isinstance(gap, dict):
            errors.append(f"{location} must be an object")
            continue
        if set(gap) != GAP_FIELDS:
            errors.append(
                f"{location} fields must be {', '.join(sorted(GAP_FIELDS))}; "
                f"got {', '.join(sorted(gap))}"
            )
        gap_id = gap.get("id")
        if not isinstance(gap_id, str) or re.fullmatch(r"[a-z0-9][a-z0-9._-]*", gap_id) is None:
            errors.append(f"{location}.id is not normalized: {gap_id!r}")
            gap_id = f"#{index}"
        elif gap_id in ids:
            errors.append(f"duplicate gap id: {gap_id}")
        ids.add(gap_id)
        ordered_ids.append(gap_id)
        gap_by_id.setdefault(gap_id, gap)
        for field in ("title", "owner", "reason"):
            value = gap.get(field)
            if not isinstance(value, str) or not value.strip() or value != value.strip() or "\n" in value:
                errors.append(f"{location}.{field} must be one normalized nonempty line")
        for field, allowed in (
            ("decision", DECISIONS),
            ("status", STATUSES),
            ("priority", PRIORITIES),
            ("ease", EASE),
        ):
            if gap.get(field) not in allowed:
                errors.append(f"{location}.{field} must be one of {', '.join(sorted(allowed))}")
        if gap.get("owner") not in OWNERS:
            errors.append(f"{location}.owner must be one of {', '.join(sorted(OWNERS))}")
        impact = string_list(gap.get("impact"), f"{location}.impact", errors, allow_empty=False)
        unknown_impact = sorted(set(impact) - IMPACTS)
        if unknown_impact:
            errors.append(f"{location}.impact contains unknown values: {', '.join(unknown_impact)}")
        gap_items = string_list(gap.get("items"), f"{location}.items", errors, allow_empty=False)
        if gap_items != sorted(gap_items):
            errors.append(f"{location}.items must be sorted")
        evidence = string_list(gap.get("evidence"), f"{location}.evidence", errors, allow_empty=False)
        acceptance = string_list(
            gap.get("acceptance"), f"{location}.acceptance", errors, allow_empty=False
        )
        dependency_lists[gap_id] = string_list(
            gap.get("depends_on"), f"{location}.depends_on", errors
        )
        for item in gap_items:
            if not any(pattern.fullmatch(item) for pattern in ITEM_PATTERNS):
                errors.append(f"{location}.items contains an invalid item: {item}")
            if item in items:
                errors.append(f"item {item} belongs to both {items[item]} and {gap_id}")
            items[item] = gap_id
        for value in evidence:
            if re.match(r"^(resource|scenario|file):", value) is None:
                errors.append(f"{location}.evidence must use resource:, scenario:, or file:: {value}")
            check_path_reference(value, location, errors)
        for value in acceptance:
            check_path_reference(value, location, errors)
        status = gap.get("status")
        decision = gap.get("decision")
        priority = gap.get("priority")
        ease = gap.get("ease")
        if status == "accepted" and decision not in {"native", "never"}:
            errors.append(f"{location} accepted gaps must use decision native or never")
        if decision == "never" and status != "accepted":
            errors.append(f"{location} decision never requires accepted status")
        if decision == "park" and status != "blocked":
            errors.append(f"{location} decision park requires blocked status")
        if (priority == "none") != (status == "accepted"):
            errors.append(f"{location} priority none is reserved for accepted gaps")
        if (ease == "none") != (status == "accepted"):
            errors.append(f"{location} ease none is reserved for accepted gaps")
    if ordered_ids != sorted(ordered_ids):
        errors.append("compat/tmux-gaps.json gaps must be sorted by id")
    for index, gap in enumerate(gaps):
        if not isinstance(gap, dict):
            continue
        for dependency in dependency_lists.get(gap.get("id"), []):
            if dependency not in ids:
                errors.append(f"gaps[{index}].depends_on references unknown gap: {dependency}")
            if dependency == gap.get("id"):
                errors.append(f"gaps[{index}].depends_on references itself")
    dependencies = {
        gap_id: dependency_lists.get(gap_id, [])
        for gap_id in gap_by_id
    }
    visiting = set()
    visited = set()

    def visit(gap_id):
        if gap_id in visiting:
            errors.append(f"dependency cycle reaches {gap_id}")
            return
        if gap_id in visited:
            return
        visiting.add(gap_id)
        for dependency in dependencies.get(gap_id, []):
            if dependency in dependencies:
                visit(dependency)
        visiting.remove(gap_id)
        visited.add(gap_id)

    for gap_id in dependencies:
        visit(gap_id)
    closed = manifest.get("closed")
    if not isinstance(closed, list):
        errors.append("compat/tmux-gaps.json closed must be an array")
        closed = []
    closed_ids = []
    for index, entry in enumerate(closed):
        location = f"closed[{index}]"
        if not isinstance(entry, dict):
            errors.append(f"{location} must be an object")
            continue
        if set(entry) != CLOSED_FIELDS:
            errors.append(
                f"{location} fields must be {', '.join(sorted(CLOSED_FIELDS))}; "
                f"got {', '.join(sorted(entry))}"
            )
        closed_id = entry.get("id")
        if not isinstance(closed_id, str) or re.fullmatch(r"[a-z0-9][a-z0-9._-]*", closed_id) is None:
            errors.append(f"{location}.id is not normalized: {closed_id!r}")
        else:
            if closed_id in ids or closed_id in closed_ids:
                errors.append(f"duplicate active or closed gap id: {closed_id}")
            closed_ids.append(closed_id)
        for field in ("title", "resolution"):
            value = entry.get(field)
            if not isinstance(value, str) or not value.strip() or value != value.strip() or "\n" in value:
                errors.append(f"{location}.{field} must be one normalized nonempty line")
        closed_on = entry.get("closed_on")
        if not iso_date(closed_on):
            errors.append(f"{location}.closed_on must be a valid YYYY-MM-DD date")
        evidence = string_list(entry.get("evidence"), f"{location}.evidence", errors, allow_empty=False)
        for value in evidence:
            if re.match(r"^(resource|scenario|file):", value) is None:
                errors.append(f"{location}.evidence must use resource:, scenario:, or file:: {value}")
            check_path_reference(value, location, errors)
    if closed_ids != sorted(closed_ids):
        errors.append("compat/tmux-gaps.json closed entries must be sorted by id")
    if not isinstance(oracle, dict):
        errors.append("compat/tmux-oracle.json must contain an object")
    else:
        expected_oracle = {
            "schema",
            "pin",
            "version",
            "commands",
            "args_parse",
            "options",
            "formats",
            "format_contexts",
            "format_modifiers",
            "hooks",
            "key_bindings",
        }
        if set(oracle) != expected_oracle:
            errors.append(
                "compat/tmux-oracle.json fields must be "
                + ", ".join(sorted(expected_oracle))
                + f"; got {', '.join(sorted(oracle))}"
            )
        if oracle.get("schema") != 5:
            errors.append("compat/tmux-oracle.json schema must be 5")
        if oracle.get("pin") != pin:
            errors.append("manifest and oracle pins differ")
        if not isinstance(oracle.get("version"), str) or not oracle.get("version"):
            errors.append("compat/tmux-oracle.json version must be a nonempty string")
        commands = oracle.get("commands")
        if not isinstance(commands, list):
            errors.append("compat/tmux-oracle.json commands must be an array")
            commands = []
        command_names = []
        command_spellings = set()
        for index, command in enumerate(commands):
            location = f"oracle.commands[{index}]"
            if not isinstance(command, dict):
                errors.append(f"{location} must be an object")
                continue
            expected_fields = {
                "name",
                "aliases",
                "usage",
                "flags",
                "min_args",
                "max_args",
            }
            if set(command) != expected_fields:
                errors.append(f"{location} fields must be {', '.join(sorted(expected_fields))}")
            name = command.get("name")
            if not isinstance(name, str) or re.fullmatch(r"[a-z0-9][a-z0-9-]*", name) is None:
                errors.append(f"{location}.name is not normalized: {name!r}")
                continue
            command_names.append(name)
            aliases = string_list(command.get("aliases"), f"{location}.aliases", errors)
            flags = command.get("flags")
            if not isinstance(flags, dict):
                errors.append(f"{location}.flags must be an object")
                flags = {}
            flag_names = list(flags)
            if flag_names != sorted(flag_names):
                errors.append(f"{location}.flags must be sorted")
            for flag, arity in flags.items():
                if re.fullmatch(r"-[A-Za-z0-9]", flag) is None:
                    errors.append(f"{location}.flags contains an invalid name: {flag!r}")
                if not isinstance(arity, str) or arity not in {"none", "required", "optional"}:
                    errors.append(f"{location}.flags[{flag!r}] has invalid arity: {arity!r}")
            minimum = command.get("min_args")
            maximum = command.get("max_args")
            if not isinstance(minimum, int) or isinstance(minimum, bool) or minimum < 0:
                errors.append(f"{location}.min_args must be a nonnegative integer")
            if maximum is not None and (
                not isinstance(maximum, int) or isinstance(maximum, bool) or maximum < 0
            ):
                errors.append(f"{location}.max_args must be null or a nonnegative integer")
            if (
                isinstance(minimum, int)
                and not isinstance(minimum, bool)
                and isinstance(maximum, int)
                and not isinstance(maximum, bool)
                and maximum < minimum
            ):
                errors.append(f"{location}.max_args must be at least min_args")
            if not isinstance(command.get("usage"), str) or "\n" in command.get("usage", ""):
                errors.append(f"{location}.usage must be one line")
            for spelling in [name, *aliases]:
                if re.fullmatch(r"[a-z0-9][a-z0-9-]*", spelling) is None:
                    errors.append(f"{location} contains a non-normalized spelling: {spelling!r}")
                if spelling in command_spellings:
                    errors.append(f"duplicate oracle command spelling: {spelling}")
                command_spellings.add(spelling)
        if command_names != sorted(command_names):
            errors.append("oracle.commands must be sorted by name")
        if len(command_names) != len(set(command_names)):
            errors.append("oracle.commands contains duplicate names")
        args_parse = oracle.get("args_parse")
        if not isinstance(args_parse, dict) or not args_parse:
            errors.append("compat/tmux-oracle.json args_parse must be a nonempty object")
            args_parse = {}
        if list(args_parse) != sorted(args_parse):
            errors.append("oracle.args_parse must be sorted by command name")
        args_parse_rules = {
            "commands-or-string",
            "display-menu-items",
            "if-shell-branches",
            "run-shell-command-flag",
            "set-hook-monitor-or-value",
            "set-option-value",
        }
        if len(args_parse) != 14:
            errors.append(f"oracle.args_parse must contain 14 commands, got {len(args_parse)}")
        observed_args_parse_rules = {
            rule for rule in args_parse.values() if isinstance(rule, str)
        }
        if observed_args_parse_rules != args_parse_rules:
            errors.append("oracle.args_parse must contain all 6 recognized effective rules")
        for name, rule in args_parse.items():
            if name not in command_names:
                errors.append(f"oracle.args_parse names an unknown command: {name}")
            if not isinstance(rule, str) or rule not in args_parse_rules:
                errors.append(f"oracle.args_parse[{name!r}] has an unknown rule: {rule!r}")
            item = f"args-parse:{name}"
            if item in items and f"command:{name}" in items:
                errors.append(f"unimplemented callback command has a separate args-parse item: {item}")
        for item in items:
            if not item.startswith("args-parse:"):
                continue
            name = item.removeprefix("args-parse:")
            if name not in args_parse:
                errors.append(f"stale args-parse item: {item}")
        for field in ("options", "formats", "hooks"):
            values = string_list(oracle.get(field), f"oracle.{field}", errors, allow_empty=False)
            if values != sorted(values):
                errors.append(f"oracle.{field} must be sorted")
            pattern = (
                r"[a-z0-9][a-z0-9_]*"
                if field == "formats"
                else r"[a-z0-9][a-z0-9-]*"
            )
            for value in values:
                if re.fullmatch(pattern, value) is None:
                    errors.append(f"oracle.{field} contains a non-normalized name: {value!r}")
        format_contexts = oracle.get("format_contexts")
        if not isinstance(format_contexts, dict):
            errors.append("compat/tmux-oracle.json format_contexts must be an object")
            format_contexts = {}
        expected_context_fields = {"literal_scopes", "derived_families", "propagation"}
        if set(format_contexts) != expected_context_fields:
            errors.append(
                "oracle.format_contexts fields must be "
                + ", ".join(sorted(expected_context_fields))
            )
        literal_scopes = format_contexts.get("literal_scopes")
        if not isinstance(literal_scopes, list):
            errors.append("oracle.format_contexts.literal_scopes must be an array")
            literal_scopes = []
        literal_order = []
        literal_pairs = set()
        literal_names = set()
        for index, scope in enumerate(literal_scopes):
            location = f"oracle.format_contexts.literal_scopes[{index}]"
            if not isinstance(scope, dict) or set(scope) != {"path", "function", "names"}:
                errors.append(f"{location} fields must be function, names, path")
                continue
            path = scope.get("path")
            function = scope.get("function")
            if not isinstance(path, str) or re.fullmatch(r"[a-z0-9][a-z0-9-]*\.c", path) is None:
                errors.append(f"{location}.path is not a normalized C basename: {path!r}")
            if not isinstance(function, str) or re.fullmatch(r"[a-z0-9][a-z0-9_]*", function) is None:
                errors.append(f"{location}.function is not normalized: {function!r}")
            names = string_list(scope.get("names"), f"{location}.names", errors, allow_empty=False)
            if names != sorted(names):
                errors.append(f"{location}.names must be sorted")
            if isinstance(path, str) and isinstance(function, str):
                literal_order.append((path, function))
                for name in names:
                    if re.fullmatch(r"[a-z0-9][a-z0-9_]*", name) is None:
                        errors.append(f"{location}.names contains a non-normalized name: {name!r}")
                    literal_pairs.add((path, function, name))
                    literal_names.add(name)
        if literal_order != sorted(literal_order) or len(literal_order) != len(set(literal_order)):
            errors.append("oracle.format_contexts.literal_scopes must be unique and sorted")
        if (len(literal_scopes), len(literal_pairs), len(literal_names)) != (31, 153, 108):
            errors.append(
                "oracle format literals must contain 31 scopes, 153 scoped pairs, and 108 unique names"
            )
        derived_families = format_contexts.get("derived_families")
        if not isinstance(derived_families, list):
            errors.append("oracle.format_contexts.derived_families must be an array")
            derived_families = []
        derived_order = []
        for index, family in enumerate(derived_families):
            location = f"oracle.format_contexts.derived_families[{index}]"
            expected_fields = {"family", "names", "patterns", "producers"}
            if not isinstance(family, dict) or set(family) != expected_fields:
                errors.append(f"{location} fields must be family, names, patterns, producers")
                continue
            name = family.get("family")
            if not isinstance(name, str) or re.fullmatch(r"[a-z0-9][a-z0-9-]*", name) is None:
                errors.append(f"{location}.family is not normalized: {name!r}")
            else:
                derived_order.append(name)
            for field in ("names", "patterns"):
                values = string_list(family.get(field), f"{location}.{field}", errors)
                if values != sorted(values):
                    errors.append(f"{location}.{field} must be sorted")
            producers = family.get("producers")
            if not isinstance(producers, list) or not producers:
                errors.append(f"{location}.producers must be a nonempty array")
                producers = []
            producer_order = []
            for producer_index, producer in enumerate(producers):
                producer_location = f"{location}.producers[{producer_index}]"
                if not isinstance(producer, dict) or set(producer) != {"path", "function"}:
                    errors.append(f"{producer_location} fields must be function, path")
                    continue
                path = producer.get("path")
                function = producer.get("function")
                if not isinstance(path, str) or re.fullmatch(r"[a-z0-9][a-z0-9-]*\.c", path) is None:
                    errors.append(f"{producer_location}.path is not a normalized C basename")
                    continue
                if not isinstance(function, str) or re.fullmatch(r"[a-z0-9][a-z0-9_]*", function) is None:
                    errors.append(f"{producer_location}.function is not normalized")
                    continue
                producer_order.append((path, function))
            if producer_order != sorted(producer_order) or len(producer_order) != len(set(producer_order)):
                errors.append(f"{location}.producers must be unique and sorted")
        if derived_order != sorted(derived_order) or len(derived_order) != len(set(derived_order)):
            errors.append("oracle.format_contexts.derived_families must be unique and sorted")
        if len(derived_families) != 10:
            errors.append("oracle.format_contexts.derived_families must contain 10 families")
        propagation = format_contexts.get("propagation")
        if not isinstance(propagation, list):
            errors.append("oracle.format_contexts.propagation must be an array")
            propagation = []
        propagation_order = []
        for index, site in enumerate(propagation):
            location = f"oracle.format_contexts.propagation[{index}]"
            if not isinstance(site, dict) or set(site) != {"path", "function", "callee"}:
                errors.append(f"{location} fields must be callee, function, path")
                continue
            path = site.get("path")
            function = site.get("function")
            callee = site.get("callee")
            if not all(isinstance(value, str) for value in (path, function, callee)):
                errors.append(f"{location} values must be strings")
                continue
            propagation_order.append((path, function, callee))
        if propagation_order != sorted(propagation_order) or len(propagation_order) != len(set(propagation_order)):
            errors.append("oracle.format_contexts.propagation must be unique and sorted")
        if len(propagation) != 5:
            errors.append("oracle.format_contexts.propagation must contain 5 sites")
        format_modifiers = string_list(
            oracle.get("format_modifiers"), "oracle.format_modifiers", errors, allow_empty=False
        )
        if format_modifiers != sorted(format_modifiers):
            errors.append("oracle.format_modifiers must be sorted")
        if len(format_modifiers) != 36:
            errors.append("oracle.format_modifiers must contain 36 tokens")
        key_bindings = oracle.get("key_bindings")
        if not isinstance(key_bindings, list):
            errors.append("compat/tmux-oracle.json key_bindings must be an array")
            key_bindings = []
        binding_order = []
        binding_keys = set()
        for index, binding in enumerate(key_bindings):
            location = f"oracle.key_bindings[{index}]"
            if not isinstance(binding, dict):
                errors.append(f"{location} must be an object")
                continue
            expected_fields = {"table", "key", "repeat", "command"}
            if set(binding) != expected_fields:
                errors.append(f"{location} fields must be {', '.join(sorted(expected_fields))}")
            table = binding.get("table")
            key = binding.get("key")
            command = binding.get("command")
            if not isinstance(table, str) or re.fullmatch(r"[a-z0-9][a-z0-9-]*", table) is None:
                errors.append(f"{location}.table is not normalized: {table!r}")
            if not isinstance(key, str) or not key or "\n" in key:
                errors.append(f"{location}.key must be one nonempty line")
            if not isinstance(command, str) or not command or "\n" in command:
                errors.append(f"{location}.command must be one nonempty line")
            if not isinstance(binding.get("repeat"), bool):
                errors.append(f"{location}.repeat must be a boolean")
            if (
                isinstance(table, str)
                and isinstance(key, str)
                and isinstance(command, str)
                and isinstance(binding.get("repeat"), bool)
            ):
                identity = (table, key)
                if identity in binding_keys:
                    errors.append(f"duplicate oracle key binding: {table} {key}")
                binding_keys.add(identity)
                binding_order.append(
                    (table, key, binding["repeat"], command)
                )
        if binding_order != sorted(binding_order):
            errors.append("oracle.key_bindings must be sorted")
    try:
        fetch_tmux = FETCH_TMUX.read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"cannot read {FETCH_TMUX.relative_to(ROOT)}: {error}")
    else:
        commit_match = re.search(r'^TMUX_COMMIT="([0-9a-f]{40})"$', fetch_tmux, re.MULTILINE)
        version_match = re.search(r'^TMUX_VERSION="([^"]+)"$', fetch_tmux, re.MULTILINE)
        if commit_match is None or commit_match.group(1) != pin:
            errors.append("compat/fetch-tmux.sh TMUX_COMMIT differs from the manifest pin")
        if (
            version_match is None
            or not isinstance(oracle, dict)
            or version_match.group(1) != oracle.get("version")
        ):
            errors.append("compat/fetch-tmux.sh TMUX_VERSION differs from the oracle version")
    for path in PIN_DOCS:
        try:
            contents = path.read_text(encoding="utf-8")
        except OSError as error:
            errors.append(f"cannot read {path.relative_to(ROOT)}: {error}")
            continue
        documented_pins = set(re.findall(r"\b[0-9a-f]{40}\b", contents))
        expected_documented_pins = {pin} if isinstance(pin, str) else set()
        if documented_pins != expected_documented_pins:
            errors.append(
                f"{path.relative_to(ROOT)} must name only the manifest pin; "
                f"got {', '.join(sorted(documented_pins)) or 'none'}"
            )
    differentials = manifest.get("known_differentials")
    if not isinstance(differentials, list):
        errors.append("compat/tmux-gaps.json known_differentials must be an array")
        differentials = []
    registered = {}
    for index, differential in enumerate(differentials):
        location = f"known_differentials[{index}]"
        if not isinstance(differential, dict):
            errors.append(f"{location} must be an object")
            continue
        if set(differential) != DIFFERENTIAL_FIELDS:
            errors.append(
                f"{location} fields must be {', '.join(sorted(DIFFERENTIAL_FIELDS))}; "
                f"got {', '.join(sorted(differential))}"
            )
        scenario = differential.get("scenario")
        if not isinstance(scenario, str) or scenario != Path(scenario).as_posix() or not scenario.startswith("known/"):
            errors.append(f"{location}.scenario must be a normalized known/*.txt path")
            continue
        if not scenario.endswith(".txt"):
            errors.append(f"{location}.scenario must end in .txt")
        if scenario in registered:
            errors.append(f"duplicate known differential scenario: {scenario}")
        registered[scenario] = differential
        gap_id = differential.get("gap")
        if not isinstance(gap_id, str):
            errors.append(f"{location}.gap must be a gap id string")
        elif gap_id not in ids:
            errors.append(f"{location}.gap references unknown gap: {gap_id}")
        elif gap_by_id[gap_id].get("status") != "accepted":
            errors.append(f"{location}.gap must reference an accepted divergence: {gap_id}")
        for field in ("topo", "geo", "fmt", "out", "warn"):
            value = differential.get(field)
            if not isinstance(value, int) or isinstance(value, bool) or value < 0:
                errors.append(f"{location}.{field} must be a nonnegative integer")
    scenarios = list(registered)
    if scenarios != sorted(scenarios):
        errors.append("known_differentials must be sorted by scenario")
    existing = {
        path.relative_to(SCENARIOS).as_posix()
        for path in (SCENARIOS / "known").glob("*.txt")
        if path.is_file()
    }
    missing = sorted(existing - set(registered))
    stale = sorted(set(registered) - existing)
    if missing:
        errors.append(f"known scenarios missing from manifest: {', '.join(missing)}")
    if stale:
        errors.append(f"manifest references missing known scenarios: {', '.join(stale)}")
    for scenario in sorted(existing & set(registered)):
        path = SCENARIOS / scenario
        headers = re.findall(r"(?m)^# gap: ([a-z0-9][a-z0-9._-]*)\s*$", path.read_text(encoding="utf-8"))
        if len(headers) != 1:
            errors.append(f"{path.relative_to(ROOT)} must contain exactly one # gap: <id> header")
        elif headers[0] != registered[scenario].get("gap"):
            errors.append(
                f"{path.relative_to(ROOT)} names {headers[0]}, manifest names "
                f"{registered[scenario].get('gap')}"
            )
    if include_report and not errors:
        expected = render_report(manifest, oracle)
        if not REPORT.exists():
            errors.append(f"missing {REPORT.relative_to(ROOT)}; run tmux-tracker.py write-report")
        elif REPORT.read_text(encoding="utf-8") != expected:
            errors.append(f"{REPORT.relative_to(ROOT)} is stale; run tmux-tracker.py write-report")
    return errors


def md_cell(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def counts(values, order):
    counted = Counter(values)
    return ", ".join(f"{name}: {counted[name]}" for name in order if counted[name])


def render_report(manifest, oracle):
    gaps = manifest["gaps"]
    priorities = ["now", "next", "later", "none"]
    ease_order = {name: index for index, name in enumerate(["easy", "medium", "hard", "hardest", "none"])}
    item_counts = Counter(item.split(":", 1)[0] for gap in gaps for item in gap["items"])
    item_order = [
        "command",
        "flag",
        "flag-arity",
        "positional-min",
        "positional-max",
        "args-parse",
        "extension-flag",
        "native-command",
        "native-alias",
        "option",
        "format",
        "context-format",
        "native-context-format",
        "hook",
        "key",
        "binding",
        "native-key",
        "semantic",
        "presentation",
        "protocol",
    ]
    command_items = item_counts["command"]
    flag_arities = Counter(
        arity for command in oracle["commands"] for arity in command["flags"].values()
    )
    literal_context_count = sum(
        len(scope["names"]) for scope in oracle["format_contexts"]["literal_scopes"]
    )
    derived_context_count = len(oracle["format_contexts"]["derived_families"])
    lines = [
        "---",
        "type: Reference",
        "title: tmux compatibility gap report",
        'description: "Live TODO and status report for tmux compatibility gaps, decisions, evidence, and acceptance gates."',
        "resource: compat/tmux-gaps.json",
        "tags: [tmux, compatibility, gaps, tracker]",
        f"timestamp: {manifest['updated_on']}T00:00:00-03:00",
        "---",
        "",
        "# Overview",
        "",
        "> `compat/tmux-tracker.py write-report` generates this file. Edit the registry instead.",
        "",
        "`compat/tmux-gaps.json` owns the backlog. The compatibility gate checks IDs, decisions,",
        "dependencies, evidence paths, known scenarios, and the source-backed inventories described",
        "below.",
        "",
        f"Pinned tmux commit: `{manifest['pin']}`.",
        "",
        f"Tracked gap groups: **{len(gaps)}**. Classified items: **{sum(item_counts.values())}**.",
        "",
        f"- Status: {counts((gap['status'] for gap in gaps), ['open', 'blocked', 'accepted'])}.",
        f"- Decision: {counts((gap['decision'] for gap in gaps), ['adopt', 'native', 'park', 'never'])}.",
        f"- Priority: {counts((gap['priority'] for gap in gaps), priorities)}.",
        f"- Closed history entries: {len(manifest['closed'])}.",
        f"- Surface: {counts((item.split(':', 1)[0] for gap in gaps for item in gap['items']), item_order)}.",
        "",
        "## Measured surface",
        "",
        f"The pinned oracle contains {len(oracle['commands'])} commands, "
        f"{sum(len(command['aliases']) for command in oracle['commands'])} aliases, "
        f"{sum(flag_arities.values())} command-flag shapes "
        f"({flag_arities['none']} valueless, {flag_arities['required']} required-value, "
        f"{flag_arities['optional']} optional-value), positional minimum and maximum bounds, "
        f"{len(oracle['options'])} options, {len(oracle['formats'])} global formats, "
        f"{literal_context_count} scoped literal context pairs across "
        f"{len(oracle['format_contexts']['literal_scopes'])} source producers, "
        f"{derived_context_count} derived context families, "
        f"{len(oracle['format_modifiers'])} format modifiers, "
        f"{len(oracle['hooks'])} hooks, and {len(oracle['key_bindings'])} default bindings across "
        f"{len({binding['table'] for binding in oracle['key_bindings']})} tables. zz has catalog "
        f"entries for {len(oracle['commands']) - command_items} of those commands. The registry "
        f"classifies {item_counts['flag']} catalogued-unsupported upstream flag pairs, "
        f"{item_counts['flag-arity']} implemented flag-arity mismatches, "
        f"{item_counts['positional-min']} positional-minimum mismatches, "
        f"{item_counts['positional-max']} positional-maximum mismatches, "
        f"{len(oracle['args_parse'])} callback-bearing commands across "
        f"{len(set(oracle['args_parse'].values()))} effective `args_parse` rules, "
        f"{item_counts['args-parse']} implemented commands without verified callback behavior, "
        f"{item_counts['extension-flag']} zz-only flags on tmux command names, "
        f"{item_counts['native-command']} native command names, "
        f"{item_counts['option']} options absent from `BEHAVES`, "
        f"{item_counts['format']} known limited formats, "
        f"{item_counts['context-format']} scoped context-format gaps, "
        f"{item_counts['native-context-format']} accepted-native context-format names, "
        f"{item_counts['hook']} currently documented hook-producer gaps, "
        f"{item_counts['key']} omitted default keys, "
        f"{item_counts['binding']} divergent shared default bindings, "
        f"{item_counts['native-key']} zz-only default keys.",
        "",
        "## Enforcement boundary",
        "",
        "The gate reconciles command names, aliases, flag arities, positional bounds, custom",
        "`args_parse` rules, option names, global formats, scoped and derived context producers,",
        "format modifiers, hook names,",
        "and default key presence against the clean pinned tmux source and binary. It also reconciles",
        "options absent from `BEHAVES`, constant-backed formats against the live registry, omitted",
        "and zz-only default keys against zz's key tables, rendered commands plus repeat bits for",
        "shared default bindings, the native roster against catalog minus oracle, every pinned",
        "canonical prefix against the resolver, and known scenarios against exact tuples.",
        "",
        "These structural checks cannot prove that runtime parsing applies each inventoried `args_parse`",
        "rule, context-format value correctness, nonconstant format correctness, or whether a hook fires,",
        "or that a structurally matching binding behaves identically at runtime. Differential scenarios,",
        "attached-client fixtures, unit tests, and manual GUI checks supply that behavioral evidence. The",
        "tracker keeps the remaining semantic discovery work explicit instead of treating matching",
        "structure as proof.",
        "",
    ]
    for priority in priorities:
        selected = [gap for gap in gaps if gap["priority"] == priority]
        if not selected:
            continue
        selected.sort(key=lambda gap: (ease_order[gap["ease"]], gap["id"]))
        lines.extend(
            [
                f"## {priority.title()}",
                "",
                "| ID | Gap | Decision | Status | Ease | Owner | Impact | Depends on |",
                "| --- | --- | --- | --- | --- | --- | --- | --- |",
            ]
        )
        for gap in selected:
            lines.append(
                "| "
                + " | ".join(
                    md_cell(value)
                    for value in (
                        f"`{gap['id']}`",
                        gap["title"],
                        gap["decision"],
                        gap["status"],
                        gap["ease"],
                        gap["owner"],
                        ", ".join(gap["impact"]),
                        ", ".join(gap["depends_on"]) or "none",
                    )
                )
                + " |"
            )
        lines.append("")
    lines.extend(["## Gap details", ""])
    for gap in sorted(gaps, key=lambda gap: gap["id"]):
        lines.extend(
            [
                f"### `{gap['id']}`: {gap['title']}",
                "",
                gap["reason"],
                "",
                f"- Decision: `{gap['decision']}`",
                f"- Status: `{gap['status']}`",
                f"- Priority and ease: `{gap['priority']}` / `{gap['ease']}`",
                f"- Owner: `{gap['owner']}`",
                f"- User impact: {', '.join(gap['impact'])}",
                f"- Items: {', '.join(f'`{item}`' for item in gap['items'])}",
                f"- Depends on: {', '.join(f'`{item}`' for item in gap['depends_on']) or 'none'}",
                "- Evidence:",
            ]
        )
        lines.extend(f"  - `{item}`" for item in gap["evidence"])
        if not gap["evidence"]:
            lines.append("  - none")
        lines.append("- Acceptance:")
        lines.extend(f"  - `{item}`" for item in gap["acceptance"])
        if not gap["acceptance"]:
            lines.append("  - none")
        lines.append("")
    lines.extend(
        [
            "## Known differential scenarios",
            "",
            "| Scenario | Gap | TOPO | GEO | FMT | OUT | WARN |",
            "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for differential in sorted(manifest["known_differentials"], key=lambda item: item["scenario"]):
        lines.append(
            "| "
            + " | ".join(
                md_cell(differential[field])
                for field in ("scenario", "gap", "topo", "geo", "fmt", "out", "warn")
            )
            + " |"
        )
    lines.append("")
    lines.extend(["## Closed history", ""])
    if manifest["closed"]:
        lines.extend(
            [
                "| ID | Closed | Resolution | Evidence |",
                "| --- | --- | --- | --- |",
            ]
        )
        for entry in manifest["closed"]:
            lines.append(
                "| "
                + " | ".join(
                    md_cell(value)
                    for value in (
                        f"`{entry['id']}`",
                        entry["closed_on"],
                        entry["resolution"],
                        ", ".join(f"`{item}`" for item in entry["evidence"]),
                    )
                )
                + " |"
            )
    else:
        lines.append("No live tracker gap has closed since the canonical ledger began.")
    lines.append("")
    return "\n".join(lines)


def normalize_scenario(value, differentials):
    scenario = value.replace("\\", "/")
    prefix = "compat/scenarios/"
    if scenario.startswith(prefix):
        scenario = scenario[len(prefix) :]
    candidates = [scenario]
    if not scenario.endswith(".txt"):
        candidates.append(f"{scenario}.txt")
    if "/" not in scenario:
        candidates.extend([f"known/{candidate}" for candidate in list(candidates)])
    matches = [item for item in differentials if item.get("scenario") in candidates]
    if len(matches) != 1:
        return None
    return matches[0]


def print_errors(errors):
    for error in errors:
        print(f"error: {error}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description="Validate and render the tmux compatibility backlog")
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("check")
    subcommands.add_parser("write-report")
    known = subcommands.add_parser("known-tuple")
    known.add_argument("scenario")
    arguments = parser.parse_args()
    errors = []
    manifest = load_json(MANIFEST, errors)
    oracle = load_json(ORACLE, errors)
    if errors:
        print_errors(errors)
        return 1
    errors = validate_manifest(manifest, oracle, include_report=arguments.command == "check")
    if errors:
        print_errors(errors)
        return 1
    if arguments.command == "known-tuple":
        differentials = manifest.get("known_differentials", []) if isinstance(manifest, dict) else []
        differential = normalize_scenario(arguments.scenario, differentials)
        if differential is None:
            print(f"error: no known differential for {arguments.scenario}", file=sys.stderr)
            return 1
        print(" ".join(str(differential[field]) for field in ("topo", "geo", "fmt", "out", "warn")))
        return 0
    if arguments.command == "write-report":
        REPORT.write_text(render_report(manifest, oracle), encoding="utf-8")
        print(f"wrote {REPORT.relative_to(ROOT)}")
    else:
        print(f"{MANIFEST.relative_to(ROOT)} is valid and {REPORT.relative_to(ROOT)} is current")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
