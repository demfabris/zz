import os
import pathlib
import sys
import tty

tty.setraw(0)
sink = pathlib.Path(sys.argv[1])
sink.write_text("")
pathlib.Path(sys.argv[1] + ".ready").write_text("1")
while True:
    chunk = os.read(0, 64)
    if not chunk:
        break
    with sink.open("a") as handle:
        handle.write(chunk.hex())
