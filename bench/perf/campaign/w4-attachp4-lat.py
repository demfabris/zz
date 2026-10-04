import json
import os
import socket
import statistics
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "groups"))

import isolate
import probe
from attach import MARK_A, MARK_B, QUIET, content, setup
from ptyclient import PtyClient


class Ctx:
    def session(self, mux, name, cols=180, rows=50, command=None):
        args = ["new-session", "-d", "-s", name, "-x", str(cols), "-y", str(rows)]
        if command:
            args.append(command)
        mux.run(*args, check=True)
        mux.run("set-option", "-g", "default-size", f"{cols}x{rows}")


class StampProxy:
    def __init__(self, listen, upstream):
        self.listen = listen
        self.upstream = upstream
        self.lock = threading.Lock()
        self.conns = []
        try:
            os.unlink(listen)
        except FileNotFoundError:
            pass
        self.server = socket.socket(socket.AF_UNIX)
        self.server.bind(listen)
        self.server.listen(64)
        threading.Thread(target=self._accept, daemon=True).start()

    def take(self):
        with self.lock:
            conns, self.conns = self.conns, []
        return conns

    def _accept(self):
        while True:
            try:
                client, _ = self.server.accept()
            except OSError:
                return
            upstream = socket.socket(socket.AF_UNIX)
            try:
                upstream.connect(self.upstream)
            except OSError:
                client.close()
                continue
            events = []
            with self.lock:
                self.conns.append(events)
            threading.Thread(target=self._pump, args=(client, upstream, False, events), daemon=True).start()
            threading.Thread(target=self._pump, args=(upstream, client, True, events), daemon=True).start()

    def _pump(self, src, dst, downstream, events):
        while True:
            try:
                data = src.recv(1 << 20)
            except OSError:
                data = b""
            if not data:
                break
            now = time.perf_counter()
            try:
                dst.sendall(data)
            except OSError:
                break
            events.append((now, downstream, len(data)))
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass

    def close(self):
        try:
            self.server.close()
        except OSError:
            pass
        try:
            os.unlink(self.listen)
        except OSError:
            pass


def phases(conns, t0, seen):
    for events in conns:
        events = sorted(events)
        for i, (t, down, size) in enumerate(events):
            if down and size >= 1000:
                ups = [e for e in events[:i] if not e[1]]
                if ups:
                    first = ups[0][0]
                    return {
                        "lat": (t - ups[-1][0]) * 1000,
                        "spawn": (first - t0) * 1000,
                        "hello": (ups[-1][0] - first) * 1000,
                        "paint": (seen - t) * 1000 if seen else None,
                    }
    return {}


TARGETS = {"a": (MARK_A, 1), "b": (MARK_B, 4)}


def once(mux, env, run_env, target):
    marker, count = TARGETS[target]
    s0 = mux.sample()
    client = PtyClient(mux.attach_argv(target), run_env, env, cols=180, rows=50)
    seen = client.read_until(content(marker, count), 5.0)
    ttfc = (seen - client.t0) * 1000 if seen else None
    client.drain_quiet(QUIET, 3.0)
    client.detach(mux.detach_keys)
    time.sleep(0.2)
    s1 = mux.sample()
    return ttfc, (s1.cpu_ns - s0.cpu_ns) / 1e6, (client.t0, seen)


def stats(values):
    values = [v for v in values if v is not None]
    if not values:
        return {}
    values.sort()
    pick = lambda q: values[min(len(values) - 1, int(q * len(values)))]
    return {"n": len(values), "median": statistics.median(values), "mean": statistics.mean(values), "p10": pick(0.1), "p90": pick(0.9), "min": values[0], "max": values[-1]}


def main():
    out_path = sys.argv[1]
    bins = sys.argv[2:]
    runs = int(os.environ.get("RUNS", "30"))
    target = os.environ.get("TARGET", "b")
    envs, muxes, proxies = [], [], []
    try:
        ctx = Ctx()
        for n, binary in enumerate(bins):
            env = isolate.Env()
            env.zz_socket = f"/tmp/{env.tag}-{n}.sock"
            env.env["ZZ_SOCKET"] = env.zz_socket
            envs.append(env)
            mux = isolate.Zz(binary, env)
            mux.env["ZZ_SOCKET"] = env.zz_socket
            muxes.append(mux)
            setup(ctx, mux)
            proxies.append(StampProxy(env.path("px.sock"), env.zz_socket))
        time.sleep(1.0)
        rows = {b: {"lat": [], "spawn": [], "hello": [], "paint": [], "pttfc": [], "ttfc": [], "cpu": []} for b in bins}
        for i in range(runs):
            order = list(range(len(bins)))
            if i % 2:
                order.reverse()
            for n in order:
                mux, env, proxy, binary = muxes[n], envs[n], proxies[n], bins[n]
                if not os.environ.get("NOPROXY"):
                    proxy.take()
                    pttfc, _, (t0, seen) = once(mux, env, dict(mux.env, ZZ_SOCKET=proxy.listen), target)
                    split = phases(proxy.take(), t0, seen)
                    for key in ("lat", "spawn", "hello", "paint"):
                        rows[binary][key].append(split.get(key))
                    rows[binary]["pttfc"].append(pttfc)
                    time.sleep(0.1)
                if not os.environ.get("NODIRECT"):
                    ttfc, cpu, _ = once(mux, env, mux.env, target)
                    rows[binary]["ttfc"].append(ttfc)
                    rows[binary]["cpu"].append(cpu)
                    time.sleep(0.1)
        result = {b: {k: stats(v) for k, v in r.items()} for b, r in rows.items()}
        result["raw"] = rows
    finally:
        for proxy in proxies:
            proxy.close()
        for env in envs:
            env.cleanup()
    with open(out_path, "w") as f:
        json.dump(result, f, indent=1)
    for binary in bins:
        print(os.path.basename(binary))
        for key in ("spawn", "hello", "lat", "paint", "pttfc", "ttfc", "cpu"):
            s = result[binary][key]
            if s:
                print(f"  {key:6s} n {s['n']:3d} median {s['median']:7.3f} mean {s['mean']:7.3f} p10 {s['p10']:7.3f} p90 {s['p90']:7.3f}")


if __name__ == "__main__":
    main()
