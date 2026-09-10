"""A pty in the middle of an attached client that can stop reading on command.

compat/tui-output-backpressure.sh needs a terminal that stops draining while
the client keeps painting, and an outer pinned tmux always drains its panes.
This relay sits between the two: it runs inside an outer tmux pane, gives the
client a pty of its own at the pane's size, and copies master to stdout and
stdin to master. SIGUSR1 stops the copy in the master direction, SIGUSR2
resumes it. Keys keep flowing while the copy is stopped, which is the whole
point: the client's terminal is unreadable and its input is not.

The status file is the fixture's observable. Every poll it holds one line,
`hold=<0|1> avail=<bytes> forwarded=<bytes> restores=<n> tail=<bytes>
exited=<0|1>`, where avail is FIONREAD on the master: the bytes the kernel is
holding for a reader that is not reading. A bounded wait on avail going flat is
how the fixture knows the pty buffer is full and the client is really backed
up, instead of sleeping. restores counts the times the client left the
alternate screen, tail is how many bytes it wrote after the last one and
escapes is how many of those bytes were ESC. A detach message after the
restore is a line of text; a paint after the restore carries escapes.

After the client exits the relay takes the terminal over: it restores the
cooked modes, clears the screen and prints DONE_MARKER, so whatever the outer
pane decodes from then on arrived after the client was gone. It then stays
alive so the pane it runs in is never a dead pane with tmux's own text in it.
--late-paint injects a paint twice, once accounted into the tail as though the
client had written it after handing the terminal back and once after the
marker: one sabotage for each of the two assertions the teardown case makes.
"""

import errno
import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import tty

POLL = 0.05
CHUNK = 1 << 16
DONE_MARKER = "RELAY-DONE"
RESTORE = b"\x1b[?1049l"
ESCAPE = b"\x1b"
LATE_PAINT = b"\x1b[5;1H\x1b[41mRELAY-LATE-PAINT\x1b[0m"
TAIL_SAMPLE = 512

hold = False
resized = False


def on_hold(_signum, _frame):
    global hold
    hold = True


def on_resume(_signum, _frame):
    global hold
    hold = False


def on_winch(_signum, _frame):
    global resized
    resized = True


def window_size(fd):
    try:
        packed = fcntl.ioctl(fd, termios.TIOCGWINSZ, struct.pack("HHHH", 0, 0, 0, 0))
    except OSError:
        return (24, 80, 0, 0)
    return struct.unpack("HHHH", packed)


def pending(fd):
    try:
        return struct.unpack("i", fcntl.ioctl(fd, termios.FIONREAD, b"\0\0\0\0"))[0]
    except OSError:
        return -1


def write_status(path, forwarded, master, exited, restores=0, tail=0, escapes=0):
    line = "hold=%d avail=%d forwarded=%d restores=%d tail=%d escapes=%d exited=%d\n" % (
        1 if hold else 0,
        pending(master) if master is not None else -1,
        forwarded,
        restores,
        tail,
        escapes,
        1 if exited else 0,
    )
    temporary = path + ".tmp"
    with open(temporary, "w") as handle:
        handle.write(line)
    os.replace(temporary, path)


def main(argv):
    global resized
    late_paint = False
    if argv and argv[0] == "--late-paint":
        late_paint = True
        argv = argv[1:]
    if len(argv) < 3:
        sys.stderr.write("usage: tui-output-relay.py [--late-paint] STATUS PIDFILE -- CMD...\n")
        return 2
    status_path, pid_path = argv[0], argv[1]
    argv = argv[2:]
    if argv and argv[0] == "--":
        argv = argv[1:]

    signal.signal(signal.SIGUSR1, on_hold)
    signal.signal(signal.SIGUSR2, on_resume)
    signal.signal(signal.SIGWINCH, on_winch)

    # The pane the relay runs in hands it a cooked terminal: without this the
    # keys the fixture sends would sit in the line discipline until a newline
    # and be echoed back over the client's screen.
    outer = termios.tcgetattr(0)
    tty.setraw(0)

    rows, columns, xpixel, ypixel = window_size(1)
    child, master = pty.fork()
    if child == 0:
        os.execvp(argv[0], argv)
        os._exit(127)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, xpixel, ypixel))
    with open(pid_path, "w") as handle:
        handle.write("%d\n" % child)

    forwarded = 0
    restores = 0
    tail = 0
    escapes = 0
    sample = b""
    exited = False
    write_status(status_path, forwarded, master, exited, restores, tail, escapes)
    while True:
        if resized:
            resized = False
            fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", *window_size(1)))
        watch = [0] if hold else [0, master]
        try:
            ready, _, _ = select.select(watch, [], [], POLL)
        except InterruptedError:
            ready = []
        if 0 in ready:
            keys = os.read(0, CHUNK)
            if keys:
                os.write(master, keys)
        if master in ready:
            try:
                chunk = os.read(master, CHUNK)
            except OSError as error:
                if error.errno == errno.EIO:
                    chunk = b""
                else:
                    raise
            if not chunk:
                exited = True
                break
            os.write(1, chunk)
            forwarded += len(chunk)
            position = chunk.rfind(RESTORE)
            if position < 0:
                tail += len(chunk)
                escapes += chunk.count(ESCAPE)
                sample = (sample + chunk)[-TAIL_SAMPLE:]
            else:
                restores += chunk.count(RESTORE)
                after = chunk[position + len(RESTORE):]
                tail = len(after)
                escapes = after.count(ESCAPE)
                sample = after[:TAIL_SAMPLE]
        write_status(status_path, forwarded, master, exited, restores, tail, escapes)

    os.close(master)
    try:
        os.waitpid(child, 0)
    except ChildProcessError:
        pass
    if late_paint:
        os.write(1, LATE_PAINT)
        forwarded += len(LATE_PAINT)
        tail += len(LATE_PAINT)
        escapes += LATE_PAINT.count(ESCAPE)
        sample = (sample + LATE_PAINT)[-TAIL_SAMPLE:]
    with open(status_path + ".tail", "w") as handle:
        handle.write(repr(sample) + "\n")
    write_status(status_path, forwarded, None, True, restores, tail, escapes)
    termios.tcsetattr(0, termios.TCSADRAIN, outer)
    os.write(1, ("\x1b[2J\x1b[3J\x1b[H%s\r\n" % DONE_MARKER).encode())
    if late_paint:
        os.write(1, LATE_PAINT)
    while True:
        try:
            ready, _, _ = select.select([0], [], [], POLL)
        except InterruptedError:
            ready = []
        if 0 in ready and not os.read(0, CHUNK):
            break
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
