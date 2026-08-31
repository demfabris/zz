import fcntl
import os
import pty
import struct
import sys
import termios

pid, master = pty.fork()
if pid == 0:
    os.execvp(sys.argv[1], sys.argv[1:])
fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
while True:
    try:
        if not os.read(master, 4096):
            break
    except OSError:
        break
os.waitpid(pid, 0)
