import sys
import time

rate = float(sys.argv[1])
line = "line " + "x" * 70 + "\n"
period = 1.0 / rate
due = time.monotonic()
while True:
    sys.stdout.write(line)
    sys.stdout.flush()
    due += period
    delay = due - time.monotonic()
    if delay > 0:
        time.sleep(delay)
    else:
        due = time.monotonic()
