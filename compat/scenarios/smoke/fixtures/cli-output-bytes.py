import os
import pathlib
import subprocess
import sys

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=15)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stderr))
    return result.stdout


def require_bytes(args, expected):
    actual = tmux(*args)
    if actual != expected:
        raise AssertionError((args, actual, expected))
    octets = subprocess.run(["od", "-An", "-tx1", "-v"], input=actual,
                            capture_output=True, check=True).stdout.split()
    if octets != [f"{byte:02x}".encode() for byte in expected]:
        raise AssertionError((args, octets))


result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")
try:
    require_bytes(["display-message", "-p", ""], b"\n")
    require_bytes(["display", "-p", "\n"], b"\n\n")
    require_bytes(["display", "-p", "line\n"], b"line\n\n")
    require_bytes(["display", "-lp", "#{literal}"], b"#{literal}\n")
    tmux("set-buffer", "-b", "bytes", "hello")
    require_bytes(["show-buffer", "-b", "bytes"], b"hello")
    chars = subprocess.run(["od", "-c"], input=tmux("show-buffer", "-b", "bytes"),
                           capture_output=True, check=True).stdout
    if not chars.endswith(b"0000005\n"):
        raise AssertionError(chars)
    tmux("set-buffer", "-b", "bytes", "hello\n")
    require_bytes(["showb", "-bbytes"], b"hello\n")
    path = pathlib.Path(os.environ["HOME"]) / "cli-output-source.conf"
    path.write_text("display-message -p ''\ndisplay-message -p line\ndisplay-message -p ''\n")
    require_bytes(["source-file", str(path)], b"\nline\n\n")
    tmux("set-buffer", "-b", "bytes", "hello")
    mixed = b"hello\n" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else b"hello"
    require_bytes(["show-buffer", "-b", "bytes", ";", "display", "-p", ""], mixed)
    result = "clean:8"
except Exception as error:
    print(repr(error), flush=True)
finally:
    tmux("set-environment", "-g", "ZZ_CLI_OUTPUT_BYTES", result)
