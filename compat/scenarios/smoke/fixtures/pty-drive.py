"""Attach a client on a real pty and drive it from step files.

The overlay scenarios need an attached client they can resize and type into
while a differential fixture runs CLI commands beside it. The shell writes
``STEPDIR/step-N`` and waits for ``STEPDIR/ack-N``, so every step is ordered
against the commands around it without sleeping on guesses.

Steps are one line: ``size COLS ROWS``, ``keys HEXBYTES``, or ``quit``.
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
    columns = int(sys.argv[2])
    rows = int(sys.argv[3])
    command = sys.argv[4:]

    pid, master = pty.fork()
    if pid == 0:
        os.execvp(command[0], command)
    resize(master, columns, rows)

    threading.Thread(target=drain, args=(master,), daemon=True).start()

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


def drain(master):
    while True:
        try:
            if not os.read(master, 4096):
                break
        except OSError:
            break


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
