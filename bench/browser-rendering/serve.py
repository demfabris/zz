#!/usr/bin/env python3
"""Serve pages/ to every browser under test and collect their self-reports.

The page POSTs its own rAF statistics to /report when a run finishes, so no
target needs a debugging port, a driver, or any automation: if it can open a
URL, it can be benchmarked.

    ./serve.py                 # then paste the printed URL into each app
    ./serve.py --port 9000
"""

from __future__ import annotations

import argparse
import json
import socket
import sys
from datetime import datetime, timezone
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
PAGES = HERE / "pages"
RESULTS = HERE / "results"
ENGINE_JSONL = RESULTS / "engine.jsonl"

MAX_BODY = 1 << 20  # 1 MiB; reports are ~1 KiB


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(PAGES), **kwargs)

    def end_headers(self):
        # A cached flicker.html would silently benchmark a stale page.
        self.send_header("Cache-Control", "no-store, max-age=0")
        super().end_headers()

    def do_POST(self):  # noqa: N802 - stdlib naming
        if self.path.split("?")[0] != "/report":
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length") or 0)
        except ValueError:
            self.send_error(400, "bad Content-Length")
            return
        if length <= 0 or length > MAX_BODY:
            self.send_error(400, "bad body length")
            return
        try:
            payload = json.loads(self.rfile.read(length).decode("utf-8"))
        except (ValueError, UnicodeDecodeError) as exc:
            self.send_error(400, "bad JSON: %s" % exc)
            return

        payload["received_at"] = datetime.now(timezone.utc).strftime(
            "%Y-%m-%dT%H:%M:%SZ"
        )
        RESULTS.mkdir(exist_ok=True)
        # Single write to an O_APPEND handle stays atomic if two panes report at once.
        with ENGINE_JSONL.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(payload, separators=(",", ":")) + "\n")

        label = payload.get("label") or "(unlabelled)"
        print(
            "  report  %-14s %6.1f engine fps  p50 %.2fms  long %.1f%%  load=%sms"
            % (
                label,
                payload.get("engine_fps", 0),
                (payload.get("interval_ms") or {}).get("p50", 0),
                payload.get("long_frame_pct", 0),
                payload.get("load_ms", 0),
            ),
            flush=True,
        )
        for reason in payload.get("tainted") or []:
            print("       !! TAINTED: %s, discard this run" % reason, flush=True)
        self.send_response(204)
        self.end_headers()

    def log_message(self, fmt, *args):
        pass  # the /report line above is the only output worth having


def lan_ip():
    """Best-effort LAN address, for driving the page from another machine."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.connect(("192.0.2.1", 9))  # TEST-NET-1: routed nowhere, never sends
        return sock.getsockname()[0]
    except OSError:
        return None
    finally:
        sock.close()


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--port", type=int, default=8777)
    ap.add_argument(
        "--host",
        default="127.0.0.1",
        help="bind address (0.0.0.0 to reach other machines)",
    )
    ap.add_argument(
        "--secs", type=int, default=20, help="run length baked into the printed URLs"
    )
    args = ap.parse_args()

    if not (PAGES / "flicker.html").is_file():
        sys.exit("missing %s" % (PAGES / "flicker.html"))

    # Redirected to a file or a pane's log, stdout is block-buffered and the
    # banner (and every report) would sit invisible in the buffer for minutes.
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except AttributeError:
        pass

    try:
        httpd = ThreadingHTTPServer((args.host, args.port), Handler)
    except OSError as exc:
        sys.exit("cannot bind %s:%d: %s" % (args.host, args.port, exc))

    base = "http://%s:%d" % (
        "127.0.0.1" if args.host in ("0.0.0.0", "") else args.host,
        args.port,
    )
    print("serving %s on %s" % (PAGES, base))
    print("reports -> %s\n" % ENGINE_JSONL)
    print("paste one URL per app (label it, it ends up in the results):")
    for label in ("chrome", "zz", "codex"):
        print(
            "  %-8s %s/flicker.html?label=%s&secs=%d" % (label, base, label, args.secs)
        )
    print("\n  add &load=4   for 4ms of synthetic engine work per frame")
    print("  add &auto=0   to start on click instead of after 2s")
    if args.host in ("0.0.0.0", ""):
        ip = lan_ip()
        if ip:
            print("\n  LAN: http://%s:%d/flicker.html" % (ip, args.port))
    print("\nctrl-c to stop")

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nstopped")
    finally:
        httpd.server_close()


if __name__ == "__main__":
    main()
