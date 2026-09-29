import os
import sys
import time

source, out = sys.argv[1], sys.argv[2]
start = time.perf_counter()
pid = os.posix_spawn("/bin/cat", ["/bin/cat", source], os.environ)
os.waitpid(pid, 0)
elapsed = time.perf_counter() - start
with open(out + ".tmp", "w") as f:
    f.write(f"{elapsed:.6f}\n")
os.replace(out + ".tmp", out)
if len(sys.argv) > 3:
    sys.stdout.write(sys.argv[3] + "\n")
    sys.stdout.flush()
time.sleep(1e6)
