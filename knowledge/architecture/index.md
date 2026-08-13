# Architecture

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [End-to-end data flow](data-flow.md) - How terminal frames, browser pixels, ACP updates, and user input move among the daemon, GUI, PTY workers, CEF, and agents.
* [zz system overview](overview.md) - zz is a cross-platform GPUI workspace that multiplexes native terminal, Chromium browser, and Agent panes over a persistent daemon that several of the user's devices attach to at once.
* [Process & threading model](process-model.md) - How zz splits work across the persistent daemon, GUI and CLI clients, CEF subprocesses, ACP agents, and per-PTY worker threads.
<!-- okf:listing:end -->
