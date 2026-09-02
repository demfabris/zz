"""Attach a client on a real pty that answers a Kitty graphics probe.

Same step protocol as ``chooser-drive.py`` - ``size COLS ROWS``, ``keys
HEXBYTES``, ``snap NAME``, ``quit`` - with one addition: the drain thread
answers the terminal queries the client writes at startup the way a Kitty
terminal would, so the client's graphics bridge enables itself.

The raw TUI writes ``ESC _ G i=<id>,...,a=q,...;<payload> ESC \\`` for the
direct probe and again for the file probe, then a DA1 request and a cell-size
request. A Kitty terminal answers a graphics query with ``ESC _ G i=<id>;OK
ESC \\`` when it can take the image and ``ESC _ G i=<id>;E<message> ESC \\``
when it cannot. Answering OK to the direct probe and an error to the file probe
enables graphics with the inline transport, which is the shape the placement
assertions want.
"""

import fcntl
import os
import pty
import re
import struct
import sys
import termios
import threading
import time

STEP_TIMEOUT = 120.0
DIRECT_PROBE = 4294967295
FILE_PROBE = 4294967294

GRAPHICS_QUERY = re.compile(rb"\x1b_G([^;\x1b]*);?([^\x1b]*)\x1b\\")


def main():
    step_dir = sys.argv[1]
    snap_dir = sys.argv[2]
    columns = int(sys.argv[3])
    rows = int(sys.argv[4])
    command = sys.argv[5:]

    pid, master = pty.fork()
    if pid == 0:
        os.execvp(command[0], command)
    resize(master, columns, rows)

    drawn = bytearray()
    lock = threading.Lock()
    threading.Thread(target=drain, args=(master, drawn, lock), daemon=True).start()

    step = 0
    while True:
        step += 1
        path = os.path.join(step_dir, "step-%d" % step)
        if not wait_for(path):
            break
        with open(path, "r") as handle:
            line = handle.readline().strip()
        action, _, rest = line.partition(" ")
        if action == "size":
            width, height = rest.split()
            resize(master, int(width), int(height))
        elif action == "keys":
            os.write(master, bytes.fromhex(rest))
        elif action == "snap":
            with lock:
                payload = bytes(drawn)
                del drawn[:]
            with open(os.path.join(snap_dir, rest), "wb") as handle:
                handle.write(payload)
        elif action == "quit":
            acknowledge(step_dir, step)
            break
        acknowledge(step_dir, step)

    try:
        os.close(master)
    except OSError:
        pass
    os.waitpid(pid, 0)


def resize(master, columns, rows):
    fcntl.ioctl(
        master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0)
    )


def answer(master, pending):
    for match in GRAPHICS_QUERY.finditer(pending):
        keys = dict(
            field.split(b"=", 1)
            for field in match.group(1).split(b",")
            if b"=" in field
        )
        if keys.get(b"a") != b"q":
            continue
        try:
            image_id = int(keys.get(b"i", b"0"))
        except ValueError:
            continue
        if image_id == FILE_PROBE:
            reply = b"\x1b_Gi=%d;ENOENT:no file transport\x1b\\" % image_id
        else:
            reply = b"\x1b_Gi=%d;OK\x1b\\" % image_id
        os.write(master, reply)
    if b"\x1b[c" in pending:
        os.write(master, b"\x1b[?62;22c")
    if b"\x1b[16t" in pending:
        os.write(master, b"\x1b[6;18;8t")


def drain(master, drawn, lock):
    pending = bytearray()
    while True:
        try:
            chunk = os.read(master, 4096)
            if not chunk:
                break
        except OSError:
            break
        with lock:
            drawn += chunk
        pending += chunk
        answer(master, bytes(pending))
        del pending[:]


def wait_for(path):
    deadline = time.time() + STEP_TIMEOUT
    while time.time() < deadline:
        if os.path.exists(path):
            return True
        time.sleep(0.02)
    return False


def acknowledge(step_dir, step):
    path = os.path.join(step_dir, "ack-%d" % step)
    with open(path, "w") as handle:
        handle.write("ok\n")


main()
