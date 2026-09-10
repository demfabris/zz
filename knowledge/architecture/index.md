# Architecture

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Agent peer bus](agent-peer-bus.md) - How the daemon exchanges prompts between Agent panes and local Claude Code sessions through Claude Code's peer registry and Unix inbox sockets.
* [End-to-end data flow](data-flow.md) - How terminal frames, browser pixels, ACP updates, and user input move among the daemon, GUI, PTY workers, CEF, and agents.
* [zz system overview](overview.md) - zz multiplexes terminal, Chromium browser, and Agent panes over a persistent daemon shared by desktop, native Apple, and terminal clients.
* [Process & threading model](process-model.md) - How zz splits work across the persistent daemon, GUI and CLI clients, CEF subprocesses, ACP agents, and per-PTY worker threads.
<!-- okf:listing:end -->
