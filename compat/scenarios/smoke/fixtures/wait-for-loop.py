import select
import subprocess
import sys
import time

prefix = sys.argv[1:]
children = []


def cli(*args):
    result = subprocess.run(prefix + list(args), text=True, capture_output=True, timeout=5)
    if result.returncode:
        raise RuntimeError(result.stderr.strip())
    return result.stdout.strip()


def start(flag, channel, label):
    args = ["display-message", "-p", "ready", ";", "wait-for"]
    if flag:
        args.append(flag)
    args += [channel, ";", "display-message", "-p", label]
    child = subprocess.Popen(prefix + args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    children.append(child)
    if not select.select([child.stdout], [], [], 5)[0] or child.stdout.readline().strip() != "ready":
        raise RuntimeError("waiter did not start")
    time.sleep(0.03)
    if child.poll() is not None:
        raise RuntimeError("waiter returned before its release")
    return child


def finish(child, label):
    output, error = child.communicate(timeout=5)
    if child.returncode or output.strip() != label or error:
        raise RuntimeError("unexpected waiter completion: " + repr((output, error, child.returncode)))


try:
    first = start(None, "loop-signal", "first")
    second = start(None, "loop-signal", "second")
    if cli("display-message", "-p", "unrelated") != "unrelated":
        raise RuntimeError("another queue could not run")
    cli("wait-for", "-S", "loop-signal")
    finish(first, "first")
    finish(second, "second")
    print("signal blocked=2 completed=2 unrelated=ok")

    cli("wait-for", "-S", "loop-sticky")
    cli("wait-for", "loop-sticky")
    print("sticky consumed-after-signaler-exit")

    cli("wait-for", "-L", "loop-lock")
    lockers = [start("-L", "loop-lock", str(index)) for index in range(3)]
    for index, child in enumerate(lockers):
        if any(waiter.poll() is not None for waiter in lockers[index:]):
            raise RuntimeError("a locker returned before its unlock")
        cli("wait-for", "-U", "loop-lock")
        finish(child, str(index))
    cli("wait-for", "-U", "loop-lock")
    cli("wait-for", "-L", "loop-lock")
    cli("wait-for", "-U", "loop-lock")
    print("lock fifo=0,1,2 unlock-order=ok reacquired=ok")
finally:
    for child in children:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=5)
