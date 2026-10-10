"""Answer the terminal queries a tmux 3.8 client holds input for.

tmux 3.8 asks a client whose pty reports no pixel size for it with CSI 14 t
(tty.c tty_resize) and, while that query is outstanding, holds every lone
Escape for 500 ms (tty-keys.c tty_keys_next), so an Escape and the key typed
after it merge into a meta key. Every pty driver feeds the bytes it reads from
its client through one ``Answerer``, which replies the way a terminal with no
pixel size would. A query split across two reads is still answered once.
"""

import os

WINDOW_PIXELS_QUERY = b"\x1b[14t"
WINDOW_PIXELS_ANSWER = b"\x1b[4;0;0t"


class Answerer:
    def __init__(self, master):
        self.master = master
        self.tail = b""

    def feed(self, data):
        seen = self.tail + data
        for _ in range(seen.count(WINDOW_PIXELS_QUERY)):
            os.write(self.master, WINDOW_PIXELS_ANSWER)
        self.tail = seen[-(len(WINDOW_PIXELS_QUERY) - 1):]
