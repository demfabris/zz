#!/usr/bin/env python3

import argparse
import ast
import json
import os
import re
import secrets
import subprocess
import sys
from pathlib import Path


SCHEMA = 5
PIN = "d77c9dc6aa021e4bc61f0da128c591af695e6466"
VERSION = "tmux next-3.8"
ROOT = Path(__file__).resolve().parent.parent
ORACLE = ROOT / "compat" / "tmux-oracle.json"
MANIFEST = ROOT / "compat" / "tmux-gaps.json"
FETCH_TMUX = ROOT / "compat" / "fetch-tmux.sh"
DEFAULT_TMUX = ROOT / "compat" / ".cache" / "tmux-src" / "tmux"

FORMAT_INSERTION_CALLS = frozenset(
    {"format_add", "format_add_cb", "format_add_tv", "cmdq_add_format", "format_merge"}
)
FORMAT_DERIVED_FAMILIES = {
    "current-file": {"names": ["current_file"], "patterns": []},
    "hook": {"names": ["hook"], "patterns": []},
    "hook-argument": {"names": [], "patterns": ["hook_argument_N"]},
    "hook-arguments": {"names": ["hook_arguments"], "patterns": []},
    "hook-flag": {"names": [], "patterns": ["hook_flag_X"]},
    "hook-flag-value": {"names": [], "patterns": ["hook_flag_X_N"]},
    "run-shell-position": {"names": [], "patterns": ["N"]},
    "window-neighbour-active": {
        "names": ["next_window_active", "prev_window_active"],
        "patterns": [],
    },
    "window-neighbour-index": {
        "names": ["next_window_index", "prev_window_index"],
        "patterns": [],
    },
    "window-neighbour-user-option": {
        "names": [],
        "patterns": ["next_@*", "prev_@*"],
    },
}
FORMAT_DERIVED_PRODUCERS = {
    "current-file": {("cfg.c", "load_cfg"), ("cfg.c", "load_cfg_from_buffer")},
    "hook": {("cmd-queue.c", "cmdq_insert_hook")},
    "hook-argument": {("cmd-queue.c", "cmdq_insert_hook")},
    "hook-arguments": {("cmd-queue.c", "cmdq_insert_hook")},
    "hook-flag": {("cmd-queue.c", "cmdq_insert_hook")},
    "hook-flag-value": {("cmd-queue.c", "cmdq_insert_hook")},
    "run-shell-position": {("cmd-run-shell.c", "cmd_run_shell_exec")},
    "window-neighbour-active": {("format.c", "format_add_window_neighbour")},
    "window-neighbour-index": {("format.c", "format_add_window_neighbour")},
    "window-neighbour-user-option": {("format.c", "format_add_window_neighbour")},
}
FORMAT_DERIVED_CALL_COUNTS = {
    "current-file": 2,
    "hook": 1,
    "hook-argument": 1,
    "hook-arguments": 1,
    "hook-flag": 2,
    "hook-flag-value": 1,
    "run-shell-position": 1,
    "window-neighbour-active": 1,
    "window-neighbour-index": 1,
    "window-neighbour-user-option": 1,
}


def fail(message):
    raise RuntimeError(message)


def run(arguments, env):
    result = subprocess.run(
        [str(argument) for argument in arguments],
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}"
        fail(f"{' '.join(str(argument) for argument in arguments)}: {detail}")
    return result.stdout


