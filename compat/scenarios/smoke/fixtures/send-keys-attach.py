"""Attach one client on a real pty and keep every byte it is sent.

``record OUT COLS ROWS COMMAND...`` runs COMMAND on a pty of the given size and
appends the multiplexer's writes to OUT until the process is killed. The
send-keys fixtures need a genuine attached client because the target-client
selector, the read-only guard, and copy mode all read client state that a
detached command client does not have.
"""

import fcntl
import os
import pty
import struct
import sys
import termios


def record(path, columns, rows, command):
    pid, master = pty.fork()
    if pid == 0:
        os.execvp(command[0], command)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    with open(path, "ab", buffering=0) as handle:
        while True:
            try:
                chunk = os.read(master, 4096)
            except OSError:
                break
            if not chunk:
                break
            handle.write(chunk)
    os.waitpid(pid, 0)


def main():
    if len(sys.argv) < 6 or sys.argv[1] != "record":
        sys.exit(2)
    record(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), sys.argv[5:])


if __name__ == "__main__":
    main()
