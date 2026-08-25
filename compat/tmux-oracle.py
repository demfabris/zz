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


SCHEMA = 4
PIN = "d77c9dc6aa021e4bc61f0da128c591af695e6466"
VERSION = "tmux next-3.8"
ROOT = Path(__file__).resolve().parent.parent
ORACLE = ROOT / "compat" / "tmux-oracle.json"
MANIFEST = ROOT / "compat" / "tmux-gaps.json"
FETCH_TMUX = ROOT / "compat" / "fetch-tmux.sh"
DEFAULT_TMUX = ROOT / "compat" / ".cache" / "tmux-src" / "tmux"


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


def literal_format_names(path):
    pattern = re.compile(
        r'\bformat_add(?:_cb|_tv)?\s*\(\s*[^,]+,\s*"([a-z0-9_]+)"',
        re.MULTILINE,
    )
    names = set(pattern.findall(path.read_text(encoding="utf-8")))
    if not names:
        fail(f"pinned source contains no literal context format names in {path}")
    return sorted(names)


def source_format_contexts(source):
    command_item = literal_format_names(source / "cmd-queue.c")
    if command_item != ["command"]:
        fail(f"unexpected command-item format names: {command_item}")
    return {
        "command-item": command_item,
        "list-commands": literal_format_names(source / "cmd-list-commands.c"),
        "list-keys": literal_format_names(source / "cmd-list-keys.c"),
    }


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
