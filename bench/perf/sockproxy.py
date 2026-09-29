import os
import socket
import struct
import threading


class SockProxy:
    def __init__(self, listen, upstream):
        self.listen = listen
        self.upstream = upstream
        self.lock = threading.Lock()
        self.connections = 0
        self.c2s = 0
        self.s2c = 0
        self.s2c_frames = 0
        self._sockets = []
        try:
            os.unlink(listen)
        except FileNotFoundError:
            pass
        self.server = socket.socket(socket.AF_UNIX)
        self.server.bind(listen)
        self.server.listen(64)
        threading.Thread(target=self._accept, daemon=True).start()

    def counters(self):
        with self.lock:
            return {"connections": self.connections, "c2s": self.c2s, "s2c": self.s2c, "s2c_frames": self.s2c_frames}

    def _accept(self):
        while True:
            try:
                client, _ = self.server.accept()
            except OSError:
                return
            upstream = socket.socket(socket.AF_UNIX)
            try:
                upstream.connect(self.upstream)
            except OSError:
                client.close()
                continue
            with self.lock:
                self.connections += 1
                self._sockets += [client, upstream]
            threading.Thread(target=self._pump, args=(client, upstream, False), daemon=True).start()
            threading.Thread(target=self._pump, args=(upstream, client, True), daemon=True).start()

    def _pump(self, src, dst, downstream):
        pending = bytearray()
        while True:
            try:
                data = src.recv(1 << 20)
            except OSError:
                data = b""
            if not data:
                break
            try:
                dst.sendall(data)
            except OSError:
                break
            frames = 0
            if downstream:
                pending += data
                at = 0
                while len(pending) - at >= 4:
                    (length,) = struct.unpack_from("<I", pending, at)
                    if len(pending) - at < 4 + length:
                        break
                    at += 4 + length
                    frames += 1
                del pending[:at]
            with self.lock:
                if downstream:
                    self.s2c += len(data)
                    self.s2c_frames += frames
                else:
                    self.c2s += len(data)
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass

    def close(self):
        try:
            self.server.close()
        except OSError:
            pass
        with self.lock:
            sockets = list(self._sockets)
        for s in sockets:
            try:
                s.close()
            except OSError:
                pass
        try:
            os.unlink(self.listen)
        except OSError:
            pass
