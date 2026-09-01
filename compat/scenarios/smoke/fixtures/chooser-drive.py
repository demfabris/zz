"""Attach a client on a real pty, drive it, and snapshot what it drew.

Same step protocol as ``pty-drive.py`` with one addition: ``snap NAME`` writes
everything the client has written since the previous snapshot to
``SNAPDIR/NAME``. The chooser row-format probes need the bytes an attached
client drew, because ``capture-pane`` reads the pane's own screen and never the
chooser the client is showing over it.

Steps are one line: ``size COLS ROWS``, ``keys HEXBYTES``, ``snap NAME``, or
``quit``.
"""

import fcntl
import os
import pty
import struct
import sys
import termios
import threading
import time

STEP_TIMEOUT = 120.0


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


def drain(master, drawn, lock):
    while True:
        try:
            chunk = os.read(master, 4096)
            if not chunk:
                break
        except OSError:
            break
        with lock:
            drawn += chunk


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
