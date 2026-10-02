import random
import re
import shlex
import time

import isolate
import timing
from groups.control import Control
from groups.copy import history_file
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
    fairness(ctx)


def fairness(ctx):
    path = ctx.env.path("echo-capture-history.txt")
    history_file(path)
    original_shards = ctx.zz.env.get("ZZ_PTY_SHARDS")
    commands = {
        "list_keys": "list-keys -F '#{key_table} #{key_string} #{key_command}'\n",
        "capture_history": "capture-pane -p -t fair:1.0 -S -\n",
    }
    try:
        for shard_name in ("default", "k1"):
            ctx.reset()
            if shard_name == "k1":
                ctx.zz.env["ZZ_PTY_SHARDS"] = "1"
            elif original_shards is None:
                ctx.zz.env.pop("ZZ_PTY_SHARDS", None)
            else:
                ctx.zz.env["ZZ_PTY_SHARDS"] = original_shards
            for mux in ctx.muxes:
                ctx.session(mux, "fair", 180, 50, ctx.pane("echo.py", 0))
                mux.run("set-option", "-g", "status", "off", check=True)
                mux.run("new-window", "-d", "-t", "fair:1", f"cat {shlex.quote(path)}; exec sleep 1000000", check=True)
                for window in range(2, 20):
                    mux.run("new-window", "-d", "-t", f"fair:{window}", "exec sleep 1000000", check=True)
                if not ctx.wait(lambda mux=mux: mux.out("display-message", "-p", "-t", "fair:1.0", "#{history_size}") == str(isolate.HISTORY_LIMIT), 30):
                    raise RuntimeError(f"{mux.name} fairness history did not fill")
            clients = {}
            controls = {}
            latencies = {name: {m.name: [] for m in ctx.muxes} for name in ("idle", *commands)}
            counts = {m.name: 0 for m in ctx.muxes}
            try:
                for mux in ctx.muxes:
                    clients[mux.name] = ctx.attach(mux, "fair:0", 180, 50)
                    controls[mux.name] = Control(ctx, mux, "fair:1")
                    controls[mux.name].settle()
                ctx.drain(list(clients.values()), 0.5)
                for iteration in range(ctx.pick(200, 100)):
                    variants = ("idle", *commands) if iteration % 2 == 0 else (*reversed(commands), "idle")
                    for name in variants:
                        for mux in ctx.order(iteration):
                            client = clients[mux.name]
                            control = controls[mux.name]
                            if counts[mux.name] and counts[mux.name] % LINE == 0:
                                client.write(b"\r")
                                client.drain(0.02)
                            target = control.ends + 1
                            errors = control.errors
                            if name != "idle":
                                control.send(commands[name])
                                time.sleep((iteration % 5) * 0.00025)
                            counts[mux.name] += 1
                            ms = keystroke(client, LETTERS[iteration % len(LETTERS)], counts[mux.name])
                            if ms is None:
                                raise RuntimeError(f"{mux.name} lost an echo during {name} with {shard_name} shards")
                            latencies[name][mux.name].append(ms)
                            if name != "idle" and (not control.wait_ends(target, 10) or control.errors != errors):
                                raise RuntimeError(f"{mux.name} fairness {name} failed")
                            client.drain(0.002)
                suffix = "p20" + (".k1" if shard_name == "k1" else "")
                p99 = {name: {mux: timing.quantile(sorted(values), 0.99) for mux, values in samples.items()} for name, samples in latencies.items()}
                note = f"20 panes; another pane echoes during an uncached listing or a 10000-row, 180-column history capture; shard setting {shard_name}; idle samples interleaved"
                for name, samples in p99.items():
                    ctx.add(f"echo.p99.{name}.{suffix}", "ms", "wall", samples["zz"], samples["tmux"], note)
                for name in commands:
                    increase = {mux: max(0, p99[name][mux] - p99["idle"][mux]) for mux in ("zz", "tmux")}
                    ctx.add(f"echo.increase.{name}.{suffix}", "ms", "wall", increase["zz"], increase["tmux"], note)
            finally:
                for mux in ctx.muxes:
                    if mux.name in clients:
                        clients[mux.name].detach(mux.detach_keys)
                    if mux.name in controls:
                        controls[mux.name].close()
    finally:
        if original_shards is None:
            ctx.zz.env.pop("ZZ_PTY_SHARDS", None)
        else:
            ctx.zz.env["ZZ_PTY_SHARDS"] = original_shards
