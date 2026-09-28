import os
import select
import sys
import time
import tty

TAG = "§".encode()
hz = float(sys.argv[1])
tty.setraw(0)
period = 1.0 / hz if hz > 0 else None
due = time.monotonic() + (period or 0)
ticks = 0
keys = 0
while True:
    timeout = max(0.0, due - time.monotonic()) if period else None
    ready, _, _ = select.select([0], [], [], timeout)
    if ready:
        data = os.read(0, 256)
        if not data:
            break
        out = []
        for byte in data:
            if byte == 13:
                out.append(b"\r\n")
            else:
                keys += 1
                out.append(TAG + b"%03d" % (keys % 1000))
        os.write(1, b"".join(out))
    if period and time.monotonic() >= due:
        ticks += 1
        os.write(1, b"\r\n" + b"." * 40 + b" %08d" % ticks)
        due += period
