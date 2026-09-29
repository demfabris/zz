import random
import re
import time

import timing
from sockproxy import SockProxy

LETTERS = b"qwzxjvkbmyup"
TAG = "\u00a7".encode()
ESCAPES = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?|\x1b[P^_].*?\x1b\\|\x1b[()*+#%][ -~]|\x1b[ -~]|[\x00-\x1f\x7f]", re.DOTALL)
LINE = 25


def text(data):
    return ESCAPES.sub(b"", bytes(data))


def keystroke(client, char, count, timeout=1.0):
    start = len(client.buf)
    token = TAG + b"%03d" % (count % 1000)
    t0 = time.perf_counter()
    client.write(bytes([char]))
    seen = client.read_until(lambda buf: token in text(buf[start:]), timeout)
    return (seen - t0) * 1000 if seen else None


def run(ctx):
    runs = ctx.pick(200, 100)
    rng = random.Random(7)
    variants = [("idle", 0), ("busy30", 30)]
    sent = {}
    for mux in ctx.muxes:
        for name, hz in variants:
            ctx.session(mux, f"e{name}", 120, 40, ctx.pane("echo.py", hz))
        mux.run("set-option", "-g", "status", "off", check=True)
    time.sleep(1.0)
    for name, _ in variants:
        clients = {m.name: ctx.attach(m, f"e{name}", 120, 40) for m in ctx.muxes}
        ctx.drain(list(clients.values()), 1.5)
        lat = {m.name: [] for m in ctx.muxes}
        missed = {m.name: 0 for m in ctx.muxes}
        for i in range(runs):
            char = LETTERS[i % len(LETTERS)]
            for mux in ctx.order(i):
                client = clients[mux.name]
                if i and i % LINE == 0:
                    client.write(b"\r")
                    client.drain(0.05)
                key = (mux.name, name)
                sent[key] = sent.get(key, 0) + 1
                ms = keystroke(client, char, sent[key])
                if ms is None:
                    missed[mux.name] += 1
                else:
                    lat[mux.name].append(ms)
                client.drain(rng.uniform(0.02, 0.05))
        notes = [f"{n} missed {k}/{runs} echoes" for n, k in missed.items() if k] + ["the pane echoes key N as U+00A7 and N, matched in the tty text with escape sequences removed"]
        ctx.add(f"echo.p50.{name}", "ms", "wall", lat["zz"], lat["tmux"], notes)
        p99 = {n: timing.quantile(sorted(v), 0.99) for n, v in lat.items()}
        ctx.add(f"echo.p99.{name}", "ms", "wall", p99["zz"], p99["tmux"])
        for mux in ctx.muxes:
            clients[mux.name].detach(mux.detach_keys)
    proxy_path = ctx.env.path("px.sock")
    proxy = SockProxy(proxy_path, ctx.zz.socket)
    try:
        env = dict(ctx.zz.env, ZZ_SOCKET=proxy_path)
        for name, _ in variants:
            client = ctx.attach(ctx.zz, f"e{name}", 120, 40, env=env)
            client.drain(1.5)
            per = []
            for i in range(20):
                if i % LINE == 0:
                    client.write(b"\r")
                    client.drain(0.05)
                before = proxy.counters()["s2c"]
                key = ("zz", name)
                sent[key] = sent.get(key, 0) + 1
                if keystroke(client, LETTERS[i % len(LETTERS)], sent[key]) is not None:
                    client.drain(0.06)
                    per.append(proxy.counters()["s2c"] - before)
                client.drain(0.03)
            note = "zz only: daemon to client bytes per keystroke, echo plus 60 ms"
            if name != "idle":
                note += "; includes the 30 Hz ticker output in the same window"
            ctx.add(f"echo.wire_bytes.{name}", "B", "bytes", per, None, note)
            client.detach(ctx.zz.detach_keys)
    finally:
        proxy.close()
