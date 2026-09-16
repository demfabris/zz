#!/usr/bin/env python3
import os
import signal
import sys

signal.signal(signal.SIGTTOU, signal.SIG_IGN)
parent_group = os.getpgrp()
ready_read, ready_write = os.pipe()
pid = os.fork()
if pid == 0:
    os.close(ready_write)
    os.setpgid(0, 0)
    os.read(ready_read, 1)
    os.close(ready_read)
    signal.signal(signal.SIGTTOU, signal.SIG_DFL)
    os.execvp(sys.argv[1], sys.argv[1:])
os.close(ready_read)
os.setpgid(pid, pid)
os.tcsetpgrp(0, pid)
os.write(ready_write, b'1')
os.close(ready_write)
try:
    _, status = os.waitpid(pid, 0)
finally:
    os.tcsetpgrp(0, parent_group)
code = os.waitstatus_to_exitcode(status)
sys.exit(code if code >= 0 else 128 - code)
