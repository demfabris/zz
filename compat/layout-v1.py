#!/usr/bin/env python3

import json
import sys


def checksum(layout):
    value = 0
    for byte in layout.encode():
        value = (value >> 1) + ((value & 1) << 15)
        value = (value + byte) & 0xFFFF
    return value


def tiled(node):
    if node["t"] == "p":
        return None if "z" in node else node
    children = [child for child in map(tiled, node["c"]) if child is not None]
    if not children:
        return None
    if len(children) == 1:
        return children[0]
    return {**node, "c": children}


def cell(node):
    head = f"{node['w']}x{node['h']},{node['x']},{node['y']}"
    kind = node["t"]
    if kind == "p":
        return f"{head},{node['I'].removeprefix('%')}"
    children = [cell(child) for child in node["c"]]
    if kind == "h":
        return f"{head}{{{','.join(children)}}}"
    if kind == "v":
        return f"{head}[{','.join(children)}]"
    raise ValueError(f"unknown layout cell type {kind!r}")


def v1(layout):
    if not layout.startswith("{"):
        return layout
    tree = json.loads(layout)
    if tree.get("V") != 2:
        raise ValueError(f"unknown layout version in {layout!r}")
    root = tiled(tree["L"])
    if root is None:
        return ""
    if root["t"] == "p":
        root = {**root, "x": 0, "y": 0}
    body = cell(root)
    return f"{checksum(body):04x},{body}"


def main():
    for line in sys.stdin:
        fields = line.rstrip("\n").split(":", 2)
        if len(fields) == 3:
            fields[2] = v1(fields[2])
        print(":".join(fields))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