def manifest_pin():
    if not MANIFEST.exists():
        fail(f"missing {MANIFEST.relative_to(ROOT)}")
    try:
        data = json.loads(MANIFEST.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {MANIFEST.relative_to(ROOT)}: {error}")
    pin = data.get("pin")
    if pin != PIN:
        fail(f"{MANIFEST.relative_to(ROOT)} pin must be {PIN}, got {pin!r}")
    return pin


def checksum(path, env):
    fields = run(["cksum", path], env).split()
    if len(fields) < 2 or not all(field.isdecimal() for field in fields[:2]):
        fail(f"cannot parse cksum output for {path}")
    return " ".join(fields[:2])


def verify_build_stamp(path, source, env, expected_pin, version):
    stamp_path = source.parent / "tmux-build.stamp"
    try:
        lines = stamp_path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        fail(f"cannot read tmux build provenance stamp {stamp_path}: {error}")
    stamp = {}
    for line in lines:
        key, separator, value = line.partition("=")
        if not separator or not key or key in stamp:
            fail(f"malformed tmux build provenance stamp {stamp_path}")
        stamp[key] = value
    expected = {
        "commit": expected_pin,
        "version": version,
        "script-cksum": checksum(FETCH_TMUX, env),
        "binary-cksum": checksum(path, env),
    }
    if stamp != expected:
        fail(f"tmux build provenance stamp does not match source, recipe, and binary: {stamp_path}")


def verify_tmux(path, expected_pin):
    if not path.is_file() or not os.access(path, os.X_OK):
        fail(f"tmux is not executable: {path}")
    env = os.environ.copy()
    env.pop("TMUX", None)
    env.pop("TMUX_PANE", None)
    env["LC_ALL"] = "C"
    version = run([path, "-V"], env).strip()
    if version != VERSION:
        fail(f"tmux must report {VERSION!r}, got {version!r}")
    source = path.resolve().parent
    if not (source / ".git").exists() or not any(source.glob("cmd-*.c")):
        fail("tmux binary must live in the clean pinned source checkout")
    commit = run(["git", "-C", source, "rev-parse", "HEAD"], env).strip()
    if commit != expected_pin:
        fail(f"tmux source must be at {expected_pin}, got {commit}")
    dirty = run(
        ["git", "-C", source, "status", "--porcelain", "--untracked-files=all"], env
    ).strip()
    if dirty:
        fail(f"tmux source checkout is dirty:\n{dirty}")
    verify_build_stamp(path, source, env, expected_pin, version)
    return env, version, source


def flag_shapes(specification):
    flags = {}
    index = 0
    while index < len(specification):
        character = specification[index]
        if not character.isalnum() or len(character) != 1:
            fail(f"invalid command flag template character: {character!r}")
        index += 1
        colons = 0
        while index < len(specification) and specification[index] == ":":
            colons += 1
            index += 1
        if colons > 2:
            fail(f"invalid command flag template near {character!r}: {specification!r}")
        arity = ("none", "required", "optional")[colons]
        name = f"-{character}"
        if name in flags:
            fail(f"duplicate command flag in template: {specification!r}")
        flags[name] = arity
    return dict(sorted(flags.items()))


def source_commands(source):
    commands = {}
    entry = re.compile(r"const struct cmd_entry\s+\w+\s*=\s*\{(.*?)^\};", re.MULTILINE | re.DOTALL)
    for path in sorted(source.glob("cmd-*.c")):
        contents = path.read_text(encoding="utf-8")
        for match in entry.finditer(contents):
            body = match.group(1)
            name_match = re.search(r'^\s*\.name\s*=\s*"([^"]+)"\s*,', body, re.MULTILINE)
            args_match = re.search(
                r'^\s*\.args\s*=\s*\{\s*("(?:[^"\\]|\\.)*")\s*,\s*(-?[0-9]+)\s*,\s*(-?[0-9]+)\s*,\s*([A-Za-z_][A-Za-z0-9_]*|NULL)\s*\}',
                body,
                re.MULTILINE,
            )
            if name_match is None or args_match is None:
                fail(f"cannot parse command entry in {path}")
            name = name_match.group(1)
            specification = ast.literal_eval(args_match.group(1))
            minimum = int(args_match.group(2))
            maximum = int(args_match.group(3))
            if minimum < 0 or (maximum != -1 and maximum < minimum):
                fail(f"invalid positional arity for {name} in {path}")
            if name in commands:
                fail(f"duplicate command entry in pinned source: {name}")
            commands[name] = {
                "flags": flag_shapes(specification),
                "min_args": minimum,
                "max_args": None if maximum == -1 else maximum,
                "args_parse_callback": (
                    None if args_match.group(4) == "NULL" else args_match.group(4)
                ),
            }
    return commands


def function_body(contents, name, path):
    match = re.search(
        rf"static\s+enum\s+args_parse_type\s+{re.escape(name)}\s*\([^;]*?\)\s*\{{",
        contents,
        re.DOTALL,
    )
    if match is None:
        return None
    start = match.end()
    depth = 1
    index = start
    while index < len(contents) and depth:
        if contents[index] == "{":
            depth += 1
        elif contents[index] == "}":
            depth -= 1
        index += 1
    if depth:
        fail(f"unterminated args_parse callback {name} in {path}")
    return contents[start : index - 1]


def classify_args_parse_body(body, name):
    normalized = re.sub(r"\s+", "", body)
    if normalized == "return(ARGS_PARSE_COMMANDS_OR_STRING);":
        return "commands-or-string"
    if normalized == (
        "u_inti=0;enumargs_parse_typetype=ARGS_PARSE_STRING;for(;;){"
        "type=ARGS_PARSE_STRING;if(i==idx)break;if(*args_string(args,i++)=='\\0')continue;"
        "type=ARGS_PARSE_STRING;if(i++==idx)break;type=ARGS_PARSE_COMMANDS_OR_STRING;"
        "if(i++==idx)break;}return(type);"
    ):
        return "display-menu-items"
    if normalized == (
        "if(idx==1||idx==2)return(ARGS_PARSE_COMMANDS_OR_STRING);"
        "return(ARGS_PARSE_STRING);"
    ):
        return "if-shell-branches"
    if normalized == (
        "if(args_has(args,'C'))return(ARGS_PARSE_COMMANDS_OR_STRING);"
        "return(ARGS_PARSE_STRING);"
    ):
        return "run-shell-command-flag"
    if normalized == (
        "if(args_has(args,'B'))return(ARGS_PARSE_COMMANDS_OR_STRING);"
        "if(idx==1)return(ARGS_PARSE_COMMANDS_OR_STRING);return(ARGS_PARSE_STRING);"
    ):
        return "set-option-callback"
    fail(f"unclassified args_parse callback body: {name}")


def source_args_parse(source, commands):
    callbacks = {
        command["args_parse_callback"]
        for command in commands.values()
        if command["args_parse_callback"] is not None
    }
    if len(callbacks) != 9:
        fail(f"expected 9 args_parse callbacks, got {len(callbacks)}")
    callback_rules = {}
    for path in sorted(source.glob("cmd-*.c")):
        contents = path.read_text(encoding="utf-8")
        for callback in sorted(callbacks):
            body = function_body(contents, callback, path)
            if body is None:
                continue
            if callback in callback_rules:
                fail(f"duplicate args_parse callback definition: {callback}")
            callback_rules[callback] = classify_args_parse_body(body, callback)
    missing = sorted(callbacks - callback_rules.keys())
    if missing:
        fail(f"missing args_parse callback definitions: {', '.join(missing)}")
    effective = {}
    for name, command in commands.items():
        callback = command["args_parse_callback"]
        if callback is None:
            continue
        rule = callback_rules[callback]
        if rule == "set-option-callback":
            if name == "set-hook":
                rule = "set-hook-monitor-or-value"
            elif name in {"set-option", "set-window-option"}:
                rule = "set-option-value"
            else:
                fail(f"unexpected command using set-option args_parse callback: {name}")
        effective[name] = rule
    if len(effective) != 14:
        fail(f"expected 14 commands with args_parse callbacks, got {len(effective)}")
    if len(set(effective.values())) != 6:
        fail(f"expected 6 effective args_parse rules, got {len(set(effective.values()))}")
    return dict(sorted(effective.items()))


def source_formats(source):
    path = source / "format.c"
    contents = path.read_text(encoding="utf-8")
    match = re.search(
        r"static const struct format_table_entry format_table\[\]\s*=\s*\{(.*?)^\};",
        contents,
        re.MULTILINE | re.DOTALL,
    )
    if match is None:
        fail(f"cannot parse format table in {path}")
    names = re.findall(r'^\s*\{\s*"([a-z0-9_]+)"\s*,', match.group(1), re.MULTILINE)
    if not names or len(names) != len(set(names)):
        fail(f"pinned format table is empty or contains duplicate names in {path}")
    return sorted(names)


def mask_c(contents):
    masked = list(contents)
    index = 0
    while index < len(contents):
        character = contents[index]
        following = contents[index + 1] if index + 1 < len(contents) else ""
        if character == "/" and following == "/":
            end = contents.find("\n", index + 2)
            if end == -1:
                end = len(contents)
            for offset in range(index, end):
                masked[offset] = " "
            index = end
            continue
        if character == "/" and following == "*":
            end = contents.find("*/", index + 2)
            if end == -1:
                fail("unterminated C block comment")
            for offset in range(index, end + 2):
                if contents[offset] != "\n":
                    masked[offset] = " "
            index = end + 2
            continue
        if character in {'"', "'"}:
            quote = character
            end = index + 1
            while end < len(contents):
                if contents[end] == "\\":
                    end += 2
                    continue
                if contents[end] == quote:
                    end += 1
                    break
                end += 1
            else:
                fail("unterminated C string or character literal")
            for offset in range(index, end):
                if contents[offset] != "\n":
                    masked[offset] = " "
            index = end
            continue
        index += 1
    return "".join(masked)


def source_function_bodies(contents, path):
    masked = mask_c(contents)
    pattern = re.compile(
        r"^([A-Za-z_][A-Za-z0-9_]*)\([^;{}]*?\)\n\{", re.MULTILINE
    )
    functions = []
    for match in pattern.finditer(masked):
        opening = match.end() - 1
        depth = 1
        closing = opening + 1
        while closing < len(masked) and depth:
            if masked[closing] == "{":
                depth += 1
            elif masked[closing] == "}":
                depth -= 1
            closing += 1
        if depth:
            fail(f"unterminated function {match.group(1)} in {path}")
        functions.append(
            {
                "name": match.group(1),
                "signature_start": match.start(),
                "body_start": opening + 1,
                "body_end": closing - 1,
            }
        )
    return functions


def skip_c_quote(contents, index):
    quote = contents[index]
    index += 1
    while index < len(contents):
        if contents[index] == "\\":
            index += 2
            continue
        if contents[index] == quote:
            return index + 1
        index += 1
    fail("unterminated C string or character literal")


def skip_c_trivia(contents, index):
    while index < len(contents):
        if contents[index].isspace():
            index += 1
            continue
        if contents.startswith("//", index):
            end = contents.find("\n", index + 2)
            return len(contents) if end == -1 else skip_c_trivia(contents, end)
        if contents.startswith("/*", index):
            end = contents.find("*/", index + 2)
            if end == -1:
                fail("unterminated C block comment")
            index = end + 2
            continue
        break
    return index


def c_call_arguments(contents, opening):
    arguments = []
    start = opening + 1
    index = start
    parentheses = 1
    brackets = 0
    braces = 0
    while index < len(contents):
        if contents.startswith("//", index):
            end = contents.find("\n", index + 2)
            index = len(contents) if end == -1 else end
            continue
        if contents.startswith("/*", index):
            end = contents.find("*/", index + 2)
            if end == -1:
                fail("unterminated C block comment")
            index = end + 2
            continue
        character = contents[index]
        if character in {'"', "'"}:
            index = skip_c_quote(contents, index)
            continue
        if character == "(":
            parentheses += 1
        elif character == ")":
            parentheses -= 1
            if parentheses == 0:
                argument = contents[start:index].strip()
                if argument or arguments:
                    arguments.append(argument)
                return arguments, index + 1
        elif character == "[":
            brackets += 1
        elif character == "]":
            brackets -= 1
        elif character == "{":
            braces += 1
        elif character == "}":
            braces -= 1
        elif character == "," and parentheses == 1 and brackets == 0 and braces == 0:
            arguments.append(contents[start:index].strip())
            start = index + 1
        if parentheses < 1 or brackets < 0 or braces < 0:
            fail("malformed C call expression")
        index += 1
    fail("unterminated C call expression")


def c_calls(contents, names):
    calls = []
    index = 0
    while index < len(contents):
        if contents.startswith("//", index):
            end = contents.find("\n", index + 2)
            index = len(contents) if end == -1 else end
            continue
        if contents.startswith("/*", index):
            end = contents.find("*/", index + 2)
            if end == -1:
                fail("unterminated C block comment")
            index = end + 2
            continue
        if contents[index] in {'"', "'"}:
            index = skip_c_quote(contents, index)
            continue
        if contents[index].isalpha() or contents[index] == "_":
            end = index + 1
            while end < len(contents) and (
                contents[end].isalnum() or contents[end] == "_"
            ):
                end += 1
            name = contents[index:end]
            opening = skip_c_trivia(contents, end)
            if name in names and opening < len(contents) and contents[opening] == "(":
                arguments, call_end = c_call_arguments(contents, opening)
                calls.append((index, name, arguments))
                index = call_end
                continue
            index = end
            continue
        index += 1
    return calls


def c_string_literal(expression):
    match = re.fullmatch(r'\s*("(?:[^"\\]|\\.)*")\s*', expression, re.DOTALL)
    if match is None:
        return None
    value = ast.literal_eval(match.group(1))
    if not isinstance(value, str):
        fail(f"invalid C string literal: {expression!r}")
    return value


def normalized_c(expression):
    return re.sub(r"\s+", "", expression)


def generated_key_template(generators, insertion, key):
    template = None
    for offset, name, arguments in generators:
        if offset >= insertion:
            break
        if name == "xsnprintf" and len(arguments) >= 3:
            destination = normalized_c(arguments[0])
            candidate = arguments[2]
        elif name == "xasprintf" and len(arguments) >= 2:
            destination = normalized_c(arguments[0]).removeprefix("&")
            candidate = arguments[1]
        else:
            continue
        if destination == key:
            template = c_string_literal(candidate)
    if template is None:
        fail(f"no source template found for dynamic format key {key!r}")
    return template


def source_format_contexts(source):
    literal_scopes = {}
    derived_producers = {family: set() for family in FORMAT_DERIVED_FAMILIES}
    derived_call_counts = {family: 0 for family in FORMAT_DERIVED_FAMILIES}
    propagation_calls = []
    literal_wrappers = {
        ("cfg.c", "load_cfg", "current_file"): "current-file",
        ("cfg.c", "load_cfg_from_buffer", "current_file"): "current-file",
        ("cmd-queue.c", "cmdq_insert_hook", "hook"): "hook",
        ("cmd-queue.c", "cmdq_insert_hook", "hook_arguments"): "hook-arguments",
    }
    pass_through = {
        ("cmd-queue.c", "cmdq_add_format", "format_add", "key"),
        ("format.c", "format_merge", "format_add", "fe->key"),
    }
    generated_families = {
        (
            "cmd-queue.c",
            "cmdq_insert_hook",
            "cmdq_add_format",
            "tmp",
            "hook_argument_%d",
        ): "hook-argument",
        (
            "cmd-queue.c",
            "cmdq_insert_hook",
            "cmdq_add_format",
            "tmp",
            "hook_flag_%c",
        ): "hook-flag",
        (
            "cmd-queue.c",
            "cmdq_insert_hook",
            "cmdq_add_format",
            "tmp",
            "hook_flag_%c_%d",
        ): "hook-flag-value",
        (
            "cmd-run-shell.c",
            "cmd_run_shell_exec",
            "format_add",
            "key",
            "%u",
        ): "run-shell-position",
        (
            "format.c",
            "format_add_window_neighbour",
            "format_add",
            "key",
            "%s_window_index",
        ): "window-neighbour-index",
        (
            "format.c",
            "format_add_window_neighbour",
            "format_add",
            "key",
            "%s_window_active",
        ): "window-neighbour-active",
        (
            "format.c",
            "format_add_window_neighbour",
            "format_add",
            "prefixed",
            "%s_%s",
        ): "window-neighbour-user-option",
    }
    expected_propagation = {
        ("cmd-queue.c", "cmdq_add_format", "format_add"),
        ("cmd-queue.c", "cmdq_add_formats", "format_merge"),
        ("cmd-queue.c", "cmdq_merge_formats", "format_merge"),
        ("format.c", "format_merge", "format_add"),
        ("notify.c", "notify_parse_hook", "format_merge"),
    }

    for path in sorted(source.glob("*.c")):
        contents = path.read_text(encoding="utf-8")
        if not re.search(
            r"\b(?:format_add(?:_cb|_tv)?|cmdq_add_format|format_merge)\s*\(",
            contents,
        ):
            continue
        functions = source_function_bodies(contents, path)
        for function in functions:
            body = contents[function["body_start"] : function["body_end"]]
            generators = c_calls(body, {"xasprintf", "xsnprintf"})
            for offset, callee, arguments in c_calls(body, FORMAT_INSERTION_CALLS):
                location = (path.name, function["name"])
                if callee == "format_merge":
                    propagation_calls.append((*location, callee))
                    continue
                if len(arguments) < 2:
                    fail(f"malformed {callee} call in {path.name}:{function['name']}")
                key_expression = normalized_c(arguments[1])
                literal = c_string_literal(arguments[1])
                if callee.startswith("format_add") and literal is not None:
                    if re.fullmatch(r"[a-z0-9_]+", literal) is None:
                        fail(
                            f"invalid literal format key {literal!r} in "
                            f"{path.name}:{function['name']}"
                        )
                    literal_scopes.setdefault(location, set()).add(literal)
                    continue
                if callee == "cmdq_add_format" and literal is not None:
                    family = literal_wrappers.get((*location, literal))
                    if family is None:
                        fail(
                            f"unclassified cmdq_add_format producer "
                            f"{path.name}:{function['name']}:{literal}"
                        )
                elif (*location, callee, key_expression) in pass_through:
                    propagation_calls.append((*location, callee))
                    continue
                else:
                    template = generated_key_template(generators, offset, key_expression)
                    family = generated_families.get(
                        (*location, callee, key_expression, template)
                    )
                    if family is None:
                        fail(
                            f"unclassified nonliteral format insertion in "
                            f"{path.name}:{function['name']}: "
                            f"{callee} key {key_expression!r} from {template!r}"
                        )
                derived_producers[family].add(location)
                derived_call_counts[family] += 1

        all_calls = c_calls(contents, FORMAT_INSERTION_CALLS)
        for offset, callee, _ in all_calls:
            if any(
                function["body_start"] <= offset < function["body_end"]
                for function in functions
            ):
                continue
            if any(
                function["signature_start"] <= offset < function["body_start"]
                and function["name"] == callee
                for function in functions
            ):
                continue
            fail(f"format insertion outside a function body in {path.name}: {callee}")

    literal_scope_count = len(literal_scopes)
    literal_pair_count = sum(len(names) for names in literal_scopes.values())
    literal_names = set().union(*literal_scopes.values())
    if (literal_scope_count, literal_pair_count, len(literal_names)) != (31, 153, 108):
        fail(
            "expected 31 literal format scopes, 153 scoped pairs, and 108 unique "
            f"names, got {literal_scope_count}, {literal_pair_count}, {len(literal_names)}"
        )
    if derived_producers != FORMAT_DERIVED_PRODUCERS:
        fail(
            "derived format producers changed: "
            f"expected {FORMAT_DERIVED_PRODUCERS!r}, got {derived_producers!r}"
        )
    if derived_call_counts != FORMAT_DERIVED_CALL_COUNTS:
        fail(
            "derived format call counts changed: "
            f"expected {FORMAT_DERIVED_CALL_COUNTS!r}, got {derived_call_counts!r}"
        )
    if (
        len(propagation_calls) != len(expected_propagation)
        or set(propagation_calls) != expected_propagation
    ):
        fail(
            "format propagation sites changed: "
            f"expected {expected_propagation!r}, got {propagation_calls!r}"
        )

    return {
        "literal_scopes": [
            {"path": path, "function": function, "names": sorted(names)}
            for (path, function), names in sorted(literal_scopes.items())
        ],
        "derived_families": [
            {
                "family": family,
                "names": FORMAT_DERIVED_FAMILIES[family]["names"],
                "patterns": FORMAT_DERIVED_FAMILIES[family]["patterns"],
                "producers": [
                    {"path": path, "function": function}
                    for path, function in sorted(derived_producers[family])
                ],
            }
            for family in sorted(FORMAT_DERIVED_FAMILIES)
        ],
        "propagation": [
            {"path": path, "function": function, "callee": callee}
            for path, function, callee in sorted(expected_propagation)
        ],
    }


def source_format_modifiers(source):
    path = source / "format.c"
    contents = path.read_text(encoding="utf-8")
    functions = {
        function["name"]: function for function in source_function_bodies(contents, path)
    }
    function = functions.get("format_build_modifiers")
    if function is None:
        fail(f"cannot find format_build_modifiers in {path}")
    body = contents[function["body_start"] : function["body_end"]]
    single_sources = []
    double_sources = []
    for _, name, arguments in c_calls(body, {"memcmp", "strchr"}):
        if name == "strchr" and len(arguments) == 2:
            if normalized_c(arguments[1]) != "cp[0]":
                fail(f"unclassified modifier strchr in {path}: {arguments!r}")
            source_chars = c_string_literal(arguments[0])
            if source_chars is None:
                fail(f"nonliteral modifier strchr in {path}: {arguments!r}")
            single_sources.append(source_chars)
        elif name == "memcmp" and len(arguments) == 3:
            if normalized_c(arguments[1]) != "cp" or normalized_c(arguments[2]) != "2":
                fail(f"unclassified modifier memcmp in {path}: {arguments!r}")
            token = c_string_literal(arguments[0])
            if token is None or len(token) != 2:
                fail(f"invalid modifier memcmp token in {path}: {arguments!r}")
            double_sources.append(token)
        else:
            fail(f"unclassified modifier parser call in {path}: {name}{arguments!r}")
    if len(single_sources) != 2 or len(double_sources) != 7:
        fail(
            "expected two single-character modifier sources and seven double-character "
            f"modifier sources, got {single_sources!r} and {double_sources!r}"
        )
    modifiers = set(double_sources)
    for source_chars in single_sources:
        modifiers.update(source_chars)
    if len(modifiers) != 36:
        fail(f"expected 36 format modifiers, got {len(modifiers)}: {sorted(modifiers)!r}")
    return sorted(modifiers)


def split_records(output, fields, label, separator):
    records = []
    for number, line in enumerate(output.splitlines(), start=1):
        parts = line.split(separator)
        if len(parts) != fields:
            fail(f"malformed {label} record on line {number}: {line!r}")
        records.append(parts)
    return records


def option_names(outputs):
    names = set()
    for output in outputs:
        for line in output.splitlines():
            name = line.split(maxsplit=1)[0]
            names.add(re.sub(r"\[[0-9]+\]$", "", name))
    return sorted(names)


def hook_names(outputs):
    return sorted(
        {
            re.sub(r"\[[0-9]+\]$", "", line.split(maxsplit=1)[0])
            for output in outputs
            for line in output.splitlines()
        }
    )


def capture(path):
    expected_pin = manifest_pin()
    env, version, source = verify_tmux(path, expected_pin)
    socket = f"zz-oracle-{os.getpid()}-{secrets.token_hex(4)}"
    separator = "__ZZ_ORACLE_FIELD__"
    base = [path, "-L", socket, "-f", "/dev/null"]
    try:
        run(base + ["new-session", "-d", "-s", "zz-oracle"], env)
        command_output = run(
            base
            + [
                "list-commands",
                "-F",
                separator.join(
                    [
                        "#{command_list_name}",
                        "#{command_list_alias}",
                        "#{command_list_usage}",
                    ]
                ),
            ],
            env,
        )
        key_output = run(
            base
            + [
                "list-keys",
                "-a",
                "-F",
                separator.join(
                    ["#{key_table}", "#{key_string}", "#{key_repeat}", "#{key_command}"]
                ),
            ],
            env,
        )
        option_outputs = [
            run(base + ["show-options", "-s"], env),
            run(base + ["show-options", "-g", "-t", "=zz-oracle"], env),
            run(base + ["show-options", "-g", "-w", "-t", "=zz-oracle:0"], env),
        ]
        hook_outputs = [
            run(base + ["show-hooks", "-g", "-t", "=zz-oracle"], env),
            run(base + ["show-hooks", "-g", "-w", "-t", "=zz-oracle:0"], env),
            run(base + ["show-hooks", "-g", "-p", "-t", "=zz-oracle:0.0"], env),
        ]
    finally:
        subprocess.run(
            base + ["kill-server"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=env,
        )
    pinned_commands = source_commands(source)
    args_parse = source_args_parse(source, pinned_commands)
    records = split_records(command_output, 3, "command", separator)
    observable_names = {name for name, _, _ in records}
    if observable_names != set(pinned_commands):
        missing = sorted(observable_names - set(pinned_commands))
        stale = sorted(set(pinned_commands) - observable_names)
        fail(f"pinned source and list-commands differ; missing={missing}, stale={stale}")
    commands = []
    for name, alias, usage in records:
        commands.append(
            {
                "name": name,
                "aliases": [alias] if alias else [],
                "usage": usage,
                "flags": pinned_commands[name]["flags"],
                "min_args": pinned_commands[name]["min_args"],
                "max_args": pinned_commands[name]["max_args"],
            }
        )
    key_bindings = []
    for table, key, repeat, command in split_records(key_output, 4, "key binding", separator):
        if repeat not in {"0", "1"}:
            fail(f"invalid repeat value for {table} key {key!r}: {repeat!r}")
        key_bindings.append(
            {"table": table, "key": key, "repeat": repeat == "1", "command": command}
        )
    commands.sort(key=lambda command: command["name"])
    key_bindings.sort(
        key=lambda binding: (
            binding["table"],
            binding["key"],
            binding["repeat"],
            binding["command"],
        )
    )
    return {
        "schema": SCHEMA,
        "pin": expected_pin,
        "version": version,
        "commands": commands,
        "args_parse": args_parse,
        "options": option_names(option_outputs),
        "formats": source_formats(source),
        "format_contexts": source_format_contexts(source),
        "format_modifiers": source_format_modifiers(source),
        "hooks": hook_names(hook_outputs),
        "key_bindings": key_bindings,
    }


def encoded(data):
    return json.dumps(data, ensure_ascii=False, indent=2) + "\n"


def main():
    parser = argparse.ArgumentParser(description="Capture and verify the pinned tmux catalog")
    parser.add_argument("--tmux", type=Path, default=DEFAULT_TMUX)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    arguments = parser.parse_args()
    data = capture(arguments.tmux)
    rendered = encoded(data)
    if arguments.write:
        ORACLE.write_text(rendered, encoding="utf-8")
        print(f"wrote {ORACLE.relative_to(ROOT)}")
        return 0
    if not ORACLE.exists():
        print(f"error: missing {ORACLE.relative_to(ROOT)}; run with --write", file=sys.stderr)
        return 1
    current = ORACLE.read_text(encoding="utf-8")
    try:
        json.loads(current)
    except json.JSONDecodeError as error:
        fail(f"cannot parse {ORACLE.relative_to(ROOT)}: {error}")
    if current != rendered:
        print(f"error: {ORACLE.relative_to(ROOT)} is stale; run with --write", file=sys.stderr)
        return 1
    print(f"{ORACLE.relative_to(ROOT)} matches {VERSION} at {PIN}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from error
