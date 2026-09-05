import os
import pathlib
import subprocess
import sys

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args, data=None):
    return subprocess.run(["tmux", *args], input=data, capture_output=True, timeout=15)


def require(condition, message):
    if not condition:
        raise AssertionError(message)


result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")
try:
    payload = b"a\0b\xff"
    require(tmux("load-buffer", "-", data=payload).returncode == 0, "load stdin")
    saved = tmux("save-buffer", "-")
    require(saved.returncode == 0 and saved.stdout == payload, repr(saved))
    octets = subprocess.run(["od", "-An", "-tx1", "-v"], input=saved.stdout, capture_output=True, check=True)
    require(octets.stdout.split() == [b"61", b"00", b"62", b"ff"], "od byte oracle")
    require(tmux("load-buffer", "-b", "named", "-", data=payload).returncode == 0, "named stdin")
    saved = tmux("save-buffer", "-a", "-b", "named", "-")
    require(saved.returncode == 0 and saved.stdout == payload, "append stdout")
    require(tmux("load-buffer", "-b", "named", "-", data=b"").returncode == 0, "empty stdin")
    require(tmux("save-buffer", "-b", "named", "-").stdout == payload, "empty preserves named buffer")
    path = pathlib.Path(os.environ["HOME"]) / "buffer-stream-append"
    require(tmux("save-buffer", "-b", "named", str(path)).returncode == 0, "save file")
    require(tmux("save-buffer", "-a", "-b", "named", str(path)).returncode == 0, "append file")
    require(path.read_bytes() == payload * 2, "file append bytes")
    require(tmux("load-buffer", "-", "extra", data=b"invalid").returncode != 0, "public arity")
    require(tmux("loadb", "-bnamed", "--", "-", data=b"tail\n").returncode == 0, "alias compact options")
    require(tmux("saveb", "-bnamed", "-").stdout == b"tail\n", "alias newline bytes")
    control = tmux("-C", "attach-session", "-t", "w",
                   data=b"load-buffer -b named - extra\n\n")
    require(b"%error " in control.stdout, "Control rejects a forged stdin tail")
    require(tmux("save-buffer", "-b", "named", "-").stdout == b"tail\n", "Control preserves buffer")
    result = "clean:11"
except Exception as error:
    print(repr(error), flush=True)
finally:
    tmux("set-environment", "-g", "ZZ_BUFFER_STREAMS", result)
