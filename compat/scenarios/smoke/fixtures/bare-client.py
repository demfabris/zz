"""Run one client on a real pty and report what it wrote and how it exited.

``server_client_default_command`` only runs for a client that reaches the
server with an empty command vector, and such a client is a terminal client, so
the probe has to give it a pty rather than a pipe. Everything the client writes
before it exits lands in ``OUTFILE``; the exit status goes to stdout as
``exit=N``.

    bare-client.py OUTFILE SECONDS COMMAND [ARG...]
"""

import fcntl
import os
import pty
import select
import struct
import sys
import termios
import time

COLUMNS = 80
ROWS = 24


def main():
    sys.stdout.reconfigure(line_buffering=True)
    out_path = sys.argv[1]
    deadline = time.time() + float(sys.argv[2])
    command = sys.argv[3:]

    pid, master = pty.fork()
    if pid == 0:
        os.execvp(command[0], command)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLUMNS, 0, 0))

    drawn = bytearray()
    while time.time() < deadline:
        readable, _, _ = select.select([master], [], [], 0.2)
        if not readable:
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            break
        if not chunk:
            break
        drawn += chunk
    try:
        os.close(master)
    except OSError:
        pass

    with open(out_path, "wb") as handle:
        handle.write(bytes(drawn))

    status = reap(pid, deadline)
    print("exit=%s" % status)


def reap(pid, deadline):
    while True:
        reaped, status = os.waitpid(pid, os.WNOHANG)
        if reaped == pid:
            if os.WIFEXITED(status):
                return os.WEXITSTATUS(status)
            return "signal"
        if time.time() >= deadline:
            return "stalled"
        time.sleep(0.05)


main()
