#!/usr/bin/env python3
"""Fail when committed campaign evidence carries a credential value or an environment dump.

    python3 compat/evidence-secrets.py

Corpus runs, probes and hook census scenarios can capture the environment of whatever process started
them. On 2026-09-16 that put a Claude Code messaging token into evidence pushed to a public branch, and
the prefix-based secret scans missed it because the token had no known prefix. This guard reads every
tracked evidence file, archive members included, and rejects a credential-shaped NAME=value whose value
is literal, plus any file that looks like a whole process environment.
"""

import io
import re
import subprocess
import sys
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCOPES = ["compat/tui/evidence", "compat/results", "compat/orchestration"]
CREDENTIAL = re.compile(
    rb"\b[A-Z0-9_]*(TOKEN|SECRET|PASSWORD|PASSWD|API_KEY|CREDENTIAL|PRIVATE_KEY)[A-Z0-9_]*"
    rb"=(?![\"']?\$)(?!\[REDACTED)(?![\"']?<)([\"']?)([A-Za-z0-9_\-./+=:]{16,})"
)
ENVIRONMENT_MARKERS = [re.compile(name + rb"=(?!\[REDACTED)") for name in
                       (rb"SSH_AUTH_SOCK", rb"DBUS_SESSION_BUS_ADDRESS", rb"XDG_RUNTIME_DIR", rb"\bHOME")]


def tracked():
    out = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z", "--", *SCOPES],
                         capture_output=True, check=True).stdout
    return [p for p in out.decode().split("\0") if p]


def blobs(path, data):
    if path.endswith((".tar.gz", ".tgz", ".tar")):
        try:
            with tarfile.open(fileobj=io.BytesIO(data)) as archive:
                for member in archive.getmembers():
                    if member.isfile():
                        yield f"{path}:{member.name}", archive.extractfile(member).read()
        except tarfile.TarError:
            yield path, data
    else:
        yield path, data


def main():
    problems = []
    for path in tracked():
        try:
            data = (ROOT / path).read_bytes()
        except OSError:
            continue
        for name, content in blobs(path, data):
            if b"\0" in content[:4096]:
                continue
            for match in CREDENTIAL.finditer(content):
                line = content.count(b"\n", 0, match.start()) + 1
                variable = match.group(0).split(b"=", 1)[0].decode()
                problems.append(f"{name}:{line}: {variable} carries a literal value")
            if sum(bool(marker.search(content)) for marker in ENVIRONMENT_MARKERS) >= 3:
                problems.append(f"{name}: looks like a full process environment dump")
    if problems:
        print("evidence-secrets: committed evidence must not carry credentials or environment dumps")
        for problem in problems[:40]:
            print(f"  {problem}")
        if len(problems) > 40:
            print(f"  ... and {len(problems) - 40} more")
        return 1
    print("evidence-secrets: no credential values or environment dumps in tracked evidence")
    return 0


if __name__ == "__main__":
    sys.exit(main())
