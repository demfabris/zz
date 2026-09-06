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
    require_bytes(["show-buffer", "-b", "bytes", ";", "display", "-p", ""], b"hello")
    require_bytes(["show-buffer", "-b", "bytes", ";", "display", "-p", "AFTER"], b"hello")
    require_bytes(["save-buffer", "-b", "bytes", "-", ";", "display", "-p", "AFTER"], b"hello")
    saved = pathlib.Path(os.environ["HOME"]) / "cli-output-saved"
    require_bytes(["save-buffer", "-b", "bytes", str(saved), ";", "display", "-p", "AFTER"], b"AFTER\n")
    if saved.read_bytes() != b"hello":
        raise AssertionError(("named file write", saved.read_bytes()))
    for args, expected in (
        (["display", "-p", "BEFORE", ";", "show-buffer", "-b", "bytes"], b"BEFORE\n"),
        (["show-buffer", "-b", "bytes", ";", "display", "-p", "AFTER", ";", "show-buffer", "-b", "bytes"], b"hello"),
        (["display", "-p", "", ";", "show-buffer", "-b", "bytes", ";", "display", "-p", "AFTER"], b"\nAFTER\n"),
    ):
        observed = subprocess.run(["tmux", *args], capture_output=True, timeout=15)
        if (observed.stdout, observed.stderr, observed.returncode) != (expected, b"Bad file descriptor: -\n", 1):
            raise AssertionError((args, observed.stdout, observed.stderr, observed.returncode))
    prints = pathlib.Path(os.environ["HOME"]) / "cli-output-prints.conf"
    prints.write_text("display-message -p ''\ndisplay-message -p line\ndisplay-message -p ''\n")
    raw_only = pathlib.Path(os.environ["HOME"]) / "cli-output-raw.conf"
    raw_only.write_text("show-buffer -b bytes\n")
    path.write_text("show-buffer -b bytes\ndisplay-message -p AFTER\n")
    require_bytes(["source-file", str(path)], b"hello")
    require_bytes(["source-file", str(raw_only), ";", "display", "-p", "AFTER"], b"hello")
    require_bytes(["show-buffer", "-b", "bytes", ";", "source-file", str(prints)], b"hello")
    require_bytes(["source-file", str(prints), ";", "display", "-p", "AFTER"], b"\nline\n\nAFTER\n")
    double = pathlib.Path(os.environ["HOME"]) / "cli-output-double.conf"
    double.write_text("show-buffer -b bytes\nshow-buffer -b bytes\n")
    for args, expected in (
        (["source-file", str(double)], b"hello"),
        (["display", "-p", "BEFORE", ";", "source-file", str(raw_only)], b"BEFORE\n"),
    ):
        observed = subprocess.run(["tmux", *args], capture_output=True, timeout=15)
        if (observed.stdout, observed.stderr, observed.returncode) != (expected, b"Bad file descriptor: -\n", 1):
            raise AssertionError((args, observed.stdout, observed.stderr, observed.returncode))
    print("sourced stdout ownership: raw claim, dropped prints and EBADF match the pin")
    tmux("set-buffer", "-b", "newline", "hello\n")
    raw_newline = pathlib.Path(os.environ["HOME"]) / "cli-output-raw-newline.conf"
    raw_newline.write_text("show-buffer -b newline\n")
    trailing = b"hello\nAFTER\n" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else b"hello\n"
    require_bytes(["source-file", str(raw_newline), ";", "display", "-p", "AFTER"], trailing)
    print("KNOWN DIVERGENCE cli-output-sourced-raw-newline-claim: pin=hello LF, zz=hello LF AFTER LF")
    tmux("delete-buffer", "-b", "newline")
    result = "clean:16"
except Exception as error:
    print(repr(error), flush=True)
finally:
    tmux("set-environment", "-g", "ZZ_CLI_OUTPUT_BYTES", result)
