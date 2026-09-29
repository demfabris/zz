import fcntl
import os
import pty
import select
import signal
import struct
import termios
import time


class PtyClient:
    def __init__(self, argv, env, envobj, cols=180, rows=50):
        self.envobj = envobj
        self.t0 = time.perf_counter()
        pid, fd = pty.fork()
        if pid == 0:
            try:
                os.execve(argv[0], argv, env)
            finally:
                os._exit(127)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.pid = pid
        self.fd = fd
        self.buf = bytearray()
        self.total = 0
        self.first_byte = None
        self.closed = False
        envobj.clients.add(pid)

    def _read_once(self, timeout):
        if self.closed:
            return b""
        ready, _, _ = select.select([self.fd], [], [], max(timeout, 0))
        if not ready:
            return None
        try:
            data = os.read(self.fd, 1 << 16)
        except OSError:
            data = b""
        if not data:
            self.closed = True
            return b""
        if self.first_byte is None:
            self.first_byte = time.perf_counter()
        self.total += len(data)
        return data

    def read_until(self, predicate, timeout, keep=True):
        deadline = time.perf_counter() + timeout
        while True:
            if predicate(self.buf):
                return time.perf_counter()
            remaining = deadline - time.perf_counter()
            if remaining <= 0 or self.closed:
                return None
            data = self._read_once(min(remaining, 0.05))
            if data and keep:
                self.buf.extend(data)

    def drain(self, seconds):
        deadline = time.perf_counter() + seconds
        got = 0
        while True:
            remaining = deadline - time.perf_counter()
            if remaining <= 0 or self.closed:
                return got
            data = self._read_once(min(remaining, 0.05))
            if data:
                got += len(data)

    def drain_quiet(self, quiet, cap):
        deadline = time.perf_counter() + cap
        last = time.perf_counter()
        while not self.closed:
            now = time.perf_counter()
            if now - last >= quiet or now >= deadline:
                return now - last >= quiet
            data = self._read_once(min(quiet - (now - last), deadline - now))
            if data:
                last = time.perf_counter()
        return False

    def write(self, data):
        os.write(self.fd, data)

    def detach(self, keys, timeout=3.0):
        try:
            self.write(keys)
        except OSError:
            pass
        deadline = time.perf_counter() + timeout
        while time.perf_counter() < deadline:
            pid, _ = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self._closed_child()
                return True
            self._read_once(0.02)
        self.kill()
        return False

    def kill(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
        except (ProcessLookupError, ChildProcessError):
            pass
        self._closed_child()

    def _closed_child(self):
        self.envobj.clients.discard(self.pid)
        try:
            os.close(self.fd)
        except OSError:
            pass
        self.closed = True
