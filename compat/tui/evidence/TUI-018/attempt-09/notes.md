# Fourth pass matrix declaration

Rebased campaign/tui-stream-alias-3 onto origin/main f9c52359e3da88cbde735076b38e22cdf3e003c6. The CLI changes now live in zz-cli. The pre-fix rebased tip is 1b725c50. Built zz-cli through /tmp/zz-cargo.sh and preserved target/debug/zz-alias4-baseline before production edits.

The fixture declares 51 matrix cells before fixes: 50 assertions and one registration of the existing 1 MiB consumed-input decision. Each asserted row compares stdout, stderr, process status and resulting option state, with a named one-channel sabotage. The initial run failed 27 assertions. Its attached control capture initially redirected stdout, but tmux -CC writes command frames to its stdin PTY. baseline-attached-pty.txt uses outer tmux pipe-pane to capture that PTY and isolates the real regressions. Earlier failed capture runs remain here.

Closed descriptor rows exposed the pinned libevent diagnostic `[err] evsig_cb: recv: Bad file descriptor`, distinct from an empty pipe. The matrix keeps these measurements visible.
