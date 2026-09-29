import hashlib
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(os.path.dirname(HERE), ".cache", "perf")
MIB = 1 << 20
BLOCK = 4 * MIB
SEED = 20260928

UNICODE_POOLS = [
    [chr(c) for c in range(0x20, 0x7F)],
    [chr(c) for c in range(0xC0, 0x180)],
    [chr(c) for c in range(0x4E00, 0x4F00)],
    [chr(c) for c in range(0x1F600, 0x1F640)],
    ["e\u0301", "a\u0308", "n\u0303", "o\u0302"],
    [chr(c) for c in range(0x0410, 0x0450)],
]


def _ascii_block(rng):
    alphabet = [bytes([c]) for c in range(0x20, 0x7F)]
    out = bytearray()
    while len(out) < BLOCK:
        width = rng.randint(1, 170)
        out += b"".join(rng.choices(alphabet, k=width)) + b"\n"
    return bytes(out[:BLOCK])


def _unicode_block(rng):
    out = bytearray()
    weights = [60, 10, 12, 6, 4, 8]
    while len(out) < BLOCK:
        cells = rng.randint(1, 80)
        pool_line = rng.choices(UNICODE_POOLS, weights=weights, k=cells)
        out += "".join(rng.choice(pool) for pool in pool_line).encode() + b"\n"
    cut = BLOCK
    while cut > 0 and out[cut - 1] != 0x0A:
        cut -= 1
    return bytes(out[:cut])


def fixture(kind, mib):
    os.makedirs(CACHE, exist_ok=True)
    path = os.path.join(CACHE, f"{kind}-{mib}mib-{SEED}.txt")
    if os.path.exists(path) and os.path.getsize(path) > 0:
        return path
    rng = random.Random(f"{SEED}-{kind}")
    block = _ascii_block(rng) if kind == "ascii" else _unicode_block(rng)
    target = mib * MIB
    tmp = path + ".part"
    with open(tmp, "wb") as f:
        written = 0
        while written < target:
            chunk = block[: target - written]
            f.write(chunk)
            written += len(chunk)
    os.replace(tmp, path)
    return path


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(MIB), b""):
            h.update(chunk)
    return h.hexdigest()
