#!/usr/bin/env python3
"""Report the pinned names no proof in this repo ever exercises.

A name that only appears in the oracle, the catalog or a Rust unit test is
registered, not fired. This walks the differential corpus and the attached
fixtures and prints, per family, the pinned names none of them mention:

    python3 compat/census.py            # human report
    python3 compat/census.py --json     # machine report
    python3 compat/census.py --quiet    # counts only

Exit status is always 0: this is a drift report, not a gate.
"""

import argparse
import json
import pathlib
import re
import sys

COMPAT = pathlib.Path(__file__).resolve().parent
REPO = COMPAT.parent
ORACLE = COMPAT / "tmux-oracle.json"
CONSUMERS_SOURCE = REPO / "crates/zz-mux/src/command.rs"
CONSUMERS_CONST = "TMUX_OPTION_CONSUMERS"

CORPUS_ROOTS = [
    COMPAT / "scenarios",
    COMPAT / "attached-client.sh",
    COMPAT / "status-row.sh",
    COMPAT / "diff-scenario.sh",
]

FORMAT_ALIASES = {
    "#D": "pane_id",
    "#F": "window_flags",
    "#H": "host",
    "#I": "window_index",
    "#P": "pane_index",
    "#S": "session_name",
    "#T": "pane_title",
    "#W": "window_name",
    "#h": "host_short",
}

MODIFIER_EXEMPT = {
    "C", "E", "I", "L", "N", "O", "P", "R", "S", "T", "V", "W",
    "a", "b", "c", "d", "l", "m", "n", "q", "s", "t", "w",
    "!", "!!", "!=", "&&", "<", "<=", "=", "==", ">", ">=", "||",
}


def corpus_files():
    files = []
    for root in CORPUS_ROOTS:
        if root.is_file():
            files.append(root)
        elif root.is_dir():
            files.extend(sorted(path for path in root.rglob("*") if path.is_file()))
    return files


def corpus_text(files):
    chunks = []
    for path in files:
        try:
            body = path.read_text(errors="replace")
        except OSError:
            continue
        chunks.append("\n".join(line for line in body.splitlines()
                                 if not line.lstrip().startswith("#")))
    return "\n".join(chunks)


def consumed_options():
    source = CONSUMERS_SOURCE.read_text()
    start = source.index(f"pub const {CONSUMERS_CONST}: &[&str] = &[")
    end = source.index("];", start)
    return sorted(set(re.findall(r'"([^"]+)"', source[start:end])))


def word_hits(text, name):
    return re.search(r"(?<![A-Za-z0-9_@-])" + re.escape(name) + r"(?![A-Za-z0-9_-])", text)


def modifier_hits(text, modifier):
    return re.search(r"#\{" + re.escape(modifier) + r"[-|:/0-9]", text)


def census():
    oracle = json.loads(ORACLE.read_text())
    files = corpus_files()
    text = corpus_text(files)
    consumers = consumed_options()

    report = {
        "corpus_files": len(files),
        "corpus_roots": [str(root.relative_to(REPO)) for root in CORPUS_ROOTS],
    }

    report["options"] = {
        "tracked": len(consumers),
        "never_exercised": [name for name in consumers if not word_hits(text, name)],
    }
    report["hooks"] = {
        "tracked": len(oracle["hooks"]),
        "never_exercised": [name for name in oracle["hooks"]
                            if not word_hits(text, name)],
    }
    formats = oracle["formats"]
    report["formats"] = {
        "tracked": len(formats),
        "never_exercised": [name for name in formats if not word_hits(text, name)],
    }
    report["format_aliases"] = {
        "tracked": len(FORMAT_ALIASES),
        "never_exercised": sorted(alias for alias in FORMAT_ALIASES
                                  if alias not in text),
    }
    modifiers = [m for m in oracle["format_modifiers"] if m not in MODIFIER_EXEMPT]
    report["format_modifiers"] = {
        "tracked": len(modifiers),
        "exempt": sorted(MODIFIER_EXEMPT),
        "never_exercised": [m for m in modifiers if not modifier_hits(text, m)],
    }
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--quiet", action="store_true")
    arguments = parser.parse_args()

    report = census()
    if arguments.json:
        json.dump(report, sys.stdout, indent=2)
        sys.stdout.write("\n")
        return 0

    print(f"census: {report['corpus_files']} corpus files under "
          + ", ".join(report["corpus_roots"]))
    for family in ("options", "hooks", "formats", "format_aliases",
                   "format_modifiers"):
        block = report[family]
        missing = block["never_exercised"]
        print(f"{family}: {len(missing)} of {block['tracked']} never exercised")
        if missing and not arguments.quiet:
            for name in missing:
                print(f"    {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
